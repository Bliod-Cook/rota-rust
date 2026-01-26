//! Dashboard handlers

use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use sysinfo::System;
use tracing::debug;

use crate::api::server::AppState;
use crate::error::RotaError;
use crate::models::{ChartTimeRange, SystemMetrics};
use crate::repository::DashboardRepository;

/// Get dashboard statistics
pub async fn get_stats(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, RotaError> {
    let repo = DashboardRepository::new(state.db.pool().clone());
    let stats = repo.get_stats().await?;
    Ok(Json(stats))
}

/// Query parameters for chart data
#[derive(Debug, Deserialize, Default)]
pub struct ChartQuery {
    pub range: Option<String>,
}

/// Get chart data for dashboard
pub async fn get_chart_data(
    State(state): State<AppState>,
    Query(query): Query<ChartQuery>,
) -> Result<impl IntoResponse, RotaError> {
    let repo = DashboardRepository::new(state.db.pool().clone());

    let time_range = ChartTimeRange {
        range: query.range,
        start: None,
        end: None,
    };

    debug!("Fetching chart data for range={:?}", time_range.range);

    let chart_data = repo.get_request_chart(&time_range).await?;
    Ok(Json(chart_data))
}

/// Get system metrics
pub async fn get_system_metrics() -> Result<impl IntoResponse, RotaError> {
    let mut sys = System::new_all();
    sys.refresh_all();

    // Calculate CPU usage from all CPUs
    let cpus = sys.cpus();
    let cpu_usage = if cpus.is_empty() {
        0.0
    } else {
        cpus.iter().map(|cpu| cpu.cpu_usage() as f64).sum::<f64>() / cpus.len() as f64
    };

    let total_memory = sys.total_memory();
    let used_memory = sys.used_memory();
    let memory_usage = if total_memory > 0 {
        (used_memory as f64 / total_memory as f64) * 100.0
    } else {
        0.0
    };

    let metrics = SystemMetrics {
        cpu_usage,
        memory_usage,
        memory_total: total_memory,
        memory_used: used_memory,
        uptime: System::uptime(),
        active_connections: 0, // Would need to track this separately
    };

    Ok(Json(metrics))
}
