use crate::auth::{AuthProvider, JwtAuth, OAuthAuth, StaticTokenAuth};
use crate::config::{AuthConfig, AuthType, Config};
use crate::core::ServerManager;
use crate::http_server::middleware::{
    auth_middleware, create_rate_limit_layer, security_headers_middleware, size_limit_middleware,
    AuthMiddlewareState, RateLimitConfig as HttpRateLimitConfig, ScopeValidationState,
    SecurityHeadersConfig, SizeLimitConfig,
};
use crate::http_server::routes;
use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use jsonwebtoken::Algorithm;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

pub struct HttpServer {
    config: Config,
    server_manager: Arc<ServerManager>,
}

impl HttpServer {
    pub fn new(config: Config, server_manager: Arc<ServerManager>) -> Self {
        Self {
            config,
            server_manager,
        }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let app = self.create_router().await?;

        let addr = SocketAddr::from((
            self.config.server.host.parse::<std::net::IpAddr>()?,
            self.config.server.port,
        ));

        info!("Starting HTTP server on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .await?;

        Ok(())
    }

    async fn create_router(&self) -> anyhow::Result<Router> {
        let server_manager = self.server_manager.clone();

        let mut mcp_router = Router::new()
            .route("/mcp", post(routes::mcp_handler))
            .route("/mcp/:server", post(routes::server_handler))
            .with_state(server_manager);

        // Rate limiting
        let rate_limit_config = HttpRateLimitConfig {
            requests_per_minute: self.config.rate_limit.requests_per_minute,
            burst_size: self.config.rate_limit.burst_size,
        };
        mcp_router = mcp_router.layer(create_rate_limit_layer(&rate_limit_config));

        // Size limits
        let size_limit_config = SizeLimitConfig::default();
        mcp_router = mcp_router.layer(middleware::from_fn_with_state(
            size_limit_config,
            size_limit_middleware,
        ));

        // Authentication and scope validation
        if self.config.features.auth {
            if self.config.features.scope_validation && !self.config.auth.required_scopes.is_empty()
            {
                let scope_state = Arc::new(ScopeValidationState {
                    required_scopes: self.config.auth.required_scopes.clone(),
                });
                mcp_router = mcp_router.layer(middleware::from_fn_with_state(
                    scope_state,
                    crate::http_server::middleware::scope_validation_middleware,
                ));
            }

            let provider = build_auth_provider(&self.config.auth).await?;
            let auth_state = Arc::new(AuthMiddlewareState::new(provider, true));
            mcp_router = mcp_router.layer(middleware::from_fn_with_state(
                auth_state,
                auth_middleware,
            ));
        }

        let mut app = Router::new()
            .route("/health", get(routes::health))
            .merge(mcp_router);

        // Security headers for all responses
        let security_config = SecurityHeadersConfig::default();
        app = app.layer(middleware::from_fn_with_state(
            security_config,
            security_headers_middleware,
        ));

        Ok(app)
    }
}

async fn build_auth_provider(auth: &AuthConfig) -> anyhow::Result<Arc<dyn AuthProvider>> {
    fn parse_algorithms(algs: &[String]) -> anyhow::Result<Vec<Algorithm>> {
        let mut parsed = Vec::new();
        for alg in algs {
            let normalized = alg.trim().to_ascii_uppercase();
            let parsed_alg = match normalized.as_str() {
                "HS256" => Algorithm::HS256,
                "HS384" => Algorithm::HS384,
                "HS512" => Algorithm::HS512,
                "RS256" => Algorithm::RS256,
                "RS384" => Algorithm::RS384,
                "RS512" => Algorithm::RS512,
                "ES256" => Algorithm::ES256,
                "ES384" => Algorithm::ES384,
                "PS256" => Algorithm::PS256,
                "PS384" => Algorithm::PS384,
                "PS512" => Algorithm::PS512,
                "EDDSA" => Algorithm::EdDSA,
                _ => {
                    return Err(anyhow::anyhow!(format!(
                        "Unsupported JWT algorithm: {}",
                        alg
                    )))
                }
            };
            parsed.push(parsed_alg);
        }
        Ok(parsed)
    }

    match auth.auth_type {
        AuthType::None => Err(anyhow::anyhow!(
            "auth.type is none but features.auth is enabled"
        )),
        AuthType::Static => {
            let token = auth
                .token
                .clone()
                .ok_or_else(|| anyhow::anyhow!("auth.token is required for static auth"))?;
            Ok(Arc::new(StaticTokenAuth::new(token)))
        }
        AuthType::Jwt => {
            let secret = auth
                .jwt_secret
                .clone()
                .ok_or_else(|| anyhow::anyhow!("auth.jwt_secret is required for jwt auth"))?;
            let issuer = auth
                .issuer
                .clone()
                .ok_or_else(|| anyhow::anyhow!("auth.issuer is required for jwt auth"))?;
            Ok(Arc::new(JwtAuth::new(secret).with_issuer(issuer)))
        }
        AuthType::OAuth => {
            let client_id = auth
                .client_id
                .clone()
                .ok_or_else(|| anyhow::anyhow!("auth.client_id is required for oauth auth"))?;
            let client_secret = auth
                .client_secret
                .clone()
                .ok_or_else(|| anyhow::anyhow!("auth.client_secret is required for oauth auth"))?;

            let mut oauth = if let Some(issuer) = auth.issuer.clone() {
                OAuthAuth::from_discovery(client_id, client_secret, issuer)
                    .await
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?
            } else {
                let auth_url = auth
                    .auth_url
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("auth.auth_url is required for oauth auth"))?;
                let token_url = auth
                    .token_url
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("auth.token_url is required for oauth auth"))?;
                OAuthAuth::new(client_id, client_secret, auth_url, token_url)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?
            };

            if let Some(url) = auth.introspection_url.clone() {
                oauth = oauth.with_introspection_url(url);
            }
            if let Some(url) = auth.userinfo_url.clone() {
                oauth = oauth.with_userinfo_url(url);
            }
            if let Some(url) = auth.jwks_url.clone() {
                oauth = oauth.with_jwks_url(url);
            }
            if !auth.expected_audiences.is_empty() {
                oauth = oauth.with_expected_audiences(auth.expected_audiences.clone());
            }
            if !auth.allowed_algs.is_empty() {
                oauth = oauth.with_allowed_algs(parse_algorithms(&auth.allowed_algs)?);
            }

            oauth = oauth.with_jwks_cache_ttl(Duration::from_secs(
                auth.jwks_cache_ttl_seconds,
            ));

            oauth = oauth.with_allow_unverified_jwt(auth.allow_unverified_jwt);

            Ok(Arc::new(oauth))
        }
    }
}
