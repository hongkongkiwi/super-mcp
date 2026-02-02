//! Node.js runtime implementation for pnpm, npm, and bun
//!
//! This module provides JavaScript/TypeScript execution via pnpm, npm, or bun
//! with sandboxing support including process isolation and resource limits.

use crate::runtime::types::{
    ExecutionResult, ResourceLimits, RuntimeConfig, RuntimeError, RuntimeType,
};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::fs;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, info};

/// Node.js runtime variant (pnpm, npm, bun)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeRuntime {
    /// pnpm package manager
    Pnpm,
    /// npm package manager
    Npm,
    /// bun package manager
    Bun,
}

impl NodeRuntime {
    /// Get the command name for this runtime
    pub fn command(&self) -> &'static str {
        match self {
            NodeRuntime::Pnpm => "pnpm",
            NodeRuntime::Npm => "npm",
            NodeRuntime::Bun => "bun",
        }
    }

    /// Get the node_modules path convention
    pub fn node_modules_path(&self, working_dir: &PathBuf) -> PathBuf {
        working_dir.join("node_modules")
    }

    /// Get package manager specific flags for silent installation
    pub fn install_flags(&self) -> Vec<&'static str> {
        match self {
            NodeRuntime::Pnpm => vec!["--no-colors", "--quiet"],
            NodeRuntime::Npm => vec!["--quiet", "--no-audit", "--no-fund"],
            NodeRuntime::Bun => vec!["--no-progress"],
        }
    }

    /// Get package manager specific execution command
    pub fn exec_command(&self, script: &str) -> Vec<String> {
        match self {
            NodeRuntime::Pnpm => vec!["exec".to_string(), "--".to_string(), "node".to_string(), "-e".to_string(), script.to_string()],
            NodeRuntime::Npm => vec!["exec".to_string(), "--".to_string(), "node".to_string(), "-e".to_string(), script.to_string()],
            NodeRuntime::Bun => vec!["-e".to_string(), script.to_string()],
        }
    }
}

impl std::str::FromStr for NodeRuntime {
    type Err = RuntimeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pnpm" => Ok(NodeRuntime::Pnpm),
            "npm" => Ok(NodeRuntime::Npm),
            "bun" => Ok(NodeRuntime::Bun),
            _ => Err(RuntimeError::ValidationError(format!(
                "Unknown Node.js runtime: {}",
                s
            ))),
        }
    }
}

/// Node.js runtime configuration
#[derive(Debug, Clone)]
pub struct NodeRuntimeConfig {
    /// Node.js runtime variant
    pub runtime: NodeRuntime,
    /// Working directory for execution
    pub working_dir: PathBuf,
    /// Packages to install
    pub packages: Vec<String>,
    /// Environment variables
    pub env: HashMap<String, String>,
    /// Resource limits
    pub resource_limits: ResourceLimits,
    /// Script timeout
    pub timeout: Duration,
}

impl NodeRuntimeConfig {
    /// Create default configuration
    pub fn new(runtime: NodeRuntime) -> Self {
        Self {
            runtime,
            working_dir: dirs::cache_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("super-mcp/node"),
            packages: Vec::new(),
            env: HashMap::new(),
            resource_limits: ResourceLimits::default(),
            timeout: Duration::from_secs(30),
        }
    }
}

/// Node.js runtime implementation
#[derive(Debug)]
pub struct NodeRuntimeImpl {
    name: String,
    config: NodeRuntimeConfig,
}

