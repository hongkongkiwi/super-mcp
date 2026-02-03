//! Runtime types and traits for sandboxed script execution

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;


/// Runtime type enumeration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeType {
    /// Python via WASM (Pyodide-like)
    PythonWasm,
    /// Node.js via pnpm
    NodePnpm,
    /// Node.js via npm
    NodeNpm,
    /// Node.js via bun
    NodeBun,
}

/// Resource limits for runtime execution
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ResourceLimits {
    /// Maximum memory in megabytes
    pub max_memory_mb: u64,
    /// Maximum CPU percentage (0-100)
    pub max_cpu_percent: u32,
    /// Timeout in seconds
    pub timeout_seconds: u64,
    /// Allow network access
    pub network_access: bool,
    /// File system access level
    pub filesystem: RuntimeFilesystemAccess,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_mb: 512,
            max_cpu_percent: 50,
            timeout_seconds: 30,
            network_access: false,
            filesystem: RuntimeFilesystemAccess::ReadOnly,
        }
    }
}

/// File system access levels for runtime execution
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFilesystemAccess {
    /// No file system access
    None,
    /// Read-only access
    ReadOnly,
    /// Read and write access
    ReadWrite,
    /// Specific paths allowed
    Paths(Vec<String>),
}

/// Re-export for backward compatibility
pub type FilesystemAccess = RuntimeFilesystemAccess;

/// Runtime configuration from config file
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RuntimeConfig {
    /// Runtime name
    pub name: String,
    /// Runtime type
    pub type_: RuntimeType,
    /// Packages to install (for Node.js)
    pub packages: Vec<String>,
    /// Working directory for script execution
    pub working_dir: Option<String>,
    /// Environment variables
    pub env: HashMap<String, String>,
    /// Resource limits
    pub resource_limits: ResourceLimits,
    /// Whether this runtime is enabled
    pub enabled: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            type_: RuntimeType::NodeNpm,
            packages: Vec::new(),
            working_dir: None,
            env: HashMap::new(),
            resource_limits: ResourceLimits::default(),
            enabled: true,
        }
    }
}

/// Result of a runtime execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Whether execution was successful
    pub success: bool,
    /// Standard output
    pub stdout: String,
    /// Standard error
    pub stderr: String,
    /// Exit code
    pub exit_code: i32,
    /// Execution time in milliseconds
    pub execution_time_ms: u64,
    /// Output as JSON value (if applicable)
    pub output_value: Option<Value>,
}

/// Trait for runtime implementations
#[async_trait]
pub trait Runtime: Send + Sync {
    /// Get the runtime name
    fn name(&self) -> &str;

    /// Get the runtime type
    fn runtime_type(&self) -> RuntimeType;

    /// Validate that the runtime is available
    async fn validate(&self) -> Result<(), RuntimeError>;

    /// Get resource limits
    fn resource_limits(&self) -> &ResourceLimits;

    /// Execute a script
    async fn execute(
        &self,
        script: &str,
        input: Option<Value>,
    ) -> Result<ExecutionResult, RuntimeError>;

    /// Execute a script file
    async fn execute_file(
        &self,
        path: &std::path::Path,
        input: Option<Value>,
    ) -> Result<ExecutionResult, RuntimeError>;

    /// Install required packages for this runtime
    async fn install_packages(&self) -> Result<(), RuntimeError>;
}

/// Runtime execution error
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("runtime not found: {0}")]
    RuntimeNotFound(String),

    #[error("validation error: {0}")]
    ValidationError(String),

    #[error("execution error: {0}")]
    ExecutionError(String),

    #[error("timeout after {0} seconds")]
    Timeout(u64),

    #[error("resource limit exceeded: {0}")]
    ResourceLimitExceeded(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("package installation error: {0}")]
    InstallError(String),
}

/// Helper function to convert FilesystemAccess to sandbox trait type
#[allow(dead_code)]
pub fn to_sandbox_filesystem_constraint(
    access: &FilesystemAccess,
) -> crate::sandbox::FilesystemConstraint {
    match access {
        FilesystemAccess::None => crate::sandbox::FilesystemConstraint::Paths(vec![]),
        FilesystemAccess::ReadOnly => crate::sandbox::FilesystemConstraint::ReadOnly,
        FilesystemAccess::ReadWrite => crate::sandbox::FilesystemConstraint::Full,
        FilesystemAccess::Paths(paths) => crate::sandbox::FilesystemConstraint::Paths(paths.clone()),
    }
}
