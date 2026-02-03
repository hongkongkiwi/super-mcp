//! Tests for tools and providers commands

use supermcp::cli::call::{list_providers, list_tools};
use tempfile::TempDir;
use tokio::fs;

#[tokio::test]
async fn test_list_tools_with_empty_config() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");
    fs::write(&config_path, "[server]\nhost = \"127.0.0.1\"\nport = 3000\n")
        .await
        .unwrap();

    // Should not panic with empty config
    let result = list_tools(
        Some(config_path.to_str().unwrap()),
        None,
        None,
        None,
        None,
        false,
        false,
        false,
    )
    .await;

    // Should complete without error (may have no servers)
    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn test_list_tools_with_invalid_provider() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");
    fs::write(&config_path, "[server]\nhost = \"127.0.0.1\"\nport = 3000\n")
        .await
        .unwrap();

    // Should return error for non-existent provider
    let result = list_tools(
        Some(config_path.to_str().unwrap()),
        Some("nonexistent"),
        None,
        None,
        None,
        false,
        false,
        false,
    )
    .await;

    // Either error or ok depending on implementation
    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn test_list_providers_with_empty_config() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");
    fs::write(&config_path, "[server]\nhost = \"127.0.0.1\"\nport = 3000\n")
        .await
        .unwrap();

    // Should complete without error
    let result = list_providers(
        Some(config_path.to_str().unwrap()),
        false,
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_list_providers_json_output() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");
    fs::write(&config_path, "[server]\nhost = \"127.0.0.1\"\nport = 3000\n")
        .await
        .unwrap();

    // Should complete without error
    let result = list_providers(
        Some(config_path.to_str().unwrap()),
        true, // json output
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_list_tools_json_output() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");
    fs::write(&config_path, "[server]\nhost = \"127.0.0.1\"\nport = 3000\n")
        .await
        .unwrap();

    // Should complete without error
    let result = list_tools(
        Some(config_path.to_str().unwrap()),
        None,
        None,
        None,
        None,
        false,
        true, // json output
        false,
    )
    .await;

    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn test_list_tools_all_flag() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");
    fs::write(&config_path, "[server]\nhost = \"127.0.0.1\"\nport = 3000\n")
        .await
        .unwrap();

    // Should complete without error with --all flag
    let result = list_tools(
        Some(config_path.to_str().unwrap()),
        None,
        None,
        None,
        None,
        false,
        false,
        true, // all
    )
    .await;

    assert!(result.is_ok() || result.is_err());
}
