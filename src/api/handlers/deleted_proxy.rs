//! Deleted proxy management handlers

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use tracing::info;

use crate::api::server::AppState;
use crate::error::RotaError;
use crate::models::DeletedProxyListParams;
use crate::proxy::rotation::ProxySelector;
use crate::repository::{DeletedProxyRepository, ProxyRepository};

/// Query parameters for listing deleted proxies
#[derive(Debug, Deserialize, Default)]
pub struct ListDeletedProxiesQuery {
    pub page: Option<i64>,
    pub limit: Option<i64>,
}

/// List deleted proxies
pub async fn list_deleted_proxies(
    State(state): State<AppState>,
    Query(query): Query<ListDeletedProxiesQuery>,
) -> Result<impl IntoResponse, RotaError> {
    let repo = DeletedProxyRepository::new(state.db.pool().clone());

    let params = DeletedProxyListParams {
        page: query.page,
        limit: query.limit,
    };

    let response = repo.list(&params).await?;
    Ok(Json(response))
}

/// Permanently delete a deleted proxy record
pub async fn delete_deleted_proxy(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, RotaError> {
    let repo = DeletedProxyRepository::new(state.db.pool().clone());

    let deleted = repo.delete(id).await?;

    if deleted {
        info!(id = id, "Deleted deleted proxy record");
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(RotaError::NotFound(format!(
            "Deleted proxy with id {} not found",
            id
        )))
    }
}

/// Restore a deleted proxy
pub async fn restore_deleted_proxy(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<impl IntoResponse, RotaError> {
    let repo = DeletedProxyRepository::new(state.db.pool().clone());

    let restored = repo.restore(id).await?;

    match restored {
        Some(proxy) => {
            // Refresh selector to include restored proxy
            refresh_selector(&state).await?;
            info!(id = proxy.id, address = %proxy.address, "Restored proxy");
            Ok(Json(proxy))
        }
        None => Err(RotaError::NotFound(format!(
            "Deleted proxy with id {} not found",
            id
        ))),
    }
}

async fn refresh_selector(state: &AppState) -> Result<(), RotaError> {
    let repo = ProxyRepository::new(state.db.pool().clone());
    let remove_unhealthy = state.settings_tx.borrow().rotation.remove_unhealthy;
    let proxies = if remove_unhealthy {
        repo.get_all_usable().await?
    } else {
        repo.get_all().await?
    };
    state.selector.refresh(proxies).await?;
    Ok(())
}
