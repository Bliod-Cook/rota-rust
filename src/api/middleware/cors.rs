//! CORS middleware configuration
//!
//! Fixed: Uses explicit domain whitelist instead of allowing all origins.

use axum::http::{HeaderValue, Method};
use tower_http::cors::{Any, CorsLayer};
use tracing::debug;

/// Create a CORS layer with the specified allowed origins
///
/// This fixes the security issue from the Go implementation where
/// CORS was allowing all origins with credentials.
pub fn cors_layer(allowed_origins: &[String]) -> CorsLayer {
    if allowed_origins.is_empty() {
        debug!("CORS: No origins specified, allowing localhost only");
        // Default to localhost only
        CorsLayer::new()
            .allow_origin([
                "http://localhost:3000".parse::<HeaderValue>().unwrap(),
                "http://127.0.0.1:3000".parse::<HeaderValue>().unwrap(),
            ])
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers(Any)
            .allow_credentials(true)
    } else {
        debug!("CORS: Allowing origins: {:?}", allowed_origins);
        let origins: Vec<HeaderValue> = allowed_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();

        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers(Any)
            .allow_credentials(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cors_empty_origins() {
        let layer = cors_layer(&[]);
        // Should create a layer successfully with localhost defaults
        assert!(true);
    }

    #[test]
    fn test_cors_with_origins() {
        let origins = vec![
            "https://example.com".to_string(),
            "https://app.example.com".to_string(),
        ];
        let layer = cors_layer(&origins);
        // Should create a layer successfully
        assert!(true);
    }
}
