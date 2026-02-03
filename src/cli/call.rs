//! Direct MCP tool invocation - lightweight client functionality
//!
//! This module provides MCPorter-like functionality for calling MCP tools
//! and skills directly without running a proxy server.

use crate::cli::expand_path;
use crate::cli::skill_provider::SkillProvider;
use crate::config::{Config, McpServerConfig, SandboxConfig};
// Note: JsonRpcRequest is used internally by McpProvider
use crate::core::provider::{McpProvider, Provider, ProviderRegistry, ProviderType, Tool, ToolResult};
use crate::core::server::{ManagedServer, TransportType};
use crate::utils::errors::{McpError, McpResult};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info};

/// Parse arguments in shell-friendly format: key:value or key=value
pub fn parse_call_args(args: &[String]) -> McpResult<Value> {
    let mut map = serde_json::Map::new();

    for arg in args {
        // Try key:value or key=value syntax
        let parts: Vec<&str> = if arg.contains(':') {
            arg.splitn(2, ':').collect()
        } else if arg.contains('=') {
            arg.splitn(2, '=').collect()
        } else {
            // Bare argument - treat as positional or flag
            map.insert(arg.clone(), Value::Bool(true));
            continue;
        };

        if parts.len() == 2 {
            let key = parts[0].trim();
            let value = parts[1].trim();

            // Try to parse as JSON, fallback to string
            let parsed_value = if let Ok(json_val) = serde_json::from_str::<Value>(value) {
                json_val
            } else {
                Value::String(value.to_string())
            };

            map.insert(key.to_string(), parsed_value);
        }
    }

    Ok(Value::Object(map))
}

/// Parse function-call style arguments: toolName(key: value, key2: value)
pub fn parse_function_style(input: &str) -> McpResult<(String, Value)> {
    // Find the opening parenthesis
    let paren_idx = input.find('(').ok_or_else(|| {
        McpError::InvalidRequest(
            "Function-style syntax requires parentheses: toolName(key:value)".to_string(),
        )
    })?;

    let tool_name = input[..paren_idx].trim().to_string();
    let args_str = &input[paren_idx + 1..];

    // Find the matching closing parenthesis
    let close_idx = args_str.rfind(')').ok_or_else(|| {
        McpError::InvalidRequest("Missing closing parenthesis".to_string())
    })?;

    let args_content = &args_str[..close_idx];

    // Parse key: value or key=value pairs
    let mut map = serde_json::Map::new();

    if !args_content.trim().is_empty() {
        // Simple parser for comma-separated key:value pairs
        // This is a basic implementation - quotes and nested structures need JSON syntax
        for pair in args_content.split(',') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }

            let parts: Vec<&str> = if pair.contains(':') {
                pair.splitn(2, ':').collect()
            } else if pair.contains('=') {
                pair.splitn(2, '=').collect()
            } else {
                continue;
            };

            if parts.len() == 2 {
                let key = parts[0].trim().trim_matches('"').trim_matches('\'');
                let value = parts[1].trim();

                let parsed_value = if value.starts_with('"') || value.starts_with('\'') {
                    // String literal
                    Value::String(value.trim_matches('"').trim_matches('\'').to_string())
                } else if let Ok(n) = value.parse::<i64>() {
                    Value::Number(n.into())
                } else if let Ok(b) = value.parse::<bool>() {
                    Value::Bool(b)
                } else if value == "null" {
                    Value::Null
                } else {
                    // Try JSON parsing
                    serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()))
                };

                map.insert(key.to_string(), parsed_value);
            }
        }
    }

    Ok((tool_name, Value::Object(map)))
}

