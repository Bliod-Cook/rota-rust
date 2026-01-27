//! Proxy rotation strategies
//!
//! This module provides various strategies for selecting proxies from the pool.

mod least_conn;
mod dynamic;
mod random;
mod round_robin;
mod time_based;

pub use dynamic::DynamicProxySelector;
pub use least_conn::LeastConnectionsSelector;
pub use random::RandomSelector;
pub use round_robin::RoundRobinSelector;
pub use time_based::TimeBasedSelector;

use async_trait::async_trait;
use std::sync::Arc;

use crate::error::Result;
use crate::models::Proxy;

/// Strategy types for proxy rotation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RotationStrategy {
    #[default]
    Random,
    RoundRobin,
    LeastConnections,
    TimeBased,
}

impl RotationStrategy {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "round_robin" | "roundrobin" | "round-robin" => Self::RoundRobin,
            "least_connections" | "leastconnections" | "least-connections" | "least_conn" => {
                Self::LeastConnections
            }
            "time_based" | "timebased" | "time-based" => Self::TimeBased,
            _ => Self::Random,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Random => "random",
            Self::RoundRobin => "round_robin",
            Self::LeastConnections => "least_connections",
            Self::TimeBased => "time_based",
        }
    }
}

/// Trait for proxy selection strategies
///
/// Implementations of this trait provide different algorithms for
/// selecting proxies from the available pool.
#[async_trait]
pub trait ProxySelector: Send + Sync {
    /// Select a proxy from the available pool
    ///
    /// Returns an error if no proxies are available
    async fn select(&self) -> Result<Arc<Proxy>>;

    /// Refresh the internal proxy list
    ///
    /// Should be called when proxies are added/removed/updated
    async fn refresh(&self, proxies: Vec<Proxy>) -> Result<()>;

    /// Get the number of available proxies
    fn available_count(&self) -> usize;

    /// Get the strategy name
    fn strategy_name(&self) -> &'static str;

    /// Mark a proxy as being used (for connection tracking)
    fn acquire(&self, proxy_id: i64);

    /// Mark a proxy as no longer being used
    fn release(&self, proxy_id: i64);
}

/// Connection tracker for proxies
///
/// Used by least-connections strategy to track active connections
#[derive(Debug, Default)]
pub struct ConnectionTracker {
    connections: dashmap::DashMap<i64, usize>,
}

impl ConnectionTracker {
    pub fn new() -> Self {
        Self {
            connections: dashmap::DashMap::new(),
        }
    }

    pub fn acquire(&self, proxy_id: i64) {
        self.connections
            .entry(proxy_id)
            .and_modify(|c| *c += 1)
            .or_insert(1);
    }

    pub fn release(&self, proxy_id: i64) {
        self.connections.entry(proxy_id).and_modify(|c| {
            if *c > 0 {
                *c -= 1;
            }
        });
    }

    pub fn get(&self, proxy_id: i64) -> usize {
        self.connections.get(&proxy_id).map(|v| *v).unwrap_or(0)
    }

    pub fn clear(&self) {
        self.connections.clear();
    }
}

/// Create a proxy selector based on the strategy type
pub fn create_selector(strategy: RotationStrategy) -> Box<dyn ProxySelector> {
    match strategy {
        RotationStrategy::Random => Box::new(RandomSelector::new()),
        RotationStrategy::RoundRobin => Box::new(RoundRobinSelector::new()),
        RotationStrategy::LeastConnections => Box::new(LeastConnectionsSelector::new()),
        RotationStrategy::TimeBased => Box::new(TimeBasedSelector::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rotation_strategy_from_str() {
        assert_eq!(
            RotationStrategy::from_str("random"),
            RotationStrategy::Random
        );
        assert_eq!(
            RotationStrategy::from_str("round-robin"),
            RotationStrategy::RoundRobin
        );
        assert_eq!(
            RotationStrategy::from_str("least_conn"),
            RotationStrategy::LeastConnections
        );
        assert_eq!(
            RotationStrategy::from_str("timebased"),
            RotationStrategy::TimeBased
        );
        assert_eq!(
            RotationStrategy::from_str("unknown"),
            RotationStrategy::Random
        );
    }

    #[test]
    fn test_rotation_strategy_as_str() {
        assert_eq!(RotationStrategy::Random.as_str(), "random");
        assert_eq!(RotationStrategy::RoundRobin.as_str(), "round_robin");
        assert_eq!(
            RotationStrategy::LeastConnections.as_str(),
            "least_connections"
        );
        assert_eq!(RotationStrategy::TimeBased.as_str(), "time_based");
    }

    #[test]
    fn test_create_selector_strategy_name() {
        assert_eq!(
            create_selector(RotationStrategy::Random).strategy_name(),
            "random"
        );
        assert_eq!(
            create_selector(RotationStrategy::RoundRobin).strategy_name(),
            "round_robin"
        );
        assert_eq!(
            create_selector(RotationStrategy::LeastConnections).strategy_name(),
            "least_connections"
        );
        assert_eq!(
            create_selector(RotationStrategy::TimeBased).strategy_name(),
            "time_based"
        );
    }

    #[test]
    fn test_connection_tracker_counts() {
        let tracker = ConnectionTracker::new();

        assert_eq!(tracker.get(1), 0);
        tracker.acquire(1);
        tracker.acquire(1);
        assert_eq!(tracker.get(1), 2);

        tracker.release(1);
        assert_eq!(tracker.get(1), 1);

        tracker.release(1);
        tracker.release(1);
        assert_eq!(tracker.get(1), 0);

        tracker.acquire(1);
        tracker.clear();
        assert_eq!(tracker.get(1), 0);
    }
}
