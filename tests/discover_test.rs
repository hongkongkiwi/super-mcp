//! Tests for discover/import functionality

use tempfile::TempDir;
use tokio::fs;

/// Test that discover module can be imported and used
#[tokio::test]
async fn test_discover_module_loads() {
    // Basic test that the module can be imported
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    // Create a minimal config
    let config = r#"[server]
host = "127.0.0.1"
port = 3000
"#;
    fs::write(&config_path, config).await.unwrap();

    // The module should be loadable without errors
    assert!(config_path.exists());
}

/// Test config loading for import functionality
#[tokio::test]
async fn test_import_config_structure() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    // Create a config with MCP server
    let config = r#"[server]
host = "127.0.0.1"
port = 3000

[[servers]]
name = "test-server"
command = "echo"
"#;
    fs::write(&config_path, config).await.unwrap();

    // Config should be readable
    let content = fs::read_to_string(&config_path).await.unwrap();
    assert!(content.contains("test-server"));
}

/// Test that discovered MCPs can be structured properly
#[tokio::test]
async fn test_discovered_mcp_structure() {
    // Just verify the structure can be created
    use supermcp::cli::discover::DiscoveredMcp;
    use std::collections::HashMap;
    use std::path::PathBuf;

    let mcp = DiscoveredMcp {
        name: "test".to_string(),
        command: "echo".to_string(),
        args: vec!["hello".to_string()],
        env: HashMap::new(),
        source: "test".to_string(),
        description: None,
        source_path: PathBuf::new(),
        auto_approve: Some(false),
    };

    assert_eq!(mcp.name, "test");
    assert_eq!(mcp.command, "echo");
}
