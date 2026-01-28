//! Time-based proxy rotation strategy

use async_trait::async_trait;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{ConnectionTracker, ProxySelector};
use crate::error::{Result, RotaError};
use crate::models::Proxy;

/// Rotates to the next proxy after a configurable time interval
///
/// Uses atomic operations for lock-free timing checks.
pub struct TimeBasedSelector {
    proxies: RwLock<Vec<Arc<Proxy>>>,
    current_index: RwLock<usize>,
    last_rotation: RwLock<Instant>,
    /// Rotation interval in seconds
    rotation_interval_secs: AtomicU64,
    tracker: ConnectionTracker,
}

impl TimeBasedSelector {
    pub fn new() -> Self {
        Self::with_interval(Duration::from_secs(60))
    }

    pub fn with_interval(interval: Duration) -> Self {
        Self {
            proxies: RwLock::new(Vec::new()),
            current_index: RwLock::new(0),
            last_rotation: RwLock::new(Instant::now()),
            rotation_interval_secs: AtomicU64::new(interval.as_secs()),
            tracker: ConnectionTracker::new(),
        }
    }

    /// Update the rotation interval
    pub fn set_interval(&self, interval: Duration) {
        self.rotation_interval_secs
            .store(interval.as_secs(), Ordering::Relaxed);
    }

    /// Get the current rotation interval
    pub fn get_interval(&self) -> Duration {
        Duration::from_secs(self.rotation_interval_secs.load(Ordering::Relaxed))
    }

    fn maybe_rotate(&self, proxy_count: usize) {
        if proxy_count == 0 {
            return;
        }

        let interval = Duration::from_secs(self.rotation_interval_secs.load(Ordering::Relaxed));
        let now = Instant::now();

        let should_rotate = {
            let last = self.last_rotation.read();
            now.duration_since(*last) >= interval
        };

        if should_rotate {
            let mut index = self.current_index.write();
            let mut last = self.last_rotation.write();

            // Double-check after acquiring write locks
            if now.duration_since(*last) >= interval {
                *index = (*index + 1) % proxy_count;
                *last = now;
            }
        }
    }
}

impl Default for TimeBasedSelector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProxySelector for TimeBasedSelector {
    async fn select(&self) -> Result<Arc<Proxy>> {
        let proxies = self.proxies.read();

        if proxies.is_empty() {
            return Err(RotaError::NoProxiesAvailable);
        }

        // Check if we need to rotate
        self.maybe_rotate(proxies.len());

        let index = *self.current_index.read();
        proxies
            .get(index)
            .cloned()
            .ok_or(RotaError::NoProxiesAvailable)
    }

    async fn refresh(&self, proxies: Vec<Proxy>) -> Result<()> {
        let mut guard = self.proxies.write();
        let new_len = proxies.len();
        *guard = proxies.into_iter().map(Arc::new).collect();

        // Adjust current index if it's out of bounds
        if new_len > 0 {
            let mut index = self.current_index.write();
            if *index >= new_len {
                *index = 0;
            }
        }

        Ok(())
    }

    fn available_count(&self) -> usize {
        self.proxies.read().len()
    }

    fn strategy_name(&self) -> &'static str {
        "time_based"
    }

    fn acquire(&self, proxy_id: i64) {
        self.tracker.acquire(proxy_id);
    }

    fn release(&self, proxy_id: i64) {
        self.tracker.release(proxy_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn create_test_proxy(id: i32, address: &str) -> Proxy {
        Proxy {
            id,
            address: address.to_string(),
            protocol: "http".to_string(),
            username: None,
            password: None,
            status: "idle".to_string(),
            requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            avg_response_time: 0,
            last_check: None,
            last_error: None,
            auto_delete_after_failed_seconds: None,
            invalid_since: None,
            failure_reasons: serde_json::Value::Array(Vec::new()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_time_based_empty() {
        let selector = TimeBasedSelector::new();
        let result = selector.select().await;
        assert!(matches!(result, Err(RotaError::NoProxiesAvailable)));
    }

    #[tokio::test]
    async fn test_time_based_same_proxy_within_interval() {
        let selector = TimeBasedSelector::with_interval(Duration::from_secs(60));
        let proxies = vec![
            create_test_proxy(1, "127.0.0.1:8081"),
            create_test_proxy(2, "127.0.0.1:8082"),
            create_test_proxy(3, "127.0.0.1:8083"),
        ];
        selector.refresh(proxies).await.unwrap();

        // Multiple calls within interval should return same proxy
        let first = selector.select().await.unwrap();
        for _ in 0..10 {
            let selected = selector.select().await.unwrap();
            assert_eq!(selected.id, first.id);
        }
    }

    #[tokio::test]
    async fn test_time_based_rotates_after_interval() {
        let selector = TimeBasedSelector::with_interval(Duration::from_secs(60));
        let proxies = vec![
            create_test_proxy(1, "127.0.0.1:8081"),
            create_test_proxy(2, "127.0.0.1:8082"),
        ];
        selector.refresh(proxies).await.unwrap();

        let first = selector.select().await.unwrap();
        assert_eq!(first.id, 1);

        // Fast-forward time by mutating the internal timestamp.
        *selector.last_rotation.write() = Instant::now() - Duration::from_secs(61);

        let second = selector.select().await.unwrap();
        assert_eq!(second.id, 2);
    }

    #[tokio::test]
    async fn test_time_based_interval_update() {
        let selector = TimeBasedSelector::with_interval(Duration::from_secs(60));
        assert_eq!(selector.get_interval(), Duration::from_secs(60));

        selector.set_interval(Duration::from_secs(120));
        assert_eq!(selector.get_interval(), Duration::from_secs(120));
    }

    #[tokio::test]
    async fn test_time_based_refresh_adjusts_index() {
        let selector = TimeBasedSelector::with_interval(Duration::from_secs(60));
        let proxies = vec![
            create_test_proxy(1, "127.0.0.1:8081"),
            create_test_proxy(2, "127.0.0.1:8082"),
        ];
        selector.refresh(proxies).await.unwrap();

        *selector.current_index.write() = 10;

        let new_proxies = vec![create_test_proxy(99, "127.0.0.1:8099")];
        selector.refresh(new_proxies).await.unwrap();

        let selected = selector.select().await.unwrap();
        assert_eq!(selected.id, 99);
    }
}
