//! Unified provider abstraction for MCP servers and skills
//!
//! This module provides a common interface for different types of tool providers:
//! - MCP servers (stdio, SSE, HTTP)
//! - Kimi CLI skills
//! - Future provider types

use crate::core::protocol::JsonRpcRequest;
use crate::utils::errors::McpResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The type of tool provider
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    /// MCP server via stdio
    McpStdio,
    /// MCP server via SSE
    McpSse,
    /// MCP server via Streamable HTTP
    McpHttp,
    /// Kimi CLI skill
    Skill,
    /// Custom provider
    Custom,
}

impl ProviderType {
    /// Check if this is an MCP provider
    pub fn is_mcp(&self) -> bool {
        matches!(
            self,
            ProviderType::McpStdio | ProviderType::McpSse | ProviderType::McpHttp
        )
    }

    /// Check if this is a skill provider
    pub fn is_skill(&self) -> bool {
        matches!(self, ProviderType::Skill)
    }
}

impl std::fmt::Display for ProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderType::McpStdio => write!(f, "mcp-stdio"),
            ProviderType::McpSse => write!(f, "mcp-sse"),
            ProviderType::McpHttp => write!(f, "mcp-http"),
            ProviderType::Skill => write!(f, "skill"),
            ProviderType::Custom => write!(f, "custom"),
        }
    }
}

/// Tool parameter schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterSchema {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
    #[serde(rename = "type")]
    pub param_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
}

/// Tool definition from any provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// Unique tool name (prefixed with provider name)
    pub name: String,
    /// Human-readable description
    pub description: Option<String>,
    /// Provider that owns this tool
    pub provider: String,
    /// Provider type
    #[serde(rename = "provider_type")]
    pub provider_type: ProviderType,
    /// Parameter schemas
    pub parameters: Vec<ParameterSchema>,
    /// Additional metadata
    #[serde(skip_serializing_if = "HashMap::is_empty", default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

impl Tool {
    /// Get the display name (without provider prefix)
    pub fn display_name(&self) -> &str {
        self.name
            .strip_prefix(&format!("{}.", self.provider))
            .unwrap_or(&self.name)
    }

    /// Get the snake_case version of the name
    pub fn snake_name(&self) -> String {
        self.display_name().replace('-', "_")
    }
}

/// Tool call result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Content formatted for display
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<serde_json::Value>>,
}

impl ToolResult {
    pub fn success(data: impl Serialize) -> McpResult<Self> {
        Ok(Self {
            success: true,
            data: Some(serde_json::to_value(data)?),
            error: None,
            content: None,
        })
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message.into()),
            content: None,
        }
    }

    pub fn with_content(mut self, content: Vec<serde_json::Value>) -> Self {
        self.content = Some(content);
        self
    }

    /// Get text content if available
    pub fn text(&self) -> Option<String> {
        self.content.as_ref()?.iter().find_map(|c| {
            c.get("text").and_then(|t| t.as_str()).map(String::from)
        })
    }
}

/// Unified provider trait for both MCP servers and skills
#[async_trait]
pub trait Provider: Send + Sync {
    /// Get the provider name
    fn name(&self) -> &str;

    /// Get the provider type
    fn provider_type(&self) -> ProviderType;

    /// Check if provider is connected/available
    async fn is_available(&self) -> bool;

    /// List all tools provided by this provider
    async fn list_tools(&self) -> McpResult<Vec<Tool>>;

    /// Call a tool by name
    async fn call_tool(&self, name: &str, arguments: serde_json::Value) -> McpResult<ToolResult>;

    /// Get provider metadata
    fn metadata(&self) -> HashMap<String, serde_json::Value> {
        HashMap::new()
    }
}

/// Provider that wraps an MCP server (adapter pattern)
pub struct McpProvider {
    name: String,
    provider_type: ProviderType,
    server: crate::core::server::ManagedServer,
}

impl McpProvider {
    pub fn new(name: String, provider_type: ProviderType, server: crate::core::server::ManagedServer) -> Self {
        Self {
            name,
            provider_type,
            server,
        }
    }
}

