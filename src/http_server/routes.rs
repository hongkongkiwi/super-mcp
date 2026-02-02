use crate::core::lazy_loader::ToolSchema;
use crate::core::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::core::{RequestRouter, RoutingStrategy};
use crate::http_server::server::AppState;
use axum::{
    extract::{Json, Path, Query, State},
    response::Json as AxumJson,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::debug;

/// Health check endpoint
pub async fn health() -> AxumJson<serde_json::Value> {
    AxumJson(serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// Main MCP handler - routes requests to appropriate servers
pub async fn mcp_handler(
    State(state): State<Arc<AppState>>,
    Json(request): Json<JsonRpcRequest>,
) -> Result<Json<JsonRpcResponse>, crate::utils::errors::McpError> {
    let servers = state.server_manager.list_servers();
    if servers.is_empty() {
        return Err(crate::utils::errors::McpError::ServerNotFound(
            "No servers configured".to_string(),
        ));
    }

    let mut router = RequestRouter::new(RoutingStrategy::Capability);
    for name in &servers {
        if let Some(server) = state.server_manager.get_server(name) {
            router.register_server(name.clone(), server.config.tags.clone());
        }
    }

    let server_name = router.route(&request)?;

    let response = state.server_manager.send_request(&server_name, request).await?;

    Ok(Json(response))
}

/// Server-specific MCP handler
pub async fn server_handler(
    Path(server_name): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(request): Json<JsonRpcRequest>,
) -> Result<Json<JsonRpcResponse>, crate::utils::errors::McpError> {
    let response = state
        .server_manager
        .send_request(&server_name, request)
        .await?;

    Ok(Json(response))
}

/// Tool list meta-tool - lists available tools with optional filtering
pub async fn tool_list_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<Value>,
) -> AxumJson<serde_json::Value> {
    let server_filter: Option<Vec<String>> = params
        .get("server")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|s| s.as_str().map(|s| s.to_string()))
                .collect()
        });

    let tag_filter: Option<Vec<String>> = params
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|s| s.as_str().map(|s| s.to_string()))
                .collect()
        });

    let tools_result: Result<Vec<ToolSchema>, _> = match &state.lazy_loader {
        Some(loader) => loader.list_tools(
            server_filter.as_ref().map(|v| v.as_slice()),
            tag_filter.as_ref().map(|v| v.as_slice()),
        ).await,
        None => {
            // Fallback to eager loading
            let mut all_tools = Vec::new();
            for server_name in state.server_manager.list_servers() {
                if let Some(server) = state.server_manager.get_server(&server_name) {
                    // Check filters
                    if let Some(ref servers) = server_filter {
                        if !servers.contains(&server_name) {
                            continue;
                        }
                    }
                    if let Some(ref tags) = tag_filter {
                        if !tags.iter().any(|tag| server.config.tags.contains(tag)) {
                            continue;
                        }
                    }

                    // Fetch tools from server
                    let request = JsonRpcRequest::new("tools/list", None);
                    match state.server_manager.send_request(&server_name, request).await {
                        Ok(response) => {
                            if let Some(result) = response.result {
                                if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
                                    for tool in tools {
                                        all_tools.push(ToolSchema {
                                            name: tool
                                                .get("name")
                                                .and_then(|n| n.as_str())
                                                .unwrap_or("")
                                                .to_string(),
                                            description: tool
                                                .get("description")
                                                .and_then(|d| d.as_str())
                                                .unwrap_or("")
                                                .to_string(),
                                            input_schema: tool
                                                .get("inputSchema")
                                                .cloned()
                                                .unwrap_or(json!({})),
                                            server_name: server_name.clone(),
                                        });
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            debug!("Failed to fetch tools from {}: {}", server_name, e);
                        }
                    }
                }
            }
            Ok(all_tools)
        }
    };

    match tools_result {
        Ok(tools) => AxumJson(json!({
            "tools": tools.iter().map(|t| json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema,
                "server": t.server_name,
            })).collect::<Vec<_>>(),
            "count": tools.len(),
        })),
        Err(e) => AxumJson(json!({
            "error": e.to_string(),
        })),
    }
}

