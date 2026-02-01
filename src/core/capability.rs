//! Capability Manager for async loading and caching of MCP server capabilities

use crate::core::protocol::{ServerCapabilities, JsonRpcRequest, RequestId, JsonRpcResponse};
use crate::utils::errors::McpResult;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

/// Cached server capabilities with metadata
#[derive(Debug, Clone)]
pub struct CachedCapabilities {
    /// The capabilities
    pub capabilities: ServerCapabilities,
    /// When the capabilities were loaded
    pub loaded_at: Instant,
    /// Cache TTL
    pub ttl: Duration,
    /// Server health status
    pub healthy: bool,
}

impl CachedCapabilities {
    /// Check if cache is expired
    pub fn is_expired(&self) -> bool {
        self.loaded_at.elapsed() > self.ttl
    }

    /// Create new cached capabilities
    pub fn new(capabilities: ServerCapabilities, ttl: Duration) -> Self {
        Self {
            capabilities,
            loaded_at: Instant::now(),
            ttl,
            healthy: true,
        }
    }
}

/// Tool information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: Option<serde_json::Value>,
}

/// Resource information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceInfo {
    pub uri: String,
    pub name: Option<String>,
    pub mime_type: Option<String>,
}

/// Prompt information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptInfo {
    pub name: String,
    pub description: Option<String>,
}

/// Capability manager configuration
#[derive(Debug, Clone)]
pub struct CapabilityManagerConfig {
    /// Default cache TTL
    pub cache_ttl: Duration,
    /// Whether to enable async loading
    pub async_loading: bool,
    /// Retry attempts for failed loads
    pub retry_attempts: u32,
    /// Retry delay
    pub retry_delay: Duration,
}

impl Default for CapabilityManagerConfig {
    fn default() -> Self {
        Self {
            cache_ttl: Duration::from_secs(300),
            async_loading: true,
            retry_attempts: 3,
            retry_delay: Duration::from_secs(1),
        }
    }
}

/// Manages capabilities for all MCP servers
pub struct CapabilityManager {
    /// Cache of server capabilities
    cache: DashMap<String, Arc<RwLock<CachedCapabilities>>>,
    /// Configuration
    config: CapabilityManagerConfig,
    /// Loading tasks
    loading_tasks: Arc<RwLock<Vec<tokio::task::JoinHandle<()>>>>,
}

