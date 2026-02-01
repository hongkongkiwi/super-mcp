//! Token cache with TTL for authentication
//!
//! Implements an LRU cache with TTL for validated tokens to reduce
//! redundant token validation overhead.

use crate::auth::provider::Session;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Cached session with metadata
#[derive(Debug, Clone)]
pub struct CachedSession {
    /// The validated session
    pub session: Session,
    /// When the session was cached
    pub cached_at: Instant,
    /// Cache TTL
    pub ttl: Duration,
    /// Number of times this cache entry was accessed
    pub access_count: u64,
}

impl CachedSession {
    /// Check if the cached session is expired
    pub fn is_expired(&self) -> bool {
        self.cached_at.elapsed() > self.ttl
    }

    /// Create a new cached session
    pub fn new(session: Session, ttl: Duration) -> Self {
        Self {
            session,
            cached_at: Instant::now(),
            ttl,
            access_count: 0,
        }
    }

    /// Record an access to this cache entry
    pub fn record_access(&mut self) {
        self.access_count += 1;
    }
}

/// Token cache configuration
#[derive(Debug, Clone)]
pub struct TokenCacheConfig {
    /// Default TTL for cached tokens
    pub default_ttl: Duration,
    /// Maximum cache size
    pub max_size: usize,
    /// Cleanup interval
    pub cleanup_interval: Duration,
}

impl Default for TokenCacheConfig {
    fn default() -> Self {
        Self {
            default_ttl: Duration::from_secs(300), // 5 minutes
            max_size: 10000,
            cleanup_interval: Duration::from_secs(60),
        }
    }
}

/// LRU cache with TTL for validated tokens
pub struct TokenCache {
    /// Cache storage: token hash -> cached session
    cache: DashMap<String, Arc<RwLock<CachedSession>>>,
    /// Configuration
    config: TokenCacheConfig,
}

impl TokenCache {
    /// Create a new token cache
    pub fn new(config: TokenCacheConfig) -> Self {
        let cache = Self {
            cache: DashMap::with_capacity(config.max_size),
            config,
        };

        // Start cleanup task
        cache.start_cleanup_task();

        cache
    }

