use crate::error::{Result, RotaError};
use crate::models::{DeletedProxy, DeletedProxyListParams, PaginatedResponse, Proxy};
use sqlx::{PgPool, Postgres, QueryBuilder};
use tracing::info;

/// Repository for deleted proxy database operations
#[derive(Clone)]
pub struct DeletedProxyRepository {
    pool: PgPool,
}

impl DeletedProxyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// List deleted proxies with pagination
    pub async fn list(
        &self,
        params: &DeletedProxyListParams,
    ) -> Result<PaginatedResponse<DeletedProxy>> {
        let page = params.page.unwrap_or(1).max(1);
        let limit = params.limit.unwrap_or(20).clamp(1, 100);
        let offset = (page - 1) * limit;

        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM deleted_proxies")
            .fetch_one(&self.pool)
            .await?;

        let mut data_query = QueryBuilder::<Postgres>::new(
            r#"
            SELECT id, address, protocol, username, password, status,
                   requests, successful_requests, failed_requests,
                   avg_response_time, last_check, last_error,
                   auto_delete_after_failed_seconds, invalid_since, deleted_at, failure_reasons,
                   created_at, updated_at
            FROM deleted_proxies
            "#,
        );

        data_query
            .push(" ORDER BY deleted_at DESC LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);

        let proxies: Vec<DeletedProxy> = data_query.build_query_as().fetch_all(&self.pool).await?;

        Ok(PaginatedResponse::new(proxies, total, page, limit))
    }

    /// Get a deleted proxy by ID
    pub async fn get_by_id(&self, id: i32) -> Result<Option<DeletedProxy>> {
        let proxy = sqlx::query_as::<_, DeletedProxy>(
            r#"
            SELECT id, address, protocol, username, password, status,
                   requests, successful_requests, failed_requests,
                   avg_response_time, last_check, last_error,
                   auto_delete_after_failed_seconds, invalid_since, deleted_at, failure_reasons,
                   created_at, updated_at
            FROM deleted_proxies
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(proxy)
    }

    /// Permanently delete a deleted proxy record
    pub async fn delete(&self, id: i32) -> Result<bool> {
        let result = sqlx::query("DELETE FROM deleted_proxies WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        let deleted = result.rows_affected() > 0;
        if deleted {
            info!(id = id, "Deleted deleted_proxy record");
        }
        Ok(deleted)
    }

    /// Restore a deleted proxy back into the active proxies table.
    ///
    /// - Keeps the same `id`
    /// - Sets status to `idle`
    /// - Clears `invalid_since` and `failure_reasons`
    pub async fn restore(&self, id: i32) -> Result<Option<Proxy>> {
        let mut tx = self.pool.begin().await?;

        let deleted = sqlx::query_as::<_, DeletedProxy>(
            r#"
            SELECT id, address, protocol, username, password, status,
                   requests, successful_requests, failed_requests,
                   avg_response_time, last_check, last_error,
                   auto_delete_after_failed_seconds, invalid_since, deleted_at, failure_reasons,
                   created_at, updated_at
            FROM deleted_proxies
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(deleted) = deleted else {
            return Ok(None);
        };

        let inserted = sqlx::query_as::<_, Proxy>(
            r#"
            INSERT INTO proxies (
                id, address, protocol, username, password, status,
                requests, successful_requests, failed_requests, avg_response_time,
                last_check, last_error,
                auto_delete_after_failed_seconds, invalid_since, failure_reasons,
                created_at, updated_at
            )
            VALUES (
                $1, $2, $3, $4, $5, 'idle',
                $6, $7, $8, $9,
                $10, $11,
                $12, NULL, '[]'::jsonb,
                $13, NOW()
            )
            RETURNING id, address, protocol, username, password, status,
                      requests, successful_requests, failed_requests,
                      avg_response_time, last_check, last_error,
                      auto_delete_after_failed_seconds, invalid_since, failure_reasons,
                      created_at, updated_at
            "#,
        )
        .bind(deleted.id)
        .bind(&deleted.address)
        .bind(&deleted.protocol)
        .bind(&deleted.username)
        .bind(&deleted.password)
        .bind(deleted.requests)
        .bind(deleted.successful_requests)
        .bind(deleted.failed_requests)
        .bind(deleted.avg_response_time)
        .bind(deleted.last_check)
        .bind(&deleted.last_error)
        .bind(deleted.auto_delete_after_failed_seconds)
        .bind(deleted.created_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(db_err) if db_err.constraint() == Some("proxies_pkey") => {
                RotaError::InvalidRequest(format!("Proxy id {} already exists", id))
            }
            _ => RotaError::Database(e),
        })?;

        sqlx::query("DELETE FROM deleted_proxies WHERE id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;

        info!(id = inserted.id, address = %inserted.address, "Restored proxy");

        Ok(Some(inserted))
    }
}
