//! ProviderRegistry integration tests

use supermcp::core::provider::{ProviderRegistry, ProviderType, Tool, ToolResult};
use supermcp::core::provider::Provider;
use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug)]
struct MockProvider {
    name: String,
    ptype: ProviderType,
}

#[async_trait]
impl Provider for MockProvider {
    fn name(&self) -> &str {
        &self.name
    }
    fn provider_type(&self) -> ProviderType {
        self.ptype
    }
    async fn is_available(&self) -> bool {
        true
    }
    async fn list_tools(&self) -> supermcp::utils::errors::McpResult<Vec<Tool>> {
        Ok(vec![])
    }
    async fn call_tool(&self, _: &str, _: Value) -> supermcp::utils::errors::McpResult<ToolResult> {
        Ok(ToolResult::success("ok").unwrap())
    }
}

#[tokio::test]
async fn test_registry_register_and_get() {
    let registry = ProviderRegistry::new();

    let provider1 = Box::new(MockProvider {
        name: "mock1".to_string(),
        ptype: ProviderType::McpStdio,
    });
    let provider2 = Box::new(MockProvider {
        name: "mock2".to_string(),
        ptype: ProviderType::McpHttp,
    });

    registry.register(provider1);
    registry.register(provider2);

    assert_eq!(registry.list().len(), 2);
    assert!(registry.get("mock1").is_some());
    assert!(registry.get("mock2").is_some());
    assert!(registry.get("nonexistent").is_none());

    let stdio_providers = registry.list_by_type(ProviderType::McpStdio);
    assert_eq!(stdio_providers.len(), 1);
    assert_eq!(stdio_providers[0], "mock1");
}

#[tokio::test]
async fn test_registry_find_tool() {
    let registry = ProviderRegistry::new();
    // find_tool should work even with empty registry
    let result = registry.find_tool("provider.tool").await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_registry_list_by_type() {
    let registry = ProviderRegistry::new();
    assert_eq!(registry.list().len(), 0);
    assert_eq!(registry.list_by_type(ProviderType::McpStdio).len(), 0);
}
