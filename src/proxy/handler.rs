//! Proxy request handler with retry logic
//!
//! Handles incoming HTTP/HTTPS requests and forwards them through upstream proxies.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::PROXY_AUTHORIZATION;
use hyper::{Method, Request, Response, StatusCode};
use tokio::sync::broadcast;
use tracing::{debug, error, info, instrument, warn};

use crate::error::{Result, RotaError};
use crate::models::{Proxy, RequestRecord};
use crate::proxy::rotation::ProxySelector;
use crate::proxy::transport::ProxyTransport;
use crate::proxy::tunnel::{TunnelGuard, TunnelHandler};

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
}

impl ProxyHandler {
    pub fn new(
        selector: Arc<dyn ProxySelector>,
        config: ProxyHandlerConfig,
        log_sender: Option<broadcast::Sender<RequestRecord>>,
    ) -> Self {
        Self {
            selector,
            config,
            log_sender,
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

        // Select a proxy with retry logic
        let mut attempts = 0;
        let max_attempts = self.config.max_retries + 1;
        let mut last_error = None;

        while attempts < max_attempts {
            attempts += 1;

            let proxy = match self.selector.select().await {
                Ok(p) => p,
                Err(e) => {
                    error!("No proxy available: {}", e);
                    return Ok(self.error_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "No proxies available",
                    ));
                }
            };

            // Create tunnel guard to track connection
            let _guard = TunnelGuard::new(proxy.id as i64, self.selector.clone());

            debug!(
                "Attempting CONNECT through proxy {} (attempt {}/{})",
                proxy.address, attempts, max_attempts
            );

            // Try to establish tunnel
            match tokio::time::timeout(
                self.config.connect_timeout,
                TunnelHandler::tunnel_through_proxy(&proxy, &target_host, target_port),
            )
            .await
            {
                Ok(Ok(_connection)) => {
                    // Return 200 Connection Established
                    // The actual tunneling will be handled after upgrade
                    info!(
                        "CONNECT tunnel established through {} to {}:{}",
                        proxy.address, target_host, target_port
                    );

                    // Log the request
                    self.log_request(
                        "CONNECT",
                        &authority,
                        true,
                        0,
                        Some(&proxy),
                        None,
                    );

                    return Ok(Response::builder()
                        .status(StatusCode::OK)
                        .body(Full::new(Bytes::new()))
                        .unwrap());
                }
                Ok(Err(e)) => {
                    warn!(
                        "CONNECT through {} failed: {} (attempt {}/{})",
                        proxy.address, e, attempts, max_attempts
                    );
                    last_error = Some(e);
                }
                Err(_) => {
                    warn!(
                        "CONNECT through {} timed out (attempt {}/{})",
                        proxy.address, attempts, max_attempts
                    );
                    last_error = Some(RotaError::Timeout);
                }
            }
        }

        error!(
            "All CONNECT attempts failed after {} attempts",
            max_attempts
        );
        Ok(self.error_response(
            StatusCode::BAD_GATEWAY,
            &format!(
                "Failed to establish tunnel: {}",
                last_error.unwrap_or(RotaError::NoProxiesAvailable)
            ),
        ))
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
                    return Ok(self.error_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "No proxies available",
                    ));
                }
            };

            // Track connection
            let _guard = TunnelGuard::new(proxy.id as i64, self.selector.clone());

            debug!(
                "Forwarding HTTP request through proxy {} (attempt {}/{})",
                proxy.address, attempts, max_attempts
            );

            match self
                .forward_request(&proxy, &parts, body_bytes.clone(), &target_host, target_port)
                .await
            {
                Ok(response) => {
                    let duration = start.elapsed();
                    let status = response.status().as_u16();

                    // Log the request
                    self.log_request(
                        method.as_str(),
                        &uri.to_string(),
                        status < 400,
                        duration.as_millis() as i32,
                        Some(&proxy),
                        None,
                    );

                    return Ok(response);
                }
                Err(e) => {
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

        // Log failed request
        self.log_request(
            method.as_str(),
            &uri.to_string(),
            false,
            duration.as_millis() as i32,
            None,
            last_error.as_ref().map(|e| e.to_string()).as_deref(),
        );

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
            format!("http://{}{}", target_host, parts.uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/"))
        } else {
            format!("http://{}:{}{}", target_host, target_port, parts.uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/"))
        };

        // Connect to proxy (address format is "host:port")
        let stream = tokio::time::timeout(
            self.config.connect_timeout,
            tokio::net::TcpStream::connect(&proxy.address),
        )
        .await
        .map_err(|_| RotaError::Timeout)?
        .map_err(|e| RotaError::ProxyConnectionFailed(format!("Connect failed: {}", e)))?;

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
            let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, credentials);
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
        let response = tokio::time::timeout(self.config.request_timeout, sender.send_request(request))
            .await
            .map_err(|_| RotaError::Timeout)?
            .map_err(|e| RotaError::ProxyConnectionFailed(format!("Request failed: {}", e)))?;

        // Collect response body
        let (parts, body) = response.into_parts();
        let body_bytes = body
            .collect()
            .await
            .map_err(|e| RotaError::ProxyConnectionFailed(format!("Failed to read response: {}", e)))?
            .to_bytes();

        Ok(Response::from_parts(parts, Full::new(body_bytes)))
    }

    /// Create an error response
    fn error_response(&self, status: StatusCode, message: &str) -> Response<Full<Bytes>> {
        Response::builder()
            .status(status)
            .header("Content-Type", "text/plain")
            .body(Full::new(Bytes::from(message.to_string())))
            .unwrap()
    }

    /// Log a request
    fn log_request(
        &self,
        method: &str,
        url: &str,
        success: bool,
        response_time: i32,
        proxy: Option<&Proxy>,
        error_message: Option<&str>,
    ) {
        if !self.config.enable_logging {
            return;
        }

        let record = RequestRecord {
            proxy_id: proxy.map(|p| p.id).unwrap_or(0),
            proxy_address: proxy.map(|p| p.address.clone()).unwrap_or_default(),
            requested_url: url.to_string(),
            method: method.to_string(),
            success,
            response_time,
            status_code: if success { 200 } else { 502 },
            error_message: error_message.map(|s| s.to_string()),
            timestamp: chrono::Utc::now(),
        };

        if let Some(sender) = &self.log_sender {
            // Use try_send to avoid blocking - this fixes the memory leak issue from Go
            let _ = sender.send(record);
        }
    }
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
