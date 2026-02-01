//! Cloud hosting support for MCP-One
//!
//! Provides multi-tenancy, horizontal scaling, and distributed operation.

pub mod cluster;
pub mod multi_tenant;
pub mod state;

pub use cluster::{ClusterManager, ClusterConfig, NodeInfo};
pub use multi_tenant::{TenantManager, Tenant, TenantConfig};
pub use state::{DistributedState, StateBackend};
