//! Proxy request handler with retry logic
//!
//! Handles incoming HTTP/HTTPS requests and forwards them through upstream proxies.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::PROXY_AUTHORIZATION;
use hyper::upgrade::OnUpgrade;
use hyper::{Method, Request, Response, StatusCode};
use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{debug, error, info, instrument, warn};

use crate::config::EgressProxyConfig;
use crate::error::{Result, RotaError};
use crate::models::{Proxy, RequestRecord};
use crate::proxy::egress;
use crate::proxy::rotation::ProxySelector;
use crate::proxy::transport::ProxyTransport;
use crate::proxy::tunnel::{TunnelGuard, TunnelHandler};
use crate::repository::{LogRepository, ProxyRepository};

/// Configuration for proxy handler
#[derive(Clone)]
pub struct ProxyHandlerConfig {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Timeout for upstream proxy connections
    pub connect_timeout: Duration,
    /// Timeout for request/response
    pub request_timeout: Duration,
    /// Whether to log requests
    pub enable_logging: bool,
}

impl Default for ProxyHandlerConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30),
            enable_logging: true,
        }
    }
}

/// Proxy request handler
pub struct ProxyHandler {
    selector: Arc<dyn ProxySelector>,
    config: ProxyHandlerConfig,
    log_sender: Option<broadcast::Sender<RequestRecord>>,
    db_pool: PgPool,
    egress_proxy: Option<EgressProxyConfig>,
}

impl ProxyHandler {
    pub fn new(
        selector: Arc<dyn ProxySelector>,
        config: ProxyHandlerConfig,
        log_sender: Option<broadcast::Sender<RequestRecord>>,
        db_pool: PgPool,
        egress_proxy: Option<EgressProxyConfig>,
    ) -> Self {
        Self {
            selector,
            config,
            log_sender,
            db_pool,
            egress_proxy,
        }
    }

    /// Handle an incoming proxy request
    #[instrument(skip(self, req), fields(method = %req.method(), uri = %req.uri()))]
    pub async fn handle(
        &self,
        req: Request<Incoming>,
        client_ip: String,
    ) -> Result<Response<Full<Bytes>>> {
        let method = req.method().clone();

        // Handle CONNECT requests (HTTPS tunneling)
        if method == Method::CONNECT {
            return self.handle_connect(req, client_ip).await;
        }

        // Handle regular HTTP requests
        self.handle_http(req, client_ip).await
    }

