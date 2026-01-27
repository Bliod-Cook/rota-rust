//! Health check endpoint

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;
use serde_json::json;
use sysinfo::System;

use crate::api::server::AppState;
use crate::error::RotaError;

/// Health check endpoint
pub async fn health_check() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "status": "healthy",
            "service": "rota-proxy"
        })),
    )
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    version: &'static str,
    uptime: u64,
    proxies: ProxyStatusSummary,
    requests: RequestStats,
    system: SystemStats,
}

#[derive(Debug, Serialize)]
struct ProxyStatusSummary {
    total: i64,
    active: i64,
    failed: i64,
    idle: i64,
}

#[derive(Debug, Serialize)]
struct RequestStats {
    total: i64,
    last_minute: i64,
    last_hour: i64,
}

#[derive(Debug, Serialize)]
struct SystemStats {
    cpu_usage: f64,
    memory_usage: u64,
    memory_total: u64,
}

/// Detailed status endpoint (version, uptime, proxy/request/system stats)
pub async fn status(State(state): State<AppState>) -> Result<impl IntoResponse, RotaError> {
    let pool = state.db.pool();

    let (total, active, failed, idle, total_requests): (i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"
            SELECT
                COUNT(*)::bigint AS total,
                COUNT(*) FILTER (WHERE status = 'active')::bigint AS active,
                COUNT(*) FILTER (WHERE status = 'failed')::bigint AS failed,
                COUNT(*) FILTER (WHERE status = 'idle')::bigint AS idle,
                COALESCE(SUM(requests), 0)::bigint AS total_requests
            FROM proxies
            "#,
    )
    .fetch_one(pool)
    .await?;

    let last_minute: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM proxy_requests WHERE timestamp >= NOW() - INTERVAL '1 minute'",
    )
    .fetch_one(pool)
    .await?;

    let last_hour: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM proxy_requests WHERE timestamp >= NOW() - INTERVAL '1 hour'",
    )
    .fetch_one(pool)
    .await?;

    let mut sys = System::new_all();
    sys.refresh_all();

    let cpus = sys.cpus();
    let cpu_usage = if cpus.is_empty() {
        0.0
    } else {
        cpus.iter().map(|cpu| cpu.cpu_usage() as f64).sum::<f64>() / cpus.len() as f64
    };

    let response = StatusResponse {
        version: env!("CARGO_PKG_VERSION"),
        uptime: state.started_at.elapsed().as_secs(),
        proxies: ProxyStatusSummary {
            total,
            active,
            failed,
            idle,
        },
        requests: RequestStats {
            total: total_requests,
            last_minute,
            last_hour,
        },
        system: SystemStats {
            cpu_usage,
            memory_usage: sys.used_memory(),
            memory_total: sys.total_memory(),
        },
    };

    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::response::IntoResponse;
    use http_body_util::BodyExt;

    #[tokio::test]
    async fn test_health_check_response_shape() {
        let response = health_check().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("failed to collect body")
            .to_bytes();
        let payload: serde_json::Value =
            serde_json::from_slice(&body).expect("body must be valid json");

        assert_eq!(payload.get("status").and_then(|v| v.as_str()), Some("healthy"));
        assert_eq!(
            payload.get("service").and_then(|v| v.as_str()),
            Some("rota-proxy")
        );
    }
}
