//! Landlock filesystem access control for Linux sandboxing
//!
//! This module provides filesystem access restrictions using the Landlock
//! Linux Security Module (LSM). Landlock allows unprivileged processes to
//! create security sandboxes that can restrict access to the filesystem.

use landlock::{
    Access, AccessFs, PathBeneath, PathFd, Ruleset, RulesetAttr, RulesetCreated, RulesetStatus,
};
use std::path::Path;
use tracing::{debug, info, warn};

/// Apply Landlock restrictions to limit filesystem access
///
/// # Arguments
///
/// * `allowed_paths` - List of paths that should be accessible
/// * `read_only` - If true, all paths are granted read-only access
///
/// # Returns
///
/// Returns Ok(()) if restrictions were applied successfully, or an error
/// if Landlock is not available or could not be applied.
///
/// # Example
///
/// ```rust,no_run
/// use super_mcp::sandbox::linux_landlock::apply_landlock_restrictions;
///
/// let paths = vec!["/tmp/workdir".to_string()];
/// apply_landlock_restrictions(paths, false).expect("Failed to apply Landlock");
/// ```
pub fn apply_landlock_restrictions(
    allowed_paths: Vec<String>,
    read_only: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "Applying Landlock restrictions: {} paths, read_only={}",
        allowed_paths.len(),
        read_only
    );

    // Create a ruleset with filesystem access rights
    let mut ruleset = Ruleset::default();

    // Determine access rights based on read_only flag
    let access_rights = if read_only {
        AccessFs::from_all(Access::Read)
    } else {
        AccessFs::from_all(Access::ReadWrite)
    };

    debug!("Landlock access rights: {:?}", access_rights);

    // Add allowed paths
    let mut valid_paths = 0;
    for path_str in allowed_paths {
        let path = Path::new(&path_str);
        if path.exists() {
            match PathFd::new(path) {
                Ok(path_fd) => {
                    let beneath = PathBeneath::new(path_fd, access_rights);
                    match ruleset.add_rule(beneath) {
                        Ok(_) => {
                            debug!("Added Landlock rule for path: {}", path_str);
                            valid_paths += 1;
                        }
                        Err(e) => {
                            warn!("Failed to add Landlock rule for {}: {}", path_str, e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to open path {} for Landlock: {}", path_str, e);
                }
            }
        } else {
            warn!("Path does not exist, skipping Landlock rule: {}", path_str);
        }
    }

    // Also allow read-only access to system libraries (required for dynamic linking)
    let system_paths = ["/lib", "/lib64", "/usr/lib", "/usr/lib64"];
    for path_str in &system_paths {
        let path = Path::new(path_str);
        if path.exists() {
            match PathFd::new(path) {
                Ok(path_fd) => {
                    let beneath = PathBeneath::new(path_fd, AccessFs::from_all(Access::Read));
                    match ruleset.add_rule(beneath) {
                        Ok(_) => debug!("Added Landlock rule for system path: {}", path_str),
                        Err(e) => {
                            warn!("Failed to add Landlock rule for {}: {}", path_str, e)
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to open system path {}: {}", path_str, e);
                }
            }
        }
    }

    // Allow read-only access to essential system directories
    let essential_paths = ["/etc/ld.so.cache", "/etc/ld.so.conf", "/etc/ld.so.conf.d"];
    for path_str in &essential_paths {
        let path = Path::new(path_str);
        if path.exists() {
            match PathFd::new(path) {
                Ok(path_fd) => {
                    let beneath = PathBeneath::new(path_fd, AccessFs::from_all(Access::Read));
                    match ruleset.add_rule(beneath) {
                        Ok(_) => debug!("Added Landlock rule for essential path: {}", path_str),
                        Err(e) => {
                            warn!("Failed to add Landlock rule for {}: {}", path_str, e)
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to open essential path {}: {}", path_str, e);
                }
            }
        }
    }

    if valid_paths == 0 {
        warn!("No valid paths were added to Landlock ruleset");
    }

    // Restrict the process
    info!("Applying Landlock self-restriction");
    let status = ruleset.restrict_self()?;

    match status.ruleset {
        RulesetStatus::FullyEnforced => {
            info!("Landlock fully enforced");
            Ok(())
        }
        RulesetStatus::PartiallyEnforced => {
            warn!("Landlock only partially enforced - some restrictions may not be active");
            Ok(())
        }
        RulesetStatus::NotEnforced => {
            Err("Landlock not supported on this system".into())
        }
    }
}

/// Check if Landlock is available on this system
///
/// Returns true if Landlock is supported by the kernel and can be used.
pub fn is_landlock_available() -> bool {
    landlock_supported_by_kernel()
}

/// Get detailed Landlock availability information
///
/// Returns a tuple of (available, fully_supported) where:
/// - available: Landlock can be used (may be partial)
/// - fully_supported: Landlock is fully supported by the kernel
pub fn get_landlock_status() -> (bool, bool) {
    let available = landlock_supported_by_kernel();
    (available, false)
}

fn landlock_supported_by_kernel() -> bool {
    if std::path::Path::new("/proc/self/attr/landlock").exists() {
        return true;
    }

    if let Ok(lsm) = std::fs::read_to_string("/sys/kernel/security/lsm") {
        return lsm.split(',').any(|entry| entry.trim() == "landlock");
    }

    false
}

/// Apply Landlock restrictions with a specific set of access rights
///
/// This is a more flexible version that allows specifying exact access rights.
pub fn apply_landlock_with_rights(
    allowed_paths: Vec<(String, AccessFs)>,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "Applying Landlock restrictions with custom rights: {} paths",
        allowed_paths.len()
    );

    let mut ruleset = Ruleset::default();

    for (path_str, access_rights) in allowed_paths {
        let path = Path::new(&path_str);
        if path.exists() {
            match PathFd::new(path) {
                Ok(path_fd) => {
                    let beneath = PathBeneath::new(path_fd, access_rights);
                    match ruleset.add_rule(beneath) {
                        Ok(_) => debug!("Added Landlock rule for: {}", path_str),
                        Err(e) => warn!("Failed to add rule for {}: {}", path_str, e),
                    }
                }
                Err(e) => warn!("Failed to open {}: {}", path_str, e),
            }
        } else {
            warn!("Path does not exist: {}", path_str);
        }
    }

    let status = ruleset.restrict_self()?;

    match status.ruleset {
        RulesetStatus::FullyEnforced => {
            info!("Landlock fully enforced with custom rights");
            Ok(())
        }
        RulesetStatus::PartiallyEnforced => {
            warn!("Landlock partially enforced with custom rights");
            Ok(())
        }
        RulesetStatus::NotEnforced => Err("Landlock not supported".into()),
    }
}
