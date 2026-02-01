//! Linux sandbox implementation
//!
//! This module provides Linux-specific sandboxing using:
//! - seccomp-bpf for syscall filtering
//! - Landlock for filesystem access control
//! - Linux namespaces for process isolation
//!
//! The implementation is in submodules:
//! - `linux_seccomp.rs` - seccomp-bpf syscall filtering
//! - `linux_landlock.rs` - Landlock filesystem restrictions
//! - `linux_full.rs` - Full sandbox orchestration

// Re-export the full implementation
pub use linux_full::{LinuxSandboxFull as LinuxSandbox, SandboxAvailabilityReport, NamespaceSupport, create_best_effort_sandbox};

// Submodules
mod linux_seccomp;
mod linux_landlock;
mod linux_full;

use crate::config::McpServerConfig;
use crate::sandbox::traits::{FilesystemConstraint, Sandbox, SandboxConstraints};
use crate::utils::errors::McpResult;
use async_trait::async_trait;
use tokio::process::Child;

// Re-export key functions for convenience
pub use linux_seccomp::{apply_seccomp_filter, apply_restrictive_seccomp, is_seccomp_available};
pub use linux_landlock::{apply_landlock_restrictions, is_landlock_available, get_landlock_status};
