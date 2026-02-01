//! macOS Seatbelt sandbox implementation
//!
//! This module implements sandboxing using macOS's Seatbelt (sandbox) system.
//! Seatbelt uses a profile-based system to restrict process capabilities.

use crate::config::McpServerConfig;
use crate::sandbox::traits::{FilesystemConstraint, Sandbox, SandboxConstraints};
use crate::utils::errors::{McpError, McpResult};
use async_trait::async_trait;
use tokio::process::Child;

/// macOS Seatbelt sandbox
pub struct MacOSSandbox {
    constraints: SandboxConstraints,
    profile: String,
}

impl MacOSSandbox {
    /// Create a new macOS sandbox from configuration
    pub fn from_config(config: &McpServerConfig) -> Self {
        let constraints = SandboxConstraints {
            network: config.sandbox.network,
            filesystem: match &config.sandbox.filesystem {
                crate::config::FilesystemAccess::Simple(s) if s == "readonly" => {
                    FilesystemConstraint::ReadOnly
                }
                crate::config::FilesystemAccess::Simple(s) if s == "full" => {
                    FilesystemConstraint::Full
                }
                crate::config::FilesystemAccess::Paths(paths) => {
                    FilesystemConstraint::Paths(paths.clone())
                }
                _ => FilesystemConstraint::ReadOnly,
            },
            env_inherit: config.sandbox.env_inherit,
            max_memory_mb: config.sandbox.max_memory_mb,
            max_cpu_percent: config.sandbox.max_cpu_percent,
        };

        let profile = Self::generate_profile(&constraints);

        Self {
            constraints,
            profile,
        }
    }

    /// Generate a Seatbelt sandbox profile based on constraints
    fn generate_profile(constraints: &SandboxConstraints) -> String {
        let mut rules = Vec::new();

        // Version declaration
        rules.push("(version 1)".to_string());

        // Deny everything by default
        rules.push("(deny default)".to_string());

        // Allow basic process operations
        rules.push("(allow process-exec (subprocess))".to_string());
        rules.push("(allow process-fork)".to_string());
        rules.push("(allow signal (target self))".to_string());

        // Allow reading system libraries and frameworks
        rules.push("(allow file-read* (subpath \"/usr/lib\"))".to_string());
        rules.push("(allow file-read* (subpath \"/System/Library\"))".to_string());
        rules.push("(allow file-read* (subpath \"/Library/Frameworks\"))".to_string());
        rules.push("(allow file-read* (subpath \"/System/Library/Frameworks\"))".to_string());

        // Allow reading from /dev and /tmp
        rules.push("(allow file-read* (subpath \"/dev\"))".to_string());
        rules.push("(allow file-read* file-write* (subpath \"/tmp\"))".to_string());
        rules.push("(allow file-read* file-write* (subpath \"/var/tmp\"))".to_string());

        // Allow reading from user's home directory basics
        rules.push("(allow file-read* (literal \"/etc/passwd\"))".to_string());
        rules.push("(allow file-read* (regex #\"^/Users/[^/]+/.bashrc$\"))".to_string());
        rules.push("(allow file-read* (regex #\"^/Users/[^/]+/.profile$\"))".to_string());
        rules.push("(allow file-read* (regex #\"^/Users/[^/]+/.zshenv$\"))".to_string());

        // Filesystem access rules
        match &constraints.filesystem {
            FilesystemConstraint::Full => {
                // Allow full filesystem access
                rules.push("(allow file-read* file-write*)".to_string());
            }
            FilesystemConstraint::ReadOnly => {
                // Allow reading most places, but restrict writing
                rules.push("(allow file-read*)".to_string());
                // Allow writing to temp directories only
                rules.push("(allow file-write* (subpath \"/tmp\"))".to_string());
                rules.push("(allow file-write* (subpath \"/var/tmp\"))".to_string());
            }
            FilesystemConstraint::Paths(paths) => {
                // Allow specific paths
                for path in paths {
                    // Expand home directory
                    let expanded = if path.starts_with("~/") {
                        dirs::home_dir()
                            .map(|h| h.join(&path[2..]).to_string_lossy().to_string())
                            .unwrap_or_else(|| path.clone())
                    } else {
                        path.clone()
                    };

                    rules.push(format!(
                        "(allow file-read* file-write* (subpath \"{}\"))",
                        expanded
                    ));
                }
                // Also allow temp directory
                rules.push("(allow file-read* file-write* (subpath \"/tmp\"))".to_string());
                rules.push("(allow file-read* file-write* (subpath \"/var/tmp\"))".to_string());
            }
        }

        // Network access
        if constraints.network {
            rules.push("(allow network-outbound)".to_string());
            rules.push("(allow network-inbound)".to_string());
            rules.push("(allow system-socket)".to_string());
        } else {
            rules.push("(deny network*)".to_string());
        }

        // Allow IPC for local communication
        rules.push("(allow ipc-posix*)".to_string());
        rules.push("(allow mach-lookup (global-name \"com.apple.system.notification_center\"))".to_string());

        // Allow system info queries
        rules.push("(allow system-info)".to_string());

        // Allow sysctl read
        rules.push("(allow sysctl-read)".to_string());

        rules.join("\n")
    }

