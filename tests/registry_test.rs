//! Registry client tests

use super_mcp::registry::{RegistryClient, types::RegistryConfig};
use std::path::PathBuf;

fn create_test_registry_config() -> RegistryConfig {
    RegistryConfig {
        url: "https://registry.modelcontextprotocol.io".to_string(),
        cache_dir: PathBuf::from("/tmp/super-mcp-test-registry"),
        cache_ttl_hours: 24,
    }
}

#[test]
fn test_registry_client_creation() {
    let config = create_test_registry_config();
    let _client = RegistryClient::new(config);
    // Just test that it doesn't panic
}

#[test]
fn test_registry_config_default() {
    let config = RegistryConfig::default();
    assert!(!config.url.is_empty());
    assert_eq!(config.cache_ttl_hours, 24);
}

// Note: These tests require network access and may be flaky
// They are marked as ignored by default

#[tokio::test]
#[ignore]
async fn test_registry_search() {
    let config = create_test_registry_config();
    let client = RegistryClient::new(config);
    
    // This will fail without network, but tests the structure
    let _result = client.search("filesystem").await;
    // Result may be error if registry is not available
}

#[tokio::test]
#[ignore]
async fn test_registry_info() {
    let config = create_test_registry_config();
    let client = RegistryClient::new(config);
    
    let _result = client.get_info("filesystem").await;
    // Result depends on network and registry availability
}

#[tokio::test]
#[ignore]
async fn test_registry_cache_refresh() {
    let config = create_test_registry_config();
    let client = RegistryClient::new(config);
    
    let _result = client.refresh_cache().await;
    // Result depends on network availability
}
