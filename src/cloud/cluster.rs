//! Cluster management for distributed MCP-One deployment
//!
//! Provides node discovery, leader election, and distributed state coordination.

use crate::utils::errors::{McpError, McpResult};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Cluster node information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Node ID
    pub id: String,
    /// Node address
    pub address: SocketAddr,
    /// Node role
    pub role: NodeRole,
    /// Node status
    pub status: NodeStatus,
    /// Last heartbeat timestamp
    pub last_heartbeat: chrono::DateTime<chrono::Utc>,
    /// Node metadata
    pub metadata: NodeMetadata,
}

/// Node role in cluster
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeRole {
    /// Leader node (coordinates cluster)
    Leader,
    /// Follower node (replicates state)
    Follower,
    /// Read replica (serves read-only traffic)
    ReadReplica,
}

/// Node status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum NodeStatus {
    /// Node is healthy
    Healthy,
    /// Node is unhealthy
    Unhealthy,
    /// Node is joining
    Joining,
    /// Node is leaving
    Leaving,
    /// Node is offline
    Offline,
}

/// Node metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeMetadata {
    /// Software version
    pub version: String,
    /// Server count
    pub server_count: usize,
    /// Connection count
    pub connection_count: usize,
    /// CPU usage (0-100)
    pub cpu_usage: f32,
    /// Memory usage (0-100)
    pub memory_usage: f32,
    /// Region/datacenter
    pub region: Option<String>,
    /// Zone within region
    pub zone: Option<String>,
}

/// Cluster configuration
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    /// Node ID (auto-generated if not set)
    pub node_id: Option<String>,
    /// Bind address for cluster communication
    pub bind_addr: SocketAddr,
    /// Seed nodes for discovery
    pub seed_nodes: Vec<String>,
    /// Heartbeat interval
    pub heartbeat_interval: Duration,
    /// Heartbeat timeout
    pub heartbeat_timeout: Duration,
    /// Leader election timeout
    pub election_timeout: Duration,
    /// Minimum nodes for quorum
    pub min_quorum: usize,
    /// Enable read replicas
    pub enable_read_replicas: bool,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            node_id: None,
            bind_addr: "0.0.0.0:7946".parse().unwrap(),
            seed_nodes: vec![],
            heartbeat_interval: Duration::from_secs(1),
            heartbeat_timeout: Duration::from_secs(5),
            election_timeout: Duration::from_secs(10),
            min_quorum: 3,
            enable_read_replicas: true,
        }
    }
}

/// Cluster manager
pub struct ClusterManager {
    /// Local node ID
    node_id: String,
    /// Cluster configuration
    config: ClusterConfig,
    /// All nodes in cluster
    nodes: DashMap<String, NodeInfo>,
    /// Current leader
    current_leader: Arc<RwLock<Option<String>>>,
    /// Local node role
    role: Arc<RwLock<NodeRole>>,
    /// Cluster state
    state: Arc<RwLock<ClusterState>>,
}

/// Cluster state
#[derive(Debug, Clone)]
struct ClusterState {
    /// Term number (for Raft-like consensus)
    term: u64,
    /// Last vote cast
    voted_for: Option<String>,
    /// Last state change
    last_change: Instant,
}

impl ClusterManager {
    /// Create a new cluster manager
    pub fn new(config: ClusterConfig) -> Self {
        let node_id = config.node_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
        
        let manager = Self {
            node_id: node_id.clone(),
            config,
            nodes: DashMap::new(),
            current_leader: Arc::new(RwLock::new(None)),
            role: Arc::new(RwLock::new(NodeRole::Follower)),
            state: Arc::new(RwLock::new(ClusterState {
                term: 0,
                voted_for: None,
                last_change: Instant::now(),
            })),
        };

        // Add self to nodes
        let self_node = NodeInfo {
            id: node_id,
            address: manager.config.bind_addr,
            role: NodeRole::Follower,
            status: NodeStatus::Joining,
            last_heartbeat: chrono::Utc::now(),
            metadata: NodeMetadata {
                version: env!("CARGO_PKG_VERSION").to_string(),
                ..Default::default()
            },
        };
        
        manager.nodes.insert(manager.node_id.clone(), self_node);
        manager
    }

    /// Initialize and join cluster
    pub async fn init(&self) -> McpResult<()> {
        info!("Initializing cluster manager for node: {}", self.node_id);

        // Start heartbeat task
        self.start_heartbeat_task();

        // Start leader election task
        self.start_election_task();

        // Join cluster via seed nodes
        if !self.config.seed_nodes.is_empty() {
            self.join_cluster().await?;
        } else {
            info!("No seed nodes configured, starting as single-node cluster");
            self.become_leader().await;
        }

        Ok(())
    }

