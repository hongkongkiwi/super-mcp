//! OAuth 2.1 authentication provider
use crate::auth::provider::{AuthProvider, Session, Tokens};
use crate::utils::errors::{McpError, McpResult};
use async_trait::async_trait;
use chrono::Utc;
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use oauth2::{
    basic::BasicClient, reqwest::async_http_client, AuthUrl, AuthorizationCode, ClientId,
    ClientSecret, CsrfToken, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info};

/// OAuth 2.1 authentication provider
pub struct OAuthAuth {
    client: Arc<BasicClient>,
    introspection_url: Option<String>,
    userinfo_url: Option<String>,
    allow_unverified_jwt: bool,
    jwks_url: Option<String>,
    jwks_cache: Arc<RwLock<Option<JwksCache>>>,
    jwks_cache_ttl: Duration,
    expected_audiences: Vec<String>,
    allowed_algs: Vec<Algorithm>,
    issuer: Option<String>,
}

#[derive(Debug, Clone)]
struct JwksCache {
    fetched_at: Instant,
    jwks: Arc<JwkSet>,
}

/// OAuth token introspection response
#[derive(Debug, Deserialize)]
struct TokenIntrospectionResponse {
    active: bool,
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    exp: Option<i64>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    client_id: Option<String>,
}

/// OAuth userinfo response
#[derive(Debug, Deserialize)]
struct UserInfoResponse {
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

impl OAuthAuth {
    /// Create a new OAuth authentication provider
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
            introspection_url: None,
            userinfo_url: None,
            allow_unverified_jwt: false,
            jwks_url: None,
            jwks_cache: Arc::new(RwLock::new(None)),
            jwks_cache_ttl: Duration::from_secs(300),
            expected_audiences: Vec::new(),
            allowed_algs: Vec::new(),
            issuer: None,
        })
    }

    /// Create a new OAuth provider with endpoints from well-known discovery
    pub async fn from_discovery(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        issuer_url: impl Into<String>,
    ) -> McpResult<Self> {
        let issuer = issuer_url.into();
        let discovery_url = format!("{}/.well-known/openid-configuration", issuer.trim_end_matches('/'));
        
        debug!("Fetching OAuth discovery from: {}", discovery_url);
        
        let client = reqwest::Client::new();
        let response = client
            .get(&discovery_url)
            .send()
            .await
            .map_err(|e| McpError::TransportError(format!("Discovery request failed: {}", e)))?;
        
        if !response.status().is_success() {
            return Err(McpError::TransportError(format!(
                "Discovery returned error: {}",
                response.status()
            )));
        }
        
        let discovery: serde_json::Value = response
            .json()
            .await
            .map_err(|e| McpError::InternalError(format!("Failed to parse discovery response: {}", e)))?;
        
        let auth_url = discovery
            .get("authorization_endpoint")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::ConfigError("Missing authorization_endpoint in discovery".to_string()))?;
        
        let token_url = discovery
            .get("token_endpoint")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::ConfigError("Missing token_endpoint in discovery".to_string()))?;
        
        let introspection_url = discovery
            .get("introspection_endpoint")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        
        let userinfo_url = discovery
            .get("userinfo_endpoint")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let jwks_url = discovery
            .get("jwks_uri")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let issuer_from_discovery = discovery
            .get("issuer")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| issuer.clone());
        
        let mut oauth = Self::new(client_id, client_secret, auth_url, token_url)?;
        oauth.introspection_url = introspection_url;
        oauth.userinfo_url = userinfo_url;
        oauth.jwks_url = jwks_url;
        oauth.issuer = Some(issuer_from_discovery);
        
        info!("OAuth provider configured from discovery endpoint");
        Ok(oauth)
    }

    /// Set the redirect URL
    pub fn with_redirect_url(mut self, url: impl Into<String>) -> McpResult<Self> {
        let client = Arc::get_mut(&mut self.client).unwrap();
        *client = client
            .clone()
            .set_redirect_uri(
                RedirectUrl::new(url.into()).map_err(|e| McpError::ConfigError(e.to_string()))?,
            );
        Ok(self)
    }

    /// Set the token introspection URL
    pub fn with_introspection_url(mut self, url: impl Into<String>) -> Self {
        self.introspection_url = Some(url.into());
        self
    }

    /// Set the userinfo URL
    pub fn with_userinfo_url(mut self, url: impl Into<String>) -> Self {
        self.userinfo_url = Some(url.into());
        self
    }

    /// Set the JWKS URL for JWT validation
    pub fn with_jwks_url(mut self, url: impl Into<String>) -> Self {
        self.jwks_url = Some(url.into());
        self
    }

    /// Set expected audiences for JWT validation
    pub fn with_expected_audiences(mut self, audiences: Vec<String>) -> Self {
        self.expected_audiences = audiences;
        self
    }

    /// Set allowed JWT algorithms for JWKS validation
    pub fn with_allowed_algs(mut self, algs: Vec<Algorithm>) -> Self {
        self.allowed_algs = algs;
        self
    }

    /// Set JWKS cache TTL
    pub fn with_jwks_cache_ttl(mut self, ttl: Duration) -> Self {
        self.jwks_cache_ttl = ttl;
        self
    }

    /// Set expected token issuer for JWT validation
    pub fn with_issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = Some(issuer.into());
        self
    }

    /// Allow unverified JWT parsing (unsafe; for dev/testing only)
    pub fn with_allow_unverified_jwt(mut self, allow: bool) -> Self {
        self.allow_unverified_jwt = allow;
        self
    }

    /// Generate authorization URL for OAuth flow
    pub fn get_authorization_url(&self, scopes: Vec<String>) -> (String, CsrfToken) {
        let mut request = self.client.authorize_url(CsrfToken::new_random);

        for scope in scopes {
            request = request.add_scope(Scope::new(scope));
        }

        let (url, csrf) = request.url();
        (url.to_string(), csrf)
    }

    /// Exchange authorization code for tokens
    pub async fn exchange_code(&self, code: &str) -> McpResult<Tokens> {
        let code = AuthorizationCode::new(code.to_string());

        let token_response = self
            .client
            .exchange_code(code)
            .request_async(async_http_client)
            .await
            .map_err(|e| McpError::AuthError(format!("Token exchange failed: {}", e)))?;

        Ok(Tokens {
            access_token: token_response.access_token().secret().to_string(),
            refresh_token: token_response.refresh_token().map(|t| t.secret().to_string()),
            expires_in: token_response.expires_in().map(|d| d.as_secs() as i64),
        })
    }

    /// Introspect a token to get detailed information
    async fn introspect_token(&self, token: &str) -> McpResult<TokenIntrospectionResponse> {
        let url = self
            .introspection_url
            .as_ref()
            .ok_or_else(|| McpError::AuthError("Token introspection not configured".to_string()))?;

        let client = reqwest::Client::new();
        let params = [
            ("token", token),
            ("token_type_hint", "access_token"),
        ];

        let response = client
            .post(url)
            .form(&params)
            .send()
            .await
            .map_err(|e| McpError::TransportError(format!("Introspection request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(McpError::AuthError(format!(
                "Introspection returned error: {}",
                response.status()
            )));
        }

        let introspection: TokenIntrospectionResponse = response
            .json()
            .await
            .map_err(|e| McpError::InternalError(format!("Failed to parse introspection response: {}", e)))?;

        Ok(introspection)
    }

    /// Get user information from userinfo endpoint
    async fn get_userinfo(&self, token: &str) -> McpResult<UserInfoResponse> {
        let url = self
            .userinfo_url
            .as_ref()
            .ok_or_else(|| McpError::AuthError("Userinfo endpoint not configured".to_string()))?;

        let client = reqwest::Client::new();
        let response = client
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| McpError::TransportError(format!("Userinfo request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(McpError::AuthError(format!(
                "Userinfo returned error: {}",
                response.status()
            )));
        }

        let userinfo: UserInfoResponse = response
            .json()
            .await
            .map_err(|e| McpError::InternalError(format!("Failed to parse introspection response: {}", e)))?;

        Ok(userinfo)
    }

    async fn fetch_jwks(&self) -> McpResult<Arc<JwkSet>> {
        let url = self
            .jwks_url
            .as_ref()
            .ok_or_else(|| McpError::AuthError("JWKS URL not configured".to_string()))?;

        let client = reqwest::Client::new();
        let response = client
            .get(url)
            .send()
            .await
            .map_err(|e| McpError::TransportError(format!("JWKS request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(McpError::AuthError(format!(
                "JWKS returned error: {}",
                response.status()
            )));
        }

        let jwks: JwkSet = response
            .json()
            .await
            .map_err(|e| McpError::InternalError(format!("Failed to parse JWKS response: {}", e)))?;

        Ok(Arc::new(jwks))
    }

    async fn get_jwks(&self, force_refresh: bool) -> McpResult<Arc<JwkSet>> {
        if !force_refresh && !self.jwks_cache_ttl.is_zero() {
            let cache = self.jwks_cache.read().await;
            if let Some(cached) = cache.as_ref() {
                if cached.fetched_at.elapsed() < self.jwks_cache_ttl {
                    return Ok(Arc::clone(&cached.jwks));
                }
            }
        }

        let jwks = self.fetch_jwks().await?;
        let mut cache = self.jwks_cache.write().await;
        *cache = Some(JwksCache {
            fetched_at: Instant::now(),
            jwks: Arc::clone(&jwks),
        });
        Ok(jwks)
    }

    fn build_jwt_validation(&self, allowed_algs: &[Algorithm]) -> Validation {
        let mut validation = Validation::new(
            *allowed_algs
                .first()
                .unwrap_or(&Algorithm::RS256),
        );
        validation.algorithms = allowed_algs.to_vec();

        if let Some(issuer) = &self.issuer {
            if !issuer.is_empty() {
                validation.set_issuer(std::slice::from_ref(issuer));
            }
        }

        if !self.expected_audiences.is_empty() {
            validation.set_audience(&self.expected_audiences);
        }

        validation
    }

    async fn validate_with_jwks(
        &self,
        token: &str,
        header: &jsonwebtoken::Header,
    ) -> McpResult<Session> {
        let allowed_algs = if self.allowed_algs.is_empty() {
            vec![Algorithm::RS256]
        } else {
            self.allowed_algs.clone()
        };

        if !allowed_algs.contains(&header.alg) {
            return Err(McpError::AuthError(format!(
                "Disallowed JWT algorithm: {:?}",
                header.alg
            )));
        }

        let validation = self.build_jwt_validation(&allowed_algs);

        let jwks = self.get_jwks(false).await?;
        let claims = if let Some(kid) = header.kid.as_deref() {
            if let Some(jwk) = jwks.find(kid) {
                Self::decode_with_jwk(token, jwk, &validation)?
            } else {
                let refreshed = self.get_jwks(true).await?;
                let jwk = refreshed.find(kid).ok_or_else(|| {
                    McpError::AuthError(format!("No matching JWK found for kid {}", kid))
                })?;
                Self::decode_with_jwk(token, jwk, &validation)?
            }
        } else {
            if jwks.keys.len() != 1 {
                return Err(McpError::AuthError(
                    "JWT kid is missing and JWKS has multiple keys".to_string(),
                ));
            }
            let jwk = jwks.keys.first().ok_or_else(|| {
                McpError::AuthError("JWKS did not contain any keys".to_string())
            })?;
            Self::decode_with_jwk(token, jwk, &validation)?
        };

        let (user_id, scopes) = Self::extract_from_claims(&claims);
        let expires_at = Self::extract_expiration(&claims);

        Ok(Session {
            user_id,
            token: token.to_string(),
            scopes,
            expires_at,
        })
    }

    fn decode_with_jwk(
        token: &str,
        jwk: &jsonwebtoken::jwk::Jwk,
        validation: &Validation,
    ) -> McpResult<serde_json::Value> {
        let key = DecodingKey::from_jwk(jwk)
            .map_err(|e| McpError::AuthError(format!("Invalid JWK: {}", e)))?;
        let data = decode::<serde_json::Value>(token, &key, validation)
            .map_err(|e| McpError::AuthError(format!("JWT verification failed: {}", e)))?;
        Ok(data.claims)
    }

    /// Parse JWT token without verification (for extracting claims)
    fn parse_jwt_claims(token: &str) -> Option<serde_json::Value> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return None;
        }

        // Decode base64 payload
        use base64::Engine;
        let payload = parts[1];
        let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .ok()?;

        serde_json::from_slice(&decoded).ok()
    }

    /// Extract user_id and scopes from JWT claims
    fn extract_from_claims(claims: &serde_json::Value) -> (String, Vec<String>) {
        let user_id = claims
            .get("sub")
            .and_then(|v| v.as_str())
            .or_else(|| claims.get("client_id").and_then(|v| v.as_str()))
            .unwrap_or("unknown")
            .to_string();

        let scopes = claims
            .get("scope")
            .and_then(|v| v.as_str())
            .map(|s| s.split_whitespace().map(|s| s.to_string()).collect())
            .or_else(|| {
                claims
                    .get("scp")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
            })
            .unwrap_or_default();

        (user_id, scopes)
    }

    /// Extract expiration from JWT claims
    fn extract_expiration(claims: &serde_json::Value) -> Option<chrono::DateTime<Utc>> {
        claims
            .get("exp")
            .and_then(|v| v.as_i64())
            .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
    }
}

