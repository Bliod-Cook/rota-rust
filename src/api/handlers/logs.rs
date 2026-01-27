//! Log handlers
//!
//! Fixed: Uses streaming response instead of loading 10000 records to memory.

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use tokio_stream::wrappers::ReceiverStream;
use tracing::debug;

use crate::api::server::AppState;
use crate::error::RotaError;
use crate::models::LogListParams;
use crate::repository::LogRepository;

/// Query parameters for listing logs
#[derive(Debug, Deserialize, Default)]
pub struct ListLogsQuery {
    pub page: Option<i64>,
    pub limit: Option<i64>,
    pub level: Option<String>,
    pub search: Option<String>,
}

/// List logs with pagination
pub async fn list_logs(
    State(state): State<AppState>,
    Query(query): Query<ListLogsQuery>,
) -> Result<impl IntoResponse, RotaError> {
    let repo = LogRepository::new(state.db.pool().clone());

    let params = LogListParams {
        page: query.page,
        limit: query.limit,
        level: query.level,
        search: query.search,
        start_time: None,
        end_time: None,
    };

    let response = repo.list(&params).await?;
    Ok(Json(response))
}

/// Query parameters for exporting logs
#[derive(Debug, Deserialize, Default)]
pub struct ExportLogsQuery {
    pub format: Option<String>,
    pub limit: Option<i64>,
}

/// Export logs as CSV/JSON stream
///
/// FIXED: Uses streaming response instead of loading all records to memory.
/// The Go implementation loaded up to 10000 records into memory at once.
pub async fn export_logs(
    State(state): State<AppState>,
    Query(query): Query<ExportLogsQuery>,
) -> Result<Response, RotaError> {
    let format = query.format.as_deref().unwrap_or("csv");
    let limit = query.limit.unwrap_or(1000).clamp(1, 10000);

    debug!("Exporting logs as {} (limit: {})", format, limit);

    let repo = LogRepository::new(state.db.pool().clone());

    // Fetch logs in batches and stream
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, std::io::Error>>(32);

    let content_type = match format {
        "json" => "application/json",
        _ => "text/csv",
    };

    let format_owned = format.to_string();

    // Spawn task to fetch and stream logs
    tokio::spawn(async move {
        // For CSV, send header first
        if format_owned == "csv" {
            let header = "timestamp,level,message,details\n";
            let _ = tx.send(Ok(header.to_string())).await;
        } else {
            let _ = tx.send(Ok("[".to_string())).await;
        }

        let page_size = 100i64;
        let mut fetched = 0i64;
        let mut page = 1i64;
        let mut first = true;

        while fetched < limit {
            let remaining = (limit - fetched).min(page_size);
            let params = LogListParams {
                page: Some(page),
                limit: Some(remaining),
                level: None,
                search: None,
                start_time: None,
                end_time: None,
            };

            match repo.list(&params).await {
                Ok(response) => {
                    if response.data.is_empty() {
                        break;
                    }

                    for log in &response.data {
                        let line = if format_owned == "csv" {
                            format!(
                                "{},{},{},{}\n",
                                log.timestamp,
                                log.level,
                                log.message.replace(',', ";").replace('\n', " "),
                                log.details
                                    .as_deref()
                                    .unwrap_or("")
                                    .replace(',', ";")
                                    .replace('\n', " "),
                            )
                        } else {
                            let prefix = if first { "" } else { "," };
                            first = false;
                            format!(
                                "{}{}",
                                prefix,
                                serde_json::to_string(&log).unwrap_or_default()
                            )
                        };

                        if tx.send(Ok(line)).await.is_err() {
                            return;
                        }
                    }

                    fetched += response.data.len() as i64;
                    page += 1;
                }
                Err(e) => {
                    let _ = tx
                        .send(Err(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            e.to_string(),
                        )))
                        .await;
                    return;
                }
            }
        }

        // Close JSON array
        if format_owned == "json" {
            let _ = tx.send(Ok("]".to_string())).await;
        }
    });

    let stream = ReceiverStream::new(rx);
    let body = Body::from_stream(stream);

    let filename = format!(
        "logs-{}.{}",
        chrono::Utc::now().format("%Y%m%d-%H%M%S"),
        format
    );

    Ok(Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(body)
        .unwrap())
}
