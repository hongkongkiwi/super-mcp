//! CLI commands for runtime management

use crate::cli::expand_path;
use crate::utils::errors::McpResult;
use serde_json::json;
use std::collections::HashMap;

/// Runtime type string representation
#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeTypeCli {
    PythonWasm,
    NodePnpm,
    NodeNpm,
    NodeBun,
}

impl std::str::FromStr for RuntimeTypeCli {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "python_wasm" | "python-wasm" | "pythonwasm" | "python" => Ok(RuntimeTypeCli::PythonWasm),
            "node_pnpm" | "node-pnpm" | "nodepnpm" | "pnpm" => Ok(RuntimeTypeCli::NodePnpm),
            "node_npm" | "node-npm" | "nodenpm" | "npm" => Ok(RuntimeTypeCli::NodeNpm),
            "node_bun" | "node-bun" | "nodebun" | "bun" => Ok(RuntimeTypeCli::NodeBun),
            _ => Err(format!("Unknown runtime type: {}. Valid types are: python_wasm, node_pnpm, node_npm, node_bun", s)),
        }
    }
}

impl RuntimeTypeCli {
    /// Convert to config RuntimeType
    pub fn to_config_type(&self) -> crate::runtime::types::RuntimeType {
        match self {
            RuntimeTypeCli::PythonWasm => crate::runtime::types::RuntimeType::PythonWasm,
            RuntimeTypeCli::NodePnpm => crate::runtime::types::RuntimeType::NodePnpm,
            RuntimeTypeCli::NodeNpm => crate::runtime::types::RuntimeType::NodeNpm,
            RuntimeTypeCli::NodeBun => crate::runtime::types::RuntimeType::NodeBun,
        }
    }

    /// Get human-readable name
    pub fn name(&self) -> &'static str {
        match self {
            RuntimeTypeCli::PythonWasm => "Python (WASM)",
            RuntimeTypeCli::NodePnpm => "Node.js (pnpm)",
            RuntimeTypeCli::NodeNpm => "Node.js (npm)",
            RuntimeTypeCli::NodeBun => "Node.js (bun)",
        }
    }
}

/// Add a new runtime
#[allow(clippy::too_many_arguments)]
pub async fn add(
    config_path: &str,
    name: &str,
    type_str: &str,
    packages: Option<Vec<String>>,
    working_dir: Option<String>,
    env: Option<Vec<String>>,
    max_memory: Option<u64>,
    max_cpu: Option<u32>,
    timeout: Option<u64>,
    network: Option<bool>,
) -> McpResult<()> {
    use crate::config::ConfigManager;
    use crate::utils::errors::McpError;

    let type_ = type_str.parse::<RuntimeTypeCli>()
        .map_err(McpError::ConfigError)?;

    let expanded_path = expand_path(config_path);
    let config_manager = ConfigManager::new(&expanded_path).await?;
    let mut config = config_manager.get_config();

    // Check if runtime with same name exists
    if config.runtimes.iter().any(|r| r.name == name) {
        return Err(McpError::ConfigError(format!("Runtime '{}' already exists", name)));
    }

    // Parse environment variables
    let mut env_map = HashMap::new();
    if let Some(env_vars) = env {
        for env_var in env_vars {
            if let Some((key, value)) = env_var.split_once('=') {
                env_map.insert(key.to_string(), value.to_string());
            }
        }
    }

    // Create runtime config
    let runtime_config = crate::runtime::types::RuntimeConfig {
        name: name.to_string(),
        type_: type_.to_config_type(),
        packages: packages.unwrap_or_default(),
        working_dir,
        env: env_map,
        resource_limits: crate::runtime::types::ResourceLimits {
            max_memory_mb: max_memory.unwrap_or(512),
            max_cpu_percent: max_cpu.unwrap_or(50),
            timeout_seconds: timeout.unwrap_or(30),
            network_access: network.unwrap_or(false),
            filesystem: crate::runtime::types::RuntimeFilesystemAccess::ReadOnly,
        },
        enabled: true,
    };

    config.runtimes.push(runtime_config);

    // Save configuration
    config_manager.save(&config).await?;
    println!("Added runtime '{}' ({})", name, type_.name());

    Ok(())
}

