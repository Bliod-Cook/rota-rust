//! Health checking for upstream proxies
//!
//! Periodically checks proxy availability and updates health status.

use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::time::{interval, timeout};
use tracing::{debug, error, info, instrument, warn};

use futures::StreamExt;

use crate::database::Database;
use crate::error::Result;
use crate::models::{Proxy, Settings};
use crate::proxy::rotation::ProxySelector;
use crate::proxy::transport::ProxyTransport;
use crate::repository::ProxyRepository;

/// Health checker configuration
#[derive(Clone)]
pub struct HealthCheckerConfig {
    /// Interval between health checks
    pub check_interval: Duration,
    /// Timeout for each health check
    pub check_timeout: Duration,
    /// URL to use for health checks
    pub check_url: String,
}

impl Default for HealthCheckerConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(30),
            check_timeout: Duration::from_secs(10),
            check_url: "http://www.google.com".to_string(),
        }
    }
}

/// Health checker for upstream proxies
pub struct HealthChecker {
    db: Database,
    config: HealthCheckerConfig,
    selector: Arc<dyn ProxySelector>,
}

impl HealthChecker {
    /// Create a new health checker
    pub fn new(
        db: Database,
        config: HealthCheckerConfig,
        selector: Arc<dyn ProxySelector>,
    ) -> Self {
        Self {
            db,
            config,
            selector,
        }
    }

