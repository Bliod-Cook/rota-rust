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
