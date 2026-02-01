//! Multi-tenancy support for cloud hosting
//!
//! Allows multiple tenants (organizations/users) to share a single MCP-One
//! instance while maintaining isolation between tenants.

use crate::config::Config;
use crate::utils::errors::{McpError, McpResult};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

/// Tenant information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    /// Unique tenant ID
    pub id: String,
    /// Tenant name
    pub name: String,
    /// Tenant configuration
    pub config: TenantConfig,
    /// Created at timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Updated at timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
    /// Whether tenant is active
    pub active: bool,
    /// Resource usage
    pub resource_usage: ResourceUsage,
}

/// Tenant configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantConfig {
    /// Maximum number of servers
    pub max_servers: usize,
    /// Maximum concurrent connections
    pub max_connections: usize,
    /// Rate limit per minute
    pub rate_limit_per_minute: u32,
    /// Allowed server tags
    pub allowed_tags: Vec<String>,
    /// Quota: requests per day
    pub daily_request_quota: u64,
    /// Custom domain (optional)
    pub custom_domain: Option<String>,
    /// Feature flags
    pub features: TenantFeatures,
}

impl Default for TenantConfig {
    fn default() -> Self {
        Self {
            max_servers: 10,
            max_connections: 100,
            rate_limit_per_minute: 1000,
            allowed_tags: vec!["*".to_string()],
            daily_request_quota: 100_000,
            custom_domain: None,
            features: TenantFeatures::default(),
        }
    }
}

/// Tenant feature flags
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantFeatures {
    /// Enable sandboxing
    pub sandboxing: bool,
    /// Enable audit logging
    pub audit_logging: bool,
    /// Enable custom auth
    pub custom_auth: bool,
    /// Enable registry access
    pub registry_access: bool,
    /// Enable presets
    pub presets: bool,
}

impl Default for TenantFeatures {
    fn default() -> Self {
        Self {
            sandboxing: true,
            audit_logging: true,
            custom_auth: true,
            registry_access: true,
            presets: true,
        }
    }
}

