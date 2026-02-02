//! Request/Response size limit middleware

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

/// Size limit configuration
#[derive(Debug, Clone, Copy)]
pub struct SizeLimitConfig {
    /// Maximum request body size in bytes
    pub max_request_size: usize,
    /// Maximum response body size in bytes
    pub max_response_size: usize,
    /// Maximum header size in bytes
    pub max_header_size: usize,
}

impl Default for SizeLimitConfig {
    fn default() -> Self {
        Self {
            max_request_size: 10 * 1024 * 1024,  // 10 MB
            max_response_size: 50 * 1024 * 1024, // 50 MB
            max_header_size: 64 * 1024,          // 64 KB
        }
    }
}

/// Size limit error
#[derive(Debug)]
pub enum SizeLimitError {
    RequestTooLarge { size: usize, limit: usize },
    ResponseTooLarge { size: usize, limit: usize },
    HeadersTooLarge { size: usize, limit: usize },
}

impl std::fmt::Display for SizeLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SizeLimitError::RequestTooLarge { size, limit } => {
                write!(f, "Request body too large: {} bytes (limit: {} bytes)", size, limit)
            }
            SizeLimitError::ResponseTooLarge { size, limit } => {
                write!(f, "Response body too large: {} bytes (limit: {} bytes)", size, limit)
            }
            SizeLimitError::HeadersTooLarge { size, limit } => {
                write!(f, "Headers too large: {} bytes (limit: {} bytes)", size, limit)
            }
        }
    }
}

impl std::error::Error for SizeLimitError {}

impl IntoResponse for SizeLimitError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            SizeLimitError::RequestTooLarge { .. } => {
                (StatusCode::PAYLOAD_TOO_LARGE, "REQUEST_TOO_LARGE", self.to_string())
            }
            SizeLimitError::ResponseTooLarge { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, "RESPONSE_TOO_LARGE", self.to_string())
            }
            SizeLimitError::HeadersTooLarge { .. } => {
                (StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE, "HEADERS_TOO_LARGE", self.to_string())
            }
        };

        let body = axum::Json(json!({
            "error": code,
            "message": message,
        }));

        (status, body).into_response()
    }
}

/// Size limit middleware
pub async fn size_limit_middleware(
    State(config): State<SizeLimitConfig>,
    request: Request,
    next: Next,
) -> Result<Response, SizeLimitError> {
    // Check headers size
    let headers_size = request
        .headers()
        .iter()
        .map(|(k, v)| k.as_str().len() + v.len())
        .sum::<usize>();

    if headers_size > config.max_header_size {
        return Err(SizeLimitError::HeadersTooLarge {
            size: headers_size,
            limit: config.max_header_size,
        });
    }

    // Check content-length header if present
    if let Some(content_length) = request.headers().get("content-length") {
        if let Ok(length_str) = content_length.to_str() {
            if let Ok(length) = length_str.parse::<usize>() {
                if length > config.max_request_size {
                    return Err(SizeLimitError::RequestTooLarge {
                        size: length,
                        limit: config.max_request_size,
                    });
                }
            }
        }
    }

    // Process request and check response size
    let response = next.run(request).await;

    // Check response body size
    // Note: This is a simplified check. In production, you'd want to check the actual body size.
    // For now, we rely on the content-length header.
    if let Some(content_length) = response.headers().get("content-length") {
        if let Ok(length_str) = content_length.to_str() {
            if let Ok(length) = length_str.parse::<usize>() {
                if length > config.max_response_size {
                    return Err(SizeLimitError::ResponseTooLarge {
                        size: length,
                        limit: config.max_response_size,
                    });
                }
            }
        }
    }

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_size_limit_config_default() {
        let config = SizeLimitConfig::default();
        assert_eq!(config.max_request_size, 10 * 1024 * 1024);
        assert_eq!(config.max_response_size, 50 * 1024 * 1024);
        assert_eq!(config.max_header_size, 64 * 1024);
    }

    #[test]
    fn test_size_limit_error_display() {
        let err = SizeLimitError::RequestTooLarge {
            size: 100,
            limit: 50,
        };
        assert!(err.to_string().contains("Request body too large"));
    }
}
