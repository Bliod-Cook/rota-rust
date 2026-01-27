use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

/// Unified error type for the Rota application
#[derive(Error, Debug)]
pub enum RotaError {
    // Database errors
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Database connection failed: {0}")]
    DatabaseConnection(String),

    // Proxy errors
    #[error("No proxies available")]
    NoProxiesAvailable,

    #[error("Proxy connection failed: {0}")]
    ProxyConnectionFailed(String),

    #[error("All proxies exhausted after {attempts} attempts")]
    AllProxiesExhausted { attempts: u32 },

    #[error("Proxy not found: {id}")]
    ProxyNotFound { id: i32 },

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid proxy address: {0}")]
    InvalidProxyAddress(String),

    #[error("Unsupported proxy protocol: {0}")]
    UnsupportedProtocol(String),

    // Tunnel errors
    #[error("Tunnel error: {0}")]
    TunnelError(String),

    #[error("CONNECT failed: {0}")]
    ConnectFailed(String),

    // Authentication errors
    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("JWT error: {0}")]
    JwtError(#[from] jsonwebtoken::errors::Error),

    #[error("Missing authorization header")]
    MissingAuthHeader,

    #[error("Invalid authorization header format")]
    InvalidAuthHeader,

    // Rate limiting
    #[error("Rate limit exceeded for {client_ip}")]
    RateLimitExceeded { client_ip: String },

    // Configuration errors
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Missing environment variable: {0}")]
    MissingEnvVar(String),

    // Request errors
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Request timeout")]
    RequestTimeout,

    #[error("Operation timed out")]
    Timeout,

    // I/O errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    // HTTP errors
    #[error("HTTP error: {0}")]
    Http(String),

    // Settings errors
    #[error("Settings not found: {key}")]
    SettingsNotFound { key: String },

    // Internal errors
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type alias for Rota operations
pub type Result<T> = std::result::Result<T, RotaError>;

impl RotaError {
    /// Get the HTTP status code for this error
    pub fn status_code(&self) -> StatusCode {
        match self {
            // 400 Bad Request
            RotaError::InvalidRequest(_)
            | RotaError::InvalidProxyAddress(_)
            | RotaError::UnsupportedProtocol(_)
            | RotaError::InvalidConfig(_) => StatusCode::BAD_REQUEST,

            // 401 Unauthorized
            RotaError::AuthenticationFailed
            | RotaError::InvalidCredentials
            | RotaError::MissingAuthHeader
            | RotaError::InvalidAuthHeader
            | RotaError::JwtError(_) => StatusCode::UNAUTHORIZED,

            // 404 Not Found
            RotaError::ProxyNotFound { .. }
            | RotaError::SettingsNotFound { .. }
            | RotaError::NotFound(_) => StatusCode::NOT_FOUND,

            // Timeout
            RotaError::Timeout => StatusCode::GATEWAY_TIMEOUT,

            // 429 Too Many Requests
            RotaError::RateLimitExceeded { .. } => StatusCode::TOO_MANY_REQUESTS,

            // 502 Bad Gateway
            RotaError::ProxyConnectionFailed(_)
            | RotaError::TunnelError(_)
            | RotaError::ConnectFailed(_)
            | RotaError::AllProxiesExhausted { .. } => StatusCode::BAD_GATEWAY,

            // 503 Service Unavailable
            RotaError::NoProxiesAvailable | RotaError::DatabaseConnection(_) => {
                StatusCode::SERVICE_UNAVAILABLE
            }

            // 504 Gateway Timeout
            RotaError::RequestTimeout => StatusCode::GATEWAY_TIMEOUT,

            // 500 Internal Server Error
            RotaError::Database(_)
            | RotaError::Io(_)
            | RotaError::Http(_)
            | RotaError::MissingEnvVar(_)
            | RotaError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Check if this is a client error (4xx)
    pub fn is_client_error(&self) -> bool {
        self.status_code().is_client_error()
    }

    /// Check if this is a server error (5xx)
    pub fn is_server_error(&self) -> bool {
        self.status_code().is_server_error()
    }
}

// Implement IntoResponse for API error responses
impl IntoResponse for RotaError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = json!({
            "error": self.to_string(),
        });

        (status, Json(body)).into_response()
    }
}

// Convert from hyper errors
impl From<hyper::Error> for RotaError {
    fn from(err: hyper::Error) -> Self {
        RotaError::Http(err.to_string())
    }
}

// Convert from URL parse errors
impl From<url::ParseError> for RotaError {
    fn from(err: url::ParseError) -> Self {
        RotaError::InvalidProxyAddress(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_status_code_mapping() {
        assert_eq!(
            RotaError::InvalidRequest("bad".to_string()).status_code(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            RotaError::InvalidProxyAddress("bad".to_string()).status_code(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            RotaError::AuthenticationFailed.status_code(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            RotaError::ProxyNotFound { id: 1 }.status_code(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            RotaError::RateLimitExceeded {
                client_ip: "127.0.0.1".to_string()
            }
            .status_code(),
            StatusCode::TOO_MANY_REQUESTS
        );
        assert_eq!(
            RotaError::Timeout.status_code(),
            StatusCode::GATEWAY_TIMEOUT
        );
        assert_eq!(
            RotaError::NoProxiesAvailable.status_code(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[test]
    fn test_error_client_server_helpers() {
        assert!(RotaError::InvalidRequest("bad".to_string()).is_client_error());
        assert!(!RotaError::InvalidRequest("bad".to_string()).is_server_error());

        assert!(RotaError::NoProxiesAvailable.is_server_error());
        assert!(!RotaError::NoProxiesAvailable.is_client_error());
    }
}
