//! JWT authentication provider
use crate::auth::provider::{AuthProvider, Session, Tokens};
use crate::utils::errors::{McpError, McpResult};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,                    // Subject (user_id)
    scopes: Vec<String>,
    exp: i64,                       // Expiration time
    iat: i64,                       // Issued at
    jti: String,                    // JWT ID
}

/// JWT authentication provider
pub struct JwtAuth {
    secret: String,
    issuer: String,
    default_expiry: Duration,
}

impl JwtAuth {
    pub fn new(secret: impl Into<String>) -> Self {
        Self {
            secret: secret.into(),
            issuer: "super-mcp".to_string(),
            default_expiry: Duration::hours(24),
        }
    }

    pub fn with_issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = issuer.into();
        self
    }

    pub fn with_default_expiry(mut self, hours: i64) -> Self {
        self.default_expiry = Duration::hours(hours);
        self
    }
}

#[async_trait]
impl AuthProvider for JwtAuth {
    async fn validate_token(
        &self,
        token: &str,
    ) -> McpResult<Session> {
        let mut validation = Validation::default();
        validation.set_issuer(std::slice::from_ref(&self.issuer));

        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.secret.as_bytes()),
            &validation,
        )
        .map_err(|e| McpError::AuthError(format!("Invalid token: {}", e)))?;

        let claims = token_data.claims;
        let expires_at = chrono::DateTime::from_timestamp(claims.exp, 0);

        Ok(Session {
            user_id: claims.sub,
            token: token.to_string(),
            scopes: claims.scopes,
            expires_at,
        })
    }

    async fn refresh_token(
        &self,
        token: &str,
    ) -> McpResult<Tokens> {
        // For JWT, refresh just generates a new token with extended expiry
        // In a real implementation, you'd verify this is a refresh token
        let session = self.validate_token(token).await?;
        self.generate_token(&session.user_id, session.scopes).await
    }

    async fn generate_token(
        &self,
        user_id: &str,
        scopes: Vec<String>,
    ) -> McpResult<Tokens> {
        let now = Utc::now();
        let expires_at = now + self.default_expiry;

        let claims = Claims {
            sub: user_id.to_string(),
            scopes: scopes.clone(),
            exp: expires_at.timestamp(),
            iat: now.timestamp(),
            jti: Uuid::new_v4().to_string(),
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
        .map_err(|e| McpError::AuthError(format!("Token generation failed: {}", e)))?;

        Ok(Tokens {
            access_token: token,
            refresh_token: None, // Could generate separate refresh token
            expires_in: Some(self.default_expiry.num_seconds()),
        })
    }

    fn is_configured(&self) -> bool {
        !self.secret.is_empty()
    }
}