/// Tool schema meta-tool - gets schema for a specific tool
pub async fn tool_schema_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<Value>,
) -> AxumJson<serde_json::Value> {
    let name = match params.get("name").and_then(|n| n.as_str()) {
        Some(n) => n.to_string(),
        None => {
            return AxumJson(json!({
                "error": "Missing required parameter: name"
            }));
        }
    };

    let server_filter = params.get("server").and_then(|s| s.as_str()).map(|s| s.to_string());

    // Look for tool across all servers
    let mut found: Option<(String, ToolSchema)> = None;
    let servers: Vec<String> = if let Some(s) = &server_filter {
        vec![s.clone()]
    } else {
        state.server_manager.list_servers()
    };

    for server_name in servers {
        if let Some(loader) = &state.lazy_loader {
            if let Ok(Some(schema)) = loader.get_tool_schema(&server_name, &name).await {
                found = Some((server_name.clone(), schema));
                break;
            }
        } else {
            // Fallback: fetch from server directly
            let request = JsonRpcRequest::new("tools/list", None);
            match state.server_manager.send_request(&server_name, request).await {
                Ok(response) => {
                    if let Some(result) = response.result {
                        if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
                            for tool in tools {
                                if tool.get("name").and_then(|n| n.as_str()) == Some(&name) {
                                    found = Some((
                                        server_name.clone(),
                                        ToolSchema {
                                            name: name.clone(),
                                            description: tool
                                                .get("description")
                                                .and_then(|d| d.as_str())
                                                .unwrap_or("")
                                                .to_string(),
                                            input_schema: tool
                                                .get("inputSchema")
                                                .cloned()
                                                .unwrap_or(json!({})),
                                            server_name: server_name.clone(),
                                        },
                                    ));
                                    break;
                                }
                            }
                        }
                    }
                }
                Err(_) => continue,
            }
        }
    }

    match found {
        Some((server_name, schema)) => AxumJson(json!({
            "name": schema.name,
            "description": schema.description,
            "inputSchema": schema.input_schema,
            "server": server_name,
        })),
        None => AxumJson(json!({
            "error": format!("Tool '{}' not found", name),
        })),
    }
}

/// Tool invoke meta-tool - invokes a tool on a specific server
pub async fn tool_invoke_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> Result<AxumJson<serde_json::Value>, crate::utils::errors::McpError> {
    let server = match body.get("server").and_then(|s| s.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return Ok(AxumJson(json!({
                "error": "Missing required parameter: server"
            })));
        }
    };

    let tool = match body.get("tool").and_then(|t| t.as_str()) {
        Some(t) => t.to_string(),
        None => {
            return Ok(AxumJson(json!({
                "error": "Missing required parameter: tool"
            })));
        }
    };

    let arguments = body.get("arguments").cloned().or(Some(json!({})));

    let request = JsonRpcRequest::new(
        "tools/call",
        Some(json!({
            "name": tool,
            "arguments": arguments,
        })),
    );

    let response = state.server_manager.send_request(&server, request).await?;

    match response.result {
        Some(result) => Ok(AxumJson(result)),
        None => {
            if let Some(error) = response.error {
                Ok(AxumJson(json!({
                    "error": error.message,
                    "code": error.code,
                })))
            } else {
                Ok(AxumJson(json!({
                    "error": "Unknown error occurred"
                })))
            }
        }
    }
}

/// List all configured servers
pub async fn list_servers_handler(
    State(state): State<Arc<AppState>>,
) -> AxumJson<serde_json::Value> {
    let servers = state.server_manager.list_servers();
    let mut server_info = Vec::new();

    for name in &servers {
        if let Some(server) = state.server_manager.get_server(name) {
            server_info.push(json!({
                "name": name,
                "tags": server.config.tags,
                "command": format!("{} {}", server.config.command, server.config.args.join(" ")),
                "description": server.config.description,
            }));
        }
    }

    AxumJson(json!({
        "servers": server_info,
        "count": server_info.len(),
    }))
}

/// Get server status
pub async fn server_status_handler(
    Path(server_name): Path<String>,
    State(state): State<Arc<AppState>>,
) -> AxumJson<serde_json::Value> {
    match state.server_manager.get_server_status(&server_name).await {
        Ok(status) => AxumJson(json!({
            "name": status.name,
            "connected": status.connected,
            "transport_type": format!("{:?}", status.transport_type),
            "tags": status.tags,
            "command": status.command,
        })),
        Err(e) => AxumJson(json!({
            "error": e.to_string(),
        })),
    }
}

/// Get cache statistics
pub async fn cache_stats_handler(
    State(state): State<Arc<AppState>>,
) -> AxumJson<serde_json::Value> {
    if let Some(loader) = &state.lazy_loader {
        let stats = loader.cache_stats();
        AxumJson(json!({
            "tools_count": stats.tools_count,
            "resources_count": stats.resources_count,
            "prompts_count": stats.prompts_count,
            "metrics": {
                "hits": stats.metrics.hits,
                "misses": stats.metrics.misses,
                "evictions": stats.metrics.evictions,
                "insertions": stats.metrics.insertions,
                "hit_rate_percent": stats.metrics.hit_rate(),
            },
        }))
    } else {
        AxumJson(json!({
            "message": "Lazy loading not enabled",
        }))
    }
}

/// Clear cache for a specific server or all
pub async fn cache_clear_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<Value>,
) -> AxumJson<serde_json::Value> {
    let server = params.get("server").and_then(|s| s.as_str());

    if let Some(loader) = &state.lazy_loader {
        if let Some(server_name) = server {
            loader.invalidate_cache(server_name);
            AxumJson(json!({
                "message": format!("Cache cleared for server: {}", server_name),
            }))
        } else {
            // Clear all caches
            loader.cache().clear_all();
            AxumJson(json!({
                "message": "All caches cleared",
            }))
        }
    } else {
        AxumJson(json!({
            "message": "Lazy loading not enabled",
        }))
    }
}
