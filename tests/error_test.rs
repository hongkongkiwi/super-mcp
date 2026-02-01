use axum::http::StatusCode;
use mcp_one::utils::errors::McpError;

#[test]
fn test_error_status_codes() {
    assert_eq!(
        McpError::ServerNotFound("test".to_string()).status_code(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        McpError::AuthError("test".to_string()).status_code(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        McpError::AuthorizationError("test".to_string()).status_code(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        McpError::Timeout(5000).status_code(),
        StatusCode::GATEWAY_TIMEOUT
    );
}

#[test]
fn test_error_codes() {
    assert_eq!(
        McpError::ServerNotFound("test".to_string()).error_code(),
        "SERVER_NOT_FOUND"
    );
    assert_eq!(
        McpError::SandboxError("test".to_string()).error_code(),
        "SANDBOX_ERROR"
    );
}
