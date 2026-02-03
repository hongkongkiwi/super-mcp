//! Registry commands for searching and installing MCP servers

use crate::cli::{ensure_config_dir, expand_path};
use crate::config::{Config, McpServerConfig, SandboxConfig};
use crate::registry::{RegistryClient, RegistryEntry};
use crate::registry::types::RegistryConfig;
use crate::utils::errors::{McpError, McpResult};
use shellexpand::tilde;
use std::io::{self, Write};
use std::path::PathBuf;

fn create_registry_config(config: &Config) -> RegistryConfig {
    let cache_dir = tilde(&config.registry.cache_dir).to_string();
    RegistryConfig {
        url: config.registry.url.clone(),
        cache_dir: PathBuf::from(cache_dir),
        cache_ttl_hours: config.registry.cache_ttl_hours,
    }
}

/// Search for MCP servers in the registry
pub async fn search(config_path: &str, query: &str) -> McpResult<()> {
    let path = PathBuf::from(expand_path(config_path));

    // Load config to get registry settings
    let registry_config = if path.exists() {
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| McpError::ConfigError(format!("Failed to read config: {}", e)))?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| McpError::ConfigError(format!("Failed to parse config: {}", e)))?;
        create_registry_config(&config)
    } else {
        RegistryConfig::default()
    };

    let client = RegistryClient::new(registry_config)?;

    println!("Searching registry for: '{}'...\n", query);

    match client.search(query).await {
        Ok(results) => {
            if results.entries.is_empty() {
                println!("No servers found matching '{}'.", query);
                return Ok(());
            }

            println!("Found {} result(s):\n", results.total);

            for entry in &results.entries {
                print_entry_summary(entry);
                println!();
            }

            Ok(())
        }
        Err(e) => {
            println!("Search failed: {}", e);
            println!("\nNote: The registry service may not be available.");
            println!("You can still add servers manually using 'mcpo mcp add <name> <command>'");
            Err(e)
        }
    }
}

/// Install an MCP server from the registry
pub async fn install(config_path: &str, name: &str) -> McpResult<()> {
    let path = PathBuf::from(expand_path(config_path));
    ensure_config_dir(&path).await?;

    // Load config to get registry settings
    let registry_config = if path.exists() {
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| McpError::ConfigError(format!("Failed to read config: {}", e)))?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| McpError::ConfigError(format!("Failed to parse config: {}", e)))?;
        create_registry_config(&config)
    } else {
        RegistryConfig::default()
    };

    let client = RegistryClient::new(registry_config)?;

    println!("Looking up '{}' in registry...", name);

    match client.install(name).await {
        Ok(entry) => {
            println!("✓ Found server: {} v{}", entry.name, entry.version);

            if let Some(cmd) = &entry.install_command {
                println!("\nRegistry install command:\n  {}", cmd);
                if confirm_install()? {
                    run_install_command(cmd).await?;
                } else {
                    println!("Skipped install command.");
                }
            }

            // Load existing config or create new
            let mut config = if path.exists() {
                let content = tokio::fs::read_to_string(&path)
                    .await
                    .map_err(|e| McpError::ConfigError(format!("Failed to read config: {}", e)))?;
                toml::from_str::<Config>(&content)
                    .map_err(|e| McpError::ConfigError(format!("Failed to parse config: {}", e)))?
            } else {
                Config::default()
            };

            // Check if server already exists
            if config.servers.iter().any(|s| s.name == entry.name) {
                println!();
                println!("⚠ Server '{}' is already configured.", entry.name);
                println!("Use 'mcpo mcp remove {}' first if you want to replace it.", entry.name);
                return Ok(());
            }

            // Add the server from registry entry
            let server_config = McpServerConfig {
                name: entry.name.clone(),
                command: entry.command,
                args: entry.args,
                env: entry.env,
                tags: entry.tags,
                description: Some(entry.description),
                sandbox: SandboxConfig::default(),
            };

            config.servers.push(server_config);

            // Save config
            save_config(&path, &config).await?;

            println!("✓ Installed '{}' to your configuration.", entry.name);
            println!("\nTo use this server, run:");
            println!("  mcpo serve --config {}", path.display());

            Ok(())
        }
        Err(e) => {
            println!("Install failed: {}", e);
            println!("\nNote: The registry service may not be available.");
            println!("You can still add servers manually using 'mcpo mcp add <name> <command>'");
            Err(e)
        }
    }
}

