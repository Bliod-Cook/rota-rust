//! Round-robin proxy selection strategy

use async_trait::async_trait;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::{ConnectionTracker, ProxySelector};
use crate::error::{Result, RotaError};
use crate::models::Proxy;

/// Selects proxies in round-robin order
///
/// Uses atomic operations for lock-free index tracking.
pub struct RoundRobinSelector {
    proxies: RwLock<Vec<Arc<Proxy>>>,
    index: AtomicUsize,
    tracker: ConnectionTracker,
}

impl RoundRobinSelector {
    pub fn new() -> Self {
        Self {
            proxies: RwLock::new(Vec::new()),
            index: AtomicUsize::new(0),
            tracker: ConnectionTracker::new(),
        }
    }
}

impl Default for RoundRobinSelector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProxySelector for RoundRobinSelector {
    async fn select(&self) -> Result<Arc<Proxy>> {
        let proxies = self.proxies.read();

        if proxies.is_empty() {
            return Err(RotaError::NoProxiesAvailable);
        }

        let len = proxies.len();
        // Atomically increment and get the previous value, then wrap around
        let idx = self.index.fetch_add(1, Ordering::Relaxed) % len;

        proxies.get(idx).cloned().ok_or(RotaError::NoProxiesAvailable)
    }

    async fn refresh(&self, proxies: Vec<Proxy>) -> Result<()> {
        let mut guard = self.proxies.write();
        *guard = proxies.into_iter().map(Arc::new).collect();
        // Reset index on refresh to avoid potential issues with changed list size
        self.index.store(0, Ordering::Relaxed);
        Ok(())
    }

    fn available_count(&self) -> usize {
        self.proxies.read().len()
    }

    fn strategy_name(&self) -> &'static str {
        "round_robin"
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
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_round_robin_empty() {
        let selector = RoundRobinSelector::new();
        let result = selector.select().await;
        assert!(matches!(result, Err(RotaError::NoProxiesAvailable)));
    }

    #[tokio::test]
    async fn test_round_robin_order() {
        let selector = RoundRobinSelector::new();
        let proxies = vec![
            create_test_proxy(1, "127.0.0.1:8081"),
            create_test_proxy(2, "127.0.0.1:8082"),
            create_test_proxy(3, "127.0.0.1:8083"),
        ];
        selector.refresh(proxies).await.unwrap();

        // Should cycle through 1, 2, 3, 1, 2, 3...
        assert_eq!(selector.select().await.unwrap().id, 1);
        assert_eq!(selector.select().await.unwrap().id, 2);
        assert_eq!(selector.select().await.unwrap().id, 3);
        assert_eq!(selector.select().await.unwrap().id, 1);
        assert_eq!(selector.select().await.unwrap().id, 2);
        assert_eq!(selector.select().await.unwrap().id, 3);
    }

    #[tokio::test]
    async fn test_round_robin_refresh_resets_index() {
        let selector = RoundRobinSelector::new();
        let proxies = vec![
            create_test_proxy(1, "127.0.0.1:8081"),
            create_test_proxy(2, "127.0.0.1:8082"),
        ];
        selector.refresh(proxies).await.unwrap();

        // Advance the index
        selector.select().await.unwrap();
        selector.select().await.unwrap();

        // Refresh should reset
        let new_proxies = vec![
            create_test_proxy(10, "127.0.0.1:8091"),
            create_test_proxy(20, "127.0.0.1:8092"),
        ];
        selector.refresh(new_proxies).await.unwrap();

        // Should start from the beginning
        assert_eq!(selector.select().await.unwrap().id, 10);
    }
}
