//! Security headers middleware
//!
//! Adds security headers to all HTTP responses to protect against
//! common web vulnerabilities.

use axum::{
    body::Body,
    extract::Request,
    http::header::{self, HeaderValue},
    middleware::Next,
    response::Response,
};

/// Security headers configuration
#[derive(Debug, Clone)]
pub struct SecurityHeadersConfig {
    /// Content Security Policy
    pub content_security_policy: Option<String>,
    /// X-Frame-Options
    pub frame_options: FrameOptions,
    /// X-Content-Type-Options
    pub content_type_options: bool,
    /// Strict-Transport-Security (HSTS)
    pub hsts: Option<HstsConfig>,
    /// X-XSS-Protection
    pub xss_protection: XssProtection,
    /// Referrer-Policy
    pub referrer_policy: ReferrerPolicy,
    /// Permissions-Policy
    pub permissions_policy: Option<String>,
}

impl Default for SecurityHeadersConfig {
    fn default() -> Self {
        Self {
            content_security_policy: Some(
                "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline';".to_string()
            ),
            frame_options: FrameOptions::Deny,
            content_type_options: true,
            hsts: Some(HstsConfig::default()),
            xss_protection: XssProtection::Block,
            referrer_policy: ReferrerPolicy::StrictOriginWhenCrossOrigin,
            permissions_policy: Some(
                "camera=(), microphone=(), geolocation=(), payment=()".to_string()
            ),
        }
    }
}

/// X-Frame-Options header values
#[derive(Debug, Clone)]
pub enum FrameOptions {
    Deny,
    SameOrigin,
    AllowFrom(String),
}

impl FrameOptions {
    fn to_header_value(&self) -> HeaderValue {
        match self {
            FrameOptions::Deny => HeaderValue::from_static("DENY"),
            FrameOptions::SameOrigin => HeaderValue::from_static("SAMEORIGIN"),
            FrameOptions::AllowFrom(origin) => {
                HeaderValue::from_str(&format!("ALLOW-FROM {}", origin)).unwrap_or_else(|_| {
                    HeaderValue::from_static("DENY")
                })
            }
        }
    }
}

/// HSTS configuration
#[derive(Debug, Clone)]
pub struct HstsConfig {
    /// Max age in seconds
    pub max_age: u64,
    /// Include subdomains
    pub include_subdomains: bool,
    /// Enable preload
    pub preload: bool,
}

impl Default for HstsConfig {
    fn default() -> Self {
        Self {
            max_age: 31536000, // 1 year
            include_subdomains: true,
            preload: false,
        }
    }
}

impl HstsConfig {
    fn to_header_value(&self) -> HeaderValue {
        let mut value = format!("max-age={}", self.max_age);
        if self.include_subdomains {
            value.push_str("; includeSubDomains");
        }
        if self.preload {
            value.push_str("; preload");
        }
        HeaderValue::from_str(&value).unwrap_or_else(|_| {
            HeaderValue::from_static("max-age=31536000")
        })
    }
}

/// X-XSS-Protection header values
#[derive(Debug, Clone)]
pub enum XssProtection {
    Disable,
    Enable,
    Block,
}

impl XssProtection {
    fn to_header_value(&self) -> HeaderValue {
        match self {
            XssProtection::Disable => HeaderValue::from_static("0"),
            XssProtection::Enable => HeaderValue::from_static("1"),
            XssProtection::Block => HeaderValue::from_static("1; mode=block"),
        }
    }
}

/// Referrer-Policy header values
#[derive(Debug, Clone)]
pub enum ReferrerPolicy {
    NoReferrer,
    NoReferrerWhenDowngrade,
    Origin,
    OriginWhenCrossOrigin,
    SameOrigin,
    StrictOrigin,
    StrictOriginWhenCrossOrigin,
    UnsafeUrl,
}

