use crate::error::{Result, RotaError};
use std::env;

/// Application configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct Config {
    /// Proxy server configuration
    pub proxy: ProxyServerConfig,
    /// API server configuration
    pub api: ApiServerConfig,
    /// Database configuration
    pub database: DatabaseConfig,
    /// Admin credentials
    pub admin: AdminConfig,
    /// Logging configuration
    pub log: LogConfig,
}

#[derive(Debug, Clone)]
pub struct ProxyServerConfig {
    /// Port for the proxy server (default: 8000)
    pub port: u16,
    /// Host to bind to (default: 0.0.0.0)
    pub host: String,
    /// Maximum retry attempts for failed requests
    pub max_retries: u32,
    /// Connection timeout in seconds
    pub connect_timeout: u64,
    /// Request timeout in seconds
    pub request_timeout: u64,
    /// Enable proxy authentication
    pub auth_enabled: bool,
    /// Authentication username
    pub auth_username: String,
    /// Authentication password
    pub auth_password: String,
    /// Enable rate limiting
    pub rate_limit_enabled: bool,
    /// Rate limit requests per second
    pub rate_limit_per_second: u32,
    /// Rate limit burst size
    pub rate_limit_burst: u32,
    /// Rotation strategy (random, round_robin, least_connections, time_based)
    pub rotation_strategy: String,
}

#[derive(Debug, Clone)]
pub struct ApiServerConfig {
    /// Port for the API server (default: 8001)
    pub port: u16,
    /// Host to bind to (default: 0.0.0.0)
    pub host: String,
    /// Allowed CORS origins (comma-separated, empty = localhost only)
    pub cors_origins: Vec<String>,
    /// JWT secret for token generation
    pub jwt_secret: String,
}

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    /// Database host
    pub host: String,
    /// Database port
    pub port: u16,
    /// Database user
    pub user: String,
    /// Database password
    pub password: String,
    /// Database name
    pub name: String,
    /// SSL mode (disable, require, prefer)
    pub ssl_mode: String,
    /// Maximum connections in pool
    pub max_connections: u32,
    /// Minimum connections in pool
    pub min_connections: u32,
}

#[derive(Debug, Clone)]
pub struct AdminConfig {
    /// Admin username for dashboard
    pub username: String,
    /// Admin password for dashboard
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Log level (debug, info, warn, error)
    pub level: String,
    /// Output format (json, pretty)
    pub format: String,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        Ok(Config {
            proxy: ProxyServerConfig {
                port: get_env_or("PROXY_PORT", "8000").parse().map_err(|_| {
                    RotaError::InvalidConfig("PROXY_PORT must be a valid port number".into())
                })?,
                host: get_env_or("PROXY_HOST", "0.0.0.0"),
                max_retries: get_env_or("PROXY_MAX_RETRIES", "3").parse().unwrap_or(3),
                connect_timeout: get_env_or("PROXY_CONNECT_TIMEOUT", "10").parse().unwrap_or(10),
                request_timeout: get_env_or("PROXY_REQUEST_TIMEOUT", "30").parse().unwrap_or(30),
                auth_enabled: get_env_or("PROXY_AUTH_ENABLED", "false").parse().unwrap_or(false),
                auth_username: get_env_or("PROXY_AUTH_USERNAME", ""),
                auth_password: get_env_or("PROXY_AUTH_PASSWORD", ""),
                rate_limit_enabled: get_env_or("PROXY_RATE_LIMIT_ENABLED", "false").parse().unwrap_or(false),
                rate_limit_per_second: get_env_or("PROXY_RATE_LIMIT_PER_SECOND", "100").parse().unwrap_or(100),
                rate_limit_burst: get_env_or("PROXY_RATE_LIMIT_BURST", "200").parse().unwrap_or(200),
                rotation_strategy: get_env_or("PROXY_ROTATION_STRATEGY", "random"),
            },
            api: ApiServerConfig {
                port: get_env_or("API_PORT", "8001").parse().map_err(|_| {
                    RotaError::InvalidConfig("API_PORT must be a valid port number".into())
                })?,
                host: get_env_or("API_HOST", "0.0.0.0"),
                cors_origins: get_env_or("CORS_ORIGINS", "")
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                jwt_secret: get_env_or("JWT_SECRET", ""),
            },
            database: DatabaseConfig {
                host: get_env_or("DB_HOST", "localhost"),
                port: get_env_or("DB_PORT", "5432").parse().map_err(|_| {
                    RotaError::InvalidConfig("DB_PORT must be a valid port number".into())
                })?,
                user: get_env_or("DB_USER", "rota"),
                password: get_env_or("DB_PASSWORD", "rota_password"),
                name: get_env_or("DB_NAME", "rota"),
                ssl_mode: get_env_or("DB_SSLMODE", "disable"),
                max_connections: get_env_or("DB_MAX_CONNECTIONS", "50")
                    .parse()
                    .map_err(|_| {
                        RotaError::InvalidConfig(
                            "DB_MAX_CONNECTIONS must be a valid number".into(),
                        )
                    })?,
                min_connections: get_env_or("DB_MIN_CONNECTIONS", "5")
                    .parse()
                    .map_err(|_| {
                        RotaError::InvalidConfig(
                            "DB_MIN_CONNECTIONS must be a valid number".into(),
                        )
                    })?,
            },
            admin: AdminConfig {
                username: get_env_or("ROTA_ADMIN_USER", "admin"),
                password: get_env_or("ROTA_ADMIN_PASSWORD", "admin"),
            },
            log: LogConfig {
                level: get_env_or("LOG_LEVEL", "info"),
                format: get_env_or("LOG_FORMAT", "json"),
            },
        })
    }

    /// Get the database connection URL
    pub fn database_url(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}?sslmode={}",
            self.database.user,
            self.database.password,
            self.database.host,
            self.database.port,
            self.database.name,
            self.database.ssl_mode
        )
    }

    /// Get the proxy server address
    pub fn proxy_addr(&self) -> String {
        format!("{}:{}", self.proxy.host, self.proxy.port)
    }

    /// Get the API server address
    pub fn api_addr(&self) -> String {
        format!("{}:{}", self.api.host, self.api.port)
    }
}

/// Get environment variable with a default value
fn get_env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        // Clear any existing env vars for test isolation
        let config = Config::from_env().unwrap();

        assert_eq!(config.proxy.port, 8000);
        assert_eq!(config.api.port, 8001);
        assert_eq!(config.database.host, "localhost");
    }
}
