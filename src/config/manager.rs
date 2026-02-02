use crate::compat::{OneMcpConfigAdapter, StandardMcpConfigAdapter};
use crate::config::Config;
use crate::utils::errors::{McpError, McpResult};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, error, info};

/// Supported config file formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// JSON format
    Json,
    /// YAML format
    Yaml,
}

impl ConfigFormat {
    /// Detect format from file extension and content
    pub fn detect(path: &PathBuf, content: &str) -> Self {
        let ext = path.extension().and_then(|ext| ext.to_str());

        match ext {
            Some("json") => ConfigFormat::Json,
            Some("yml") | Some("yaml") => ConfigFormat::Yaml,
            _ => {
                if content.trim_start().starts_with('{') {
                    ConfigFormat::Json
                } else {
                    ConfigFormat::Yaml
                }
            }
        }
    }

    /// Detect format from file extension only
    pub fn from_path(path: &PathBuf) -> Self {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => ConfigFormat::Json,
            Some("yml") | Some("yaml") | _ => ConfigFormat::Yaml,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ConfigEvent {
    Reloaded,
    Error(String),
}

pub struct ConfigManager {
    path: PathBuf,
    format: ConfigFormat,
    config: Arc<RwLock<Config>>,
    event_tx: broadcast::Sender<ConfigEvent>,
    _watcher: RecommendedWatcher,
}

impl ConfigManager {
    pub async fn new(path: impl Into<PathBuf>) -> McpResult<Self> {
        let path = path.into();
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| McpError::ConfigError(format!("Failed to read config: {}", e)))?;

        let format = ConfigFormat::detect(&path, &content);
        debug!("Detected config format: {:?}", format);

        let config = Self::parse_content(&path, &content, format).await?;
        let config = Arc::new(RwLock::new(config));

        let (event_tx, _) = broadcast::channel(16);
        let event_tx_clone = event_tx.clone();
        let config_clone = config.clone();
        let path_clone = path.clone();

        let rt_handle = tokio::runtime::Handle::try_current()
            .map_err(|e| McpError::ConfigError(format!("No Tokio runtime available: {}", e)))?;
        let watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            match res {
                Ok(event) => {
                    if event.kind.is_modify() {
                        info!("Config file changed, reloading...");
                        let config_clone = config_clone.clone();
                        let event_tx_clone = event_tx_clone.clone();
                        let path_clone = path_clone.clone();
                        let rt = rt_handle.clone();

                        rt.spawn(async move {
                            let content = match tokio::fs::read_to_string(&path_clone).await {
                                Ok(c) => c,
                                Err(e) => {
                                    let _ = event_tx_clone.send(ConfigEvent::Error(e.to_string()));
                                    return;
                                }
                            };
                            let format = ConfigFormat::detect(&path_clone, &content);
                            match Self::parse_content(&path_clone, &content, format).await {
                                Ok(new_config) => {
                                    *config_clone.write() = new_config;
                                    let _ = event_tx_clone.send(ConfigEvent::Reloaded);
                                }
                                Err(e) => {
                                    error!("Failed to reload config: {}", e);
                                    let _ = event_tx_clone.send(ConfigEvent::Error(e.to_string()));
                                }
                            }
                        });
                    }
                }
                Err(e) => {
                    error!("Config watcher error: {}", e);
                }
            }
        })
        .map_err(|e| McpError::ConfigError(e.to_string()))?;

        let mut manager = Self {
            path,
            format,
            config,
            event_tx,
            _watcher: watcher,
        };