    /// Handle HTTP CONNECT request (HTTPS tunneling)
    #[instrument(skip(self, req), fields(uri = %req.uri()))]
    async fn handle_connect(
        &self,
        req: Request<Incoming>,
        client_ip: String,
    ) -> Result<Response<Full<Bytes>>> {
        let uri = req.uri().clone();
        let authority = uri
            .authority()
            .map(|a| a.to_string())
            .unwrap_or_else(|| uri.to_string());

        let (target_host, target_port) = ProxyTransport::parse_authority(&authority)?;

        debug!(
            "CONNECT request to {}:{} from {}",
            target_host, target_port, client_ip
        );

        let method_str = "CONNECT".to_string();
        let requested_url = authority.clone();

        // Select a proxy with retry logic
        let mut attempts = 0;
        let max_attempts = self.config.max_retries + 1;
        let mut last_error = None;
        let mut selected: Option<(Arc<Proxy>, Box<dyn crate::proxy::transport::ProxyConnection>)> =
            None;

        while attempts < max_attempts {
            attempts += 1;

            let proxy = match self.selector.select().await {
                Ok(p) => p,
                Err(e) => {
                    error!("No proxy available: {}", e);
                    return Ok(self
                        .error_response(StatusCode::SERVICE_UNAVAILABLE, "No proxies available"));
                }
            };

            debug!(
                "Attempting CONNECT through proxy {} (attempt {}/{})",
                proxy.address, attempts, max_attempts
            );

            // Try to establish tunnel (don't respond 200 until this succeeds)
            let attempt_start = Instant::now();
            match tokio::time::timeout(
                self.config.connect_timeout,
                ProxyTransport::connect(
                    &proxy,
                    &target_host,
                    target_port,
                    self.egress_proxy.as_ref(),
                ),
            )
            .await
            {
                Ok(Ok(connection)) => {
                    let attempt_duration = attempt_start.elapsed();
                    let record = RequestRecord {
                        proxy_id: proxy.id,
                        proxy_address: proxy.address.clone(),
                        requested_url: requested_url.clone(),
                        method: method_str.clone(),
                        success: true,
                        response_time: attempt_duration.as_millis() as i32,
                        status_code: 200,
                        error_message: None,
                        timestamp: chrono::Utc::now(),
                    };
                    self.broadcast_request_record(&record);
                    self.persist_request_record(record);

                    selected = Some((proxy.clone(), connection));

                    // Return 200 Connection Established. The actual tunneling is handled after
                    // the client upgrades the connection.
                    info!(
                        "CONNECT tunnel established through {} to {}:{}",
                        proxy.address, target_host, target_port
                    );

                    break;
                }
                Ok(Err(e)) => {
                    let attempt_duration = attempt_start.elapsed();
                    let record = RequestRecord {
                        proxy_id: proxy.id,
                        proxy_address: proxy.address.clone(),
                        requested_url: requested_url.clone(),
                        method: method_str.clone(),
                        success: false,
                        response_time: attempt_duration.as_millis() as i32,
                        status_code: 502,
                        error_message: Some(e.to_string()),
                        timestamp: chrono::Utc::now(),
                    };
                    self.broadcast_request_record(&record);
                    self.persist_request_record(record);

                    warn!(
                        "CONNECT through {} failed: {} (attempt {}/{})",
                        proxy.address, e, attempts, max_attempts
                    );
                    last_error = Some(e);
                }
                Err(_) => {
                    let attempt_duration = attempt_start.elapsed();
                    let record = RequestRecord {
                        proxy_id: proxy.id,
                        proxy_address: proxy.address.clone(),
                        requested_url: requested_url.clone(),
                        method: method_str.clone(),
                        success: false,
                        response_time: attempt_duration.as_millis() as i32,
                        status_code: 502,
                        error_message: Some(RotaError::Timeout.to_string()),
                        timestamp: chrono::Utc::now(),
                    };
                    self.broadcast_request_record(&record);
                    self.persist_request_record(record);

                    warn!(
                        "CONNECT through {} timed out (attempt {}/{})",
                        proxy.address, attempts, max_attempts
                    );
                    last_error = Some(RotaError::Timeout);
                }
            }
        }

        let Some((proxy, connection)) = selected else {
            error!(
                "All CONNECT attempts failed after {} attempts",
                max_attempts
            );
            return Ok(self.error_response(
                StatusCode::BAD_GATEWAY,
                &format!(
                    "Failed to establish tunnel: {}",
                    last_error.unwrap_or(RotaError::NoProxiesAvailable)
                ),
            ));
        };

        let on_upgrade: OnUpgrade = hyper::upgrade::on(req);
        let _guard = TunnelGuard::new(proxy.id as i64, self.selector.clone());

        tokio::spawn(async move {
            let _guard = _guard;
            match on_upgrade.await {
                Ok(upgraded) => {
                    let client = hyper_util::rt::TokioIo::new(upgraded);
                    let _ = TunnelHandler::copy_bidirectional(client, connection).await;
                }
                Err(e) => {
                    debug!("CONNECT upgrade failed: {}", e);
                }
            }
        });

        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(Full::new(Bytes::new()))
            .unwrap())

    }

    /// Handle regular HTTP request
    #[instrument(skip(self, req), fields(method = %req.method(), uri = %req.uri()))]
    async fn handle_http(
        &self,
        req: Request<Incoming>,
        client_ip: String,
    ) -> Result<Response<Full<Bytes>>> {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let start = Instant::now();
        let requested_url = uri.to_string();
        let method_str = method.as_str().to_string();

        // Parse target from URI
        let (target_host, target_port) = ProxyTransport::parse_target(&uri)?;

        // Collect request body
        let (parts, body) = req.into_parts();
        let body_bytes = body
            .collect()
            .await
            .map_err(|e| RotaError::InvalidRequest(format!("Failed to read body: {}", e)))?
            .to_bytes();

        // Retry loop
        let mut attempts = 0;
        let max_attempts = self.config.max_retries + 1;
        let mut last_error = None;

        while attempts < max_attempts {
            attempts += 1;

            let proxy = match self.selector.select().await {
                Ok(p) => p,
                Err(e) => {
                    error!("No proxy available: {}", e);
                    return Ok(self
                        .error_response(StatusCode::SERVICE_UNAVAILABLE, "No proxies available"));
                }
            };

            // Track connection
            let _guard = TunnelGuard::new(proxy.id as i64, self.selector.clone());

            debug!(
                "Forwarding HTTP request through proxy {} (attempt {}/{})",
                proxy.address, attempts, max_attempts
            );

            let attempt_start = Instant::now();
            match self
                .forward_request(
                    &proxy,
                    &parts,
                    body_bytes.clone(),
                    &target_host,
                    target_port,
                )
                .await
            {
                Ok(response) => {
                    let attempt_duration = attempt_start.elapsed();
                    let status_code = response.status().as_u16() as i32;
                    let success = true;

                    let record = RequestRecord {
                        proxy_id: proxy.id,
                        proxy_address: proxy.address.clone(),
                        requested_url: requested_url.clone(),
                        method: method_str.clone(),
                        success,
                        response_time: attempt_duration.as_millis() as i32,
                        status_code,
                        error_message: None,
                        timestamp: chrono::Utc::now(),
                    };
                    self.broadcast_request_record(&record);
                    self.persist_request_record(record);

                    return Ok(response);
                }
                Err(e) => {
                    let attempt_duration = attempt_start.elapsed();
                    let record = RequestRecord {
                        proxy_id: proxy.id,
                        proxy_address: proxy.address.clone(),
                        requested_url: requested_url.clone(),
                        method: method_str.clone(),
                        success: false,
                        response_time: attempt_duration.as_millis() as i32,
                        status_code: 502,
                        error_message: Some(e.to_string()),
                        timestamp: chrono::Utc::now(),
                    };
                    self.broadcast_request_record(&record);
                    self.persist_request_record(record);

                    warn!(
                        "Request through {} failed: {} (attempt {}/{})",
                        proxy.address, e, attempts, max_attempts
                    );
                    last_error = Some(e);
                }
            }
        }

        let duration = start.elapsed();
        error!("All HTTP attempts failed after {} attempts", max_attempts);

        // Record the overall failure (no specific proxy to attribute).
        let record = RequestRecord {
            proxy_id: 0,
            proxy_address: String::new(),
            requested_url: uri.to_string(),
            method: method_str.clone(),
            success: false,
            response_time: duration.as_millis() as i32,
            status_code: 502,
            error_message: last_error.as_ref().map(|e| e.to_string()),
            timestamp: chrono::Utc::now(),
        };
        self.broadcast_request_record(&record);
        self.persist_request_record(record);

        Ok(self.error_response(
            StatusCode::BAD_GATEWAY,
            &format!(
                "All proxies failed: {}",
                last_error.unwrap_or(RotaError::NoProxiesAvailable)
            ),
        ))
    }

    /// Forward HTTP request through proxy
    async fn forward_request(
        &self,
        proxy: &Proxy,
        parts: &http::request::Parts,
        body: Bytes,
        target_host: &str,
        target_port: u16,
    ) -> Result<Response<Full<Bytes>>> {
        // Build the full target URL
        let uri_str = if target_port == 80 {
            format!(
                "http://{}{}",
                target_host,
                parts
                    .uri
                    .path_and_query()
                    .map(|pq| pq.as_str())
                    .unwrap_or("/")
            )
        } else {
            format!(
                "http://{}:{}{}",
                target_host,
                target_port,
                parts
                    .uri
                    .path_and_query()
                    .map(|pq| pq.as_str())
                    .unwrap_or("/")
            )
        };

        // Connect to proxy (address format is "host:port")
        let stream = tokio::time::timeout(
            self.config.connect_timeout,
            egress::connect_to_addr(self.egress_proxy.as_ref(), &proxy.address),
        )
        .await
        .map_err(|_| RotaError::Timeout)?
        ?;

        // Build request
        let mut builder = Request::builder()
            .method(parts.method.clone())
            .uri(&uri_str);

        // Copy headers, except hop-by-hop headers
        for (name, value) in &parts.headers {
            if !is_hop_by_hop_header(name.as_str()) {
                builder = builder.header(name, value);
            }
        }

        // Add proxy authentication if needed
        if let (Some(username), Some(password)) = (&proxy.username, &proxy.password) {
            let credentials = format!("{}:{}", username, password);
            let encoded =
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, credentials);
            builder = builder.header(PROXY_AUTHORIZATION, format!("Basic {}", encoded));
        }

        let request = builder
            .body(Full::new(body))
            .map_err(|e| RotaError::InvalidRequest(format!("Failed to build request: {}", e)))?;

        // Send request using hyper
        let io = hyper_util::rt::TokioIo::new(stream);
        let (mut sender, conn) = hyper::client::conn::http1::handshake(io)
            .await
            .map_err(|e| RotaError::ProxyConnectionFailed(format!("Handshake failed: {}", e)))?;

        // Spawn connection handler
        tokio::spawn(async move {
            if let Err(e) = conn.await {
                debug!("Connection ended: {}", e);
            }
        });

        // Send request with timeout
        let response =
            tokio::time::timeout(self.config.request_timeout, sender.send_request(request))
                .await
                .map_err(|_| RotaError::Timeout)?
                .map_err(|e| RotaError::ProxyConnectionFailed(format!("Request failed: {}", e)))?;

        // Collect response body
        let (parts, body) = response.into_parts();
        let body_bytes = body
            .collect()
            .await
            .map_err(|e| {
                RotaError::ProxyConnectionFailed(format!("Failed to read response: {}", e))
            })?
            .to_bytes();

        Ok(Response::from_parts(parts, Full::new(body_bytes)))
    }

    fn persist_request_record(&self, record: RequestRecord) {
        let pool = self.db_pool.clone();
        tokio::spawn(async move {
            let log_repo = LogRepository::new(pool.clone());
            if let Err(e) = log_repo.record_request(&record).await {
                warn!(
                    proxy_id = record.proxy_id,
                    proxy_address = %record.proxy_address,
                    error = %e,
                    "Failed to record proxy request"
                );
            }

            if record.proxy_id != 0 {
                let proxy_repo = ProxyRepository::new(pool);
                if let Err(e) = proxy_repo
                    .record_request(
                        record.proxy_id,
                        record.success,
                        record.response_time,
                        record.error_message.as_deref(),
                    )
                    .await
                {
                    warn!(
                        proxy_id = record.proxy_id,
                        proxy_address = %record.proxy_address,
                        error = %e,
                        "Failed to update proxy statistics"
                    );
                }
            }
        });
    }

    fn broadcast_request_record(&self, record: &RequestRecord) {
        if !self.config.enable_logging {
            return;
        }

        if let Some(sender) = &self.log_sender {
            let _ = sender.send(record.clone());
        }
    }

    /// Create an error response
    fn error_response(&self, status: StatusCode, message: &str) -> Response<Full<Bytes>> {
        Response::builder()
            .status(status)
            .header("Content-Type", "text/plain")
            .body(Full::new(Bytes::from(message.to_string())))
            .unwrap()
    }

    // NOTE: logging/broadcast is handled via `broadcast_request_record` so status codes stay
    // consistent with persisted records.
}

/// Check if a header is a hop-by-hop header that should not be forwarded
fn is_hop_by_hop_header(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailers"
            | "transfer-encoding"
            | "upgrade"
    )
}
