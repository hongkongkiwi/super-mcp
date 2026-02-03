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

    // Both stdio and http adhoc providers should have unique names
    // This test will fail before the fix because both use "adhoc"
    let registry = build_registry(
        Some(config_path.to_str().unwrap()),
        Some("echo hello"),
        Some("http://localhost:8080/sse"),
        None,
    )
    .await
    .unwrap();

    let providers = registry.list();
    // Should have 2 separate adhoc providers with different names
    let adhoc_providers: Vec<_> = providers
        .iter()
        .filter(|p| p.starts_with("adhoc-"))
        .collect();
    assert_eq!(adhoc_providers.len(), 2);
}
