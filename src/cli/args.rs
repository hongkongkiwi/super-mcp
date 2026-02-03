//! CLI argument types - shared between binary and tests
//!
//! This module contains all CLI argument types used by both the binary
//! and integration tests.

use crate::config::types::LazyLoadingMode;
use clap::{Parser, Subcommand};

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum LazyLoadingModeCli {
    /// Return meta-tools instead of actual tools
    Metatool,
    /// Preload some servers, lazy load others
    Hybrid,
    /// Full lazy loading - fetch schemas on demand
    Full,
}

impl From<LazyLoadingModeCli> for LazyLoadingMode {
    fn from(val: LazyLoadingModeCli) -> Self {
        match val {
            LazyLoadingModeCli::Metatool => LazyLoadingMode::Metatool,
            LazyLoadingModeCli::Hybrid => LazyLoadingMode::Hybrid,
            LazyLoadingModeCli::Full => LazyLoadingMode::Full,
        }
    }
}

#[derive(Parser)]
#[command(name = "supermcp")]
#[command(about = "Super MCP - Secure MCP server proxy with advanced sandboxing")]
#[command(version)]
pub enum Cli {
    /// Start the Super MCP server
    Serve(ServeArgs),
    /// Manage MCP servers
    Mcp(McpArgs),
    /// Manage presets
    Preset(PresetArgs),
    /// Search and install from registry
    Registry(RegistryArgs),
    /// Install/uninstall startup manager
    Install(InstallArgs),
    /// Validate configuration file
    Validate(ValidateArgs),
    /// Migrate from 1MCP configuration
    Migrate(MigrateArgs),
    /// Show migration guide and feature comparison
    Guide,
    /// Manage runtimes
    Runtime(RuntimeArgs),
    /// Call an MCP tool directly (lightweight client)
    Call(CallArgs),
    /// List tools from an MCP server or skill
    Tools(ToolsArgs),
    /// List all available providers (MCPs and skills)
    Providers(ProvidersArgs),
    /// Import MCP servers from AI editors (cursor, claude, vscode, etc.)
    Import(ImportArgs),
}

#[derive(Parser)]
pub struct ServeArgs {
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/supermcp/config.toml")]
    pub config: String,
    /// Host to bind to
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    pub host: String,
    /// Port to bind to
    #[arg(short, long, default_value = "3000")]
    pub port: u16,
    /// Log level
    #[arg(short, long, default_value = "info")]
    pub log_level: String,
    /// Enable lazy loading mode (metatool, hybrid, full)
    #[arg(long, value_enum)]
    pub lazy: Option<LazyLoadingModeCli>,
}

#[derive(Parser)]
pub struct McpArgs {
    #[command(subcommand)]
    pub command: McpCommand,
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/super-mcp/config.toml", global = true)]
    pub config: String,
}

#[derive(Subcommand, Debug)]
pub enum McpCommand {
    /// Add a new MCP server
    Add {
        name: String,
        command: String,
        /// Arguments for the command
        #[arg(short, long, value_delimiter = ' ')]
        args: Option<Vec<String>>,
        /// Environment variables (KEY=value format)
        #[arg(short, long, value_delimiter = ',')]
        env: Option<Vec<String>>,
        /// Tags for the server
        #[arg(short, long, value_delimiter = ',')]
        tags: Option<Vec<String>>,
        /// Description of the server
        #[arg(short, long)]
        description: Option<String>,
    },
    /// List configured MCP servers
    List,
    /// Remove an MCP server
    Remove { name: String },
    /// Show MCP server status
    Status { name: Option<String> },
    /// Edit an MCP server
    Edit {
        name: String,
        /// New command
        #[arg(short, long)]
        command: Option<String>,
        /// New arguments
        #[arg(short, long, value_delimiter = ' ')]
        args: Option<Vec<String>>,
        /// New environment variables
        #[arg(short, long, value_delimiter = ',')]
        env: Option<Vec<String>>,
        /// New tags
        #[arg(short, long, value_delimiter = ',')]
        tags: Option<Vec<String>>,
        /// New description
        #[arg(short, long)]
        description: Option<String>,
    },
}

#[derive(Parser)]
pub struct PresetArgs {
    #[command(subcommand)]
    pub command: PresetCommand,
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/super-mcp/config.toml", global = true)]
    pub config: String,
}

#[derive(Subcommand, Debug)]
pub enum PresetCommand {
    /// Create a new preset
    Create {
        name: String,
        /// Tags for the preset (comma-separated)
        #[arg(short, long, value_delimiter = ',')]
        tags: Option<Vec<String>>,
        /// Description of the preset
        #[arg(short, long)]
        description: Option<String>,
    },
    /// List available presets
    List,
    /// Edit a preset
    Edit {
        name: String,
        /// New tags
        #[arg(short, long, value_delimiter = ',')]
        tags: Option<Vec<String>>,
        /// New description
        #[arg(short, long)]
        description: Option<String>,
    },
    /// Test a preset (shows matching servers)
    Test { name: String },
    /// Remove a preset
    Remove { name: String },
}

#[derive(Parser)]
pub struct RegistryArgs {
    #[command(subcommand)]
    pub command: RegistryCommand,
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/super-mcp/config.toml", global = true)]
    pub config: String,
}

#[derive(Subcommand, Debug)]
pub enum RegistryCommand {
    /// Search for MCP servers in the registry
    Search { query: String },
    /// Install an MCP server from the registry
    Install { name: String },
    /// Show registry information
    Info { name: String },
    /// Refresh the registry cache
    Refresh,
}

