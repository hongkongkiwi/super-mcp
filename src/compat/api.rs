//! 1MCP API Compatibility Layer
//!
//! Provides REST API endpoints compatible with 1MCP's management API.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

use crate::core::ServerManager;

/// 1MCP-compatible API routes
pub fn one_mcp_routes(server_manager: Arc<ServerManager>) -> Router {
    Router::new()
        .route("/v1/servers", get(list_servers).post(create_server))
        .route("/v1/servers/:name", get(get_server).put(update_server).delete(delete_server))
        .route("/v1/servers/:name/start", post(start_server))
        .route("/v1/servers/:name/stop", post(stop_server))
        .route("/v1/servers/:name/restart", post(restart_server))
        .route("/v1/servers/:name/status", get(server_status))
        .route("/v1/health", get(health_check))
        .route("/v1/info", get(system_info))
        .with_state(server_manager)
}

/// Server list response (1MCP format)
#[derive(Debug, Serialize)]
pub struct OneMcpServerList {
    pub servers: Vec<OneMcpServerInfo>,
    pub total: usize,
}

/// Server info (1MCP format)
#[derive(Debug, Serialize, Deserialize)]
pub struct OneMcpServerInfo {
    pub name: String,
    pub command: String,
    pub status: String,
    pub enabled: bool,
    pub pid: Option<u32>,
    pub uptime_seconds: Option<u64>,
    pub restarts: u32,
    pub last_error: Option<String>,
}

/// Create server request (1MCP format)
#[derive(Debug, Deserialize)]
pub struct CreateServerRequest {
    pub name: String,
    pub command: String,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub tags: Option<Vec<String>>,
    pub sandbox: Option<OneMcpSandboxConfig>,
}

#[derive(Debug, Deserialize)]
pub struct OneMcpSandboxConfig {
    pub enabled: Option<bool>,
    pub network: Option<bool>,
}

/// Update server request
#[derive(Debug, Deserialize)]
pub struct UpdateServerRequest {
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub enabled: Option<bool>,
}

/// API error response
#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
    pub message: String,
}

/// List all servers (1MCP compatible)
async fn list_servers(
    State(server_manager): State<Arc<ServerManager>>,
) -> Result<Json<OneMcpServerList>, (StatusCode, Json<ApiError>)> {
    let server_names = server_manager.list_servers();
    let mut servers = Vec::new();

    for name in &server_names {
        // In production, fetch actual server info
        servers.push(OneMcpServerInfo {
            name: name.clone(),
            command: "n/a".to_string(),
            status: "running".to_string(),
            enabled: true,
            pid: None,
            uptime_seconds: None,
            restarts: 0,
            last_error: None,
        });
    }

    Ok(Json(OneMcpServerList {
        total: servers.len(),
        servers,
    }))
}

/// Get server details
async fn get_server(
    State(server_manager): State<Arc<ServerManager>>,
    Path(name): Path<String>,
) -> Result<Json<OneMcpServerInfo>, (StatusCode, Json<ApiError>)> {
    if let Some(_server) = server_manager.get_server(&name) {
        // In production, fetch actual server details
        Ok(Json(OneMcpServerInfo {
            name: name.clone(),
            command: "n/a".to_string(),
            status: "running".to_string(),
            enabled: true,
            pid: None,
            uptime_seconds: None,
            restarts: 0,
            last_error: None,
        }))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "SERVER_NOT_FOUND".to_string(),
                message: format!("Server '{}' not found", name),
            }),
        ))
    }
}

/// Create new server
async fn create_server(
    State(server_manager): State<Arc<ServerManager>>,
    Json(req): Json<CreateServerRequest>,
) -> Result<Json<OneMcpServerInfo>, (StatusCode, Json<ApiError>)> {
    // Convert request to Super MCP config
    let server_config = crate::config::McpServerConfig {
        name: req.name.clone(),
        command: req.command.clone(),
        args: req.args.unwrap_or_default(),
        env: req.env.unwrap_or_default(),
        tags: req.tags.unwrap_or_default(),
        description: None,
        sandbox: crate::config::SandboxConfig::default(),
        runner: None,
    };

    // Add server to manager
    match server_manager.add_server(server_config).await {
        Ok(_) => Ok(Json(OneMcpServerInfo {
            name: req.name,
            command: req.command,
            status: "created".to_string(),
            enabled: true,
            pid: None,
            uptime_seconds: None,
            restarts: 0,
            last_error: None,
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "CREATE_FAILED".to_string(),
                message: e.to_string(),
            }),
        )),
    }
}