    /// Get the path to the sandbox-exec binary
    fn sandbox_exec_path() -> &'static str {
        "/usr/bin/sandbox-exec"
    }

    /// Check if sandbox-exec is available
    pub fn is_available() -> bool {
        std::path::Path::new(Self::sandbox_exec_path()).exists()
    }
}

#[async_trait]
impl Sandbox for MacOSSandbox {
    async fn spawn(&self, config: &McpServerConfig) -> McpResult<Child> {
        if !Self::is_available() {
            return Err(McpError::SandboxError(
                "sandbox-exec is not available on this system".to_string()
            ));
        }

        let mut cmd = tokio::process::Command::new(Self::sandbox_exec_path());

        // Add the profile
        cmd.arg("-p").arg(&self.profile);

        // Set the command to run
        cmd.arg(config.command.clone());
        cmd.args(&config.args);

        // Set environment variables
        if !self.constraints.env_inherit {
            cmd.env_clear();
        }
        
        // Add minimal environment
        cmd.env("PATH", "/usr/bin:/bin:/usr/local/bin");
        
        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        // Setup stdio
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Spawn the process
        let child = cmd.spawn().map_err(|e| {
            McpError::SandboxError(format!("Failed to spawn sandboxed process: {}", e))
        })?;

        Ok(child)
    }

    fn constraints(&self) -> &SandboxConstraints {
        &self.constraints
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_profile() {
        let constraints = SandboxConstraints {
            network: false,
            filesystem: FilesystemConstraint::ReadOnly,
            env_inherit: false,
            max_memory_mb: 512,
            max_cpu_percent: 50,
        };

        let profile = MacOSSandbox::generate_profile(&constraints);
        
        assert!(profile.contains("(version 1)"));
        assert!(profile.contains("(deny default)"));
        assert!(profile.contains("(allow file-read*)"));
        assert!(profile.contains("(deny network*)"));
    }

    #[test]
    fn test_generate_profile_with_network() {
        let constraints = SandboxConstraints {
            network: true,
            filesystem: FilesystemConstraint::Full,
            env_inherit: false,
            max_memory_mb: 512,
            max_cpu_percent: 50,
        };

        let profile = MacOSSandbox::generate_profile(&constraints);
        
        assert!(profile.contains("(allow network-outbound)"));
        assert!(profile.contains("(allow file-read* file-write*)"));
    }

    #[test]
    fn test_generate_profile_with_paths() {
        let constraints = SandboxConstraints {
            network: false,
            filesystem: FilesystemConstraint::Paths(vec!["/tmp/test".to_string()]),
            env_inherit: false,
            max_memory_mb: 512,
            max_cpu_percent: 50,
        };

        let profile = MacOSSandbox::generate_profile(&constraints);
        
        assert!(profile.contains("/tmp/test"));
    }
}
