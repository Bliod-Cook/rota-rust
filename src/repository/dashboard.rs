use crate::error::Result;
use crate::models::{ChartData, ChartDataPoint, ChartTimeRange, DashboardStats};
use sqlx::PgPool;

/// Repository for dashboard statistics
#[derive(Clone)]
pub struct DashboardRepository {
    pool: PgPool,
}

impl DashboardRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get dashboard statistics
    pub async fn get_stats(&self) -> Result<DashboardStats> {
        // Get proxy counts
        let active_proxies = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM proxies WHERE status = 'active'",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        let total_proxies = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM proxies")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);

        // Get request statistics
        let total_requests = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(SUM(requests), 0) FROM proxies",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        // Get average success rate
        let avg_success_rate: f64 = sqlx::query_scalar(
            r#"
            SELECT COALESCE(
                AVG(
                    CASE WHEN requests > 0
                    THEN (successful_requests::float / requests::float) * 100
                    ELSE 0
                    END
                ),
                0
            ) FROM proxies
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0.0);

        // Get average response time
        let avg_response_time: i32 = sqlx::query_scalar(
            "SELECT COALESCE(AVG(avg_response_time), 0)::INTEGER FROM proxies WHERE requests > 0",
        )
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        // Get growth metrics (comparing last 24h to previous 24h)
        let (request_growth, success_rate_growth, response_time_delta) =
            self.get_growth_metrics().await.unwrap_or((0.0, 0.0, 0));

        Ok(DashboardStats {
            active_proxies,
            total_proxies,
            total_requests,
            avg_success_rate,
            avg_response_time,
            request_growth,
            success_rate_growth,
            response_time_delta,
        })
    }

    /// Calculate growth metrics comparing current period to previous period
    async fn get_growth_metrics(&self) -> Result<(f64, f64, i32)> {
        // This is a simplified version - in production you'd want more sophisticated
        // time-series analysis using the proxy_requests table

        // For now, return neutral growth
        Ok((0.0, 0.0, 0))
    }

    /// Get request count chart data
    pub async fn get_request_chart(&self, range: &ChartTimeRange) -> Result<ChartData> {
        let start = range.start_time();
        let end = range.end_time();
        let interval = range.interval();

        let query = format!(
            r#"
            SELECT
                time_bucket(INTERVAL '{}', timestamp) AS bucket,
                COUNT(*)::float AS value
            FROM proxy_requests
            WHERE timestamp >= $1 AND timestamp <= $2
            GROUP BY bucket
            ORDER BY bucket
            "#,
            interval
        );

        let rows: Vec<(chrono::DateTime<chrono::Utc>, f64)> = sqlx::query_as(&query)
            .bind(start)
            .bind(end)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();

        let data: Vec<ChartDataPoint> = rows
            .into_iter()
            .map(|(timestamp, value)| ChartDataPoint { timestamp, value })
            .collect();

        Ok(ChartData {
            data,
            label: "Requests".to_string(),
        })
    }

    /// Get success rate chart data
    pub async fn get_success_rate_chart(&self, range: &ChartTimeRange) -> Result<ChartData> {
        let start = range.start_time();
        let end = range.end_time();
        let interval = range.interval();

        let query = format!(
            r#"
            SELECT
                time_bucket(INTERVAL '{}', timestamp) AS bucket,
                COALESCE(
                    (SUM(CASE WHEN success THEN 1 ELSE 0 END)::float /
                     NULLIF(COUNT(*), 0)::float) * 100,
                    0
                ) AS value
            FROM proxy_requests
            WHERE timestamp >= $1 AND timestamp <= $2
            GROUP BY bucket
            ORDER BY bucket
            "#,
            interval
        );

        let rows: Vec<(chrono::DateTime<chrono::Utc>, f64)> = sqlx::query_as(&query)
            .bind(start)
            .bind(end)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();

        let data: Vec<ChartDataPoint> = rows
            .into_iter()
            .map(|(timestamp, value)| ChartDataPoint { timestamp, value })
            .collect();

        Ok(ChartData {
            data,
            label: "Success Rate %".to_string(),
        })
    }

    /// Get response time chart data
    pub async fn get_response_time_chart(&self, range: &ChartTimeRange) -> Result<ChartData> {
        let start = range.start_time();
        let end = range.end_time();
        let interval = range.interval();

        let query = format!(
            r#"
            SELECT
                time_bucket(INTERVAL '{}', timestamp) AS bucket,
                COALESCE(AVG(response_time)::float, 0) AS value
            FROM proxy_requests
            WHERE timestamp >= $1 AND timestamp <= $2
            GROUP BY bucket
            ORDER BY bucket
            "#,
            interval
        );

        let rows: Vec<(chrono::DateTime<chrono::Utc>, f64)> = sqlx::query_as(&query)
            .bind(start)
            .bind(end)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();

        let data: Vec<ChartDataPoint> = rows
            .into_iter()
            .map(|(timestamp, value)| ChartDataPoint { timestamp, value })
            .collect();

        Ok(ChartData {
            data,
            label: "Response Time (ms)".to_string(),
        })
    }
}
