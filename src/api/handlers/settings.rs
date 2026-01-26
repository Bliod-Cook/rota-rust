//! Settings handlers

use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use tracing::info;

use crate::api::server::AppState;
use crate::error::RotaError;
use crate::models::Settings;
use crate::repository::SettingsRepository;

/// Get all settings
pub async fn get_settings(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, RotaError> {
    let repo = SettingsRepository::new(state.db.pool().clone());
    let settings = repo.get_all().await?;
    Ok(Json(settings))
}

/// Update settings
pub async fn update_settings(
    State(state): State<AppState>,
    Json(settings): Json<Settings>,
) -> Result<impl IntoResponse, RotaError> {
    let repo = SettingsRepository::new(state.db.pool().clone());
    repo.update_all(&settings).await?;

    info!("Settings updated");

    Ok(Json(settings))
}