/// Resource usage for a tenant
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceUsage {
    /// Current server count
    pub server_count: usize,
    /// Current connection count
    pub connection_count: usize,
    /// Requests today
    pub requests_today: u64,
    /// Last request timestamp
    pub last_request_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Tenant manager
pub struct TenantManager {
    /// Tenants storage
    tenants: DashMap<String, Arc<RwLock<Tenant>>>,
    /// Domain to tenant mapping
    domain_mapping: DashMap<String, String>,
    /// Default tenant config
    default_config: TenantConfig,
}

impl TenantManager {
    /// Create a new tenant manager
    pub fn new(default_config: TenantConfig) -> Self {
        Self {
            tenants: DashMap::new(),
            domain_mapping: DashMap::new(),
            default_config,
        }
    }

    /// Create a new tenant
    pub async fn create_tenant(&self, name: impl Into<String>, config: Option<TenantConfig>) -> McpResult<Tenant> {
        let name = name.into();
        let id = Uuid::new_v4().to_string();
        let now = chrono::Utc::now();

        let tenant = Tenant {
            id: id.clone(),
            name: name.clone(),
            config: config.unwrap_or_else(|| self.default_config.clone()),
            created_at: now,
            updated_at: now,
            active: true,
            resource_usage: ResourceUsage::default(),
        };

        self.tenants.insert(id.clone(), Arc::new(RwLock::new(tenant.clone())));
        
        info!("Created tenant: {} ({})", name, id);
        Ok(tenant)
    }

    /// Get tenant by ID
    pub async fn get_tenant(&self, tenant_id: &str) -> Option<Arc<RwLock<Tenant>>> {
        self.tenants.get(tenant_id).map(|t| t.clone())
    }

    /// Get tenant by domain
    pub async fn get_tenant_by_domain(&self, domain: &str) -> Option<Arc<RwLock<Tenant>>> {
        self.domain_mapping
            .get(domain)
            .and_then(|id| self.tenants.get(&*id).map(|t| t.clone()))
    }

    /// Update tenant
    pub async fn update_tenant(&self, tenant_id: &str, config: TenantConfig) -> McpResult<()> {
        if let Some(tenant) = self.tenants.get(tenant_id) {
            let mut t = tenant.write().await;
            t.config = config;
            t.updated_at = chrono::Utc::now();
            debug!("Updated tenant: {}", tenant_id);
            Ok(())
        } else {
            Err(McpError::ConfigError(format!("Tenant not found: {}", tenant_id)))
        }
    }

    /// Delete tenant
    pub async fn delete_tenant(&self, tenant_id: &str) -> McpResult<()> {
        if let Some((id, tenant)) = self.tenants.remove(tenant_id) {
            // Remove domain mappings
            let t = tenant.read().await;
            if let Some(domain) = &t.config.custom_domain {
                self.domain_mapping.remove(domain);
            }
            
            info!("Deleted tenant: {}", id);
            Ok(())
        } else {
            Err(McpError::ConfigError(format!("Tenant not found: {}", tenant_id)))
        }
    }

    /// Set custom domain for tenant
    pub async fn set_domain(&self, tenant_id: &str, domain: impl Into<String>) -> McpResult<()> {
        let domain = domain.into();
        
        if let Some(tenant) = self.tenants.get(tenant_id) {
            let mut t = tenant.write().await;
            
            // Remove old domain mapping
            if let Some(old_domain) = &t.config.custom_domain {
                self.domain_mapping.remove(old_domain);
            }
            
            // Add new domain mapping
            self.domain_mapping.insert(domain.clone(), tenant_id.to_string());
            t.config.custom_domain = Some(domain);
            t.updated_at = chrono::Utc::now();
            
            Ok(())
        } else {
            Err(McpError::ConfigError(format!("Tenant not found: {}", tenant_id)))
        }
    }

    /// Check if tenant can create more servers
    pub async fn can_create_server(&self, tenant_id: &str) -> McpResult<bool> {
        if let Some(tenant) = self.tenants.get(tenant_id) {
            let t = tenant.read().await;
            Ok(t.resource_usage.server_count < t.config.max_servers && t.active)
        } else {
            Err(McpError::ConfigError(format!("Tenant not found: {}", tenant_id)))
        }
    }

    /// Record server creation
    pub async fn record_server_created(&self, tenant_id: &str) -> McpResult<()> {
        if let Some(tenant) = self.tenants.get(tenant_id) {
            let mut t = tenant.write().await;
            t.resource_usage.server_count += 1;
            debug!("Tenant {} server count: {}", tenant_id, t.resource_usage.server_count);
            Ok(())
        } else {
            Err(McpError::ConfigError(format!("Tenant not found: {}", tenant_id)))
        }
    }

    /// Record server deletion
    pub async fn record_server_deleted(&self, tenant_id: &str) -> McpResult<()> {
        if let Some(tenant) = self.tenants.get(tenant_id) {
            let mut t = tenant.write().await;
            if t.resource_usage.server_count > 0 {
                t.resource_usage.server_count -= 1;
            }
            Ok(())
        } else {
            Err(McpError::ConfigError(format!("Tenant not found: {}", tenant_id)))
        }
    }

    /// Check rate limit for tenant
    pub async fn check_rate_limit(&self, tenant_id: &str) -> McpResult<bool> {
        if let Some(tenant) = self.tenants.get(tenant_id) {
            let t = tenant.read().await;
            // Simplified rate check - in production use a proper rate limiter
            Ok(t.active)
        } else {
            Err(McpError::ConfigError(format!("Tenant not found: {}", tenant_id)))
        }
    }

    /// List all tenants
    pub async fn list_tenants(&self) -> Vec<Tenant> {
        let mut tenants = Vec::new();
        for entry in self.tenants.iter() {
            tenants.push(entry.read().await.clone());
        }
        tenants
    }

    /// Get tenant count
    pub fn tenant_count(&self) -> usize {
        self.tenants.len()
    }

    /// Get tenant stats
    pub async fn get_stats(&self, tenant_id: &str) -> McpResult<ResourceUsage> {
        if let Some(tenant) = self.tenants.get(tenant_id) {
            let t = tenant.read().await;
            Ok(t.resource_usage.clone())
        } else {
            Err(McpError::ConfigError(format!("Tenant not found: {}", tenant_id)))
        }
    }
}

impl Default for TenantManager {
    fn default() -> Self {
        Self::new(TenantConfig::default())
    }
}

/// Tenant-aware configuration wrapper
pub struct TenantAwareConfig {
    /// Base configuration
    pub base: Config,
    /// Tenant ID
    pub tenant_id: String,
    /// Tenant-specific overrides
    pub overrides: TenantConfig,
}

impl TenantAwareConfig {
    /// Apply tenant constraints to configuration
    pub fn apply_constraints(&mut self) {
        // Limit servers based on tenant quota
        if self.base.servers.len() > self.overrides.max_servers {
            self.base.servers.truncate(self.overrides.max_servers);
        }

        // Filter allowed tags
        if !self.overrides.allowed_tags.contains(&"*".to_string()) {
            for server in &mut self.base.servers {
                server.tags.retain(|tag| {
                    self.overrides.allowed_tags.contains(tag)
                });
            }
        }

        // Disable features if not enabled for tenant
        if !self.overrides.features.sandboxing {
            for server in &mut self.base.servers {
                server.sandbox.enabled = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tenant_config_default() {
        let config = TenantConfig::default();
        assert_eq!(config.max_servers, 10);
        assert_eq!(config.max_connections, 100);
        assert_eq!(config.rate_limit_per_minute, 1000);
    }

    #[tokio::test]
    async fn test_create_tenant() {
        let manager = TenantManager::default();
        let tenant = manager.create_tenant("test-tenant", None).await.unwrap();
        
        assert_eq!(tenant.name, "test-tenant");
        assert!(tenant.active);
        
        // Verify it was stored
        let retrieved = manager.get_tenant(&tenant.id).await;
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_tenant_server_limit() {
        let manager = TenantManager::default();
        let tenant = manager.create_tenant("test-tenant", None).await.unwrap();
        
        // Should be able to create servers up to limit
        assert!(manager.can_create_server(&tenant.id).await.unwrap());
        
        // Record some servers
        for _ in 0..10 {
            manager.record_server_created(&tenant.id).await.unwrap();
        }
        
        // Should hit limit
        assert!(!manager.can_create_server(&tenant.id).await.unwrap());
    }

    #[tokio::test]
    async fn test_delete_tenant() {
        let manager = TenantManager::default();
        let tenant = manager.create_tenant("test-tenant", None).await.unwrap();
        let id = tenant.id.clone();
        
        manager.delete_tenant(&id).await.unwrap();
        
        assert!(manager.get_tenant(&id).await.is_none());
    }

    #[tokio::test]
    async fn test_list_tenants() {
        let manager = TenantManager::default();
        
        manager.create_tenant("tenant-1", None).await.unwrap();
        manager.create_tenant("tenant-2", None).await.unwrap();
        
        let tenants = manager.list_tenants().await;
        assert_eq!(tenants.len(), 2);
    }
}