    /// Join existing cluster
    async fn join_cluster(&self) -> McpResult<()> {
        for seed in &self.config.seed_nodes {
            info!("Attempting to join cluster via seed node: {}", seed);
            
            // In a real implementation, this would make an RPC call
            // For now, we just simulate successful join
            match self.contact_seed_node(seed).await {
                Ok(_) => {
                    info!("Successfully joined cluster via {}", seed);
                    return Ok(());
                }
                Err(e) => {
                    warn!("Failed to contact seed node {}: {}", seed, e);
                }
            }
        }

        // If we can't join any seed nodes, start as leader
        warn!("Could not join any seed nodes, starting as single-node cluster");
        self.become_leader().await;
        Ok(())
    }

    /// Contact a seed node to join cluster
    async fn contact_seed_node(&self, _seed: &str) -> McpResult<()> {
        // Placeholder: in production, this would be an RPC call
        // For now, simulate failure
        Err(McpError::TransportError("Seed node contact not implemented".to_string()))
    }

    /// Start heartbeat task
    fn start_heartbeat_task(&self) {
        let node_id = self.node_id.clone();
        let nodes = self.nodes.clone();
        let heartbeat_interval = self.config.heartbeat_interval;
        let heartbeat_timeout = self.config.heartbeat_timeout;

        tokio::spawn(async move {
            let mut ticker = interval(heartbeat_interval);
            
            loop {
                ticker.tick().await;

                // Update own heartbeat
                if let Some(mut node) = nodes.get_mut(&node_id) {
                    node.last_heartbeat = chrono::Utc::now();
                }

                // Check for dead nodes
                let now = chrono::Utc::now();
                let dead_nodes: Vec<String> = nodes
                    .iter()
                    .filter(|n| {
                        n.id != node_id && 
                        now.signed_duration_since(n.last_heartbeat).num_seconds() > 
                        heartbeat_timeout.as_secs() as i64
                    })
                    .map(|n| n.id.clone())
                    .collect();

                for dead_id in dead_nodes {
                    if let Some(mut node) = nodes.get_mut(&dead_id) {
                        if node.status == NodeStatus::Healthy {
                            warn!("Node {} appears to be dead", dead_id);
                            node.status = NodeStatus::Unhealthy;
                        }
                    }
                }

                // Send heartbeats to other nodes (placeholder)
                debug!("Sending heartbeats to cluster");
            }
        });
    }

    /// Start leader election task
    fn start_election_task(&self) {
        let node_id = self.node_id.clone();
        let current_leader = self.current_leader.clone();
        let role = self.role.clone();
        let state = self.state.clone();
        let election_timeout = self.config.election_timeout;

        tokio::spawn(async move {
            let mut ticker = interval(election_timeout);
            
            loop {
                ticker.tick().await;

                let leader = current_leader.read().await.clone();
                let r = *role.read().await;

                // If no leader and we're a follower, start election
                if leader.is_none() && r == NodeRole::Follower {
                    info!("No leader detected, starting election");
                    
                    let mut s = state.write().await;
                    s.term += 1;
                    s.voted_for = Some(node_id.clone());
                    drop(s);

                    // Become leader for now (simplified)
                    // In production, this would run the full Raft election
                    *role.write().await = NodeRole::Leader;
                    *current_leader.write().await = Some(node_id.clone());
                    
                    info!("Elected as leader for term {}", state.read().await.term);
                }
            }
        });
    }

    /// Become leader
    async fn become_leader(&self) {
        info!("Becoming cluster leader");
        
        *self.role.write().await = NodeRole::Leader;
        *self.current_leader.write().await = Some(self.node_id.clone());
        
        if let Some(mut node) = self.nodes.get_mut(&self.node_id) {
            node.role = NodeRole::Leader;
            node.status = NodeStatus::Healthy;
        }
    }

    /// Get current leader
    pub async fn get_leader(&self) -> Option<String> {
        self.current_leader.read().await.clone()
    }

    /// Check if this node is the leader
    pub async fn is_leader(&self) -> bool {
        *self.role.read().await == NodeRole::Leader
    }

    /// Get local node ID
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    /// Get local role
    pub async fn role(&self) -> NodeRole {
        *self.role.read().await
    }

    /// Get all nodes
    pub fn get_nodes(&self) -> Vec<NodeInfo> {
        self.nodes.iter().map(|n| n.clone()).collect()
    }

