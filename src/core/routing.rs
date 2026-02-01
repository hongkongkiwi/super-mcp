//! Smart request routing for MCP servers

use crate::core::protocol::JsonRpcRequest;
use crate::utils::errors::{McpError, McpResult};
use std::collections::HashMap;
use tracing::debug;

/// Routing strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingStrategy {
    FirstAvailable,
    MethodPrefix,
    Capability,
    RoundRobin,
    Direct,
}

impl Default for RoutingStrategy {
    fn default() -> Self {
        RoutingStrategy::Capability
    }
}

/// Server route information
#[derive(Debug, Clone)]
pub struct ServerRoute {
    pub name: String,
    pub tags: Vec<String>,
    pub priority: u32,
    pub healthy: bool,
    pub load: u32,
}

/// Request router
pub struct RequestRouter {
    strategy: RoutingStrategy,
    routes: HashMap<String, ServerRoute>,
    round_robin_counter: std::sync::atomic::AtomicUsize,
    method_prefixes: HashMap<String, Vec<String>>,
}

impl RequestRouter {
    pub fn new(strategy: RoutingStrategy) -> Self {
        Self {
            strategy,
            routes: HashMap::new(),
            round_robin_counter: std::sync::atomic::AtomicUsize::new(0),
            method_prefixes: HashMap::new(),
        }
    }

    pub fn register_server(&mut self, name: impl Into<String>, tags: Vec<String>) {
        let name = name.into();
        let route = ServerRoute {
            name: name.clone(),
            tags,
            priority: 100,
            healthy: true,
            load: 0,
        };
        self.routes.insert(name, route);
    }

    pub fn unregister_server(&mut self, name: &str) {
        self.routes.remove(name);
    }

    pub fn set_server_health(&mut self, name: &str, healthy: bool) {
        if let Some(route) = self.routes.get_mut(name) {
            route.healthy = healthy;
        }
    }

    pub fn register_method_prefix(&mut self, prefix: impl Into<String>, servers: Vec<String>) {
        self.method_prefixes.insert(prefix.into(), servers);
    }

    pub fn route(&self, request: &JsonRpcRequest) -> McpResult<String> {
        match self.strategy {
            RoutingStrategy::FirstAvailable => self.route_first_available(),
            RoutingStrategy::MethodPrefix => self.route_by_method_prefix(request),
            RoutingStrategy::Capability => self.route_by_capability(request),
            RoutingStrategy::RoundRobin => self.route_round_robin(),
            RoutingStrategy::Direct => Err(McpError::InvalidRequest(
                "Direct routing requires explicit server name".to_string()
            )),
        }
    }

    fn route_first_available(&self) -> McpResult<String> {
        for route in self.routes.values() {
            if route.healthy {
                return Ok(route.name.clone());
            }
        }
        Err(McpError::ServerNotFound("No healthy servers available".to_string()))
    }

    fn route_by_method_prefix(&self, request: &JsonRpcRequest) -> McpResult<String> {
        let method = &request.method;

        for (prefix, servers) in &self.method_prefixes {
            if method.starts_with(prefix) || method.contains(prefix.trim_end_matches('/')) {
                for server_name in servers {
                    if let Some(route) = self.routes.get(server_name) {
                        if route.healthy {
                            debug!("Routed method '{}' to '{}' by prefix '{}'", method, server_name, prefix);
                            return Ok(server_name.clone());
                        }
                    }
                }
            }
        }

        self.route_first_available()
    }

    fn route_by_capability(&self, request: &JsonRpcRequest) -> McpResult<String> {
        let method = &request.method;

        let target_capability = if method.starts_with("tools/") {
            Some("tools")
        } else if method.starts_with("resources/") {
            Some("resources")
        } else if method.starts_with("prompts/") {
            Some("prompts")
        } else {
            None
        };

        if let Some(cap) = target_capability {
            for route in self.routes.values() {
                if route.healthy && route.tags.contains(&cap.to_string()) {
                    debug!("Routed method '{}' to '{}' by capability '{}'", method, route.name, cap);
                    return Ok(route.name.clone());
                }
            }
        }

        if let Ok(server) = self.route_by_method_prefix(request) {
            return Ok(server);
        }

        self.route_least_loaded()
    }

    fn route_round_robin(&self) -> McpResult<String> {
        let healthy_servers: Vec<_> = self
            .routes
            .values()
            .filter(|r| r.healthy)
            .collect();

        if healthy_servers.is_empty() {
            return Err(McpError::ServerNotFound("No healthy servers available".to_string()));
        }

        let counter = self.round_robin_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let index = counter % healthy_servers.len();

        Ok(healthy_servers[index].name.clone())
    }

    fn route_least_loaded(&self) -> McpResult<String> {
        self.routes
            .values()
            .filter(|r| r.healthy)
            .min_by_key(|r| r.load)
            .map(|r| r.name.clone())
            .ok_or_else(|| McpError::ServerNotFound("No healthy servers available".to_string()))
    }

    pub fn route_to_server(&self, server_name: &str) -> McpResult<String> {
        if let Some(route) = self.routes.get(server_name) {
            if route.healthy {
                Ok(server_name.to_string())
            } else {
                Err(McpError::ServerNotFound(format!(
                    "Server '{}' is not healthy",
                    server_name
                )))
            }
        } else {
            Err(McpError::ServerNotFound(format!(
                "Server '{}' not found",
                server_name
            )))
        }
    }

    pub fn get_healthy_servers(&self) -> Vec<String> {
        self.routes
            .values()
            .filter(|r| r.healthy)
            .map(|r| r.name.clone())
            .collect()
    }
}

impl Default for RequestRouter {
    fn default() -> Self {
        Self::new(RoutingStrategy::default())
    }
}

/// Routing middleware
pub struct RoutingMiddleware {
    router: RequestRouter,
}

impl RoutingMiddleware {
    pub fn new(router: RequestRouter) -> Self {
        Self { router }
    }

    pub fn route(&self, request: &JsonRpcRequest) -> McpResult<String> {
        self.router.route(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_registration() {
        let mut router = RequestRouter::new(RoutingStrategy::FirstAvailable);
        router.register_server("server1", vec!["filesystem".to_string()]);
        router.register_server("server2", vec!["network".to_string()]);

        assert_eq!(router.routes.len(), 2);
    }

    #[test]
    fn test_route_first_available() {
        let mut router = RequestRouter::new(RoutingStrategy::FirstAvailable);
        router.register_server("server1", vec![]);
        router.register_server("server2", vec![]);

        let request = JsonRpcRequest::new("test", None);
        let result = router.route(&request);
        
        assert!(result.is_ok());
    }

    #[test]
    fn test_route_by_capability() {
        let mut router = RequestRouter::new(RoutingStrategy::Capability);
        router.register_server("tools-server", vec!["tools".to_string()]);

        let request = JsonRpcRequest::new("tools/list", None);
        let result = router.route(&request);
        
        assert_eq!(result.unwrap(), "tools-server");
    }

    #[test]
    fn test_route_to_unhealthy_server_fails() {
        let mut router = RequestRouter::new(RoutingStrategy::FirstAvailable);
        router.register_server("server1", vec![]);
        router.set_server_health("server1", false);

        let request = JsonRpcRequest::new("test", None);
        let result = router.route(&request);
        
        assert!(result.is_err());
    }
}
