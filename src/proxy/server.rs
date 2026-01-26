//! Proxy server implementation using hyper
//!
//! Handles incoming proxy requests and forwards them through upstream proxies.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, watch};
use tracing::{debug, error, info, instrument};

use std::time::Duration;

use crate::config::ProxyServerConfig;
use crate::error::Result;
use crate::models::RequestRecord;
use crate::proxy::handler::{ProxyHandler, ProxyHandlerConfig};
use crate::proxy::middleware::{ProxyAuth, RateLimiter};
use crate::proxy::rotation::ProxySelector;

/// Proxy server
pub struct ProxyServer {
    config: ProxyServerConfig,
    handler: Arc<ProxyHandler>,
    auth: ProxyAuth,
    rate_limiter: RateLimiter,
}

impl ProxyServer {
    /// Create a new proxy server
    pub fn new(
        config: ProxyServerConfig,
        selector: Arc<dyn ProxySelector>,
        log_sender: Option<broadcast::Sender<RequestRecord>>,
    ) -> Self {
        let handler_config = ProxyHandlerConfig {
            max_retries: config.max_retries,
            connect_timeout: Duration::from_secs(config.connect_timeout),
            request_timeout: Duration::from_secs(config.request_timeout),
            enable_logging: true,
        };

        let handler = Arc::new(ProxyHandler::new(selector, handler_config, log_sender));

        let auth = if config.auth_enabled {
            ProxyAuth::new(true, config.auth_username.clone(), config.auth_password.clone())
        } else {
            ProxyAuth::disabled()
        };

        let rate_limiter = if config.rate_limit_enabled {
            RateLimiter::new(true, config.rate_limit_per_second, config.rate_limit_burst)
        } else {
            RateLimiter::disabled()
        };

        Self {
            config,
            handler,
            auth,
            rate_limiter,
        }
    }

    /// Run the proxy server
    #[instrument(skip(self, shutdown))]
    pub async fn run(&self, mut shutdown: watch::Receiver<bool>) -> Result<()> {
        let addr: SocketAddr = format!("{}:{}", self.config.host, self.config.port)
            .parse()
            .expect("Invalid proxy server address");

        let listener = TcpListener::bind(addr).await?;
        info!("Proxy server listening on {}", addr);

        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, client_addr)) => {
                            let handler = self.handler.clone();
                            let auth = self.auth.clone();
                            let rate_limiter = self.rate_limiter.clone();

                            tokio::spawn(async move {
                                if let Err(e) = Self::handle_connection(
                                    stream,
                                    client_addr,
                                    handler,
                                    auth,
                                    rate_limiter,
                                ).await {
                                    debug!("Connection error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            error!("Accept error: {}", e);
                        }
                    }
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("Proxy server shutting down");
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// Handle a single connection
    async fn handle_connection(
        stream: tokio::net::TcpStream,
        client_addr: SocketAddr,
        handler: Arc<ProxyHandler>,
        auth: ProxyAuth,
        rate_limiter: RateLimiter,
    ) -> Result<()> {
        let io = TokioIo::new(stream);
        let client_ip = client_addr.ip().to_string();

        let service = service_fn(move |req: Request<Incoming>| {
            let handler = handler.clone();
            let auth = auth.clone();
            let rate_limiter = rate_limiter.clone();
            let client_ip = client_ip.clone();

            async move {
                // Check rate limit
                if let Err(_e) = rate_limiter.check(&client_ip) {
                    return Ok::<_, Infallible>(
                        Response::builder()
                            .status(StatusCode::TOO_MANY_REQUESTS)
                            .body(Full::new(Bytes::from("Rate limit exceeded")))
                            .unwrap(),
                    );
                }

                // Check authentication
                if let Err(_e) = auth.validate(&req) {
                    return Ok(auth.challenge_response());
                }

                // Handle the request
                match handler.handle(req, client_ip).await {
                    Ok(response) => Ok(response),
                    Err(e) => {
                        error!("Request handling error: {}", e);
                        Ok(Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(Full::new(Bytes::from(format!("Error: {}", e))))
                            .unwrap())
                    }
                }
            }
        });

        http1::Builder::new()
            .preserve_header_case(true)
            .title_case_headers(true)
            .serve_connection(io, service)
            .with_upgrades()
            .await
            .map_err(|e| crate::error::RotaError::ProxyConnectionFailed(e.to_string()))?;

        Ok(())
    }
}

/// Builder for creating a proxy server
pub struct ProxyServerBuilder {
    config: ProxyServerConfig,
    selector: Option<Arc<dyn ProxySelector>>,
    log_sender: Option<broadcast::Sender<RequestRecord>>,
}

impl ProxyServerBuilder {
    pub fn new(config: ProxyServerConfig) -> Self {
        Self {
            config,
            selector: None,
            log_sender: None,
        }
    }

    pub fn selector(mut self, selector: Arc<dyn ProxySelector>) -> Self {
        self.selector = Some(selector);
        self
    }

    pub fn log_sender(mut self, sender: broadcast::Sender<RequestRecord>) -> Self {
        self.log_sender = Some(sender);
        self
    }

    pub fn build(self) -> ProxyServer {
        let selector = self.selector.expect("Proxy selector is required");
        ProxyServer::new(self.config, selector, self.log_sender)
    }
}
