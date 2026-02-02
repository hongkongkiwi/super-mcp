//! WebAssembly sandbox for MCP servers
//!
//! This module implements sandboxing using WebAssembly, providing:
//! - Cross-platform isolation (works on any platform with WASM runtime)
//! - Fine-grained capability-based security
//! - Near-native performance
//! - No OS-level privileges required

use crate::config::McpServerConfig;
use crate::sandbox::traits::{FilesystemConstraint, Sandbox, SandboxConstraints};
use crate::utils::errors::{McpError, McpResult};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::process::Child;
use tracing::{debug, error, info, warn};

/// WASM sandbox configuration
#[derive(Debug, Clone)]
pub struct WasmSandboxConfig {
    /// WASM runtime to use
    pub runtime: WasmRuntime,
    /// Maximum memory per instance (in MB)
    pub max_memory_mb: u64,
    /// Enable WASI (WebAssembly System Interface)
    pub enable_wasi: bool,
    /// Allowed host functions
    pub allowed_host_functions: Vec<String>,
    /// Pre-opened directories for WASI
    pub preopened_dirs: Vec<String>,
    /// Environment variables to expose
    pub env_vars: HashMap<String, String>,
}

impl Default for WasmSandboxConfig {
    fn default() -> Self {
        Self {
            runtime: WasmRuntime::default(),
            max_memory_mb: 512,
            enable_wasi: true,
            allowed_host_functions: vec![],
            preopened_dirs: vec![],
            env_vars: HashMap::new(),
        }
    }
}

/// WASM runtime backend
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmRuntime {
    /// Wasmtime (Bytecode Alliance)
    Wasmtime,
    /// Wasmer
    Wasmer,
    /// WAMR (WebAssembly Micro Runtime) - for embedded
    Wamr,
}

impl Default for WasmRuntime {
    fn default() -> Self {
        WasmRuntime::Wasmtime
    }
}

/// WASM sandbox implementation
pub struct WasmSandbox {
    constraints: SandboxConstraints,
    config: WasmSandboxConfig,
}

impl WasmSandbox {
    /// Create a new WASM sandbox from server configuration
    pub fn from_config(server_config: &McpServerConfig) -> Self {
        let constraints = SandboxConstraints {
            network: server_config.sandbox.network,
            filesystem: match &server_config.sandbox.filesystem {
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
            env_inherit: server_config.sandbox.env_inherit,
            max_memory_mb: server_config.sandbox.max_memory_mb,
            max_cpu_percent: server_config.sandbox.max_cpu_percent,
        };

        let wasm_config = WasmSandboxConfig {
            max_memory_mb: server_config.sandbox.max_memory_mb,
            ..Default::default()
        };

        Self {
            constraints,
            config: wasm_config,
        }
    }

    /// Check if WASM sandboxing is available
    pub fn is_available() -> bool {
        // Check for wasmtime or wasmer installation
        which::which("wasmtime").is_ok() || which::which("wasmer").is_ok()
    }

    /// Build the WASM runtime command
    fn build_wasm_command(&self, wasm_path: &str) -> tokio::process::Command {
        let mut cmd = match self.config.runtime {
            WasmRuntime::Wasmtime => {
                let mut c = tokio::process::Command::new("wasmtime");
                c.arg("run");
                
                // Memory limit
                c.arg(format!("--max-memory={}mb", self.config.max_memory_mb));
                
                // WASI
                if self.config.enable_wasi {
                    c.arg("--wasi");
                    
                    // Pre-opened directories
                    for dir in &self.config.preopened_dirs {
                        c.arg("--dir").arg(dir);
                    }
                }
                
                // Networking
                if self.constraints.network {
                    c.arg("--allow-net");
                }
                
                c.arg(wasm_path);
                c
            }
            WasmRuntime::Wasmer => {
                let mut c = tokio::process::Command::new("wasmer");
                c.arg("run");
                
                // Enable WASI
                if self.config.enable_wasi {
                    c.arg("--enable-all");
                }
                
                // Environment variables
                for (key, value) in &self.config.env_vars {
                    c.env(key, value);
                }
                
                c.arg(wasm_path);
                c
            }
            WasmRuntime::Wamr => {
                // WAMR is typically embedded, use iwasm CLI
                let mut c = tokio::process::Command::new("iwasm");
                c.arg(wasm_path);
                c
            }
        };

        // Set resource limits
        cmd.env("WASM_MEMORY_LIMIT", format!("{}mb", self.config.max_memory_mb));
        
        cmd
    }

    /// Convert a native command to WASM if possible
    /// For now, this assumes the command is already a WASM module
    fn resolve_wasm_path(&self, command: &str) -> Option<String> {
        // Check if it's already a .wasm file
        if command.ends_with(".wasm") {
            return Some(command.to_string());
        }

        // Check for companion .wasm file
        let wasm_path = format!("{}.wasm", command);
        if std::path::Path::new(&wasm_path).exists() {
            return Some(wasm_path);
        }

        // Check in standard locations
        let locations = [
            format!("./{}.wasm", command),
            format!("/usr/local/lib/super-mcp/wasm/{}.wasm", command),
            format!("{}/.super-mcp/wasm/{}.wasm", dirs::home_dir()?.display(), command),
        ];

        for loc in &locations {
            if std::path::Path::new(loc).exists() {
                return Some(loc.clone());
            }
        }

        None
    }
}

#[async_trait]
impl Sandbox for WasmSandbox {
    async fn spawn(&self, config: &McpServerConfig) -> McpResult<Child> {
        // Try to resolve WASM path
        let wasm_path = self.resolve_wasm_path(&config.command)
            .ok_or_else(|| McpError::SandboxError(
                format!("Could not find WASM module for command: {}. ", config.command) +
                "Make sure the .wasm file exists or the command is a WASM module."
            ))?;

        info!("Spawning WASM sandbox: {}", wasm_path);

        let mut cmd = self.build_wasm_command(&wasm_path);
        
        // Add arguments
        cmd.args(&config.args);

        // Set up environment
        if !self.constraints.env_inherit {
            cmd.env_clear();
        }
        
        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        // Add WASM-specific environment
        for (key, value) in &self.config.env_vars {
            cmd.env(key, value);
        }

        // Setup stdio
        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Spawn the process
        let child = cmd.spawn().map_err(|e| {
            McpError::SandboxError(format!(
                "Failed to spawn WASM runtime: {}. ", e) +
                "Make sure wasmtime or wasmer is installed."
            ))
        })?;

