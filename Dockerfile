# Multi-stage Dockerfile for MCP-One
# Creates a minimal, secure image for running the MCP proxy server

# Stage 1: Builder
FROM rust:1.83-slim-bookworm AS builder

WORKDIR /usr/src/mcp-one

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests first for better layer caching
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs to cache dependencies
RUN mkdir -p src && \
    echo "fn main() {}" > src/main.rs && \
    echo "pub fn lib() {}" > src/lib.rs

# Build dependencies (cached if Cargo.toml/Cargo.lock haven't changed)
RUN cargo build --release && \
    rm -rf src

# Copy actual source code
COPY src ./src

# Build the actual application
RUN touch src/main.rs src/lib.rs && \
    cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    # For sandbox support (optional but recommended)
    libseccomp2 \
    # For debugging
    curl \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

# Create non-root user for security
RUN groupadd -r mcpo && useradd -r -g mcpo -m -d /home/mcpo mcpo

# Create necessary directories
RUN mkdir -p /etc/mcp-one /var/log/mcp-one && \
    chown -R mcpo:mcpo /etc/mcp-one /var/log/mcp-one

# Copy binary from builder
COPY --from=builder /usr/src/mcp-one/target/release/mcpo /usr/local/bin/mcpo

# Copy default config
COPY --chown=mcpo:mcpo config.example.toml /etc/mcp-one/config.toml

# Switch to non-root user
USER mcpo

# Expose default port
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:3000/health || exit 1

# Set environment variables
ENV MCP_ONE_CONFIG=/etc/mcp-one/config.toml
ENV RUST_LOG=info

# Default command
ENTRYPOINT ["mcpo"]
CMD ["serve", "--config", "/etc/mcp-one/config.toml"]
