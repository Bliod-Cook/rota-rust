use crate::error::Result;
use crate::models::{CreateLogRequest, Log, LogListParams, PaginatedResponse, RequestRecord};
use sqlx::{PgPool, Postgres, QueryBuilder};

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

        // Count query
        let mut count_query = QueryBuilder::<Postgres>::new("SELECT COUNT(*) FROM logs WHERE 1=1");
        if let Some(ref level) = params.level {
            if !level.is_empty() {
                count_query.push(" AND level = ").push_bind(level);
            }
        }
        if let Some(ref search) = params.search {
            if !search.is_empty() {
                count_query
                    .push(" AND message ILIKE ")
                    .push_bind(format!("%{}%", search));
            }
        }
        if let Some(start_time) = params.start_time {
            count_query.push(" AND timestamp >= ").push_bind(start_time);
        }
        if let Some(end_time) = params.end_time {
            count_query.push(" AND timestamp <= ").push_bind(end_time);
        }

        let total: i64 = count_query
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await?;

        // Data query
        let mut data_query = QueryBuilder::<Postgres>::new(
            r#"
            SELECT id, timestamp, level, message, details, metadata
            FROM logs
            WHERE 1=1
            "#,
        );
        if let Some(ref level) = params.level {
            if !level.is_empty() {
                data_query.push(" AND level = ").push_bind(level);
            }
        }
        if let Some(ref search) = params.search {
            if !search.is_empty() {
                data_query
                    .push(" AND message ILIKE ")
                    .push_bind(format!("%{}%", search));
            }
        }
        if let Some(start_time) = params.start_time {
            data_query.push(" AND timestamp >= ").push_bind(start_time);
        }
        if let Some(end_time) = params.end_time {
            data_query.push(" AND timestamp <= ").push_bind(end_time);
        }

        data_query
            .push(" ORDER BY timestamp DESC LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);

        let logs: Vec<Log> = data_query.build_query_as().fetch_all(&self.pool).await?;

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
        let result =
            sqlx::query("DELETE FROM logs WHERE timestamp < NOW() - INTERVAL '1 day' * $1")
                .bind(days)
                .execute(&self.pool)
                .await?;

        Ok(result.rows_affected())
    }
}