#[async_trait]
impl Provider for McpProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> ProviderType {
        self.provider_type
    }

    async fn is_available(&self) -> bool {
        self.server.is_connected().await
    }

    async fn list_tools(&self) -> McpResult<Vec<Tool>> {
        let request = JsonRpcRequest::new("tools/list", None);
        let response = self.server.send_request(request).await?;

        if let Some(error) = response.error {
            return Err(crate::utils::errors::McpError::ToolExecutionError(error.message));
        }

        let tools = response
            .result
            .and_then(|r| r.get("tools").cloned())
            .and_then(|t| t.as_array().cloned())
            .unwrap_or_default();

        let mut result = Vec::new();
        for tool in tools {
            let name = tool
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();
            
            let description = tool
                .get("description")
                .and_then(|d| d.as_str())
                .map(String::from);

            let parameters = parse_mcp_schema(&tool);

            result.push(Tool {
                name: format!("{}.{}", self.name, name),
                description,
                provider: self.name.clone(),
                provider_type: self.provider_type,
                parameters,
                metadata: HashMap::new(),
            });
        }

        Ok(result)
    }

    async fn call_tool(&self, name: &str, arguments: serde_json::Value) -> McpResult<ToolResult> {
        // Remove provider prefix if present
        let tool_name = name
            .strip_prefix(&format!("{}.", self.name))
            .unwrap_or(name)
            .to_string();

        let request = JsonRpcRequest::new(
            "tools/call",
            Some(serde_json::json!({
                "name": tool_name,
                "arguments": arguments,
            })),
        );

        let response = self.server.send_request(request).await?;

        if let Some(error) = response.error {
            return Ok(ToolResult::error(error.message));
        }

        if let Some(result) = response.result {
            let content = result
                .get("content")
                .and_then(|c| c.as_array().cloned())
                .unwrap_or_default();

            Ok(ToolResult {
                success: true,
                data: Some(result.clone()),
                error: None,
                content: Some(content),
            })
        } else {
            Ok(ToolResult::success(serde_json::json!({"status": "ok"}))?)
        }
    }
}

/// Parse MCP tool schema into our ParameterSchema format
fn parse_mcp_schema(tool: &serde_json::Value) -> Vec<ParameterSchema> {
    let schema = tool.get("inputSchema");
    let properties = schema.and_then(|s| s.get("properties"));
    let required: Vec<String> = schema
        .and_then(|s| s.get("required"))
        .and_then(|r| r.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let mut params = Vec::new();

    if let Some(props) = properties.and_then(|p| p.as_object()) {
        for (name, schema) in props {
            let param_type = schema
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("any")
                .to_string();

            let description = schema
                .get("description")
                .and_then(|d| d.as_str())
                .map(String::from);

            let default = schema.get("default").cloned();

            params.push(ParameterSchema {
                name: name.clone(),
                description,
                required: required.contains(name),
                param_type,
                default,
            });
        }
    }

    params
}

/// Provider registry that manages all providers
pub struct ProviderRegistry {
    providers: dashmap::DashMap<String, Box<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: dashmap::DashMap::new(),
        }
    }

    pub fn register(&self, provider: Box<dyn Provider>) {
        let name = provider.name().to_string();
        self.providers.insert(name, provider);
    }

    pub fn get(&self, name: &str) -> Option<dashmap::mapref::one::Ref<'_, String, Box<dyn Provider>>> {
        self.providers.get(name)
    }

    pub fn list(&self) -> Vec<String> {
        self.providers.iter().map(|e| e.key().clone()).collect()
    }

    pub fn list_by_type(&self, provider_type: ProviderType) -> Vec<String> {
        self.providers
            .iter()
            .filter(|e| e.provider_type() == provider_type)
            .map(|e| e.key().clone())
            .collect()
    }

    pub async fn list_all_tools(&self) -> McpResult<Vec<Tool>> {
        let mut all_tools = Vec::new();
        for entry in self.providers.iter() {
            match entry.list_tools().await {
                Ok(tools) => all_tools.extend(tools),
                Err(e) => {
                    tracing::warn!("Failed to list tools from {}: {}", entry.name(), e);
                }
            }
        }
        Ok(all_tools)
    }

    /// Find a tool by full name (provider.tool)
    pub async fn find_tool(&self, full_name: &str) -> McpResult<Option<(Tool, String)>> {
        // Parse provider name from full tool name
        let parts: Vec<&str> = full_name.splitn(2, '.').collect();
        if parts.len() != 2 {
            return Err(crate::utils::errors::McpError::InvalidRequest(
                format!("Invalid tool name format: {}. Use provider.tool_name", full_name)
            ));
        }

        let provider_name = parts[0];
        let tool_name = parts[1];

        if let Some(provider) = self.get(provider_name) {
            let tools = provider.list_tools().await?;
            if let Some(tool) = tools.into_iter().find(|t| t.display_name() == tool_name) {
                return Ok(Some((tool, provider_name.to_string())));
            }
        }

        Ok(None)
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_type_display() {
        assert_eq!(ProviderType::McpStdio.to_string(), "mcp-stdio");
        assert_eq!(ProviderType::Skill.to_string(), "skill");
    }

    #[test]
    fn test_tool_display_name() {
        let tool = Tool {
            name: "filesystem.read_file".to_string(),
            description: Some("Read a file".to_string()),
            provider: "filesystem".to_string(),
            provider_type: ProviderType::McpStdio,
            parameters: vec![],
            metadata: HashMap::new(),
        };

        assert_eq!(tool.display_name(), "read_file");
        assert_eq!(tool.snake_name(), "read_file");
    }

    #[test]
    fn test_tool_result() {
        let result = ToolResult::success("hello").unwrap();
        assert!(result.success);
        assert!(result.error.is_none());

        let result = ToolResult::error("something went wrong");
        assert!(!result.success);
        assert_eq!(result.error, Some("something went wrong".to_string()));
    }
}
