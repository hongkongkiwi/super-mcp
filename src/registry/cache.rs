//! Local caching for registry data
use crate::registry::types::{RegistryEntry, RegistryConfig};
use crate::utils::errors::{McpError, McpResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use tracing::{debug, info};

/// Cached registry data
#[derive(Debug, Serialize, Deserialize)]
struct CachedData {
    entries: HashMap<String, RegistryEntry>,
    last_updated: SystemTime,
}

/// Registry cache manager
pub struct RegistryCache {
    cache_dir: PathBuf,
    ttl: Duration,
}

impl RegistryCache {
    pub fn new(config: &RegistryConfig) -> Self {
        Self {
            cache_dir: config.cache_dir.clone(),
            ttl: Duration::from_secs(config.cache_ttl_hours * 3600),
        }
    }

    /// Ensure cache directory exists
    async fn ensure_cache_dir(&self) -> McpResult<()> {
        if !self.cache_dir.exists() {
            tokio::fs::create_dir_all(&self.cache_dir)
                .await
                .map_err(|e| McpError::InternalError(format!("Failed to create cache dir: {}", e)))?;
        }
        Ok(())
    }

    fn cache_file_path(&self) -> PathBuf {
        self.cache_dir.join("registry.json")
    }

    /// Load cached entries if they exist and are not expired
    pub async fn load(&self) -> McpResult<Option<HashMap<String, RegistryEntry>>> {
        self.ensure_cache_dir().await?;

        let cache_file = self.cache_file_path();
        if !cache_file.exists() {
            return Ok(None);
        }

        let content = tokio::fs::read_to_string(&cache_file)
            .await
            .map_err(|e| McpError::InternalError(format!("Failed to read cache: {}", e)))?;

        let cached: CachedData = serde_json::from_str(&content)
            .map_err(|e| McpError::InternalError(format!("Failed to parse cache: {}", e)))?;

        // Check if cache is expired
        let now = SystemTime::now();
        let age = now.duration_since(cached.last_updated).unwrap_or(Duration::MAX);

        if age > self.ttl {
            debug!("Registry cache expired");
            return Ok(None);
        }

        info!("Loaded {} entries from registry cache", cached.entries.len());
        Ok(Some(cached.entries))
    }

    /// Save entries to cache
    pub async fn save(&self, entries: &HashMap<String, RegistryEntry>) -> McpResult<()> {
        self.ensure_cache_dir().await?;

        let cached = CachedData {
            entries: entries.clone(),
            last_updated: SystemTime::now(),
        };

        let content = serde_json::to_string_pretty(&cached)
            .map_err(|e| McpError::InternalError(format!("Failed to serialize cache: {}", e)))?;

        let cache_file = self.cache_file_path();
        tokio::fs::write(&cache_file, content)
            .await
            .map_err(|e| McpError::InternalError(format!("Failed to write cache: {}", e)))?;

        info!("Saved {} entries to registry cache", entries.len());
        Ok(())
    }

    /// Clear the cache
    pub async fn clear(&self) -> McpResult<()> {
        let cache_file = self.cache_file_path();
        if cache_file.exists() {
            tokio::fs::remove_file(&cache_file)
                .await
                .map_err(|e| McpError::InternalError(format!("Failed to clear cache: {}", e)))?;
        }
        Ok(())
    }
}
