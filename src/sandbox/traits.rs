use crate::config::McpServerConfig;
use crate::utils::errors::McpResult;
use async_trait::async_trait;
use tokio::process::Child;

/// Constraints for sandboxed processes
#[derive(Debug, Clone)]
pub struct SandboxConstraints {
    pub network: bool,
    pub filesystem: FilesystemConstraint,
    pub env_inherit: bool,
    pub max_memory_mb: u64,
    pub max_cpu_percent: u32,
}

#[derive(Debug, Clone)]
pub enum FilesystemConstraint {
    Full,
    ReadOnly,
    Paths(Vec<String>),
}

/// Trait for sandbox implementations
#[async_trait]
pub trait Sandbox: Send + Sync {
    /// Spawn a process with sandbox constraints applied
    async fn spawn(&self, config: &McpServerConfig) -> McpResult<Child>;

    /// Return the constraints this sandbox enforces
    fn constraints(&self) -> &SandboxConstraints;
}

impl Default for SandboxConstraints {
    fn default() -> Self {
        Self {
            network: false,
            filesystem: FilesystemConstraint::ReadOnly,
            env_inherit: false,
            max_memory_mb: 512,
            max_cpu_percent: 50,
        }
    }
}
