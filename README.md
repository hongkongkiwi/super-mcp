# SuperMCP

A secure, high-performance Model Context Protocol (MCP) server proxy with advanced sandboxing, written in Rust.

## Features

- **Security First**: Each MCP server runs in a sandboxed environment with platform-native isolation
- **Hot Reload**: Configuration changes are applied without restarting
- **Multiple Transports**: Supports stdio, SSE, HTTP, and Streamable HTTP
- **Tag-Based Access Control**: Control which servers clients can access
- **Rate Limiting**: Built-in protection against abuse
- **Audit Logging**: Comprehensive security event logging
- **Lightweight Client**: Direct MCP tool invocation without running a server
- **Unified Provider Architecture**: Support MCPs, skills, and custom providers through one interface

## Quick Start

### Installation

```bash
cargo install supermcp
```

### Configuration

Create a configuration file at `~/.config/supermcp/config.toml`:

```toml
[server]
host = "127.0.0.1"
port = 3000

[[servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/docs"]
tags = ["filesystem"]

[servers.sandbox]
network = false
filesystem = "readonly"
```

### Running as a Server

```bash
supermcp serve
```

### Using the Lightweight Client

Call MCP tools directly without running a server:

```bash
# List tools from a configured server
supermcp tools filesystem

# Call a tool using shell-friendly syntax
supermcp call filesystem.list_directory path:/home/user/docs

# Use function-style syntax
supermcp call "filesystem.read_file(path: /home/user/docs/readme.md)"

# Ad-hoc connections without configuration
supermcp call --stdio "npx -y @modelcontextprotocol/server-filesystem /tmp" list_directory path:/tmp

# List tools from ad-hoc server
supermcp tools --stdio "npx -y @modelcontextprotocol/server-filesystem /tmp" --schema
```

## Unified Provider Architecture

SuperMCP supports multiple types of tool providers through a unified interface:

| Provider Type | Description | Example |
|--------------|-------------|---------|
| **MCP Stdio** | Local MCP servers via stdio | `@modelcontextprotocol/server-filesystem` |
| **MCP SSE** | MCP servers via Server-Sent Events | `https://mcp.example.com/sse` |
| **MCP HTTP** | MCP servers via Streamable HTTP | `https://mcp.example.com/mcp` |
| **Skills** | Kimi CLI skills | Custom skill directories |

### Import from AI Editors

SuperMCP can import MCP server configurations from popular AI editors and tools:

```bash
# Import from all supported sources
supermcp import all

# Import from specific editor
supermcp import cursor
supermcp import claude
supermcp import vscode
supermcp import codex
supermcp import kimi-cli
supermcp import windsurf
supermcp import opencode
supermcp import gemini
supermcp import qwen
supermcp import github-copilot

# Dry run - see what would be imported
supermcp import all --dry-run

# JSON output for scripting
supermcp import all --json
```

**Supported Sources:**

| Source | Config Location |
|--------|----------------|
| **Cursor** | `~/.cursor/mcp.json` |
| **Claude Desktop** | `~/Library/Application Support/Claude/claude_desktop_config.json` |
| **VS Code** (Cline, Kilo, Roo, Continue) | VS Code settings.json |
| **Codex** (OpenAI) | `~/.codex/config.json` |
| **Kimi CLI** | `~/.kimi/config.json` |
| **Windsurf** (Codeium) | `~/.windsurf/mcp.json` |
| **OpenCode** | `~/.opencode/config.json` |
| **Gemini** (Google) | `~/.gemini/config.json` |
| **Qwen** (Alibaba) | `~/.qwen/config.json` |
| **GitHub Copilot** | `~/.github/copilot/mcp.json` (future) |

### Listing All Providers

```bash
# List all available providers (MCPs and skills)
supermcp providers

# JSON output for scripting
supermcp providers --json
```

### Tool Discovery (All Providers)

```bash
# List tools from all providers
supermcp tools --all

# List tools from a specific provider
supermcp tools filesystem
supermcp tools my-skill

# Show with schema details
supermcp tools filesystem --schema

# JSON output
supermcp tools --all --json
```

## Lightweight Client (MCPorter-style)

SuperMCP includes a lightweight client for direct tool invocation without running a proxy server. This is inspired by [MCPorter](https://github.com/steipete/mcporter) but integrated directly into SuperMCP.

### Direct Tool Calls

```bash
# Using configured providers
supermcp call server.tool_name param1:value1 param2:value2
supermcp call filesystem.list_directory path:/tmp
supermcp call my-skill.do_something input:hello

# Auto-discovery - tool name only (searches all providers)
supermcp call list_directory path:/tmp

# Function-style syntax (useful for complex arguments)
supermcp call "filesystem.read_file(path: /tmp/test.txt)"

# With JSON output
supermcp call filesystem.list_directory path:/tmp --json
```

### Ad-hoc Connections

Connect to any MCP server without pre-configuration:

```bash
# Stdio transport
supermcp call --stdio "npx -y @modelcontextprotocol/server-filesystem /tmp" list_directory path:/tmp

# With environment variables
supermcp call --stdio "npx -y some-mcp-server" --env API_KEY=secret get_data id:123

# HTTP/SSE transport
supermcp call --http-url https://mcp.example.com/sse get_status
```

### Using Skills

```bash
# Call a skill directly
supermcp call --skill my-skill do_something input:hello

# List tools from a skill
supermcp tools --skill my-skill
```

### Argument Syntax

The `call` command supports flexible argument formats:

- **Colon format**: `key:value` (shell-friendly, no quotes needed)
- **Equals format**: `key=value` (traditional style)
- **JSON values**: `config:{"nested":"value"}`
- **Numbers and booleans**: `count:42 enabled:true`

## Architecture

SuperMCP uses a layered architecture:

- **Security Layer**: Platform-native sandboxing (seccomp/Landlock on Linux, seatbelt on macOS)
- **Core Layer**: Server lifecycle management, capability handling
- **Transport Layer**: stdio, SSE, HTTP, Streamable HTTP support
- **Application Layer**: CLI, configuration management, audit logging

## Development

### Building

```bash
cargo build --release
```

### Testing

```bash
cargo test
```

## License

MIT OR Apache-2.0
