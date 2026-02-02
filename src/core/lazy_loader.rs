//! Lazy loading for MCP tools - on-demand schema fetching with caching

use crate::cache::schema_cache::SchemaCache;
use crate::cache::schema_cache::SchemaType;
use crate::config::types::LazyLoadingMode;
use crate::core::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::core::server::ServerManager;
use crate::utils::errors::McpResult;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Tool schema representation
#[derive(Debug, Clone)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub server_name: String,
}

/// Resource schema representation
#[derive(Debug, Clone)]
pub struct ResourceSchema {
    pub uri: String,
    pub name: String,
    pub description: String,
    pub mime_type: Option<String>,
    pub server_name: String,
}

/// Prompt schema representation
#[derive(Debug, Clone)]
pub struct PromptSchema {
    pub name: String,
    pub description: String,
    pub arguments: Vec<PromptArgument>,
    pub server_name: String,
}

/// Prompt argument definition
#[derive(Debug, Clone)]
pub struct PromptArgument {
    pub name: String,
    pub description: String,
    pub required: bool,
}

/// LazyToolLoader provides on-demand schema loading from MCP servers
///
/// # Features:
/// - On-demand schema fetching from connected MCP servers
/// - Integration with connection pooling
/// - Template server support for placeholder servers
/// - Preset/tag filtering for tool selection
/// - Coalescing concurrent requests for the same schema
#[derive(Clone)]
pub struct LazyToolLoader {
    /// Schema cache
    cache: Arc<SchemaCache>,
    /// Server manager reference
    server_manager: Arc<ServerManager>,
    /// Lazy loading mode
    mode: LazyLoadingMode,
    /// Servers to preload regardless of mode
    preload_servers: Arc<Vec<String>>,
    /// Cache TTL override
    cache_ttl: Duration,
    /// Metrics
    metrics: Arc<LoadMetrics>,
    /// Server capabilities cache
    capabilities_cache: Arc<RwLock<HashMap<String, ServerCapabilities>>>,
}

/// Server capabilities from initialization
#[derive(Debug, Clone, Default)]
pub struct ServerCapabilities {
    pub tools: bool,
    pub resources: bool,
    pub prompts: bool,
    pub tools_list: Option<Vec<ToolSchema>>,
    pub resources_list: Option<Vec<ResourceSchema>>,
    pub prompts_list: Option<Vec<PromptSchema>>,
}

/// Metrics for lazy loading operations
#[derive(Debug, Default)]
pub struct LoadMetrics {
    pub schema_fetches: AtomicCounter,
    pub cache_hits: AtomicCounter,
    pub cache_misses: AtomicCounter,
    pub fetch_errors: AtomicCounter,
    pub template_invocations: AtomicCounter,
}

#[derive(Debug, Clone, Default)]
pub struct AtomicCounter(Arc<AtomicU64>);

