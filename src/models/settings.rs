use serde::{Deserialize, Serialize};

/// Complete application settings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    pub authentication: AuthenticationSettings,
    pub rotation: RotationSettings,
    pub rate_limit: RateLimitSettings,
    pub healthcheck: HealthCheckSettings,
    pub log_retention: LogRetentionSettings,
}

/// Proxy server authentication settings
/// Controls authentication for incoming requests to the PROXY server (port 8000)
/// NOT for dashboard/API login
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationSettings {
    /// Enable authentication for proxy requests
    pub enabled: bool,
    /// Username for proxy authentication
    pub username: String,
    /// Password for proxy authentication (write-only, not exposed in JSON response)
    #[serde(skip_serializing, default)]
    pub password: String,
}

impl Default for AuthenticationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            username: String::new(),
            password: String::new(),
        }
    }
}

/// Proxy rotation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationSettings {
    /// Rotation method: random, roundrobin, least_conn, time_based
    pub method: String,
    /// Time-based rotation settings
    pub time_based: TimeBasedSettings,
    /// Remove unhealthy proxies from rotation
    pub remove_unhealthy: bool,
    /// Enable fallback to next proxy on failure
    pub fallback: bool,
    /// Maximum fallback retries
    pub fallback_max_retries: i32,
    /// Follow HTTP redirects
    pub follow_redirect: bool,
    /// Request timeout in seconds
    pub timeout: i32,
    /// Per-proxy retry count
    pub retries: i32,
    /// Allowed protocols filter (empty = all)
    pub allowed_protocols: Vec<String>,
    /// Maximum response time in milliseconds (0 = no limit)
    pub max_response_time: i32,
    /// Minimum success rate percentage (0-100, 0 = no minimum)
    pub min_success_rate: f64,
}

impl Default for RotationSettings {
    fn default() -> Self {
        Self {
            method: "random".to_string(),
            time_based: TimeBasedSettings::default(),
            remove_unhealthy: true,
            fallback: true,
            fallback_max_retries: 3,
            follow_redirect: true,
            timeout: 30,
            retries: 2,
            allowed_protocols: vec![],
            max_response_time: 0,
            min_success_rate: 0.0,
        }
    }
}

/// Time-based rotation settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeBasedSettings {
    /// Interval in seconds
    pub interval: i32,
}

impl Default for TimeBasedSettings {
    fn default() -> Self {
        Self { interval: 60 }
    }
}

/// Rate limiting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitSettings {
    /// Enable rate limiting
    pub enabled: bool,
    /// Time interval in seconds
    pub interval: i32,
    /// Maximum requests per interval
    pub max_requests: i32,
}

impl Default for RateLimitSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            interval: 60,
            max_requests: 100,
        }
    }
}

/// Health check configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckSettings {
    /// Timeout in seconds
    pub timeout: i32,
    /// Number of concurrent health check workers
    pub workers: i32,
    /// URL to check
    pub url: String,
    /// Expected HTTP status code
    pub status: i32,
    /// Custom headers
    pub headers: Vec<String>,
}

impl Default for HealthCheckSettings {
    fn default() -> Self {
        Self {
            timeout: 10,
            workers: 20,
            url: "https://httpbin.org/ip".to_string(),
            status: 200,
            headers: vec![],
        }
    }
}

/// Log retention and cleanup configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRetentionSettings {
    /// Enable automatic log cleanup
    pub enabled: bool,
    /// Days to keep logs (7, 15, 30, 60, 90)
    pub retention_days: i32,
    /// Compress logs older than X days
    pub compression_after_days: i32,
    /// How often to run cleanup in hours
    pub cleanup_interval_hours: i32,
}

impl Default for LogRetentionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: 30,
            compression_after_days: 7,
            cleanup_interval_hours: 24,
        }
    }
}

/// Settings database record
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SettingsRecord {
    pub key: String,
    pub value: serde_json::Value,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Settings key constants
pub mod keys {
    pub const AUTHENTICATION: &str = "authentication";
    pub const ROTATION: &str = "rotation";
    pub const RATE_LIMIT: &str = "rate_limit";
    pub const HEALTHCHECK: &str = "healthcheck";
    pub const LOG_RETENTION: &str = "log_retention";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authentication_password_is_write_only() {
        let settings = AuthenticationSettings {
            enabled: true,
            username: "admin".to_string(),
            password: "secret".to_string(),
        };

        let value = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            value.get("username").and_then(|v| v.as_str()),
            Some("admin")
        );
        assert_eq!(value.get("enabled").and_then(|v| v.as_bool()), Some(true));
        assert!(value.get("password").is_none());

        let decoded: AuthenticationSettings =
            serde_json::from_str(r#"{"enabled":true,"username":"admin","password":"secret"}"#)
                .unwrap();
        assert_eq!(decoded.password, "secret");

        let decoded_without_password: AuthenticationSettings =
            serde_json::from_str(r#"{"enabled":true,"username":"admin"}"#).unwrap();
        assert_eq!(decoded_without_password.password, "");
    }

    #[test]
    fn test_settings_serialization_never_includes_password() {
        let settings = Settings {
            authentication: AuthenticationSettings {
                enabled: true,
                username: "admin".to_string(),
                password: "secret".to_string(),
            },
            ..Settings::default()
        };

        let value = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            value
                .get("authentication")
                .and_then(|v| v.get("username"))
                .and_then(|v| v.as_str()),
            Some("admin")
        );
        assert!(value
            .get("authentication")
            .and_then(|v| v.get("password"))
            .is_none());
    }
}
