//! Integration tests

use super_mcp::config::{Config, McpServerConfig, validation::ConfigValidator};
use super_mcp::core::protocol::{JsonRpcRequest, JsonRpcResponse, RequestId};
use serde_json::json;
use std::collections::HashMap;
use tempfile::TempDir;
use tokio::fs;

#[tokio::test]
async fn test_full_config_workflow() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");
    
    // Create a complete config
    let config = Config {
        server: super_mcp::config::ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 3000,
            cert_path: None,
            key_path: None,
        },
        servers: vec![
            McpServerConfig {
                name: "filesystem".to_string(),
                command: "npx".to_string(),
                args: vec!["-y".to_string(), "@modelcontextprotocol/server-filesystem".to_string(), "/tmp".to_string()],
                env: HashMap::new(),
                tags: vec!["filesystem".to_string()],
                description: Some("Filesystem server".to_string()),
                sandbox: Default::default(),
            }
        ],
        presets: vec![
            super_mcp::config::PresetConfig {
                name: "development".to_string(),
                tags: vec!["filesystem".to_string()],
                description: Some("Dev preset".to_string()),
            }
        ],
        ..Default::default()
    };
    
    // Serialize to TOML
    let toml = toml::to_string(&config).unwrap();
    fs::write(&config_path, &toml).await.unwrap();
    
    // Validate the config
    let validator = ConfigValidator::new();
    let result = validator.validate_toml(&toml);
    assert!(result.is_ok(), "Config should be valid");
    
    // Load and verify
    let content = fs::read_to_string(&config_path).await.unwrap();
    let loaded: Config = toml::from_str(&content).unwrap();
    
    assert_eq!(loaded.servers.len(), 1);
    assert_eq!(loaded.servers[0].name, "filesystem");
    assert_eq!(loaded.presets.len(), 1);
}

#[test]
fn test_json_rpc_roundtrip() {
    // Create request
    let request = JsonRpcRequest::new(
        "tools/call",
        Some(json!({
            "name": "test_tool",
            "arguments": {"key": "value"}
        }))
    );
    
    // Serialize
    let json = serde_json::to_string(&request).unwrap();
    
    // Deserialize
    let deserialized: JsonRpcRequest = serde_json::from_str(&json).unwrap();
    
    assert_eq!(deserialized.method, "tools/call");
    assert!(deserialized.params.is_some());
}

#[test]
fn test_error_response_conversion() {
    use super_mcp::utils::errors::McpError;
    use axum::http::StatusCode;
    
    let errors = vec![
        (McpError::ServerNotFound("test".to_string()), StatusCode::NOT_FOUND),
        (McpError::AuthError("unauthorized".to_string()), StatusCode::UNAUTHORIZED),
        (McpError::AuthorizationError("forbidden".to_string()), StatusCode::FORBIDDEN),
        (McpError::InvalidRequest("bad request".to_string()), StatusCode::BAD_REQUEST),
        (McpError::Timeout(5000), StatusCode::GATEWAY_TIMEOUT),
        (McpError::TransportError("failed".to_string()), StatusCode::BAD_GATEWAY),
    ];
    
    for (error, expected_status) in errors {
        assert_eq!(error.status_code(), expected_status);
    }
}

#[test]
fn test_sandbox_constraints_default() {
    use super_mcp::sandbox::traits::SandboxConstraints;
    
    let constraints = SandboxConstraints::default();
    assert_eq!(constraints.max_memory_mb, 512);
    assert_eq!(constraints.max_cpu_percent, 50);
    assert!(!constraints.network);
}

#[tokio::test]
async fn test_config_manager_events() {
    use super_mcp::config::ConfigManager;
    
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");
    
    // Create initial config
    let config = r#"
[server]
host = "127.0.0.1"
port = 3000
"#;
    fs::write(&config_path, config).await.unwrap();
    
    // Create manager and subscribe to events
    let manager = ConfigManager::new(&config_path).await.unwrap();
    let mut rx = manager.subscribe();
    
    // Modify config to trigger event
    let new_config = r#"
[server]
host = "127.0.0.1"
port = 4000
"#;
    fs::write(&config_path, new_config).await.unwrap();
    
    // Trigger reload manually
    manager.reload().await.unwrap();
    
    // Check that config was updated
    let updated_config = manager.get_config();
    assert_eq!(updated_config.server.port, 4000);
}

#[test]
fn test_registry_types_serialization() {
    use super_mcp::registry::types::{RegistryEntry, SearchResults};
    
    let entry = RegistryEntry {
        name: "test".to_string(),
        version: "1.0.0".to_string(),
        description: "Test server".to_string(),
        author: "Test Author".to_string(),
        license: "MIT".to_string(),
        tags: vec!["test".to_string()],
        repository: Some("https://github.com/test".to_string()),
        homepage: Some("https://test.com".to_string()),
        command: "test-cmd".to_string(),
        args: vec!["arg1".to_string()],
        env: HashMap::new(),
        install_command: Some("npm install".to_string()),
        schema: None,
    };
    
    let results = SearchResults {
        total: 1,
        entries: vec![entry],
    };
    
    let json = serde_json::to_string(&results).unwrap();
    assert!(json.contains("test"));
    assert!(json.contains("1.0.0"));
}
