//! Registry types for MCP server metadata
use serde::{Deserialize, Serialize};

/// Registry server entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub license: String,
    pub tags: Vec<String>,
    pub repository: Option<String>,
    pub homepage: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    pub env: std::collections::HashMap<String, String>,
    pub install_command: Option<String>,
    pub schema: Option<serde_json::Value>,
}

/// Search results from registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub total: usize,
    pub entries: Vec<RegistryEntry>,
}

/// Registry configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    pub url: String,
    pub cache_dir: std::path::PathBuf,
    pub cache_ttl_hours: u64,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            url: "https://registry.modelcontextprotocol.io".to_string(),
            cache_dir: dirs::cache_dir()
                .unwrap_or_else(|| std::path::PathBuf::from(".cache"))
                .join("mcp-one/registry"),
            cache_ttl_hours: 24,
        }
    }
}
