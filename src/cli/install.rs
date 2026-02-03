//! Startup manager installation commands

use anyhow::{anyhow, Result as AnyhowResult};
use dialoguer::{Confirm, MultiSelect};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::fs as async_fs;

/// Supported startup managers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupManager {
    /// macOS launchd
    Launchd,
    /// Linux systemd
    Systemd,
    /// Linux OpenRC
    Openrc,
    /// Linux runit
    Runit,
    /// Windows NSSM
    Nssm,
    /// Windows Task Scheduler
    Schtasks,
}

impl StartupManager {
    /// Get the display name for the manager
    pub fn display_name(&self) -> &'static str {
        match self {
            StartupManager::Launchd => "macOS launchd (launchctl)",
            StartupManager::Systemd => "Linux systemd",
            StartupManager::Openrc => "Linux OpenRC",
            StartupManager::Runit => "Linux runit",
            StartupManager::Nssm => "Windows NSSM",
            StartupManager::Schtasks => "Windows Task Scheduler (schtasks)",
        }
    }

    /// Get the platform this manager is available on
    pub fn platform(&self) -> &'static str {
        match self {
            StartupManager::Launchd => "macOS",
            StartupManager::Systemd => "Linux",
            StartupManager::Openrc => "Linux",
            StartupManager::Runit => "Linux",
            StartupManager::Nssm => "Windows",
            StartupManager::Schtasks => "Windows",
        }
    }

    /// Check if this manager is available on the current system
    pub fn is_available(&self) -> bool {
        match self {
            StartupManager::Launchd => {
                Command::new("launchctl").arg("--version").output().is_ok()
            }
            StartupManager::Systemd => {
                Path::new("/etc/systemd/system").exists() && which::which("systemctl").is_ok()
            }
            StartupManager::Openrc => {
                which::which("rc-service").is_ok() || which::which("openrc").is_ok()
            }
            StartupManager::Runit => Path::new("/etc/service").exists(),
            StartupManager::Nssm => which::which("nssm").is_ok(),
            StartupManager::Schtasks => which::which("schtasks").is_ok(),
        }
    }
}

/// Detect available startup managers for the current OS
pub fn detect_available_managers() -> Vec<StartupManager> {
    let managers: Vec<StartupManager> = match std::env::consts::OS {
        "macos" => vec![StartupManager::Launchd],
        "linux" => vec![StartupManager::Systemd, StartupManager::Openrc, StartupManager::Runit],
        "windows" => {
            let mut managers = vec![StartupManager::Schtasks];
            if StartupManager::Nssm.is_available() {
                managers.push(StartupManager::Nssm);
            }
            managers
        }
        _ => vec![],
    };

    managers
        .into_iter()
        .filter(|m| m.is_available())
        .collect()
}

/// Detect if running in a container
pub fn is_container_environment() -> bool {
    // Check for common container indicators
    if Path::new("/.dockerenv").exists() {
        return true;
    }

    // Check for containerized cgroup
    if let Ok(cgroup) = fs::read_to_string("/proc/1/cgroup") {
        if cgroup.contains("docker") || cgroup.contains("containerd") || cgroup.contains("lxc") {
            return true;
        }
    }

    // Check for systemd container
    if Path::new("/run/systemd/container").exists() {
        return true;
    }

    false
}

/// Detect the current binary path
pub fn detect_binary_path() -> Option<PathBuf> {
    // First, try to find ourselves in PATH
    if let Ok(path) = which::which("supermcp") {
        return Some(path);
    }

    // Check common installation paths
    let common_paths = [
        "/usr/local/bin/supermcp",
        "/usr/bin/supermcp",
        "/opt/homebrew/bin/supermcp",
    ];

    for path in &common_paths {
        if Path::new(path).exists() {
            return Some(PathBuf::from(path));
        }
    }

    // Try to get the current executable path via /proc/self/exe on Linux
    if Path::new("/proc/self/exe").exists() {
        if let Ok(path) = fs::read_link("/proc/self/exe") {
            return Some(path);
        }
    }

    None
}

/// Get the current platform
#[allow(dead_code)]
pub fn get_platform() -> &'static str {
    std::env::consts::OS
}

