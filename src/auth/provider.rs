//! Authentication provider trait
use crate::utils::errors::McpResult;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Session data for authenticated users
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub user_id: String,
    pub token: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Token pair for authentication
#[derive(Debug, Clone)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
}

/// Authentication provider trait
#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// Validate an access token and return session info
    async fn validate_token(&self,
        token: &str,
    ) -> McpResult<Session>;

    /// Refresh an access token using a refresh token
    async fn refresh_token(
        &self,
        refresh_token: &str,
    ) -> McpResult<Tokens>;

    /// Generate a new token pair (for internal use)
    async fn generate_token(
        &self,
        user_id: &str,
        scopes: Vec<String>,
    ) -> McpResult<Tokens>;

    /// Check if the provider is properly configured
    fn is_configured(&self) -> bool;
}
