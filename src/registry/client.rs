//! Registry HTTP client for MCP server discovery
use crate::registry::cache::RegistryCache;
use crate::registry::types::{RegistryConfig, RegistryEntry, SearchResults};
use crate::utils::errors::{McpError, McpResult};
use reqwest::Client;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Registry client for searching and installing MCP servers
pub struct RegistryClient {
    client: Client,
    config: RegistryConfig,
    cache: RegistryCache,
}

impl RegistryClient {
    pub fn new(config: RegistryConfig) -> Self {
        let cache = RegistryCache::new(&config);

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            config,
            cache,
        }
    }

    /// Search for MCP servers in the registry
    pub async fn search(&self, query: &str) -> McpResult<SearchResults> {
        info!("Searching registry for: {}", query);

        // First try to load from cache
        if let Some(entries) = self.cache.load().await? {
            // Search in cached entries
            let filtered: Vec<_> = entries
                .values()
                .filter(|e| {
                    e.name.to_lowercase().contains(&query.to_lowercase())
                        || e.description.to_lowercase().contains(&query.to_lowercase())
                        || e.tags.iter().any(|t| t.to_lowercase() == query.to_lowercase())
                })
                .cloned()
                .collect();

            if !filtered.is_empty() {
                return Ok(SearchResults {
                    total: filtered.len(),
                    entries: filtered,
                });
            }
        }

        // If not in cache or cache miss, fetch from registry API
        self.fetch_and_search(query).await
    }

    async fn fetch_and_search(&self,
        query: &str,
    ) -> McpResult<SearchResults> {
        let url = format!("{}/api/v1/search", self.config.url);

        let response = self
            .client
            .get(&url)
            .query(&[("q", query)])
            .send()
            .await
            .map_err(|e| McpError::TransportError(format!("Registry request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(McpError::TransportError(format!(
                "Registry returned error: {}",
                response.status()
            )));
        }

        let entries: Vec<RegistryEntry> = response
            .json()
            .await
            .map_err(|e| McpError::Serialization(e))?;

        // Update cache
        let mut cache_entries = HashMap::new();
        for entry in &entries {
            cache_entries.insert(entry.name.clone(), entry.clone());
        }
        if let Err(e) = self.cache.save(&cache_entries).await {
            warn!("Failed to save registry cache: {}", e);
        }

        Ok(SearchResults {
            total: entries.len(),
            entries,
        })
    }

    /// Get detailed info about a specific server
    pub async fn get_info(&self, name: &str) -> McpResult<Option<RegistryEntry>> {
        // Try cache first
        if let Some(entries) = self.cache.load().await? {
            if let Some(entry) = entries.get(name) {
                return Ok(Some(entry.clone()));
            }
        }

        // Fetch from API
        let url = format!("{}/api/v1/servers/{}", self.config.url, name);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| McpError::TransportError(format!("Registry request failed: {}", e)))?;

        if response.status().as_u16() == 404 {
            return Ok(None);
        }

        if !response.status().is_success() {
            return Err(McpError::TransportError(format!(
                "Registry returned error: {}",
                response.status()
            )));
        }

        let entry: RegistryEntry = response
            .json()
            .await
            .map_err(|e| McpError::Serialization(e))?;

        Ok(Some(entry))
    }

    /// Install a server from the registry
    pub async fn install(&self, name: &str) -> McpResult<RegistryEntry> {
        info!("Installing server from registry: {}", name);

        let entry = self
            .get_info(name)
            .await?
            .ok_or_else(|| McpError::ServerNotFound(format!("Server '{}' not found in registry", name)))?;

        // Run install command if provided
        if let Some(install_cmd) = &entry.install_command {
            info!("Running install command: {}", install_cmd);

            let parts: Vec<_> = install_cmd.split_whitespace().collect();
            if parts.is_empty() {
                return Err(McpError::ConfigError("Empty install command".to_string()));
            }

            let output = tokio::process::Command::new(parts[0])
                .args(&parts[1..])
                .output()
                .await
                .map_err(|e| McpError::Io(e))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(McpError::InternalError(format!(
                    "Install command failed: {}",
                    stderr
                )));
            }
        }

        info!("Successfully installed server: {}", name);
        Ok(entry)
    }

    /// Refresh the local cache
    pub async fn refresh_cache(&self) -> McpResult<()> {
        info!("Refreshing registry cache");

        // Clear existing cache
        self.cache.clear().await?;

        // Fetch all entries
        let url = format!("{}/api/v1/servers", self.config.url);

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| McpError::TransportError(format!("Registry request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(McpError::TransportError(format!(
                "Registry returned error: {}",
                response.status()
            )));
        }

        let entries: Vec<RegistryEntry> = response
            .json()
            .await
            .map_err(|e| McpError::Serialization(e))?;

        // Update cache
        let mut cache_entries = HashMap::new();
        for entry in entries {
            cache_entries.insert(entry.name.clone(), entry);
        }

        self.cache.save(&cache_entries).await?;
        info!("Registry cache refreshed with {} entries", cache_entries.len());

        Ok(())
    }
}
