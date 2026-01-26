//! Least-connections proxy selection strategy

use async_trait::async_trait;
use parking_lot::RwLock;
use std::sync::Arc;

use super::{ConnectionTracker, ProxySelector};
use crate::error::{Result, RotaError};
use crate::models::Proxy;

/// Selects the proxy with the fewest active connections
///
/// This strategy helps distribute load evenly across proxies.
pub struct LeastConnectionsSelector {
    proxies: RwLock<Vec<Arc<Proxy>>>,
    tracker: ConnectionTracker,
}

impl LeastConnectionsSelector {
    pub fn new() -> Self {
        Self {
            proxies: RwLock::new(Vec::new()),
            tracker: ConnectionTracker::new(),
        }
    }
}

impl Default for LeastConnectionsSelector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProxySelector for LeastConnectionsSelector {
    async fn select(&self) -> Result<Arc<Proxy>> {
        let proxies = self.proxies.read();

        if proxies.is_empty() {
            return Err(RotaError::NoProxiesAvailable);
        }

        // Find the proxy with the least connections
        let mut min_connections = usize::MAX;
        let mut selected: Option<Arc<Proxy>> = None;

        for proxy in proxies.iter() {
            let connections = self.tracker.get(proxy.id as i64);
            if connections < min_connections {
                min_connections = connections;
                selected = Some(proxy.clone());
            }
        }

        selected.ok_or(RotaError::NoProxiesAvailable)
    }

    async fn refresh(&self, proxies: Vec<Proxy>) -> Result<()> {
        let mut guard = self.proxies.write();
        *guard = proxies.into_iter().map(Arc::new).collect();
        // Don't clear the tracker - we want to preserve connection counts
        Ok(())
    }

    fn available_count(&self) -> usize {
        self.proxies.read().len()
    }

    fn strategy_name(&self) -> &'static str {
        "least_connections"
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
    use crate::models::ProxyProtocol;

    fn create_test_proxy(id: i32, _name: &str) -> Proxy {
        Proxy {
            id,
            address: "127.0.0.1:8080".to_string(),
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
    async fn test_least_conn_empty() {
        let selector = LeastConnectionsSelector::new();
        let result = selector.select().await;
        assert!(matches!(result, Err(RotaError::NoProxiesAvailable)));
    }

    #[tokio::test]
    async fn test_least_conn_selects_lowest() {
        let selector = LeastConnectionsSelector::new();
        let proxies = vec![
            create_test_proxy(1, "proxy1"),
            create_test_proxy(2, "proxy2"),
            create_test_proxy(3, "proxy3"),
        ];
        selector.refresh(proxies).await.unwrap();

        // Initially all have 0 connections, so first should be selected
        let first = selector.select().await.unwrap();
        assert_eq!(first.id, 1);

        // Simulate connections
        selector.acquire(1);
        selector.acquire(1);
        selector.acquire(2);

        // Now proxy3 has the least (0), should be selected
        let selected = selector.select().await.unwrap();
        assert_eq!(selected.id, 3);

        // Add connection to proxy3
        selector.acquire(3);

        // Now proxy2 has least (1), should be selected
        let selected = selector.select().await.unwrap();
        assert_eq!(selected.id, 2);
    }

    #[tokio::test]
    async fn test_least_conn_release() {
        let selector = LeastConnectionsSelector::new();
        let proxies = vec![
            create_test_proxy(1, "proxy1"),
            create_test_proxy(2, "proxy2"),
        ];
        selector.refresh(proxies).await.unwrap();

        // Add connections to proxy1
        selector.acquire(1);
        selector.acquire(1);
        selector.acquire(1);

        // proxy2 should be selected (0 connections)
        assert_eq!(selector.select().await.unwrap().id, 2);

        // Release connections from proxy1
        selector.release(1);
        selector.release(1);
        selector.release(1);

        // Now proxy1 should be selected (0 connections, comes first)
        assert_eq!(selector.select().await.unwrap().id, 1);
    }
}
