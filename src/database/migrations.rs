use crate::error::{Result, RotaError};
use sqlx::PgPool;
use tracing::info;

/// Run all database migrations
pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    // Create migrations table if not exists
    create_migrations_table(pool).await?;

    // Run each migration in order
    let migrations = get_migrations();

    for (version, name, sql) in migrations {
        if !is_migration_applied(pool, version).await? {
            info!(version = version, name = name, "Applying migration");

            // Execute migration
            sqlx::query(sql)
                .execute(pool)
                .await
                .map_err(|e| {
                    RotaError::Database(e)
                })?;

            // Record migration
            record_migration(pool, version, name).await?;

            info!(version = version, name = name, "Migration applied successfully");
        }
    }

    Ok(())
}

/// Create the migrations tracking table
async fn create_migrations_table(pool: &PgPool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name VARCHAR(255) NOT NULL,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| RotaError::Database(e))?;

    Ok(())
}

/// Check if a migration has been applied
async fn is_migration_applied(pool: &PgPool, version: i32) -> Result<bool> {
    let result = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM schema_migrations WHERE version = $1",
    )
    .bind(version)
    .fetch_one(pool)
    .await
    .map_err(|e| RotaError::Database(e))?;

    Ok(result > 0)
}

/// Record a migration as applied
async fn record_migration(pool: &PgPool, version: i32, name: &str) -> Result<()> {
    sqlx::query("INSERT INTO schema_migrations (version, name) VALUES ($1, $2)")
        .bind(version)
        .bind(name)
        .execute(pool)
        .await
        .map_err(|e| RotaError::Database(e))?;

    Ok(())
}

/// Get all migrations in order
fn get_migrations() -> Vec<(i32, &'static str, &'static str)> {
    vec![
        (1, "initial_schema", MIGRATION_001_INITIAL_SCHEMA),
        (2, "settings_table", MIGRATION_002_SETTINGS_TABLE),
        (3, "logs_table", MIGRATION_003_LOGS_TABLE),
        (4, "proxy_requests_table", MIGRATION_004_PROXY_REQUESTS),
    ]
}

// Migration 1: Initial schema with proxies table
const MIGRATION_001_INITIAL_SCHEMA: &str = r#"
-- Proxies table
CREATE TABLE IF NOT EXISTS proxies (
    id SERIAL PRIMARY KEY,
    address VARCHAR(255) NOT NULL,
    protocol VARCHAR(20) NOT NULL DEFAULT 'http',
    username VARCHAR(255),
    password VARCHAR(255),
    status VARCHAR(20) NOT NULL DEFAULT 'idle',
    requests BIGINT NOT NULL DEFAULT 0,
    successful_requests BIGINT NOT NULL DEFAULT 0,
    failed_requests BIGINT NOT NULL DEFAULT 0,
    avg_response_time INTEGER NOT NULL DEFAULT 0,
    last_check TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT unique_proxy_address UNIQUE (address)
);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_proxies_status ON proxies(status);
CREATE INDEX IF NOT EXISTS idx_proxies_protocol ON proxies(protocol);
CREATE INDEX IF NOT EXISTS idx_proxies_address ON proxies(address);

-- Updated_at trigger
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

DROP TRIGGER IF EXISTS update_proxies_updated_at ON proxies;
CREATE TRIGGER update_proxies_updated_at
    BEFORE UPDATE ON proxies
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();
"#;

// Migration 2: Settings table
const MIGRATION_002_SETTINGS_TABLE: &str = r#"
-- Settings table for JSON configuration
CREATE TABLE IF NOT EXISTS settings (
    key VARCHAR(100) PRIMARY KEY,
    value JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Insert default settings
INSERT INTO settings (key, value) VALUES
    ('authentication', '{"enabled": false, "username": "", "password": ""}'),
    ('rotation', '{"method": "random", "time_based": {"interval": 60}, "remove_unhealthy": true, "fallback": true, "fallback_max_retries": 3, "follow_redirect": true, "timeout": 30, "retries": 2, "allowed_protocols": [], "max_response_time": 0, "min_success_rate": 0}'),
    ('rate_limit', '{"enabled": false, "interval": 60, "max_requests": 100}'),
    ('healthcheck', '{"timeout": 10, "workers": 20, "url": "https://httpbin.org/ip", "status": 200, "headers": []}'),
    ('log_retention', '{"enabled": true, "retention_days": 30, "compression_after_days": 7, "cleanup_interval_hours": 24}')
ON CONFLICT (key) DO NOTHING;

-- Updated_at trigger for settings
DROP TRIGGER IF EXISTS update_settings_updated_at ON settings;
CREATE TRIGGER update_settings_updated_at
    BEFORE UPDATE ON settings
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();
"#;

// Migration 3: Logs table
const MIGRATION_003_LOGS_TABLE: &str = r#"
-- Logs table
CREATE TABLE IF NOT EXISTS logs (
    id BIGSERIAL,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    level VARCHAR(20) NOT NULL,
    message TEXT NOT NULL,
    details TEXT,
    metadata JSONB,
    PRIMARY KEY (id, timestamp)
);

-- Index for timestamp-based queries
CREATE INDEX IF NOT EXISTS idx_logs_timestamp ON logs(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_logs_level ON logs(level);
"#;

// Migration 4: Proxy requests table (for TimescaleDB if available)
const MIGRATION_004_PROXY_REQUESTS: &str = r#"
-- Proxy requests table for usage tracking
CREATE TABLE IF NOT EXISTS proxy_requests (
    id BIGSERIAL,
    proxy_id INTEGER NOT NULL,
    proxy_address VARCHAR(255) NOT NULL,
    requested_url TEXT,
    method VARCHAR(10),
    success BOOLEAN NOT NULL DEFAULT false,
    response_time INTEGER NOT NULL DEFAULT 0,
    status_code INTEGER,
    error_message TEXT,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (id, timestamp)
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_proxy_requests_timestamp ON proxy_requests(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_proxy_requests_proxy_id ON proxy_requests(proxy_id);
CREATE INDEX IF NOT EXISTS idx_proxy_requests_success ON proxy_requests(success);
"#;
