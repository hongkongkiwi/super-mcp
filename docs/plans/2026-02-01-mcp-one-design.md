# MCP-One Design Document

**Date:** 2026-02-01
**Status:** Approved
**Version:** 1.0

## Overview

MCP-One is a unified Model Context Protocol (MCP) server implementation written in Rust. It acts as a secure proxy/multiplexer that aggregates multiple MCP servers into a single, unified interface with enterprise-grade security through sandboxing.

### Goals

1. **Security First**: Each MCP server runs in a sandboxed environment with platform-native isolation
2. **Performance**: Sub-50ms latency, 10,000+ concurrent connections
3. **Compatibility**: Full MCP protocol compliance with all transport types
4. **Ergonomics**: Hot-reload configuration, intuitive CLI, comprehensive logging

### Non-Goals

- Replacing 1MCP entirely (initial scope is core proxy functionality)
- GUI management interface (CLI-first approach)
- Cloud-hosted service (self-hosted only)

## Architecture

### High-Level Structure

```
┌─────────────────────────────────────────────────────────────┐
│                    Security Layer                            │
│  ┌──────────────┐ ┌──────────────┐ ┌─────────────────────┐ │
│  │   Sandbox    │ │   Seccomp    │ │   Resource          │ │
│  │   Manager    │ │   Policies   │ │   Limits            │ │
│  │  (trait)     │ │  (per-server)│ │                     │ │
│  └──────────────┘ └──────────────┘ └─────────────────────┘ │
│  ┌──────────────┐ ┌──────────────┐ ┌─────────────────────┐ │
│  │  Network     │ │   Filesystem │ │   Capability        │ │
│  │  Isolation   │ │   Landlock   │ │   Dropping          │ │
│  │  (netns)     │ │  (read-only) │ │  (setuid/prctl)     │ │
│  └──────────────┘ └──────────────┘ └─────────────────────┘ │
├─────────────────────────────────────────────────────────────┤
│                    Application Layer                         │
│  ┌──────────────┐ ┌──────────────┐ ┌─────────────────────┐ │
│  │    CLI       │ │  Config      │ │   Audit Logger      │ │
│  │  (clap)      │ │  Manager     │ │   (structured)      │ │
│  └──────────────┘ └──────────────┘ └─────────────────────┘ │
├─────────────────────────────────────────────────────────────┤
│                    Core Domain Layer                         │
│  ┌──────────────┐ ┌──────────────┐ ┌─────────────────────┐ │
│  │   Server     │ │   Client     │ │   Capability        │ │
│  │   Manager    │ │   Manager    │ │   Manager           │ │
│  │ (sandboxed)  │ │  (pooled)    │ │  (async loading)    │ │
│  └──────────────┘ └──────────────┘ └─────────────────────┘ │
│  ┌──────────────┐ ┌──────────────┐ ┌─────────────────────┐ │
│  │   Preset     │ │   Registry   │ │   Scope             │ │
│  │   Manager    │ │   Client     │ │   Validator         │ │
│  └──────────────┘ └──────────────┘ └─────────────────────┘ │
├─────────────────────────────────────────────────────────────┤
│                  Transport Layer                             │
│  ┌──────────────┐ ┌──────────────┐ ┌─────────────────────┐ │
│  │ HTTP Server  │ │    SSE       │ │  Streamable HTTP    │ │
│  │  (axum)      │ │  Transport   │ │    Transport        │ │
│  └──────────────┘ └──────────────┘ └─────────────────────┘ │
│  ┌──────────────┐ ┌──────────────┐ ┌─────────────────────┐ │
│  │ Stdio Trans  │ │   Stdio      │ │   Rate Limiter      │ │
│  │  (sandboxed) │ │   Proxy      │ │   (tower-governor)  │ │
│  └──────────────┘ └──────────────┘ └─────────────────────┘ │
├─────────────────────────────────────────────────────────────┤
│                  Infrastructure Layer                        │
│  ┌──────────────┐ ┌──────────────┐ ┌─────────────────────┐ │
│  │  Auth        │ │   Config     │ │   Event Bus         │ │
│  │  Provider    │ │  Repository  │ │  (tokio::sync)      │ │
│  │  (pluggable) │ │  (hot-reload)│ │                     │ │
│  └──────────────┘ └──────────────┘ └─────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

### Technology Stack

| Component | Technology | Rationale |
|-----------|------------|-----------|
| HTTP Framework | Axum | Tower ecosystem, middleware support, SSE |
| Async Runtime | Tokio | Industry standard, work-stealing scheduler |
| Serialization | serde + json | Standard, fast, well-supported |
| Configuration | toml + figment | Hot-reload, env substitution |
| Sandboxing | Custom traits + seccomp/Landlock/seatbelt | Platform-native security |
| CLI | clap | Derive macros, completions, man pages |
| Auth | oauth2 + jsonwebtoken | Standards-compliant |
| Rate Limiting | tower-governor | Axum integration |
| File Watching | notify | Cross-platform, efficient |

## Security Architecture

### Sandboxing Strategy

**Pluggable with Platform Defaults:**

```rust
#[async_trait]
pub trait Sandbox: Send + Sync {
    /// Spawn a process with sandbox constraints applied
    async fn spawn(&self, config: &ServerConfig) -> Result<Child, SandboxError>;

