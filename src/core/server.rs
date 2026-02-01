use crate::config::McpServerConfig;
use crate::core::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::sandbox::{create_sandbox, Sandbox};
use crate::transport::{Transport, StdioTransport, SseTransport, StreamableHttpTransport};
use crate::utils::errors::{McpError, McpResult};
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

/// Transport type for MCP servers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportType {
    /// Standard input/output transport
    Stdio,
    /// Server-Sent Events transport
    Sse,
    /// Streamable HTTP transport
    StreamableHttp,
}

impl Default for TransportType {
    fn default() -> Self {
        TransportType::Stdio
    }
}

impl std::str::FromStr for TransportType {
    type Err = McpError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "stdio" => Ok(TransportType::Stdio),
            "sse" => Ok(TransportType::Sse),
            "streamable" | "streamable-http" | "streamable_http" => Ok(TransportType::StreamableHttp),
            _ => Err(McpError::ConfigError(format!("Unknown transport type: {}", s))),
        }
    }
}

/// Server status information
#[derive(Debug, Clone)]
pub struct ServerStatus {
    pub name: String,
    pub connected: bool,
    pub transport_type: TransportType,
    pub tags: Vec<String>,
    pub command: String,
}

/// Managed MCP server instance
pub struct ManagedServer {
    pub config: McpServerConfig,
    transport: Arc<RwLock<Box<dyn Transport>>>,
    _sandbox: Arc<dyn Sandbox>,
}

impl ManagedServer {
    /// Create a new managed server with stdio transport (default)
    pub async fn new(config: McpServerConfig) -> McpResult<Self> {
        Self::with_transport(config, TransportType::Stdio, None).await
    }

    /// Create a new managed server with specified transport
    pub async fn with_transport(
        config: McpServerConfig,
        transport_type: TransportType,
        endpoint: Option<String>,
    ) -> McpResult<Self> {
        let sandbox = create_sandbox(&config);
        let sandbox_arc: Arc<dyn Sandbox> = Arc::from(sandbox);

        let transport: Box<dyn Transport> = match transport_type {
            TransportType::Stdio => {
                Box::new(
                    StdioTransport::new(
                        config.command.clone(),
                        config.args.clone(),
                        config.env.clone(),
                        sandbox_arc.clone(),
                    )
                    .await?,
                )
            }
            TransportType::Sse => {
                let endpoint = endpoint.ok_or_else(|| {
                    McpError::ConfigError("SSE transport requires an endpoint URL".to_string())
                })?;
                Box::new(SseTransport::new(endpoint).await?)
            }
            TransportType::StreamableHttp => {
                let endpoint = endpoint.ok_or_else(|| {
                    McpError::ConfigError("Streamable HTTP transport requires an endpoint URL".to_string())
                })?;
                Box::new(StreamableHttpTransport::new(endpoint).await?)
            }
        };

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

    /// Get the transport type used by this server
    pub fn transport_type(&self) -> TransportType {
        TransportType::Stdio
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

    /// Add a server with a specific transport type
    pub async fn add_server_with_transport(
        &self,
        config: McpServerConfig,
        transport_type: TransportType,
        endpoint: Option<String>,
    ) -> McpResult<()> {
        let name = config.name.clone();
        info!("Adding server: {} with transport {:?}", name, transport_type);

        let server = ManagedServer::with_transport(config, transport_type, endpoint).await?;
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

    /// Get server status information
    pub async fn get_server_status(&self, name: &str) -> McpResult<ServerStatus> {
        let server = self
            .servers
            .get(name)
            .ok_or_else(|| McpError::ServerNotFound(name.to_string()))?;

        Ok(ServerStatus {
            name: name.to_string(),
            connected: server.is_connected().await,
            transport_type: server.transport_type(),
            tags: server.config.tags.clone(),
            command: format!("{} {}", server.config.command, server.config.args.join(" ")),
        })
    }

    /// Get status for all servers
    pub async fn get_all_server_status(&self) -> Vec<ServerStatus> {
        let mut statuses = Vec::new();

        for entry in self.servers.iter() {
            let status = ServerStatus {
                name: entry.key().clone(),
                connected: entry.is_connected().await,
                transport_type: entry.transport_type(),
                tags: entry.config.tags.clone(),
                command: format!("{} {}", entry.config.command, entry.config.args.join(" ")),
            };
            statuses.push(status);
        }

        statuses
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_transport_type_from_str() {
        assert_eq!(
            TransportType::from_str("stdio").unwrap(),
            TransportType::Stdio
        );
        assert_eq!(
            TransportType::from_str("SSE").unwrap(),
            TransportType::Sse
        );
        assert_eq!(
            TransportType::from_str("streamable-http").unwrap(),
            TransportType::StreamableHttp
        );
        assert!(TransportType::from_str("unknown").is_err());
    }

    #[test]
    fn test_server_status_display() {
        let status = ServerStatus {
            name: "test".to_string(),
            connected: true,
            transport_type: TransportType::Stdio,
            tags: vec!["test".to_string()],
            command: "echo hello".to_string(),
        };

        assert_eq!(status.name, "test");
        assert!(status.connected);
    }
}