impl ReferrerPolicy {
    fn to_header_value(&self) -> HeaderValue {
        let value = match self {
            ReferrerPolicy::NoReferrer => "no-referrer",
            ReferrerPolicy::NoReferrerWhenDowngrade => "no-referrer-when-downgrade",
            ReferrerPolicy::Origin => "origin",
            ReferrerPolicy::OriginWhenCrossOrigin => "origin-when-cross-origin",
            ReferrerPolicy::SameOrigin => "same-origin",
            ReferrerPolicy::StrictOrigin => "strict-origin",
            ReferrerPolicy::StrictOriginWhenCrossOrigin => "strict-origin-when-cross-origin",
            ReferrerPolicy::UnsafeUrl => "unsafe-url",
        };
        HeaderValue::from_static(value)
    }
}

/// Security headers middleware
pub async fn security_headers_middleware(
    config: SecurityHeadersConfig,
    request: Request,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    // Content Security Policy
    if let Some(csp) = config.content_security_policy {
        if let Ok(value) = HeaderValue::from_str(&csp) {
            headers.insert(header::CONTENT_SECURITY_POLICY, value);
        }
    }

    // X-Frame-Options
    headers.insert(
        header::X_FRAME_OPTIONS,
        config.frame_options.to_header_value(),
    );

    // X-Content-Type-Options
    if config.content_type_options {
        headers.insert(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
    }

    // Strict-Transport-Security (HSTS)
    if let Some(hsts) = config.hsts {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            hsts.to_header_value(),
        );
    }

    // X-XSS-Protection
    headers.insert(
        header::X_XSS_PROTECTION,
        config.xss_protection.to_header_value(),
    );

    // Referrer-Policy
    headers.insert(
        header::REFERER_POLICY,
        config.referrer_policy.to_header_value(),
    );

    // Permissions-Policy
    if let Some(permissions) = config.permissions_policy {
        if let Ok(value) = HeaderValue::from_str(&permissions) {
            headers.insert("permissions-policy", value);
        }
    }

    // Additional security headers
    headers.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    
    response
}

/// Create a permissive CORS policy for development
pub fn permissive_cors() -> tower_http::cors::CorsLayer {
    tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any)
}

/// Create a restrictive CORS policy for production
pub fn restrictive_cors(allowed_origins: Vec<String>) -> tower_http::cors::CorsLayer {
    let origins: Vec<_> = allowed_origins
        .into_iter()
        .filter_map(|origin| origin.parse().ok())
        .collect();
    
    tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::AllowOrigin::list(origins))
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
        ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_headers_config_default() {
        let config = SecurityHeadersConfig::default();
        assert!(config.content_security_policy.is_some());
        assert!(config.content_type_options);
        assert!(config.hsts.is_some());
    }

    #[test]
    fn test_frame_options_to_header_value() {
        assert_eq!(
            FrameOptions::Deny.to_header_value(),
            HeaderValue::from_static("DENY")
        );
        assert_eq!(
            FrameOptions::SameOrigin.to_header_value(),
            HeaderValue::from_static("SAMEORIGIN")
        );
    }

    #[test]
    fn test_hsts_to_header_value() {
        let hsts = HstsConfig::default();
        let value = hsts.to_header_value();
        assert!(value.to_str().unwrap().contains("max-age=31536000"));
        assert!(value.to_str().unwrap().contains("includeSubDomains"));
    }

    #[test]
    fn test_xss_protection_to_header_value() {
        assert_eq!(
            XssProtection::Disable.to_header_value(),
            HeaderValue::from_static("0")
        );
        assert_eq!(
            XssProtection::Block.to_header_value(),
            HeaderValue::from_static("1; mode=block")
        );
    }

    #[test]
    fn test_referrer_policy_to_header_value() {
        assert_eq!(
            ReferrerPolicy::NoReferrer.to_header_value(),
            HeaderValue::from_static("no-referrer")
        );
        assert_eq!(
            ReferrerPolicy::StrictOriginWhenCrossOrigin.to_header_value(),
            HeaderValue::from_static("strict-origin-when-cross-origin")
        );
    }
}