        info!("WASM sandbox spawned with PID: {:?}", child.id());
        Ok(child)
    }

    fn constraints(&self) -> &SandboxConstraints {
        &self.constraints
    }
}

/// WASI (WebAssembly System Interface) capabilities
#[derive(Debug, Clone)]
pub struct WasiCapabilities {
    /// File access
    pub file_access: WasiFileAccess,
    /// Network access
    pub network: bool,
    /// Clock access
    pub clock: bool,
    /// Random access
    pub random: bool,
}

impl Default for WasiCapabilities {
    fn default() -> Self {
        Self {
            file_access: WasiFileAccess::ReadOnly,
            network: false,
            clock: true,
            random: true,
        }
    }
}

/// WASI file access level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasiFileAccess {
    None,
    ReadOnly,
    ReadWrite,
}

/// Helper to compile a native binary to WASM
/// This is a placeholder - actual compilation would require:
/// - WASI SDK for C/C++
/// - wasm32-wasi target for Rust
/// - Custom toolchain for other languages
pub fn compile_to_wasm(source_path: &str) -> McpResult<String> {
    // Check file extension
    let path = std::path::Path::new(source_path);
    let extension = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    match extension {
        "rs" => {
            // Rust - use cargo build --target wasm32-wasi
            info!("Compiling Rust file to WASM: {}", source_path);
            Err(McpError::ConfigError(
                "Auto-compilation not implemented. Use: cargo build --target wasm32-wasi".to_string()
            ))
        }
        "c" | "cpp" | "cc" => {
            // C/C++ - use WASI SDK
            info!("Compiling C/C++ file to WASM: {}", source_path);
            Err(McpError::ConfigError(
                "Auto-compilation not implemented. Use WASI SDK: https://github.com/WebAssembly/wasi-sdk".to_string()
            ))
        }
        "wasm" => {
            // Already WASM
            Ok(source_path.to_string())
        }
        _ => {
            Err(McpError::ConfigError(format!(
                "Cannot compile .{} files to WASM. Supported: .rs, .c, .cpp, .wasm",
                extension
            )))
        }
    }
}

/// WASM module validation
pub fn validate_wasm_module(wasm_path: &str) -> McpResult<bool> {
    use std::fs;
    
    let bytes = fs::read(wasm_path).map_err(|e| {
        McpError::ConfigError(format!("Failed to read WASM file: {}", e))
    })?;

    // Check magic number: \0asm
    if bytes.len() < 4 || &bytes[0..4] != &[0x00, 0x61, 0x73, 0x6d] {
        return Ok(false);
    }

    // Check version: 1 (little-endian)
    if bytes.len() >= 8 && &bytes[4..8] == &[0x01, 0x00, 0x00, 0x00] {
        return Ok(true);
    }

    // Could be a different version, but still valid WASM
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wasm_sandbox_config_default() {
        let config = WasmSandboxConfig::default();
        assert_eq!(config.runtime, WasmRuntime::Wasmtime);
        assert_eq!(config.max_memory_mb, 512);
        assert!(config.enable_wasi);
    }

    #[test]
    fn test_wasm_sandbox_from_config() {
        let server_config = McpServerConfig {
            name: "test".to_string(),
            command: "test.wasm".to_string(),
            args: vec![],
            env: Default::default(),
            tags: vec![],
            description: None,
            sandbox: crate::config::SandboxConfig {
                network: true,
                max_memory_mb: 256,
                ..Default::default()
            },
            runner: None,
        };

        let sandbox = WasmSandbox::from_config(&server_config);
        assert!(sandbox.constraints.network);
        assert_eq!(sandbox.config.max_memory_mb, 256);
    }

    #[test]
    fn test_wasi_capabilities_default() {
        let caps = WasiCapabilities::default();
        assert!(caps.clock);
        assert!(caps.random);
        assert!(!caps.network);
        assert_eq!(caps.file_access, WasiFileAccess::ReadOnly);
    }

    #[test]
    fn test_validate_wasm_module_invalid() {
        // Create a temporary invalid file
        let result = validate_wasm_module("/nonexistent.wasm");
        assert!(result.is_err());
    }
}
