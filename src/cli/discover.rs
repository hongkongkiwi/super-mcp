//! MCP server discovery from AI editors and tools
//!
//! Supports importing MCP configurations from:
//! - Cursor (cursor.com)
//! - Claude Desktop/Code (anthropic.com)
//! - Cline (cline.bot)
//! - Kilo Code (kilocode.com)
//! - Roo Code (github.com/RooVetGit/Roo-Code)
//! - Codex (OpenAI)
//! - Kimi CLI (kimi-cli)
//! - Qwen (Alibaba)
//! - Gemini (Google)
//! - GitHub Copilot
//! - Windsurf (codeium.com/windsurf)
//! - OpenCode
//! - Continue.dev
//!

use crate::cli::{ensure_config_dir, expand_path};
use crate::config::{Config, McpServerConfig, SandboxConfig};
use crate::utils::errors::{McpError, McpResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Information about a discovered MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredMcp {
    /// The source tool (cursor, claude, cline, etc.)
    pub source: String,
    /// Server name
    pub name: String,
    /// Command to run
    pub command: String,
    /// Arguments
    pub args: Vec<String>,
    /// Environment variables
    pub env: HashMap<String, String>,
    /// Description
    pub description: Option<String>,
    /// Source config file path
    pub source_path: PathBuf,
    /// Whether auto-approve is enabled (if available)
    pub auto_approve: Option<bool>,
}

impl DiscoveredMcp {
    /// Convert to McpServerConfig
    pub fn to_config(&self) -> McpServerConfig {
        McpServerConfig {
            name: self.name.clone(),
            command: self.command.clone(),
            args: self.args.clone(),
            env: self.env.clone(),
            tags: vec![self.source.clone(), "imported".to_string()],
            description: self.description.clone().or_else(|| {
                Some(format!("Imported from {}", self.source))
            }),
            sandbox: SandboxConfig::default(),
        }
    }
}

/// Cursor MCP configuration
#[derive(Debug, Deserialize)]
struct CursorMcpConfig {
    #[serde(default)]
    mcp_servers: HashMap<String, CursorMcpServer>,
}

#[derive(Debug, Deserialize)]
struct CursorMcpServer {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    #[serde(rename = "autoApprove")]
    auto_approve: Option<bool>,
}

/// Claude Desktop MCP configuration
#[derive(Debug, Deserialize)]
struct ClaudeDesktopConfig {
    #[serde(rename = "mcpServers", default)]
    mcp_servers: HashMap<String, ClaudeMcpServer>,
}

#[derive(Debug, Deserialize)]
struct ClaudeMcpServer {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, serde_json::Value>,
}

/// VS Code/Continue/Cline/Kilo/Roo format
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct VscodeMcpConfig {
    #[serde(rename = "mcpServers", default)]
    mcp_servers: Vec<VscodeMcpServer>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct VscodeMcpServer {
    name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    #[serde(rename = "autoApprove")]
    auto_approve: Option<bool>,
}

/// Discover MCP servers from Cursor
pub async fn discover_cursor() -> McpResult<Vec<DiscoveredMcp>> {
    let paths = vec![
        dirs::config_dir().map(|d| d.join("Cursor/mcp.json")),
        dirs::home_dir().map(|d| d.join(".cursor/mcp.json")),
    ];

    let mut results = Vec::new();

    for path in paths.into_iter().flatten() {
        if !path.exists() {
            continue;
        }

        let content = tokio::fs::read_to_string(&path).await?;
        let config: CursorMcpConfig = serde_json::from_str(&content)
            .map_err(|e| McpError::ConfigError(format!("Failed to parse Cursor config: {}", e)))?;

        for (name, server) in config.mcp_servers {
            results.push(DiscoveredMcp {
                source: "cursor".to_string(),
                name: sanitize_name(&name),
                command: server.command,
                args: server.args,
                env: server.env,
                description: None,
                source_path: path.clone(),
                auto_approve: server.auto_approve,
            });
        }
    }

    Ok(results)
}

/// Discover MCP servers from Claude Desktop
pub async fn discover_claude() -> McpResult<Vec<DiscoveredMcp>> {
    let paths = vec![
        // macOS
        dirs::home_dir().map(|d| d.join("Library/Application Support/Claude/claude_desktop_config.json")),
        // Windows
        dirs::config_dir().map(|d| d.join("Claude/claude_desktop_config.json")),
        // Linux
        dirs::config_dir().map(|d| d.join("Claude/config.json")),
    ];

    let mut results = Vec::new();

    for path in paths.into_iter().flatten() {
        if !path.exists() {
            continue;
        }

        let content = tokio::fs::read_to_string(&path).await?;
        let config: ClaudeDesktopConfig = serde_json::from_str(&content)
            .map_err(|e| McpError::ConfigError(format!("Failed to parse Claude config: {}", e)))?;

        for (name, server) in config.mcp_servers {
            // Claude uses a different env format - convert it
            let env = server
                .env
                .into_iter()
                .filter_map(|(k, v)| {
                    // Handle both string and object env values
                    match v {
                        serde_json::Value::String(s) => Some((k, s)),
                        _ => v.as_str().map(|s| (k, s.to_string())),
                    }
                })
                .collect();

            results.push(DiscoveredMcp {
                source: "claude".to_string(),
                name: sanitize_name(&name),
                command: server.command,
                args: server.args,
                env,
                description: None,
                source_path: path.clone(),
                auto_approve: None,
            });
        }
    }

    Ok(results)
}

/// Discover MCP servers from VS Code extensions (Cline, Kilo, Roo, Continue)
pub async fn discover_vscode_extensions() -> McpResult<Vec<DiscoveredMcp>> {
    let mut results = Vec::new();

    // Check various VS Code config locations
    let vscode_paths: Vec<(&str, Option<PathBuf>)> = vec![
        ("vscode", dirs::config_dir().map(|d| d.join("Code/User/settings.json"))),
        ("vscode-insiders", dirs::config_dir().map(|d| d.join("Code - Insiders/User/settings.json"))),
        ("cursor", dirs::config_dir().map(|d| d.join("Cursor/User/settings.json"))),
    ];

    for (editor, path_opt) in vscode_paths {
        let path = match path_opt {
            Some(p) => p,
            None => continue,
        };
        if !path.exists() {
            continue;
        }

        let content = tokio::fs::read_to_string(&path).await?;
        let settings: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| McpError::ConfigError(format!("Failed to parse {} settings: {}", editor, e)))?;

        // Check for various extension MCP configs
        let extensions = vec![
            ("cline", "cline.mcpServers"),
            ("kilo", "kilo.mcpServers"),
            ("roo", "roo.mcpServers"),
            ("continue", "continue.mcpServers"),
            ("windsurf", "windsurf.mcpServers"),
        ];

        for (ext_name, key) in extensions {
            if let Some(servers) = settings.get(key).and_then(|v| v.as_array()) {
                for server in servers {
                    if let (Some(name), Some(command)) = (
                        server.get("name").and_then(|n| n.as_str()),
                        server.get("command").and_then(|c| c.as_str()),
                    ) {
                        let args: Vec<String> = server
                            .get("args")
                            .and_then(|a| a.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default();

                        let env: HashMap<String, String> = server
                            .get("env")
                            .and_then(|e| e.as_object())
                            .map(|obj| {
                                obj.iter()
                                    .filter_map(|(k, v)| {
                                        v.as_str().map(|s| (k.clone(), s.to_string()))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();

                        results.push(DiscoveredMcp {
                            source: format!("{}-{}", editor, ext_name),
                            name: sanitize_name(name),
                            command: command.to_string(),
                            args,
                            env,
                            description: None,
                            source_path: path.clone(),
                            auto_approve: server
                                .get("autoApprove")
                                .and_then(|a| a.as_bool()),
                        });
                    }
                }
            }
        }
    }

    Ok(results)
}

/// Discover MCP servers from Codex
pub async fn discover_codex() -> McpResult<Vec<DiscoveredMcp>> {
    let paths = vec![
        dirs::config_dir().map(|d| d.join("codex/config.json")),
        dirs::home_dir().map(|d| d.join(".codex/config.json")),
    ];

    let mut results = Vec::new();

    for path in paths.into_iter().flatten() {
        if !path.exists() {
            continue;
        }

        let content = tokio::fs::read_to_string(&path).await?;
        // Codex uses a similar format to Claude
        if let Ok(config) = serde_json::from_str::<ClaudeDesktopConfig>(&content) {
            for (name, server) in config.mcp_servers {
                let env = server
                    .env
                    .into_iter()
                    .filter_map(|(k, v)| match v {
                        serde_json::Value::String(s) => Some((k, s)),
                        _ => v.as_str().map(|s| (k, s.to_string())),
                    })
                    .collect();

                results.push(DiscoveredMcp {
                    source: "codex".to_string(),
                    name: sanitize_name(&name),
                    command: server.command,
                    args: server.args,
                    env,
                    description: None,
                    source_path: path.clone(),
                    auto_approve: None,
                });
            }
        }
    }

    Ok(results)
}

/// Discover MCP servers from Kimi CLI
pub async fn discover_kimi_cli() -> McpResult<Vec<DiscoveredMcp>> {
    let paths = vec![
        dirs::config_dir().map(|d| d.join("kimi-cli/config.json")),
        dirs::home_dir().map(|d| d.join(".kimi/config.json")),
        dirs::home_dir().map(|d| d.join(".kimi-cli/config.json")),
    ];

    let mut results = Vec::new();

    for path in paths.into_iter().flatten() {
        if !path.exists() {
            continue;
        }

        let content = tokio::fs::read_to_string(&path).await?;
        
        // Kimi CLI may use different formats - try multiple
        // Format 1: Similar to Claude
        if let Ok(config) = serde_json::from_str::<ClaudeDesktopConfig>(&content) {
            for (name, server) in config.mcp_servers {
                let env = server
                    .env
                    .into_iter()
                    .filter_map(|(k, v)| match v {
                        serde_json::Value::String(s) => Some((k, s)),
                        _ => v.as_str().map(|s| (k, s.to_string())),
                    })
                    .collect();

                results.push(DiscoveredMcp {
                    source: "kimi-cli".to_string(),
                    name: sanitize_name(&name),
                    command: server.command,
                    args: server.args,
                    env,
                    description: None,
                    source_path: path.clone(),
                    auto_approve: None,
                });
            }
        }
    }

    Ok(results)
}

/// Discover MCP servers from Windsurf
pub async fn discover_windsurf() -> McpResult<Vec<DiscoveredMcp>> {
    let paths = vec![
        dirs::config_dir().map(|d| d.join("Windsurf/mcp.json")),
        dirs::config_dir().map(|d| d.join("windsurf/mcp.json")),
        dirs::home_dir().map(|d| d.join(".windsurf/mcp.json")),
    ];

    let mut results = Vec::new();

    for path in paths.into_iter().flatten() {
        if !path.exists() {
            continue;
        }

        let content = tokio::fs::read_to_string(&path).await?;
        
        // Try Cursor format first
        if let Ok(config) = serde_json::from_str::<CursorMcpConfig>(&content) {
            for (name, server) in config.mcp_servers {
                results.push(DiscoveredMcp {
                    source: "windsurf".to_string(),
                    name: sanitize_name(&name),
                    command: server.command,
                    args: server.args,
                    env: server.env,
                    description: None,
                    source_path: path.clone(),
                    auto_approve: server.auto_approve,
                });
            }
        }
    }

    Ok(results)
}

/// Discover MCP servers from Gemini
pub async fn discover_gemini() -> McpResult<Vec<DiscoveredMcp>> {
    let paths = vec![
        dirs::config_dir().map(|d| d.join("gemini/config.json")),
        dirs::home_dir().map(|d| d.join(".gemini/config.json")),
        dirs::home_dir().map(|d| d.join(".gemini-cli/config.json")),
    ];

    let mut results = Vec::new();

    for path in paths.into_iter().flatten() {
        if !path.exists() {
            continue;
        }

        let content = tokio::fs::read_to_string(&path).await?;
        
        if let Ok(config) = serde_json::from_str::<ClaudeDesktopConfig>(&content) {
            for (name, server) in config.mcp_servers {
                let env = server
                    .env
                    .into_iter()
                    .filter_map(|(k, v)| match v {
                        serde_json::Value::String(s) => Some((k, s)),
                        _ => v.as_str().map(|s| (k, s.to_string())),
                    })
                    .collect();

                results.push(DiscoveredMcp {
                    source: "gemini".to_string(),
                    name: sanitize_name(&name),
                    command: server.command,
                    args: server.args,
                    env,
                    description: None,
                    source_path: path.clone(),
                    auto_approve: None,
                });
            }
        }
    }

    Ok(results)
}

/// Discover MCP servers from Qwen
pub async fn discover_qwen() -> McpResult<Vec<DiscoveredMcp>> {
    let paths = vec![
        dirs::config_dir().map(|d| d.join("qwen/config.json")),
        dirs::home_dir().map(|d| d.join(".qwen/config.json")),
        dirs::home_dir().map(|d| d.join(".qwen-cli/config.json")),
    ];

    let mut results = Vec::new();

    for path in paths.into_iter().flatten() {
        if !path.exists() {
            continue;
        }

        let content = tokio::fs::read_to_string(&path).await?;
        
        if let Ok(config) = serde_json::from_str::<ClaudeDesktopConfig>(&content) {
            for (name, server) in config.mcp_servers {
                let env = server
                    .env
                    .into_iter()
                    .filter_map(|(k, v)| match v {
                        serde_json::Value::String(s) => Some((k, s)),
                        _ => v.as_str().map(|s| (k, s.to_string())),
                    })
                    .collect();

                results.push(DiscoveredMcp {
                    source: "qwen".to_string(),
                    name: sanitize_name(&name),
                    command: server.command,
                    args: server.args,
                    env,
                    description: None,
                    source_path: path.clone(),
                    auto_approve: None,
                });
            }
        }
    }

    Ok(results)
}

/// Discover MCP servers from GitHub Copilot (if/when they support MCP)
pub async fn discover_github_copilot() -> McpResult<Vec<DiscoveredMcp>> {
    // GitHub Copilot doesn't have MCP config yet, but we can check for future support
    let paths = vec![
        dirs::config_dir().map(|d| d.join("github-copilot/mcp.json")),
        dirs::home_dir().map(|d| d.join(".github/copilot/mcp.json")),
    ];

    let mut results = Vec::new();

    for path in paths.into_iter().flatten() {
        if !path.exists() {
            continue;
        }

        let content = tokio::fs::read_to_string(&path).await?;
        
        if let Ok(config) = serde_json::from_str::<CursorMcpConfig>(&content) {
            for (name, server) in config.mcp_servers {
                results.push(DiscoveredMcp {
                    source: "github-copilot".to_string(),
                    name: sanitize_name(&name),
                    command: server.command,
                    args: server.args,
                    env: server.env,
                    description: None,
                    source_path: path.clone(),
                    auto_approve: server.auto_approve,
                });
            }
        }
    }

    Ok(results)
}

/// Discover MCP servers from OpenCode
pub async fn discover_opencode() -> McpResult<Vec<DiscoveredMcp>> {
    let paths = vec![
        dirs::config_dir().map(|d| d.join("opencode/config.json")),
        dirs::home_dir().map(|d| d.join(".opencode/config.json")),
    ];

    let mut results = Vec::new();

    for path in paths.into_iter().flatten() {
        if !path.exists() {
            continue;
        }

        let content = tokio::fs::read_to_string(&path).await?;
        
        if let Ok(config) = serde_json::from_str::<ClaudeDesktopConfig>(&content) {
            for (name, server) in config.mcp_servers {
                let env = server
                    .env
                    .into_iter()
                    .filter_map(|(k, v)| match v {
                        serde_json::Value::String(s) => Some((k, s)),
                        _ => v.as_str().map(|s| (k, s.to_string())),
                    })
                    .collect();

                results.push(DiscoveredMcp {
                    source: "opencode".to_string(),
                    name: sanitize_name(&name),
                    command: server.command,
                    args: server.args,
                    env,
                    description: None,
                    source_path: path.clone(),
                    auto_approve: None,
                });
            }
        }
    }

    Ok(results)
}

/// Discover all MCP servers from all sources
pub async fn discover_all() -> McpResult<Vec<DiscoveredMcp>> {
    let mut all = Vec::new();

    // Run all discovery methods concurrently
    let discoveries = tokio::join!(
        discover_cursor(),
        discover_claude(),
        discover_vscode_extensions(),
        discover_codex(),
        discover_kimi_cli(),
        discover_windsurf(),
        discover_opencode(),
        discover_gemini(),
        discover_qwen(),
        discover_github_copilot(),
    );

    if let Ok(servers) = discoveries.0 {
        all.extend(servers);
    }
    if let Ok(servers) = discoveries.1 {
        all.extend(servers);
    }
    if let Ok(servers) = discoveries.2 {
        all.extend(servers);
    }
    if let Ok(servers) = discoveries.3 {
        all.extend(servers);
    }
    if let Ok(servers) = discoveries.4 {
        all.extend(servers);
    }
    if let Ok(servers) = discoveries.5 {
        all.extend(servers);
    }
    if let Ok(servers) = discoveries.6 {
        all.extend(servers);
    }
    if let Ok(servers) = discoveries.7 {
        all.extend(servers);
    }
    if let Ok(servers) = discoveries.8 {
        all.extend(servers);
    }
    if let Ok(servers) = discoveries.9 {
        all.extend(servers);
    }

    // Deduplicate by name (prefer first found)
    let mut seen = std::collections::HashSet::new();
    let deduplicated: Vec<_> = all
        .into_iter()
        .filter(|mcp| {
            let key = format!("{}:{}", mcp.source, mcp.name);
            seen.insert(key)
        })
        .collect();

    Ok(deduplicated)
}

/// Import discovered MCPs into config
pub async fn import_discovered(
    config_path: &str,
    mcps: Vec<DiscoveredMcp>,
    dry_run: bool,
) -> McpResult<Vec<String>> {
    if dry_run {
        return Ok(mcps.into_iter().map(|m| m.name).collect());
    }

    let path = PathBuf::from(expand_path(config_path));
    ensure_config_dir(&path).await?;

    // Load existing config
    let mut config = if path.exists() {
        let content = tokio::fs::read_to_string(&path).await?;
        toml::from_str::<Config>(&content)
            .map_err(|e| McpError::ConfigError(format!("Failed to parse config: {}", e)))?
    } else {
        Config::default()
    };

    let mut imported = Vec::new();

    for mcp in mcps {
        // Check for name conflicts
        if config.servers.iter().any(|s| s.name == mcp.name) {
            // Rename with source prefix
            let new_name = format!("{}-{}", mcp.source, mcp.name);
            if config.servers.iter().any(|s| s.name == new_name) {
                tracing::warn!("Skipping {} (already exists)", mcp.name);
                continue;
            }
            let mut config_mcp = mcp.to_config();
            config_mcp.name = new_name.clone();
            config.servers.push(config_mcp);
            imported.push(new_name);
        } else {
            config.servers.push(mcp.to_config());
            imported.push(mcp.name.clone());
        }
    }

    // Save config
    let content = toml::to_string_pretty(&config)
        .map_err(|e| McpError::ConfigError(format!("Failed to serialize config: {}", e)))?;
    tokio::fs::write(&path, content).await?;

    Ok(imported)
}

/// Sanitize server name for use in config
fn sanitize_name(name: &str) -> String {
    // Replace spaces and special chars with underscores
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("My Server"), "my_server");
        assert_eq!(sanitize_name("server@123"), "server_123");
        assert_eq!(sanitize_name("Valid-Name_123"), "valid-name_123");
    }
}
