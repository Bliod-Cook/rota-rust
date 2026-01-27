//! Proxy management handlers

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use tracing::info;

use crate::api::server::AppState;
use crate::error::RotaError;
use crate::models::{CreateProxyRequest, ProxyListParams, UpdateProxyRequest};
use crate::repository::ProxyRepository;

/// Query parameters for listing proxies
#[derive(Debug, Deserialize, Default)]
pub struct ListProxiesQuery {
    pub page: Option<i64>,
    pub limit: Option<i64>,
    pub search: Option<String>,
    pub status: Option<String>,
    pub protocol: Option<String>,
    pub sort_field: Option<String>,
    pub sort_order: Option<String>,
}

/// List all proxies
pub async fn list_proxies(
    State(state): State<AppState>,
    Query(query): Query<ListProxiesQuery>,
) -> Result<impl IntoResponse, RotaError> {
    let repo = ProxyRepository::new(state.db.pool().clone());

    let params = ProxyListParams {
        page: query.page,
        limit: query.limit,
        search: query.search,
        status: query.status,
        protocol: query.protocol,
        sort_field: query.sort_field,
        sort_order: query.sort_order,
    };

    let response = repo.list(&params).await?;
    Ok(Json(response))
}

/// Get a single proxy
pub async fn get_proxy(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, RotaError> {
    let repo = ProxyRepository::new(state.db.pool().clone());
    let proxy = repo.get_by_id(id).await?;

    match proxy {
        Some(p) => Ok(Json(p)),
        None => Err(RotaError::NotFound(format!(
            "Proxy with id {} not found",
            id
        ))),
    }
}

/// Create a new proxy
pub async fn create_proxy(
    State(state): State<AppState>,
    Json(req): Json<CreateProxyRequest>,
) -> Result<impl IntoResponse, RotaError> {
    let repo = ProxyRepository::new(state.db.pool().clone());

    // Validate request
    if req.address.is_empty() {
        return Err(RotaError::InvalidRequest("Address is required".to_string()));
    }

    let proxy = repo.create(&req).await?;

    // Refresh selector with new proxy list
    let proxies = repo.get_all_usable().await?;
    state.selector.refresh(proxies).await?;

    info!(id = proxy.id, address = %proxy.address, "Created proxy");

    Ok((StatusCode::CREATED, Json(proxy)))
}

/// Update a proxy
pub async fn update_proxy(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(req): Json<UpdateProxyRequest>,
) -> Result<impl IntoResponse, RotaError> {
    let repo = ProxyRepository::new(state.db.pool().clone());

    let proxy = repo.update(id, &req).await?;

    match proxy {
        Some(p) => {
            // Refresh selector with updated proxy list
            let proxies = repo.get_all_usable().await?;
            state.selector.refresh(proxies).await?;

            info!(id = p.id, address = %p.address, "Updated proxy");
            Ok(Json(p))
        }
        None => Err(RotaError::NotFound(format!(
            "Proxy with id {} not found",
            id
        ))),
    }
}

/// Delete a proxy
pub async fn delete_proxy(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, RotaError> {
    let repo = ProxyRepository::new(state.db.pool().clone());

    let deleted = repo.delete(id).await?;

    if deleted {
        // Refresh selector
        let proxies = repo.get_all_usable().await?;
        state.selector.refresh(proxies).await?;

        info!(id = id, "Deleted proxy");
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(RotaError::NotFound(format!(
            "Proxy with id {} not found",
            id
        )))
    }
}

/// Toggle proxy status
pub async fn toggle_proxy(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, RotaError> {
    let repo = ProxyRepository::new(state.db.pool().clone());

    // Get current proxy
    let proxy = repo.get_by_id(id).await?;

    match proxy {
        Some(p) => {
            // Toggle status: active <-> idle, failed -> idle
            let new_status = match p.status.as_str() {
                "active" => "idle",
                "idle" => "active",
                "failed" => "idle",
                _ => "idle",
            };

            let update_req = UpdateProxyRequest {
                address: None,
                protocol: None,
                username: None,
                password: None,
                status: Some(new_status.to_string()),
            };

            let updated = repo.update(id, &update_req).await?;

            match updated {
                Some(updated_proxy) => {
                    // Refresh selector
                    let proxies = repo.get_all_usable().await?;
                    state.selector.refresh(proxies).await?;

                    info!(
                        id = updated_proxy.id,
                        status = %updated_proxy.status,
                        "Toggled proxy status"
                    );
                    Ok(Json(updated_proxy))
                }
                None => Err(RotaError::NotFound(format!(
                    "Proxy with id {} not found",
                    id
                ))),
            }
        }
        None => Err(RotaError::NotFound(format!(
            "Proxy with id {} not found",
            id
        ))),
    }
}
