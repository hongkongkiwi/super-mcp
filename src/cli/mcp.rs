//! MCP server management commands

use crate::cli::{ensure_config_dir, expand_path};
use crate::config::{Config, McpServerConfig, SandboxConfig};
use crate::utils::errors::{McpError, McpResult};
use std::collections::HashMap;
use std::path::PathBuf;

/// Add a new MCP server
pub async fn add(
    config_path: &str,
    name: &str,
    command: &str,
    args: Option<Vec<String>>,
    env: Option<Vec<String>>,
    tags: Option<Vec<String>>,
    description: Option<String>,
) -> McpResult<()> {
    let path = PathBuf::from(expand_path(config_path));
    ensure_config_dir(&path).await?;

    // Load existing config or create new
    let mut config = if path.exists() {
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| McpError::ConfigError(format!("Failed to read config: {}", e)))?;
        toml::from_str(&content)
            .map_err(|e| McpError::ConfigError(format!("Failed to parse config: {}", e)))?
    } else {
        Config::default()
    };

    // Check if server already exists
    if config.servers.iter().any(|s| s.name == name) {
        return Err(McpError::ConfigError(format!(
            "Server '{}' already exists. Use 'mcpo mcp remove {}' first if you want to replace it.",
            name, name
        )));
    }

    // Parse environment variables
    let env_map = parse_env_vars(env.unwrap_or_default())?;

    // Create new server config
    let server_config = McpServerConfig {
        name: name.to_string(),
        command: command.to_string(),
        args: args.unwrap_or_default(),
        env: env_map,
        tags: tags.unwrap_or_default(),
        description,
        sandbox: SandboxConfig::default(),
        runner: None,
    };

    config.servers.push(server_config);

    // Save config
    save_config(&path, &config).await?;

    println!("✓ Added MCP server '{}'", name);
    Ok(())
}

/// List all MCP servers
pub async fn list(config_path: &str) -> McpResult<()> {
    let path = PathBuf::from(expand_path(config_path));

    if !path.exists() {
        println!("No configuration file found at {}", path.display());
        return Ok(());
    }

    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| McpError::ConfigError(format!("Failed to read config: {}", e)))?;
    let config: Config = toml::from_str(&content)
        .map_err(|e| McpError::ConfigError(format!("Failed to parse config: {}", e)))?;

    if config.servers.is_empty() {
        println!("No MCP servers configured.");
        println!("Use 'mcpo mcp add <name> <command>' to add a server.");
        return Ok(());
    }

    println!("\n{:<20} {:<30} {:<15}", "NAME", "COMMAND", "TAGS");
    println!("{}", "-".repeat(70));

    for server in &config.servers {
        let cmd_display = if server.args.is_empty() {
            server.command.clone()
        } else {
            format!("{} {}", server.command, server.args.join(" "))
        };
        let cmd_display = if cmd_display.len() > 28 {
            format!("{}...", &cmd_display[..25])
        } else {
            cmd_display
        };
        let tags = if server.tags.is_empty() {
            "-".to_string()
        } else {
            server.tags.join(", ")
        };
        println!("{:<20} {:<30} {:<15}", server.name, cmd_display, tags);
    }

    println!("\nTotal: {} server(s)", config.servers.len());
    Ok(())
}

/// Remove an MCP server
pub async fn remove(config_path: &str, name: &str) -> McpResult<()> {
    let path = PathBuf::from(expand_path(config_path));

    if !path.exists() {
        return Err(McpError::ConfigError(format!(
            "Configuration file not found: {}",
            path.display()
        )));
    }

    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| McpError::ConfigError(format!("Failed to read config: {}", e)))?;
    let mut config: Config = toml::from_str(&content)
        .map_err(|e| McpError::ConfigError(format!("Failed to parse config: {}", e)))?;

    let initial_len = config.servers.len();
    config.servers.retain(|s| s.name != name);

    if config.servers.len() == initial_len {
        return Err(McpError::ServerNotFound(format!(
            "Server '{}' not found",
            name
        )));
    }

    save_config(&path, &config).await?;

    println!("✓ Removed MCP server '{}'", name);
    Ok(())
}

