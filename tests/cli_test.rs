//! CLI command tests

use super_mcp::cli;
use std::path::PathBuf;
use tempfile::TempDir;
use tokio::fs;

#[tokio::test]
async fn test_mcp_add() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    cli::mcp::add(
        config_path.to_str().unwrap(),
        "test-server",
        "echo",
        Some(vec!["hello".to_string()]),
        Some(vec!["KEY=value".to_string()]),
        Some(vec!["test".to_string()]),
        Some("Test server".to_string()),
    )
    .await
    .unwrap();

    // Verify config was created
    assert!(config_path.exists());
    
    let content = fs::read_to_string(&config_path).await.unwrap();
    assert!(content.contains("test-server"));
    assert!(content.contains("echo"));
    assert!(content.contains("hello"));
}

#[tokio::test]
async fn test_mcp_add_duplicate() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    cli::mcp::add(
        config_path.to_str().unwrap(),
        "test-server",
        "echo",
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    // Try to add the same server again
    let result = cli::mcp::add(
        config_path.to_str().unwrap(),
        "test-server",
        "cat",
        None,
        None,
        None,
        None,
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_mcp_list_empty() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    // Create empty config
    fs::write(&config_path, "[server]\nhost = \"127.0.0.1\"\nport = 3000\n")
        .await
        .unwrap();

    // Should not error even with no servers
    cli::mcp::list(config_path.to_str().unwrap()).await.unwrap();
}

#[tokio::test]
async fn test_mcp_remove() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    cli::mcp::add(
        config_path.to_str().unwrap(),
        "test-server",
        "echo",
        None,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    cli::mcp::remove(config_path.to_str().unwrap(), "test-server")
        .await
        .unwrap();

    let content = fs::read_to_string(&config_path).await.unwrap();
    assert!(!content.contains("[[servers]]"));
}

#[tokio::test]
async fn test_mcp_remove_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    fs::write(&config_path, "[server]\nhost = \"127.0.0.1\"\n")
        .await
        .unwrap();

    let result = cli::mcp::remove(config_path.to_str().unwrap(), "nonexistent").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_preset_create() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    cli::preset::create(
        config_path.to_str().unwrap(),
        "dev",
        Some(vec!["filesystem".to_string(), "local".to_string()]),
        Some("Development preset".to_string()),
    )
    .await
    .unwrap();

    let content = fs::read_to_string(&config_path).await.unwrap();
    assert!(content.contains("dev"));
    assert!(content.contains("filesystem"));
    assert!(content.contains("local"));
}

#[tokio::test]
async fn test_preset_list_empty() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    fs::write(&config_path, "[server]\nhost = \"127.0.0.1\"\n")
        .await
        .unwrap();

    cli::preset::list(config_path.to_str().unwrap()).await.unwrap();
}

#[tokio::test]
async fn test_preset_remove() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    cli::preset::create(
        config_path.to_str().unwrap(),
        "dev",
        Some(vec!["test".to_string()]),
        None,
    )
    .await
    .unwrap();

    cli::preset::remove(config_path.to_str().unwrap(), "dev")
        .await
        .unwrap();

    let content = fs::read_to_string(&config_path).await.unwrap();
    assert!(!content.contains("[[presets]]"));
}

#[tokio::test]
async fn test_preset_test() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    // Create config with server and preset
    let config = r#"
[[servers]]
name = "filesystem"
command = "npx"
tags = ["filesystem", "local"]

[[presets]]
name = "dev"
tags = ["filesystem"]
description = "Development"
"#;
    fs::write(&config_path, config).await.unwrap();

    // Should not error
    cli::preset::test(config_path.to_str().unwrap(), "dev").await.unwrap();
}

#[test]
fn test_expand_path() {
    let expanded = cli::expand_path("~/.config/test");
    assert!(!expanded.contains("~"));
}

#[test]
fn test_default_config_path() {
    let path = cli::default_config_path();
    assert!(path.to_string_lossy().contains("super-mcp"));
}
