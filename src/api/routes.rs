//! API route definitions

use axum::routing::{delete, get, post, put};
use axum::Router;

use super::handlers;
use super::server::AppState;
use super::websocket;

/// Create the API router with all routes
pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Health check (no auth required)
        .route("/health", get(handlers::health::health_check))
        .route("/api/status", get(handlers::health::status))
        // Temporary compatibility: forward /api/v1/* to /api/*
        .route("/api/v1/status", get(handlers::health::status))
        // Auth routes
        .route("/api/auth/login", post(handlers::auth::login))
        // Temporary compatibility: forward /api/v1/* to /api/*
        .route("/api/v1/auth/login", post(handlers::auth::login))
        // Protected routes
        .nest("/api", protected_routes())
        // Temporary compatibility: forward /api/v1/* to /api/*
        .nest("/api/v1", protected_routes())
        .with_state(state)
}

/// Routes that require authentication
fn protected_routes() -> Router<AppState> {
    Router::new()
        // Proxy management
        .route("/proxies", get(handlers::proxy::list_proxies))
        .route("/proxies", post(handlers::proxy::create_proxy))
        .route("/proxies/bulk", post(handlers::proxy::bulk_create_proxies))
        .route("/proxies/:id", get(handlers::proxy::get_proxy))
        .route("/proxies/:id", put(handlers::proxy::update_proxy))
        .route("/proxies/:id", delete(handlers::proxy::delete_proxy))
        .route("/proxies/:id/toggle", post(handlers::proxy::toggle_proxy))
        // Deleted proxies archive
        .route(
            "/deleted_proxies",
            get(handlers::deleted_proxy::list_deleted_proxies),
        )
        .route(
            "/deleted_proxies/:id",
            delete(handlers::deleted_proxy::delete_deleted_proxy),
        )
        .route(
            "/deleted_proxies/:id/restore",
            post(handlers::deleted_proxy::restore_deleted_proxy),
        )
        // Settings
        .route("/settings", get(handlers::settings::get_settings))
        .route("/settings", put(handlers::settings::update_settings))
        // Logs
        .route("/logs", get(handlers::logs::list_logs))
        .route("/logs/export", get(handlers::logs::export_logs))
        // Dashboard
        .route("/dashboard/stats", get(handlers::dashboard::get_stats))
        .route("/dashboard/chart", get(handlers::dashboard::get_chart_data))
        .route(
            "/dashboard/system",
            get(handlers::dashboard::get_system_metrics),
        )
        // WebSocket endpoints
        .route("/ws/dashboard", get(websocket::dashboard::dashboard_ws))
        .route("/ws/logs", get(websocket::logs::logs_ws))
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::Body;
    use axum::http::{header, Method, Request, StatusCode};
    use serde_json::json;
    use sqlx::postgres::PgPoolOptions;
    use std::sync::Arc;
    use std::time::Instant;
    use tokio::sync::{broadcast, watch};
    use tower::ServiceExt;

    use crate::config::{
        AdminConfig, ApiServerConfig, Config, DatabaseConfig, LogConfig, ProxyServerConfig,
    };
    use crate::database::Database;
    use crate::models::{RequestRecord, Settings};
    use crate::proxy::middleware::RateLimiter;
    use crate::proxy::rotation::{create_selector, DynamicProxySelector, RotationStrategy};

    fn test_state() -> AppState {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://rota:rota_password@localhost:5432/rota")
            .expect("failed to create lazy PgPool");

        let config = Config {
            proxy: ProxyServerConfig {
                port: 8000,
                host: "127.0.0.1".to_string(),
                max_retries: 3,
                connect_timeout: 10,
                request_timeout: 30,
                auth_enabled: false,
                auth_username: "".to_string(),
                auth_password: "".to_string(),
                rate_limit_enabled: false,
                rate_limit_per_second: 100,
                rate_limit_burst: 200,
                rotation_strategy: "random".to_string(),
                egress_proxy: None,
            },
            api: ApiServerConfig {
                port: 8001,
                host: "127.0.0.1".to_string(),
                cors_origins: Vec::new(),
                jwt_secret: "test-secret".to_string(),
            },
            database: DatabaseConfig {
                host: "localhost".to_string(),
                port: 5432,
                user: "rota".to_string(),
                password: "rota_password".to_string(),
                name: "rota".to_string(),
                ssl_mode: "disable".to_string(),
                max_connections: 1,
                min_connections: 0,
            },
            admin: AdminConfig {
                username: "admin".to_string(),
                password: "admin".to_string(),
            },
            log: LogConfig {
                level: "info".to_string(),
                format: "json".to_string(),
            },
        };

        let (log_sender, _) = broadcast::channel::<RequestRecord>(1);
        let (settings_tx, _) = watch::channel(Settings::default());

        let base_selector = Arc::from(create_selector(RotationStrategy::Random));
        let selector = Arc::new(DynamicProxySelector::new(base_selector));

        AppState {
            db: Database::from_pool(pool),
            config: config.clone(),
            jwt_auth: crate::api::middleware::JwtAuth::new(&config.api.jwt_secret),
            started_at: Instant::now(),
            selector,
            log_sender,
            settings_tx,
            rate_limiter: RateLimiter::disabled(),
        }
    }

    #[tokio::test]
    async fn test_api_v1_login_route_exists() {
        let app = create_router(test_state());

        let body = json!({
            "username": "nope",
            "password": "nope",
        })
        .to_string();

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/auth/login")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_api_v1_ws_route_is_registered() {
        let app = create_router(test_state());

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/ws/logs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_ne!(response.status(), StatusCode::NOT_FOUND);
    }
}
