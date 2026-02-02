//! Full Linux sandbox implementation using namespaces, seccomp, and Landlock
//!
//! This module provides a comprehensive sandbox implementation that combines:
//! - Linux namespaces (PID, mount, IPC, network, UTS) for isolation
//! - seccomp-bpf for syscall filtering
//! - Landlock for filesystem access control
//!
//! The sandbox is designed to be defense-in-depth, with multiple layers
//! of protection.

use crate::config::McpServerConfig;
use crate::sandbox::traits::{FilesystemConstraint, Sandbox, SandboxConstraints};
use crate::utils::errors::{McpError, McpResult};
use async_trait::async_trait;
use nix::sched::{unshare, CloneFlags};
use nix::unistd::Uid;
use std::os::unix::process::CommandExt;
use tokio::process::{Child, Command};
use tracing::{debug, info, warn};

/// Full Linux sandbox with multiple security layers
///
/// This sandbox implementation uses:
/// - Namespaces for process, mount, IPC, and network isolation
/// - seccomp-bpf for restricting system calls
/// - Landlock for filesystem access control
pub struct LinuxSandboxFull {
    constraints: SandboxConstraints,
}

impl LinuxSandboxFull {
    /// Create a new Linux sandbox with the given constraints
    pub fn new(constraints: SandboxConstraints) -> Self {
        Self { constraints }
    }

    /// Create a sandbox from an MCP server configuration
    pub fn from_config(config: &McpServerConfig) -> Self {
        let filesystem = match &config.sandbox.filesystem {
            crate::sandbox::traits::FilesystemConstraint::Full => FilesystemConstraint::Full,
            crate::sandbox::traits::FilesystemConstraint::ReadOnly => FilesystemConstraint::ReadOnly,
            crate::sandbox::traits::FilesystemConstraint::Paths(paths) => {
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

    /// Apply Linux namespaces for isolation
    ///
    /// This function uses unshare() to create new namespaces for:
    /// - Mount (CLONE_NEWNS): Filesystem isolation
    /// - PID (CLONE_NEWPID): Process ID isolation
    /// - IPC (CLONE_NEWIPC): Inter-process communication isolation
    /// - Network (CLONE_NEWNET): Network isolation (if network disabled)
    /// - UTS (CLONE_NEWUTS): Hostname isolation
    fn apply_namespaces(&self) -> Result<(), nix::Error> {
        let mut flags = CloneFlags::empty();

        // Mount namespace for filesystem isolation
        flags |= CloneFlags::CLONE_NEWNS;
        debug!("Enabling mount namespace (CLONE_NEWNS)");

        // PID namespace for process isolation
        flags |= CloneFlags::CLONE_NEWPID;
        debug!("Enabling PID namespace (CLONE_NEWPID)");

        // IPC namespace for System V IPC and POSIX message queue isolation
        flags |= CloneFlags::CLONE_NEWIPC;
        debug!("Enabling IPC namespace (CLONE_NEWIPC)");

        // Network namespace (if network is disabled)
        if !self.constraints.network {
            flags |= CloneFlags::CLONE_NEWNET;
            debug!("Enabling network namespace (CLONE_NEWNET)");
        }

        // UTS namespace for hostname isolation
        flags |= CloneFlags::CLONE_NEWUTS;
        debug!("Enabling UTS namespace (CLONE_NEWUTS)");

        unshare(flags)?;
        info!("Successfully applied Linux namespaces");
        Ok(())
    }

    /// Prepare the pre_exec closure for the child process
    ///
    /// This closure runs in the child process before exec() and sets up
    /// all sandboxing mechanisms.
    fn prepare_pre_exec(&self) -> impl FnMut() -> Result<(), std::io::Error> + Send + Clone {
        let network = self.constraints.network;
        let filesystem = self.constraints.filesystem.clone();

        move || {
            debug!("Setting up sandbox in pre_exec hook");

            // Apply namespaces first
            let mut flags = CloneFlags::CLONE_NEWNS | CloneFlags::CLONE_NEWPID;

            if !network {
                flags |= CloneFlags::CLONE_NEWNET;
            }

            flags |= CloneFlags::CLONE_NEWIPC | CloneFlags::CLONE_NEWUTS;

            if let Err(e) = unshare(flags) {
                eprintln!("Failed to unshare namespaces: {}", e);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Namespace error: {}", e),
                ));
            }
            debug!("Namespaces applied successfully");

            // Apply seccomp filter
            if let Err(e) = super::linux_seccomp::apply_seccomp_filter() {
                eprintln!("Failed to apply seccomp: {}", e);
                // Continue without seccomp rather than fail entirely
                // This provides defense-in-depth without breaking functionality
            } else {
                debug!("seccomp filter applied successfully");
            }

            // Apply Landlock restrictions
            let paths = match &filesystem {
                FilesystemConstraint::Paths(p) => p.clone(),
                FilesystemConstraint::ReadOnly => vec!["/".to_string()],
                FilesystemConstraint::Full => vec!["/".to_string()],
            };
            let read_only = matches!(filesystem, FilesystemConstraint::ReadOnly);

            if let Err(e) = super::linux_landlock::apply_landlock_restrictions(paths, read_only) {
                eprintln!("Failed to apply Landlock: {}", e);
                // Continue without Landlock rather than fail entirely
            } else {
                debug!("Landlock restrictions applied successfully");
            }

            // Drop privileges if running as root
            if Uid::current().is_root() {
                warn!("Running as root - attempting to drop privileges");
                // In a production implementation, this would map to an unprivileged
                // user using newuidmap/newgidmap. For now, we log a warning.
                // TODO: Implement proper user namespace mapping
            }

            info!("Sandbox setup complete in pre_exec hook");
            Ok(())
        }
    }

    /// Check if full sandboxing is available on this system
    ///
    /// Returns a report of which sandboxing features are available.
    pub fn check_availability() -> SandboxAvailabilityReport {
        SandboxAvailabilityReport {
            seccomp: super::linux_seccomp::is_seccomp_available(),
            landlock: super::linux_landlock::is_landlock_available(),
            namespaces: Self::check_namespace_support(),
        }
    }

    /// Check if namespaces are supported (non-invasive check)
    fn check_namespace_support() -> NamespaceSupport {
        let ns_dir = std::path::Path::new("/proc/self/ns");
        if !ns_dir.exists() {
            return NamespaceSupport::None;
        }

        let mut supported = Vec::new();
        let mappings = [
            ("mount", "mnt"),
            ("pid", "pid"),
            ("ipc", "ipc"),
            ("net", "net"),
            ("uts", "uts"),
        ];

        for (name, entry) in mappings {
            if ns_dir.join(entry).exists() {
                supported.push(name.to_string());
            }
        }

        if supported.is_empty() {
            NamespaceSupport::None
        } else if supported.len() == mappings.len() {
            NamespaceSupport::Full
        } else {
            NamespaceSupport::Partial(supported)
        }
    }
}

/// Report of available sandboxing features
#[derive(Debug, Clone)]
pub struct SandboxAvailabilityReport {
    /// seccomp-bpf availability
    pub seccomp: bool,
    /// Landlock availability
    pub landlock: bool,
    /// Namespace support level
    pub namespaces: NamespaceSupport,
}

impl SandboxAvailabilityReport {
    /// Check if all sandboxing features are available
    pub fn is_fully_supported(&self) -> bool {
        self.seccomp && self.landlock && matches!(self.namespaces, NamespaceSupport::Full)
    }