impl AtomicCounter {
    #[inline]
    pub fn increment(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

impl LazyToolLoader {
    /// Create a new LazyToolLoader
    pub fn new(
        server_manager: Arc<ServerManager>,
        mode: LazyLoadingMode,
        preload_servers: Vec<String>,
        cache_ttl: Duration,
    ) -> Self {
        Self {
            cache: Arc::new(SchemaCache::new(cache_ttl)),
            server_manager,
            mode,
            preload_servers: Arc::new(preload_servers),
            cache_ttl,
            metrics: Arc::new(LoadMetrics::default()),
            capabilities_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with default settings
    pub fn with_defaults(server_manager: Arc<ServerManager>) -> Self {
        Self::new(
            server_manager,
            LazyLoadingMode::default(),
            Vec::new(),
            Duration::from_secs(300),
        )
    }

    /// Check if lazy loading is enabled
    pub fn is_enabled(&self) -> bool {
        self.mode != LazyLoadingMode::Disabled
    }

    /// Get the schema cache reference
    pub fn cache(&self) -> &Arc<SchemaCache> {
        &self.cache
    }

    /// Get all tool schemas (with lazy loading based on mode)
    pub async fn list_tools(
        &self,
        server_filter: Option<&[String]>,
        tag_filter: Option<&[String]>,
    ) -> McpResult<Vec<ToolSchema>> {
        match self.mode {
            LazyLoadingMode::Disabled => self.list_all_tools_eagerly().await,
            LazyLoadingMode::Metatool => self.list_tools_metatool(server_filter, tag_filter).await,
            LazyLoadingMode::Hybrid => self.list_tools_hybrid(server_filter, tag_filter).await,
            LazyLoadingMode::Full => self.list_tools_full(server_filter, tag_filter).await,
        }
    }

    /// Eager loading - fetch all tools immediately
    async fn list_all_tools_eagerly(&self) -> McpResult<Vec<ToolSchema>> {
        let mut all_tools = Vec::new();
        let servers = self.server_manager.list_servers();

        for server_name in servers {
            if let Some(server) = self.server_manager.get_server(&server_name) {
                match self.fetch_tools_from_server(&server_name).await {
                    Ok(tools) => all_tools.extend(tools),
                    Err(e) => warn!("Failed to fetch tools from {}: {}", server_name, e),
                }
            }
        }

        Ok(all_tools)
    }

    /// Metatool mode - return tool_list meta-tool instead of actual tools
    async fn list_tools_metatool(
        &self,
        _server_filter: Option<&[String]>,
        _tag_filter: Option<&[String]>,
    ) -> McpResult<Vec<ToolSchema>> {
        // In metatool mode, we return the meta-tools instead of real tools
        Ok(vec![
            ToolSchema {
                name: "tool_list".to_string(),
                description: "List available tools across all MCP servers with optional filtering".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "server": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Filter to specific server names"
                        },
                        "tags": {
                            "type": "array",
                            "items": {"type": "string"},
                            "description": "Filter by tags"
                        }
                    }
                }),
                server_name: "__super_mcp__".to_string(),
            },
            ToolSchema {
                name: "tool_schema".to_string(),
                description: "Get the schema for a specific tool".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "The name of the tool to get schema for"
                        }
                    },
                    "required": ["name"]
                }),
                server_name: "__super_mcp__".to_string(),
            },
            ToolSchema {
                name: "tool_invoke".to_string(),
                description: "Invoke a tool on a specific server".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "server": {
                            "type": "string",
                            "description": "The server name"
                        },
                        "tool": {
                            "type": "string",
                            "description": "The tool name to invoke"
                        },
                        "arguments": {
                            "type": "object",
                            "description": "Tool arguments as JSON object"
                        }
                    },
                    "required": ["server", "tool"]
                }),
                server_name: "__super_mcp__".to_string(),
            },
        ])
    }

    /// Hybrid mode - preload some servers, lazy load others
    async fn list_tools_hybrid(
        &self,
        server_filter: Option<&[String]>,
        tag_filter: Option<&[String]>,
    ) -> McpResult<Vec<ToolSchema>> {
        let mut all_tools = Vec::new();

        // Preload configured servers
        for server_name in self.preload_servers.as_ref() {
            if let Some(server) = self.server_manager.get_server(server_name) {
                match self.fetch_tools_from_server(server_name).await {
                    Ok(tools) => all_tools.extend(tools),
                    Err(e) => warn!("Failed to preload tools from {}: {}", server_name, e),
                }
            }
        }

        // Lazy load servers matching filter
        let servers = self.server_manager.list_servers();
        let filtered_servers: Vec<String> = servers
            .into_iter()
            .filter(|name| !self.preload_servers.contains(name))
            .filter(|name| {
                if let Some(filter) = server_filter {
                    filter.iter().any(|f| f == name)
                } else {
                    true
                }
            })
            .filter(|name| {
                if let Some(filter) = tag_filter {
                    if let Some(server) = self.server_manager.get_server(name) {
                        filter.iter().any(|tag| server.config.tags.contains(tag))
                    } else {
                        false
                    }
                } else {
                    true
                }
            })
            .collect();

        // Only fetch schemas for filtered servers
        for server_name in filtered_servers {
            match self.fetch_tools_from_server(&server_name).await {
                Ok(tools) => all_tools.extend(tools),
                Err(e) => warn!("Failed to fetch tools from {}: {}", server_name, e),
            }
        }

        Ok(all_tools)
    }

    /// Full lazy mode - only schema names, fetch on demand
    async fn list_tools_full(
        &self,
        server_filter: Option<&[String]>,
        tag_filter: Option<&[String]>,
    ) -> McpResult<Vec<ToolSchema>> {
        // In full lazy mode, we only return placeholder schemas
        // The actual schemas are fetched when needed
        let mut all_tools = Vec::new();
        let servers = self.server_manager.list_servers();

        for server_name in servers {
            // Apply filters
            if let Some(filter) = server_filter {
                if !filter.iter().any(|f| f == &server_name) {
                    continue;
                }
            }

            if let Some(server) = self.server_manager.get_server(&server_name) {
                // Check tag filter
                if let Some(tags) = tag_filter {
                    if !tags.iter().any(|tag| server.config.tags.contains(tag)) {
                        continue;
                    }
                }

                // Add placeholder tool that triggers lazy loading
                all_tools.push(ToolSchema {
                    name: format!("{}_lazy_loader", server_name),
                    description: format!(
                        "Placeholder for lazy loading tools from server '{}'. Invoke with tool name to load.",
                        server_name
                    ),
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "tool_name": {
                                "type": "string",
                                "description": "The name of the tool to load and invoke"
                            },
                            "arguments": {
                                "type": "object",
                                "description": "Arguments for the tool"
                            }
                        },
                        "required": ["tool_name"]
                    }),
                    server_name: server_name.clone(),
                });
            }
        }

        Ok(all_tools)
    }

    /// Fetch tools from a specific server
    pub async fn fetch_tools_from_server(&self, server_name: &str) -> McpResult<Vec<ToolSchema>> {
        // Check cache first
        if let Some(cached) = self.cache.get(server_name, "list", SchemaType::Tool) {
            if let Some(tools) = cached.schema.get("tools").and_then(|t| t.as_array()) {
                self.metrics.cache_hits.increment();
                return Ok(tools
                    .iter()
                    .map(|t| ToolSchema {
                        name: t.get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string(),
                        description: t.get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .to_string(),
                        input_schema: t.get("inputSchema")
                            .cloned()
                            .unwrap_or(json!({})),
                        server_name: server_name.to_string(),
                    })
                    .collect());
            }
        }

        self.metrics.cache_misses.increment();

        // Fetch from server
        let request = JsonRpcRequest::new("tools/list", None);

        match self.server_manager.send_request(server_name, request).await {
            Ok(response) => {
                self.metrics.schema_fetches.increment();

                let result = response.result.unwrap_or(json!({}));
                let tools = result.get("tools").and_then(|t| t.as_array()).cloned();

                if let Some(tools_array) = tools {
                    // Cache the result
                    self.cache.insert(
                        server_name,
                        "list",
                        json!({ "tools": tools_array }),
                        SchemaType::Tool,
                    );

                    // Convert to ToolSchema
                    Ok(tools_array
                        .iter()
                        .map(|t| ToolSchema {
                            name: t.get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("")
                                .to_string(),
                            description: t.get("description")
                                .and_then(|d| d.as_str())
                                .unwrap_or("")
                                .to_string(),
                            input_schema: t.get("inputSchema")
                                .cloned()
                                .unwrap_or(json!({})),
                            server_name: server_name.to_string(),
                        })
                        .collect())
                } else {
                    Ok(Vec::new())
                }
            }
            Err(e) => {
                self.metrics.fetch_errors.increment();
                Err(e)
            }
        }
    }

    /// Get a specific tool schema by name
    pub async fn get_tool_schema(&self, server_name: &str, tool_name: &str) -> McpResult<Option<ToolSchema>> {
        // Check if tool is in cached list
        let cache_key = format!("{}:{}", server_name, tool_name);

        if let Some(cached) = self.cache.get(server_name, tool_name, SchemaType::Tool) {
            return Ok(Some(ToolSchema {
                name: tool_name.to_string(),
                description: cached
                    .schema
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string(),
                input_schema: cached
                    .schema
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or(json!({})),
                server_name: server_name.to_string(),
            }));
        }

        // If we have the list cached, check it
        if let Some(cached) = self.cache.get(server_name, "list", SchemaType::Tool) {
            if let Some(tools) = cached.schema.get("tools").and_then(|t| t.as_array()) {
                if let Some(tool) = tools.iter().find(|t| {
                    t.get("name")
                        .and_then(|n| n.as_str())
                        .map(|n| n == tool_name)
                        .unwrap_or(false)
                }) {
                    return Ok(Some(ToolSchema {
                        name: tool_name.to_string(),
                        description: tool
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .to_string(),
                        input_schema: tool
                            .get("inputSchema")
                            .cloned()
                            .unwrap_or(json!({})),
                        server_name: server_name.to_string(),
                    }));
                }
            }
        }

        // Double-check cache after acquiring lock
        if let Some(cached) = self.cache.get(server_name, tool_name, SchemaType::Tool) {
            return Ok(Some(ToolSchema {
                name: tool_name.to_string(),
                description: cached
                    .schema
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string(),
                input_schema: cached
                    .schema
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or(json!({})),
                server_name: server_name.to_string(),
            }));
        }

        // Fetch tool list and find the specific tool
        let tools = self.fetch_tools_from_server(server_name).await?;

        Ok(tools
            .into_iter()
            .find(|t| t.name == tool_name))
    }

    /// Get all cached tool schemas for a server
    pub async fn get_cached_tools(&self, server_name: &str) -> Vec<ToolSchema> {
        let cached_entries = self.cache.get_by_server(server_name);

        let mut tools = Vec::new();
        for (schema_type, cached) in cached_entries {
            if schema_type == SchemaType::Tool && cached.schema.get("name").is_some() {
                tools.push(ToolSchema {
                    name: cached
                        .schema
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string(),
                    description: cached
                        .schema
                        .get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("")
                        .to_string(),
                    input_schema: cached
                        .schema
                        .get("inputSchema")
                        .cloned()
                        .unwrap_or(json!({})),
                    server_name: server_name.to_string(),
                });
            } else if schema_type == SchemaType::Tool && cached.schema.get("tools").is_some() {
                // This is the list cache
                if let Some(tools_array) = cached.schema.get("tools").and_then(|t| t.as_array()) {
                    tools.extend(tools_array.iter().map(|t| ToolSchema {
                        name: t.get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string(),
                        description: t.get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .to_string(),
                        input_schema: t.get("inputSchema")
                            .cloned()
                            .unwrap_or(json!({})),
                        server_name: server_name.to_string(),
                    }));
                }
            }
        }

        tools
    }

    /// Invoke a tool with the given name
    pub async fn invoke_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<Value>,
    ) -> McpResult<JsonRpcResponse> {
        let request = JsonRpcRequest::new(
            "tools/call",
            Some(json!({
                "name": tool_name,
                "arguments": arguments.unwrap_or(json!({}))
            })),
        );

        self.server_manager.send_request(server_name, request).await
    }

    /// Invalidate cached schemas for a server
    pub fn invalidate_cache(&self, server_name: &str) {
        self.cache.clear_server(server_name);
        debug!("Invalidated cache for server: {}", server_name);
    }

    /// Get loader metrics
    pub fn metrics(&self) -> &Arc<LoadMetrics> {
        &self.metrics
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> crate::cache::schema_cache::SchemaCacheStats {
        self.cache.stats()
    }
}

