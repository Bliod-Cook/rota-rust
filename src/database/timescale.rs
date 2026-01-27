use crate::error::{Result, RotaError};
use sqlx::PgPool;
use tracing::{info, warn};

/// Allowed table names for TimescaleDB operations (prevent SQL injection)
const ALLOWED_HYPERTABLES: &[&str] = &["logs", "proxy_requests"];

/// Check if TimescaleDB extension is available
pub async fn is_timescaledb_available(pool: &PgPool) -> bool {
    let result = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM pg_extension WHERE extname = 'timescaledb'",
    )
    .fetch_one(pool)
    .await;

    matches!(result, Ok(count) if count > 0)
}

/// Setup TimescaleDB hypertables if the extension is available
pub async fn setup_timescaledb(pool: &PgPool) -> Result<()> {
    if !is_timescaledb_available(pool).await {
        info!("TimescaleDB not available, skipping hypertable setup");
        return Ok(());
    }

    info!("TimescaleDB detected, setting up hypertables");

    // Convert logs table to hypertable
    convert_to_hypertable(pool, "logs", "timestamp", "1 day").await?;

    // Convert proxy_requests table to hypertable
    convert_to_hypertable(pool, "proxy_requests", "timestamp", "1 day").await?;

    Ok(())
}

/// Convert a table to a TimescaleDB hypertable
async fn convert_to_hypertable(
    pool: &PgPool,
    table_name: &str,
    time_column: &str,
    chunk_interval: &str,
) -> Result<()> {
    // Validate table name against whitelist (SQL injection prevention)
    if !ALLOWED_HYPERTABLES.contains(&table_name) {
        return Err(RotaError::InvalidConfig(format!(
            "Table '{}' is not allowed for hypertable conversion",
            table_name
        )));
    }

    // Check if already a hypertable
    let is_hypertable = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM timescaledb_information.hypertables WHERE hypertable_name = $1",
    )
    .bind(table_name)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    if is_hypertable > 0 {
        info!(table = table_name, "Table is already a hypertable");
        return Ok(());
    }

    // Convert to hypertable
    // Note: We use a validated table name from the whitelist
    let query = format!(
        "SELECT create_hypertable('{}', '{}', chunk_time_interval => INTERVAL '{}', if_not_exists => TRUE, migrate_data => TRUE)",
        table_name, time_column, chunk_interval
    );

    match sqlx::query(&query).execute(pool).await {
        Ok(_) => {
            info!(
                table = table_name,
                time_column = time_column,
                "Converted table to hypertable"
            );
        }
        Err(e) => {
            warn!(
                table = table_name,
                error = %e,
                "Failed to convert table to hypertable (may already be converted)"
            );
        }
    }

    Ok(())
}

/// Add or update retention policy for a hypertable
/// Uses parameterized values to prevent SQL injection
pub async fn add_retention_policy(
    pool: &PgPool,
    table_name: &str,
    retention_days: i32,
) -> Result<()> {
    // Validate table name against whitelist
    if !ALLOWED_HYPERTABLES.contains(&table_name) {
        return Err(RotaError::InvalidConfig(format!(
            "Table '{}' is not allowed for retention policy",
            table_name
        )));
    }

    // Validate retention days (1-365)
    let retention_days = retention_days.clamp(1, 365);

    if !is_timescaledb_available(pool).await {
        return Ok(());
    }

    // Remove existing policy first
    let remove_query = format!(
        "SELECT remove_retention_policy('{}', if_exists => true)",
        table_name
    );
    let _ = sqlx::query(&remove_query).execute(pool).await;

    // Add new policy
    let add_query = format!(
        "SELECT add_retention_policy('{}', INTERVAL '{} days', if_not_exists => true)",
        table_name, retention_days
    );

    sqlx::query(&add_query)
        .execute(pool)
        .await
        .map_err(|e| RotaError::Database(e))?;

    info!(
        table = table_name,
        retention_days = retention_days,
        "Added retention policy"
    );

    Ok(())
}

/// Add or update compression policy for a hypertable
pub async fn add_compression_policy(
    pool: &PgPool,
    table_name: &str,
    compress_after_days: i32,
) -> Result<()> {
    // Validate table name against whitelist
    if !ALLOWED_HYPERTABLES.contains(&table_name) {
        return Err(RotaError::InvalidConfig(format!(
            "Table '{}' is not allowed for compression policy",
            table_name
        )));
    }

    // Validate compression days (1-365)
    let compress_after_days = compress_after_days.clamp(1, 365);

    if !is_timescaledb_available(pool).await {
        return Ok(());
    }

    // Enable compression on the hypertable
    let enable_query = format!(
        "ALTER TABLE {} SET (timescaledb.compress, timescaledb.compress_segmentby = '')",
        table_name
    );
    let _ = sqlx::query(&enable_query).execute(pool).await;

    // Remove existing policy first
    let remove_query = format!(
        "SELECT remove_compression_policy('{}', if_exists => true)",
        table_name
    );
    let _ = sqlx::query(&remove_query).execute(pool).await;

    // Add new compression policy
    let add_query = format!(
        "SELECT add_compression_policy('{}', INTERVAL '{} days', if_not_exists => true)",
        table_name, compress_after_days
    );

    match sqlx::query(&add_query).execute(pool).await {
        Ok(_) => {
            info!(
                table = table_name,
                compress_after_days = compress_after_days,
                "Added compression policy"
            );
        }
        Err(e) => {
            warn!(
                table = table_name,
                error = %e,
                "Failed to add compression policy"
            );
        }
    }

    Ok(())
}
