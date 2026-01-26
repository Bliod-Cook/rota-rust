use crate::config::Config;
use crate::error::{Result, RotaError};
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;
use tracing::info;

/// Database connection pool wrapper
#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    /// Create a new database connection pool
    pub async fn new(config: &Config) -> Result<Self> {
        let database_url = config.database_url();

        info!(
            host = %config.database.host,
            port = %config.database.port,
            database = %config.database.name,
            "Connecting to database"
        );

        let pool = PgPoolOptions::new()
            .min_connections(config.database.min_connections)
            .max_connections(config.database.max_connections)
            .acquire_timeout(Duration::from_secs(10))
            .idle_timeout(Duration::from_secs(30 * 60)) // 30 minutes
            .max_lifetime(Duration::from_secs(60 * 60)) // 1 hour
            .connect(&database_url)
            .await
            .map_err(|e| RotaError::DatabaseConnection(e.to_string()))?;

        info!("Database connection pool established");

        Ok(Database { pool })
    }

    /// Get a reference to the connection pool
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Check if the database is healthy
    pub async fn health_check(&self) -> Result<Duration> {
        let start = std::time::Instant::now();

        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|e| RotaError::Database(e))?;

        Ok(start.elapsed())
    }

    /// Get pool statistics
    pub fn pool_stats(&self) -> PoolStats {
        PoolStats {
            size: self.pool.size(),
            idle: self.pool.num_idle() as u32,
        }
    }

    /// Run database migrations
    pub async fn run_migrations(&self) -> Result<()> {
        info!("Running database migrations");

        super::migrations::run_migrations(&self.pool).await?;

        info!("Database migrations completed");
        Ok(())
    }

    /// Close the connection pool
    pub async fn close(&self) {
        info!("Closing database connection pool");
        self.pool.close().await;
    }
}

/// Pool statistics
#[derive(Debug, Clone)]
pub struct PoolStats {
    pub size: u32,
    pub idle: u32,
}

impl std::ops::Deref for Database {
    type Target = PgPool;

    fn deref(&self) -> &Self::Target {
        &self.pool
    }
}
