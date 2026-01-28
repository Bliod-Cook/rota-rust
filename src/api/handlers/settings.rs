//! Settings handlers

use std::time::Duration;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use tracing::info;

use crate::api::server::AppState;
use crate::error::RotaError;
use crate::models::Settings;
use crate::proxy::rotation::ProxySelector;
use crate::proxy::rotation::RotationStrategy;
use crate::repository::{ProxyRepository, SettingsRepository};

/// Get all settings
pub async fn get_settings(State(state): State<AppState>) -> Result<impl IntoResponse, RotaError> {
    Ok(Json(state.settings_tx.borrow().clone()))
}

/// Update settings
pub async fn update_settings(
    State(state): State<AppState>,
    Json(settings): Json<Settings>,
) -> Result<impl IntoResponse, RotaError> {
    let repo = SettingsRepository::new(state.db.pool().clone());
    repo.update_all(&settings).await?;

    let _ = state.settings_tx.send(settings.clone());

    // Apply rate limiting immediately (proxy server uses the shared instance).
    state.rate_limiter.apply_settings(&settings.rate_limit);

    // Refresh proxies & apply rotation strategy immediately.
    let proxy_repo = ProxyRepository::new(state.db.pool().clone());
    let proxies = if settings.rotation.remove_unhealthy {
        proxy_repo.get_all_usable().await?
    } else {
        proxy_repo.get_all().await?
    };
    state.selector.refresh(proxies).await?;

    let strategy = RotationStrategy::from_str(&settings.rotation.method);
    let interval_secs = settings.rotation.time_based.interval.max(1) as u64;
    state
        .selector
        .set_strategy(strategy, Duration::from_secs(interval_secs))
        .await?;

    info!("Settings updated");

    Ok(Json(settings))
}