/// Execute a direct tool call
pub async fn execute(
    config_path: Option<&str>,
    target: &str,
    args: Vec<String>,
    stdio_cmd: Option<&str>,
    http_url: Option<&str>,
    skill_name: Option<&str>,
    _env_vars: Vec<String>,
    json_output: bool,
) -> McpResult<()> {
    // Build provider registry
    let registry = build_registry(config_path, stdio_cmd, http_url, skill_name).await?;

    // Parse the tool name and arguments
    let (tool_name, params) = if target.contains('(') {
        // Function-style: toolName(args...)
        parse_function_style(target)?
    } else if target.contains('.') {
        // Server.tool format
        let parts: Vec<&str> = target.splitn(2, '.').collect();
        let tool_name = parts[1].to_string();
        let params = parse_call_args(&args)?;
        (tool_name, params)
    } else {
        // Just the tool name, args are separate
        let tool_name = target.to_string();
        let params = parse_call_args(&args)?;
        (tool_name, params)
    };

    // Find the provider and tool
    let (provider_name, tool_name) = if tool_name.contains('.') {
        let parts: Vec<&str> = tool_name.splitn(2, '.').collect();
        (parts[0].to_string(), parts[1].to_string())
    } else {
        // Try to find the tool in any provider
        let all_tools = registry.list_all_tools().await?;
        let matching = all_tools
            .into_iter()
            .filter(|t| t.display_name() == tool_name || t.snake_name() == tool_name)
            .collect::<Vec<_>>();

        if matching.is_empty() {
            return Err(McpError::ToolExecutionError(format!(
                "Tool '{}' not found in any provider",
                tool_name
            )));
        }

        if matching.len() > 1 {
            let providers: Vec<_> = matching.iter().map(|t| t.provider.clone()).collect();
            return Err(McpError::InvalidRequest(format!(
                "Ambiguous tool name '{}'. Found in providers: {}. Use provider.tool_name format.",
                tool_name,
                providers.join(", ")
            )));
        }

        let tool = &matching[0];
        (tool.provider.clone(), tool.display_name().to_string())
    };

    // Convert snake_case to kebab-case for MCP compatibility
    let tool_name_kebab = tool_name.replace('_', "-");
    let full_tool_name = format!("{}.{}", provider_name, tool_name_kebab);

    info!("Calling tool: {} with params: {}", full_tool_name, params);

    // Find the provider and call the tool
    let provider = registry
        .get(&provider_name)
        .ok_or_else(|| McpError::ServerNotFound(provider_name.clone()))?;

    let result = provider.call_tool(&full_tool_name, params).await?;

    // Handle the result
    if !result.success {
        if json_output {
            let output = serde_json::json!({
                "success": false,
                "error": result.error,
            });
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        } else {
            eprintln!("Error: {}", result.error.as_deref().unwrap_or_default());
        }
        return Err(McpError::ToolExecutionError(
            result.error.unwrap_or_default(),
        ));
    }

    // Print the result
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "success": true,
                "data": result.data,
                "content": result.content,
            }))
            .unwrap_or_default()
        );
    } else {
        print_tool_result(&result);
    }

    Ok(())
}

/// Build the provider registry from all sources
pub async fn build_registry(
    config_path: Option<&str>,
    stdio_cmd: Option<&str>,
    http_url: Option<&str>,
    skill_name: Option<&str>,
) -> McpResult<ProviderRegistry> {
    let registry = ProviderRegistry::new();

    // Add MCP servers from config
    if let Ok(config) = load_config(config_path).await {
        for server_config in config.servers {
            let name = server_config.name.clone();
            match ManagedServer::new(server_config).await {
                Ok(server) => {
                    let provider = McpProvider::new(name.clone(), ProviderType::McpStdio, server);
                    registry.register(Box::new(provider));
                    debug!("Registered MCP provider: {}", name);
                }
                Err(e) => {
                    tracing::warn!("Failed to connect to MCP server {}: {}", name, e);
                }
            }
        }
    }

    // Add ad-hoc stdio server if specified
    if let Some(cmd) = stdio_cmd {
        let server = create_adhoc_stdio_server(cmd, vec![]).await?;
        let provider = McpProvider::new("adhoc-stdio".to_string(), ProviderType::McpStdio, server);
        registry.register(Box::new(provider));
    }

    // Add ad-hoc HTTP server if specified
    if let Some(url) = http_url {
        let server = create_adhoc_http_server(url).await?;
        let provider_type = if url.contains("/sse") {
            ProviderType::McpSse
        } else {
            ProviderType::McpHttp
        };
        let provider_name = if url.contains("/sse") {
            "adhoc-sse".to_string()
        } else {
            "adhoc-http".to_string()
        };
        let provider = McpProvider::new(provider_name, provider_type, server);
        registry.register(Box::new(provider));
    }

    // Add skill if specified
    if let Some(name) = skill_name {
        if let Some(provider) = load_skill_provider(name).await? {
            registry.register(provider);
        }
    }

    // Auto-discover and load skills
    if let Ok(skills) = discover_skills().await {
        for skill in skills {
            registry.register(skill);
        }
    }

    Ok(registry)
}

/// Create an ad-hoc stdio server from a command string
async fn create_adhoc_stdio_server(cmd: &str, env_vars: Vec<String>) -> McpResult<ManagedServer> {
    // Parse the command
    let parts = shell_words::split(cmd)
        .map_err(|e| McpError::ConfigError(format!("Failed to parse command: {}", e)))?;

    if parts.is_empty() {
        return Err(McpError::ConfigError("Empty command".to_string()));
    }

    let command = parts[0].clone();
    let args = parts[1..].to_vec();

    // Parse environment variables
    let env = parse_env_vars(&env_vars)?;

    let config = McpServerConfig {
        name: "adhoc".to_string(),
        command,
        args,
        env,
        tags: vec!["adhoc".to_string()],
        description: Some("Ad-hoc stdio connection".to_string()),
        sandbox: SandboxConfig::default(),
    };

    ManagedServer::new(config).await
}