/// List all runtimes
pub async fn list(config_path: &str, json_output: bool) -> McpResult<()> {
    use crate::config::ConfigManager;

    let expanded_path = expand_path(config_path);
    let config_manager = ConfigManager::new(&expanded_path).await?;
    let config = config_manager.get_config();

    if config.runtimes.is_empty() {
        println!("No runtimes configured.");
        println!("\nAdd a runtime with:");
        println!("  supermcp runtime add <name> <type>");
        return Ok(());
    }

    if json_output {
        let json_runtimes: Vec<_> = config.runtimes.iter().map(|r| {
            json!({
                "name": r.name,
                "type": format!("{:?}", r.type_),
                "enabled": r.enabled,
                "packages": r.packages,
                "resource_limits": {
                    "max_memory_mb": r.resource_limits.max_memory_mb,
                    "max_cpu_percent": r.resource_limits.max_cpu_percent,
                    "timeout_seconds": r.resource_limits.timeout_seconds,
                    "network_access": r.resource_limits.network_access,
                }
            })
        }).collect();

        let output = json!({
            "runtimes": json_runtimes,
            "count": config.runtimes.len()
        });

        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Configured Runtimes:");
        println!("--------------------");

        for (i, runtime) in config.runtimes.iter().enumerate() {
            let status = if runtime.enabled { "enabled" } else { "disabled" };
            let type_name = match runtime.type_ {
                crate::runtime::types::RuntimeType::PythonWasm => "Python (WASM)",
                crate::runtime::types::RuntimeType::NodePnpm => "Node.js (pnpm)",
                crate::runtime::types::RuntimeType::NodeNpm => "Node.js (npm)",
                crate::runtime::types::RuntimeType::NodeBun => "Node.js (bun)",
            };

            println!("{}. {} ({}) - {}", i + 1, runtime.name, type_name, status);

            if !runtime.packages.is_empty() {
                println!("   Packages: {}", runtime.packages.join(", "));
            }

            println!("   Memory limit: {} MB", runtime.resource_limits.max_memory_mb);
            println!("   CPU limit: {}%", runtime.resource_limits.max_cpu_percent);
            println!("   Timeout: {}s", runtime.resource_limits.timeout_seconds);
            println!("   Network access: {}", if runtime.resource_limits.network_access { "allowed" } else { "blocked" });
            println!();
        }
    }

    Ok(())
}

/// Remove a runtime
pub async fn remove(config_path: &str, name: &str) -> McpResult<()> {
    use crate::config::ConfigManager;
    use crate::utils::errors::McpError;

    let expanded_path = expand_path(config_path);
    let config_manager = ConfigManager::new(&expanded_path).await?;
    let mut config = config_manager.get_config();

    let original_len = config.runtimes.len();
    config.runtimes.retain(|r| r.name != name);

    if config.runtimes.len() == original_len {
        return Err(McpError::ConfigError(format!("Runtime '{}' not found", name)));
    }

    config_manager.save(&config).await?;
    println!("Removed runtime '{}'", name);

    Ok(())
}

