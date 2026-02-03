//! Request/response filtering based on scopes
//!
//! This module implements filtering of MCP capabilities based on user scopes.

use crate::core::protocol::{
    JsonRpcRequest, JsonRpcResponse, RequestId,
};
use crate::utils::errors::McpResult;
use std::collections::HashSet;
use tracing::debug;

/// Scope-based filter for MCP capabilities
#[derive(Default)]
pub struct CapabilityFilter {
    /// Allowed server tags
    allowed_tags: HashSet<String>,
    /// Allowed tool names (None = all allowed)
    allowed_tools: Option<HashSet<String>>,
    /// Denied tool names
    denied_tools: HashSet<String>,
}

impl CapabilityFilter {
    /// Create a new filter from user scopes
    pub fn from_scopes(scopes: &[String]) -> Self {
        let mut allowed_tags = HashSet::new();
        let mut allowed_tools = None;
        let mut denied_tools = HashSet::new();

        let _has_wildcard = scopes.iter().any(|s| s == "*");

        for scope in scopes {
            if scope == "*" {
                // Wildcard is handled separately
                continue;
            }
            
            if let Some(tag) = scope.strip_prefix("tag:") {
                allowed_tags.insert(tag.to_string());
            } else if let Some(tool) = scope.strip_prefix("tool:") {
                allowed_tools
                    .get_or_insert_with(HashSet::new)
                    .insert(tool.to_string());
            } else if let Some(tool) = scope.strip_prefix("-tool:") {
                denied_tools.insert(tool.to_string());
            }
        }

        // If no specific tags are set but we have tool scopes,
        // allow access to all servers
        if allowed_tags.is_empty() && allowed_tools.is_none() {
            allowed_tags.insert("*".to_string());
        }

        Self {
            allowed_tags,
            allowed_tools,
            denied_tools,
        }
    }

    /// Check if user can access a server with given tags
    pub fn can_access_server(&self, server_tags: &[String]) -> bool {
        if self.allowed_tags.contains("*") {
            return true;
        }
        server_tags.iter().any(|tag| self.allowed_tags.contains(tag))
    }

    /// Check if user can use a specific tool
    pub fn can_use_tool(&self, tool_name: &str) -> bool {
        if self.denied_tools.contains(tool_name) {
            return false;
        }
        match &self.allowed_tools {
            None => true,
            Some(tools) => tools.contains(tool_name),
        }
    }

    /// Filter a request based on scopes
    pub fn filter_request(&self, request: &JsonRpcRequest) -> McpResult<bool> {
        let method = request.method.as_str();

        if method == "tools/call" || method == "tools/invoke" {
            if let Some(params) = &request.params {
                if let Some(name) = params.get("name").and_then(|v| v.as_str()) {
                    if !self.can_use_tool(name) {
                        debug!("Tool '{}' denied by scope filter", name);
                        return Ok(false);
                    }
                }
            }
        }

        Ok(true)
    }

    /// Filter tools/list response
    pub fn filter_tools_list(&self, response: &mut JsonRpcResponse) {
        if let Some(result) = &mut response.result {
            if let Some(tools) = result.get_mut("tools").and_then(|v| v.as_array_mut()) {
                tools.retain(|tool| {
                    if let Some(name) = tool.get("name").and_then(|v| v.as_str()) {
                        let allowed = self.can_use_tool(name);
                        if !allowed {
                            debug!("Filtered out tool '{}' from list", name);
                        }
                        allowed
                    } else {
                        true
                    }
                });
            }
        }
    }

    /// Create an error response for denied access
    pub fn create_denied_response(&self, request_id: Option<RequestId>, resource: &str) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request_id,
            result: None,
            error: Some(crate::core::protocol::JsonRpcError {
                code: -32000,
                message: format!("Access denied: '{}' is not allowed by your scopes", resource),
                data: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_from_scopes_wildcard() {
        let filter = CapabilityFilter::from_scopes(&["*".to_string()]);
        assert!(filter.can_access_server(&["any".to_string()]));
        assert!(filter.can_use_tool("any_tool"));
    }

    #[test]
    fn test_filter_from_scopes_tags() {
        let filter = CapabilityFilter::from_scopes(&["tag:filesystem".to_string(), "tag:network".to_string()]);
        assert!(filter.can_access_server(&["filesystem".to_string()]));
        assert!(filter.can_access_server(&["network".to_string()]));
        assert!(!filter.can_access_server(&["other".to_string()]));
    }

    #[test]
    fn test_filter_tools() {
        let filter = CapabilityFilter::from_scopes(&["tool:read_file".to_string(), "tool:write_file".to_string()]);
        assert!(filter.can_use_tool("read_file"));
        assert!(filter.can_use_tool("write_file"));
        assert!(!filter.can_use_tool("delete_file"));
    }

    #[test]
    fn test_filter_denied_tools() {
        let filter = CapabilityFilter::from_scopes(&["*".to_string(), "-tool:dangerous".to_string()]);
        assert!(filter.can_use_tool("safe_tool"));
        assert!(!filter.can_use_tool("dangerous"));
    }
}
