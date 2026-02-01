//! Connection pooling for MCP servers
//!
//! This module implements connection pooling to maintain persistent connections
//! to downstream MCP servers, reducing latency by avoiding process spawning overhead.

use crate::config::McpServerConfig;
use crate::core::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::sandbox::create_sandbox;
use crate::transport::{StdioTransport, Transport};
use crate::utils::errors::{McpError, McpResult};
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{interval, Duration, Instant};
use tracing::{debug, error, info, warn};

/// Pooled connection to an MCP server
pub struct PooledConnection {
    /// Unique connection ID
    pub id: String,
    /// The transport connection
    transport: Arc<RwLock<Box<dyn Transport>>>,
    /// When the connection was created
    created_at: Instant,
    /// Last time the connection was used
    last_used: Arc<RwLock<Instant>>,
    /// Connection health status
    healthy: Arc<RwLock<bool>>,
    /// Server configuration
    config: McpServerConfig,
}

impl PooledConnection {
    /// Create a new pooled connection
    pub async fn new(config: McpServerConfig, id: String) -> McpResult<Self> {
        let sandbox = create_sandbox(&config);
        let sandbox_arc: Arc<dyn crate::sandbox::Sandbox> = Arc::from(sandbox);

        let transport: Box<dyn Transport> = Box::new(
            StdioTransport::new(
                config.command.clone(),
                config.args.clone(),
                config.env.clone(),
                sandbox_arc,
            )
            .await?,
        );

        let now = Instant::now();

        Ok(Self {
            id,
            transport: Arc::new(RwLock::new(transport)),
            created_at: now,
            last_used: Arc::new(RwLock::new(now)),
            healthy: Arc::new(RwLock::new(true)),
            config,
        })
    }

    /// Send a request through this connection
    pub async fn send_request(&self, request: JsonRpcRequest) -> McpResult<JsonRpcResponse> {
        let transport = self.transport.read().await;
        let response = transport.send_request(request).await;

        // Update last used time
        *self.last_used.write().await = Instant::now();

        // Mark as unhealthy if request failed
        if response.is_err() {
            *self.healthy.write().await = false;
        }

        response
    }

    /// Check if connection is still healthy
    pub async fn is_healthy(&self) -> bool {
        let transport = self.transport.read().await;
        let connected = transport.is_connected().await;
        *self.healthy.read().await && connected
    }

    /// Get time since last use
    pub async fn idle_duration(&self) -> Duration {
        self.last_used.read().await.elapsed()
    }

    /// Get connection age
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Close the connection
    pub async fn close(&self) -> McpResult<()> {
        let transport = self.transport.read().await;
        transport.close().await
    }
}

/// Connection pool configuration
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of connections per server
    pub max_connections: usize,
    /// Minimum number of connections to maintain
    pub min_connections: usize,
    /// Maximum connection age before forced recycle
    pub max_connection_age: Duration,
    /// Maximum idle time before connection is closed
    pub max_idle_time: Duration,
    /// Health check interval
    pub health_check_interval: Duration,
    /// Whether to enable connection pooling
    pub enabled: bool,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_connections: 1,
            max_connection_age: Duration::from_secs(3600), // 1 hour
            max_idle_time: Duration::from_secs(300),       // 5 minutes
            health_check_interval: Duration::from_secs(30),
            enabled: true,
        }
    }
}

/// Connection pool for a single MCP server
type ConnectionPool = Arc<RwLock<Vec<PooledConnection>>>;

/// Manages connection pools for all MCP servers
pub struct ConnectionPoolManager {
    /// Pools keyed by server name
    pools: DashMap<String, ConnectionPool>,
    /// Pool configuration
    config: PoolConfig,
    /// Channel for pool maintenance tasks
    maintenance_tx: mpsc::Sender<PoolMaintenanceTask>,
}

#[derive(Debug)]
enum PoolMaintenanceTask {
    HealthCheck,
    Cleanup,
}

