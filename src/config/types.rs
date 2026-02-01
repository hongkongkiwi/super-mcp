use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use validator::Validate;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Validate)]
pub struct Config {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub features: FeaturesConfig,
    #[serde(default)]
    pub rate_limit: RateLimitConfig,
    #[serde(default)]
    pub audit: AuditConfig,
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
    #[serde(default)]
    pub presets: Vec<PresetConfig>,
    #[serde(default)]
    pub registry: RegistryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3000,
            cert_path: None,
            key_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct AuthConfig {
    pub auth_type: AuthType,
    pub issuer: Option<String>,
    pub client_id: Option<String>,
    pub token: Option<String>, // For static auth
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    None,
    Static,
    Jwt,
    OAuth,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            auth_type: AuthType::None,
            issuer: None,
            client_id: None,
            token: None,
        }
    }
}

impl Default for AuthType {
    fn default() -> Self {
        AuthType::None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct FeaturesConfig {
    pub auth: bool,
    pub scope_validation: bool,
    pub sandbox: bool,
    pub hot_reload: bool,
    pub audit_logging: bool,
}

impl Default for FeaturesConfig {
    fn default() -> Self {
        Self {
            auth: true,
            scope_validation: true,
            sandbox: true,
            hot_reload: true,
            audit_logging: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    pub burst_size: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 100,
            burst_size: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct AuditConfig {
    pub path: String,
    pub format: LogFormat,
    pub max_size_mb: u64,
    pub max_files: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    Json,
    Pretty,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            path: "/var/log/super-mcp/audit.log".to_string(),
            format: LogFormat::Json,
            max_size_mb: 100,
            max_files: 10,
        }
    }
}

impl Default for LogFormat {
    fn default() -> Self {
        LogFormat::Json
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub sandbox: SandboxConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SandboxConfig {
    pub enabled: bool,
    #[serde(rename = "type")]
    pub sandbox_type: SandboxType,
    pub network: bool,
    pub filesystem: FilesystemAccess,
    pub env_inherit: bool,
    pub max_memory_mb: u64,
    pub max_cpu_percent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SandboxType {
    Default,
    Container,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FilesystemAccess {
    Simple(String),           // "readonly" or "full"
    Paths(Vec<String>),       // ["/allowed/path"]
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sandbox_type: SandboxType::Default,
            network: false,
            filesystem: FilesystemAccess::Simple("readonly".to_string()),
            env_inherit: false,
            max_memory_mb: 512,
            max_cpu_percent: 50,
        }
    }
}

impl Default for SandboxType {
    fn default() -> Self {
        SandboxType::Default
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PresetConfig {
    pub name: String,
    pub tags: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RegistryConfig {
    pub url: String,
    pub cache_dir: String,
    pub cache_ttl_hours: u64,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            url: "https://registry.modelcontextprotocol.io".to_string(),
            cache_dir: "~/.cache/super-mcp/registry".to_string(),
            cache_ttl_hours: 24,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            auth: AuthConfig::default(),
            features: FeaturesConfig::default(),
            rate_limit: RateLimitConfig::default(),
            audit: AuditConfig::default(),
            servers: vec![],
            presets: vec![],
            registry: RegistryConfig::default(),
        }
    }
}