    /// Return the constraints this sandbox enforces
    fn constraints(&self) -> &SandboxConstraints;
}
```

**Default Implementations:**

1. **Linux**: seccomp-bpf + namespaces (pid, net, mount, ipc) + Landlock
2. **macOS**: seatbelt (sandbox profiles)
3. **Windows**: Job Objects + AppContainer + ACLs
4. **Container**: Docker/Podman (fallback for all platforms)

**Per-Server Configuration:**

```yaml
servers:
  filesystem:
    command: npx -y @modelcontextprotocol/server-filesystem
    sandbox:
      type: default  # Platform native
      network: false
      filesystem: readonly  # Options: full, readonly, ["/allowed/path"]
      env_inherit: false
      max_memory_mb: 512
      max_cpu_percent: 50
```

### Additional Security Layers

1. **Rate Limiting**: Per-IP, per-user, per-server with tower-governor
2. **Request/Response Limits**: Size limits, timeout enforcement
3. **Security Headers**: CSP, HSTS, X-Frame-Options, etc.
4. **Audit Logging**: Structured JSON logs for all security events
5. **Token Management**: Short-lived sessions (configurable TTL), refresh tokens
6. **Scope Validation**: Tag-based access control (e.g., `tag:filesystem` grants access to filesystem-tagged servers)

### Sandboxing Implementation Details

**Linux (Primary Target):**
```rust
pub struct LinuxSandbox {
    seccomp_filter: SeccompFilter,
    namespace_flags: CloneFlags,
    landlock_rules: Vec<LandlockRule>,
    cgroup_limits: ResourceLimits,
}

impl Sandbox for LinuxSandbox {
    async fn spawn(&self, config: &ServerConfig) -> Result<Child, SandboxError> {
        // 1. Create namespaces before fork
        // 2. Fork process
        // 3. In child:
        //    - Mount tmpfs as root (pivot_root)
        //    - Drop capabilities (capset)
        //    - Apply seccomp filter
        //    - Apply Landlock rules
        //    - Move to cgroup
        //    - execve()
        // 4. In parent: return Child handle
    }
}
```

**macOS:**
- Use sandbox profiles via `sandbox_init()`
- Optional: seatbelt extensions for more control

**Windows:**
- Create AppContainer with capabilities
- Assign process to Job Object with limits
- Apply ACLs to restrict filesystem/registry access

## Data Flow

### Client Request Flow

```
1. Client → HTTP/SSE request with Bearer token
   ↓
2. Auth middleware validates token (pluggable provider)
   ↓
3. Scope validator checks if client can access requested server/tags
   ↓
4. Request router forwards to appropriate ServerManager
   ↓
5. ServerManager checks if MCP server process is running
   ├─ Yes → Reuse existing connection (pooled)
   └─ No → Spawn via Sandbox with configured policy
   ↓
6. MCP request serialized → sent to stdio/SSE/HTTP transport
   ↓
7. Response received → Capability filtering applied
   ↓
8. Response returned to client
```

### Configuration Hot-Reload Flow

```
1. File watcher detects config change (notify crate)
   ↓
