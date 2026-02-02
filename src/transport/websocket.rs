//! WebSocket transport for MCP communication
//!
//! Provides bidirectional streaming communication over WebSocket.

use crate::core::protocol::{JsonRpcRequest, JsonRpcResponse, RequestId};
use crate::core::SharedRequestIdGenerator;
use crate::transport::traits::Transport;
use crate::utils::errors::{McpError, McpResult};
use async_trait::async_trait;
use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info};
use url::Url;

/// WebSocket transport for MCP servers
pub struct WebSocketTransport {
    /// Pending requests keyed by id
    pending: Arc<DashMap<RequestId, oneshot::Sender<JsonRpcResponse>>>,
    /// Connection status
    is_connected: Arc<RwLock<bool>>,
    /// Write handle for sending messages
    write_tx: mpsc::Sender<Message>,
    request_id_gen: SharedRequestIdGenerator,
}

impl WebSocketTransport {
    /// Create a new WebSocket transport
    pub async fn new(url: impl Into<String>) -> McpResult<Self> {
        let url = url
            .into()
            .parse::<Url>()
            .map_err(|e| McpError::TransportError(format!("Invalid URL: {}", e)))?;

        info!("Connecting to WebSocket: {}", url);

        let (ws_stream, _) = connect_async(url.as_str())
            .await
            .map_err(|e| McpError::TransportError(format!("WebSocket connection failed: {}", e)))?;

        let (write, read) = ws_stream.split();

        let (write_tx, mut write_rx) = mpsc::channel::<Message>(100);

        let is_connected = Arc::new(RwLock::new(true));
        let is_connected_clone = is_connected.clone();
        let pending: Arc<DashMap<RequestId, oneshot::Sender<JsonRpcResponse>>> =
            Arc::new(DashMap::new());

        // Spawn writer task
        let mut write = write;
        tokio::spawn(async move {
            while let Some(msg) = write_rx.recv().await {
                if let Err(e) = write.send(msg).await {
                    error!("WebSocket send error: {}", e);
                    break;
                }
            }
            *is_connected_clone.write().await = false;
        });

        // Spawn reader task
        let pending_clone = pending.clone();
        let is_connected_clone2 = is_connected.clone();
        tokio::spawn(async move {
            let mut read = read;
            while let Some(result) = read.next().await {
                match result {
                    Ok(msg) => {
                        if let Message::Text(text) = msg {
                            debug!("WebSocket received: {}", text);
                            
                            // Try to parse as response
                            match serde_json::from_str::<JsonRpcResponse>(&text) {
                                Ok(response) => {
                                    if let Some(id) = response.id.clone() {
                                        if let Some((_, tx)) = pending_clone.remove(&id) {
                                            let _ = tx.send(response);
                                        } else {
                                            debug!("Received WebSocket response with unknown id: {:?}", id);
                                        }
                                    } else {
                                        debug!("Received WebSocket response without id, ignoring");
                                    }
                                }
                                Err(e) => {
                                    debug!("Failed to parse WebSocket message: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("WebSocket read error: {}", e);
                        break;
                    }
                }
            }
            *is_connected_clone2.write().await = false;
            pending_clone.clear();
            info!("WebSocket reader task ended");
        });

        let transport = Self {
            pending,
            is_connected,
            write_tx,
            request_id_gen: SharedRequestIdGenerator::new(),
        };

        // Send initialize request
        transport.send_initialize().await?;

        info!("WebSocket transport connected");
        Ok(transport)
    }

    /// Send initialize request
    async fn send_initialize(&self) -> McpResult<()> {
        let init_request = JsonRpcRequest::new(
            "initialize",
            Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "super-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
        );

        let json = serde_json::to_string(&init_request)?;
        self.write_tx
            .send(Message::Text(json.into()))
            .await
            .map_err(|e| McpError::TransportError(format!("Failed to send init: {}", e)))?;

        Ok(())
    }

}

#[async_trait]
impl Transport for WebSocketTransport {
    async fn send_request(&self, request: JsonRpcRequest) -> McpResult<JsonRpcResponse> {
        if !self.is_connected().await {
            return Err(McpError::TransportError("WebSocket not connected".to_string()));
        }

        let mut request = request;
        if request.id.is_none() {
            request.id = Some(self.request_id_gen.next_id());
        }
        let request_id = request
            .id
            .clone()
            .ok_or_else(|| McpError::InvalidRequest("Missing request id".to_string()))?;

        let (tx, rx) = oneshot::channel();
        self.pending.insert(request_id.clone(), tx);

        let json = serde_json::to_string(&request)?;
        debug!("WebSocket sending: {}", json);

        if let Err(e) = self.write_tx.send(Message::Text(json.into())).await {
            self.pending.remove(&request_id);
            return Err(McpError::TransportError(format!("Failed to send: {}", e)));
        }

        // Wait for response
        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(McpError::TransportError("Response channel closed".to_string())),
            Err(_) => {
                self.pending.remove(&request_id);
                Err(McpError::Timeout(30000))
            }
        }
    }

    async fn send_notification(&self, request: JsonRpcRequest) -> McpResult<()> {
        if !self.is_connected().await {
            return Err(McpError::TransportError("WebSocket not connected".to_string()));
        }

        let mut request = request;
        request.id = None;

        let json = serde_json::to_string(&request)?;
        debug!("WebSocket sending notification: {}", json);

        self.write_tx
            .send(Message::Text(json.into()))
            .await
            .map_err(|e| McpError::TransportError(format!("Failed to send: {}", e)))?;

        Ok(())
    }

    async fn is_connected(&self) -> bool {
        *self.is_connected.read().await
    }

    async fn close(&self) -> McpResult<()> {
        info!("Closing WebSocket transport");

        // Send close frame
        let _ = self.write_tx.send(Message::Close(None)).await;

        *self.is_connected.write().await = false;
        self.pending.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_url_parsing() {
        let url = "ws://localhost:3000/mcp".to_string();
        let parsed = url.parse::<Url>();
        assert!(parsed.is_ok());
        
        let url = parsed.unwrap();
        assert_eq!(url.scheme(), "ws");
        assert_eq!(url.host_str(), Some("localhost"));
        assert_eq!(url.port(), Some(3000));
    }

    #[test]
    fn test_websocket_url_parsing_wss() {
        let url = "wss://example.com/mcp".to_string();
        let parsed = url.parse::<Url>();
        assert!(parsed.is_ok());
        
        let url = parsed.unwrap();
        assert_eq!(url.scheme(), "wss");
    }
}