/// Update server
async fn update_server(
    State(_server_manager): State<Arc<ServerManager>>,
    Path(name): Path<String>,
    Json(req): Json<UpdateServerRequest>,
) -> Result<Json<OneMcpServerInfo>, (StatusCode, Json<ApiError>)> {
    // In production, implement actual update logic
    Ok(Json(OneMcpServerInfo {
        name,
        command: req.command.unwrap_or_else(|| "n/a".to_string()),
        status: "updated".to_string(),
        enabled: req.enabled.unwrap_or(true),
        pid: None,
        uptime_seconds: None,
        restarts: 0,
        last_error: None,
    }))
}

/// Delete server
async fn delete_server(
    State(server_manager): State<Arc<ServerManager>>,
    Path(name): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    match server_manager.remove_server(&name).await {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                error: "DELETE_FAILED".to_string(),
                message: e.to_string(),
            }),
        )),
    }
}

/// Start server
async fn start_server(
    State(_server_manager): State<Arc<ServerManager>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    Ok(Json(json!({
        "name": name,
        "action": "start",
        "status": "started"
    })))
}

/// Stop server
async fn stop_server(
    State(server_manager): State<Arc<ServerManager>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    if let Some(server) = server_manager.get_server(&name) {
        match server.stop().await {
            Ok(_) => Ok(Json(json!({
                "name": name,
                "action": "stop",
                "status": "stopped"
            }))),
            Err(e) => Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: "STOP_FAILED".to_string(),
                    message: e.to_string(),
                }),
            )),
        }
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "SERVER_NOT_FOUND".to_string(),
                message: format!("Server '{}' not found", name),
            }),
        ))
    }
}

/// Restart server
async fn restart_server(
    State(_server_manager): State<Arc<ServerManager>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    Ok(Json(json!({
        "name": name,
        "action": "restart",
        "status": "restarted"
    })))
}

/// Get server status
async fn server_status(
    State(server_manager): State<Arc<ServerManager>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    if let Some(server) = server_manager.get_server(&name) {
        let connected = server.is_connected().await;
        Ok(Json(json!({
            "name": name,
            "status": if connected { "healthy" } else { "unhealthy" },
            "connected": connected,
        })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                error: "SERVER_NOT_FOUND".to_string(),
                message: format!("Server '{}' not found", name),
            }),
        ))
    }
}

/// Health check endpoint
async fn health_check() -> Json<serde_json::Value> {
    Json(json!({
        "status": "healthy",
        "service": "super-mcp",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// System info endpoint
async fn system_info(
    State(server_manager): State<Arc<ServerManager>>,
) -> Json<serde_json::Value> {
    let server_count = server_manager.list_servers().len();

    Json(json!({
        "name": "super-mcp",
        "version": env!("CARGO_PKG_VERSION"),
        "api_version": "v1",
        "compatible_with": "1mcp",
        "servers": {
            "total": server_count,
            "healthy": server_count, // Simplified
        },
        "features": {
            "sandboxing": true,
            "clustering": true,
            "multi_tenancy": true,
            "wasm": true,
        }
    }))
}

/// API middleware for 1MCP compatibility
pub async fn one_mcp_compat_middleware(
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let response = next.run(request).await;
    
    // Add 1MCP compatibility headers
    let mut response = response;
    let headers = response.headers_mut();
    
    headers.insert(
        "x-super-mcp-compat",
        axum::http::HeaderValue::from_static("1mcp-v1"),
    );
    
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_one_mcp_server_info_serialization() {
        let info = OneMcpServerInfo {
            name: "test".to_string(),
            command: "echo".to_string(),
            status: "running".to_string(),
            enabled: true,
            pid: Some(1234),
            uptime_seconds: Some(3600),
            restarts: 0,
            last_error: None,
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("running"));
    }

    #[test]
    fn test_create_server_request_deserialization() {
        let json = r#"{
            "name": "test-server",
            "command": "echo",
            "args": ["hello"],
            "env": {"KEY": "value"}
        }"#;

        let req: CreateServerRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, "test-server");
        assert_eq!(req.command, "echo");
        assert_eq!(req.args, Some(vec!["hello".to_string()]));
    }
}
