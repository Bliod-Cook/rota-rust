use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::RwLock;

use super::{create_selector, ProxySelector, RotationStrategy, TimeBasedSelector};
use crate::error::Result;
use crate::models::Proxy;

/// A proxy selector that can swap the underlying strategy at runtime.
pub struct DynamicProxySelector {
    inner: RwLock<Arc<dyn ProxySelector>>,
    proxies: RwLock<Vec<Proxy>>,
}

impl DynamicProxySelector {
    pub fn new(initial: Arc<dyn ProxySelector>) -> Self {
        Self {
            inner: RwLock::new(initial),
            proxies: RwLock::new(Vec::new()),
        }
    }

    pub async fn set_strategy(
        &self,
        strategy: RotationStrategy,
        time_based_interval: Duration,
    ) -> Result<()> {
        let selector: Arc<dyn ProxySelector> = match strategy {
            RotationStrategy::TimeBased => {
                Arc::new(TimeBasedSelector::with_interval(time_based_interval))
            }
            _ => Arc::from(create_selector(strategy)),
        };

        // Carry over the latest proxy list to the new selector.
        let proxies = self.proxies.read().clone();
        selector.refresh(proxies).await?;

        *self.inner.write() = selector;
        Ok(())
    }
}

#[async_trait]
impl ProxySelector for DynamicProxySelector {
    async fn select(&self) -> Result<Arc<Proxy>> {
        let selector = self.inner.read().clone();
        selector.select().await
    }

    async fn refresh(&self, proxies: Vec<Proxy>) -> Result<()> {
        *self.proxies.write() = proxies.clone();
        let selector = self.inner.read().clone();
        selector.refresh(proxies).await
    }

    fn available_count(&self) -> usize {
        self.inner.read().available_count()
    }

    fn strategy_name(&self) -> &'static str {
        self.inner.read().strategy_name()
    }

    fn acquire(&self, proxy_id: i64) {
        self.inner.read().acquire(proxy_id);
    }

    fn release(&self, proxy_id: i64) {
        self.inner.read().release(proxy_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::proxy::rotation::RoundRobinSelector;

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
    async fn test_dynamic_selector_refresh_propagates() {
        let inner: Arc<dyn ProxySelector> = Arc::new(RoundRobinSelector::new());
        let selector = DynamicProxySelector::new(inner);

        selector
            .refresh(vec![
                create_test_proxy(1, "127.0.0.1:8081"),
                create_test_proxy(2, "127.0.0.1:8082"),
            ])
            .await
            .unwrap();

        assert_eq!(selector.available_count(), 2);
        assert_eq!(selector.strategy_name(), "round_robin");
        assert_eq!(selector.select().await.unwrap().id, 1);

        selector
            .refresh(vec![create_test_proxy(99, "127.0.0.1:8099")])
            .await
            .unwrap();

        assert_eq!(selector.available_count(), 1);
        assert_eq!(selector.select().await.unwrap().id, 99);
    }

    #[tokio::test]
    async fn test_dynamic_selector_switch_strategy_preserves_proxies_and_tracking() {
        let inner: Arc<dyn ProxySelector> = Arc::new(RoundRobinSelector::new());
        let selector = DynamicProxySelector::new(inner);

        selector
            .refresh(vec![
                create_test_proxy(1, "127.0.0.1:8081"),
                create_test_proxy(2, "127.0.0.1:8082"),
                create_test_proxy(3, "127.0.0.1:8083"),
            ])
            .await
            .unwrap();

        // Prove the initial strategy is in effect.
        assert_eq!(selector.strategy_name(), "round_robin");
        assert_eq!(selector.select().await.unwrap().id, 1);
        assert_eq!(selector.select().await.unwrap().id, 2);

        // Swap to least-connections and ensure the proxy list is carried over.
        selector
            .set_strategy(RotationStrategy::LeastConnections, Duration::from_secs(60))
            .await
            .unwrap();

        assert_eq!(selector.strategy_name(), "least_connections");
        assert_eq!(selector.available_count(), 3);

        // Acquire/release should be forwarded to the active strategy.
        selector.acquire(1);
        selector.acquire(1);
        selector.acquire(2);

        assert_eq!(selector.select().await.unwrap().id, 3);

        selector.release(1);
        selector.release(1);
        selector.release(2);

        assert_eq!(selector.select().await.unwrap().id, 1);
    }
}