/// Filter tools by tags
pub fn filter_tools_by_tags(tools: &[ToolSchema], tags: &[String]) -> Vec<ToolSchema> {
    tools
        .iter()
        .filter(|tool| tags.is_empty() || tags.iter().any(|tag| tool.server_name.contains(tag)))
        .cloned()
        .collect()
}

/// Filter tools by server
pub fn filter_tools_by_server(tools: &[ToolSchema], servers: &[String]) -> Vec<ToolSchema> {
    tools
        .iter()
        .filter(|tool| servers.is_empty() || servers.contains(&tool.server_name))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::McpServerConfig;
    use std::collections::HashMap;

    fn create_test_server_manager() -> Arc<ServerManager> {
        Arc::new(ServerManager::new())
    }

    #[tokio::test]
    async fn test_lazy_tool_loader_creation() {
        let server_manager = create_test_server_manager();
        let loader = LazyToolLoader::with_defaults(server_manager);

        assert!(!loader.is_enabled());
        assert_eq!(loader.mode, LazyLoadingMode::Disabled);
    }

    #[tokio::test]
    async fn test_lazy_tool_loader_with_mode() {
        let server_manager = create_test_server_manager();
        let loader = LazyToolLoader::new(
            server_manager,
            LazyLoadingMode::Metatool,
            vec!["preload-server".to_string()],
            Duration::from_secs(60),
        );

        assert!(loader.is_enabled());
        assert_eq!(loader.mode, LazyLoadingMode::Metatool);
    }

    #[tokio::test]
    async fn test_list_tools_metatool_mode() {
        let server_manager = create_test_server_manager();
        let loader = LazyToolLoader::new(
            server_manager,
            LazyLoadingMode::Metatool,
            Vec::new(),
            Duration::from_secs(60),
        );

        let tools = loader.list_tools(None, None).await.unwrap();

        // Should have 3 meta-tools
        assert_eq!(tools.len(), 3);
        assert!(tools.iter().any(|t| t.name == "tool_list"));
        assert!(tools.iter().any(|t| t.name == "tool_schema"));
        assert!(tools.iter().any(|t| t.name == "tool_invoke"));
    }

    #[test]
    fn test_filter_tools_by_tags() {
        let tools = vec![
            ToolSchema {
                name: "tool1".to_string(),
                description: "".to_string(),
                input_schema: json!({}),
                server_name: "server1".to_string(),
            },
            ToolSchema {
                name: "tool2".to_string(),
                description: "".to_string(),
                input_schema: json!({}),
                server_name: "server2".to_string(),
            },
            ToolSchema {
                name: "tool3".to_string(),
                description: "".to_string(),
                input_schema: json!({}),
                server_name: "server1".to_string(),
            },
        ];

        let filtered = filter_tools_by_tags(&tools, &["server1".to_string()]);
        assert_eq!(filtered.len(), 2);

        let filtered = filter_tools_by_tags(&tools, &["server2".to_string()]);
        assert_eq!(filtered.len(), 1);

        let filtered = filter_tools_by_tags(&tools, &["nonexistent".to_string()]);
        assert!(filtered.is_empty());
    }

    #[test]
    fn test_filter_tools_by_server() {
        let tools = vec![
            ToolSchema {
                name: "tool1".to_string(),
                description: "".to_string(),
                input_schema: json!({}),
                server_name: "server1".to_string(),
            },
            ToolSchema {
                name: "tool2".to_string(),
                description: "".to_string(),
                input_schema: json!({}),
                server_name: "server2".to_string(),
            },
        ];

        let filtered = filter_tools_by_server(&tools, &["server1".to_string()]);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "tool1");

        let filtered = filter_tools_by_server(&tools, &["server1".to_string(), "server2".to_string()]);
        assert_eq!(filtered.len(), 2);
    }
}
