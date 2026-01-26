//! CORS middleware configuration
//!
//! Fixed: Uses explicit domain whitelist instead of allowing all origins.

use axum::http::{HeaderValue, Method};
use axum::http::header;
use tower_http::cors::CorsLayer;
use tracing::debug;

/// Create a CORS layer with the specified allowed origins
///
/// This fixes the security issue from the Go implementation where
/// CORS was allowing all origins with credentials.
pub fn cors_layer(allowed_origins: &[String]) -> CorsLayer {
    let allowed_headers = [header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT];

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
            .allow_headers(allowed_headers)
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
            .allow_headers(allowed_headers)
            .allow_credentials(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::get;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_cors_empty_origins_allows_localhost() {
        let app = axum::Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(cors_layer(&[]));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/")
                    .header("Origin", "http://localhost:3000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap()
                .to_str()
                .unwrap(),
            "http://localhost:3000"
        );
    }

    #[tokio::test]
    async fn test_cors_empty_origins_blocks_other_origins() {
        let app = axum::Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(cors_layer(&[]));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/")
                    .header("Origin", "https://example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(response
            .headers()
            .get("access-control-allow-origin")
            .is_none());
    }

    #[tokio::test]
    async fn test_cors_with_origins_allows_configured() {
        let origins = vec![
            "https://example.com".to_string(),
            "https://app.example.com".to_string(),
        ];

        let app = axum::Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(cors_layer(&origins));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/")
                    .header("Origin", "https://app.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap()
                .to_str()
                .unwrap(),
            "https://app.example.com"
        );
    }
}
