//! Request logging middleware

use axum::body::Body;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;
use std::time::Instant;
use tracing::{debug, info};

/// Request logging middleware
pub struct RequestLogging;

impl RequestLogging {
    /// Log request details
    pub async fn log_request(req: Request<Body>, next: Next) -> Response {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let start = Instant::now();

        debug!("{} {} - started", method, uri);

        let response = next.run(req).await;

        let duration = start.elapsed();
        let status = response.status();

        info!(
            "{} {} - {} in {:?}",
            method, uri, status, duration
        );

        response
    }
}
