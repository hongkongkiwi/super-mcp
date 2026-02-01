//! CLI command implementations

pub mod mcp;
pub mod preset;
pub mod registry;

use crate::utils::errors::McpResult;
use std::path::PathBuf;

/// Get the default config path
pub fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .map(|p| p.join("super-mcp/config.toml"))
        .unwrap_or_else(|| PathBuf::from("~/.config/super-mcp/config.toml"))
}

/// Expand tilde in path
pub fn expand_path(path: &str) -> String {
    shellexpand::tilde(path).to_string()
}

/// Ensure config directory exists
pub async fn ensure_config_dir(config_path: &PathBuf) -> McpResult<()> {
    if let Some(parent) = config_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| crate::utils::errors::McpError::ConfigError(format!("Failed to create config dir: {}", e)))?;
    }
    Ok(())
}
