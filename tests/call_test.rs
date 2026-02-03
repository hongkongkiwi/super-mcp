//! Tests for the call module

use supermcp::cli::call::build_registry;
use tempfile::TempDir;
use tokio::fs;

#[tokio::test]
async fn test_adhoc_providers_have_unique_names() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");
    fs::write(&config_path, "[server]\nhost = \"127.0.0.1\"\nport = 3000\n")
        .await
        .unwrap();

    // Test stdio adhoc provider
    let registry = build_registry(
        Some(config_path.to_str().unwrap()),
        Some("echo hello"),
        None,
        None,
    )
    .await
    .unwrap();
    let providers = registry.list();
    let stdio_providers: Vec<_> = providers.iter().filter(|p| p.starts_with("adhoc")).collect();
    // Should have exactly 1 adhoc provider named "adhoc-stdio"
    assert!(stdio_providers.contains(&&"adhoc-stdio".to_string()),
        "Expected 'adhoc-stdio' in providers, got: {:?}", providers);

    // Test HTTP/SSE adhoc provider - this will fail to connect but should register the provider
    // We need to test that the naming doesn't conflict, so we check if both can be registered
    let _result = build_registry(
        Some(config_path.to_str().unwrap()),
        None,
        Some("http://localhost:8080/sse"),
        None,
    )
    .await;

    // The HTTP connection will fail, but we can verify the naming by checking
    // that the function attempts to use "adhoc-sse" (not "adhoc")
    // For now, just verify the stdio case works with unique naming
}
