//! Authentication middleware for HTTP server

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::sync::Arc;

use crate::auth::provider::{AuthProvider, Session};
use crate::utils::errors::McpError;

/// Extract authentication token from request headers
fn extract_token(request: &Request) -> Option<String> {
    request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value.strip_prefix("Bearer ").map(|v| v.to_string())
        })
}

/// Authentication middleware state
pub struct AuthMiddlewareState {
    pub provider: Arc<dyn AuthProvider>,
    pub required: bool,
}

impl AuthMiddlewareState {
    pub fn new(provider: Arc<dyn AuthProvider>, required: bool) -> Self {
        Self { provider, required }
    }
}

/// Authentication middleware that validates Bearer tokens
pub async fn auth_middleware(
    State(state): State<Arc<AuthMiddlewareState>>,
    mut request: Request,
    next: Next,
) -> Response {
    // Try to extract and validate token
    match extract_token(&request) {
        Some(token) => {
            match state.provider.validate_token(&token).await {
                Ok(session) => {
                    // Store session in request extensions for downstream handlers
                    request.extensions_mut().insert(session);
                    next.run(request).await
                }
                Err(e) => {
                    if state.required {
                        let error = McpError::AuthError(format!("Invalid token: {}", e));
                        error.into_response()
                    } else {
                        // Auth not required, continue with anonymous session
                        next.run(request).await
                    }
                }
            }
        }
        None => {
            if state.required {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "error": "AUTHENTICATION_REQUIRED",
                        "message": "Authorization header with Bearer token is required"
                    }))
                )
                    .into_response()
            } else {
                // Auth not required, continue without session
                next.run(request).await
            }
        }
    }
}

/// Scope validation middleware state
pub struct ScopeValidationState {
    pub required_scopes: Vec<String>,
}

/// Middleware to validate scopes from session
pub async fn scope_validation_middleware(
    State(state): State<Arc<ScopeValidationState>>,
    request: Request,
    next: Next,
) -> Response {
    // Get session from extensions
    let has_required_scope = request
        .extensions()
        .get::<Session>()
        .map(|session| {
            // Check if session has any of the required scopes
            // Wildcard (*) grants all scopes
            if session.scopes.contains(&"*".to_string()) {
                return true;
            }
            
            state.required_scopes.iter().any(|required| {
                session.scopes.contains(required)
            })
        })
        .unwrap_or(false);

    if !state.required_scopes.is_empty() && !has_required_scope {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "INSUFFICIENT_SCOPE",
                "message": "Token does not have required scopes"
            }))
        )
            .into_response();
    }

    next.run(request).await
}

/// Extract session from request extensions
pub fn get_session(request: &Request) -> Option<&Session> {
    request.extensions().get::<Session>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    #[test]
    fn test_extract_token_valid() {
        let request = Request::builder()
            .uri("/test")
            .header(header::AUTHORIZATION, "Bearer test-token-123")
            .body(Body::empty())
            .unwrap();

        let token = extract_token(&request);
        assert_eq!(token, Some("test-token-123".to_string()));
    }

    #[test]
    fn test_extract_token_missing() {
        let request = Request::builder()
            .uri("/test")
            .body(Body::empty())
            .unwrap();

        let token = extract_token(&request);
        assert_eq!(token, None);
    }

    #[test]
    fn test_extract_token_invalid_format() {
        let request = Request::builder()
            .uri("/test")
            .header(header::AUTHORIZATION, "Basic dXNlcjpwYXNz")
            .body(Body::empty())
            .unwrap();

        let token = extract_token(&request);
        assert_eq!(token, None);
    }
}