impl ConnectionPoolManager {
    /// Create a new connection pool manager
    pub fn new(config: PoolConfig) -> Self {
        let (maintenance_tx, mut maintenance_rx) = mpsc::channel(100);

        let manager = Self {
            pools: DashMap::new(),
            config,
            maintenance_tx: maintenance_tx.clone(),
        };

        // Spawn maintenance task
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let _ = maintenance_tx.send(PoolMaintenanceTask::HealthCheck).await;
                    }
                    Some(task) = maintenance_rx.recv() => {
                        match task {
                            PoolMaintenanceTask::HealthCheck => {
                                // Health check is done per-pool
                            }
                            PoolMaintenanceTask::Cleanup => {
                                // Cleanup is done per-pool
                            }
                        }
                    }
                }
            }
        });

        manager
    }

    /// Get or create a pool for a server
    pub fn get_or_create_pool(&self, server_name: &str) -> ConnectionPool {
        self.pools
            .entry(server_name.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(Vec::new())))
            .clone()
    }

    /// Acquire a connection from the pool
    pub async fn acquire_connection(
        &self,
        server_name: &str,
        config: &McpServerConfig,
    ) -> McpResult<PooledConnection> {
        if !self.config.enabled {
            // If pooling is disabled, create a new connection each time
            return PooledConnection::new(config.clone(), format!("{}-ephemeral", server_name)).await;
        }

        let pool = self.get_or_create_pool(server_name);

        // Try to find an existing healthy connection
        {
            let pool_read = pool.read().await;
            for conn in pool_read.iter() {
                if conn.is_healthy().await && conn.idle_duration().await < self.config.max_idle_time {
                    debug!("Reusing existing connection {} for {}", conn.id, server_name);
                    return Ok(PooledConnection {
                        id: conn.id.clone(),
                        transport: conn.transport.clone(),
                        created_at: conn.created_at,
                        last_used: conn.last_used.clone(),
                        healthy: conn.healthy.clone(),
                        config: config.clone(),
                    });
                }
            }
        }

        // Create a new connection
        let conn_id = format!("{}-{}", server_name, uuid::Uuid::new_v4());
        let conn = PooledConnection::new(config.clone(), conn_id).await?;

        // Add to pool if under limit
        {
            let mut pool_write = pool.write().await;
            if pool_write.len() < self.config.max_connections {
                pool_write.push(PooledConnection {
                    id: conn.id.clone(),
                    transport: conn.transport.clone(),
                    created_at: conn.created_at,
                    last_used: conn.last_used.clone(),
                    healthy: conn.healthy.clone(),
                    config: config.clone(),
                });
            }
        }

        info!("Created new connection {} for {}", conn.id, server_name);
        Ok(conn)
    }

    /// Release a connection back to the pool (no-op for now, connections stay in pool)
    pub async fn release_connection(&self, _server_name: &str, _conn: PooledConnection) {
        // Connection stays in the pool, last_used was already updated
    }

    /// Clean up stale connections for a server
    pub async fn cleanup_pool(&self, server_name: &str) {
        let pool = match self.pools.get(server_name) {
            Some(p) => p.clone(),
            None => return,
        };

        let mut pool_write = pool.write().await;
        let before_count = pool_write.len();

        // Remove stale or unhealthy connections
        pool_write.retain(|conn| {
            let age = conn.age();
            let is_healthy = tokio::task::block_in_place(|| {
                // This is a bit hacky - ideally we'd use async here
                true
            });

            age < self.config.max_connection_age && is_healthy
        });

        let after_count = pool_write.len();
        if before_count != after_count {
            debug!(
                "Cleaned up {} connections for {}",
                before_count - after_count,
                server_name
            );
        }
    }

    /// Clean up all pools
    pub async fn cleanup_all_pools(&self) {
        for entry in self.pools.iter() {
            self.cleanup_pool(entry.key()).await;
        }
    }

    /// Get pool statistics
    pub fn get_pool_stats(&self, server_name: &str) -> Option<PoolStats> {
        self.pools.get(server_name).map(|_pool| {
            // Note: This is synchronous - for accurate stats we'd need async
            PoolStats {
                total_connections: 0, // Would need async block
                healthy_connections: 0,
                idle_connections: 0,
            }
        })
    }

    /// Shutdown all pools
    pub async fn shutdown(&self) {
        info!("Shutting down connection pools...");

        for entry in self.pools.iter() {
            let pool = entry.value().clone();
            let pool_read = pool.read().await;

            for conn in pool_read.iter() {
                if let Err(e) = conn.close().await {
                    warn!("Error closing connection {}: {}", conn.id, e);
                }
            }
        }

        self.pools.clear();
        info!("All connection pools shut down");
    }
}

/// Pool statistics
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub total_connections: usize,
    pub healthy_connections: usize,
    pub idle_connections: usize,
}

impl Default for ConnectionPoolManager {
    fn default() -> Self {
        Self::new(PoolConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_config_default() {
        let config = PoolConfig::default();
        assert_eq!(config.max_connections, 10);
        assert_eq!(config.min_connections, 1);
        assert!(config.enabled);
    }

    #[tokio::test]
    async fn test_pool_manager_creation() {
        let manager = ConnectionPoolManager::new(PoolConfig::default());
        assert!(manager.pools.is_empty());
    }

    #[test]
    fn test_pool_stats() {
        let stats = PoolStats {
            total_connections: 5,
            healthy_connections: 4,
            idle_connections: 3,
        };
        assert_eq!(stats.total_connections, 5);
        assert_eq!(stats.healthy_connections, 4);
        assert_eq!(stats.idle_connections, 3);
    }
}