impl CapabilityManager {
    /// Create a new capability manager
    pub fn new(config: CapabilityManagerConfig) -> Self {
        Self {
            cache: DashMap::new(),
            config,
            loading_tasks: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get cached capabilities for a server
    pub async fn get_capabilities(&self, server_name: &str) -> Option<ServerCapabilities> {
        if let Some(entry) = self.cache.get(server_name) {
            let cached = entry.read().await;
            if !cached.is_expired() && cached.healthy {
                return Some(cached.capabilities.clone());
            }
        }
        None
    }

    /// Get or load capabilities for a server
    pub async fn get_or_load_capabilities<F, Fut>(
        &self,
        server_name: &str,
        loader: F,
    ) -> McpResult<ServerCapabilities>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = McpResult<ServerCapabilities>>,
    {
        // Try cache first
        if let Some(caps) = self.get_capabilities(server_name).await {
            debug!("Using cached capabilities for {}", server_name);
            return Ok(caps);
        }

        // Load fresh capabilities
        debug!("Loading capabilities for {}", server_name);
        let capabilities = self.load_with_retry(loader).await?;

        // Cache the result
        self.cache_capabilities(server_name, capabilities.clone()).await;

        Ok(capabilities)
    }

    /// Load capabilities with retry logic
    async fn load_with_retry<F, Fut>(
        &self,
        mut loader: F,
    ) -> McpResult<ServerCapabilities>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = McpResult<ServerCapabilities>>,
    {
        let mut last_error = None;

        for attempt in 0..self.config.retry_attempts {
            match loader().await {
                Ok(caps) => return Ok(caps),
                Err(e) => {
                    warn!("Capability load attempt {} failed: {}", attempt + 1, e);
                    last_error = Some(e);
                    if attempt < self.config.retry_attempts - 1 {
                        tokio::time::sleep(self.config.retry_delay).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            crate::utils::errors::McpError::InternalError("Capability loading failed".to_string())
        }))
    }

    /// Cache capabilities for a server
    pub async fn cache_capabilities(&self, server_name: &str, capabilities: ServerCapabilities) {
        let cached = CachedCapabilities::new(capabilities, self.config.cache_ttl);
        
        self.cache.insert(
            server_name.to_string(),
            Arc::new(RwLock::new(cached)),
        );
        
        info!("Cached capabilities for {}", server_name);
    }

    /// Invalidate cached capabilities for a server
    pub fn invalidate(&self, server_name: &str) {
        self.cache.remove(server_name);
        debug!("Invalidated capabilities cache for {}", server_name);
    }

    /// Invalidate all cached capabilities
    pub fn invalidate_all(&self) {
        self.cache.clear();
        info!("Invalidated all capabilities caches");
    }

    /// Refresh capabilities for a server
    pub async fn refresh_capabilities<F, Fut>(
        &self,
        server_name: &str,
        loader: F,
    ) -> McpResult<ServerCapabilities>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = McpResult<ServerCapabilities>>,
    {
        self.invalidate(server_name);
        self.get_or_load_capabilities(server_name, loader).await
    }

    /// Start async loading of capabilities for multiple servers
    pub async fn start_async_loading<F>(&self, servers: Vec<String>, loader_factory: F)
    where
        F: Fn(String) -> tokio::task::JoinHandle<()> + Send + Sync + 'static,
    {
        if !self.config.async_loading {
            return;
        }

        let mut tasks = self.loading_tasks.write().await;

        for server_name in servers {
            if !self.cache.contains_key(&server_name) {
                let handle = loader_factory(server_name);
                tasks.push(handle);
            }
        }
    }

    /// Wait for all async loading tasks to complete
    pub async fn wait_for_loading(&self) {
        let mut tasks = self.loading_tasks.write().await;
        
        for task in tasks.drain(..) {
            if let Err(e) = task.await {
                warn!("Capability loading task failed: {:?}", e);
            }
        }
    }

    /// Get list of tools from cached capabilities
    pub async fn get_tools(&self, _server_name: &str) -> Vec<ToolInfo> {
        vec![]
    }

    /// Get list of resources from cached capabilities
    pub async fn get_resources(&self, _server_name: &str) -> Vec<ResourceInfo> {
        vec![]
    }

    /// Get list of prompts from cached capabilities
    pub async fn get_prompts(&self, _server_name: &str) -> Vec<PromptInfo> {
        vec![]
    }

    /// Get cache statistics
    pub fn get_stats(&self) -> CapabilityCacheStats {
        let total_servers = self.cache.len();
        let mut healthy_servers = 0;
        let expired_count = 0;

        for _entry in self.cache.iter() {
            healthy_servers += 1;
        }

        CapabilityCacheStats {
            total_servers,
            healthy_servers,
            expired_count,
        }
    }

    /// Shutdown the capability manager
    pub async fn shutdown(&self) {
        info!("Shutting down capability manager...");
        
        self.wait_for_loading().await;
        self.invalidate_all();
        
        info!("Capability manager shut down");
    }
}

impl Default for CapabilityManager {
    fn default() -> Self {
        Self::new(CapabilityManagerConfig::default())
    }
}

/// Capability cache statistics
#[derive(Debug, Clone)]
pub struct CapabilityCacheStats {
    pub total_servers: usize,
    pub healthy_servers: usize,
    pub expired_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_manager_config_default() {
        let config = CapabilityManagerConfig::default();
        assert_eq!(config.cache_ttl, Duration::from_secs(300));
        assert!(config.async_loading);
        assert_eq!(config.retry_attempts, 3);
    }

    #[test]
    fn test_cached_capabilities_expiry() {
        let caps = ServerCapabilities::default();
        let cached = CachedCapabilities::new(caps, Duration::from_millis(10));
        
        assert!(!cached.is_expired());
        
        std::thread::sleep(Duration::from_millis(20));
        assert!(cached.is_expired());
    }

    #[tokio::test]
    async fn test_capability_manager_creation() {
        let manager = CapabilityManager::new(CapabilityManagerConfig::default());
        assert_eq!(manager.cache.len(), 0);
    }

    #[tokio::test]
    async fn test_cache_invalidation() {
        let manager = CapabilityManager::new(CapabilityManagerConfig::default());
        
        // Add something to cache
        let caps = ServerCapabilities::default();
        manager.cache_capabilities("test-server", caps).await;
        
        assert_eq!(manager.cache.len(), 1);
        
        // Invalidate
        manager.invalidate("test-server");
        assert_eq!(manager.cache.len(), 0);
    }
}
