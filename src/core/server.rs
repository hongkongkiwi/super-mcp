use crate::config::McpServerConfig;
use crate::core::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::sandbox::{create_sandbox, Sandbox};
use crate::transport::{Transport, StdioTransport};
use crate::utils::errors::{McpError, McpResult};
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

/// Managed MCP server instance
pub struct ManagedServer {
    pub config: McpServerConfig,
    transport: Arc<RwLock<Box<dyn Transport>>>,
    _sandbox: Arc<dyn Sandbox>,
}

impl ManagedServer {
    pub async fn new(config: McpServerConfig) -> McpResult<Self> {
        let sandbox = create_sandbox(&config);
        let sandbox_arc: Arc<dyn Sandbox> = Arc::from(sandbox);

        let transport: Box<dyn Transport> = Box::new(
            StdioTransport::new(
                config.command.clone(),
                config.args.clone(),
                config.env.clone(),
                sandbox_arc.clone(),
            )
            .await?,
        );

        Ok(Self {
            config,
            transport: Arc::new(RwLock::new(transport)),
            _sandbox: sandbox_arc,
        })
    }

    pub async fn send_request(&self, request: JsonRpcRequest) -> McpResult<JsonRpcResponse> {
        let transport = self.transport.read().await;
        transport.send_request(request).await
    }

    pub async fn is_connected(&self) -> bool {
        self.transport.read().await.is_connected().await
    }

    pub async fn stop(&self) -> McpResult<()> {
        let transport = self.transport.read().await;
        transport.close().await
    }
}

/// Manages multiple MCP servers
pub struct ServerManager {
    servers: DashMap<String, ManagedServer>,
}

impl ServerManager {
    pub fn new() -> Self {
        Self {
            servers: DashMap::new(),
        }
    }

    pub async fn add_server(&self, config: McpServerConfig) -> McpResult<()> {
        let name = config.name.clone();
        info!("Adding server: {}", name);

        let server = ManagedServer::new(config).await?;
        self.servers.insert(name, server);

        Ok(())
    }

    pub async fn remove_server(&self, name: &str) -> McpResult<()> {
        info!("Removing server: {}", name);

        if let Some((_, server)) = self.servers.remove(name) {
            server.stop().await?;
        } else {
            return Err(McpError::ServerNotFound(name.to_string()));
        }

        Ok(())
    }

    pub fn get_server(&self, name: &str) -> Option<dashmap::mapref::one::Ref<'_, String, ManagedServer>> {
        self.servers.get(name)
    }

    pub async fn send_request(
        &self,
        server_name: &str,
        request: JsonRpcRequest,
    ) -> McpResult<JsonRpcResponse> {
        let server = self
            .servers
            .get(server_name)
            .ok_or_else(|| McpError::ServerNotFound(server_name.to_string()))?;

        server.send_request(request).await
    }

    pub fn list_servers(&self) -> Vec<String> {
        self.servers.iter().map(|entry| entry.key().clone()).collect()
    }

    pub async fn get_servers_by_tags(&self, tags: &[String]) -> Vec<String> {
        self.servers
            .iter()
            .filter(|entry| {
                let server_tags: std::collections::HashSet<_> =
                    entry.config.tags.iter().cloned().collect();
                tags.iter().any(|tag| server_tags.contains(tag))
            })
            .map(|entry| entry.key().clone())
            .collect()
    }

    pub async fn stop_all(&self) {
        for entry in self.servers.iter() {
            if let Err(e) = entry.stop().await {
                error!("Failed to stop server {}: {}", entry.key(), e);
            }
        }
        self.servers.clear();
    }
}

impl Default for ServerManager {
    fn default() -> Self {
        Self::new()
    }
}
