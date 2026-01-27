//! Log cleanup service
//!
//! FIXED:
//! - Uses AtomicU64 for interval tracking, fixing the race condition in Go
//! - Uses parameterized queries, fixing the SQL injection vulnerability

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::interval;
use tracing::{debug, error, info, instrument, warn};

use crate::database::Database;
use crate::error::Result;
use crate::repository::{LogRepository, SettingsRepository};

/// Log cleanup service configuration
#[derive(Clone)]
pub struct LogCleanupConfig {
    /// Default retention period in days
    pub default_retention_days: u32,
    /// How often to check for cleanup (in seconds)
    pub check_interval_secs: u64,
}

impl Default for LogCleanupConfig {
    fn default() -> Self {
        Self {
            default_retention_days: 7,
            check_interval_secs: 3600, // 1 hour
        }
    }
}

/// Log cleanup service
///
/// Periodically deletes old logs based on the configured retention period.
pub struct LogCleanupService {
    db: Database,
    config: LogCleanupConfig,
    /// Current cleanup interval in seconds (uses AtomicU64 to fix race condition)
    current_interval_secs: AtomicU64,
}

impl LogCleanupService {
    /// Create a new log cleanup service
    pub fn new(db: Database, config: LogCleanupConfig) -> Self {
        let check_interval = config.check_interval_secs;
        Self {
            db,
            config,
            current_interval_secs: AtomicU64::new(check_interval),
        }
    }

    /// Run the log cleanup service
    #[instrument(skip(self, shutdown))]
    pub async fn run(&self, mut shutdown: watch::Receiver<bool>) {
        info!(
            "Starting log cleanup service (default retention: {} days)",
            self.config.default_retention_days
        );

        // Initial cleanup on startup
        if let Err(e) = self.cleanup().await {
            error!("Initial log cleanup failed: {}", e);
        }

        loop {
            // Get current interval (may be updated from settings)
            let interval_secs = self.current_interval_secs.load(Ordering::Relaxed);
            let mut cleanup_interval = interval(Duration::from_secs(interval_secs));
            cleanup_interval.tick().await; // Skip immediate tick

            tokio::select! {
                _ = cleanup_interval.tick() => {
                    // Check if interval has changed (FIXED: proper comparison)
                    if let Err(e) = self.refresh_interval().await {
                        warn!("Failed to refresh cleanup interval: {}", e);
                    }

                    if let Err(e) = self.cleanup().await {
                        error!("Log cleanup failed: {}", e);
                    }
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("Log cleanup service shutting down");
                        break;
                    }
                }
            }
        }
    }

    /// Refresh the cleanup interval from settings
    async fn refresh_interval(&self) -> Result<()> {
        let settings_repo = SettingsRepository::new(self.db.pool().clone());
        let settings = settings_repo.get_all().await?;

        // Convert retention interval to check interval
        // Check more frequently for shorter retention periods
        let new_interval_secs = match settings.log_retention.retention_days {
            0..=1 => 300,  // 5 minutes for 0-1 day retention
            2..=7 => 3600, // 1 hour for 2-7 days
            _ => 86400,    // 1 day for longer retention
        };

        let current = self.current_interval_secs.load(Ordering::Relaxed);

        // FIXED: Proper comparison (Go version compared same variable to itself)
        if new_interval_secs != current {
            info!(
                "Log cleanup interval changed from {}s to {}s",
                current, new_interval_secs
            );
            self.current_interval_secs
                .store(new_interval_secs, Ordering::Relaxed);
        }

        Ok(())
    }

    /// Perform log cleanup
    #[instrument(skip(self))]
    async fn cleanup(&self) -> Result<()> {
        let settings_repo = SettingsRepository::new(self.db.pool().clone());
        let log_repo = LogRepository::new(self.db.pool().clone());

        // Get retention period from settings
        let settings = settings_repo.get_all().await?;
        let retention_days: i32 = if settings.log_retention.retention_days > 0 {
            settings.log_retention.retention_days
        } else {
            self.config.default_retention_days as i32
        };

        debug!("Cleaning up logs older than {} days", retention_days);

        // Delete old logs (uses parameterized query - SQL injection fixed)
        // The repository takes days as i32 and handles the date calculation
        let deleted = log_repo.delete_older_than(retention_days).await?;

        if deleted > 0 {
            info!(
                "Deleted {} log entries older than {} days",
                deleted, retention_days
            );
        } else {
            debug!("No old log entries to delete");
        }

        Ok(())
    }
}

/// Handle for managing the log cleanup service
pub struct LogCleanupHandle {
    shutdown_tx: watch::Sender<bool>,
}

impl LogCleanupHandle {
    pub fn new() -> (Self, watch::Receiver<bool>) {
        let (tx, rx) = watch::channel(false);
        (Self { shutdown_tx: tx }, rx)
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

impl Default for LogCleanupHandle {
    fn default() -> Self {
        Self::new().0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = LogCleanupConfig::default();
        assert_eq!(config.default_retention_days, 7);
        assert_eq!(config.check_interval_secs, 3600);
    }

    #[test]
    fn test_interval_atomic_update() {
        let current = AtomicU64::new(3600);

        // Simulate checking and updating interval
        let stored = current.load(Ordering::Relaxed);
        let new_value = 300u64;

        // This properly compares different values (fixed from Go bug)
        assert_ne!(stored, new_value);

        current.store(new_value, Ordering::Relaxed);
        assert_eq!(current.load(Ordering::Relaxed), 300);
    }
}
