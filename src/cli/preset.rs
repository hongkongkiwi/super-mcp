//! Preset management commands

use crate::cli::{ensure_config_dir, expand_path};
use crate::config::{Config, PresetConfig};
use crate::utils::errors::{McpError, McpResult};
use std::io::{self, Write};
use std::path::PathBuf;

/// Create a new preset
pub async fn create(
    config_path: &str,
    name: &str,
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

    // Check if preset already exists
    if config.presets.iter().any(|p| p.name == name) {
        return Err(McpError::ConfigError(format!(
            "Preset '{}' already exists. Use 'mcpo preset remove {}' first if you want to replace it.",
            name, name
        )));
    }

    // If no tags provided, prompt for them
    let tags = match tags {
        Some(t) if !t.is_empty() => t,
        _ => {
            println!("Enter tags for this preset (comma-separated, matching server tags):");
            let mut input = String::new();
            io::stdin().read_line(&mut input).map_err(|e| McpError::Io(e))?;
            input
                .trim()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        }
    };

    // If no description provided, prompt for it
    let description = match description {
        Some(d) => Some(d),
        None => {
            print!("Enter description for this preset (optional): ");
            io::stdout().flush().map_err(|e| McpError::Io(e))?;
            let mut input = String::new();
            io::stdin().read_line(&mut input).map_err(|e| McpError::Io(e))?;
            let trimmed = input.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
    };

    let preset = PresetConfig {
        name: name.to_string(),
        tags,
        description,
    };

    config.presets.push(preset);

    // Save config
    save_config(&path, &config).await?;

    println!("✓ Created preset '{}'", name);
    Ok(())
}

/// List all presets
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

    if config.presets.is_empty() {
        println!("No presets configured.");
        println!("Use 'mcpo preset create <name>' to create a preset.");
        return Ok(());
    }

    println!("\n{:<20} {:<30} {:<30}", "NAME", "TAGS", "DESCRIPTION");
    println!("{}", "-".repeat(85));

    for preset in &config.presets {
        let tags = preset.tags.join(", ");
        let tags_display = if tags.len() > 28 {
            format!("{}...", &tags[..25])
        } else {
            tags
        };
        let desc = preset.description.as_deref().unwrap_or("-");
        let desc_display = if desc.len() > 28 {
            format!("{}...", &desc[..25])
        } else {
            desc.to_string()
        };
        println!(
            "{:<20} {:<30} {:<30}",
            preset.name, tags_display, desc_display
        );
    }

    println!("\nTotal: {} preset(s)", config.presets.len());
    Ok(())
}

/// Edit a preset
pub async fn edit(
    config_path: &str,
    name: &str,
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

    let preset = config
        .presets
        .iter_mut()
        .find(|p| p.name == name)
        .ok_or_else(|| McpError::ConfigError(format!("Preset '{}' not found", name)))?;

    if let Some(t) = tags {
        preset.tags = t;
    }
    if let Some(d) = description {
        preset.description = Some(d);
    }

    save_config(&path, &config).await?;

    println!("✓ Updated preset '{}'", name);
    Ok(())
}

/// Test a preset (shows which servers would be included)
pub async fn test(config_path: &str, name: &str) -> McpResult<()> {
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
    let config: Config = toml::from_str(&content)
        .map_err(|e| McpError::ConfigError(format!("Failed to parse config: {}", e)))?;

    let preset = config
        .presets
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| McpError::ConfigError(format!("Preset '{}' not found", name)))?;

    println!("\nPreset: {}", preset.name);
    if let Some(desc) = &preset.description {
        println!("Description: {}", desc);
    }
    println!("Tags: {}", preset.tags.join(", "));

    // Find matching servers
    let preset_tags: std::collections::HashSet<_> = preset.tags.iter().cloned().collect();
    let matching_servers: Vec<_> = config
        .servers
        .iter()
        .filter(|s| s.tags.iter().any(|tag| preset_tags.contains(tag)))
        .collect();

    if matching_servers.is_empty() {
        println!("\n⚠ No servers match this preset's tags.");
        println!("Servers with matching tags would be included when using this preset.");
    } else {
        println!("\nMatching servers ({}):", matching_servers.len());
        for server in matching_servers {
            let matching_tags: Vec<_> = server
                .tags
                .iter()
                .filter(|t| preset_tags.contains(*t))
                .cloned()
                .collect();
            println!(
                "  • {} (matches: {})",
                server.name,
                matching_tags.join(", ")
            );
        }
    }

    Ok(())
}

/// Remove a preset
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

    let initial_len = config.presets.len();
    config.presets.retain(|p| p.name != name);

    if config.presets.len() == initial_len {
        return Err(McpError::ConfigError(format!(
            "Preset '{}' not found",
            name
        )));
    }

    save_config(&path, &config).await?;

    println!("✓ Removed preset '{}'", name);
    Ok(())
}

async fn save_config(path: &PathBuf, config: &Config) -> McpResult<()> {
    let content = toml::to_string_pretty(config)
        .map_err(|e| McpError::ConfigError(format!("Failed to serialize config: {}", e)))?;
    tokio::fs::write(path, content)
        .await
        .map_err(|e| McpError::ConfigError(format!("Failed to write config: {}", e)))?;
    Ok(())
}
