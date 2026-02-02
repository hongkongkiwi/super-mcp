//! SSE (Server-Sent Events) transport for MCP communication
use crate::core::protocol::{JsonRpcRequest, JsonRpcResponse, RequestId};
use crate::core::SharedRequestIdGenerator;
use crate::transport::traits::Transport;
use crate::utils::errors::{McpError, McpResult};
use async_trait::async_trait;
use dashmap::DashMap;
use futures::stream::StreamExt;
use reqwest::header::{ACCEPT, CACHE_CONTROL, CONTENT_TYPE};
use std::sync::Arc;
use tokio::sync::{oneshot, RwLock};
use tracing::{debug, error, info};
use url::Url;

/// SSE transport for MCP servers
pub struct SseTransport {
    endpoint: Url,
    client: reqwest::Client,
    session_id: Arc<RwLock<Option<String>>>,
    pending: Arc<DashMap<RequestId, oneshot::Sender<JsonRpcResponse>>>,
    is_connected: Arc<RwLock<bool>>,
    request_id_gen: SharedRequestIdGenerator,
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

        let transport = Self {
            endpoint,
            client,
            session_id: Arc::new(RwLock::new(None)),
            pending: Arc::new(DashMap::new()),
            is_connected: Arc::new(RwLock::new(false)),
            request_id_gen: SharedRequestIdGenerator::new(),
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
        let pending = self.pending.clone();
        let is_connected = self.is_connected.clone();

        tokio::spawn(async move {
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut event_data = String::new();

            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        buffer.push_str(&text);

                        while let Some(pos) = buffer.find('\n') {
                            let mut line = buffer[..pos].to_string();
                            buffer = buffer[pos + 1..].to_string();

                            if line.ends_with('\r') {
                                line.pop();
                            }

                            if line.is_empty() {
                                if !event_data.is_empty() {
                                    let payload = event_data.trim_end_matches('\n');
                                    match serde_json::from_str::<JsonRpcResponse>(payload) {
                                        Ok(response) => {
                                            if let Some(id) = response.id.clone() {
                                                if let Some((_, tx)) = pending.remove(&id) {
                                                    let _ = tx.send(response);
                                                } else {
                                                    debug!("Received SSE response with unknown id: {:?}", id);
                                                }
                                            } else {
                                                debug!("Received SSE response without id, ignoring");
                                            }
                                        }
                                        Err(e) => {
                                            debug!("Failed to parse SSE data: {}", e);
                                        }
                                    }
                                    event_data.clear();
                                }
                                continue;
                            }

                            if let Some(data) = line.strip_prefix("data:") {
                                let data = data.trim_start();
                                event_data.push_str(data);
                                event_data.push('\n');
                            }
                        }
                    }
                    Err(e) => {
                        error!("SSE stream error: {}", e);
                        break;
                    }
                }
            }

            if !event_data.is_empty() {
                let payload = event_data.trim_end_matches('\n');
                match serde_json::from_str::<JsonRpcResponse>(payload) {
                    Ok(response) => {
                        if let Some(id) = response.id.clone() {
                            if let Some((_, tx)) = pending.remove(&id) {
                                let _ = tx.send(response);
                            } else {
                                debug!("Received SSE response with unknown id: {:?}", id);
                            }
                        } else {
                            debug!("Received SSE response without id, ignoring");
                        }
                    }
                    Err(e) => {
                        debug!("Failed to parse SSE data: {}", e);
                    }
                }
            }

            info!("SSE reader task ended");
            *is_connected.write().await = false;
            pending.clear();
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
            self.pending.remove(&request_id);
            return Err(McpError::TransportError(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        // Wait for response via SSE channel
        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => Err(McpError::TransportError("Response channel closed".to_string())),
            Err(_) => {
                self.pending.remove(&request_id);
                Err(McpError::Timeout(30000))
            }
        }
    }

    async fn send_notification(&self,
        request: JsonRpcRequest,
    ) -> McpResult<()> {
        if !self.is_connected().await {
            return Err(McpError::TransportError("Transport not connected".to_string()));
        }

        let mut request = request;
        request.id = None;

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
        self.pending.clear();
        Ok(())
    }
}