2. Config parsed and validated (serde + validator)
   ↓
3. Diff calculated (added/removed/modified servers)
   ↓
4. Event bus broadcasts changes
   ├─ ServerManager: Start/stop/restart affected servers
   ├─ PresetManager: Update available presets
   └─ ClientManager: Notify connected clients of changes
   ↓
5. Audit log records configuration change
```

### Registry Integration Flow

```
1. User searches: `mcpo registry search filesystem`
   ↓
2. HTTP client queries registry API with caching
   ↓
3. Results displayed with install command
   ↓
4. On install: Download schema, validate, add to config
   ↓
5. Server installed (npm/pip/cargo depending on package)
```

## Component Design

### Core Traits

```rust
// Pluggable authentication
#[async_trait]
pub trait AuthProvider: Send + Sync {
    async fn validate_token(&self, token: &str) -> Result<Session, AuthError>;
    async fn refresh_token(&self, refresh_token: &str) -> Result<Tokens, AuthError>;
    async fn generate_token(&self, claims: Claims) -> Result<Tokens, AuthError>;
}

// Sandboxing abstraction
#[async_trait]
pub trait Sandbox: Send + Sync {
    async fn spawn(&self, config: &ServerConfig) -> Result<Child, SandboxError>;
    fn constraints(&self) -> &SandboxConstraints;
}

// Transport abstraction
#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, message: JsonRpcMessage) -> Result<JsonRpcMessage, TransportError>;
    async fn close(&self) -> Result<(), TransportError>;
    fn is_connected(&self) -> bool;
}

// Storage for auth data
#[async_trait]
pub trait AuthStorage: Send + Sync {
    async fn store_session(&self, session: &Session) -> Result<(), StorageError>;
    async fn get_session(&self, id: &str) -> Option<Session>;
    async fn delete_session(&self, id: &str) -> Result<(), StorageError>;
}
```

### Key Structs

```rust
/// Manages lifecycle of MCP server processes
pub struct ServerManager {
    servers: DashMap<String, ManagedServer>,
    sandbox: Arc<dyn Sandbox>,
    event_bus: broadcast::Sender<ServerEvent>,
}

/// Connection pooling and reuse to downstream servers
pub struct ClientManager {
    connections: DashMap<String, PooledConnection>,
    config: ClientConfig,
}

/// Manages async loading of MCP capabilities
pub struct CapabilityManager {
    cache: Arc<DashMap<String, ServerCapabilities>>,
    loading_tasks: JoinSet<()>,
}

/// Tag-based server filtering and preset storage
pub struct PresetManager {
    presets: DashMap<String, Preset>,
    storage: Arc<dyn PresetStorage>,
}

/// HTTP client with caching for MCP registry
pub struct RegistryClient {
    http: reqwest::Client,
    cache: Arc<RwLock<RegistryCache>>,
    config: RegistryConfig,
}

/// File watching, validation, hot-reload
pub struct ConfigManager {
    watcher: notify::RecommendedWatcher,
    current_config: Arc<RwLock<Config>>,
    event_bus: broadcast::Sender<ConfigEvent>,
}