    /// Get healthy nodes
    pub fn get_healthy_nodes(&self) -> Vec<NodeInfo> {
        self.nodes
            .iter()
            .filter(|n| n.status == NodeStatus::Healthy)
            .map(|n| n.clone())
            .collect()
    }

    /// Get node count
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get healthy node count
    pub fn healthy_node_count(&self) -> usize {
        self.nodes
            .iter()
            .filter(|n| n.status == NodeStatus::Healthy)
            .count()
    }

    /// Check if quorum is available
    pub fn has_quorum(&self) -> bool {
        let healthy = self.healthy_node_count();
        healthy >= self.config.min_quorum && healthy > self.node_count() / 2
    }

    /// Update local node metadata
    pub async fn update_metadata(&self, metadata: NodeMetadata) {
        if let Some(mut node) = self.nodes.get_mut(&self.node_id) {
            node.metadata = metadata;
        }
    }

    /// Get least loaded node for request routing
    pub fn get_least_loaded_node(&self) -> Option<NodeInfo> {
        self.nodes
            .iter()
            .filter(|n| n.status == NodeStatus::Healthy)
            .min_by_key(|n| n.metadata.connection_count)
            .map(|n| n.clone())
    }

    /// Shutdown cluster manager
    pub async fn shutdown(&self) {
        info!("Shutting down cluster manager");
        
        if let Some(mut node) = self.nodes.get_mut(&self.node_id) {
            node.status = NodeStatus::Leaving;
        }
        
        // Step down if leader
        if self.is_leader().await {
            *self.current_leader.write().await = None;
        }
    }
}

/// Cluster-aware request routing
pub struct ClusterRouter {
    cluster: Arc<ClusterManager>,
}

impl ClusterRouter {
    /// Create a new cluster router
    pub fn new(cluster: Arc<ClusterManager>) -> Self {
        Self { cluster }
    }

    /// Route request to appropriate node
    pub async fn route_request(&self) -> Option<NodeInfo> {
        // If local node can handle, use it
        if self.cluster.is_leader().await {
            return self.cluster.nodes.get(&self.cluster.node_id).map(|n| n.clone());
        }

        // Otherwise route to leader
        if let Some(leader_id) = self.cluster.get_leader().await {
            return self.cluster.nodes.get(&leader_id).map(|n| n.clone());
        }

        // Fallback to least loaded node
        self.cluster.get_least_loaded_node()
    }

    /// Get read replica for read operations
    pub async fn get_read_replica(&self) -> Option<NodeInfo> {
        // Find a healthy read replica
        self.cluster
            .nodes
            .iter()
            .find(|n| n.role == NodeRole::ReadReplica && n.status == NodeStatus::Healthy)
            .map(|n| n.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cluster_config_default() {
        let config = ClusterConfig::default();
        assert_eq!(config.heartbeat_interval, Duration::from_secs(1));
        assert_eq!(config.heartbeat_timeout, Duration::from_secs(5));
        assert_eq!(config.min_quorum, 3);
    }

    #[tokio::test]
    async fn test_cluster_manager_creation() {
        let config = ClusterConfig::default();
        let manager = ClusterManager::new(config);
        
        assert_eq!(manager.node_count(), 1);
        assert!(manager.get_nodes().len() > 0);
    }

    #[tokio::test]
    async fn test_become_leader() {
        let config = ClusterConfig::default();
        let manager = ClusterManager::new(config);
        
        manager.become_leader().await;
        
        assert!(manager.is_leader().await);
        assert_eq!(manager.role().await, NodeRole::Leader);
        assert_eq!(manager.get_leader().await, Some(manager.node_id().to_string()));
    }

    #[test]
    fn test_node_metadata_default() {
        let metadata = NodeMetadata::default();
        assert_eq!(metadata.server_count, 0);
        assert_eq!(metadata.connection_count, 0);
    }

    #[tokio::test]
    async fn test_update_metadata() {
        let config = ClusterConfig::default();
        let manager = ClusterManager::new(config);
        
        let metadata = NodeMetadata {
            version: "1.0.0".to_string(),
            server_count: 5,
            connection_count: 100,
            cpu_usage: 50.0,
            memory_usage: 60.0,
            region: Some("us-east-1".to_string()),
            zone: Some("a".to_string()),
        };
        
        manager.update_metadata(metadata.clone()).await;
        
        let node = manager.nodes.get(manager.node_id()).unwrap();
        assert_eq!(node.metadata.server_count, 5);
        assert_eq!(node.metadata.connection_count, 100);
    }
}
