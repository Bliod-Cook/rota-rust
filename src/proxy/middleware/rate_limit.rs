//! Rate limiting middleware for the proxy server
//!
//! Uses the governor crate for efficient, lock-free rate limiting.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use arc_swap::ArcSwap;
use dashmap::DashMap;
use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter as GovRateLimiter};
use tracing::{debug, warn};

use crate::error::{Result, RotaError};
use crate::models::RateLimitSettings;

#[derive(Debug)]
struct ClientLimiter {
    limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    last_seen_ms: std::sync::atomic::AtomicU64,
}

impl ClientLimiter {
    fn new(
        limiter: Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
        now_ms: u64,
    ) -> Self {
        Self {
            limiter,
            last_seen_ms: std::sync::atomic::AtomicU64::new(now_ms),
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone, Copy)]
struct RateLimiterConfig {
    enabled: bool,
    quota: Quota,
    max_idle: Duration,
}

/// Rate limiter for proxy requests
pub struct RateLimiter {
    config: Arc<ArcSwap<RateLimiterConfig>>,
    /// Rate limiters per client IP
    limiters: Arc<DashMap<String, ClientLimiter>>,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(enabled: bool, requests_per_second: u32, burst_size: u32) -> Self {
        let requests_per_second = NonZeroU32::new(requests_per_second.max(1)).unwrap();
        let burst_size = NonZeroU32::new(burst_size.max(1)).unwrap();

        Self {
            config: Arc::new(ArcSwap::from_pointee(RateLimiterConfig {
                enabled,
                quota: Quota::per_second(requests_per_second).allow_burst(burst_size),
                max_idle: Duration::from_secs(10 * 60),
            })),
            limiters: Arc::new(DashMap::new()),
        }
    }

    /// Create a disabled rate limiter
    pub fn disabled() -> Self {
        Self::new(false, 100, 100)
    }

    /// Check if rate limiting is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.load().enabled
    }

    pub fn apply_settings(&self, settings: &RateLimitSettings) {
        let enabled = settings.enabled;

        let interval_secs = settings.interval.max(1) as u64;
        let max_requests = settings.max_requests.max(1) as u32;
        let max_burst = NonZeroU32::new(max_requests).unwrap_or_else(|| NonZeroU32::new(1).unwrap());

        let mut replenish_1_per = Duration::from_secs(interval_secs) / max_burst.get();
        if replenish_1_per.is_zero() {
            replenish_1_per = Duration::from_nanos(1);
        }

        // Quota: allow `max_requests` over `interval` seconds, with burst == max_requests.
        let quota = Quota::with_period(replenish_1_per)
            .unwrap_or_else(|| Quota::per_second(NonZeroU32::new(1).unwrap()))
            .allow_burst(max_burst);

        let max_idle = self.config.load().max_idle;

        // Clear per-client state so new quota applies immediately.
        self.limiters.clear();
        self.config.store(Arc::new(RateLimiterConfig {
            enabled,
            quota,
            max_idle,
        }));
    }

    /// Check if a request from the given client IP is allowed
    pub fn check(&self, client_ip: &str) -> Result<()> {
        if !self.config.load().enabled {
            return Ok(());
        }

        let limiter = self.get_or_create_limiter(client_ip);

        match limiter.check() {
            Ok(_) => {
                debug!("Rate limit check passed for {}", client_ip);
                Ok(())
            }
            Err(_) => {
                warn!("Rate limit exceeded for {}", client_ip);
                Err(RotaError::RateLimitExceeded {
                    client_ip: client_ip.to_string(),
                })
            }
        }
    }

    /// Get or create a rate limiter for the given client IP
    fn get_or_create_limiter(
        &self,
        client_ip: &str,
    ) -> Arc<GovRateLimiter<NotKeyed, InMemoryState, DefaultClock>> {
        let now_ms = now_ms();
        let quota = self.config.load().quota;

        let entry = self.limiters.entry(client_ip.to_string()).or_insert_with(|| {
            ClientLimiter::new(Arc::new(GovRateLimiter::direct(quota)), now_ms)
        });

        entry
            .last_seen_ms
            .store(now_ms, std::sync::atomic::Ordering::Relaxed);

        entry.limiter.clone()
    }

    /// Clean up old rate limiters (call periodically)
    pub fn cleanup(&self) {
        let now_ms = now_ms();
        let max_idle_ms = self.config.load().max_idle.as_millis() as u64;

        self.limiters.retain(|_, entry| {
            let last_seen = entry
                .last_seen_ms
                .load(std::sync::atomic::Ordering::Relaxed);
            now_ms.saturating_sub(last_seen) <= max_idle_ms
        });
    }

    /// Get the number of tracked clients
    pub fn client_count(&self) -> usize {
        self.limiters.len()
    }
}

impl Clone for RateLimiter {
    fn clone(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
            limiters: Arc::clone(&self.limiters),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_disabled() {
        let limiter = RateLimiter::disabled();
        // Should always pass when disabled
        for _ in 0..100 {
            assert!(limiter.check("192.168.1.1").is_ok());
        }
    }

    #[test]
    fn test_rate_limiter_allows_within_limit() {
        let limiter = RateLimiter::new(true, 10, 10);

        // Should allow up to burst size
        for i in 0..10 {
            assert!(
                limiter.check("192.168.1.1").is_ok(),
                "Failed on request {}",
                i
            );
        }
    }

    #[test]
    fn test_rate_limiter_blocks_over_limit() {
        let limiter = RateLimiter::new(true, 1, 2);

        // First two should pass (burst)
        assert!(limiter.check("192.168.1.1").is_ok());
        assert!(limiter.check("192.168.1.1").is_ok());

        // Third should fail
        assert!(matches!(
            limiter.check("192.168.1.1"),
            Err(RotaError::RateLimitExceeded { .. })
        ));
    }

    #[test]
    fn test_rate_limiter_per_ip() {
        let limiter = RateLimiter::new(true, 1, 1);

        // Different IPs should have separate limits
        assert!(limiter.check("192.168.1.1").is_ok());
        assert!(limiter.check("192.168.1.2").is_ok());
        assert!(limiter.check("192.168.1.3").is_ok());

        // Same IP should be limited
        assert!(matches!(
            limiter.check("192.168.1.1"),
            Err(RotaError::RateLimitExceeded { .. })
        ));
    }

    #[test]
    fn test_client_count() {
        let limiter = RateLimiter::new(true, 10, 10);

        limiter.check("192.168.1.1").ok();
        limiter.check("192.168.1.2").ok();
        limiter.check("192.168.1.3").ok();

        assert_eq!(limiter.client_count(), 3);
    }
}
