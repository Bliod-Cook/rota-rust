//! Proxy auto-delete (archive) service
//!
//! Periodically scans for proxies that have been continuously failed for longer than their
//! configured `auto_delete_after_failed_seconds` and moves them into `deleted_proxies`.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::interval;
use tracing::{error, info, instrument};

use crate::database::Database;
use crate::error::Result;
use crate::models::Settings;
use crate::proxy::rotation::{DynamicProxySelector, ProxySelector};
use crate::repository::ProxyRepository;

/// Proxy auto-delete service configuration
#[derive(Clone)]
pub struct ProxyAutoDeleteConfig {
    /// How often to scan for expired failed proxies
    pub check_interval: Duration,
    /// Max number of proxies to archive per scan
    pub batch_limit: i64,
}

impl Default for ProxyAutoDeleteConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(60),
            batch_limit: 100,
        }
    }
}

/// Proxy auto-delete service
pub struct ProxyAutoDeleteService {
    db: Database,
    selector: Arc<DynamicProxySelector>,
    config: ProxyAutoDeleteConfig,
}

impl ProxyAutoDeleteService {
    pub fn new(
        db: Database,
        selector: Arc<DynamicProxySelector>,
        config: ProxyAutoDeleteConfig,
    ) -> Self {
        Self {
            db,
            selector,
            config,
        }
    }

    /// Run the proxy auto-delete service
    #[instrument(skip(self, shutdown, settings_rx))]
    pub async fn run(
        &self,
        mut shutdown: watch::Receiver<bool>,
        mut settings_rx: watch::Receiver<Settings>,
    ) {
        info!(
            "Starting proxy auto-delete service (interval: {}s, batch_limit: {})",
            self.config.check_interval.as_secs(),
            self.config.batch_limit
        );

        // Initial scan on startup.
        let settings = settings_rx.borrow().clone();
        if let Err(e) = self.scan_and_archive(&settings).await {
            error!("Initial proxy auto-delete scan failed: {}", e);
        }

        let mut ticker = interval(self.config.check_interval);
        ticker.tick().await; // Skip immediate tick

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    let settings = settings_rx.borrow().clone();
                    if let Err(e) = self.scan_and_archive(&settings).await {
                        error!("Proxy auto-delete scan failed: {}", e);
                    }
                }
                _ = settings_rx.changed() => {
                    // Settings updates are read on the next tick; we just keep the latest.
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("Proxy auto-delete service shutting down");
                        break;
                    }
                }
            }
        }
    }

    #[instrument(skip(self))]
    async fn scan_and_archive(&self, settings: &Settings) -> Result<()> {
        let repo = ProxyRepository::new(self.db.pool().clone());

        let mut total_archived = 0usize;

        loop {
            let archived_ids = repo.archive_expired_failed(self.config.batch_limit).await?;

            if archived_ids.is_empty() {
                break;
            }

            total_archived += archived_ids.len();

            // Refresh selector after changes.
            let proxies = if settings.rotation.remove_unhealthy {
                repo.get_all_usable().await?
            } else {
                repo.get_all().await?
            };
            if let Err(e) = self.selector.refresh(proxies).await {
                error!("Failed to refresh selector after archiving proxies: {}", e);
            }

            // Continue draining if there are more candidates.
            if archived_ids.len() < self.config.batch_limit as usize {
                break;
            }
        }

        if total_archived > 0 {
            info!(count = total_archived, "Archived expired failed proxies");
        }

        Ok(())
    }
}

/// Handle for managing the proxy auto-delete service
pub struct ProxyAutoDeleteHandle {
    shutdown_tx: watch::Sender<bool>,
}

impl ProxyAutoDeleteHandle {
    pub fn new() -> (Self, watch::Receiver<bool>) {
        let (tx, rx) = watch::channel(false);
        (Self { shutdown_tx: tx }, rx)
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

impl Default for ProxyAutoDeleteHandle {
    fn default() -> Self {
        Self::new().0
    }
}