#[async_trait]
impl AuthProvider for OAuthAuth {
    async fn validate_token(&self, token: &str) -> McpResult<Session> {
        debug!("Validating OAuth token");

        if self.jwks_url.is_some() {
            match decode_header(token) {
                Ok(header) => {
                    let session = self.validate_with_jwks(token, &header).await?;
                    return Ok(session);
                }
                Err(_) => {
                    debug!("Token is not a JWT; skipping JWKS validation");
                }
            }
        }

        // Try introspection first if available
        if self.introspection_url.is_some() {
            let introspection = self.introspect_token(token).await?;
            if !introspection.active {
                return Err(McpError::AuthError("Token is not active".to_string()));
            }

            let user_id = introspection
                .sub
                .or(introspection.username)
                .or(introspection.client_id)
                .unwrap_or_else(|| "unknown".to_string());

            let scopes = introspection
                .scope
                .map(|s| s.split_whitespace().map(|s| s.to_string()).collect())
                .unwrap_or_default();

            let expires_at = introspection
                .exp
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0));

            return Ok(Session {
                user_id,
                token: token.to_string(),
                scopes,
                expires_at,
            });
        }

        // Fallback: userinfo endpoint (validates token on provider side)
        if self.userinfo_url.is_some() {
            let userinfo = self.get_userinfo(token).await?;
            let user_id = userinfo
                .sub
                .or(userinfo.preferred_username)
                .or(userinfo.email)
                .or(userinfo.name)
                .unwrap_or_else(|| "unknown".to_string());

            return Ok(Session {
                user_id,
                token: token.to_string(),
                scopes: Vec::new(),
                expires_at: None,
            });
        }

        if self.allow_unverified_jwt {
            let claims = Self::parse_jwt_claims(token)
                .ok_or_else(|| McpError::AuthError("Invalid token format".to_string()))?;

            let (user_id, scopes) = Self::extract_from_claims(&claims);
            let expires_at = Self::extract_expiration(&claims);

            if let Some(exp) = expires_at {
                if exp < Utc::now() {
                    return Err(McpError::AuthError("Token has expired".to_string()));
                }
            }

            return Ok(Session {
                user_id,
                token: token.to_string(),
                scopes,
                expires_at,
            });
        }

        Err(McpError::AuthError(
            "No OAuth token validation method configured".to_string(),
        ))
    }

    async fn refresh_token(&self, refresh_token: &str) -> McpResult<Tokens> {
        let refresh_token = RefreshToken::new(refresh_token.to_string());

        let token_response = self
            .client
            .exchange_refresh_token(&refresh_token)
            .request_async(async_http_client)
            .await
            .map_err(|e| McpError::AuthError(format!("Token refresh failed: {}", e)))?;

        Ok(Tokens {
            access_token: token_response.access_token().secret().to_string(),
            refresh_token: token_response.refresh_token().map(|t| t.secret().to_string()),
            expires_in: token_response.expires_in().map(|d| d.as_secs() as i64),
        })
    }

    async fn generate_token(&self, _user_id: &str, _scopes: Vec<String>) -> McpResult<Tokens> {
        Err(McpError::AuthError(
            "Token generation not supported for OAuth. Use the OAuth flow instead.".to_string(),
        ))
    }

    fn is_configured(&self) -> bool {
        // OAuth is configured if client is initialized
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_jwt_claims() {
        // Create a simple JWT payload
        let claims = serde_json::json!({
            "sub": "user123",
            "scope": "read write",
            "exp": 1234567890
        });

        // Encode as base64 (simplified test)
        use base64::Engine;
        let payload = serde_json::to_string(&claims).unwrap();
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.as_bytes());
        
        let jwt = format!("header.{}.signature", encoded);
        
        let parsed = OAuthAuth::parse_jwt_claims(&jwt);
        assert!(parsed.is_some());
        
        let parsed_claims = parsed.unwrap();
        assert_eq!(parsed_claims.get("sub").unwrap(), "user123");
    }

    #[test]
    fn test_extract_from_claims() {
        let claims = serde_json::json!({
            "sub": "user123",
            "scope": "read write admin"
        });

        let (user_id, scopes) = OAuthAuth::extract_from_claims(&claims);
        assert_eq!(user_id, "user123");
        assert_eq!(scopes, vec!["read", "write", "admin"]);
    }

    #[test]
    fn test_extract_expiration() {
        let now = Utc::now().timestamp();
        let claims = serde_json::json!({
            "exp": now + 3600
        });

        let exp = OAuthAuth::extract_expiration(&claims);
        assert!(exp.is_some());
    }
}
