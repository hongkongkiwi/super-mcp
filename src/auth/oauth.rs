//! OAuth 2.1 authentication provider
use crate::auth::provider::{AuthProvider, Session, Tokens};
use crate::utils::errors::{McpError, McpResult};
use async_trait::async_trait;
use oauth2::{
    basic::BasicClient, AuthUrl, ClientId, ClientSecret, CsrfToken,
    RedirectUrl, Scope, TokenUrl,
};
use std::sync::Arc;

/// OAuth 2.1 authentication provider
pub struct OAuthAuth {
    client: Arc<BasicClient>,
}

impl OAuthAuth {
    pub fn new(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        auth_url: impl Into<String>,
        token_url: impl Into<String>,
    ) -> McpResult<Self> {
        let client = BasicClient::new(
            ClientId::new(client_id.into()),
            Some(ClientSecret::new(client_secret.into())),
            AuthUrl::new(auth_url.into()).map_err(|e| McpError::ConfigError(e.to_string()))?,
            Some(TokenUrl::new(token_url.into()).map_err(|e| McpError::ConfigError(e.to_string()))?),
        );

        Ok(Self {
            client: Arc::new(client),
        })
    }

    pub fn with_redirect_url(mut self, url: impl Into<String>) -> McpResult<Self> {
        let client = Arc::get_mut(&mut self.client).unwrap();
        *client = client
            .clone()
            .set_redirect_uri(
                RedirectUrl::new(url.into()).map_err(|e| McpError::ConfigError(e.to_string()))?,
            );
        Ok(self)
    }

    /// Generate authorization URL for OAuth flow
    pub fn get_authorization_url(
        &self,
        scopes: Vec<String>,
    ) -> (String, CsrfToken) {
        let mut request = self.client.authorize_url(CsrfToken::new_random);

        for scope in scopes {
            request = request.add_scope(Scope::new(scope));
        }

        let (url, csrf) = request.url();
        (url.to_string(), csrf)
    }
}

#[async_trait]
impl AuthProvider for OAuthAuth {
    async fn validate_token(
        &self,
        _token: &str,
    ) -> McpResult<Session> {
        // For OAuth, token validation typically involves introspection endpoint
        // This is a simplified implementation
        Err(McpError::AuthError("OAuth token validation not implemented".to_string()))
    }

    async fn refresh_token(
        &self,
        _refresh_token: &str,
    ) -> McpResult<Tokens> {
        Err(McpError::AuthError("OAuth refresh not implemented".to_string()))
    }

    async fn generate_token(
        &self,
        _user_id: &str,
        _scopes: Vec<String>,
    ) -> McpResult<Tokens> {
        Err(McpError::AuthError("Use OAuth flow instead".to_string()))
    }

    fn is_configured(&self) -> bool {
        // OAuth is configured if client is initialized
        true
    }
}