/// Structured security event logging
pub struct AuditLogger {
    writer: Arc<dyn Write + Send + Sync>,
    formatter: LogFormat,
}
```

## Error Handling & Resilience

### Error Strategy

1. **Domain-specific errors**: Each module defines its own error type with `thiserror`
2. **Application errors**: `anyhow` for top-level error handling
3. **API errors**: Structured JSON responses with error codes

```rust
#[derive(Error, Debug)]
pub enum ServerError {
    #[error("server not found: {0}")]
    NotFound(String),
    #[error("sandbox error: {0}")]
    Sandbox(#[from] SandboxError),
    #[error("transport error: {0}")]
    Transport(#[from] TransportError),
    #[error("timeout after {0}ms")]
    Timeout(u64),
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::NotFound(_) => (StatusCode::NOT_FOUND, "SERVER_NOT_FOUND"),
            Self::Sandbox(_) => (StatusCode::INTERNAL_SERVER_ERROR, "SANDBOX_ERROR"),
            Self::Transport(_) => (StatusCode::BAD_GATEWAY, "TRANSPORT_ERROR"),
            Self::Timeout(_) => (StatusCode::GATEWAY_TIMEOUT, "TIMEOUT"),
        };

        Json(json!({
            "error": code,
            "message": self.to_string(),
        }))
        .into_response()
    }
}
```

### Resilience Patterns

1. **Circuit Breaker**: For registry calls and flaky servers
   ```rust
   pub struct CircuitBreaker {
       failures: AtomicU32,
       threshold: u32,
       timeout: Duration,
       state: AtomicU8, // Closed, Open, HalfOpen
   }
   ```

2. **Connection Pooling**: Health checks, max connections, idle timeout
3. **Automatic Retry**: Exponential backoff for transient failures
4. **Graceful Degradation**: Continue operating when optional servers unavailable
5. **Request Timeouts**: Configurable per-server
6. **Memory Limits**: Graceful shutdown on OOM

## Project Structure

```
mcp-one/
├── Cargo.toml
├── README.md
├── docs/
│   ├── architecture.md
│   ├── security.md
│   ├── configuration.md
│   └── api.md
├── src/
│   ├── main.rs              # CLI entry point
│   ├── lib.rs               # Library exports
│   ├── cli/                 # Command-line interface
│   │   ├── mod.rs
│   │   ├── serve.rs         # Serve command
│   │   ├── registry.rs      # Registry commands
│   │   └── preset.rs        # Preset commands
│   ├── auth/                # Authentication
│   │   ├── mod.rs
│   │   ├── provider.rs      # AuthProvider trait
│   │   ├── oauth.rs         # OAuth 2.1 implementation
│   │   ├── jwt.rs           # JWT handling
│   │   ├── storage.rs       # AuthStorage trait
│   │   └── memory_storage.rs # In-memory storage
│   ├── sandbox/             # Sandboxing implementations
│   │   ├── mod.rs
│   │   ├── traits.rs        # Sandbox trait
│   │   ├── linux.rs         # seccomp/namespaces
│   │   ├── macos.rs         # seatbelt
│   │   ├── windows.rs       # AppContainer
│   │   └── container.rs     # Docker/Podman fallback
│   ├── core/                # Core business logic
│   │   ├── mod.rs
│   │   ├── server.rs        # ServerManager
│   │   ├── client.rs        # ClientManager
│   │   ├── capability.rs    # CapabilityManager
│   │   ├── preset.rs        # PresetManager
│   │   └── filtering.rs     # Request/response filtering
│   ├── transport/           # Transport implementations
│   │   ├── mod.rs
│   │   ├── traits.rs        # Transport trait
│   │   ├── stdio.rs         # Stdio transport
│   │   ├── sse.rs           # SSE transport
│   │   ├── http.rs          # HTTP transport
│   │   └── streamable.rs    # Streamable HTTP
│   ├── config/              # Configuration management
│   │   ├── mod.rs
│   │   ├── manager.rs       # ConfigManager with hot-reload
│   │   ├── parser.rs        # TOML parsing
│   │   └── validation.rs    # Config validation
│   ├── registry/            # MCP Registry integration
│   │   ├── mod.rs
│   │   ├── client.rs        # HTTP client
│   │   └── cache.rs         # Local caching
│   ├── http_server/         # HTTP server
│   │   ├── mod.rs
│   │   ├── server.rs        # Axum server setup
│   │   ├── routes.rs        # Route handlers
│   │   ├── middleware/      # Auth, rate limit, security
│   │   └── sse.rs           # SSE endpoint handling
│   ├── audit/               # Audit logging
│   │   ├── mod.rs
│   │   └── logger.rs        # Structured audit logging
│   └── utils/               # Utilities
│       ├── mod.rs
│       └── errors.rs        # Common error types
└── tests/                   # Integration tests
    ├── integration_tests.rs
    └── fixtures/
```

## Configuration Format

**Default location:** `~/.config/mcp-one/config.toml`

```toml
# mcp-one configuration
# Place at ~/.config/mcp-one/config.toml

# Server settings
[server]
host = "127.0.0.1"
port = 3000
# Optional: HTTPS configuration
# cert_path = "/path/to/cert.pem"
# key_path = "/path/to/key.pem"

