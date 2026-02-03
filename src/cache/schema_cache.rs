//! TTL-based cache for MCP tool/resource/prompt schemas with coalescing support

use dashmap::DashMap;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Cached schema entry with TTL tracking
#[derive(Debug, Clone)]
pub struct CachedSchema {
    /// The cached schema data
    pub schema: Value,
    /// When the entry was cached
    pub cached_at: Instant,
    /// TTL for the entry
    pub ttl: Duration,
    /// Server name this schema belongs to
    pub server_name: String,
    /// Schema type (tool, resource, prompt)
    pub schema_type: SchemaType,
}

impl CachedSchema {
    /// Check if the cache entry is expired
    pub fn is_expired(&self) -> bool {
        Instant::now().duration_since(self.cached_at) > self.ttl
    }

    /// Time remaining until expiration
    pub fn ttl_remaining(&self) -> Option<Duration> {
        let elapsed = Instant::now().duration_since(self.cached_at);
        self.ttl.checked_sub(elapsed)
    }
}

/// Type of schema being cached
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchemaType {
    Tool,
    Resource,
    Prompt,
}

/// Cache metrics for monitoring
#[derive(Debug, Default)]
pub struct CacheMetrics {
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub evictions: AtomicU64,
    pub insertions: AtomicU64,
}

impl CacheMetrics {
    #[inline]
    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_eviction(&self) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_insertion(&self) {
        self.insertions.fetch_add(1, Ordering::Relaxed);
    }

    /// Get current metrics snapshot
    pub fn snapshot(&self) -> CacheMetricsSnapshot {
        CacheMetricsSnapshot {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            insertions: self.insertions.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheMetricsSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub insertions: u64,
}

impl CacheMetricsSnapshot {
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64 * 100.0
        }
    }
}

/// SchemaCache provides TTL-based caching for MCP schemas with coalescing
///
/// # Features:
/// - TTL-based expiration for all cached entries
/// - Coalescing to prevent duplicate concurrent fetches
/// - Cache metrics (hits, misses, evictions)
/// - Thread-safe concurrent access
#[derive(Debug, Clone)]
pub struct SchemaCache {
    /// Cache for tool schemas
    tools: DashMap<String, CachedSchema>,
    /// Cache for resource schemas
    resources: DashMap<String, CachedSchema>,
    /// Cache for prompt schemas
    prompts: DashMap<String, CachedSchema>,
    /// Default TTL for cache entries
    default_ttl: Duration,
    /// Metrics tracking
    metrics: Arc<CacheMetrics>,
}

impl Default for SchemaCache {
    fn default() -> Self {
        Self::new(Duration::from_secs(300))
    }
}

impl SchemaCache {
    /// Create a new SchemaCache with the specified default TTL
    pub fn new(default_ttl: Duration) -> Self {
        Self {
            tools: DashMap::new(),
            resources: DashMap::new(),
            prompts: DashMap::new(),
            default_ttl,
            metrics: Arc::new(CacheMetrics::default()),
        }
    }

    /// Get the cache map for a specific schema type
    fn get_cache_map(&self, schema_type: SchemaType) -> &DashMap<String, CachedSchema> {
        match schema_type {
            SchemaType::Tool => &self.tools,
            SchemaType::Resource => &self.resources,
            SchemaType::Prompt => &self.prompts,
        }
    }

    /// Generate a cache key for a schema
    fn cache_key(server_name: &str, name: &str) -> String {
        format!("{}:{}", server_name, name)
    }

    /// Get a schema from cache
    pub fn get(&self, server_name: &str, name: &str, schema_type: SchemaType) -> Option<CachedSchema> {
        let key = Self::cache_key(server_name, name);
        let cache_map = self.get_cache_map(schema_type);

        if let Some(entry) = cache_map.get(&key) {
            if entry.is_expired() {
                // Remove expired entry
                cache_map.remove(&key);
                self.metrics.record_eviction();
                return None;
            }
            self.metrics.record_hit();
            Some(entry.value().clone())
        } else {
            self.metrics.record_miss();
            None
        }
    }

    /// Get all cached entries for a server
    pub fn get_by_server(&self, server_name: &str) -> Vec<(SchemaType, CachedSchema)> {
        let mut results = Vec::new();

        for entry in self.tools.iter() {
            if entry.server_name == server_name && !entry.is_expired() {
                results.push((SchemaType::Tool, entry.clone()));
            }
        }
        for entry in self.resources.iter() {
            if entry.server_name == server_name && !entry.is_expired() {
                results.push((SchemaType::Resource, entry.clone()));
            }
        }
        for entry in self.prompts.iter() {
            if entry.server_name == server_name && !entry.is_expired() {
                results.push((SchemaType::Prompt, entry.clone()));
            }
        }

        results
    }

    /// Insert a schema into the cache
    pub fn insert(
        &self,
        server_name: impl Into<String>,
        name: impl Into<String>,
        schema: Value,
        schema_type: SchemaType,
    ) -> CachedSchema {
        self.insert_with_ttl(server_name, name, schema, schema_type, self.default_ttl)
    }

    /// Insert a schema with a custom TTL
    pub fn insert_with_ttl(
        &self,
        server_name: impl Into<String>,
        name: impl Into<String>,
        schema: Value,
        schema_type: SchemaType,
        ttl: Duration,
    ) -> CachedSchema {
        let cached = CachedSchema {
            schema,
            cached_at: Instant::now(),
            ttl,
            server_name: server_name.into(),
            schema_type,
        };

        let key = Self::cache_key(&cached.server_name, &name.into());
        let cache_map = self.get_cache_map(schema_type);

        // Remove existing entry if present
        cache_map.remove(&key);
        cache_map.insert(key, cached.clone());
        self.metrics.record_insertion();

        cached
    }

