//! HTTP server middleware

pub mod auth;
pub mod rate_limit;
pub mod security;
pub mod size_limit;

pub use auth::{
    auth_middleware, scope_validation_middleware, AuthMiddlewareState, ScopeValidationState,
    get_session,
};
pub use rate_limit::{rate_limit_middleware, RateLimitConfig, RateLimitManager, create_rate_limit_layer};
pub use security::{
    security_headers_middleware, SecurityHeadersConfig, FrameOptions, HstsConfig,
    XssProtection, ReferrerPolicy, permissive_cors, restrictive_cors,
};
pub use size_limit::{size_limit_middleware, SizeLimitConfig, SizeLimitError};;
