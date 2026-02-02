//! Standard MCP Configuration Formats
//!
//! Provides support for various MCP config formats:
//! - mcp.json (Claude Code format)
//! - presets.json (1MCP preset format)
//! - Smithery AI config format
//! - Generic MCP config format
//!
//! These formats can be used interchangeably with Super MCP config.

use crate::config::{Config as SuperMcpConfig, McpServerConfig, SandboxConfig, AuthConfig, ServerConfig, FeaturesConfig, PresetConfig};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Presets.json format (1MCP preset configuration)
/// Used for grouping servers by preset/tags
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PresetsConfig {
    /// Preset definitions
    #[serde(default)]
    pub presets: Vec<PresetDefinition>,
    /// Optional server configurations
    #[serde(default)]
    pub servers: Vec<PresetServer>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PresetDefinition {
    pub name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetServer {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub description: Option<String>,
}

/// Claude Code mcp.json format
/// Used by Claude Code and Claude.app for MCP server configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpJsonConfig {
    /// MCP servers
    #[serde(default)]
    pub mcpServers: HashMap<String, McpServerEntry>,
}

/// Server entry in mcp.json format
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpServerEntry {
    /// Command to run
    pub command: String,
    /// Arguments for the command
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Whether the server is disabled
    #[serde(default)]
    pub disabled: bool,
}

/// Backwards compatibility type alias
pub type ClaudeCodeMcpConfig = McpJsonConfig;

/// Smithery AI config format
/// Used by Smithery for MCP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmitheryConfig {
    /// Server configuration
    pub server: Option<SmitheryServer>,
    /// MCP servers
    #[serde(default)]
    pub mcp: HashMap<String, SmitheryMcpServer>,
    /// Authentication
    pub auth: Option<SmitheryAuth>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmitheryServer {
    pub name: String,
    pub version: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmitheryMcpServer {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub scope: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmitheryAuth {
    #[serde(rename = "type")]
    pub auth_type: String,
    pub config: Option<HashMap<String, String>>,
}

/// Generic MCP config format (subset of Super MCP)
/// Used for simple MCP server configurations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericMcpConfig {
    /// Server configuration
    #[serde(default)]
    pub server: GenericServerConfig,
    /// MCP servers
    #[serde(default)]
    pub servers: Vec<GenericServer>,
    /// Authentication
    #[serde(default)]
    pub auth: GenericAuth,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GenericServerConfig {
    pub host: String,
    pub port: u16,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericServer {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub description: Option<String>,
    pub sandbox: Option<GenericSandbox>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GenericSandbox {
    pub enabled: bool,
    pub network: bool,
    pub filesystem: Option<String>,
    pub max_memory_mb: Option<u64>,
    pub max_cpu_percent: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GenericAuth {
    #[serde(rename = "type")]
    pub auth_type: String,
    pub token: Option<String>,
    pub jwt_secret: Option<String>,
}

/// Standard MCP config adapter
pub struct StandardMcpConfigAdapter;

impl StandardMcpConfigAdapter {
    /// Detect the config format from content
    pub fn detect_format(content: &str) -> StandardConfigFormat {
        // Try to parse as mcp.json format first
        if let Ok(config) = serde_json::from_str::<McpJsonConfig>(content) {
            if !config.mcpServers.is_empty() {
                return StandardConfigFormat::McpJson;
            }
        }

        // Try presets.json format
        if let Ok(config) = serde_yaml::from_str::<PresetsConfig>(content) {
            if !config.presets.is_empty() || !config.servers.is_empty() {
                return StandardConfigFormat::PresetsJson;
            }
        }
        if let Ok(config) = serde_json::from_str::<PresetsConfig>(content) {
            if !config.presets.is_empty() || !config.servers.is_empty() {
                return StandardConfigFormat::PresetsJson;
            }
        }

        // Try Smithery format
        if let Ok(config) = serde_json::from_str::<SmitheryConfig>(content) {
            if config.server.is_some() || !config.mcp.is_empty() {
                return StandardConfigFormat::Smithery;
            }
        }

        // Check for generic MCP format markers
        if content.contains("\"mcpServers\"") {
            return StandardConfigFormat::McpJson;
        }

        // Check for presets format
        if content.contains("\"presets\"") && content.contains("\"servers\"") {
            return StandardConfigFormat::PresetsJson;
        }

        // Check for "servers" array (generic format)
        if content.contains("\"servers\"") && content.contains("\"command\"") {
            return StandardConfigFormat::Generic;
        }

        // Default to generic format
        StandardConfigFormat::Generic
    }

    /// Convert mcp.json config to Super MCP config
    pub fn convert_mcp_json(config: &McpJsonConfig) -> SuperMcpConfig {
        debug!("Converting mcp.json configuration");

        let mut super_mcp = SuperMcpConfig::default();

        for (name, server_config) in &config.mcpServers {
            if server_config.disabled {
                debug!("Skipping disabled server: {}", name);
                continue;
            }

            let server = McpServerConfig {
                name: name.clone(),
                command: server_config.command.clone(),
                args: server_config.args.clone(),
                env: server_config.env.clone(),
                tags: vec![name.clone()],
                description: Some(format!("MCP server from mcp.json")),
                sandbox: SandboxConfig::default(),
            };

            super_mcp.servers.push(server);
        }

        info!("Converted {} servers from mcp.json format", super_mcp.servers.len());
        super_mcp
    }

    /// Convert presets.json config to Super MCP config
    pub fn convert_presets_json(config: &PresetsConfig) -> SuperMcpConfig {
        debug!("Converting presets.json configuration");

        let mut super_mcp = SuperMcpConfig::default();

        // Convert presets
        for preset in &config.presets {
            if preset.enabled {
                super_mcp.presets.push(PresetConfig {
                    name: preset.name.clone(),
                    tags: preset.tags.clone(),
                    description: preset.description.clone(),
                });
            }
        }

        // Convert servers with preset tags
        for server in &config.servers {
            let server_config = McpServerConfig {
                name: server.name.clone(),
                command: server.command.clone(),
                args: server.args.clone(),
                env: server.env.clone(),
                tags: server.tags.clone(),
                description: server.description.clone(),
                sandbox: SandboxConfig::default(),
            };

            super_mcp.servers.push(server_config);
        }

        info!("Converted {} presets and {} servers from presets.json",
              super_mcp.presets.len(), super_mcp.servers.len());
        super_mcp
    }

    /// Backwards compatibility: Convert Claude Code config to Super MCP config
    pub fn convert_claude_code(config: &McpJsonConfig) -> SuperMcpConfig {
        Self::convert_mcp_json(config)
    }

    /// Convert Smithery config to Super MCP config
    pub fn convert_smithery(config: &SmitheryConfig) -> SuperMcpConfig {
        debug!("Converting Smithery configuration");

        let mut super_mcp = SuperMcpConfig::default();

        // Set server name from Smithery config
        if let Some(server) = &config.server {
            super_mcp.server.host = "0.0.0.0".to_string();
            super_mcp.server.port = 3000;
        }

        // Convert MCP servers
        for (name, mcp_server) in &config.mcp {
            if mcp_server.enabled.unwrap_or(true) {
                let server = McpServerConfig {
                    name: name.clone(),
                    command: mcp_server.command.clone(),
                    args: mcp_server.args.clone(),
                    env: mcp_server.env.clone(),
                    tags: mcp_server.scope.clone().map(|s| vec![s]).unwrap_or_default(),
                    description: Some(format!("MCP server from Smithery config")),
                    sandbox: SandboxConfig::default(),
                };

                super_mcp.servers.push(server);
            }
        }

        info!("Converted {} servers from Smithery format", super_mcp.servers.len());
        super_mcp
    }

    /// Convert generic MCP config to Super MCP config
    pub fn convert_generic(config: &GenericMcpConfig) -> SuperMcpConfig {
        debug!("Converting generic MCP configuration");

        let mut super_mcp = SuperMcpConfig::default();

        // Convert server config
        super_mcp.server.host = config.server.host.clone();
        super_mcp.server.port = config.server.port;
        super_mcp.server.cert_path = config.server.cert_path.clone();
        super_mcp.server.key_path = config.server.key_path.clone();

        // Convert servers
        for server in &config.servers {
            let sandbox = match &server.sandbox {
                Some(s) => SandboxConfig {
                    enabled: s.enabled,
                    sandbox_type: crate::config::SandboxType::Default,
                    network: s.network,
                    filesystem: match &s.filesystem {
                        Some(fs) => crate::config::FilesystemAccess::Simple(fs.clone()),
                        None => crate::config::FilesystemAccess::Simple("readonly".to_string()),
                    },
                    max_memory_mb: s.max_memory_mb.unwrap_or(512),
                    max_cpu_percent: s.max_cpu_percent.unwrap_or(50),
                    env_inherit: true,
                },
                None => SandboxConfig::default(),
            };

            let server_config = McpServerConfig {
                name: server.name.clone(),
                command: server.command.clone(),
                args: server.args.clone(),
                env: server.env.clone(),
                tags: server.tags.clone(),
                description: server.description.clone(),
                sandbox,
            };

            super_mcp.servers.push(server_config);
        }

        // Convert auth
        super_mcp.auth = Self::convert_generic_auth(&config.auth);

        info!("Converted {} servers from generic MCP format", super_mcp.servers.len());
        super_mcp
    }

    fn convert_generic_auth(auth: &GenericAuth) -> AuthConfig {
        use crate::config::AuthType;

        let auth_type = match auth.auth_type.as_str() {
            "jwt" | "JWT" => AuthType::Jwt,
            "oauth" | "OAuth" | "oidc" | "OIDC" => AuthType::OAuth,
            "static" | "Static" => AuthType::Static,
            _ => AuthType::None,
        };

        AuthConfig {
            auth_type,
            token: auth.token.clone(),
            issuer: None,
            client_id: None,
            client_secret: None,
            jwt_secret: auth.jwt_secret.clone(),
            auth_url: None,
            token_url: None,
            introspection_url: None,
            userinfo_url: None,
            jwks_url: None,
            expected_audiences: Vec::new(),
            allowed_algs: Vec::new(),
            jwks_cache_ttl_seconds: 300,
            allow_unverified_jwt: false,
            required_scopes: Vec::new(),
        }
    }

    /// Load and convert any standard MCP config
    pub async fn load_and_convert(content: &str, format: StandardConfigFormat) -> Result<SuperMcpConfig, String> {
        match format {
            StandardConfigFormat::McpJson => {
                let config: McpJsonConfig = serde_json::from_str(content)
                    .map_err(|e| format!("Failed to parse mcp.json: {}", e))?;
                Ok(Self::convert_mcp_json(&config))
            }
            StandardConfigFormat::PresetsJson => {
                // Try JSON first, then YAML
                if content.trim_start().starts_with('{') {
                    let config: PresetsConfig = serde_json::from_str(content)
                        .map_err(|e| format!("Failed to parse presets.json: {}", e))?;
                    Ok(Self::convert_presets_json(&config))
                } else {
                    let config: PresetsConfig = serde_yaml::from_str(content)
                        .map_err(|e| format!("Failed to parse presets.json: {}", e))?;
                    Ok(Self::convert_presets_json(&config))
                }
            }
            StandardConfigFormat::Smithery => {
                let config: SmitheryConfig = serde_json::from_str(content)
                    .map_err(|e| format!("Failed to parse Smithery config: {}", e))?;
                Ok(Self::convert_smithery(&config))
            }
            StandardConfigFormat::Generic => {
                // Try to detect if it's JSON or YAML
                if content.trim_start().starts_with('{') {
                    let config: GenericMcpConfig = serde_json::from_str(content)
                        .map_err(|e| format!("Failed to parse generic MCP config: {}", e))?;
                    Ok(Self::convert_generic(&config))
                } else {
                    let config: GenericMcpConfig = serde_yaml::from_str(content)
                        .map_err(|e| format!("Failed to parse generic MCP config: {}", e))?;
                    Ok(Self::convert_generic(&config))
                }
            }
        }
    }

    /// Extract servers from various formats without full conversion
    pub fn extract_servers(content: &str) -> Vec<McpServerConfig> {
        let format = Self::detect_format(content);

        match format {
            StandardConfigFormat::McpJson => {
                if let Ok(config) = serde_json::from_str::<McpJsonConfig>(content) {
                    config.mcpServers.into_iter()
                        .filter(|(_, s)| !s.disabled)
                        .map(|(name, sc)| McpServerConfig {
                            name,
                            command: sc.command,
                            args: sc.args,
                            env: sc.env,
                            tags: vec![],
                            description: Some("MCP server".to_string()),
                            sandbox: SandboxConfig::default(),
                        })
                        .collect()
                } else {
                    vec![]
                }
            }
            StandardConfigFormat::PresetsJson => {
                if content.trim_start().starts_with('{') {
                    if let Ok(config) = serde_json::from_str::<PresetsConfig>(content) {
                        config.servers.into_iter()
                            .map(|s| McpServerConfig {
                                name: s.name,
                                command: s.command,
                                args: s.args,
                                env: s.env,
                                tags: s.tags,
                                description: s.description,
                                sandbox: SandboxConfig::default(),
                            })
                            .collect()
                    } else {
                        vec![]
                    }
                } else if let Ok(config) = serde_yaml::from_str::<PresetsConfig>(content) {
                    config.servers.into_iter()
                        .map(|s| McpServerConfig {
                            name: s.name,
                            command: s.command,
                            args: s.args,
                            env: s.env,
                            tags: s.tags,
                            description: s.description,
                            sandbox: SandboxConfig::default(),
                        })
                        .collect()
                } else {
                    vec![]
                }
            }
            StandardConfigFormat::Smithery => {
                if let Ok(config) = serde_json::from_str::<SmitheryConfig>(content) {
                    config.mcp.into_iter()
                        .filter(|(_, s)| s.enabled.unwrap_or(true))
                        .map(|(name, sc)| McpServerConfig {
                            name,
                            command: sc.command,
                            args: sc.args,
                            env: sc.env,
                            tags: vec![],
                            description: Some("MCP server".to_string()),
                            sandbox: SandboxConfig::default(),
                        })
                        .collect()
                } else {
                    vec![]
                }
            }
            StandardConfigFormat::Generic => {
                if let Ok(config) = serde_json::from_str::<GenericMcpConfig>(content) {
                    config.servers.into_iter()
                        .map(|s| McpServerConfig {
                            name: s.name,
                            command: s.command,
                            args: s.args,
                            env: s.env,
                            tags: s.tags,
                            description: s.description,
                            sandbox: match s.sandbox {
                                Some(sb) => SandboxConfig {
                                    enabled: sb.enabled,
                                    sandbox_type: crate::config::SandboxType::Default,
                                    network: sb.network,
                                    filesystem: match sb.filesystem {
                                        Some(fs) => crate::config::FilesystemAccess::Simple(fs),
                                        None => crate::config::FilesystemAccess::Simple("readonly".to_string()),
                                    },
                                    max_memory_mb: sb.max_memory_mb.unwrap_or(512),
                                    max_cpu_percent: sb.max_cpu_percent.unwrap_or(50),
                                    env_inherit: true,
                                },
                                None => SandboxConfig::default(),
                            },
                        })
                        .collect()
                } else {
                    vec![]
                }
            }
        }
    }
}

/// Standard config format enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardConfigFormat {
    /// mcp.json format (Claude Code)
    McpJson,
    /// presets.json format (1MCP)
    PresetsJson,
    /// Smithery AI config format
    Smithery,
    /// Generic MCP config format
    Generic,
}

impl Default for StandardConfigFormat {
    fn default() -> Self {
        StandardConfigFormat::Generic
    }
}

/// Convert Super MCP config to standard formats
pub struct StandardMcpConfigWriter;

impl StandardMcpConfigWriter {
    /// Write Super MCP config as mcp.json
    pub fn to_mcp_json(config: &SuperMcpConfig) -> String {
        let mut mcp_servers = HashMap::new();

        for server in &config.servers {
            let server_config = McpServerEntry {
                command: server.command.clone(),
                args: server.args.clone(),
                env: server.env.clone(),
                disabled: false,
            };
            mcp_servers.insert(server.name.clone(), server_config);
        }

        let config_output = McpJsonConfig { mcpServers: mcp_servers };
        serde_json::to_string_pretty(&config_output).unwrap_or_default()
    }

    /// Write Super MCP config as presets.json
    pub fn to_presets_json(config: &SuperMcpConfig) -> String {
        let servers: Vec<PresetServer> = config.servers.iter()
            .map(|s| PresetServer {
                name: s.name.clone(),
                command: s.command.clone(),
                args: s.args.clone(),
                env: s.env.clone(),
                tags: s.tags.clone(),
                description: s.description.clone(),
            })
            .collect();

        let presets: Vec<PresetDefinition> = config.presets.iter()
            .map(|p| PresetDefinition {
                name: p.name.clone(),
                tags: p.tags.clone(),
                description: p.description.clone(),
                enabled: true,
            })
            .collect();

        let config_output = PresetsConfig {
            presets,
            servers,
        };

        serde_yaml::to_string(&config_output).unwrap_or_default()
    }

    /// Write Super MCP config as Claude Code mcp.json (backwards compatibility)
    pub fn to_claude_code(config: &SuperMcpConfig) -> String {
        Self::to_mcp_json(config)
    }

    /// Write Super MCP config as generic JSON
    pub fn to_generic_json(config: &SuperMcpConfig) -> String {
        let mut servers = Vec::new();

        for server in &config.servers {
            let server_config = GenericServer {
                name: server.name.clone(),
                command: server.command.clone(),
                args: server.args.clone(),
                env: server.env.clone(),
                tags: server.tags.clone(),
                description: server.description.clone(),
                sandbox: Some(GenericSandbox {
                    enabled: server.sandbox.enabled,
                    network: server.sandbox.network,
                    filesystem: match &server.sandbox.filesystem {
                        crate::config::FilesystemAccess::Simple(s) => Some(s.clone()),
                        _ => None,
                    },
                    max_memory_mb: Some(server.sandbox.max_memory_mb),
                    max_cpu_percent: Some(server.sandbox.max_cpu_percent),
                }),
            };
            servers.push(server_config);
        }

        let config_output = GenericMcpConfig {
            server: GenericServerConfig {
                host: config.server.host.clone(),
                port: config.server.port,
                cert_path: config.server.cert_path.clone(),
                key_path: config.server.key_path.clone(),
            },
            servers,
            auth: GenericAuth {
                auth_type: "none".to_string(),
                token: None,
                jwt_secret: None,
            },
        };

        serde_json::to_string_pretty(&config_output).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_mcp_json_format() {
        let json = r#"{
  "mcpServers": {
    "filesystem": {
      "command": "uvx",
      "args": ["@modelcontextprotocol/server-filesystem", "/tmp"]
    }
  }
}"#;

        let format = StandardMcpConfigAdapter::detect_format(json);
        assert_eq!(format, StandardConfigFormat::McpJson);
    }

    #[test]
    fn test_convert_mcp_json() {
        let json = r#"{
  "mcpServers": {
    "filesystem": {
      "command": "uvx",
      "args": ["@modelcontextprotocol/server-filesystem", "/tmp"],
      "env": {}
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_TOKEN": "test"
      }
    }
  }
}"#;

        let config: McpJsonConfig = serde_json::from_str(json).unwrap();
        let super_mcp = StandardMcpConfigAdapter::convert_mcp_json(&config);

        assert_eq!(super_mcp.servers.len(), 2);
        assert_eq!(super_mcp.servers[0].name, "filesystem");
        assert_eq!(super_mcp.servers[0].command, "uvx");
        assert_eq!(super_mcp.servers[1].name, "github");
    }

    #[test]
    fn test_convert_mcp_json_disabled_server() {
        let json = r#"{
  "mcpServers": {
    "enabled": {
      "command": "echo",
      "args": ["hello"]
    },
    "disabled": {
      "command": "echo",
      "args": ["world"],
      "disabled": true
    }
  }
}"#;

        let config: McpJsonConfig = serde_json::from_str(json).unwrap();
        let super_mcp = StandardMcpConfigAdapter::convert_mcp_json(&config);

        assert_eq!(super_mcp.servers.len(), 1);
        assert_eq!(super_mcp.servers[0].name, "enabled");
    }

    #[test]
    fn test_presets_json_detection() {
        let yaml = r#"
presets:
  - name: development
    tags: [dev, test]
    enabled: true

servers:
  - name: filesystem
    command: uvx
    args:
      - "@modelcontextprotocol/server-filesystem"
      - /tmp
    tags: [filesystem]
"#;

        let format = StandardMcpConfigAdapter::detect_format(yaml);
        assert_eq!(format, StandardConfigFormat::PresetsJson);
    }

    #[test]
    fn test_convert_presets_json() {
        let yaml = r#"
presets:
  - name: development
    tags: [dev]
    enabled: true

servers:
  - name: test-server
    command: echo
    args: ["hello"]
    tags: [test]
"#;

        let config: PresetsConfig = serde_yaml::from_str(yaml).unwrap();
        let super_mcp = StandardMcpConfigAdapter::convert_presets_json(&config);

        assert_eq!(super_mcp.presets.len(), 1);
        assert_eq!(super_mcp.presets[0].name, "development");
        assert_eq!(super_mcp.servers.len(), 1);
        assert_eq!(super_mcp.servers[0].name, "test-server");
    }

    #[test]
    fn test_write_mcp_json_format() {
        let mut super_mcp = SuperMcpConfig::default();
        super_mcp.servers.push(McpServerConfig {
            name: "test".to_string(),
            command: "uvx".to_string(),
            args: vec!["@test/server".to_string()],
            env: HashMap::new(),
            tags: vec![],
            description: None,
            sandbox: SandboxConfig::default(),
        });

        let output = StandardMcpConfigWriter::to_mcp_json(&super_mcp);
        let parsed: McpJsonConfig = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed.mcpServers.len(), 1);
        assert!(parsed.mcpServers.contains_key("test"));
    }

    #[test]
    fn test_write_presets_json_format() {
        let mut super_mcp = SuperMcpConfig::default();
        super_mcp.servers.push(McpServerConfig {
            name: "test".to_string(),
            command: "uvx".to_string(),
            args: vec!["@test/server".to_string()],
            env: HashMap::new(),
            tags: vec!["test".to_string()],
            description: None,
            sandbox: SandboxConfig::default(),
        });
        super_mcp.presets.push(PresetConfig {
            name: "development".to_string(),
            tags: vec!["test".to_string()],
            description: Some("Dev preset".to_string()),
        });

        let output = StandardMcpConfigWriter::to_presets_json(&super_mcp);
        let parsed: PresetsConfig = serde_yaml::from_str(&output).unwrap();

        assert_eq!(parsed.presets.len(), 1);
        assert_eq!(parsed.servers.len(), 1);
    }

    #[test]
    fn test_extract_servers() {
        let json = r#"{
  "mcpServers": {
    "server1": {
      "command": "echo",
      "args": ["hello"]
    }
  }
}"#;

        let servers = StandardMcpConfigAdapter::extract_servers(json);
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "server1");
        assert_eq!(servers[0].command, "echo");
    }

    #[test]
    fn test_detect_smithery_format() {
        let json = r#"{
  "server": {
    "name": "test-server",
    "version": "1.0.0",
    "description": "Test server"
  },
  "mcp": {
    "test": {
      "command": "uvx",
      "args": ["@test/server"],
      "enabled": true
    }
  }
}"#;

        let format = StandardMcpConfigAdapter::detect_format(json);
        assert_eq!(format, StandardConfigFormat::Smithery);
    }
}
