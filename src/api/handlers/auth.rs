//! Authentication handlers

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::api::middleware::AuthError;
use crate::api::server::AppState;

/// Login request
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// Login response
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub expires_in: i64,
}

/// Handle login request
pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<impl IntoResponse, AuthError> {
    // Validate credentials against configured admin
    if req.username != state.config.admin.username || req.password != state.config.admin.password {
        warn!("Login failed for user: {}", req.username);
        return Err(AuthError::WrongCredentials);
    }

    // Generate JWT token
    let expiry_hours = 24;
    let token = state.jwt_auth.generate_token(&req.username, expiry_hours)?;

    info!("User {} logged in successfully", req.username);

    Ok((
        StatusCode::OK,
        Json(LoginResponse {
            token,
            expires_in: expiry_hours * 3600,
        }),
    ))
}
