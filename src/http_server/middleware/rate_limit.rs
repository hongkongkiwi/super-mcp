//! Rate limiting middleware using tower-governor

use axum::{
    body::Body,
    extract::{ConnectInfo, Request},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use governor::{
    clock::DefaultClock,
    middleware::NoOpMiddleware,
    state::InMemoryState,
    Quota, RateLimiter,
};
use std::net::SocketAddr;
use std::sync::Arc;

/// Rate limiter configuration
type GovernorRateLimiter = RateLimiter<String, InMemoryState, DefaultClock, NoOpMiddleware>;

pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    pub burst_size: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 100,
            burst_size: 10,
        }
    }
}

/// Rate limiter manager that supports per-IP and per-user limits
pub struct RateLimitManager {
    /// Global rate limiter for anonymous requests
    global_limiter: Arc<GovernorRateLimiter>,
    /// Per-user rate limiters
    user_limiters: Arc<dashmap::DashMap<String, Arc<GovernorRateLimiter>>>,
    config: RateLimitConfig,
}

impl RateLimitManager {
    pub fn new(config: RateLimitConfig) -> Self {
        let quota = Quota::per_minute(
            std::num::NonZeroU32::new(config.requests_per_minute).unwrap_or_else(|| std::num::NonZeroU32::new(100).unwrap())
        )
        .allow_burst(
            std::num::NonZeroU32::new(config.burst_size).unwrap_or_else(|| std::num::NonZeroU32::new(10).unwrap())
        );

        Self {
            global_limiter: Arc::new(RateLimiter::keyed(quota)),
            user_limiters: Arc::new(dashmap::DashMap::new()),
            config,
        }
    }

    /// Get or create a rate limiter for a specific user
    pub fn get_user_limiter(&self, user_id: &str) -> Arc<GovernorRateLimiter> {
        self.user_limiters
            .entry(user_id.to_string())
            .or_insert_with(|| {
                let quota = Quota::per_minute(
                    std::num::NonZeroU32::new(self.config.requests_per_minute).unwrap_or_else(|| std::num::NonZeroU32::new(100).unwrap())
                )
                .allow_burst(
                    std::num::NonZeroU32::new(self.config.burst_size).unwrap_or_else(|| std::num::NonZeroU32::new(10).unwrap())
                );
                Arc::new(RateLimiter::keyed(quota))
            })
            .clone()
    }

    /// Get the global rate limiter
    pub fn global_limiter(&self) -> Arc<GovernorRateLimiter> {
        self.global_limiter.clone()
    }

    /// Clean up stale user limiters (call periodically)
    pub fn cleanup_stale_limiters(&self, max_age: std::time::Duration) {
        // This is a placeholder - in production, you'd track last access time
        // and remove entries older than max_age
    }
}

impl Default for RateLimitManager {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}

/// Rate limiting middleware
pub async fn rate_limit_middleware(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    // For now, just use IP-based rate limiting
    // In a full implementation, we'd check for auth token and use user-based limiting
    
    // This is a simplified implementation
    // A full implementation would use the RateLimitManager
    
    next.run(request).await
}

/// Create a tower-governor layer for Axum
pub fn create_rate_limit_layer(
    config: &RateLimitConfig,
) -> tower_governor::GovernorLayer<String, InMemoryState, DefaultClock, NoOpMiddleware> {
    use governor::clock::DefaultClock;
    use governor::middleware::NoOpMiddleware;
    use governor::state::InMemoryState;
    use std::num::NonZeroU32;

    let quota = Quota::per_minute(
        NonZeroU32::new(config.requests_per_minute).unwrap_or_else(|| NonZeroU32::new(100).unwrap())
    )
    .allow_burst(
        NonZeroU32::new(config.burst_size).unwrap_or_else(|| NonZeroU32::new(10).unwrap())
    );

    let governor = GovernorConfigBuilder::default()
        .quota(quota)
        .key_extractor(tower_governor::key_extractor::SmartIpKeyExtractor)
        .use_headers()
        .finish()
        .expect("Failed to create governor config");

    tower_governor::GovernorLayer {
        config: Arc::new(governor),
    }
}

use tower_governor::governor::GovernorConfigBuilder;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_manager_creation() {
        let manager = RateLimitManager::new(RateLimitConfig {
            requests_per_minute: 60,
            burst_size: 5,
        });

        let limiter = manager.get_user_limiter("user1");
        assert!(!manager.user_limiters.is_empty());
    }

    #[test]
    fn test_rate_limit_config_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.requests_per_minute, 100);
        assert_eq!(config.burst_size, 10);
    }
}
