//! Logs WebSocket handler
//!
//! Provides real-time log streaming.
//! FIXED: Uses bounded channels with try_send to prevent memory leaks.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use super::WS_BUFFER_SIZE;
use crate::api::server::AppState;
use crate::models::RequestRecord;

/// WebSocket handler for log streaming
pub async fn logs_ws(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_logs_ws(socket, state))
}

/// Handle WebSocket connection for logs
async fn handle_logs_ws(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<RequestRecord>(WS_BUFFER_SIZE);

    info!("Logs WebSocket connected");

    // Subscribe to log broadcasts
    let mut log_rx = state.log_sender.subscribe();

    // Spawn task to receive broadcasts and forward to channel
    let forward_task = tokio::spawn(async move {
        loop {
            match log_rx.recv().await {
                Ok(record) => {
                    // Use try_send to avoid blocking - fixes memory leak from Go
                    if tx.try_send(record).is_err() {
                        debug!("Logs WebSocket buffer full, dropping log entry");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!("Logs WebSocket lagged, missed {} messages", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    debug!("Log broadcast channel closed");
                    break;
                }
            }
        }
    });

    // Spawn task to send logs to WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(record) = rx.recv().await {
            match serde_json::to_string(&record) {
                Ok(json) => {
                    if sender.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    error!("Failed to serialize log record: {}", e);
                }
            }
        }
    });

    // Handle incoming messages (mainly for ping/pong and close)
    let receive_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Close(_)) => {
                    debug!("Logs WebSocket received close");
                    break;
                }
                Ok(Message::Ping(_)) => {
                    debug!("Logs WebSocket ping received");
                    // Pong is handled automatically by axum
                }
                Err(e) => {
                    debug!("Logs WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }
    });

    // Wait for any task to complete
    tokio::select! {
        _ = forward_task => {
            debug!("Forward task ended");
        }
        _ = send_task => {
            debug!("Send task ended");
        }
        _ = receive_task => {
            debug!("Receive task ended");
        }
    }

    info!("Logs WebSocket disconnected");
}
