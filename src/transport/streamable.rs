//! Streamable HTTP transport for MCP communication
//!
//! This transport uses HTTP POST requests with streaming JSON responses.
//! Multiple JSON-RPC messages can be received in a single response,
//! separated by newlines (newline-delimited JSON).

use crate::core::protocol::{JsonRpcRequest, JsonRpcResponse, RequestId};
use crate::core::SharedRequestIdGenerator;
use crate::transport::traits::Transport;
use crate::utils::errors::{McpError, McpResult};
use async_trait::async_trait;
use dashmap::DashMap;
use futures::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{oneshot, RwLock};
use tracing::{debug, info, warn};
use url::Url;

/// Streamable HTTP transport for MCP servers
pub struct StreamableHttpTransport {
    endpoint: Url,
    client: reqwest::Client,
    session_id: Arc<RwLock<Option<String>>>,
    pending: Arc<DashMap<RequestId, oneshot::Sender<JsonRpcResponse>>>,
    is_connected: Arc<RwLock<bool>>,
    request_id_gen: SharedRequestIdGenerator,
}

impl StreamableHttpTransport {
    pub async fn new(endpoint: impl Into<String>) -> McpResult<Self> {
        let endpoint = endpoint
            .into()
            .parse::<Url>()
            .map_err(|e| McpError::TransportError(format!("Invalid URL: {}", e)))?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .pool_max_idle_per_host(10)
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

        // Initialize connection
        transport.initialize().await?;

        Ok(transport)
    }

    async fn initialize(&self) -> McpResult<()> {
        info!("Initializing Streamable HTTP transport: {}", self.endpoint);

        // Send initialize request to establish session
        let mut init_request = JsonRpcRequest::new(
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

        if init_request.id.is_none() {
            init_request.id = Some(self.request_id_gen.next_id());
        }
        let request_id = init_request
            .id
            .clone()
            .ok_or_else(|| McpError::InvalidRequest("Missing request id".to_string()))?;

        let (tx, rx) = oneshot::channel();
        self.pending.insert(request_id.clone(), tx);

        let json = serde_json::to_string(&init_request)?;

        let response = self
            .client
            .post(self.endpoint.clone())
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/x-ndjson")
            .body(json)
            .send()
            .await
            .map_err(|e| McpError::TransportError(format!("Initialize failed: {}", e)))?;

        if !response.status().is_success() {
            self.pending.remove(&request_id);
            return Err(McpError::TransportError(format!(
                "HTTP error during initialize: {}",
                response.status()
            )));
        }

        // Extract session ID from response headers if present
        if let Some(session_id) = response.headers().get("mcp-session-id") {
            if let Ok(id) = session_id.to_str() {
                *self.session_id.write().await = Some(id.to_string());
                info!("Streamable HTTP session established: {}", id);
            }
        }

        // Start response reader for streaming responses
        self.start_reader(response).await;

        *self.is_connected.write().await = true;
        info!("Streamable HTTP transport initialized");

        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(Ok(_response)) => Ok(()),
            Ok(Err(_)) => Err(McpError::TransportError("Initialize response channel closed".to_string())),
            Err(_) => {
                self.pending.remove(&request_id);
                Err(McpError::Timeout(30000))
            }
        }
    }

    async fn start_reader(&self, response: reqwest::Response) {
        let pending = self.pending.clone();

        tokio::spawn(async move {
            // Get the response bytes as a stream
            let stream = response.bytes_stream();

            // Convert to lines
            let reader = tokio_util::io::StreamReader::new(stream.map(|result| {
                result.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
            }));

            let buf_reader = BufReader::new(reader);
            let mut lines = buf_reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }

                debug!("Received streamable line: {}", line);

                match serde_json::from_str::<JsonRpcResponse>(&line) {
                    Ok(response) => {
                        if let Some(id) = response.id.clone() {
                            if let Some((_, tx)) = pending.remove(&id) {
                                let _ = tx.send(response);
                            } else {
                                debug!("Received streamable response with unknown id: {:?}", id);
                            }
                        } else {
                            debug!("Received streamable response without id, ignoring");
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse streamable response: {}", e);
                    }
                }
            }

            info!("Streamable HTTP reader task ended");
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
impl Transport for StreamableHttpTransport {
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
        debug!("Sending streamable request: {}", json);

        let session_id = self.session_id.read().await.clone();
        let url = self.build_request_url(session_id);

        let response = self
            .client
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/x-ndjson")
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

        // Start reader for this response stream
        self.start_reader(response).await;

        // Wait for response via channel
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
        debug!("Sending streamable notification: {}", json);

        let session_id = self.session_id.read().await.clone();
        let url = self.build_request_url(session_id);

        let response = self
            .client
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/x-ndjson")
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

        // For notifications, we don't wait for a response
        // But we do start a reader to handle any async responses
        self.start_reader(response).await;

        Ok(())
    }

    async fn is_connected(&self) -> bool {
        *self.is_connected.read().await
    }

    async fn close(&self) -> McpResult<()> {
        info!("Closing Streamable HTTP transport");

        // Optionally send session termination request
        let session_id = self.session_id.read().await.clone();
        if let Some(id) = session_id {
            let _ = self
                .client
                .delete(self.endpoint.clone())
                .query(&[("session_id", id)])
                .send()
                .await;
        }

        *self.is_connected.write().await = false;
        self.pending.clear();
        Ok(())
    }
}
