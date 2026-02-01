//! Static token authentication provider
use crate::auth::provider::{AuthProvider, Session, Tokens};
use crate::utils::errors::{McpError, McpResult};
use async_trait::async_trait;

/// Static token authentication (for development/simple deployments)
pub struct StaticTokenAuth {
    token: String,
    user_id: String,
    scopes: Vec<String>,
}

impl StaticTokenAuth {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            user_id: "admin".to_string(),
            scopes: vec!["*".to_string()], // Full access
        }
    }

    pub fn with_user(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = user_id.into();
        self
    }

    pub fn with_scopes(mut self, scopes: Vec<String>) -> Self {
        self.scopes = scopes;
        self
    }
}

#[async_trait]
impl AuthProvider for StaticTokenAuth {
    async fn validate_token(
        &self,
        token: &str,
    ) -> McpResult<Session> {
        if token == self.token {
            Ok(Session {
                user_id: self.user_id.clone(),
                token: token.to_string(),
                scopes: self.scopes.clone(),
                expires_at: None, // Static tokens don't expire
            })
        } else {
            Err(McpError::AuthError("Invalid token".to_string()))
        }
    }

    async fn refresh_token(
        &self,
        _refresh_token: &str,
    ) -> McpResult<Tokens> {
        // Static tokens don't support refresh
        Err(McpError::AuthError("Token refresh not supported".to_string()))
    }

    async fn generate_token(
        &self,
        _user_id: &str,
        _scopes: Vec<String>,
    ) -> McpResult<Tokens> {
        Err(McpError::AuthError("Token generation not supported".to_string()))
    }

    fn is_configured(&self) -> bool {
        !self.token.is_empty()
    }
}
