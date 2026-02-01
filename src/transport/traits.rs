use crate::core::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::utils::errors::McpResult;
use async_trait::async_trait;

/// Transport for MCP communication
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send a request and wait for response
    async fn send_request(&self, request: JsonRpcRequest) -> McpResult<JsonRpcResponse>;

    /// Send a notification (no response expected)
    async fn send_notification(&self, request: JsonRpcRequest) -> McpResult<()>;

    /// Check if transport is connected
    async fn is_connected(&self) -> bool;

    /// Close the transport
    async fn close(&self) -> McpResult<()>;
}

/// Transport factory trait
pub trait TransportFactory: Send + Sync {
    fn create(&self) -> Box<dyn Transport>;
}