/// Create an ad-hoc HTTP/SSE server
async fn create_adhoc_http_server(url: &str) -> McpResult<ManagedServer> {
    // Determine transport type from URL
    let transport_type = if url.starts_with("http://") || url.starts_with("https://") {
        // Try to detect if it's SSE or regular HTTP
        if url.contains("/sse") || url.contains("stream") {
            TransportType::Sse
        } else {
            TransportType::StreamableHttp
        }
    } else {
        return Err(McpError::ConfigError(format!(
            "Invalid URL scheme: {}. Use http:// or https://",
            url
        )));
    };

    let config = McpServerConfig {
        name: "adhoc".to_string(),
        command: String::new(),
        args: vec![],
        env: HashMap::new(),
        tags: vec!["adhoc".to_string()],
        description: Some(format!("Ad-hoc HTTP connection: {}", url)),
        sandbox: SandboxConfig::default(),
    };

    ManagedServer::with_transport(config, transport_type, Some(url.to_string())).await
}

/// Load a skill provider by name
async fn load_skill_provider(name: &str) -> McpResult<Option<Box<dyn crate::core::provider::Provider>>> {
    let skill_paths = vec![
        dirs::config_dir()
            .map(|d| d.join(format!("agents/skills/{}", name))),
        dirs::home_dir()
            .map(|d| d.join(format!(".config/agents/skills/{}", name))),
        Some(PathBuf::from(format!("./skills/{}", name))),
    ];

    for path in skill_paths.into_iter().flatten() {
        let skill_file = path.join("SKILL.md");
        if skill_file.exists() {
            debug!("Found skill: {} at {:?}", name, path);
            match SkillProvider::new(name, path).await {
                Ok(provider) => return Ok(Some(Box::new(provider))),
                Err(e) => {
                    tracing::warn!("Failed to load skill {}: {}", name, e);
                    return Ok(None);
                }
            }
        }
    }

    Ok(None)
}

/// Discover all available skills
async fn discover_skills() -> McpResult<Vec<Box<dyn crate::core::provider::Provider>>> {
    let mut providers = Vec::new();

    let skill_dirs = vec![
        dirs::config_dir().map(|d| d.join("agents/skills")),
        dirs::home_dir().map(|d| d.join(".config/agents/skills")),
        Some(PathBuf::from("./skills")),
    ];

    for dir in skill_dirs.into_iter().flatten() {
        if !dir.exists() {
            continue;
        }

        let mut entries = tokio::fs::read_dir(&dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                let skill_file = path.join("SKILL.md");
                if skill_file.exists() {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string();
                    debug!("Discovered skill: {}", name);
                    match SkillProvider::new(&name, path).await {
                        Ok(provider) => providers.push(Box::new(provider) as Box<dyn Provider>),
                        Err(e) => tracing::warn!("Failed to load skill {}: {}", name, e),
                    }
                }
            }
        }
    }

    Ok(providers)
}

/// List tools from all providers
pub async fn list_tools(
    config_path: Option<&str>,
    provider_filter: Option<&str>,
    stdio_cmd: Option<&str>,
    http_url: Option<&str>,
    skill_name: Option<&str>,
    show_schema: bool,
    json_output: bool,
    all: bool,
) -> McpResult<()> {
    let registry = build_registry(config_path, stdio_cmd, http_url, skill_name).await?;

    let all_tools;

    if let Some(filter) = provider_filter {
        // List tools from specific provider
        if let Some(provider) = registry.get(filter) {
            all_tools = provider.list_tools().await?;
        } else {
            return Err(McpError::ServerNotFound(filter.to_string()));
        }
    } else if all {
        // List from all providers
        all_tools = registry.list_all_tools().await?;
    } else {
        // List from all providers by default
        all_tools = registry.list_all_tools().await?;
    }

    // Group by provider
    let by_provider: HashMap<String, Vec<Tool>> = {
        let mut map: HashMap<String, Vec<Tool>> = HashMap::new();
        for tool in all_tools {
            map.entry(tool.provider.clone()).or_default().push(tool);
        }
        map
    };

    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "providers": by_provider.keys().collect::<Vec<_>>(),
                "tools": by_provider.values().flatten().collect::<Vec<_>>(),
            }))
            .unwrap()
        );
    } else {
        // Print by provider
        for (provider, tools) in &by_provider {
            println!("\n📦 {} ({} tools)", provider, tools.len());
            println!("{}", "─".repeat(50));

            for tool in tools {
                print_tool(&tool, show_schema);
            }
        }

        let total: usize = by_provider.values().map(|v| v.len()).sum();
        let provider_count = by_provider.len();
        println!("\n\nTotal: {} tools from {} providers", total, provider_count);
    }

    Ok(())
}

