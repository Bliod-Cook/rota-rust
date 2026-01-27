//! Dashboard WebSocket handler
//!
//! Provides real-time dashboard updates.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use super::WS_BUFFER_SIZE;
use crate::api::server::AppState;
use crate::models::DashboardStats;
use crate::repository::DashboardRepository;

/// WebSocket handler for dashboard updates
pub async fn dashboard_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_dashboard_ws(socket, state))
}

/// Handle WebSocket connection for dashboard
async fn handle_dashboard_ws(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<DashboardStats>(WS_BUFFER_SIZE);

    info!("Dashboard WebSocket connected");

    // Spawn task to fetch and send dashboard updates
    let db = state.db.clone();
    let mut fetch_task = tokio::spawn(async move {
        let mut update_interval = interval(Duration::from_secs(2));

        loop {
            update_interval.tick().await;

            let repo = DashboardRepository::new(db.pool().clone());
            match repo.get_stats().await {
                Ok(stats) => {
                    // Use try_send to avoid blocking - fixes memory leak from Go
                    match tx.try_send(stats) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            debug!("Dashboard WebSocket buffer full, dropping update");
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => {
                            break;
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch dashboard stats: {}", e);
                }
            }
        }
    });

    // Spawn task to send updates to WebSocket
    let mut send_task = tokio::spawn(async move {
        while let Some(stats) = rx.recv().await {
            match serde_json::to_string(&stats) {
                Ok(json) => {
                    if sender.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    error!("Failed to serialize dashboard stats: {}", e);
                }
            }
        }
    });

    // Handle incoming messages (mainly for ping/pong and close)
    let mut receive_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Close(_)) => {
                    debug!("Dashboard WebSocket received close");
                    break;
                }
                Ok(Message::Ping(_data)) => {
                    debug!("Dashboard WebSocket ping received");
                    // Pong is handled automatically by axum
                }
                Err(e) => {
                    debug!("Dashboard WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });

    // Wait for any task to complete
    tokio::select! {
        _ = &mut fetch_task => {}
        _ = &mut send_task => {}
        _ = &mut receive_task => {}
    }

    fetch_task.abort();
    send_task.abort();
    receive_task.abort();
    let _ = tokio::join!(fetch_task, send_task, receive_task);

    info!("Dashboard WebSocket disconnected");
}
