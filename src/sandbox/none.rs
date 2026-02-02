use crate::config::McpServerConfig;
use crate::sandbox::traits::{Sandbox, SandboxConstraints};
use crate::utils::errors::McpResult;
use async_trait::async_trait;
use tokio::process::{Child, Command};

/// No-op sandbox that runs commands without restrictions
pub struct NoSandbox {
    constraints: SandboxConstraints,
}

impl NoSandbox {
    pub fn new() -> Self {
        Self {
            constraints: SandboxConstraints {
                network: true,
                filesystem: super::traits::FilesystemConstraint::Full,
                env_inherit: true,
                max_memory_mb: 0, // No limit
                max_cpu_percent: 100,
            },
        }
    }
}

impl Default for NoSandbox {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Sandbox for NoSandbox {
    async fn spawn(&self, config: &McpServerConfig) -> McpResult<Child> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Inherit environment by default; clear only when disabled
        if !self.constraints.env_inherit {
            cmd.env_clear();
        }
        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        let child = cmd.spawn()?;
        Ok(child)
    }

    fn constraints(&self) -> &SandboxConstraints {
        &self.constraints
    }
}
