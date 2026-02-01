use crate::config::McpServerConfig;
use crate::sandbox::traits::{FilesystemConstraint, Sandbox, SandboxConstraints};
use crate::utils::errors::{McpError, McpResult};
use async_trait::async_trait;
use tokio::process::{Child, Command};
use tracing::warn;

/// Linux sandbox using seccomp and namespaces
pub struct LinuxSandbox {
    constraints: SandboxConstraints,
}

impl LinuxSandbox {
    pub fn new(constraints: SandboxConstraints) -> Self {
        Self { constraints }
    }

    pub fn from_config(config: &McpServerConfig) -> Self {
        let filesystem = match &config.sandbox.filesystem {
            super::traits::FilesystemConstraint::Full => FilesystemConstraint::Full,
            super::traits::FilesystemConstraint::ReadOnly => FilesystemConstraint::ReadOnly,
            super::traits::FilesystemConstraint::Paths(paths) => {
                FilesystemConstraint::Paths(paths.clone())
            }
        };

        Self {
            constraints: SandboxConstraints {
                network: config.sandbox.network,
                filesystem,
                env_inherit: config.sandbox.env_inherit,
                max_memory_mb: config.sandbox.max_memory_mb,
                max_cpu_percent: config.sandbox.max_cpu_percent,
            },
        }
    }
}

#[async_trait]
impl Sandbox for LinuxSandbox {
    async fn spawn(&self, config: &McpServerConfig) -> McpResult<Child> {
        warn!("Linux sandbox is not fully implemented yet, using basic restrictions");

        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Apply environment restrictions
        if !self.constraints.env_inherit {
            cmd.env_clear();
        }

        // Apply custom environment variables
        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        // TODO: Implement actual sandboxing:
        // 1. Create namespaces (clone3 with CLONE_NEWNS | CLONE_NEWPID | CLONE_NEWNET)
        // 2. Apply seccomp filter
        // 3. Apply Landlock rules
        // 4. Move to cgroup
        // 5. Pivot root to tmpfs

        let child = cmd.spawn()?;
        Ok(child)
    }

    fn constraints(&self) -> &SandboxConstraints {
        &self.constraints
    }
}
