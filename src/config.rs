use crate::error::{Result, RotaError};
use std::env;
use url::Url;

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
    /// Optional forward/egress proxy for dialing upstream proxies
    pub egress_proxy: Option<EgressProxyConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EgressProxyProtocol {
    Http,
    Socks5,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressProxyConfig {
    pub protocol: EgressProxyProtocol,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
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
                connect_timeout: get_env_or("PROXY_CONNECT_TIMEOUT", "10")
                    .parse()
                    .unwrap_or(10),
                request_timeout: get_env_or("PROXY_REQUEST_TIMEOUT", "30")
                    .parse()
                    .unwrap_or(30),
                auth_enabled: get_env_or("PROXY_AUTH_ENABLED", "false")
                    .parse()
                    .unwrap_or(false),
                auth_username: get_env_or("PROXY_AUTH_USERNAME", ""),
                auth_password: get_env_or("PROXY_AUTH_PASSWORD", ""),
                rate_limit_enabled: get_env_or("PROXY_RATE_LIMIT_ENABLED", "false")
                    .parse()
                    .unwrap_or(false),
                rate_limit_per_second: get_env_or("PROXY_RATE_LIMIT_PER_SECOND", "100")
                    .parse()
                    .unwrap_or(100),
                rate_limit_burst: get_env_or("PROXY_RATE_LIMIT_BURST", "200")
                    .parse()
                    .unwrap_or(200),
                rotation_strategy: get_env_or("PROXY_ROTATION_STRATEGY", "random"),
                egress_proxy: parse_egress_proxy()?,
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
                        RotaError::InvalidConfig("DB_MAX_CONNECTIONS must be a valid number".into())
                    })?,
                min_connections: get_env_or("DB_MIN_CONNECTIONS", "5").parse().map_err(|_| {
                    RotaError::InvalidConfig("DB_MIN_CONNECTIONS must be a valid number".into())
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

fn parse_egress_proxy() -> Result<Option<EgressProxyConfig>> {
    let raw = env::var("ROTA_EGRESS_PROXY").unwrap_or_default();
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }

    let url = Url::parse(raw).map_err(|e| {
        RotaError::InvalidConfig(format!("ROTA_EGRESS_PROXY must be a valid URL: {}", e))
    })?;

    // Reject URLs that carry request-specific components.
    if url.fragment().is_some() || url.query().is_some() {
        return Err(RotaError::InvalidConfig(
            "ROTA_EGRESS_PROXY must not include query/fragment".into(),
        ));
    }
    if !(url.path().is_empty() || url.path() == "/") {
        return Err(RotaError::InvalidConfig(
            "ROTA_EGRESS_PROXY must not include a path".into(),
        ));
    }

    let protocol = match url.scheme().to_lowercase().as_str() {
        "http" | "https" => EgressProxyProtocol::Http,
        "socks5" | "socks5h" => EgressProxyProtocol::Socks5,
        other => {
            return Err(RotaError::InvalidConfig(format!(
                "ROTA_EGRESS_PROXY has unsupported scheme: {}",
                other
            )))
        }
    };

    let host = url.host_str().ok_or_else(|| {
        RotaError::InvalidConfig("ROTA_EGRESS_PROXY must include a host".into())
    })?;

    let port = match url.port() {
        Some(p) => p,
        None => match protocol {
            EgressProxyProtocol::Http => 80,
            EgressProxyProtocol::Socks5 => 1080,
        },
    };

    let username = if url.username().is_empty() {
        None
    } else {
        Some(url.username().to_string())
    };

    let password = match (protocol, &username, url.password()) {
        (_, None, _) => None,
        (EgressProxyProtocol::Http, Some(_), None) => Some(String::new()),
        (EgressProxyProtocol::Http, Some(_), Some(p)) => Some(p.to_string()),
        (EgressProxyProtocol::Socks5, Some(_), Some(p)) if !p.is_empty() => Some(p.to_string()),
        (EgressProxyProtocol::Socks5, Some(_), _) => {
            return Err(RotaError::InvalidConfig(
                "ROTA_EGRESS_PROXY socks5 auth requires a non-empty password".into(),
            ))
        }
    };

    Ok(Some(EgressProxyConfig {
        protocol,
        host: host.to_string(),
        port,
        username,
        password,
    }))
}

/// Get environment variable with a default value
fn get_env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    const CONFIG_ENV_KEYS: &[&str] = &[
        "PROXY_PORT",
        "PROXY_HOST",
        "PROXY_MAX_RETRIES",
        "PROXY_CONNECT_TIMEOUT",
        "PROXY_REQUEST_TIMEOUT",
        "PROXY_AUTH_ENABLED",
        "PROXY_AUTH_USERNAME",
        "PROXY_AUTH_PASSWORD",
        "PROXY_RATE_LIMIT_ENABLED",
        "PROXY_RATE_LIMIT_PER_SECOND",
        "PROXY_RATE_LIMIT_BURST",
        "PROXY_ROTATION_STRATEGY",
        "ROTA_EGRESS_PROXY",
        "API_PORT",
        "API_HOST",
        "CORS_ORIGINS",
        "JWT_SECRET",
        "DB_HOST",
        "DB_PORT",
        "DB_USER",
        "DB_PASSWORD",
        "DB_NAME",
        "DB_SSLMODE",
        "DB_MAX_CONNECTIONS",
        "DB_MIN_CONNECTIONS",
        "ROTA_ADMIN_USER",
        "ROTA_ADMIN_PASSWORD",
        "LOG_LEVEL",
        "LOG_FORMAT",
    ];

    struct EnvGuard {
        saved: Vec<(String, Option<String>)>,
    }

    impl EnvGuard {
        fn new(keys: &[&str]) -> Self {
            let saved = keys
                .iter()
                .map(|&key| {
                    let old = env::var(key).ok();
                    env::remove_var(key);
                    (key.to_string(), old)
                })
                .collect();

            Self { saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                match value {
                    Some(v) => env::set_var(key, v),
                    None => env::remove_var(key),
                }
            }
        }
    }

    #[test]
    fn test_config_from_env_defaults() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::new(CONFIG_ENV_KEYS);

        let config = Config::from_env().unwrap();

        assert_eq!(config.proxy.port, 8000);
        assert_eq!(config.proxy.host, "0.0.0.0");
        assert_eq!(config.proxy.rotation_strategy, "random");
        assert!(config.proxy.egress_proxy.is_none());

        assert_eq!(config.api.port, 8001);
        assert_eq!(config.api.host, "0.0.0.0");
        assert!(config.api.cors_origins.is_empty());

        assert_eq!(config.database.host, "localhost");
        assert_eq!(config.database.port, 5432);
    }

    #[test]
    fn test_config_from_env_overrides() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::new(CONFIG_ENV_KEYS);

        env::set_var("PROXY_PORT", "9000");
        env::set_var("PROXY_HOST", "127.0.0.1");
        env::set_var("PROXY_ROTATION_STRATEGY", "round_robin");
        env::set_var("ROTA_EGRESS_PROXY", "http://user:pass@egress.example:3128");
        env::set_var("API_PORT", "9001");
        env::set_var("CORS_ORIGINS", "https://a.example, https://b.example");
        env::set_var("DB_HOST", "db.example");

        let config = Config::from_env().unwrap();

        assert_eq!(config.proxy.port, 9000);
        assert_eq!(config.proxy.host, "127.0.0.1");
        assert_eq!(config.proxy.rotation_strategy, "round_robin");
        assert_eq!(
            config.proxy.egress_proxy,
            Some(EgressProxyConfig {
                protocol: EgressProxyProtocol::Http,
                host: "egress.example".to_string(),
                port: 3128,
                username: Some("user".to_string()),
                password: Some("pass".to_string()),
            })
        );
        assert_eq!(config.api.port, 9001);
        assert_eq!(
            config.api.cors_origins,
            vec![
                "https://a.example".to_string(),
                "https://b.example".to_string()
            ]
        );
        assert_eq!(config.database.host, "db.example");
    }

    #[test]
    fn test_config_from_env_invalid_port() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::new(CONFIG_ENV_KEYS);

        env::set_var("PROXY_PORT", "not-a-port");
        let err = Config::from_env().unwrap_err();
        assert!(matches!(err, RotaError::InvalidConfig(_)));
    }

    #[test]
    fn test_config_from_env_invalid_egress_proxy_url() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::new(CONFIG_ENV_KEYS);

        env::set_var("ROTA_EGRESS_PROXY", "not a url");
        let err = Config::from_env().unwrap_err();
        assert!(matches!(err, RotaError::InvalidConfig(_)));
    }

    #[test]
    fn test_config_from_env_socks5_egress_proxy_requires_password_if_user_set() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::new(CONFIG_ENV_KEYS);

        env::set_var("ROTA_EGRESS_PROXY", "socks5://user@egress.example:1080");
        let err = Config::from_env().unwrap_err();
        assert!(matches!(err, RotaError::InvalidConfig(_)));
    }

    #[test]
    fn test_config_formatters() {
        let config = Config {
            proxy: ProxyServerConfig {
                port: 8000,
                host: "0.0.0.0".to_string(),
                max_retries: 3,
                connect_timeout: 10,
                request_timeout: 30,
                auth_enabled: false,
                auth_username: "".to_string(),
                auth_password: "".to_string(),
                rate_limit_enabled: false,
                rate_limit_per_second: 100,
                rate_limit_burst: 200,
                rotation_strategy: "random".to_string(),
                egress_proxy: None,
            },
            api: ApiServerConfig {
                port: 8001,
                host: "0.0.0.0".to_string(),
                cors_origins: vec![],
                jwt_secret: "".to_string(),
            },
            database: DatabaseConfig {
                host: "localhost".to_string(),
                port: 5432,
                user: "rota".to_string(),
                password: "rota_password".to_string(),
                name: "rota".to_string(),
                ssl_mode: "disable".to_string(),
                max_connections: 50,
                min_connections: 5,
            },
            admin: AdminConfig {
                username: "admin".to_string(),
                password: "admin".to_string(),
            },
            log: LogConfig {
                level: "info".to_string(),
                format: "json".to_string(),
            },
        };

        assert_eq!(config.proxy_addr(), "0.0.0.0:8000");
        assert_eq!(config.api_addr(), "0.0.0.0:8001");
        assert_eq!(
            config.database_url(),
            "postgres://rota:rota_password@localhost:5432/rota?sslmode=disable"
        );
    }
}
