# Super MCP

A secure, high-performance Model Context Protocol (MCP) server proxy with advanced sandboxing, written in Rust.

## Features

- **Security First**: Each MCP server runs in a sandboxed environment with platform-native isolation
- **Hot Reload**: Configuration changes are applied without restarting
- **Multiple Transports**: Supports stdio, SSE, HTTP, and Streamable HTTP
- **Tag-Based Access Control**: Control which servers clients can access
- **Rate Limiting**: Built-in protection against abuse
- **Audit Logging**: Comprehensive security event logging

## Quick Start

### Installation

```bash
cargo install super-mcp
```

### Configuration

Create a configuration file at `~/.config/super-mcp/config.toml`:

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

### Running

```bash
supermcp serve
```

## Architecture

Super MCP uses a layered architecture:

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
