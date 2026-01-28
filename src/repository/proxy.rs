use crate::error::Result;
use crate::models::{
    CreateProxyRequest, PaginatedResponse, Proxy, ProxyListParams, ProxyWithStats,
    UpdateProxyRequest,
};
use sqlx::{PgPool, Postgres, QueryBuilder};
use tracing::info;

/// Repository for proxy database operations
#[derive(Clone)]
pub struct ProxyRepository {
    pool: PgPool,
}

impl ProxyRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get a proxy by ID
    pub async fn get_by_id(&self, id: i32) -> Result<Option<Proxy>> {
        let proxy = sqlx::query_as::<_, Proxy>(
            r#"
            SELECT id, address, protocol, username, password, status,
                   requests, successful_requests, failed_requests,
                   avg_response_time, last_check, last_error,
                   auto_delete_after_failed_seconds, invalid_since, failure_reasons,
                   created_at, updated_at
            FROM proxies
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(proxy)
    }

    /// Get all usable proxies (active or idle)
    pub async fn get_all_usable(&self) -> Result<Vec<Proxy>> {
        let proxies = sqlx::query_as::<_, Proxy>(
            r#"
            SELECT id, address, protocol, username, password, status,
                   requests, successful_requests, failed_requests,
                   avg_response_time, last_check, last_error,
                   auto_delete_after_failed_seconds, invalid_since, failure_reasons,
                   created_at, updated_at
            FROM proxies
            WHERE status IN ('active', 'idle')
            ORDER BY address
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(proxies)
    }

    /// Get all proxies (including failed)
    pub async fn get_all(&self) -> Result<Vec<Proxy>> {
        let proxies = sqlx::query_as::<_, Proxy>(
            r#"
            SELECT id, address, protocol, username, password, status,
                   requests, successful_requests, failed_requests,
                   avg_response_time, last_check, last_error,
                   auto_delete_after_failed_seconds, invalid_since, failure_reasons,
                   created_at, updated_at
            FROM proxies
            ORDER BY address
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(proxies)
    }

    /// List proxies with pagination, filtering, and sorting
    pub async fn list(
        &self,
        params: &ProxyListParams,
    ) -> Result<PaginatedResponse<ProxyWithStats>> {
        let page = params.page.unwrap_or(1).max(1);
        let limit = params.limit.unwrap_or(20).clamp(1, 100);
        let offset = (page - 1) * limit;

        // Build ORDER BY clause (sanitized)
        let sort_field = match params.sort_field.as_deref() {
            Some("address") => "address",
            Some("status") => "status",
            Some("requests") => "requests",
            Some("avg_response_time") => "avg_response_time",
            Some("created_at") => "created_at",
            Some("updated_at") => "updated_at",
            _ => "created_at",
        };

        let sort_order = match params.sort_order.as_deref() {
            Some("asc") => "ASC",
            Some("desc") => "DESC",
            _ => "DESC",
        };

        // Count query
        let mut count_query =
            QueryBuilder::<Postgres>::new("SELECT COUNT(*) FROM proxies WHERE 1=1");
        if let Some(ref status) = params.status {
            if !status.is_empty() {
                count_query.push(" AND status = ").push_bind(status);
            }
        }
        if let Some(ref protocol) = params.protocol {
            if !protocol.is_empty() {
                count_query.push(" AND protocol = ").push_bind(protocol);
            }
        }
        if let Some(ref search) = params.search {
            if !search.is_empty() {
                count_query
                    .push(" AND address ILIKE ")
                    .push_bind(format!("%{}%", search));
            }
        }

        let total: i64 = count_query
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await?;

        // Data query
        let mut data_query = QueryBuilder::<Postgres>::new(
            r#"
            SELECT id, address, protocol, username, password, status,
                   requests, successful_requests, failed_requests,
                   avg_response_time, last_check, last_error,
                   auto_delete_after_failed_seconds, invalid_since, failure_reasons,
                   created_at, updated_at
            FROM proxies
            WHERE 1=1
            "#,
        );

        if let Some(ref status) = params.status {
            if !status.is_empty() {
                data_query.push(" AND status = ").push_bind(status);
            }
        }
        if let Some(ref protocol) = params.protocol {
            if !protocol.is_empty() {
                data_query.push(" AND protocol = ").push_bind(protocol);
            }
        }
        if let Some(ref search) = params.search {
            if !search.is_empty() {
                data_query
                    .push(" AND address ILIKE ")
                    .push_bind(format!("%{}%", search));
            }
        }

        data_query
            .push(" ORDER BY ")
            .push(sort_field)
            .push(" ")
            .push(sort_order)
            .push(" LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);

        let proxies: Vec<Proxy> = data_query.build_query_as().fetch_all(&self.pool).await?;

        let data: Vec<ProxyWithStats> = proxies.into_iter().map(ProxyWithStats::from).collect();

        Ok(PaginatedResponse::new(data, total, page, limit))
    }

    /// Create a new proxy
    pub async fn create(&self, req: &CreateProxyRequest) -> Result<Proxy> {
        let proxy = sqlx::query_as::<_, Proxy>(
            r#"
            INSERT INTO proxies (address, protocol, username, password, auto_delete_after_failed_seconds)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, address, protocol, username, password, status,
                      requests, successful_requests, failed_requests,
                      avg_response_time, last_check, last_error,
                      auto_delete_after_failed_seconds, invalid_since, failure_reasons,
                      created_at, updated_at
            "#,
        )
        .bind(&req.address)
        .bind(&req.protocol)
        .bind(&req.username)
        .bind(&req.password)
        .bind(req.auto_delete_after_failed_seconds)
        .fetch_one(&self.pool)
        .await?;

        info!(id = proxy.id, address = %proxy.address, "Created proxy");
        Ok(proxy)
    }

    /// Update an existing proxy
    pub async fn update(&self, id: i32, req: &UpdateProxyRequest) -> Result<Option<Proxy>> {
        // Get current proxy
        let current = match self.get_by_id(id).await? {
            Some(p) => p,
            None => return Ok(None),
        };

        let address = req.address.as_ref().unwrap_or(&current.address);
        let protocol = req.protocol.as_ref().unwrap_or(&current.protocol);
        let username = req.username.as_ref().or(current.username.as_ref());
        let password = req.password.as_ref().or(current.password.as_ref());
        let status = req.status.as_ref().unwrap_or(&current.status);

        let proxy = sqlx::query_as::<_, Proxy>(
            r#"
            UPDATE proxies
            SET address = $2,
                protocol = $3,
                username = $4,
                password = $5,
                status = $6,
                invalid_since = CASE
                    WHEN $6 = 'failed' THEN COALESCE(invalid_since, NOW())
                    ELSE NULL
                END,
                failure_reasons = CASE
                    WHEN $6 = 'failed' THEN failure_reasons
                    ELSE '[]'::jsonb
                END
            WHERE id = $1
            RETURNING id, address, protocol, username, password, status,
                      requests, successful_requests, failed_requests,
                      avg_response_time, last_check, last_error,
                      auto_delete_after_failed_seconds, invalid_since, failure_reasons,
                      created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(address)
        .bind(protocol)
        .bind(username)
        .bind(password)
        .bind(status)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(ref p) = proxy {
            info!(id = p.id, address = %p.address, "Updated proxy");
        }

        Ok(proxy)
    }

    /// Delete a proxy
    pub async fn delete(&self, id: i32) -> Result<bool> {
        let result = sqlx::query("DELETE FROM proxies WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;

        let deleted = result.rows_affected() > 0;
        if deleted {
            info!(id = id, "Deleted proxy");
        }

        Ok(deleted)
    }

    /// Bulk create proxies
    pub async fn bulk_create(&self, requests: &[CreateProxyRequest]) -> Result<Vec<Proxy>> {
        let mut proxies = Vec::new();

        for req in requests {
            match self.create(req).await {
                Ok(proxy) => proxies.push(proxy),
                Err(e) => {
                    tracing::warn!(address = %req.address, error = %e, "Failed to create proxy in bulk");
                }
            }
        }

        Ok(proxies)
    }

    /// Bulk delete proxies
    pub async fn bulk_delete(&self, ids: &[i32]) -> Result<u64> {
        if ids.is_empty() {
            return Ok(0);
        }

        let result = sqlx::query("DELETE FROM proxies WHERE id = ANY($1)")
            .bind(ids)
            .execute(&self.pool)
            .await?;

        let deleted = result.rows_affected();
        info!(count = deleted, "Bulk deleted proxies");

        Ok(deleted)
    }

    /// Archive failed proxies whose continuous failure duration exceeds the configured threshold.
    ///
    /// Proxies are moved into `deleted_proxies` (not hard-deleted) and removed from `proxies`.
    pub async fn archive_expired_failed(&self, limit: i64) -> Result<Vec<i32>> {
        let limit = limit.clamp(1, 1000);

        let archived: Vec<i32> = sqlx::query_scalar(
            r#"
            WITH candidates AS (
                SELECT id
                FROM proxies
                WHERE status = 'failed'
                  AND auto_delete_after_failed_seconds IS NOT NULL
                  AND auto_delete_after_failed_seconds > 0
                  AND invalid_since IS NOT NULL
                  AND EXTRACT(EPOCH FROM (NOW() - invalid_since)) >= auto_delete_after_failed_seconds
                ORDER BY invalid_since ASC
                LIMIT $1
            ),
            inserted AS (
                INSERT INTO deleted_proxies (
                    id, address, protocol, username, password, status,
                    requests, successful_requests, failed_requests, avg_response_time,
                    last_check, last_error,
                    auto_delete_after_failed_seconds, invalid_since, deleted_at, failure_reasons,
                    created_at, updated_at
                )
                SELECT p.id, p.address, p.protocol, p.username, p.password, p.status,
                       p.requests, p.successful_requests, p.failed_requests, p.avg_response_time,
                       p.last_check, p.last_error,
                       p.auto_delete_after_failed_seconds, p.invalid_since, NOW(), p.failure_reasons,
                       p.created_at, p.updated_at
                FROM proxies p
                JOIN candidates c ON c.id = p.id
                ON CONFLICT (id) DO NOTHING
                RETURNING id
            ),
            deleted AS (
                DELETE FROM proxies
                WHERE id IN (SELECT id FROM inserted)
                RETURNING id
            )
            SELECT id FROM deleted
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(archived)
    }

    /// Update proxy statistics after a request
    pub async fn record_request(
        &self,
        id: i32,
        success: bool,
        response_time: i32,
        error_message: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE proxies
            SET
                requests = requests + 1,
                successful_requests = CASE
                    WHEN $2 THEN successful_requests + 1
                    ELSE successful_requests
                END,
                failed_requests = CASE
                    WHEN $2 THEN 0
                    ELSE failed_requests + 1
                END,
                avg_response_time = (
                    CASE
                        WHEN requests = 0 THEN $3
                        ELSE ((avg_response_time * requests) + $3) / (requests + 1)
                    END
                )::INTEGER,
                last_check = NOW(),
                last_error = CASE
                    WHEN $2 THEN NULL
                    ELSE $4
                END,
                status = CASE
                    WHEN $2 THEN 'active'
                    ELSE CASE
                        WHEN (failed_requests + 1) >= 3 THEN 'failed'
                        ELSE status
                    END
                END,
                invalid_since = CASE
                    WHEN $2 THEN NULL
                    ELSE CASE
                        WHEN status = 'failed' OR (failed_requests + 1) >= 3 THEN COALESCE(invalid_since, NOW())
                        ELSE NULL
                    END
                END,
                failure_reasons = CASE
                    WHEN $2 THEN '[]'::jsonb
                    ELSE append_failure_reason(
                        failure_reasons,
                        jsonb_build_object(
                            'timestamp', NOW(),
                            'source', 'request',
                            'message', COALESCE($4, '')
                        )
                    )
                END
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(success)
        .bind(response_time)
        .bind(error_message)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update proxy health check result
    pub async fn record_health_check(
        &self,
        id: i32,
        success: bool,
        error_message: Option<&str>,
    ) -> Result<()> {
        let status = if success { "active" } else { "failed" };

        sqlx::query(
            r#"
            UPDATE proxies
            SET last_check = NOW(),
                status = $2,
                last_error = $3,
                invalid_since = CASE
                    WHEN $2 = 'failed' THEN COALESCE(invalid_since, NOW())
                    ELSE NULL
                END,
                failure_reasons = CASE
                    WHEN $2 = 'failed' THEN append_failure_reason(
                        failure_reasons,
                        jsonb_build_object(
                            'timestamp', NOW(),
                            'source', 'healthcheck',
                            'message', COALESCE($3, '')
                        )
                    )
                    ELSE '[]'::jsonb
                END
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(status)
        .bind(error_message)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get proxy count by status
    pub async fn count_by_status(&self, status: &str) -> Result<i64> {
        let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM proxies WHERE status = $1")
            .bind(status)
            .fetch_one(&self.pool)
            .await?;

        Ok(count)
    }

    /// Get total proxy count
    pub async fn count_total(&self) -> Result<i64> {
        let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM proxies")
            .fetch_one(&self.pool)
            .await?;

        Ok(count)
    }
}