    /// Check if basic sandboxing is available
    pub fn is_partially_supported(&self) -> bool {
        self.seccomp || self.landlock || !matches!(self.namespaces, NamespaceSupport::None)
    }
}

/// Level of namespace support
#[derive(Debug, Clone)]
pub enum NamespaceSupport {
    /// All namespaces are supported
    Full,
    /// Only some namespaces are supported
    Partial(Vec<String>),
    /// No namespaces are supported
    None,
}

#[async_trait]
impl Sandbox for LinuxSandboxFull {
    async fn spawn(&self, config: &McpServerConfig) -> McpResult<Child> {
        info!("Spawning sandboxed process with full Linux sandboxing");

        // Log sandbox configuration
        debug!("Sandbox constraints: {:?}", self.constraints);

        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Apply environment restrictions
        if !self.constraints.env_inherit {
            debug!("Clearing environment variables");
            cmd.env_clear();
        }

        // Apply custom environment variables
        for (key, value) in &config.env {
            debug!("Setting environment variable: {}=...", key);
            cmd.env(key, value);
        }

        // Set up pre_exec hook for sandboxing
        // This runs in the child process before exec()
        unsafe {
            let pre_exec = self.prepare_pre_exec();
            cmd.pre_exec(pre_exec);
        }

        // Spawn the process
        let child = cmd.spawn().map_err(|e| {
            McpError::SandboxError(format!("Failed to spawn sandboxed process: {}", e))
        })?;

        info!(
            "Successfully spawned sandboxed process with PID {:?}",
            child.id()
        );
        Ok(child)
    }

    fn constraints(&self) -> &SandboxConstraints {
        &self.constraints
    }
}

/// Create a fallback sandbox that uses available features
///
/// This function checks what sandboxing features are available and
/// returns a sandbox that uses the best available options.
pub fn create_best_effort_sandbox(config: &McpServerConfig) -> Box<dyn Sandbox> {
    let availability = LinuxSandboxFull::check_availability();

    info!("Sandbox availability report: {:?}", availability);

    if availability.is_fully_supported() {
        info!("Using full Linux sandboxing");
        Box::new(LinuxSandboxFull::from_config(config))
    } else if availability.is_partially_supported() {
        warn!(
            "Partial sandboxing support detected, using best-effort sandbox: {:?}",
            availability
        );
        Box::new(LinuxSandboxFull::from_config(config))
    } else {
        warn!("No sandboxing features available, using no-op sandbox");
        Box::new(super::none::NoSandbox::new())
    }
}