/// Show detailed information about a registry entry
pub async fn info(config_path: &str, name: &str) -> McpResult<()> {
    let path = PathBuf::from(expand_path(config_path));

    // Load config to get registry settings
    let registry_config = if path.exists() {
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| McpError::ConfigError(format!("Failed to read config: {}", e)))?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| McpError::ConfigError(format!("Failed to parse config: {}", e)))?;
        create_registry_config(&config)
    } else {
        RegistryConfig::default()
    };

    let client = RegistryClient::new(registry_config)?;

    println!("Fetching information for '{}'...\n", name);

    match client.get_info(name).await {
        Ok(Some(entry)) => {
            print_entry_details(&entry);
            Ok(())
        }
        Ok(None) => {
            println!("Server '{}' not found in registry.", name);
            Ok(())
        }
        Err(e) => {
            println!("Failed to get info: {}", e);
            println!("\nNote: The registry service may not be available.");
            Err(e)
        }
    }
}

/// Refresh the registry cache
pub async fn refresh(config_path: &str) -> McpResult<()> {
    let path = PathBuf::from(expand_path(config_path));

    // Load config to get registry settings
    let registry_config = if path.exists() {
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| McpError::ConfigError(format!("Failed to read config: {}", e)))?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| McpError::ConfigError(format!("Failed to parse config: {}", e)))?;
        create_registry_config(&config)
    } else {
        RegistryConfig::default()
    };

    let client = RegistryClient::new(registry_config)?;

    println!("Refreshing registry cache...");

    match client.refresh_cache().await {
        Ok(_) => {
            println!("✓ Registry cache refreshed successfully.");
            Ok(())
        }
        Err(e) => {
            println!("Failed to refresh cache: {}", e);
            Err(e)
        }
    }
}

fn print_entry_summary(entry: &RegistryEntry) {
    println!("  {} v{}", entry.name, entry.version);
    println!("    {}", entry.description);
    println!("    Tags: {}", entry.tags.join(", "));
    println!("    Author: {} | License: {}", entry.author, entry.license);
}

fn confirm_install() -> McpResult<bool> {
    print!("Run install command now? [y/N]: ");
    io::stdout().flush().map_err(McpError::Io)?;

    let mut input = String::new();
    io::stdin().read_line(&mut input).map_err(McpError::Io)?;
    let input = input.trim().to_lowercase();
    Ok(matches!(input.as_str(), "y" | "yes"))
}

async fn run_install_command(install_cmd: &str) -> McpResult<()> {
    let parts = shell_words::split(install_cmd)
        .map_err(|e| McpError::ConfigError(format!("Invalid install command: {}", e)))?;
    if parts.is_empty() {
        return Err(McpError::ConfigError("Empty install command".to_string()));
    }

    let output = tokio::process::Command::new(&parts[0])
        .args(&parts[1..])
        .output()
        .await
        .map_err(McpError::Io)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(McpError::InternalError(format!(
            "Install command failed: {}",
            stderr
        )));
    }

    Ok(())
}

fn print_entry_details(entry: &RegistryEntry) {
    println!("{}", "=".repeat(60));
    println!("  {}", entry.name);
    println!("{}", "=".repeat(60));
    println!();
    println!("Version:     {}", entry.version);
    println!("Description: {}", entry.description);
    println!("Author:      {}", entry.author);
    println!("License:     {}", entry.license);
    println!("Tags:        {}", entry.tags.join(", "));
    if let Some(repo) = &entry.repository {
        println!("Repository:  {}", repo);
    }
    if let Some(homepage) = &entry.homepage {
        println!("Homepage:    {}", homepage);
    }
    println!();
    println!("Command:     {}", entry.command);
    if !entry.args.is_empty() {
        println!("Arguments:   {}", entry.args.join(" "));
    }
    if !entry.env.is_empty() {
        println!("Environment:");
        for (key, value) in &entry.env {
            println!("  {}={}", key, value);
        }
    }
    if let Some(cmd) = &entry.install_command {
        println!();
        println!("Install command: {}", cmd);
    }
    if let Some(schema) = &entry.schema {
        println!();
        println!("Schema available: Yes");
        if let Some(props) = schema.get("properties") {
            println!("Configuration properties:");
            if let Some(obj) = props.as_object() {
                for (key, val) in obj {
                    let desc = val.get("description")
                        .and_then(|d| d.as_str())
                        .unwrap_or("No description");
                    let required = val.get("required")
                        .and_then(|r| r.as_bool())
                        .unwrap_or(false);
                    let req_str = if required { " (required)" } else { "" };
                    println!("  • {}{}: {}", key, req_str, desc);
                }
            }
        }
    }
    println!();
    println!("To install this server, run:");
    println!("  mcpo registry install {}", entry.name);
}

async fn save_config(path: &PathBuf, config: &Config) -> McpResult<()> {
    let content = toml::to_string_pretty(config)
        .map_err(|e| McpError::ConfigError(format!("Failed to serialize config: {}", e)))?;
    tokio::fs::write(path, content)
        .await
        .map_err(|e| McpError::ConfigError(format!("Failed to write config: {}", e)))?;
    Ok(())
}