/// Print a tool in a readable format
fn print_tool(tool: &Tool, show_schema: bool) {
    let display_name = tool.snake_name();
    let provider_type = format!("[{}]", tool.provider_type);

    println!(
        "  {} {} - {}",
        display_name,
        provider_type,
        tool.description.as_deref().unwrap_or("No description")
    );

    if show_schema && !tool.parameters.is_empty() {
        println!("    Parameters:");
        for param in &tool.parameters {
            let req = if param.required { "*" } else { "?" };
            let desc = param.description.as_deref().unwrap_or("");
            println!(
                "      {} {}: {} - {}",
                req, param.name, param.param_type, desc
            );
        }
    }
}

/// Print tool result in a readable format
fn print_tool_result(result: &ToolResult) {
    // Handle content array format
    if let Some(content) = &result.content {
        for item in content {
            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                println!("{}", text);
            } else if let Some(data) = item.get("data") {
                println!("{}", serde_json::to_string_pretty(data).unwrap_or_default());
            } else {
                println!("{}", serde_json::to_string_pretty(item).unwrap_or_default());
            }
        }
    } else if let Some(data) = &result.data {
        // Direct result
        println!("{}", serde_json::to_string_pretty(data).unwrap_or_default());
    }
}

/// List all available providers (MCPs and skills)
pub async fn list_providers(
    config_path: Option<&str>,
    json_output: bool,
) -> McpResult<()> {
    let registry = build_registry(config_path, None, None, None).await?;
    
    let providers: Vec<_> = registry
        .list()
        .into_iter()
        .map(|name| {
            let provider = registry.get(&name).unwrap();
            serde_json::json!({
                "name": name,
                "type": provider.provider_type().to_string(),
                "available": "unknown", // Would need async call here
            })
        })
        .collect();

    if json_output {
        println!("{}", serde_json::to_string_pretty(&providers).unwrap());
    } else {
        println!("\n📦 Available Providers:\n");
        for p in &providers {
            println!(
                "  {} [{}]",
                p["name"].as_str().unwrap_or("unknown"),
                p["type"].as_str().unwrap_or("unknown")
            );
        }
        println!("\nTotal: {} providers", providers.len());
    }

    Ok(())
}

/// Load configuration from file
async fn load_config(config_path: Option<&str>) -> McpResult<Config> {
    let path = config_path
        .map(|p| PathBuf::from(expand_path(p)))
        .or_else(|| dirs::config_dir().map(|p| p.join("supermcp/config.toml")))
        .unwrap_or_else(|| PathBuf::from("~/.config/supermcp/config.toml"));

    if !path.exists() {
        return Ok(Config::default());
    }

    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| McpError::ConfigError(format!("Failed to read config: {}", e)))?;

    toml::from_str(&content)
        .map_err(|e| McpError::ConfigError(format!("Failed to parse config: {}", e)))
}

/// Parse environment variables in KEY=value format
fn parse_env_vars(env_vars: &[String]) -> McpResult<HashMap<String, String>> {
    let mut map = HashMap::new();
    for var in env_vars {
        let parts: Vec<&str> = var.splitn(2, '=').collect();
        if parts.len() != 2 {
            return Err(McpError::ConfigError(format!(
                "Invalid environment variable format: {}. Use KEY=value",
                var
            )));
        }
        map.insert(parts[0].to_string(), parts[1].to_string());
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_call_args() {
        let args = vec![
            "key1:value1".to_string(),
            "key2=value2".to_string(),
            "number:42".to_string(),
            "json:{\"nested\":\"value\"}".to_string(),
        ];

        let result = parse_call_args(&args).unwrap();
        assert_eq!(result["key1"], "value1");
        assert_eq!(result["key2"], "value2");
        assert_eq!(result["number"], 42);
        assert_eq!(result["json"]["nested"], "value");
    }

    #[test]
    fn test_parse_function_style() {
        let input = "server.tool_name(key1: value1, key2: 42)";
        let (name, params) = parse_function_style(input).unwrap();
        assert_eq!(name, "server.tool_name");
        assert_eq!(params["key1"], "value1");
        assert_eq!(params["key2"], 42);
    }

    #[test]
    fn test_parse_function_style_quotes() {
        let input = r#"search(query: "hello world", limit: 10)"#;
        let (name, params) = parse_function_style(input).unwrap();
        assert_eq!(name, "search");
        assert_eq!(params["query"], "hello world");
        assert_eq!(params["limit"], 10);
    }
}
