//! SSE (Server-Sent Events) transport for MCP communication
use crate::core::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::transport::traits::Transport;
use crate::utils::errors::{McpError, McpResult};
use async_trait::async_trait;
use futures::stream::StreamExt;
use reqwest::header::{ACCEPT, CACHE_CONTROL, CONTENT_TYPE};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info};
use url::Url;

/// SSE transport for MCP servers
pub struct SseTransport {
    endpoint: Url,
    client: reqwest::Client,
    session_id: Arc<RwLock<Option<String>>>,
    response_tx: mpsc::Sender<JsonRpcResponse>,
    response_rx: Arc<Mutex<mpsc::Receiver<JsonRpcResponse>>>,
    is_connected: Arc<RwLock<bool>>,
}

impl SseTransport {
    pub async fn new(endpoint: impl Into<String>) -> McpResult<Self> {
        let endpoint = endpoint
            .into()
            .parse::<Url>()
            .map_err(|e| McpError::TransportError(format!("Invalid URL: {}", e)))?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| McpError::TransportError(e.to_string()))?;

        let (response_tx, response_rx) = mpsc::channel(100);

        let transport = Self {
            endpoint,
            client,
            session_id: Arc::new(RwLock::new(None)),
            response_tx,
            response_rx: Arc::new(Mutex::new(response_rx)),
            is_connected: Arc::new(RwLock::new(false)),
        };

        // Connect to SSE endpoint
        transport.connect().await?;

        Ok(transport)
    }

    async fn connect(&self) -> McpResult<()> {
        info!("Connecting to SSE endpoint: {}", self.endpoint);

        // Send GET request to establish SSE connection
        let response = self
            .client
            .get(self.endpoint.clone())
            .header(ACCEPT, "text/event-stream")
            .header(CACHE_CONTROL, "no-cache")
            .send()
            .await
            .map_err(|e| McpError::TransportError(format!("Failed to connect: {}", e)))?;

        if !response.status().is_success() {
            return Err(McpError::TransportError(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        // Extract session ID from response headers if present
        if let Some(session_id) = response.headers().get("mcp-session-id") {
            if let Ok(id) = session_id.to_str() {
                *self.session_id.write().await = Some(id.to_string());
            }
        }

        // Start response reader
        self.start_reader(response).await;

        *self.is_connected.write().await = true;
        info!("SSE connection established");

        Ok(())
    }

    async fn start_reader(&self, response: reqwest::Response) {
        let response_tx = self.response_tx.clone();
        let is_connected = self.is_connected.clone();

        tokio::spawn(async move {
            let mut stream = response.bytes_stream();

            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                            // Parse SSE events (format: "data: {...}\n\n")
                            for line in text.lines() {
                                if let Some(data) = line.strip_prefix("data: ") {
                                    match serde_json::from_str::<JsonRpcResponse>(data) {
                                        Ok(response) => {
                                            if let Err(e) = response_tx.send(response).await {
                                                error!("Failed to send response: {}", e);
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            debug!("Failed to parse SSE data: {}", e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("SSE stream error: {}", e);
                        break;
                    }
                }
            }

            info!("SSE reader task ended");
            *is_connected.write().await = false;
        });
    }

    fn build_request_url(&self, session_id: Option<String>) -> Url {
        let mut url = self.endpoint.clone();

        if let Some(id) = session_id {
            url.query_pairs_mut()
                .append_pair("session_id", &id);
        }

        url
    }
}

#[async_trait]
impl Transport for SseTransport {
    async fn send_request(&self,
        request: JsonRpcRequest,
    ) -> McpResult<JsonRpcResponse> {
        if !self.is_connected().await {
            return Err(McpError::TransportError("Transport not connected".to_string()));
        }

        let json = serde_json::to_string(&request)?;
        debug!("Sending SSE request: {}", json);

        let session_id = self.session_id.read().await.clone();
        let url = self.build_request_url(session_id);

        let response = self
            .client
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json")
            .body(json)
            .send()
            .await
            .map_err(|e| McpError::TransportError(format!("Request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(McpError::TransportError(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        // Wait for response via SSE channel
        let mut rx = self.response_rx.lock().await;
        match tokio::time::timeout(std::time::Duration::from_secs(30), rx.recv()).await {
            Ok(Some(response)) => Ok(response),
            Ok(None) => Err(McpError::TransportError("Response channel closed".to_string())),
            Err(_) => Err(McpError::Timeout(30000)),
        }
    }

    async fn send_notification(&self,
        request: JsonRpcRequest,
    ) -> McpResult<()> {
        if !self.is_connected().await {
            return Err(McpError::TransportError("Transport not connected".to_string()));
        }

        let json = serde_json::to_string(&request)?;
        debug!("Sending SSE notification: {}", json);

        let session_id = self.session_id.read().await.clone();
        let url = self.build_request_url(session_id);

        let response = self
            .client
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .body(json)
            .send()
            .await
            .map_err(|e| McpError::TransportError(format!("Notification failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(McpError::TransportError(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        Ok(())
    }

    async fn is_connected(&self) -> bool {
        *self.is_connected.read().await
    }

    async fn close(&self) -> McpResult<()> {
        info!("Closing SSE transport");

        // Optionally send close message to server
        let session_id = self.session_id.read().await.clone();
        if let Some(_id) = session_id {
            // Could send DELETE request to close session
        }

        *self.is_connected.write().await = false;
        Ok(())
    }
}