#[derive(Parser)]
pub struct InstallArgs {
    /// Startup manager to use (launchd, systemd, openrc, runit, nssm, schtasks)
    #[arg(short, long)]
    pub manager: Option<String>,
    /// Path to the supermcp binary
    #[arg(short, long)]
    pub binary: Option<String>,
    /// Path to the configuration file
    #[arg(short, long)]
    pub config: Option<String>,
    /// Uninstall instead of installing
    #[arg(long)]
    pub uninstall: bool,
}

#[derive(Parser)]
pub struct ValidateArgs {
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/supermcp/config.toml")]
    pub config: String,
    /// Output format
    #[arg(short, long, default_value = "text", value_parser = ["text", "json"])]
    pub format: String,
}

#[derive(Parser)]
pub struct MigrateArgs {
    /// Input 1MCP configuration file
    #[arg(short, long)]
    pub input: String,
    /// Output Super MCP configuration file
    #[arg(short, long)]
    pub output: Option<String>,
    /// Output format (toml or json)
    #[arg(short, long, default_value = "toml")]
    pub format: String,
    /// Dry run - don't write file, just validate
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Parser)]
pub struct RuntimeArgs {
    #[command(subcommand)]
    pub command: RuntimeCommand,
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/super-mcp/config.toml", global = true)]
    pub config: String,
}

#[derive(Subcommand, Debug)]
pub enum RuntimeCommand {
    /// Add a new runtime
    Add {
        name: String,
        /// Runtime type (python_wasm, node_pnpm, node_npm, node_bun)
        #[arg(short, long)]
        type_: String,
        /// Packages to install (comma-separated for Node.js)
        #[arg(short, long, value_delimiter = ',')]
        packages: Option<Vec<String>>,
        /// Working directory
        #[arg(short, long)]
        working_dir: Option<String>,
        /// Environment variables (KEY=value format)
        #[arg(short, long, value_delimiter = ',')]
        env: Option<Vec<String>>,
        /// Maximum memory in MB
        #[arg(long)]
        max_memory: Option<u64>,
        /// Maximum CPU percentage
        #[arg(long)]
        max_cpu: Option<u32>,
        /// Timeout in seconds
        #[arg(long)]
        timeout: Option<u64>,
        /// Allow network access
        #[arg(long)]
        network: bool,
    },
    /// List all configured runtimes
    List {
        /// Output as JSON
        #[arg(short, long)]
        json: bool,
    },
    /// Remove a runtime
    Remove { name: String },
    /// Show runtime information
    Info { name: String },
    /// Validate all runtimes
    Validate,
    /// Execute a script using a runtime
    Exec {
        /// Runtime name to use
        runtime: String,
        /// Script to execute
        #[arg(short, long)]
        script: Option<String>,
        /// File to execute
        #[arg(short, long)]
        file: Option<String>,
    },
}

#[derive(Parser)]
pub struct CallArgs {
    /// Target tool to call (format: server.tool or just tool with --stdio/--http-url/--skill)
    pub target: String,
    /// Tool arguments in key:value or key=value format
    #[arg(value_name = "ARGS")]
    pub args: Vec<String>,
    /// Configuration file path
    #[arg(short, long)]
    pub config: Option<String>,
    /// Ad-hoc stdio command (e.g., "npx -y @modelcontextprotocol/server-filesystem /tmp")
    #[arg(long, conflicts_with_all = ["http_url", "skill"])]
    pub stdio: Option<String>,
    /// Ad-hoc HTTP/SSE URL
    #[arg(long, conflicts_with_all = ["stdio", "skill"])]
    pub http_url: Option<String>,
    /// Use a skill provider
    #[arg(long, conflicts_with_all = ["stdio", "http_url"])]
    pub skill: Option<String>,
    /// Environment variables (KEY=value format)
    #[arg(short, long, value_delimiter = ',')]
    pub env: Vec<String>,
    /// Output as JSON
    #[arg(short, long)]
    pub json: bool,
}

#[derive(Parser)]
pub struct ToolsArgs {
    /// Provider name to list tools from (optional if using --stdio, --http-url, or --all)
    pub provider: Option<String>,
    /// Configuration file path
    #[arg(short, long)]
    pub config: Option<String>,
    /// Ad-hoc stdio command
    #[arg(long, conflicts_with = "http_url")]
    pub stdio: Option<String>,
    /// Ad-hoc HTTP/SSE URL
    #[arg(long, conflicts_with = "stdio")]
    pub http_url: Option<String>,
    /// Skill name to list tools from
    #[arg(long)]
    pub skill: Option<String>,
    /// Show full schema for each tool
    #[arg(long)]
    pub schema: bool,
    /// List tools from all providers (MCPs and skills)
    #[arg(long)]
    pub all: bool,
    /// Output as JSON
    #[arg(short, long)]
    pub json: bool,
}

#[derive(Parser)]
pub struct ProvidersArgs {
    /// Configuration file path
    #[arg(short, long)]
    pub config: Option<String>,
    /// Output as JSON
    #[arg(short, long)]
    pub json: bool,
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum ImportSource {
    All,
    Cursor,
    Claude,
    Vscode,
    Codex,
    KimiCli,
    Windsurf,
    Opencode,
    Gemini,
    Qwen,
    #[value(name = "github-copilot")]
    GithubCopilot,
}

#[derive(Parser)]
pub struct ImportArgs {
    /// Specific source to import from (cursor, claude, vscode, codex, kimi-cli, windsurf, opencode, all)
    #[arg(value_enum)]
    pub source: ImportSource,
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/supermcp/config.toml")]
    pub config: String,
    /// Dry run - show what would be imported without saving
    #[arg(long)]
    pub dry_run: bool,
    /// Output as JSON
    #[arg(short, long)]
    pub json: bool,
}
