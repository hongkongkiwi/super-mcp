pub mod none;
pub mod traits;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "linux")]
pub mod linux_advanced;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;

pub use none::NoSandbox;
pub use traits::{FilesystemConstraint, Sandbox, SandboxConstraints};

#[cfg(target_os = "linux")]
pub use linux::LinuxSandbox;

#[cfg(target_os = "linux")]
pub use linux_advanced::{AdvancedLinuxSandbox, AdvancedLinuxSandboxConfig};

#[cfg(target_os = "macos")]
pub use macos::MacOSSandbox;

#[cfg(target_os = "windows")]
pub use windows::WindowsSandbox;

/// Create the appropriate sandbox for the current platform
pub fn create_sandbox(config: &crate::config::McpServerConfig) -> Box<dyn Sandbox> {
    if !config.sandbox.enabled {
        return Box::new(NoSandbox::new());
    }

    #[cfg(target_os = "linux")]
    {
        Box::new(LinuxSandbox::from_config(config))
    }

    #[cfg(target_os = "macos")]
    {
        if MacOSSandbox::is_available() {
            Box::new(MacOSSandbox::from_config(config))
        } else {
            tracing::warn!("sandbox-exec not available, using no-op sandbox");
            Box::new(NoSandbox::new())
        }
    }

    #[cfg(target_os = "windows")]
    {
        if WindowsSandbox::is_available() {
            Box::new(WindowsSandbox::from_config(config))
        } else {
            tracing::warn!("Windows sandboxing not available, using no-op sandbox");
            Box::new(NoSandbox::new())
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        // Fall back to no-op sandbox on other platforms
        tracing::warn!("Sandbox not implemented for this platform, using no-op");
        Box::new(NoSandbox::new())
    }
}