# Authentication (pluggable)
[auth]
type = "oauth"  # Options: oauth, static, jwt, none
# OAuth-specific
issuer = "https://auth.example.com"
client_id = "mcp-one"
# For static token (development only):
# type = "static"
# token = "dev-token-123"

# Feature flags
[features]
auth = true
scope_validation = true
sandbox = true
hot_reload = true
audit_logging = true

# Rate limiting
[rate_limit]
requests_per_minute = 100
burst_size = 10

# Audit logging
[audit]
path = "/var/log/mcp-one/audit.log"
format = "json"  # Options: json, pretty
max_size_mb = 100
max_files = 10

# MCP Servers
[[servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/docs"]
tags = ["filesystem", "local"]
description = "Local filesystem access"

[servers.sandbox]
network = false
filesystem = "readonly"  # Options: full, readonly, ["/allowed/path"]
max_memory_mb = 512
max_cpu_percent = 50

[[servers]]
name = "github"
command = "docker"
args = ["run", "-i", "--rm", "mcp/github"]
tags = ["github", "api"]
description = "GitHub integration via Docker"

# Presets (tag-based server collections)
[[presets]]
name = "development"
tags = ["filesystem", "local"]
description = "Local development tools"

[[presets]]
name = "full-access"
tags = ["*"]  # All servers
description = "Complete server access (admin only)"

# Registry settings
[registry]
url = "https://registry.modelcontextprotocol.io"
cache_dir = "~/.cache/mcp-one/registry"
cache_ttl_hours = 24
```

## Performance Targets

| Metric | Target | Notes |
|--------|--------|-------|
| Cold start (server spawn) | < 500ms | With sandbox enabled |
| Request latency (p99) | < 50ms | Excluding MCP server processing |
| Concurrent connections | 10,000+ | Limited by ulimit/file descriptors |
| Memory overhead per server | < 50MB | Including sandbox overhead |
| Config reload | < 100ms | Hot-reload latency |
| Token validation | < 5ms | With caching |

### Optimization Strategies

1. **Connection Pooling**: Keep-alive to downstream servers
2. **Token Cache**: LRU cache with TTL for auth validation
3. **Zero-Copy**: Use `bytes` crate for message passing
4. **SIMD JSON**: Optional `simd-json` for parsing
5. **Arena Allocators**: For short-lived request objects
6. **Lock-Free**: Use `DashMap` for concurrent state

## Security Checklist

- [x] Sandboxing: Platform-native isolation for each MCP server
- [x] Authentication: Pluggable with OAuth 2.1 default
- [x] Authorization: Tag-based scope validation
- [x] Rate Limiting: Per-IP, per-user, per-server
- [x] Audit Logging: Structured JSON for all security events
- [x] Input Validation: Size limits, schema validation
- [x] Secrets Management: Environment variable substitution
- [x] Network Isolation: No external access by default
- [x] Filesystem Restrictions: Read-only or allow-list
- [x] Resource Limits: Memory, CPU, file descriptors
- [x] Transport Security: TLS support, security headers
- [x] Error Handling: No sensitive info in error messages

## Testing Strategy

1. **Unit Tests**: Each module with mocked dependencies
2. **Integration Tests**: Full request/response cycles
3. **Security Tests**: Sandboxing effectiveness, escape attempts
4. **Performance Tests**: Load testing, latency benchmarks
5. **Compatibility Tests**: Against 1MCP test suite

## Future Considerations

1. **WebAssembly Sandbox**: Alternative to OS-level sandboxing
2. **Distributed Mode**: Multiple MCP-One instances with coordination
3. **Plugin System**: Dynamic loading of custom transports/auth
4. **Metrics**: Prometheus/OpenTelemetry integration
5. **Web Console**: Optional web-based management UI

## References

- [Model Context Protocol Specification](https://modelcontextprotocol.io)
- [1MCP Documentation](https://github.com/1mcp/agent)
- [seccomp-bpf Documentation](https://www.kernel.org/doc/html/latest/userspace-api/seccomp_filter.html)
- [Landlock Documentation](https://docs.kernel.org/security/landlock.html)
- [macOS Sandbox Guide](https://developer.apple.com/library/archive/documentation/Security/Conceptual/AppSandboxDesignGuide/)
