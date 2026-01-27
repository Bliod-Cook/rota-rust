use crate::error::{Result, RotaError};
use crate::models::{
    keys, AuthenticationSettings, HealthCheckSettings, LogRetentionSettings, RateLimitSettings,
    RotationSettings, Settings, SettingsRecord,
};
use sqlx::PgPool;
use tracing::info;

/// Repository for settings database operations
#[derive(Clone)]
pub struct SettingsRepository {
    pool: PgPool,
}

impl SettingsRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get all settings
    pub async fn get_all(&self) -> Result<Settings> {
        let records =
            sqlx::query_as::<_, SettingsRecord>("SELECT key, value, updated_at FROM settings")
                .fetch_all(&self.pool)
                .await?;

        let mut settings = Settings::default();

        for record in records {
            match record.key.as_str() {
                keys::AUTHENTICATION => {
                    if let Ok(v) = serde_json::from_value(record.value) {
                        settings.authentication = v;
                    }
                }
                keys::ROTATION => {
                    if let Ok(v) = serde_json::from_value(record.value) {
                        settings.rotation = v;
                    }
                }
                keys::RATE_LIMIT => {
                    if let Ok(v) = serde_json::from_value(record.value) {
                        settings.rate_limit = v;
                    }
                }
                keys::HEALTHCHECK => {
                    if let Ok(v) = serde_json::from_value(record.value) {
                        settings.healthcheck = v;
                    }
                }
                keys::LOG_RETENTION => {
                    if let Ok(v) = serde_json::from_value(record.value) {
                        settings.log_retention = v;
                    }
                }
                _ => {}
            }
        }

        Ok(settings)
    }

    /// Get a specific setting by key
    pub async fn get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<T> {
        let record = sqlx::query_as::<_, SettingsRecord>(
            "SELECT key, value, updated_at FROM settings WHERE key = $1",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| RotaError::SettingsNotFound {
            key: key.to_string(),
        })?;

        let value = serde_json::from_value(record.value).map_err(|e| {
            RotaError::Internal(format!("Failed to parse setting '{}': {}", key, e))
        })?;

        Ok(value)
    }

    /// Get authentication settings
    pub async fn get_authentication(&self) -> Result<AuthenticationSettings> {
        self.get(keys::AUTHENTICATION).await
    }

    /// Get rotation settings
    pub async fn get_rotation(&self) -> Result<RotationSettings> {
        self.get(keys::ROTATION).await
    }

    /// Get rate limit settings
    pub async fn get_rate_limit(&self) -> Result<RateLimitSettings> {
        self.get(keys::RATE_LIMIT).await
    }

    /// Get health check settings
    pub async fn get_healthcheck(&self) -> Result<HealthCheckSettings> {
        self.get(keys::HEALTHCHECK).await
    }

    /// Get log retention settings
    pub async fn get_log_retention(&self) -> Result<LogRetentionSettings> {
        self.get(keys::LOG_RETENTION).await
    }

    /// Set a specific setting
    pub async fn set<T: serde::Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let json_value = serde_json::to_value(value)
            .map_err(|e| RotaError::Internal(format!("Failed to serialize setting: {}", e)))?;

        sqlx::query(
            r#"
            INSERT INTO settings (key, value)
            VALUES ($1, $2)
            ON CONFLICT (key) DO UPDATE SET value = $2, updated_at = NOW()
            "#,
        )
        .bind(key)
        .bind(json_value)
        .execute(&self.pool)
        .await?;

        info!(key = key, "Updated setting");
        Ok(())
    }

    /// Update all settings
    pub async fn update_all(&self, settings: &Settings) -> Result<()> {
        self.set(keys::AUTHENTICATION, &settings.authentication)
            .await?;
        self.set(keys::ROTATION, &settings.rotation).await?;
        self.set(keys::RATE_LIMIT, &settings.rate_limit).await?;
        self.set(keys::HEALTHCHECK, &settings.healthcheck).await?;
        self.set(keys::LOG_RETENTION, &settings.log_retention)
            .await?;

        info!("Updated all settings");
        Ok(())
    }

    /// Reset all settings to defaults
    pub async fn reset(&self) -> Result<Settings> {
        let defaults = Settings::default();
        self.update_all(&defaults).await?;

        info!("Reset settings to defaults");
        Ok(defaults)
    }
}
