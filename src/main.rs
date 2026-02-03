use clap::Parser;
use supermcp::cli::args::{
    Cli, ImportArgs, ImportSource, McpCommand, PresetCommand,
    RegistryCommand, RuntimeCommand,
};
use supermcp::config::ConfigManager;
use supermcp::core::ServerManager;
use supermcp::http_server::HttpServer;
use std::sync::Arc;
use tracing::{error, info};

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
                    if let Err(e) = supermcp::cli::mcp::add(&args.config, &name, &command, cmd_args, env, tags, description).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                McpCommand::List => {
                    if let Err(e) = supermcp::cli::mcp::list(&args.config).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                McpCommand::Remove { name } => {
                    if let Err(e) = supermcp::cli::mcp::remove(&args.config, &name).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                McpCommand::Status { name } => {
                    if let Err(e) = supermcp::cli::mcp::status(&args.config, name.as_deref()).await {
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
                    if let Err(e) = supermcp::cli::mcp::edit(&args.config, &name, command.as_deref(), cmd_args, env, tags, description).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
        Cli::Preset(args) => {
            match args.command {
                PresetCommand::Create { name, tags, description } => {
                    if let Err(e) = supermcp::cli::preset::create(&args.config, &name, tags, description).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                PresetCommand::List => {
                    if let Err(e) = supermcp::cli::preset::list(&args.config).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                PresetCommand::Edit { name, tags, description } => {
                    if let Err(e) = supermcp::cli::preset::edit(&args.config, &name, tags, description).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                PresetCommand::Test { name } => {
                    if let Err(e) = supermcp::cli::preset::test(&args.config, &name).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                PresetCommand::Remove { name } => {
                    if let Err(e) = supermcp::cli::preset::remove(&args.config, &name).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
        Cli::Registry(args) => {
            match args.command {
                RegistryCommand::Search { query } => {
                    if let Err(e) = supermcp::cli::registry::search(&args.config, &query).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                RegistryCommand::Install { name } => {
                    if let Err(e) = supermcp::cli::registry::install(&args.config, &name).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                RegistryCommand::Info { name } => {
                    if let Err(e) = supermcp::cli::registry::info(&args.config, &name).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                RegistryCommand::Refresh => {
                    if let Err(e) = supermcp::cli::registry::refresh(&args.config).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
        Cli::Install(args) => {
            if let Err(e) = supermcp::cli::install::install(
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
                    if let Err(e) = supermcp::cli::runtime::add(
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
                    if let Err(e) = supermcp::cli::runtime::list(&args.config, json).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                RuntimeCommand::Remove { name } => {
                    if let Err(e) = supermcp::cli::runtime::remove(&args.config, &name).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                RuntimeCommand::Info { name } => {
                    if let Err(e) = supermcp::cli::runtime::info(&args.config, &name).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                RuntimeCommand::Validate => {
                    if let Err(e) = supermcp::cli::runtime::validate(&args.config).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
                RuntimeCommand::Exec { runtime, script, file } => {
                    if script.is_none() && file.is_none() {
                        eprintln!("Error: Either --script or --file must be provided");
                        std::process::exit(1);
                    }
                    if let Err(e) = supermcp::cli::runtime::execute(&args.config, &runtime, script.as_deref(), file).await {
                        eprintln!("Error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
        Cli::Call(args) => {
            if let Err(e) = supermcp::cli::call::execute(
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
            if let Err(e) = supermcp::cli::call::list_tools(
                args.config.as_deref(),
                args.provider.as_deref(),
                args.stdio.as_deref(),
                args.http_url.as_deref(),
                args.skill.as_deref(),
                args.schema,
                args.json,
                args.all,
            ).await {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        Cli::Providers(args) => {
            if let Err(e) = supermcp::cli::call::list_providers(
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
            eprintln!("  - {}", issue);
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
                println!("  - {}", note);
            }
        }
    } else if let Some(output_path) = output {
        let expanded_path = shellexpand::tilde(output_path).to_string();

        // Ensure directory exists
        if let Some(parent) = std::path::Path::new(&expanded_path).parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(&expanded_path, output_content).await?;
        println!("Configuration migrated successfully:");
        println!("  Input:  {}", input_path);
        println!("  Output: {}", expanded_path);

        if !notes.is_empty() {
            println!("\n=== Migration Notes ===");
            for note in notes {
                println!("  - {}", note);
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
                    println!("Configuration is valid: {}", path);
                }
                Err(errors) => {
                    println!("Configuration is invalid: {}", path);
                    println!("\nErrors:");
                    for error in errors {
                        println!("  - {}", error);
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
        println!("\nImporting from {:?}\n", args.source);

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
            println!("\nDry run - {} server(s) would be imported", imported.len());
            println!("  Run without --dry-run to save to config.");
        } else {
            println!("\nSuccessfully imported {} server(s) to {}", imported.len(), args.config);
        }
    }

    Ok(())
}
