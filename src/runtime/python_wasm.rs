//! Python runtime implementation
//!
//! This module provides Python execution capabilities. For true WASM sandboxing,
//! it interfaces with an external Pyodide HTTP server. For native execution,
//! it uses Python with process-level sandboxing.

use crate::runtime::types::{
    ExecutionResult, ResourceLimits, RuntimeConfig, RuntimeError, RuntimeType,
};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tracing::{debug, info};

/// Python runtime configuration
#[derive(Debug, Clone)]
pub struct PythonWasmConfig {
    /// Use Pyodide HTTP server for WASM execution
    pub use_pyodide_server: bool,
    /// Pyodide server URL
    pub pyodide_server_url: String,
    /// Native Python command
    pub python_command: String,
    /// Working directory
    pub working_dir: PathBuf,
    /// Environment variables
    pub env: HashMap<String, String>,
    /// Resource limits
    pub resource_limits: ResourceLimits,
}

impl Default for PythonWasmConfig {
    fn default() -> Self {
        Self {
            use_pyodide_server: false,
            pyodide_server_url: "http://localhost:8000".to_string(),
            python_command: "python3".to_string(),
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            env: HashMap::new(),
            resource_limits: ResourceLimits::default(),
        }
    }
}

/// Python WASM runtime implementation
///
/// This runtime supports two modes:
/// 1. WASM mode: Connects to a Pyodide HTTP server for true WASM sandboxing
/// 2. Native mode: Uses native Python with process-level sandboxing
#[derive(Debug)]
pub struct PythonWasmRuntime {
    name: String,
    config: PythonWasmConfig,
}

impl PythonWasmRuntime {
    /// Create a new Python WASM runtime from configuration
    pub fn new(name: String, config: RuntimeConfig) -> Self {
        Self {
            name: name.clone(),
            config: PythonWasmConfig {
                use_pyodide_server: config.type_ == RuntimeType::PythonWasm,
                pyodide_server_url: "http://localhost:8000".to_string(),
                python_command: std::env::var("PYTHON_COMMAND")
                    .unwrap_or_else(|_| "python3".to_string()),
                working_dir: config.working_dir.map(PathBuf::from).unwrap_or_else(|| {
                    dirs::cache_dir()
                        .unwrap_or_else(|| PathBuf::from("/tmp"))
                        .join("super-mcp/python")
                }),
                env: config.env,
                resource_limits: config.resource_limits,
            },
        }
    }

    /// Create from environment-based configuration
    pub fn from_env(name: &str) -> Self {
        Self {
            name: name.to_string(),
            config: PythonWasmConfig::default(),
        }
    }

    /// Get the Python executable path
    fn find_python(&self) -> Result<String, RuntimeError> {
        // Check environment variable first
        if let Ok(cmd) = std::env::var("PYTHON_COMMAND") {
            if which::which(&cmd).is_ok() {
                return Ok(cmd);
            }
        }

        // Try common Python paths
        let candidates = ["python3", "python", "pypy3"];
        for cmd in &candidates {
            if let Ok(path) = which::which(cmd) {
                return Ok(path.to_string_lossy().to_string());
            }
        }

        Err(RuntimeError::RuntimeNotFound(
            "Python interpreter not found".to_string(),
        ))
    }

    /// Execute script using native Python with sandboxing
    async fn execute_native(
        &self,
        script: &str,
        _input: Option<Value>,
    ) -> Result<ExecutionResult, RuntimeError> {
        let start_time = Instant::now();

        // Create a temporary file for the script
        let temp_dir = &self.config.working_dir;
        tokio::fs::create_dir_all(temp_dir).await.map_err(|e| {
            RuntimeError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        })?;

        let script_path = temp_dir.join(format!("script_{}.py", uuid::Uuid::new_v4()));
        tokio::fs::write(&script_path, script).await.map_err(|e| {
            RuntimeError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        })?;

        debug!("Executing Python script at: {:?}", script_path);

        let python_cmd = self.find_python()?;

        // Build the command with sandboxing
        let mut cmd = Command::new(&python_cmd);
        cmd.arg(&script_path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Apply environment
        cmd.env_clear();
        for (key, value) in &self.config.env {
            cmd.env(key, value);
        }

        // Apply memory limit if possible
        #[cfg(target_os = "linux")]
        {
            if self.config.resource_limits.max_memory_mb > 0 {
                use std::os::unix::process::CommandExt;
                // Note: This requires the process to have CAP_SYS_RESOURCE or be root
                // In practice, memory limits are better applied via cgroups
                cmd.memory_max(self.config.resource_limits.max_memory_mb * 1024 * 1024);
            }
        }

        // Spawn the process
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                return Err(RuntimeError::ExecutionError(format!(
                    "Failed to spawn Python process: {}",
                    e
                )));
            }
        };