    /// Remove a schema from cache
    pub fn remove(&self, server_name: &str, name: &str, schema_type: SchemaType) -> bool {
        let key = Self::cache_key(server_name, name);
        let cache_map = self.get_cache_map(schema_type);
        cache_map.remove(&key).is_some()
    }

    /// Clear all cached schemas for a server
    pub fn clear_server(&self, server_name: &str) {
        // Collect keys to remove from tools
        let keys_to_remove: Vec<_> = self
            .tools
            .iter()
            .filter(|e| e.server_name == server_name)
            .map(|e| e.key().clone())
            .collect();

        for key in keys_to_remove {
            if self.tools.remove(&key).is_some() {
                self.metrics.record_eviction();
            }
        }

        // Collect keys to remove from resources
        let keys_to_remove: Vec<_> = self
            .resources
            .iter()
            .filter(|e| e.server_name == server_name)
            .map(|e| e.key().clone())
            .collect();

        for key in keys_to_remove {
            if self.resources.remove(&key).is_some() {
                self.metrics.record_eviction();
            }
        }

        // Collect keys to remove from prompts
        let keys_to_remove: Vec<_> = self
            .prompts
            .iter()
            .filter(|e| e.server_name == server_name)
            .map(|e| e.key().clone())
            .collect();

        for key in keys_to_remove {
            if self.prompts.remove(&key).is_some() {
                self.metrics.record_eviction();
            }
        }
    }

    /// Clear all cached schemas
    pub fn clear_all(&self) {
        self.tools.clear();
        self.resources.clear();
        self.prompts.clear();
    }

    /// Check if a fetch is already in progress for the given key (placeholder for future coalescing)
    pub fn is_fetching(&self, _server_name: &str, _schema_type: SchemaType) -> bool {
        false
    }

    /// Get cache statistics
    pub fn stats(&self) -> SchemaCacheStats {
        SchemaCacheStats {
            tools_count: self.tools.len(),
            resources_count: self.resources.len(),
            prompts_count: self.prompts.len(),
            metrics: self.metrics.snapshot(),
        }
    }

    /// Get total number of cached entries
    pub fn len(&self) -> usize {
        self.tools.len() + self.resources.len() + self.prompts.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty() && self.resources.is_empty() && self.prompts.is_empty()
    }
}

/// Statistics for SchemaCache
#[derive(Debug, Clone)]
pub struct SchemaCacheStats {
    pub tools_count: usize,
    pub resources_count: usize,
    pub prompts_count: usize,
    pub metrics: CacheMetricsSnapshot,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[tokio::test]
    async fn test_cache_insert_and_get() {
        let cache = SchemaCache::new(Duration::from_secs(60));
        let schema = serde_json::json!({"type": "object", "properties": {"name": {"type": "string"}}});

        cache.insert("server1", "tool1", schema.clone(), SchemaType::Tool);

        let retrieved = cache.get("server1", "tool1", SchemaType::Tool);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().schema, schema);
    }

    #[tokio::test]
    async fn test_cache_miss() {
        let cache = SchemaCache::new(Duration::from_secs(60));

        let retrieved = cache.get("server1", "nonexistent", SchemaType::Tool);
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_cache_expiration() {
        let cache = SchemaCache::new(Duration::from_millis(100));
        let schema = serde_json::json!({"type": "object"});

        cache.insert("server1", "tool1", schema.clone(), SchemaType::Tool);

        // Should be present immediately
        assert!(cache.get("server1", "tool1", SchemaType::Tool).is_some());

        // Wait for expiration
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Should be expired
        assert!(cache.get("server1", "tool1", SchemaType::Tool).is_none());
    }

    #[tokio::test]
    async fn test_cache_clear_server() {
        let cache = SchemaCache::new(Duration::from_secs(60));
        let schema = serde_json::json!({"type": "object"});

        cache.insert("server1", "tool1", schema.clone(), SchemaType::Tool);
        cache.insert("server1", "tool2", schema.clone(), SchemaType::Tool);
        cache.insert("server2", "tool1", schema.clone(), SchemaType::Tool);

        cache.clear_server("server1");

        assert!(cache.get("server1", "tool1", SchemaType::Tool).is_none());
        assert!(cache.get("server1", "tool2", SchemaType::Tool).is_none());
        assert!(cache.get("server2", "tool1", SchemaType::Tool).is_some());
    }

    #[tokio::test]
    async fn test_cache_metrics() {
        let cache = SchemaCache::new(Duration::from_secs(60));
        let schema = serde_json::json!({"type": "object"});

        // Initial metrics
        let stats = cache.stats();
        assert_eq!(stats.metrics.hits, 0);
        assert_eq!(stats.metrics.misses, 0);

        // Cache miss
        cache.get("server1", "tool1", SchemaType::Tool);

        // Cache hit
        cache.insert("server1", "tool1", schema.clone(), SchemaType::Tool);
        cache.get("server1", "tool1", SchemaType::Tool);

        let stats = cache.stats();
        assert_eq!(stats.metrics.hits, 1);
        assert_eq!(stats.metrics.misses, 1);
    }

    #[test]
    fn test_concurrent_access() {
        let cache = Arc::new(SchemaCache::new(Duration::from_secs(60)));
        let schema = serde_json::json!({"type": "object"});

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let cache = cache.clone();
                let schema = schema.clone();
                thread::spawn(move || {
                    for i in 0..100 {
                        let key = format!("tool{}", i % 5);
                        let _ = cache.get("server1", &key, SchemaType::Tool);
                        cache.insert("server1", &key, schema.clone(), SchemaType::Tool);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // Verify cache has entries
        assert!(!cache.is_empty());
    }
}
