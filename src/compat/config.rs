//! 1MCP Configuration Adapter
//!
//! Converts 1MCP configuration format to Super MCP format.

use crate::config::{Config as SuperMcpConfig, McpServerConfig, SandboxConfig, AuthConfig, FeaturesConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{debug, info};

/// 1MCP configuration format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneMcpConfig {
    /// Server configuration
    pub server: OneMcpServerConfig,
    /// MCP servers
    pub servers: Vec<OneMcpServer>,
    /// Authentication
    pub auth: Option<OneMcpAuth>,
    /// Sandboxing
    pub sandboxing: Option<OneMcpSandboxing>,
    /// Features
    pub features: Option<OneMcpFeatures>,
    /// Logging
    pub logging: Option<OneMcpLogging>,
    /// Rate limiting
    pub rate_limiting: Option<OneMcpRateLimiting>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneMcpServerConfig {
    pub host: String,
    pub port: u16,
    #[serde(rename = "tls_enabled")]
    pub tls: Option<bool>,
    #[serde(rename = "tls_cert")]
    pub cert_path: Option<String>,
    #[serde(rename = "tls_key")]
    pub key_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneMcpServer {
    pub name: String,
    pub command: String,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub tags: Option<Vec<String>>,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub sandbox: Option<OneMcpSandbox>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneMcpSandbox {
    pub enabled: Option<bool>,
    pub network: Option<bool>,
    pub filesystem: Option<String>,
    pub max_memory: Option<u64>,
    pub max_cpu: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneMcpAuth {
    #[serde(rename = "type")]
    pub auth_type: String,
    pub jwt_secret: Option<String>,
    pub oauth_issuer: Option<String>,
    pub oauth_client_id: Option<String>,
    pub static_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneMcpSandboxing {
    pub enabled: bool,
    pub default_profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneMcpFeatures {
    pub hot_reload: Option<bool>,
    pub audit_logging: Option<bool>,
    pub scope_validation: Option<bool>,
    pub request_caching: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneMcpLogging {
    pub level: String,
    pub format: String,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneMcpRateLimiting {
    pub enabled: bool,
    pub requests_per_minute: u32,
    pub burst_size: u32,
}

/// Configuration adapter
pub struct OneMcpConfigAdapter;

impl OneMcpConfigAdapter {
    /// Convert 1MCP config to Super MCP config
    pub fn convert(config: &OneMcpConfig) -> SuperMcpConfig {
        debug!("Converting 1MCP configuration to Super MCP format");

        let mut super_mcp = SuperMcpConfig::default();

        // Convert server config
        super_mcp.server.host = config.server.host.clone();
        super_mcp.server.port = config.server.port;
        if config.server.tls.unwrap_or(false) {
            super_mcp.server.cert_path = config.server.cert_path.clone();
            super_mcp.server.key_path = config.server.key_path.clone();
        }

        // Convert servers
        super_mcp.servers = config.servers.iter()
            .filter(|s| s.enabled.unwrap_or(true))
            .map(|s| Self::convert_server(s))
            .collect();

        // Convert auth
        if let Some(auth) = &config.auth {
            super_mcp.auth = Self::convert_auth(auth);
        }

        // Convert features
        if let Some(features) = &config.features {
            super_mcp.features = Self::convert_features(features);
        }

        // Convert sandboxing defaults
        if let Some(sandboxing) = &config.sandboxing {
            for server in &mut super_mcp.servers {
                server.sandbox.enabled = sandboxing.enabled;
            }
        }

        info!(
            "Configuration converted: {} servers, auth={}, sandboxing={}",
            super_mcp.servers.len(),
            config.auth.is_some(),
            config.sandboxing.as_ref().map(|s| s.enabled).unwrap_or(false)
        );

        super_mcp
    }

    /// Convert a single server config
    fn convert_server(server: &OneMcpServer) -> McpServerConfig {
        let mut sandbox = SandboxConfig::default();

        if let Some(s) = &server.sandbox {
            sandbox.enabled = s.enabled.unwrap_or(true);
            sandbox.network = s.network.unwrap_or(false);
            if let Some(fs) = &s.filesystem {
                sandbox.filesystem = crate::config::FilesystemAccess::Simple(fs.clone());
            }
            if let Some(mem) = s.max_memory {
                sandbox.max_memory_mb = mem;
            }
            if let Some(cpu) = s.max_cpu {
                sandbox.max_cpu_percent = cpu;
            }
        }

        McpServerConfig {
            name: server.name.clone(),
            command: server.command.clone(),
            args: server.args.clone().unwrap_or_default(),
            env: server.env.clone().unwrap_or_default(),
            tags: server.tags.clone().unwrap_or_default(),
            description: server.description.clone(),
            sandbox,
            runner: None,
        }
    }

    /// Convert auth config
    fn convert_auth(auth: &OneMcpAuth) -> AuthConfig {
        use crate::config::AuthType;

        let auth_type = match auth.auth_type.as_str() {
            "jwt" => AuthType::Jwt,
            "oauth" => AuthType::OAuth,
            "static" => AuthType::Static,
            _ => AuthType::None,
        };

        AuthConfig {
            auth_type,
            token: auth.static_token.clone(),
            issuer: auth.oauth_issuer.clone(),
            client_id: auth.oauth_client_id.clone(),
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

    /// Convert features
    fn convert_features(features: &OneMcpFeatures) -> FeaturesConfig {
        FeaturesConfig {
            auth: true,
            scope_validation: features.scope_validation.unwrap_or(true),
            sandbox: true,
            hot_reload: features.hot_reload.unwrap_or(true),
            audit_logging: features.audit_logging.unwrap_or(true),
        }
    }

    /// Parse 1MCP config from YAML/JSON string
    pub fn parse(input: &str, format: ConfigFormat) -> Result<OneMcpConfig, String> {
        match format {
            ConfigFormat::Yaml => {
                serde_yaml::from_str(input).map_err(|e| format!("Failed to parse YAML: {}", e))
            }
            ConfigFormat::Json => {
                serde_json::from_str(input).map_err(|e| format!("Failed to parse JSON: {}", e))
            }
        }
    }

    /// Load and convert 1MCP config file
    pub async fn load_and_convert(path: &str) -> Result<SuperMcpConfig, String> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        let format = if path.ends_with(".json") {
            ConfigFormat::Json
        } else {
            ConfigFormat::Yaml
        };

        let one_mcp_config = Self::parse(&content, format)?;
        Ok(Self::convert(&one_mcp_config))
    }

    /// Check if config looks like 1MCP format
    pub fn detect_format(content: &str) -> bool {
        // Check for 1MCP-specific fields
        content.contains("rate_limiting:") ||
        content.contains("sandboxing:") ||
        content.contains("hot_reload:")
    }
}

/// Configuration format
#[derive(Debug, Clone, Copy)]
pub enum ConfigFormat {
    Yaml,
    Json,
}

/// Migration tool to help users migrate from 1MCP to Super MCP
pub struct OneMcpMigration;

impl OneMcpMigration {
    /// Check compatibility and generate migration report
    pub fn check_compatibility(one_mcp_config: &OneMcpConfig) -> CompatibilityReport {
        let mut report = CompatibilityReport {
            compatible: true,
            warnings: vec![],
            unsupported_features: vec![],
        };

        // Check for unsupported features
        for server in &one_mcp_config.servers {
            if server.command.contains("docker") {
                report.warnings.push(format!(
                    "Server '{}' uses Docker. Ensure Docker is available.",
                    server.name
                ));
            }
        }

        // Check auth compatibility
        if let Some(auth) = &one_mcp_config.auth {
            if auth.auth_type == "ldap" {
                report.unsupported_features.push("LDAP authentication".to_string());
                report.compatible = false;
            }
        }

        report
    }

    /// Generate Super MCP config with migration notes
    pub fn generate_config(one_mcp_config: &OneMcpConfig) -> (SuperMcpConfig, Vec<String>) {
        let super_mcp = OneMcpConfigAdapter::convert(one_mcp_config);
        let report = Self::check_compatibility(one_mcp_config);

        let notes = report
            .warnings
            .into_iter()
            .chain(report.unsupported_features.into_iter())
            .collect();

        (super_mcp, notes)
    }
}

/// Compatibility report
#[derive(Debug, Clone)]
pub struct CompatibilityReport {
    pub compatible: bool,
    pub warnings: Vec<String>,
    pub unsupported_features: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_server() {
        let one_mcp_server = OneMcpServer {
            name: "test-server".to_string(),
            command: "echo".to_string(),
            args: Some(vec!["hello".to_string()]),
            env: Some([("KEY".to_string(), "value".to_string())].into()),
            tags: Some(vec!["test".to_string()]),
            description: Some("Test server".to_string()),
            enabled: Some(true),
            sandbox: Some(OneMcpSandbox {
                enabled: Some(true),
                network: Some(false),
                filesystem: Some("readonly".to_string()),
                max_memory: Some(256),
                max_cpu: Some(50),
            }),
        };

        let converted = OneMcpConfigAdapter::convert_server(&one_mcp_server);

        assert_eq!(converted.name, "test-server");
        assert_eq!(converted.args, vec!["hello"]);
        assert_eq!(converted.env.get("KEY"), Some(&"value".to_string()));
        assert_eq!(converted.tags, vec!["test"]);
        assert!(converted.sandbox.enabled);
        assert_eq!(converted.sandbox.max_memory_mb, 256);
    }

    #[test]
    fn test_parse_yaml_config() {
        let yaml = r#"
server:
  host: 0.0.0.0
  port: 8080
servers:
  - name: test
    command: echo
    args:
      - hello
auth:
  type: jwt
  jwt_secret: secret123
"#;

        let config = OneMcpConfigAdapter::parse(yaml, ConfigFormat::Yaml).unwrap();
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.auth.unwrap().auth_type, "jwt");
    }

    #[test]
    fn test_detect_format() {
        let yaml_1mcp = r#"
server:
  host: 0.0.0.0
sandboxing:
  enabled: true
"#;
        assert!(OneMcpConfigAdapter::detect_format(yaml_1mcp));

        let super_mcp_config = r#"
[server]
host = "0.0.0.0"
"#;
        assert!(!OneMcpConfigAdapter::detect_format(super_mcp_config));
    }

    #[test]
    fn test_migration_report() {
        let config = OneMcpConfig {
            server: OneMcpServerConfig {
                host: "0.0.0.0".to_string(),
                port: 8080,
                tls: None,
                cert_path: None,
                key_path: None,
            },
            servers: vec![OneMcpServer {
                name: "docker-server".to_string(),
                command: "docker run test".to_string(),
                args: None,
                env: None,
                tags: None,
                description: None,
                enabled: None,
                sandbox: None,
            }],
            auth: None,
            sandboxing: None,
            features: None,
            logging: None,
            rate_limiting: None,
        };

        let report = OneMcpMigration::check_compatibility(&config);
        assert!(report.compatible);
        assert!(!report.warnings.is_empty()); // Should warn about Docker
    }
}