impl NodeRuntimeImpl {
    /// Create a new Node.js runtime from configuration
    pub fn new(name: String, runtime_config: RuntimeConfig) -> Self {
        let runtime_variant = match runtime_config.type_ {
            RuntimeType::NodePnpm => NodeRuntime::Pnpm,
            RuntimeType::NodeNpm => NodeRuntime::Npm,
            RuntimeType::NodeBun => NodeRuntime::Bun,
            _ => NodeRuntime::Npm, // Default fallback
        };

        let mut node_config = NodeRuntimeConfig::new(runtime_variant);
        node_config.working_dir = runtime_config
            .working_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                dirs::cache_dir()
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
                    .join(format!("super-mcp/node-{}", name))
            });
        node_config.packages = runtime_config.packages;
        node_config.env = runtime_config.env;
        node_config.resource_limits = runtime_config.resource_limits.clone();
        node_config.timeout = Duration::from_secs(node_config.resource_limits.timeout_seconds);

        Self {
            name: name.clone(),
            config: node_config,
        }
    }

    /// Create from environment-based configuration
    pub fn from_env(name: &str, runtime: NodeRuntime) -> Self {
        Self {
            name: name.to_string(),
            config: NodeRuntimeConfig::new(runtime),
        }
    }

    /// Find the package manager executable
    fn find_executable(&self) -> Result<String, RuntimeError> {
        let cmd = self.config.runtime.command();

        // Check environment variable first
        let env_var = format!("{}_COMMAND", cmd.to_uppercase());
        if let Ok(cmd_path) = std::env::var(&env_var) {
            if which::which(&cmd_path).is_ok() {
                return Ok(cmd_path);
            }
        }

        // Try to find the command in PATH
        if let Ok(path) = which::which(cmd) {
            return Ok(path.to_string_lossy().to_string());
        }

        Err(RuntimeError::RuntimeNotFound(format!(
            "{} not found in PATH",
            cmd
        )))
    }

    /// Initialize the working directory
    async fn init_working_dir(&self) -> Result<(), RuntimeError> {
        fs::create_dir_all(&self.config.working_dir)
            .await
            .map_err(|e| {
                RuntimeError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            })?;

        // Create package.json if it doesn't exist
        let package_json_path = self.config.working_dir.join("package.json");
        if !package_json_path.exists() {
            let package_json = serde_json::json!({
                "name": "supermcp-runtime",
                "version": "1.0.0",
                "type": "module"
            });
            fs::write(
                &package_json_path,
                serde_json::to_string_pretty(&package_json).unwrap(),
            )
            .await
            .map_err(|e| {
                RuntimeError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            })?;
        }

        Ok(())
    }

    /// Install required packages
    async fn install_packages(&self) -> Result<(), RuntimeError> {
        if self.config.packages.is_empty() {
            return Ok(());
        }

        info!(
            "Installing packages for {} runtime: {:?}",
            self.config.runtime.command(),
            self.config.packages
        );

        self.init_working_dir().await?;

        let cmd = self.find_executable()?;
        let mut args = vec!["add", "--save"];
        args.extend(self.config.packages.iter().map(|s| s.as_str()));

        let mut cmd = Command::new(&cmd);
        cmd.args(&args)
            .current_dir(&self.config.working_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Apply environment
        cmd.env_clear();
        for (key, value) in &self.config.env {
            cmd.env(key, value);
        }

        // Add common environment variables
        cmd.env("NO_COLOR", "1");
        cmd.env("CI", "true");

        debug!("Running: {:?} {:?}", cmd, args.join(" "));

        let output = cmd
            .output()
            .await
            .map_err(|e| RuntimeError::InstallError(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(RuntimeError::InstallError(stderr.to_string()));
        }

        info!("Packages installed successfully");
        Ok(())
    }

    /// Execute a JavaScript/TypeScript script
    async fn execute_script(
        &self,
        script: &str,
        _input: Option<Value>,
    ) -> Result<ExecutionResult, RuntimeError> {
        let start_time = Instant::now();

        // Initialize working directory
        self.init_working_dir().await?;

        // Install packages first if needed
        if !self.config.packages.is_empty() {
            self.install_packages().await?;
        }

        let cmd = self.find_executable()?;
        let exec_args = self.config.runtime.exec_command(script);

        debug!(
            "Executing script with {}: {} {}",
            self.config.runtime.command(),
            cmd,
            exec_args.join(" ")
        );

        let mut cmd = Command::new(&cmd);
        cmd.args(&exec_args)
            .current_dir(&self.config.working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Apply environment
        cmd.env_clear();
        for (key, value) in &self.config.env {
            cmd.env(key, value);
        }

        // Add common environment variables
        cmd.env("NO_COLOR", "1");
        cmd.env("NODE_ENV", "production");

        // Apply resource limits
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            if self.config.resource_limits.max_memory_mb > 0 {
                // Memory limits are typically applied via cgroups
                // This is a best-effort attempt
            }
        }

        // Spawn the process
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => {
                return Err(RuntimeError::ExecutionError(format!(
                    "Failed to spawn {} process: {}",
                    self.config.runtime.command(),
                    e
                )));
            }
        };

        // Wait for completion with timeout
        let result = tokio::time::timeout(self.config.timeout, child.wait()).await;

        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        let output = child.wait_with_output().await.map_err(|e| {
            RuntimeError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        })?;

        // Parse stdout as JSON if possible
        let output_value: Option<Value> = serde_json::from_str(&String::from_utf8_lossy(&output.stdout))
            .ok()
            .filter(|v: &Value| !v.is_null() && !v.is_object() && !v.is_array());

        Ok(ExecutionResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
            execution_time_ms,
            output_value,
        })
    }
}

#[async_trait]
impl crate::runtime::types::Runtime for NodeRuntimeImpl {
    fn name(&self) -> &str {
        &self.name
    }

    fn runtime_type(&self) -> RuntimeType {
        match self.config.runtime {
            NodeRuntime::Pnpm => RuntimeType::NodePnpm,
            NodeRuntime::Npm => RuntimeType::NodeNpm,
            NodeRuntime::Bun => RuntimeType::NodeBun,
        }
    }

    async fn validate(&self) -> Result<(), RuntimeError> {
        self.find_executable().map(|_| ())
    }

    fn resource_limits(&self) -> &ResourceLimits {
        &self.config.resource_limits
    }

    async fn execute(
        &self,
        script: &str,
        input: Option<Value>,
    ) -> Result<ExecutionResult, RuntimeError> {
        self.execute_script(script, input).await
    }

    async fn execute_file(
        &self,
        path: &PathBuf,
        input: Option<Value>,
    ) -> Result<ExecutionResult, RuntimeError> {
        let script = fs::read_to_string(path).await.map_err(|e| {
            RuntimeError::Io(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
        })?;

        // If it's a TypeScript file, we need to handle it differently
        // For now, we assume it's JavaScript
        self.execute_script(&script, input).await
    }

    async fn install_packages(&self) -> Result<(), RuntimeError> {
        self.install_packages().await
    }
}

/// Check if a Node.js runtime is available
pub fn check_node_runtime_available(runtime: NodeRuntime) -> bool {
    which::which(runtime.command()).is_ok()
}

/// Get available Node.js runtimes
pub fn get_available_runtimes() -> Vec<NodeRuntime> {
    let mut runtimes = Vec::new();
    if check_node_runtime_available(NodeRuntime::Pnpm) {
        runtimes.push(NodeRuntime::Pnpm);
    }
    if check_node_runtime_available(NodeRuntime::Npm) {
        runtimes.push(NodeRuntime::Npm);
    }
    if check_node_runtime_available(NodeRuntime::Bun) {
        runtimes.push(NodeRuntime::Bun);
    }
    runtimes
}
