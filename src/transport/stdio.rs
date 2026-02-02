use crate::core::protocol::{JsonRpcRequest, JsonRpcResponse, RequestId};
use crate::core::SharedRequestIdGenerator;
use crate::sandbox::Sandbox;
use crate::transport::traits::Transport;
use crate::utils::errors::{McpError, McpResult};
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{oneshot, Mutex, RwLock};
use tracing::{debug, error, info, warn};

/// Stdio transport for MCP servers
pub struct StdioTransport {
    child: Arc<Mutex<Child>>,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Arc<DashMap<RequestId, oneshot::Sender<JsonRpcResponse>>>,
    is_connected: Arc<RwLock<bool>>,
    request_id_gen: SharedRequestIdGenerator,
}

impl StdioTransport {
    pub async fn new(
        command: impl Into<String>,
        args: Vec<String>,
        env: std::collections::HashMap<String, String>,
        sandbox: Arc<dyn Sandbox>,
    ) -> McpResult<Self> {
        let config = crate::config::McpServerConfig {
            name: "temp".to_string(),
            command: command.into(),
            args,
            env,
            tags: vec![],
            description: None,
            sandbox: crate::config::SandboxConfig::default(),
            runner: None,
        };

        let mut child = sandbox.spawn(&config).await?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::TransportError("Failed to open stdin".to_string()))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::TransportError("Failed to open stdout".to_string()))?;

        let transport = Self {
            child: Arc::new(Mutex::new(child)),
            stdin: Arc::new(Mutex::new(stdin)),
            pending: Arc::new(DashMap::new()),
            is_connected: Arc::new(RwLock::new(true)),
            request_id_gen: SharedRequestIdGenerator::new(),
        };

        // Start response reader task
        transport.start_reader(stdout).await;

        Ok(transport)
    }

    async fn start_reader(&self, stdout: ChildStdout) {
        let pending = self.pending.clone();
        let is_connected = self.is_connected.clone();

        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                debug!("Received: {}", line);

                match serde_json::from_str::<JsonRpcResponse>(&line) {
                    Ok(response) => {
                        if let Some(id) = response.id.clone() {
                            if let Some((_, tx)) = pending.remove(&id) {
                                let _ = tx.send(response);
                            } else {
                                warn!("Received response with unknown id: {:?}", id);
                            }
                        } else {
                            debug!("Received response without id, ignoring");
                        }
                    }
                    Err(e) => {
                        warn!("Failed to parse response: {}", e);
                    }
                }
            }

            info!("Stdio reader task ended");
            *is_connected.write().await = false;
            pending.clear();
        });
    }
}

#[async_trait]
impl Transport for StdioTransport {
    async fn send_request(&self, request: JsonRpcRequest) -> McpResult<JsonRpcResponse> {
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
        debug!("Sending: {}", json);

        // Write request
        {
            let mut stdin = self.stdin.lock().await;
            if let Err(e) = stdin.write_all(json.as_bytes()).await {
                self.pending.remove(&request_id);
                return Err(McpError::Io(e));
            }
            if let Err(e) = stdin.write_all(b"\n").await {
                self.pending.remove(&request_id);
                return Err(McpError::Io(e));
            }
            if let Err(e) = stdin.flush().await {
                self.pending.remove(&request_id);
                return Err(McpError::Io(e));
            }
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
            return Err(McpError::TransportError("Transport not connected".to_string()));
        }

        let mut request = request;
        request.id = None;

        let json = serde_json::to_string(&request)?;
        debug!("Sending notification: {}", json);

        let mut stdin = self.stdin.lock().await;
        stdin.write_all(json.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;

        Ok(())
    }

    async fn is_connected(&self) -> bool {
        *self.is_connected.read().await
    }

    async fn close(&self) -> McpResult<()> {
        let mut child = self.child.lock().await;

        // Try graceful shutdown
        if let Err(e) = child.start_kill() {
            warn!("Failed to kill child process: {}", e);
        }

        match tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await {
            Ok(Ok(status)) => info!("Child process exited with: {:?}", status),
            Ok(Err(e)) => error!("Failed to wait for child: {}", e),
            Err(_) => warn!("Timeout waiting for child process"),
        }

        *self.is_connected.write().await = false;
        self.pending.clear();
        Ok(())
    }
}
