//! Additional tests for ProviderRegistry

use supermcp::core::provider::{ProviderRegistry, ProviderType, Tool, ToolResult};
use supermcp::core::provider::Provider;
use async_trait::async_trait;
use serde_json::Value;

#[derive(Debug)]
struct TestProvider {
    name: String,
    ptype: ProviderType,
    tools: Vec<Tool>,
}

impl TestProvider {
    fn new(name: &str, ptype: ProviderType, tools: Vec<Tool>) -> Self {
        Self {
            name: name.to_string(),
            ptype,
            tools,
        }
    }
}

#[async_trait]
impl Provider for TestProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> ProviderType {
        self.ptype.clone()
    }

    async fn is_available(&self) -> bool {
        true
    }

    async fn list_tools(&self) -> supermcp::utils::errors::McpResult<Vec<Tool>> {
        Ok(self.tools.clone())
    }

    async fn call_tool(&self, name: &str, _: Value) -> supermcp::utils::errors::McpResult<ToolResult> {
        Ok(ToolResult::success(format!("Called {}", name)).unwrap())
    }
}

fn make_tool(name: &str, provider: &str, _required: bool) -> Tool {
    Tool {
        name: format!("{}.{}", provider, name),
        description: Some(format!("Tool {} from {}", name, provider)),
        provider: provider.to_string(),
        provider_type: ProviderType::McpStdio,
        parameters: vec![],
        metadata: std::collections::HashMap::new(),
    }
}

#[tokio::test]
async fn test_registry_empty() {
    let registry = ProviderRegistry::new();
    assert_eq!(registry.list().len(), 0);
    assert!(registry.list_by_type(ProviderType::McpStdio).is_empty());
}

#[tokio::test]
async fn test_registry_register_same_name_twice() {
    let registry = ProviderRegistry::new();

    let provider1 = Box::new(TestProvider::new(
        "same-name",
        ProviderType::McpStdio,
        vec![],
    ));
    let provider2 = Box::new(TestProvider::new(
        "same-name",
        ProviderType::McpHttp,
        vec![],
    ));

    registry.register(provider1);
    registry.register(provider2);

    // Should only have one (last one registered)
    assert_eq!(registry.list().len(), 1);
}

#[tokio::test]
async fn test_registry_list_by_type() {
    let registry = ProviderRegistry::new();

    registry.register(Box::new(TestProvider::new(
        "stdio1",
        ProviderType::McpStdio,
        vec![],
    )));
    registry.register(Box::new(TestProvider::new(
        "stdio2",
        ProviderType::McpStdio,
        vec![],
    )));
    registry.register(Box::new(TestProvider::new(
        "http1",
        ProviderType::McpHttp,
        vec![],
    )));
    registry.register(Box::new(TestProvider::new(
        "sse1",
        ProviderType::McpSse,
        vec![],
    )));

    let stdio_providers = registry.list_by_type(ProviderType::McpStdio);
    let http_providers = registry.list_by_type(ProviderType::McpHttp);
    let sse_providers = registry.list_by_type(ProviderType::McpSse);

    assert_eq!(stdio_providers.len(), 2);
    assert_eq!(http_providers.len(), 1);
    assert_eq!(sse_providers.len(), 1);
}

#[tokio::test]
async fn test_registry_get_nonexistent() {
    let registry = ProviderRegistry::new();
    assert!(registry.get("nonexistent").is_none());
}

#[tokio::test]
async fn test_registry_list_all_tools_empty() {
    let registry = ProviderRegistry::new();
    let tools = registry.list_all_tools().await.unwrap();
    assert!(tools.is_empty());
}

#[tokio::test]
async fn test_registry_list_all_tools_with_tools() {
    let registry = ProviderRegistry::new();

    registry.register(Box::new(TestProvider::new(
        "provider1",
        ProviderType::McpStdio,
        vec![
            make_tool("tool1", "provider1", false),
            make_tool("tool2", "provider1", true),
        ],
    )));
    registry.register(Box::new(TestProvider::new(
        "provider2",
        ProviderType::McpHttp,
        vec![
            make_tool("tool3", "provider2", false),
        ],
    )));

    let tools = registry.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 3);
}

#[tokio::test]
async fn test_registry_find_tool_by_name() {
    let registry = ProviderRegistry::new();

    registry.register(Box::new(TestProvider::new(
        "provider1",
        ProviderType::McpStdio,
        vec![
            make_tool("read_file", "provider1", false),
        ],
    )));

    let result = registry.find_tool("provider1.read_file").await.unwrap();
    assert!(result.is_some());
    // find_tool returns Option<(Tool, String)>
    let (tool, prov) = result.unwrap();
    assert_eq!(tool.name, "provider1.read_file");
    assert_eq!(prov, "provider1");
}

#[tokio::test]
async fn test_registry_find_tool_not_found() {
    let registry = ProviderRegistry::new();

    registry.register(Box::new(TestProvider::new(
        "provider1",
        ProviderType::McpStdio,
        vec![],
    )));

    let result = registry.find_tool("nonexistent").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_registry_find_tool_ambiguous() {
    let registry = ProviderRegistry::new();

    registry.register(Box::new(TestProvider::new(
        "provider1",
        ProviderType::McpStdio,
        vec![make_tool("common_tool", "provider1", false)],
    )));
    registry.register(Box::new(TestProvider::new(
        "provider2",
        ProviderType::McpHttp,
        vec![make_tool("common_tool", "provider2", false)],
    )));

    // Should find the tool with explicit provider
    let result = registry.find_tool("provider1.common_tool").await.unwrap();
    assert!(result.is_some());
}
