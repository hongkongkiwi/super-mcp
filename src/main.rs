use clap::{Parser, Subcommand};
use super_mcp::cli;
use super_mcp::config::ConfigManager;
use super_mcp::core::ServerManager;
use super_mcp::http_server::HttpServer;
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
    /// Validate configuration file
    Validate(ValidateArgs),
    /// Migrate from 1MCP configuration
    Migrate(MigrateArgs),
    /// Show migration guide and feature comparison
    Guide,
}

#[derive(Parser)]
struct ServeArgs {
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/super-mcp/config.toml")]
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
struct ValidateArgs {
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/super-mcp/config.toml")]
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
            super_mcp::compat::MigrationHelper::print_migration_guide();
            println!();
            super_mcp::compat::MigrationHelper::print_feature_comparison();
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
    use super_mcp::compat::config::{OneMcpConfigAdapter, OneMcpMigration};
    use super_mcp::compat::MigrationHelper;

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
    let one_mcp_config = OneMcpConfigAdapter::parse(&yaml_content, super_mcp::compat::config::ConfigFormat::Yaml)
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
    use super_mcp::config::validation::ConfigValidator;
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