        manager.start_watching().await?;
        Ok(manager)
    }

    async fn parse_content(_path: &PathBuf, content: &str, format: ConfigFormat) -> McpResult<Config> {
        match format {
            ConfigFormat::Json => {
                if content.contains("\"mcpServers\"") {
                    let mcp_config = serde_json::from_str::<crate::compat::McpJsonConfig>(content)
                        .map_err(|e| McpError::ConfigError(format!("Failed to parse mcp.json: {}", e)))?;
                    Ok(StandardMcpConfigAdapter::convert_mcp_json(&mcp_config))
                } else if content.contains("\"mcp\"") && content.contains("\"server\"") {
                    let smithery_config: crate::compat::SmitheryConfig = serde_json::from_str(content)
                        .map_err(|e| McpError::ConfigError(format!("Failed to parse Smithery config: {}", e)))?;
                    Ok(StandardMcpConfigAdapter::convert_smithery(&smithery_config))
                } else if content.contains("\"presets\"") && content.contains("\"servers\"") {
                    let presets_config: crate::compat::PresetsConfig = serde_json::from_str(content)
                        .map_err(|e| McpError::ConfigError(format!("Failed to parse presets.json: {}", e)))?;
                    Ok(StandardMcpConfigAdapter::convert_presets_json(&presets_config))
                } else {
                    serde_json::from_str(content)
                        .map_err(|e| McpError::ConfigError(format!("Failed to parse JSON config: {}", e)))
                }
            }
            ConfigFormat::Yaml => {
                if content.contains("presets:") && content.contains("servers:") {
                    let presets_config: crate::compat::PresetsConfig = serde_yaml::from_str(content)
                        .map_err(|e| McpError::ConfigError(format!("Failed to parse presets.yaml: {}", e)))?;
                    Ok(StandardMcpConfigAdapter::convert_presets_json(&presets_config))
                } else if content.contains("sandboxing:") || content.contains("rate_limiting:") {
                    let one_mcp_config: crate::compat::config::OneMcpConfig = serde_yaml::from_str(content)
                        .map_err(|e| McpError::ConfigError(format!("Failed to parse 1MCP config: {}", e)))?;
                    Ok(OneMcpConfigAdapter::convert(&one_mcp_config))
                } else {
                    serde_yaml::from_str(content)
                        .map_err(|e| McpError::ConfigError(format!("Failed to parse YAML config: {}", e)))
                }
            }
        }
    }

    async fn start_watching(&mut self) -> McpResult<()> {
        self._watcher.watch(&self.path, RecursiveMode::NonRecursive)
            .map_err(|e| McpError::ConfigError(e.to_string()))?;
        Ok(())
    }

    pub fn get_config(&self) -> Config {
        self.config.read().clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ConfigEvent> {
        self.event_tx.subscribe()
    }

    pub async fn reload(&self) -> McpResult<()> {
        let content = tokio::fs::read_to_string(&self.path).await
            .map_err(|e| McpError::ConfigError(format!("Failed to read config: {}", e)))?;
        let new_config = Self::parse_content(&self.path, &content, self.format).await?;
        *self.config.write() = new_config;
        let _ = self.event_tx.send(ConfigEvent::Reloaded);
        Ok(())
    }

    pub async fn save(&self, config: &Config) -> McpResult<()> {
        let content = match self.format {
            ConfigFormat::Json => serde_json::to_string_pretty(config)
                .map_err(|e| McpError::ConfigError(format!("Failed to serialize JSON: {}", e)))?,
            ConfigFormat::Yaml => serde_yaml::to_string(config)
                .map_err(|e| McpError::ConfigError(format!("Failed to serialize YAML: {}", e)))?,
        };
        tokio::fs::write(&self.path, content).await
            .map_err(|e| McpError::ConfigError(format!("Failed to write config: {}", e)))?;
        *self.config.write() = config.clone();
        Ok(())
    }

    pub async fn export_mcp_json(&self) -> String {
        crate::compat::StandardMcpConfigWriter::to_mcp_json(&self.get_config())
    }

    pub async fn export_presets_json(&self) -> String {
        crate::compat::StandardMcpConfigWriter::to_presets_json(&self.get_config())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    #[tokio::test]
    async fn test_load_json_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        let content = r#"{"server": {"host": "0.0.0.0", "port": 8080}, "servers": [{"name": "test", "command": "echo", "args": ["hello"], "tags": ["test"]}]}"#;
        fs::write(&config_path, content).await.unwrap();
        let manager = ConfigManager::new(&config_path).await.unwrap();
        let config = manager.get_config();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.servers[0].name, "test");
    }

    #[tokio::test]
    async fn test_load_mcp_json_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("mcp.json");
        let content = r#"{"mcpServers": {"filesystem": {"command": "uvx", "args": ["@modelcontextprotocol/server-filesystem", "/tmp"]}}}"#;
        fs::write(&config_path, content).await.unwrap();
        let manager = ConfigManager::new(&config_path).await.unwrap();
        let config = manager.get_config();
        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.servers[0].name, "filesystem");
    }

    #[test]
    fn test_config_format_detection() {
        let cases = vec![
            ("config.json", ConfigFormat::Json),
            ("mcp.json", ConfigFormat::Json),
            ("config.yaml", ConfigFormat::Yaml),
            ("config.yml", ConfigFormat::Yaml),
        ];
        for (path, expected) in cases {
            let path_buf = PathBuf::from(path);
            assert_eq!(ConfigFormat::from_path(&path_buf), expected, "Failed for: {}", path);
        }
    }
}
