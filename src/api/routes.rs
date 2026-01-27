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
        // Auth routes
        .route("/api/auth/login", post(handlers::auth::login))
        // Protected routes
        .nest("/api", protected_routes())
        .with_state(state)
}

/// Routes that require authentication
fn protected_routes() -> Router<AppState> {
    Router::new()
        // Proxy management
        .route("/proxies", get(handlers::proxy::list_proxies))
        .route("/proxies", post(handlers::proxy::create_proxy))
        .route("/proxies/:id", get(handlers::proxy::get_proxy))
        .route("/proxies/:id", put(handlers::proxy::update_proxy))
        .route("/proxies/:id", delete(handlers::proxy::delete_proxy))
        .route("/proxies/:id/toggle", post(handlers::proxy::toggle_proxy))
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