        // Wait for completion with timeout
        let timeout = Duration::from_secs(self.config.resource_limits.timeout_seconds);
        let result = tokio::time::timeout(timeout, child.wait()).await;

        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        let output = child.wait_with_output().await.map_err(|e| {
            RuntimeError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        })?;

        // Clean up temp file
        let _ = tokio::fs::remove_file(&script_path).await;

        Ok(ExecutionResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
            execution_time_ms,
            output_value: None,
        })
    }

    /// Execute script using Pyodide HTTP server (WASM sandbox)
    async fn execute_pyodide(
        &self,
        script: &str,
        _input: Option<Value>,
    ) -> Result<ExecutionResult, RuntimeError> {
        let start_time = Instant::now();

        // Prepare the request payload for Pyodide
        let payload = serde_json::json!({
            "code": script,
            "globals": {},
        });

        debug!(
            "Sending script to Pyodide server at: {}",
            self.config.pyodide_server_url
        );

        // Make HTTP request to Pyodide server
        let client = reqwest::Client::new();
        let response = client
            .post(&format!("{}/run", self.config.pyodide_server_url))
            .json(&payload)
            .timeout(Duration::from_secs(self.config.resource_limits.timeout_seconds))
            .send()
            .await
            .map_err(|e| {
                RuntimeError::ExecutionError(format!("Failed to connect to Pyodide server: {}", e))
            })?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(RuntimeError::ExecutionError(format!(
                "Pyodide server error: {}",
                error_text
            )));
        }

        let response_text = response.text().await.map_err(|e| {
            RuntimeError::ExecutionError(format!("Failed to read Pyodide response: {}", e))
        })?;

        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        // Parse Pyodide response
        // Pyodide returns: {"stdout": "...", "stderr": "...", "result": ...}
        let pyodide_response: serde_json::Value = serde_json::from_str(&response_text)
            .map_err(|e| RuntimeError::Serialization(e))?;

        let stdout = pyodide_response
            .get("stdout")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let stderr = pyodide_response
            .get("stderr")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let success = pyodide_response.get("error").is_none();

        Ok(ExecutionResult {
            success,
            stdout,
            stderr,
            exit_code: if success { 0 } else { 1 },
            execution_time_ms,
            output_value: pyodide_response.get("result").cloned(),
        })
    }
}

#[async_trait]
impl crate::runtime::types::Runtime for PythonWasmRuntime {
    fn name(&self) -> &str {
        &self.name
    }

    fn runtime_type(&self) -> RuntimeType {
        RuntimeType::PythonWasm
    }

    async fn validate(&self) -> Result<(), RuntimeError> {
        if self.config.use_pyodide_server {
            // Check if Pyodide server is reachable
            let client = reqwest::Client::new();
            let response = tokio::time::timeout(
                Duration::from_secs(5),
                client.get(&self.config.pyodide_server_url).send(),
            )
            .await;

            match response {
                Ok(Ok(_)) => {
                    info!("Pyodide server is available at: {}", self.config.pyodide_server_url);
                    Ok(())
                }
                Ok(Err(_)) | Err(_) => Err(RuntimeError::ValidationError(
                    "Pyodide server is not reachable".to_string(),
                )),
            }
        } else {
            // Check if Python is available
            self.find_python().map(|_| ()).map_err(|e| e)
        }
    }

    fn resource_limits(&self) -> &ResourceLimits {
        &self.config.resource_limits
    }

    async fn execute(
        &self,
        script: &str,
        input: Option<Value>,
    ) -> Result<ExecutionResult, RuntimeError> {
        if self.config.use_pyodide_server {
            self.execute_pyodide(script, input).await
        } else {
            self.execute_native(script, input).await
        }
    }

    async fn execute_file(
        &self,
        path: &PathBuf,
        input: Option<Value>,
    ) -> Result<ExecutionResult, RuntimeError> {
        let script = tokio::fs::read_to_string(path).await.map_err(|e| {
            RuntimeError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        })?;
        self.execute(&script, input).await
    }

    async fn install_packages(&self) -> Result<(), RuntimeError> {
        // Python packages are typically installed via pip
        // This would require pip to be available
        info!("Python runtime does not require package installation");
        Ok(())
    }
}

/// Check if Python is available on the system
pub fn check_python_available() -> bool {
    which::which("python3").is_ok() || which::which("python").is_ok()
}

/// Check if Pyodide server is available
pub async fn check_pyodide_available(url: &str) -> bool {
    let client = reqwest::Client::new();
    if let Ok(response) = tokio::time::timeout(
        Duration::from_secs(5),
        client.get(url).send(),
    )
    .await
    {
        return response.is_ok();
    }
    false
}