/// Show MCP server status
pub async fn status(config_path: &str, name: Option<&str>) -> McpResult<()> {
    let path = PathBuf::from(expand_path(config_path));

    if !path.exists() {
        println!("No configuration file found at {}", path.display());
        return Ok(());
    }

    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| McpError::ConfigError(format!("Failed to read config: {}", e)))?;
    let config: Config = toml::from_str(&content)
        .map_err(|e| McpError::ConfigError(format!("Failed to parse config: {}", e)))?;

    match name {
        Some(server_name) => {
            let server = config
                .servers
                .iter()
                .find(|s| s.name == server_name)
                .ok_or_else(|| McpError::ServerNotFound(format!("Server '{}' not found", server_name)))?;

            print_server_details(server);
        }
        None => {
            if config.servers.is_empty() {
                println!("No MCP servers configured.");
                return Ok(());
            }

            for server in &config.servers {
                print_server_details(server);
                println!();
            }
        }
    }

    Ok(())
}

/// Edit an MCP server (update command, args, env, etc.)
pub async fn edit(
    config_path: &str,
    name: &str,
    command: Option<&str>,
    args: Option<Vec<String>>,
    env: Option<Vec<String>>,
    tags: Option<Vec<String>>,
    description: Option<String>,
) -> McpResult<()> {
    let path = PathBuf::from(expand_path(config_path));

    if !path.exists() {
        return Err(McpError::ConfigError(format!(
            "Configuration file not found: {}",
            path.display()
        )));
    }

    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| McpError::ConfigError(format!("Failed to read config: {}", e)))?;
    let mut config: Config = toml::from_str(&content)
        .map_err(|e| McpError::ConfigError(format!("Failed to parse config: {}", e)))?;

    let server = config
        .servers
        .iter_mut()
        .find(|s| s.name == name)
        .ok_or_else(|| McpError::ServerNotFound(format!("Server '{}' not found", name)))?;

    if let Some(cmd) = command {
        server.command = cmd.to_string();
    }
    if let Some(a) = args {
        server.args = a;
    }
    if let Some(e) = env {
        server.env = parse_env_vars(e)?;
    }
    if let Some(t) = tags {
        server.tags = t;
    }
    if let Some(d) = description {
        server.description = Some(d);
    }

    save_config(&path, &config).await?;

    println!("✓ Updated MCP server '{}'", name);
    Ok(())
}

fn print_server_details(server: &McpServerConfig) {
    println!("Server: {}", server.name);
    println!("  Command: {} {}", server.command, server.args.join(" "));
    if let Some(desc) = &server.description {
        println!("  Description: {}", desc);
    }
    if !server.tags.is_empty() {
        println!("  Tags: {}", server.tags.join(", "));
    }
    if !server.env.is_empty() {
        println!("  Environment:");
        for (key, value) in &server.env {
            println!("    {}={}", key, value);
        }
    }
    println!("  Sandbox: enabled={}, network={}", server.sandbox.enabled, server.sandbox.network);
}

fn parse_env_vars(env_vars: Vec<String>) -> McpResult<HashMap<String, String>> {
    let mut map = HashMap::new();
    for var in env_vars {
        let parts: Vec<&str> = var.splitn(2, '=').collect();
        if parts.len() != 2 {
            return Err(McpError::ConfigError(format!(
                "Invalid environment variable format: {}. Use KEY=value",
                var
            )));
        }
        map.insert(parts[0].to_string(), parts[1].to_string());
    }
    Ok(map)
}

async fn save_config(path: &PathBuf, config: &Config) -> McpResult<()> {
    let content = toml::to_string_pretty(config)
        .map_err(|e| McpError::ConfigError(format!("Failed to serialize config: {}", e)))?;
    tokio::fs::write(path, content)
        .await
        .map_err(|e| McpError::ConfigError(format!("Failed to write config: {}", e)))?;
    Ok(())
}
