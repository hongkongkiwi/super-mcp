use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum McpError {
    #[error("server not found: {0}")]
    ServerNotFound(String),

    #[error("sandbox error: {0}")]
    SandboxError(String),

    #[error("transport error: {0}")]
    TransportError(String),

    #[error("authentication error: {0}")]
    AuthError(String),

    #[error("authorization error: {0}")]
    AuthorizationError(String),

    #[error("configuration error: {0}")]
    ConfigError(String),

    #[error("timeout after {0}ms")]
    Timeout(u64),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("internal error: {0}")]
    InternalError(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("installation error: {0}")]
    InstallError(String),
}

impl From<anyhow::Error> for McpError {
    fn from(e: anyhow::Error) -> Self {
        McpError::InternalError(e.to_string())
    }
}

impl From<dialoguer::Error> for McpError {
    fn from(e: dialoguer::Error) -> Self {
        McpError::InstallError(e.to_string())
    }
}

impl McpError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::ServerNotFound(_) => StatusCode::NOT_FOUND,
            Self::AuthError(_) => StatusCode::UNAUTHORIZED,
            Self::AuthorizationError(_) => StatusCode::FORBIDDEN,
            Self::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            Self::Timeout(_) => StatusCode::GATEWAY_TIMEOUT,
            Self::TransportError(_) => StatusCode::BAD_GATEWAY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn error_code(&self) -> &'static str {
        match self {
            Self::ServerNotFound(_) => "SERVER_NOT_FOUND",
            Self::SandboxError(_) => "SANDBOX_ERROR",
            Self::TransportError(_) => "TRANSPORT_ERROR",
            Self::AuthError(_) => "AUTHENTICATION_ERROR",
            Self::AuthorizationError(_) => "AUTHORIZATION_ERROR",
            Self::ConfigError(_) => "CONFIG_ERROR",
            Self::Timeout(_) => "TIMEOUT",
            Self::InvalidRequest(_) => "INVALID_REQUEST",
            Self::InternalError(_) => "INTERNAL_ERROR",
            Self::Io(_) => "IO_ERROR",
            Self::Serialization(_) => "SERIALIZATION_ERROR",
            Self::InstallError(_) => "INSTALL_ERROR",
        }
    }
}

impl IntoResponse for McpError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = Json(json!({
            "error": self.error_code(),
            "message": self.to_string(),
        }));

        (status, body).into_response()
    }
}

pub type McpResult<T> = Result<T, McpError>;
