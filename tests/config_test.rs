//! Configuration management tests

use super_mcp::config::{Config, ConfigManager, ServerConfig, McpServerConfig};
use super_mcp::config::validation::ConfigValidator;
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

#[tokio::test]
async fn test_config_reload() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    let config_content = r#"
[server]
host = "127.0.0.1"
port = 3000
"#;

    fs::write(&config_path, config_content).await.unwrap();

    let manager = ConfigManager::new(&config_path).await.unwrap();
    
    // Modify the config file
    let new_config = r#"
[server]
host = "0.0.0.0"
port = 4000
"#;
    fs::write(&config_path, new_config).await.unwrap();

    // Wait a bit for the file watcher to detect the change
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Manually trigger reload
    manager.reload().await.unwrap();
    
    let config = manager.get_config();
    assert_eq!(config.server.port, 4000);
}

#[test]
fn test_default_config() {
    let config = Config::default();
    
    assert_eq!(config.server.host, "127.0.0.1");
    assert_eq!(config.server.port, 3000);
    assert!(config.servers.is_empty());
    assert!(config.presets.is_empty());
}

#[test]
fn test_config_validation_valid() {
    let validator = ConfigValidator::new();
    let toml = r#"
[server]
host = "127.0.0.1"
port = 3000

[[servers]]
name = "test"
command = "echo"
args = ["hello"]
tags = ["test"]
"#;
    
    let result = validator.validate_toml(toml);
    assert!(result.is_ok());
}

#[test]
fn test_config_validation_duplicate_servers() {
    let validator = ConfigValidator::new();
    let toml = r#"
[[servers]]
name = "test"
command = "echo"

[[servers]]
name = "test"
command = "cat"
"#;
    
    let result = validator.validate_toml(toml);
    assert!(result.is_err());
}

#[test]
fn test_config_validation_empty_server_name() {
    let validator = ConfigValidator::new();
    let toml = r#"
[[servers]]
name = ""
command = "echo"
"#;
    
    let result = validator.validate_toml(toml);
    assert!(result.is_err());
}

#[test]
fn test_config_validation_preset_without_tags() {
    let validator = ConfigValidator::new();
    let toml = r#"
[[presets]]
name = "test-preset"
tags = []
"#;
    
    let result = validator.validate_toml(toml);
    assert!(result.is_err());
}

#[test]
fn test_schema_export() {
    let validator = ConfigValidator::new();
    let schema = validator.export_schema();
    
    assert!(!schema.is_empty());
    let json: serde_json::Value = serde_json::from_str(&schema).unwrap();
    assert!(json.get("$schema").is_some());
}

#[test]
fn test_server_config_serialization() {
    let config = ServerConfig {
        host: "0.0.0.0".to_string(),
        port: 8080,
        cert_path: Some("/path/to/cert.pem".to_string()),
        key_path: Some("/path/to/key.pem".to_string()),
    };
    
    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains("0.0.0.0"));
    assert!(json.contains("8080"));
}

#[test]
fn test_mcp_server_config_default() {
    let config = McpServerConfig {
        name: "test".to_string(),
        command: "echo".to_string(),
        ..Default::default()
    };
    
    assert!(config.args.is_empty());
    assert!(config.env.is_empty());
    assert!(config.tags.is_empty());
    assert!(config.description.is_none());
}