/// Show runtime information
pub async fn info(config_path: &str, name: &str) -> McpResult<()> {
    use crate::config::ConfigManager;
    use crate::utils::errors::McpError;

    let expanded_path = expand_path(config_path);
    let config_manager = ConfigManager::new(&expanded_path).await?;
    let config = config_manager.get_config();

    let runtime = config.runtimes.iter()
        .find(|r| r.name == name)
        .ok_or_else(|| McpError::ConfigError(format!("Runtime '{}' not found", name)))?;

    let type_name = match runtime.type_ {
        crate::runtime::types::RuntimeType::PythonWasm => "Python (WASM)",
        crate::runtime::types::RuntimeType::NodePnpm => "Node.js (pnpm)",
        crate::runtime::types::RuntimeType::NodeNpm => "Node.js (npm)",
        crate::runtime::types::RuntimeType::NodeBun => "Node.js (bun)",
    };

    println!("Runtime: {}", runtime.name);
    println!("Type: {}", type_name);
    println!("Status: {}", if runtime.enabled { "enabled" } else { "disabled" });

    if !runtime.packages.is_empty() {
        println!("\nPackages:");
        for pkg in &runtime.packages {
            println!("  - {}", pkg);
        }
    }

    if let Some(ref wd) = runtime.working_dir {
        println!("\nWorking directory: {}", wd);
    }

    println!("\nResource Limits:");
    println!("  Memory: {} MB", runtime.resource_limits.max_memory_mb);
    println!("  CPU: {}%", runtime.resource_limits.max_cpu_percent);
    println!("  Timeout: {}s", runtime.resource_limits.timeout_seconds);
    println!("  Network: {}", if runtime.resource_limits.network_access { "allowed" } else { "blocked" });

    if !runtime.env.is_empty() {
        println!("\nEnvironment variables:");
        for (key, value) in &runtime.env {
            println!("  {}={}", key, value);
        }
    }

    Ok(())
}

/// Validate all runtimes
pub async fn validate(config_path: &str) -> McpResult<()> {
    use crate::config::ConfigManager;
    use crate::runtime::RuntimeManager;
    use crate::utils::errors::McpError;

    let expanded_path = expand_path(config_path);
    let config_manager = ConfigManager::new(&expanded_path).await?;
    let config = config_manager.get_config();

    if config.runtimes.is_empty() {
        println!("No runtimes configured to validate.");
        return Ok(());
    }

    println!("Validating runtimes...\n");

    let manager = RuntimeManager::new();

    // Register all runtimes
    for runtime_config in &config.runtimes {
        let _ = manager.register_auto(runtime_config.clone());
    }

    let results = manager.validate_all().await;

    let mut all_valid = true;
    for (name, result) in results {
        match result {
            Ok(()) => println!("[OK] {} - validated successfully", name),
            Err(e) => {
                println!("[FAIL] {} - {}", name, e);
                all_valid = false;
            }
        }
    }

    if all_valid {
        println!("\nAll runtimes are valid.");
    } else {
        return Err(McpError::ConfigError("Some runtimes failed validation".to_string()));
    }

    Ok(())
}

/// Execute a script using a runtime
pub async fn execute(
    config_path: &str,
    runtime_name: &str,
    script: Option<&str>,
    file: Option<String>,
) -> McpResult<()> {
    use crate::config::ConfigManager;
    use crate::runtime::RuntimeManager;
    use crate::utils::errors::McpError;

    let expanded_path = expand_path(config_path);
    let config_manager = ConfigManager::new(&expanded_path).await?;
    let config = config_manager.get_config();

    let manager = RuntimeManager::new();

    // Register all runtimes
    for runtime_config in &config.runtimes {
        let _ = manager.register_auto(runtime_config.clone());
    }

    // Find the runtime
    let runtime = manager.get(runtime_name)
        .ok_or_else(|| McpError::ConfigError(format!("Runtime '{}' not found", runtime_name)))?;

    let result = if let Some(ref file_path) = file {
        let path = std::path::PathBuf::from(file_path);
        runtime.runtime().execute_file(&path, None).await
    } else if let Some(script_content) = script {
        runtime.runtime().execute(script_content, None).await
    } else {
        return Err(McpError::ConfigError("Either script or file must be provided".to_string()));
    };

    match result {
        Ok(result) => {
            println!("Execution completed in {}ms", result.execution_time_ms);
            println!("Exit code: {}", result.exit_code);

            if !result.stdout.is_empty() {
                println!("\n--- STDOUT ---");
                println!("{}", result.stdout);
            }

            if !result.stderr.is_empty() {
                println!("\n--- STDERR ---");
                println!("{}", result.stderr);
            }

            if let Some(ref value) = result.output_value {
                println!("\n--- Output Value ---");
                println!("{}", serde_json::to_string_pretty(value)?);
            }
        }
        Err(e) => {
            return Err(McpError::ConfigError(format!("Execution failed: {}", e)));
        }
    }

    Ok(())
}
