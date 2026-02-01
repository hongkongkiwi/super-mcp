use clap::Parser;
use mcp_one::config::ConfigManager;
use mcp_one::core::ServerManager;
use mcp_one::http_server::HttpServer;
use std::sync::Arc;
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "mcpo")]
#[command(about = "Secure MCP server proxy with sandboxing")]
#[command(version)]
enum Cli {
    /// Start the MCP-One server
    Serve(ServeArgs),
    /// Manage MCP servers
    Mcp(McpArgs),
    /// Manage presets
    Preset(PresetArgs),
    /// Search and install from registry
    Registry(RegistryArgs),
}

#[derive(Parser)]
struct ServeArgs {
    /// Configuration file path
    #[arg(short, long, default_value = "~/.config/mcp-one/config.toml")]
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
}

#[derive(Parser, Debug)]
enum McpCommand {
    /// Add a new MCP server
    Add { name: String, command: String },
    /// List configured MCP servers
    List,
    /// Remove an MCP server
    Remove { name: String },
    /// Show MCP server status
    Status { name: Option<String> },
}

#[derive(Parser)]
struct PresetArgs {
    #[command(subcommand)]
    command: PresetCommand,
}

#[derive(Parser, Debug)]
enum PresetCommand {
    /// Create a new preset
    Create { name: String },
    /// List available presets
    List,
    /// Edit a preset
    Edit { name: String },
    /// Test a preset
    Test { name: String },
}

#[derive(Parser)]
struct RegistryArgs {
    #[command(subcommand)]
    command: RegistryCommand,
}

#[derive(Parser, Debug)]
enum RegistryCommand {
    /// Search for MCP servers in the registry
    Search { query: String },
    /// Install an MCP server from the registry
    Install { name: String },
    /// Show registry information
    Info { name: String },
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

            info!("Starting MCP-One server on {}:{}", args.host, args.port);
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
            println!("MCP command not yet implemented: {:?}", args.command);
        }
        Cli::Preset(args) => {
            println!("Preset command not yet implemented: {:?}", args.command);
        }
        Cli::Registry(args) => {
            println!("Registry command not yet implemented: {:?}", args.command);
        }
    }

    Ok(())
}
