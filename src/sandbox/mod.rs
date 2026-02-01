pub mod none;
pub mod traits;

#[cfg(target_os = "linux")]
pub mod linux;

pub use none::NoSandbox;
pub use traits::{FilesystemConstraint, Sandbox, SandboxConstraints};

#[cfg(target_os = "linux")]
pub use linux::LinuxSandbox;

/// Create the appropriate sandbox for the current platform
pub fn create_sandbox(config: &crate::config::McpServerConfig) -> Box<dyn Sandbox> {
    if !config.sandbox.enabled {
        return Box::new(NoSandbox::new());
    }

    #[cfg(target_os = "linux")]
    {
        Box::new(LinuxSandbox::from_config(config))
    }

    #[cfg(not(target_os = "linux"))]
    {
        // Fall back to no-op sandbox on non-Linux platforms for now
        tracing::warn!("Sandbox not implemented for this platform, using no-op");
        Box::new(NoSandbox::new())
    }
}