/// Install super-mcp as a startup service
pub async fn install(
    binary_path: Option<&str>,
    config_path: Option<&str>,
    manager: Option<&str>,
    uninstall: bool,
) -> AnyhowResult<()> {
    let binary_path = binary_path.map(|s| s.to_string()).unwrap_or_else(|| {
        detect_binary_path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/usr/local/bin/supermcp".to_string())
    });

    let config_path = config_path.map(|s| s.to_string()).unwrap_or_else(|| {
        dirs::config_dir()
            .map(|p| p.join("super-mcp/config.toml").to_string_lossy().to_string())
            .unwrap_or_else(|| "~/.config/super-mcp/config.toml".to_string())
    });

    // Expand tilde in paths
    let binary_path_expanded = shellexpand::tilde(&binary_path).to_string();
    let config_path_expanded = shellexpand::tilde(&config_path).to_string();

    // Validate binary exists
    if !uninstall && !Path::new(&binary_path_expanded).exists() {
        return Err(anyhow!(
            "Binary not found at: {}. Use --binary to specify the path.",
            binary_path_expanded
        ));
    }

    let available_managers = detect_available_managers();

    if available_managers.is_empty() {
        return Err(anyhow!(
            "No supported startup managers detected on this platform."
        ));
    }

    let selected_managers = if let Some(mgr) = manager {
        // Parse single manager from argument
        let mgr = match mgr.to_lowercase().as_str() {
            "launchd" | "macos" | "darwin" => vec![StartupManager::Launchd],
            "systemd" => vec![StartupManager::Systemd],
            "openrc" | "open-rc" => vec![StartupManager::Openrc],
            "runit" => vec![StartupManager::Runit],
            "nssm" => vec![StartupManager::Nssm],
            "schtasks" | "taskscheduler" | "task-scheduler" => {
                vec![StartupManager::Schtasks]
            }
            _ => {
                return Err(anyhow!(
                    "Unknown startup manager: {}. Valid options: launchd, systemd, openrc, runit, nssm, schtasks",
                    mgr
                ));
            }
        };

        // Verify the manager is available
        for m in &mgr {
            if !m.is_available() {
                return Err(anyhow!(
                    "Manager '{}' is not available on this system.",
                    m.display_name()
                ));
            }
        }

        mgr
    } else if uninstall {
        // For uninstall, try all available managers
        available_managers.clone()
    } else {
        // Interactive mode - present selection menu
        if !atty::is(atty::Stream::Stdin) {
            // Non-interactive mode, use first available
            vec![available_managers[0]]
        } else {
            select_managers_interactive(&available_managers)?
        }
    };

    if selected_managers.is_empty() {
        return Err(anyhow!("No startup managers selected."));
    }

    // Check for container environment
    if is_container_environment() && !uninstall {
        println!("Note: Running in a container environment.");
        println!("Some startup manager options may not be applicable.");
        if !Confirm::new()
            .with_prompt("Continue with installation anyway?")
            .default(false)
            .interact()? {
            return Ok(());
        }
    }

    // Perform installation/uninstallation
    for manager in &selected_managers {
        if uninstall {
            println!("Uninstalling from {}...", manager.display_name());
            uninstall_from_manager(manager, &binary_path_expanded, &config_path_expanded).await?;
        } else {
            println!("Installing to {}...", manager.display_name());
            install_to_manager(manager, &binary_path_expanded, &config_path_expanded).await?;
        }
    }

    Ok(())
}

fn select_managers_interactive(managers: &[StartupManager]) -> AnyhowResult<Vec<StartupManager>> {
    println!("Select startup manager(s) to install to:\n");

    let items: Vec<_> = managers.iter().map(|m| m.display_name()).collect();

    let selections = MultiSelect::new()
        .with_prompt("Select managers (use space to select, enter to confirm)")
        .items(&items)
        .defaults(&vec![true; items.len()])
        .interact()?;

    if selections.is_empty() {
        return Err(anyhow!("No managers selected."));
    }

    Ok(selections.into_iter().map(|i| managers[i]).collect())
}

