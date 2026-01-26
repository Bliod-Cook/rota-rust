use crate::error::Result;
use crate::models::{CreateLogRequest, Log, LogListParams, PaginatedResponse, RequestRecord};
use sqlx::PgPool;

/// Repository for log database operations
#[derive(Clone)]
pub struct LogRepository {
    pool: PgPool,
}

impl LogRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Create a new log entry
    pub async fn create(&self, req: &CreateLogRequest) -> Result<Log> {
        let metadata = req
            .metadata
            .as_ref()
            .map(|m| serde_json::to_value(m).unwrap_or_default());

        let log = sqlx::query_as::<_, Log>(
            r#"
            INSERT INTO logs (level, message, details, metadata)
            VALUES ($1, $2, $3, $4)
            RETURNING id, timestamp, level, message, details, metadata
            "#,
        )
        .bind(req.level.as_str())
        .bind(&req.message)
        .bind(&req.details)
        .bind(metadata)
        .fetch_one(&self.pool)
        .await?;

        Ok(log)
    }

    /// List logs with pagination and filtering
    pub async fn list(&self, params: &LogListParams) -> Result<PaginatedResponse<Log>> {
        let page = params.page.unwrap_or(1).max(1);
        let limit = params.limit.unwrap_or(50).clamp(1, 100);
        let offset = (page - 1) * limit;

        // Build WHERE clause
        let mut conditions = vec!["1=1".to_string()];

        if let Some(ref level) = params.level {
            if !level.is_empty() {
                conditions.push("level = $3".to_string());
            }
        }

        if let Some(ref search) = params.search {
            if !search.is_empty() {
                conditions.push("message ILIKE $4".to_string());
            }
        }

        if params.start_time.is_some() {
            conditions.push("timestamp >= $5".to_string());
        }

        if params.end_time.is_some() {
            conditions.push("timestamp <= $6".to_string());
        }

        let where_clause = conditions.join(" AND ");

        // Count query
        let count_query = format!("SELECT COUNT(*) FROM logs WHERE {}", where_clause);

        // Data query
        let data_query = format!(
            r#"
            SELECT id, timestamp, level, message, details, metadata
            FROM logs
            WHERE {}
            ORDER BY timestamp DESC
            LIMIT $1 OFFSET $2
            "#,
            where_clause
        );

        // Build and execute count query
        let mut count_builder = sqlx::query_scalar::<_, i64>(&count_query);
        if let Some(ref level) = params.level {
            if !level.is_empty() {
                count_builder = count_builder.bind(level);
            }
        }
        if let Some(ref search) = params.search {
            if !search.is_empty() {
                count_builder = count_builder.bind(format!("%{}%", search));
            }
        }
        if let Some(start_time) = params.start_time {
            count_builder = count_builder.bind(start_time);
        }
        if let Some(end_time) = params.end_time {
            count_builder = count_builder.bind(end_time);
        }

        let total = count_builder.fetch_one(&self.pool).await.unwrap_or(0);

        // Build and execute data query
        let mut data_builder = sqlx::query_as::<_, Log>(&data_query);
        data_builder = data_builder.bind(limit).bind(offset);
        if let Some(ref level) = params.level {
            if !level.is_empty() {
                data_builder = data_builder.bind(level);
            }
        }
        if let Some(ref search) = params.search {
            if !search.is_empty() {
                data_builder = data_builder.bind(format!("%{}%", search));
            }
        }
        if let Some(start_time) = params.start_time {
            data_builder = data_builder.bind(start_time);
        }
        if let Some(end_time) = params.end_time {
            data_builder = data_builder.bind(end_time);
        }

        let logs = data_builder.fetch_all(&self.pool).await.unwrap_or_default();

        Ok(PaginatedResponse::new(logs, total, page, limit))
    }

    /// Get logs since a specific ID (for streaming)
    pub async fn get_since(&self, last_id: i64, limit: i64) -> Result<Vec<Log>> {
        let logs = sqlx::query_as::<_, Log>(
            r#"
            SELECT id, timestamp, level, message, details, metadata
            FROM logs
            WHERE id > $1
            ORDER BY id ASC
            LIMIT $2
            "#,
        )
        .bind(last_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(logs)
    }

    /// Record a proxy request
    pub async fn record_request(&self, record: &RequestRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO proxy_requests
            (proxy_id, proxy_address, requested_url, method, success,
             response_time, status_code, error_message, timestamp)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
        )
        .bind(record.proxy_id)
        .bind(&record.proxy_address)
        .bind(&record.requested_url)
        .bind(&record.method)
        .bind(record.success)
        .bind(record.response_time)
        .bind(record.status_code)
        .bind(&record.error_message)
        .bind(record.timestamp)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Delete logs older than specified days
    pub async fn delete_older_than(&self, days: i32) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM logs WHERE timestamp < NOW() - INTERVAL '1 day' * $1",
        )
        .bind(days)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }
}