    /// Start background cleanup task
    fn start_cleanup_task(&self) {
        let cache = self.cache.clone();
        let interval = self.config.cleanup_interval;

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            
            loop {
                ticker.tick().await;
                
                let before_count = cache.len();
                cache.retain(|_key, entry| {
                    // Try to get read lock and check expiry
                    if let Ok(entry) = entry.try_read() {
                        !entry.is_expired()
                    } else {
                        // If we can't get the lock, keep the entry
                        true
                    }
                });
                let after_count = cache.len();
                
                if before_count != after_count {
                    debug!(
                        "Token cache cleanup: removed {} expired entries, {} remaining",
                        before_count - after_count,
                        after_count
                    );
                }
            }
        });
    }

    /// Get a cached session if it exists and is not expired
    pub async fn get(&self, token: &str) -> Option<Session> {
        // Use a simple hash of the token as the key
        let key = self.hash_token(token);

        if let Some(entry) = self.cache.get(&key) {
            let mut session = entry.write().await;
            
            if !session.is_expired() {
                session.record_access();
                debug!("Token cache hit for key: {}", &key[..8]);
                return Some(session.session.clone());
            }
        }

        debug!("Token cache miss for key: {}", &key[..8]);
        None
    }

    /// Cache a validated session
    pub async fn put(&self, token: &str, session: Session) {
        let key = self.hash_token(token);
        
        // Check if cache is at capacity
        if self.cache.len() >= self.config.max_size {
            // Simple eviction: remove a random entry
            // In production, you'd want proper LRU eviction
            if let Some(entry) = self.cache.iter().next() {
                let key_to_remove = entry.key().clone();
                drop(entry);
                self.cache.remove(&key_to_remove);
            }
        }

        let cached = CachedSession::new(session, self.config.default_ttl);
        self.cache.insert(key, Arc::new(RwLock::new(cached)));
        
        debug!("Cached session for token key: {}", &self.hash_token(token)[..8]);
    }

    /// Invalidate a cached token
    pub fn invalidate(&self, token: &str) {
        let key = self.hash_token(token);
        self.cache.remove(&key);
        debug!("Invalidated token cache for key: {}", &key[..8]);
    }

    /// Invalidate all cached sessions for a user
    pub fn invalidate_user(&self, user_id: &str) {
        let before_count = self.cache.len();
        self.cache.retain(|_key, entry| {
            if let Ok(entry) = entry.try_read() {
                entry.session.user_id != user_id
            } else {
                true
            }
        });
        let after_count = self.cache.len();
        
        if before_count != after_count {
            info!(
                "Invalidated {} cached sessions for user: {}",
                before_count - after_count,
                user_id
            );
        }
    }

    /// Clear all cached sessions
    pub fn clear(&self) {
        self.cache.clear();
        info!("Cleared all token cache entries");
    }

    /// Get cache statistics
    pub fn stats(&self) -> TokenCacheStats {
        let total_entries = self.cache.len();
        let mut expired_entries = 0;

        for entry in self.cache.iter() {
            if let Ok(entry) = entry.try_read() {
                if entry.is_expired() {
                    expired_entries += 1;
                }
            }
        }

        TokenCacheStats {
            total_entries,
            expired_entries,
            active_entries: total_entries - expired_entries,
        }
    }

    /// Hash a token to create a cache key
    fn hash_token(&self, token: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        token.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

impl Default for TokenCache {
    fn default() -> Self {
        Self::new(TokenCacheConfig::default())
    }
}

/// Token cache statistics
#[derive(Debug, Clone)]
pub struct TokenCacheStats {
    pub total_entries: usize,
    pub expired_entries: usize,
    pub active_entries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_session() -> Session {
        Session {
            user_id: "user123".to_string(),
            token: "test-token".to_string(),
            scopes: vec!["read".to_string()],
            expires_at: None,
        }
    }

    #[test]
    fn test_token_cache_config_default() {
        let config = TokenCacheConfig::default();
        assert_eq!(config.default_ttl, Duration::from_secs(300));
        assert_eq!(config.max_size, 10000);
    }

    #[tokio::test]
    async fn test_cache_put_and_get() {
        let cache = TokenCache::new(TokenCacheConfig::default());
        let session = create_test_session();

        // Put in cache
        cache.put("token123", session.clone()).await;

        // Get from cache
        let cached = cache.get("token123").await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().user_id, "user123");
    }

    #[tokio::test]
    async fn test_cache_miss() {
        let cache = TokenCache::new(TokenCacheConfig::default());

        let cached = cache.get("nonexistent").await;
        assert!(cached.is_none());
    }

    #[tokio::test]
    async fn test_cache_expiry() {
        let config = TokenCacheConfig {
            default_ttl: Duration::from_millis(10),
            ..Default::default()
        };
        let cache = TokenCache::new(config);
        let session = create_test_session();

        cache.put("token123", session).await;

        // Wait for expiry
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Should be expired
        let cached = cache.get("token123").await;
        assert!(cached.is_none());
    }

    #[tokio::test]
    async fn test_cache_invalidate() {
        let cache = TokenCache::new(TokenCacheConfig::default());
        let session = create_test_session();

        cache.put("token123", session).await;
        assert!(cache.get("token123").await.is_some());

        cache.invalidate("token123");
        assert!(cache.get("token123").await.is_none());
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let cache = TokenCache::new(TokenCacheConfig::default());
        let session = create_test_session();

        cache.put("token1", session.clone()).await;
        cache.put("token2", session).await;

        let stats = cache.stats();
        assert_eq!(stats.total_entries, 2);
    }
}
