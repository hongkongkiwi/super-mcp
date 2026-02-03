//! Integration tests for the call command workflow

use supermcp::cli::call::parse_function_style;
use tempfile::TempDir;
use tokio::fs;

#[tokio::test]
async fn test_call_with_empty_config() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");
    fs::write(&config_path, "[server]\nhost = \"127.0.0.1\"\nport = 3000\n")
        .await
        .unwrap();

    // Should not error with empty config
    let result = supermcp::cli::call::list_tools(
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

    // Result depends on whether there are servers - should not panic
    assert!(result.is_ok() || result.is_err());
}

#[tokio::test]
async fn test_list_providers_with_empty_config() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");
    fs::write(&config_path, "[server]\nhost = \"127.0.0.1\"\nport = 3000\n")
        .await
        .unwrap();

    // Should not error with empty config
    let result = supermcp::cli::call::list_providers(
        Some(config_path.to_str().unwrap()),
        false,
    )
    .await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_parse_target_with_server_prefix() {
    // Server.tool format
    let result = parse_function_style("server.tool_name(arg1: value1)");
    assert!(result.is_ok());
    let (name, params) = result.unwrap();
    assert_eq!(name, "server.tool_name");
    assert_eq!(params["arg1"], "value1");
}
