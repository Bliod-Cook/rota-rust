//! Random proxy selection strategy

use async_trait::async_trait;
use parking_lot::RwLock;
use rand::seq::SliceRandom;
use std::sync::Arc;

use super::{ConnectionTracker, ProxySelector};
use crate::error::{Result, RotaError};
use crate::models::Proxy;

/// Selects a random proxy from the available pool
pub struct RandomSelector {
    proxies: RwLock<Vec<Arc<Proxy>>>,
    tracker: ConnectionTracker,
}

impl RandomSelector {
    pub fn new() -> Self {
        Self {
            proxies: RwLock::new(Vec::new()),
            tracker: ConnectionTracker::new(),
        }
    }
}

impl Default for RandomSelector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProxySelector for RandomSelector {
    async fn select(&self) -> Result<Arc<Proxy>> {
        let proxies = self.proxies.read();

        if proxies.is_empty() {
            return Err(RotaError::NoProxiesAvailable);
        }

        let mut rng = rand::thread_rng();
        proxies
            .choose(&mut rng)
            .cloned()
            .ok_or(RotaError::NoProxiesAvailable)
    }

    async fn refresh(&self, proxies: Vec<Proxy>) -> Result<()> {
        let mut guard = self.proxies.write();
        *guard = proxies.into_iter().map(Arc::new).collect();
        Ok(())
    }

    fn available_count(&self) -> usize {
        self.proxies.read().len()
    }

    fn strategy_name(&self) -> &'static str {
        "random"
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
    async fn test_random_selector_empty() {
        let selector = RandomSelector::new();
        let result = selector.select().await;
        assert!(matches!(result, Err(RotaError::NoProxiesAvailable)));
    }

    #[tokio::test]
    async fn test_random_selector_single_proxy() {
        let selector = RandomSelector::new();
        let proxies = vec![create_test_proxy(1, "127.0.0.1:8081")];
        selector.refresh(proxies).await.unwrap();

        let selected = selector.select().await.unwrap();
        assert_eq!(selected.id, 1);
    }

    #[tokio::test]
    async fn test_random_selector_multiple_proxies() {
        let selector = RandomSelector::new();
        let proxies = vec![
            create_test_proxy(1, "127.0.0.1:8081"),
            create_test_proxy(2, "127.0.0.1:8082"),
            create_test_proxy(3, "127.0.0.1:8083"),
        ];
        selector.refresh(proxies).await.unwrap();

        // Select multiple times and ensure we get valid proxies
        for _ in 0..10 {
            let selected = selector.select().await.unwrap();
            assert!(selected.id >= 1 && selected.id <= 3);
        }
    }
}
