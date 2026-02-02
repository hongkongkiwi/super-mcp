use crate::core::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::core::{RequestRouter, RoutingStrategy, ServerManager};
use axum::{
    extract::{Path, State},
    response::Json,
};
use std::sync::Arc;

pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

pub async fn mcp_handler(
    State(server_manager): State<Arc<ServerManager>>,
    Json(request): Json<JsonRpcRequest>,
) -> Result<Json<JsonRpcResponse>, crate::utils::errors::McpError> {
    let servers = server_manager.list_servers();
    if servers.is_empty() {
        return Err(crate::utils::errors::McpError::ServerNotFound(
            "No servers configured".to_string(),
        ));
    }

    let mut router = RequestRouter::new(RoutingStrategy::Capability);
    for name in servers {
        if let Some(server) = server_manager.get_server(&name) {
            router.register_server(name, server.config.tags.clone());
        }
    }

    let server_name = router.route(&request)?;

    let response = server_manager.send_request(&server_name, request).await?;

    Ok(Json(response))
}

pub async fn server_handler(
    Path(server_name): Path<String>,
    State(server_manager): State<Arc<ServerManager>>,
    Json(request): Json<JsonRpcRequest>,
) -> Result<Json<JsonRpcResponse>, crate::utils::errors::McpError> {
    let response = server_manager
        .send_request(&server_name, request)
        .await?;

    Ok(Json(response))
}
