use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use validator::Validate;

// Re-export runtime types for config use
pub use crate::runtime::types::{ResourceLimits, RuntimeConfig, RuntimeType};

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Validate, Default)]
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
    pub lazy_loading: LazyLoadingConfig,
    #[serde(default)]
    pub servers: Vec<McpServerConfig>,
    #[serde(default)]
    pub presets: Vec<PresetConfig>,
    #[serde(default)]
    pub registry: RegistryConfig,
    #[serde(default)]
    pub runtimes: Vec<RuntimeConfig>,
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
    #[serde(rename = "type", alias = "auth_type")]
    pub auth_type: AuthType,
    pub issuer: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub token: Option<String>, // For static auth
    pub jwt_secret: Option<String>,
    pub auth_url: Option<String>,
    pub token_url: Option<String>,
    pub introspection_url: Option<String>,
    pub userinfo_url: Option<String>,
    pub jwks_url: Option<String>,
    pub expected_audiences: Vec<String>,
    pub allowed_algs: Vec<String>,
    pub jwks_cache_ttl_seconds: u64,
    pub allow_unverified_jwt: bool,
    pub required_scopes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    #[default]
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
            client_secret: None,
            token: None,
            jwt_secret: None,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum LogFormat {
    #[default]
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

/// Lazy loading configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LazyLoadingConfig {
    /// Lazy loading mode
    pub mode: LazyLoadingMode,
    /// Schema cache TTL in seconds
    pub schema_cache_ttl_seconds: u64,
    /// Servers to preload regardless of mode
    pub preload_servers: Vec<String>,
    /// Enable caching
    pub cache_enabled: bool,
    /// Presets to load with lazy loading
    pub preload_presets: Vec<String>,
    /// Maximum concurrent fetches per server
    pub max_concurrent_fetches: u32,
}

impl Default for LazyLoadingConfig {
    fn default() -> Self {
        Self {
            mode: LazyLoadingMode::default(),
            schema_cache_ttl_seconds: 300,
            preload_servers: Vec::new(),
            cache_enabled: true,
            preload_presets: Vec::new(),
            max_concurrent_fetches: 4,
        }
    }
}

/// Lazy loading mode
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LazyLoadingMode {
    /// Lazy loading disabled, load all schemas eagerly
    #[default]
    Disabled,
    /// Return meta-tools (tool_list, tool_schema, tool_invoke)
    Metatool,
    /// Preload configured servers, lazy load others
    Hybrid,
    /// Full lazy loading, fetch schemas on demand
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(default)]
pub struct McpServerConfig {
    pub name: String,
    /// Command to run (local binary or package runner like "uvx @mcp/server")
    pub command: String,
    /// Arguments for the command
    pub args: Vec<String>,
    /// Environment variables
    pub env: HashMap<String, String>,
    /// Tags for categorization
    pub tags: Vec<String>,
    /// Description
    pub description: Option<String>,
    /// Sandbox configuration
    pub sandbox: SandboxConfig,
}

/// Detected runner type from command
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectedRunner {
    /// Local binary (not a known package runner)
    Local,
    /// Python via uvx
    Uvx,
    /// Node.js via pnpm dlx
    Pnpm,
    /// Node.js via pnpx
    Pnpx,
    /// Node.js via npx
    Npx,
    /// Node.js via npm exec
    Npm,
    /// Bun via bunx
    Bunx,
    /// Python via pipx
    Pipx,
    /// Go via go run
    GoRun,
    /// Rust via cargo run
    CargoRun,
    /// Deno via deno run
    DenoRun,
    /// Ruby via bundle exec
    BundleExec,
    /// PHP via composer exec
    ComposerExec,
}

impl McpServerConfig {
    /// Auto-detect runner type from command
    pub fn detected_runner(&self) -> DetectedRunner {
        let exe = std::path::PathBuf::from(&self.command)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_default();

        match exe.as_str() {
            "uvx" => DetectedRunner::Uvx,
            "pnpm" | "pnpm-dlx" => DetectedRunner::Pnpm,
            "pnpx" => DetectedRunner::Pnpx,
            "npx" => DetectedRunner::Npx,
            "npm" => DetectedRunner::Npm,
            "bunx" | "bun" => DetectedRunner::Bunx,
            "pipx" => DetectedRunner::Pipx,
            "go" => DetectedRunner::GoRun,
            "cargo" => DetectedRunner::CargoRun,
            "deno" => DetectedRunner::DenoRun,
            "bundle" => DetectedRunner::BundleExec,
            "composer" => DetectedRunner::ComposerExec,
            _ => DetectedRunner::Local,
        }
    }

    /// Check if this server uses a package runner
    pub fn is_package_runner(&self) -> bool {
        self.detected_runner() != DetectedRunner::Local
    }

    /// Get the package name for package runners (if applicable)
    pub fn package_name(&self) -> Option<&str> {
        let runner = self.detected_runner();
        if runner == DetectedRunner::Local {
            return None;
        }
        // Package is typically the first arg for most runners
        self.args.first().map(|s| s.as_str())
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum SandboxType {
    #[default]
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




