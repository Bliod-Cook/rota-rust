//! JWT authentication middleware
//!
//! Fixed: Uses OsRng with fail-fast instead of insecure time-based fallback.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, error};

/// JWT claims
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// Subject (user identifier)
    pub sub: String,
    /// Expiration time (Unix timestamp)
    pub exp: i64,
    /// Issued at (Unix timestamp)
    pub iat: i64,
}

impl Claims {
    /// Create new claims for a user
    pub fn new(user_id: &str, expiry_hours: i64) -> Self {
        let now = Utc::now();
        Self {
            sub: user_id.to_string(),
            exp: (now + Duration::hours(expiry_hours)).timestamp(),
            iat: now.timestamp(),
        }
    }
}

/// JWT authentication handler
#[derive(Clone)]
pub struct JwtAuth {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
}

impl JwtAuth {
    /// Create a new JWT auth handler
    ///
    /// If the secret is empty, generates a secure random secret.
    /// FIXED: Uses OsRng with panic on failure instead of insecure time-based fallback.
    pub fn new(secret: &str) -> Self {
        let key = if secret.is_empty() {
            // Generate secure random key
            let mut key_bytes = [0u8; 32];
            OsRng
                .try_fill_bytes(&mut key_bytes)
                .expect("FATAL: Failed to generate secure random JWT key. System entropy may be unavailable.");

            debug!("Generated random JWT secret");
            key_bytes.to_vec()
        } else {
            secret.as_bytes().to_vec()
        };

        Self {
            encoding_key: EncodingKey::from_secret(&key),
            decoding_key: DecodingKey::from_secret(&key),
        }
    }

    /// Generate a JWT token for the given user
    pub fn generate_token(&self, user_id: &str, expiry_hours: i64) -> Result<String, AuthError> {
        let claims = Claims::new(user_id, expiry_hours);

        encode(&Header::default(), &claims, &self.encoding_key)
            .map_err(|e| {
                error!("Failed to generate JWT: {}", e);
                AuthError::TokenCreation
            })
    }

    /// Validate a JWT token and return the claims
    pub fn validate_token(&self, token: &str) -> Result<Claims, AuthError> {
        let validation = Validation::default();

        decode::<Claims>(token, &self.decoding_key, &validation)
            .map(|data| data.claims)
            .map_err(|e| {
                debug!("JWT validation failed: {}", e);
                AuthError::InvalidToken
            })
    }

    /// Extract token from Authorization header
    pub fn extract_token(authorization: &str) -> Option<&str> {
        if authorization.starts_with("Bearer ") {
            Some(&authorization[7..])
        } else {
            None
        }
    }
}

/// Authentication error types
#[derive(Debug)]
pub enum AuthError {
    WrongCredentials,
    TokenCreation,
    InvalidToken,
    MissingToken,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AuthError::WrongCredentials => (StatusCode::UNAUTHORIZED, "Invalid credentials"),
            AuthError::TokenCreation => (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create token"),
            AuthError::InvalidToken => (StatusCode::UNAUTHORIZED, "Invalid token"),
            AuthError::MissingToken => (StatusCode::UNAUTHORIZED, "Missing authorization token"),
        };

        (status, Json(json!({ "error": message }))).into_response()
    }
}

/// Extractor for authenticated requests
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: String,
    pub claims: Claims,
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
{
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Get Authorization header
        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(AuthError::MissingToken)?;

        // Extract token
        let token = JwtAuth::extract_token(auth_header).ok_or(AuthError::InvalidToken)?;

        // Get JWT auth from extensions (set by middleware)
        let jwt_auth = parts
            .extensions
            .get::<JwtAuth>()
            .ok_or(AuthError::InvalidToken)?;

        // Validate token
        let claims = jwt_auth.validate_token(token)?;

        Ok(AuthenticatedUser {
            user_id: claims.sub.clone(),
            claims,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jwt_generation_and_validation() {
        let auth = JwtAuth::new("test-secret");

        let token = auth.generate_token("user123", 24).unwrap();
        let claims = auth.validate_token(&token).unwrap();

        assert_eq!(claims.sub, "user123");
    }

    #[test]
    fn test_jwt_random_secret() {
        // Should not panic
        let auth = JwtAuth::new("");
        let token = auth.generate_token("user123", 24).unwrap();
        let claims = auth.validate_token(&token).unwrap();

        assert_eq!(claims.sub, "user123");
    }

    #[test]
    fn test_jwt_invalid_token() {
        let auth = JwtAuth::new("test-secret");

        let result = auth.validate_token("invalid.token.here");
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[test]
    fn test_jwt_expired_token() {
        let auth = JwtAuth::new("test-secret");

        // Expired 1 hour ago
        let token = auth.generate_token("user123", -1).unwrap();
        let result = auth.validate_token(&token);
        assert!(matches!(result, Err(AuthError::InvalidToken)));
    }

    #[test]
    fn test_extract_token() {
        assert_eq!(
            JwtAuth::extract_token("Bearer abc123"),
            Some("abc123")
        );
        assert_eq!(JwtAuth::extract_token("abc123"), None);
    }
}
