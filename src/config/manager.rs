use crate::config::Config;
use crate::utils::errors::{McpError, McpResult};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{error, info};

/// Supported config file formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// TOML format (default)
    Toml,
    /// JSON format
    Json,
}

impl ConfigFormat {
    /// Detect format from file extension
    pub fn from_path(path: &PathBuf) -> Self {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => ConfigFormat::Json,
            Some("toml") | _ => ConfigFormat::Toml,
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
    config: Arc<RwLock<Config>>,
    event_tx: broadcast::Sender<ConfigEvent>,
    _watcher: RecommendedWatcher,
}

impl ConfigManager {
    pub async fn new(path: impl Into<PathBuf>) -> McpResult<Self> {
        let path = path.into();
        let config = Self::load_config(&path).await?;
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
                            match Self::load_config(&path_clone).await {
                                Ok(new_config) => {
                                    *config_clone.write() = new_config;
                                    let _ = event_tx_clone.send(ConfigEvent::Reloaded);
                                }
                                Err(e) => {
                                    error!("Failed to reload config: {}", e);
                                    let _ = event_tx_clone
                                        .send(ConfigEvent::Error(e.to_string()));
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
            config,
            event_tx,
            _watcher: watcher,
        };

        // Start watching
        manager.start_watching().await?;

        Ok(manager)
    }

    async fn load_config(path: &PathBuf) -> McpResult<Config> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| McpError::ConfigError(format!("Failed to read config: {}", e)))?;

        let format = ConfigFormat::from_path(path);

        let config: Config = match format {
            ConfigFormat::Toml => toml::from_str(&content)
                .map_err(|e| McpError::ConfigError(format!("Failed to parse TOML config: {}", e)))?,
            ConfigFormat::Json => serde_json::from_str(&content)
                .map_err(|e| McpError::ConfigError(format!("Failed to parse JSON config: {}", e)))?,
        };

        Ok(config)
    }

    async fn start_watching(&mut self) -> McpResult<()> {
        self._watcher
            .watch(&self.path, RecursiveMode::NonRecursive)
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
        let new_config = Self::load_config(&self.path).await?;
        *self.config.write() = new_config;
        let _ = self.event_tx.send(ConfigEvent::Reloaded);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs;

    #[tokio::test]
    async fn test_load_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        let config_content = r#"
[server]
host = "0.0.0.0"
port = 8080

[[servers]]
name = "test"
command = "echo"
args = ["hello"]
tags = ["test"]
"#;

        fs::write(&config_path, config_content).await.unwrap();

        let manager = ConfigManager::new(&config_path).await.unwrap();
        let config = manager.get_config();

        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.servers[0].name, "test");
    }
}
