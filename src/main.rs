use clap::{Parser, Subcommand};
use supermcp::cli;
use supermcp::config::types::LazyLoadingMode;
use supermcp::config::ConfigManager;
use supermcp::core::ServerManager;
use supermcp::http_server::HttpServer;
use std::sync::Arc;
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "supermcp")]
#[command(about = "Super MCP - Secure MCP server proxy with advanced sandboxing")]
#[command(version)]
enum Cli {
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
struct ServeArgs {
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/supermcp/config.toml")]
    config: String,
    /// Host to bind to
    #[arg(short = 'H', long, default_value = "127.0.0.1")]
    host: String,
    /// Port to bind to
    #[arg(short, long, default_value = "3000")]
    port: u16,
    /// Log level
    #[arg(short, long, default_value = "info")]
    log_level: String,
    /// Enable lazy loading mode (metatool, hybrid, full)
    #[arg(long, value_enum)]
    lazy: Option<LazyLoadingModeCli>,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum LazyLoadingModeCli {
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
struct McpArgs {
    #[command(subcommand)]
    command: McpCommand,
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/super-mcp/config.toml", global = true)]
    config: String,
}

#[derive(Subcommand, Debug)]
enum McpCommand {
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
struct PresetArgs {
    #[command(subcommand)]
    command: PresetCommand,
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/super-mcp/config.toml", global = true)]
    config: String,
}

#[derive(Subcommand, Debug)]
enum PresetCommand {
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
struct RegistryArgs {
    #[command(subcommand)]
    command: RegistryCommand,
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/super-mcp/config.toml", global = true)]
    config: String,
}

#[derive(Subcommand, Debug)]
enum RegistryCommand {
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
struct InstallArgs {
    /// Startup manager to use (launchd, systemd, openrc, runit, nssm, schtasks)
    #[arg(short, long)]
    manager: Option<String>,
    /// Path to the supermcp binary
    #[arg(short, long)]
    binary: Option<String>,
    /// Path to the configuration file
    #[arg(short, long)]
    config: Option<String>,
    /// Uninstall instead of installing
    #[arg(long)]
    uninstall: bool,
}

#[derive(Parser)]
struct ValidateArgs {
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/supermcp/config.toml")]
    config: String,
    /// Output format
    #[arg(short, long, default_value = "text", value_parser = ["text", "json"])]
    format: String,
}

#[derive(Parser)]
struct MigrateArgs {
    /// Input 1MCP configuration file
    #[arg(short, long)]
    input: String,
    /// Output Super MCP configuration file
    #[arg(short, long)]
    output: Option<String>,
    /// Output format (toml or json)
    #[arg(short, long, default_value = "toml")]
    format: String,
    /// Dry run - don't write file, just validate
    #[arg(long)]
    dry_run: bool,
}

#[derive(Parser)]
struct RuntimeArgs {
    #[command(subcommand)]
    command: RuntimeCommand,
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/super-mcp/config.toml", global = true)]
    config: String,
}

#[derive(Parser)]
struct CallArgs {
    /// Target tool to call (format: server.tool or just tool with --stdio/--http-url/--skill)
    target: String,
    /// Tool arguments in key:value or key=value format
    #[arg(value_name = "ARGS")]
    args: Vec<String>,
    /// Configuration file path
    #[arg(short, long)]
    config: Option<String>,
    /// Ad-hoc stdio command (e.g., "npx -y @modelcontextprotocol/server-filesystem /tmp")
    #[arg(long, conflicts_with_all = ["http_url", "skill"])]
    stdio: Option<String>,
    /// Ad-hoc HTTP/SSE URL
    #[arg(long, conflicts_with_all = ["stdio", "skill"])]
    http_url: Option<String>,
    /// Use a skill provider
    #[arg(long, conflicts_with_all = ["stdio", "http_url"])]
    skill: Option<String>,
    /// Environment variables (KEY=value format)
    #[arg(short, long, value_delimiter = ',')]
    env: Vec<String>,
    /// Output as JSON
    #[arg(short, long)]
    json: bool,
}

#[derive(Parser)]
struct ToolsArgs {
    /// Provider name to list tools from (optional if using --stdio, --http-url, or --all)
    provider: Option<String>,
    /// Configuration file path
    #[arg(short, long)]
    config: Option<String>,
    /// Ad-hoc stdio command
    #[arg(long, conflicts_with = "http_url")]
    stdio: Option<String>,
    /// Ad-hoc HTTP/SSE URL
    #[arg(long, conflicts_with = "stdio")]
    http_url: Option<String>,
    /// Show full schema for each tool
    #[arg(long)]
    schema: bool,
    /// List tools from all providers (MCPs and skills)
    #[arg(long)]
    all: bool,
    /// Output as JSON
    #[arg(short, long)]
    json: bool,
}

#[derive(Parser)]
struct ProvidersArgs {
    /// Configuration file path
    #[arg(short, long)]
    config: Option<String>,
    /// Output as JSON
    #[arg(short, long)]
    json: bool,
}

#[derive(Parser)]
struct ImportArgs {
    /// Specific source to import from (cursor, claude, vscode, codex, kimi-cli, windsurf, opencode, all)
    #[arg(value_enum)]
    source: ImportSource,
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/supermcp/config.toml")]
    config: String,
    /// Dry run - show what would be imported without saving
    #[arg(long)]
    dry_run: bool,
    /// Output as JSON
    #[arg(short, long)]
    json: bool,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum ImportSource {
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

#[derive(Subcommand, Debug)]
enum RuntimeCommand {
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli {
        Cli::Serve(args) => {
            // Initialize tracing
            tracing_subscriber::fmt()
                .with_env_filter(&args.log_level)
                .init();

            info!("Starting Super MCP server on {}:{}", args.host, args.port);
            info!("Config file: {}", args.config);

            // Expand tilde in config path
            let config_path = shellexpand::tilde(&args.config).to_string();

            // Load configuration
            let config_manager = ConfigManager::new(&config_path).await?;
            let mut config = config_manager.get_config();

            // Override with CLI args
            config.server.host = args.host;
            config.server.port = args.port;

            // Apply --lazy CLI flag if provided
            if let Some(lazy_mode) = args.lazy {
                info!("Lazy loading mode: {:?}", lazy_mode);
                config.lazy_loading.mode = lazy_mode.into();
            }

            // Create server manager
            let server_manager = Arc::new(ServerManager::new());

            // Add configured servers
            for server_config in config.servers.clone() {
                info!("Configuring server: {}", server_config.name);
                if let Err(e) = server_manager.add_server(server_config).await {
                    error!("Failed to add server: {}", e);
                }
            }

            // Create and run HTTP server
            let http_server = HttpServer::new(config, server_manager);
            http_server.run().await?;
        }
        Cli::Mcp(args) => {
            match args.command {
                McpCommand::Add {
                    name,
                    command,
                    args: cmd_args,
                    env,
                    tags,
                    description,
                } => {
                    if let Err(e) = cli::mcp::add(&args.config, &name, &command, cmd_args, env, tags, description).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                McpCommand::List => {
                    if let Err(e) = cli::mcp::list(&args.config).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                McpCommand::Remove { name } => {
                    if let Err(e) = cli::mcp::remove(&args.config, &name).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                McpCommand::Status { name } => {
                    if let Err(e) = cli::mcp::status(&args.config, name.as_deref()).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                McpCommand::Edit {
                    name,
                    command,
                    args: cmd_args,
                    env,
                    tags,
                    description,
                } => {
                    if let Err(e) = cli::mcp::edit(&args.config, &name, command.as_deref(), cmd_args, env, tags, description).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
        Cli::Preset(args) => {
            match args.command {
                PresetCommand::Create { name, tags, description } => {
                    if let Err(e) = cli::preset::create(&args.config, &name, tags, description).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                PresetCommand::List => {
                    if let Err(e) = cli::preset::list(&args.config).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                PresetCommand::Edit { name, tags, description } => {
                    if let Err(e) = cli::preset::edit(&args.config, &name, tags, description).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                PresetCommand::Test { name } => {
                    if let Err(e) = cli::preset::test(&args.config, &name).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                PresetCommand::Remove { name } => {
                    if let Err(e) = cli::preset::remove(&args.config, &name).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
        Cli::Registry(args) => {
            match args.command {
                RegistryCommand::Search { query } => {
                    if let Err(e) = cli::registry::search(&args.config, &query).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                RegistryCommand::Install { name } => {
                    if let Err(e) = cli::registry::install(&args.config, &name).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                RegistryCommand::Info { name } => {
                    if let Err(e) = cli::registry::info(&args.config, &name).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                RegistryCommand::Refresh => {
                    if let Err(e) = cli::registry::refresh(&args.config).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
        Cli::Install(args) => {
            if let Err(e) = cli::install::install(
                args.binary.as_deref(),
                args.config.as_deref(),
                args.manager.as_deref(),
                args.uninstall,
            ).await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Cli::Validate(args) => {
            if let Err(e) = validate_config(&args.config, &args.format).await {
                eprintln!("Validation failed: {}", e);
                std::process::exit(1);
            }
        }
        Cli::Migrate(args) => {
            if let Err(e) = migrate_config(&args.input, args.output.as_deref(), &args.format, args.dry_run).await {
                eprintln!("Migration failed: {}", e);
                std::process::exit(1);
            }
        }
        Cli::Guide => {
            supermcp::compat::MigrationHelper::print_migration_guide();
            println!();
            supermcp::compat::MigrationHelper::print_feature_comparison();
        }
        Cli::Runtime(args) => {
            match args.command {
                RuntimeCommand::Add {
                    name,
                    type_,
                    packages,
                    working_dir,
                    env,
                    max_memory,
                    max_cpu,
                    timeout,
                    network,
                } => {
                    if let Err(e) = cli::runtime::add(
                        &args.config,
                        &name,
                        &type_,
                        packages,
                        working_dir,
                        env,
                        max_memory,
                        max_cpu,
                        timeout,
                        Some(network),
                    ).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                RuntimeCommand::List { json } => {
                    if let Err(e) = cli::runtime::list(&args.config, json).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                RuntimeCommand::Remove { name } => {
                    if let Err(e) = cli::runtime::remove(&args.config, &name).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                RuntimeCommand::Info { name } => {
                    if let Err(e) = cli::runtime::info(&args.config, &name).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                RuntimeCommand::Validate => {
                    if let Err(e) = cli::runtime::validate(&args.config).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                RuntimeCommand::Exec { runtime, script, file } => {
                    if script.is_none() && file.is_none() {
                        eprintln!("Error: Either --script or --file must be provided");
                        std::process::exit(1);
                    }
                    if let Err(e) = cli::runtime::execute(&args.config, &runtime, script.as_deref(), file).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
        Cli::Call(args) => {
            if let Err(e) = cli::call::execute(
                args.config.as_deref(),
                &args.target,
                args.args,
                args.stdio.as_deref(),
                args.http_url.as_deref(),
                args.skill.as_deref(),
                args.env,
                args.json,
            ).await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Cli::Tools(args) => {
            if let Err(e) = cli::call::list_tools(
                args.config.as_deref(),
                args.provider.as_deref(),
                args.stdio.as_deref(),
                args.http_url.as_deref(),
                None, // skill_name
                args.schema,
                args.json,
                args.all,
            ).await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Cli::Providers(args) => {
            if let Err(e) = cli::call::list_providers(
                args.config.as_deref(),
                args.json,
            ).await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Cli::Import(args) => {
            if let Err(e) = handle_import(args).await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

async fn migrate_config(
    input: &str,
    output: Option<&str>,
    format: &str,
    dry_run: bool,
) -> anyhow::Result<()> {
    use supermcp::compat::config::{OneMcpConfigAdapter, OneMcpMigration};
    use supermcp::compat::MigrationHelper;

    let input_path = shellexpand::tilde(input).to_string();
    
    // Validate input
    if let Err(issues) = MigrationHelper::validate_migration(&input_path) {
        eprintln!("Validation errors:");
        for issue in issues {
            eprintln!("  • {}", issue);
        }
        std::process::exit(1);
    }

    // Load and convert 1MCP config
    let super_mcp_config = OneMcpConfigAdapter::load_and_convert(&input_path).await
        .map_err(|e| anyhow::anyhow!("Failed to convert config: {}", e))?;

    // Get migration notes
    let yaml_content = tokio::fs::read_to_string(&input_path).await?;
    let one_mcp_config = OneMcpConfigAdapter::parse(&yaml_content, supermcp::compat::config::ConfigFormat::Yaml)
        .map_err(|e| anyhow::anyhow!("Failed to parse 1MCP config: {}", e))?;
    let (_config, notes) = OneMcpMigration::generate_config(&one_mcp_config);

    // Generate output
    let output_content = match format {
        "json" => serde_json::to_string_pretty(&super_mcp_config)?,
        _ => toml::to_string_pretty(&super_mcp_config)?,
    };

    if dry_run {
        println!("=== Dry Run - Configuration Preview ===\n");
        println!("{}", output_content);
        if !notes.is_empty() {
            println!("\n=== Migration Notes ===");
            for note in notes {
                println!("  • {}", note);
            }
        }
    } else if let Some(output_path) = output {
        let expanded_path = shellexpand::tilde(output_path).to_string();
        
        // Ensure directory exists
        if let Some(parent) = std::path::Path::new(&expanded_path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        
        tokio::fs::write(&expanded_path, output_content).await?;
        println!("✓ Configuration migrated successfully:");
        println!("  Input:  {}", input_path);
        println!("  Output: {}", expanded_path);
        
        if !notes.is_empty() {
            println!("\n=== Migration Notes ===");
            for note in notes {
                println!("  • {}", note);
            }
        }
    } else {
        // Output to stdout
        println!("{}", output_content);
    }

    Ok(())
}

async fn validate_config(config_path: &str, format: &str) -> anyhow::Result<()> {
    use supermcp::config::validation::ConfigValidator;
    use serde_json::json;

    let path = shellexpand::tilde(config_path).to_string();
    
    let validator = ConfigValidator::new();
    let result = validator.validate_file(&path).await;

    match format {
        "json" => {
            let output = match &result {
                Ok(_) => json!({
                    "valid": true,
                    "path": path,
                    "errors": []
                }),
                Err(errors) => json!({
                    "valid": false,
                    "path": path,
                    "errors": errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
                }),
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        _ => {
            match result {
                Ok(_) => {
                    println!("✓ Configuration is valid: {}", path);
                }
                Err(errors) => {
                    println!("✗ Configuration is invalid: {}", path);
                    println!("\nErrors:");
                    for error in errors {
                        println!("  • {}", error);
                    }
                    std::process::exit(1);
                }
            }
        }
    }

    Ok(())
}


async fn handle_import(args: ImportArgs) -> anyhow::Result<()> {
    use supermcp::cli::discover;
    use serde_json::json;

    // Discover MCPs based on source
    let mcps = match args.source {
        ImportSource::All => discover::discover_all().await?,
        ImportSource::Cursor => discover::discover_cursor().await?,
        ImportSource::Claude => discover::discover_claude().await?,
        ImportSource::Vscode => discover::discover_vscode_extensions().await?,
        ImportSource::Codex => discover::discover_codex().await?,
        ImportSource::KimiCli => discover::discover_kimi_cli().await?,
        ImportSource::Windsurf => discover::discover_windsurf().await?,
        ImportSource::Opencode => discover::discover_opencode().await?,
        ImportSource::Gemini => discover::discover_gemini().await?,
        ImportSource::Qwen => discover::discover_qwen().await?,
        ImportSource::GithubCopilot => discover::discover_github_copilot().await?,
    };

    if mcps.is_empty() {
        if args.json {
            println!("{}", json!({"imported": [], "count": 0}));
        } else {
            println!("No MCP servers found from {:?}.", args.source);
        }
        return Ok(());
    }

    // Import into config
    let imported = discover::import_discovered(&args.config, mcps.clone(), args.dry_run).await?;

    if args.json {
        println!("{}", json!({
            "imported": imported,
            "count": imported.len(),
            "dry_run": args.dry_run,
            "discovered": mcps.iter().map(|m| {
                json!({
                    "name": m.name,
                    "source": m.source,
                    "command": m.command,
                    "args": m.args,
                })
            }).collect::<Vec<_>>(),
        }));
    } else {
        println!("\n📥 Importing from {:?}\n", args.source);
        
        // Group by source
        let mut by_source: std::collections::HashMap<String, Vec<&discover::DiscoveredMcp>> = std::collections::HashMap::new();
        for mcp in &mcps {
            by_source.entry(mcp.source.clone()).or_default().push(mcp);
        }

        for (source, servers) in by_source {
            println!("  [{}]", source);
            for server in servers {
                let name = if imported.contains(&server.name) {
                    &server.name
                } else {
                    // It was renamed
                    imported.iter().find(|i| i.starts_with(&format!("{}-", server.name))).unwrap_or(&server.name)
                };
                println!("    - {} ({})", name, server.command);
            }
        }

        if args.dry_run {
            println!("\n✓ Dry run - {} server(s) would be imported", imported.len());
            println!("  Run without --dry-run to save to config.");
        } else {
            println!("\n✓ Successfully imported {} server(s) to {}", imported.len(), args.config);
        }
    }

    Ok(())
}