/// Install to a specific manager
async fn install_to_manager(
    manager: &StartupManager,
    binary_path: &str,
    config_path: &str,
) -> AnyhowResult<()> {
    match manager {
        StartupManager::Launchd => install_launchd(binary_path, config_path).await,
        StartupManager::Systemd => install_systemd(binary_path, config_path).await,
        StartupManager::Openrc => install_openrc(binary_path, config_path).await,
        StartupManager::Runit => install_runit(binary_path, config_path).await,
        StartupManager::Nssm => install_nssm(binary_path, config_path).await,
        StartupManager::Schtasks => install_schtasks(binary_path, config_path).await,
    }
}

/// Uninstall from a specific manager
#[allow(dead_code)]
async fn uninstall_from_manager(
    manager: &StartupManager,
    _binary_path: &str,
    _config_path: &str,
) -> AnyhowResult<()> {
    match manager {
        StartupManager::Launchd => uninstall_launchd().await,
        StartupManager::Systemd => uninstall_systemd().await,
        StartupManager::Openrc => uninstall_openrc().await,
        StartupManager::Runit => uninstall_runit().await,
        StartupManager::Nssm => uninstall_nssm().await,
        StartupManager::Schtasks => uninstall_schtasks().await,
    }
}

/// Install using macOS launchd
async fn install_launchd(binary_path: &str, config_path: &str) -> AnyhowResult<()> {
    let plist_content = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.super-mcp.agent</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>serve</string>
        <string>--config</string>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/tmp/super-mcp.out.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/super-mcp.err.log</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>RUST_LOG</key>
        <string>info</string>
    </dict>
</dict>
</plist>
"#,
        binary_path,
        config_path
    );

    let plist_path = dirs::home_dir()
        .ok_or_else(|| anyhow!("Could not determine home directory"))?
        .join("Library/LaunchAgents/com.super-mcp.agent.plist");

    // Ensure directory exists
    if let Some(parent) = plist_path.parent() {
        async_fs::create_dir_all(parent).await?;
    }

    async_fs::write(&plist_path, plist_content).await?;

    // Load the daemon
    let output = Command::new("launchctl")
        .args(["load", "-w", plist_path.to_str().unwrap()])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to load launchd daemon: {}", stderr));
    }

    // Start the daemon
    let output = Command::new("launchctl")
        .args(["start", "com.super-mcp.agent"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("Warning: Failed to start daemon (may already be running): {}", stderr);
    }

    println!("✓ Installed super-mcp as launchd daemon");
    println!("  Plist: {}", plist_path.display());
    println!("  Use 'launchctl list | grep super-mcp' to check status");

    Ok(())
}

/// Uninstall from macOS launchd
async fn uninstall_launchd() -> AnyhowResult<()> {
    // Stop the daemon
    let _ = Command::new("launchctl")
        .args(["stop", "com.super-mcp.agent"])
        .output();

    // Unload the daemon
    let _ = Command::new("launchctl")
        .args(["unload", "-w", "~/Library/LaunchAgents/com.super-mcp.agent.plist"])
        .output();

    let plist_path = dirs::home_dir()
        .ok_or_else(|| anyhow!("Could not determine home directory"))?
        .join("Library/LaunchAgents/com.super-mcp.agent.plist");

    if plist_path.exists() {
        fs::remove_file(&plist_path)?;
        println!("✓ Removed launchd plist");
    }

    println!("✓ Uninstalled super-mcp from launchd");
    Ok(())
}