    /// Run the health checker (call in a spawned task)
    #[instrument(skip(self, shutdown, settings_rx))]
    pub async fn run(
        &self,
        mut shutdown: watch::Receiver<bool>,
        mut settings_rx: watch::Receiver<Settings>,
    ) {
        info!(
            "Starting health checker with {}s interval",
            self.config.check_interval.as_secs()
        );

        let mut check_interval = interval(self.config.check_interval);

        loop {
            tokio::select! {
                _ = check_interval.tick() => {
                    let settings = settings_rx.borrow().clone();
                    if let Err(e) = self.check_all_proxies(&settings).await {
                        error!("Health check round failed: {}", e);
                    }
                }
                _ = settings_rx.changed() => {
                    // Settings updates are applied on the next tick; we just keep the latest.
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("Health checker shutting down");
                        break;
                    }
                }
            }
        }
    }

    /// Check all proxies and update their health status
    async fn check_all_proxies(&self, settings: &Settings) -> Result<()> {
        let repo = ProxyRepository::new(self.db.pool().clone());
        // Include failed proxies so they can recover when they become reachable again.
        let proxies = repo.get_all().await?;

        info!("Checking health of {} proxies", proxies.len());

        let worker_count = settings.healthcheck.workers.max(1) as usize;
        let settings = settings.clone();

        let results = futures::stream::iter(proxies)
            .map(|proxy| {
                let repo = repo.clone();
                let settings = settings.clone();
                async move {
                    let (is_healthy, error_msg) = self.check_proxy(&proxy, &settings).await;

                    if let Err(e) = repo
                        .record_health_check(proxy.id, is_healthy, error_msg.as_deref())
                        .await
                    {
                        warn!("Failed to record health check for {}: {}", proxy.address, e);
                    }

                    is_healthy
                }
            })
            .buffer_unordered(worker_count)
            .collect::<Vec<bool>>()
            .await;

        let healthy_count = results.iter().filter(|&&v| v).count();
        let unhealthy_count = results.len().saturating_sub(healthy_count);

        // Refresh the selector with updated proxy list
        // Re-fetch proxies to get updated status
        let refreshed_proxies = if settings.rotation.remove_unhealthy {
            repo.get_all_usable().await?
        } else {
            repo.get_all().await?
        };
        if let Err(e) = self.selector.refresh(refreshed_proxies).await {
            error!("Failed to refresh selector: {}", e);
        }

        info!(
            "Health check complete: {} healthy, {} unhealthy",
            healthy_count, unhealthy_count
        );

        Ok(())
    }

    /// Check a single proxy's health
    /// Returns (is_healthy, optional_error_message)
    #[instrument(skip(self), fields(proxy_id = proxy.id, proxy_address = %proxy.address))]
    async fn check_proxy(&self, proxy: &Proxy, settings: &Settings) -> (bool, Option<String>) {
        debug!("Checking health of proxy at {}", proxy.address);

        let check_url = if settings.healthcheck.url.is_empty() {
            self.config.check_url.as_str()
        } else {
            settings.healthcheck.url.as_str()
        };

        let (target_host, target_port) = match url::Url::parse(check_url)
            .ok()
            .and_then(|u| Some((u.host_str()?.to_string(), u.port_or_known_default()?)))
        {
            Some(v) => v,
            None => ("www.google.com".to_string(), 80),
        };

        let check_timeout = Duration::from_secs(settings.healthcheck.timeout.max(1) as u64);

        // Establish a proxied connection to a known host/port. This validates both:
        // 1) connectivity to the proxy itself, and 2) the proxy's ability to reach the target.
        let connect_result = timeout(
            check_timeout,
            ProxyTransport::connect(proxy, &target_host, target_port),
        )
        .await;

        match connect_result {
            Ok(Ok(_conn)) => {
                debug!(
                    "Proxy {} is healthy (CONNECT to {}:{} successful)",
                    proxy.address, target_host, target_port
                );
                (true, None)
            }
            Ok(Err(e)) => {
                let msg = format!("connect failed: {}", e);
                warn!("Proxy {} is unhealthy: {}", proxy.address, msg);
                (false, Some(msg))
            }
            Err(_) => {
                let msg = "connect timed out".to_string();
                warn!("Proxy {} is unhealthy: {}", proxy.address, msg);
                (false, Some(msg))
            }
        }
    }

    /// Perform a more thorough health check using HTTP
    #[allow(dead_code)]
    async fn check_proxy_http(&self, proxy: &Proxy) -> (bool, Option<String>) {
        // Connect to proxy
        let stream = match timeout(
            self.config.check_timeout,
            TcpStream::connect(&proxy.address),
        )
        .await
        {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                let msg = format!("connection failed: {}", e);
                warn!("Proxy {} {}", proxy.address, msg);
                return (false, Some(msg));
            }
            Err(_) => {
                let msg = "connection timed out".to_string();
                warn!("Proxy {} {}", proxy.address, msg);
                return (false, Some(msg));
            }
        };

        // Send a simple HTTP request through the proxy
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let mut stream = stream;
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: www.google.com\r\nConnection: close\r\n\r\n",
            self.config.check_url
        );

        if let Err(e) = stream.write_all(request.as_bytes()).await {
            let msg = format!("write failed: {}", e);
            warn!("Proxy {} {}", proxy.address, msg);
            return (false, Some(msg));
        }

        let mut response = vec![0u8; 1024];
        match timeout(self.config.check_timeout, stream.read(&mut response)).await {
            Ok(Ok(n)) if n > 0 => {
                let response_str = String::from_utf8_lossy(&response[..n]);
                if response_str.contains("HTTP/") {
                    debug!("Proxy {} is healthy (HTTP check successful)", proxy.address);
                    (true, None)
                } else {
                    let msg = "invalid HTTP response".to_string();
                    warn!("Proxy {} returned {}", proxy.address, msg);
                    (false, Some(msg))
                }
            }
            Ok(Ok(_)) => {
                let msg = "empty response".to_string();
                warn!("Proxy {} returned {}", proxy.address, msg);
                (false, Some(msg))
            }
            Ok(Err(e)) => {
                let msg = format!("read failed: {}", e);
                warn!("Proxy {} {}", proxy.address, msg);
                (false, Some(msg))
            }
            Err(_) => {
                let msg = "read timed out".to_string();
                warn!("Proxy {} {}", proxy.address, msg);
                (false, Some(msg))
            }
        }
    }
}

/// Guard for managing health checker lifecycle
pub struct HealthCheckerHandle {
    shutdown_tx: watch::Sender<bool>,
}

impl HealthCheckerHandle {
    pub fn new() -> (Self, watch::Receiver<bool>) {
        let (tx, rx) = watch::channel(false);
        (Self { shutdown_tx: tx }, rx)
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

impl Default for HealthCheckerHandle {
    fn default() -> Self {
        Self::new().0
    }
}