/// Install using Linux systemd
async fn install_systemd(binary_path: &str, config_path: &str) -> AnyhowResult<()> {
    let service_content = format!(r#"[Unit]
Description=Super MCP Server
After=network.target

[Service]
Type=simple
ExecStart={} serve --config {}
Restart=always
RestartSec=5
Environment=RUST_LOG=info
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
"#,
        binary_path,
        config_path
    );

    let service_path = PathBuf::from("/etc/systemd/system/super-mcp.service");

    async_fs::write(&service_path, service_content).await?;

    // Reload systemd daemon
    let output = Command::new("systemctl")
        .args(["daemon-reload"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to reload systemd daemon: {}", stderr));
    }

    // Enable and start the service
    let output = Command::new("systemctl")
        .args(["enable", "--now", "super-mcp"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to enable/start service: {}", stderr));
    }

    println!("✓ Installed super-mcp as systemd service");
    println!("  Service: /etc/systemd/system/super-mcp.service");
    println!("  Use 'systemctl status super-mcp' to check status");

    Ok(())
}

/// Uninstall from Linux systemd
async fn uninstall_systemd() -> AnyhowResult<()> {
    // Stop and disable the service
    let _ = Command::new("systemctl")
        .args(["stop", "super-mcp"])
        .output();

    let _output = Command::new("systemctl")
        .args(["disable", "super-mcp"])
        .output();

    let service_path = PathBuf::from("/etc/systemd/system/super-mcp.service");

    if service_path.exists() {
        async_fs::remove_file(&service_path).await?;
        // Reload daemon
        let _ = Command::new("systemctl").args(["daemon-reload"]).output();
        println!("✓ Removed systemd service file");
    }

    println!("✓ Uninstalled super-mcp from systemd");
    Ok(())
}

/// Install using Linux OpenRC
async fn install_openrc(binary_path: &str, config_path: &str) -> AnyhowResult<()> {
    let init_content = format!(r#"#!/sbin/openrc-run

name="super-mcp"
description="Super MCP Server"
command="{}"
command_args="serve --config {}"
command_background="yes"
pidfile="/run/${{RC_SVCNAME}}.pid"
output_log="/var/log/super-mcp.log"
error_log="/var/log/super-mcp.err"

depend() {{
    need net
}}
"#,
        binary_path,
        config_path
    );

    let init_path = PathBuf::from("/etc/init.d/super-mcp");

    async_fs::write(&init_path, init_content).await?;
    fs::set_permissions(&init_path, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;

    // Add to default runlevel
    let output = Command::new("rc-update")
        .args(["add", "super-mcp", "default"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Check if already in runlevel
        if !stderr.contains("already") {
            return Err(anyhow!("Failed to add to runlevel: {}", stderr));
        }
    }

    // Start the service
    let output = Command::new("rc-service")
        .args(["super-mcp", "start"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("Warning: Failed to start service (may already be running): {}", stderr);
    }

    println!("✓ Installed super-mcp as OpenRC service");
    println!("  Init script: /etc/init.d/super-mcp");
    println!("  Use 'rc-service super-mcp status' to check status");

    Ok(())
}

/// Uninstall from Linux OpenRC
async fn uninstall_openrc() -> AnyhowResult<()> {
    // Stop the service
    let _ = Command::new("rc-service")
        .args(["super-mcp", "stop"])
        .output();

    // Remove from runlevel
    let _ = Command::new("rc-update")
        .args(["del", "super-mcp"])
        .output();

    let init_path = PathBuf::from("/etc/init.d/super-mcp");

    if init_path.exists() {
        fs::remove_file(&init_path)?;
        println!("✓ Removed OpenRC init script");
    }

    println!("✓ Uninstalled super-mcp from OpenRC");
    Ok(())
}

/// Install using Linux runit
async fn install_runit(binary_path: &str, config_path: &str) -> AnyhowResult<()> {
    let service_dir = PathBuf::from("/etc/service/super-mcp");
    let run_file = service_dir.join("run");

    async_fs::create_dir_all(&service_dir).await?;

    let run_content = format!(r#"#!/bin/sh
exec {} serve --config {} 2>&1
"#,
        binary_path,
        config_path
    );

    async_fs::write(&run_file, run_content).await?;
    fs::set_permissions(&run_file, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;

    println!("✓ Installed super-mcp as runit service");
    println!("  Service directory: /etc/service/super-mcp");
    println!("  Use 'sv status super-mcp' to check status");

    Ok(())
}

/// Uninstall from Linux runit
async fn uninstall_runit() -> AnyhowResult<()> {
    let service_dir = PathBuf::from("/etc/service/super-mcp");

    if service_dir.exists() {
        // Stop the service (if sv is available)
        let _ = Command::new("sv")
            .args(["down", "super-mcp"])
            .output();

        async_fs::remove_dir_all(&service_dir).await?;
        println!("✓ Removed runit service directory");
    }

    println!("✓ Uninstalled super-mcp from runit");
    Ok(())
}

/// Install using Windows NSSM
async fn install_nssm(binary_path: &str, config_path: &str) -> AnyhowResult<()> {
    let binary_path = binary_path.replace('/', "\\");
    let config_path = config_path.replace('/', "\\");

    // Create the service
    let output = Command::new("nssm")
        .args([
            "install",
            "super-mcp",
            &binary_path,
        ])
        .arg("serve")
        .arg("--config")
        .arg(&config_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to install NSSM service: {}", stderr));
    }

    // Set additional parameters
    let _ = Command::new("nssm")
        .args(["set", "super-mcp", "AppStdout", r"C:\ProgramData\super-mcp\logs\stdout.log"])
        .output();

    let _ = Command::new("nssm")
        .args(["set", "super-mcp", "AppStderr", r"C:\ProgramData\super-mcp\logs\stderr.log"])
        .output();

    let _ = Command::new("nssm")
        .args(["set", "super-mcp", "AppDirectory", r"C:\Program Files\super-mcp"])
        .output();

    // Create logs directory
    let logs_dir = PathBuf::from(r"C:\ProgramData\super-mcp\logs");
    async_fs::create_dir_all(&logs_dir).await?;

    // Start the service
    let output = Command::new("nssm")
        .args(["start", "super-mcp"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("Warning: Failed to start service (may already be running): {}", stderr);
    }

    println!("✓ Installed super-mcp as NSSM service");
    println!("  Service name: super-mcp");
    println!("  Use 'nssm status super-mcp' to check status");

    Ok(())
}

/// Uninstall from Windows NSSM
async fn uninstall_nssm() -> AnyhowResult<()> {
    // Stop the service
    let _ = Command::new("nssm")
        .args(["stop", "super-mcp"])
        .output();

    // Remove the service
    let output = Command::new("nssm")
        .args(["remove", "super-mcp", "confirm"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Service might not exist
        if !stderr.contains("does not exist") {
            return Err(anyhow!("Failed to remove NSSM service: {}", stderr));
        }
    }

    println!("✓ Uninstalled super-mcp from NSSM");
    Ok(())
}

/// Install using Windows Task Scheduler
async fn install_schtasks(binary_path: &str, config_path: &str) -> AnyhowResult<()> {
    let binary_path = binary_path.replace('/', "\\");

    // Create task XML content
    let task_xml = format!(r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Super MCP Server</Description>
  </RegistrationInfo>
  <Principals>
    <Principal id="Author">
      <LogonType>Interactive</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>true</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{}</Command>
      <Arguments>serve --config {}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
        binary_path,
        config_path
    );

    // Write task XML to temp file
    let temp_xml = std::env::temp_dir().join("super-mcp-task.xml");
    async_fs::write(&temp_xml, task_xml).await?;

    // Create the task
    let output = Command::new("schtasks")
        .args(["/Create", "/TN", "super-mcp", "/XML", temp_xml.to_str().unwrap()])
        .output()?;

    // Clean up temp file
    async_fs::remove_file(&temp_xml).await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Failed to create scheduled task: {}", stderr));
    }

    // Run the task immediately
    let _ = Command::new("schtasks")
        .args(["/Run", "/TN", "super-mcp"])
        .output();

    println!("✓ Installed super-mcp as scheduled task");
    println!("  Task name: super-mcp");
    println!("  Use 'schtasks /Query /TN super-mcp' to check status");

    Ok(())
}

/// Uninstall from Windows Task Scheduler
async fn uninstall_schtasks() -> AnyhowResult<()> {
    // End the task
    let _ = Command::new("schtasks")
        .args(["/End", "/TN", "super-mcp"])
        .output();

    // Delete the task
    let output = Command::new("schtasks")
        .args(["/Delete", "/TN", "super-mcp", "/F"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Task might not exist
        if !stderr.contains("does not exist") {
            return Err(anyhow!("Failed to delete scheduled task: {}", stderr));
        }
    }

    println!("✓ Uninstalled super-mcp from Task Scheduler");
    Ok(())
}
