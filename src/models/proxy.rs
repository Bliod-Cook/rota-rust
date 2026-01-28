use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

/// Proxy protocol type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "varchar", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum ProxyProtocol {
    Http,
    Https,
    Socks4,
    Socks4a,
    Socks5,
}

impl ProxyProtocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProxyProtocol::Http => "http",
            ProxyProtocol::Https => "https",
            ProxyProtocol::Socks4 => "socks4",
            ProxyProtocol::Socks4a => "socks4a",
            ProxyProtocol::Socks5 => "socks5",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "http" => Some(ProxyProtocol::Http),
            "https" => Some(ProxyProtocol::Https),
            "socks4" => Some(ProxyProtocol::Socks4),
            "socks4a" => Some(ProxyProtocol::Socks4a),
            "socks5" => Some(ProxyProtocol::Socks5),
            _ => None,
        }
    }

    pub fn is_socks(&self) -> bool {
        matches!(
            self,
            ProxyProtocol::Socks4 | ProxyProtocol::Socks4a | ProxyProtocol::Socks5
        )
    }

    pub fn is_http(&self) -> bool {
        matches!(self, ProxyProtocol::Http | ProxyProtocol::Https)
    }
}

impl std::fmt::Display for ProxyProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Proxy status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type, Default)]
#[sqlx(type_name = "varchar", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum ProxyStatus {
    #[default]
    Idle,
    Active,
    Failed,
}

impl ProxyStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProxyStatus::Idle => "idle",
            ProxyStatus::Active => "active",
            ProxyStatus::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "idle" => Some(ProxyStatus::Idle),
            "active" => Some(ProxyStatus::Active),
            "failed" => Some(ProxyStatus::Failed),
            _ => None,
        }
    }

    pub fn is_usable(&self) -> bool {
        matches!(self, ProxyStatus::Idle | ProxyStatus::Active)
    }
}

impl std::fmt::Display for ProxyStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Proxy entity
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Proxy {
    pub id: i32,
    pub address: String,
    pub protocol: String, // Stored as string in DB
    #[serde(skip_serializing)]
    pub username: Option<String>,
    #[serde(skip_serializing)]
    pub password: Option<String>,
    pub status: String, // Stored as string in DB
    pub requests: i64,
    pub successful_requests: i64,
    pub failed_requests: i64,
    pub avg_response_time: i32,
    pub last_check: Option<DateTime<Utc>>,
    #[serde(skip_serializing)]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_delete_after_failed_seconds: Option<i32>,
    #[serde(skip_serializing)]
    pub invalid_since: Option<DateTime<Utc>>,
    #[serde(skip_serializing)]
    pub failure_reasons: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Proxy {
    /// Get the protocol enum
    pub fn protocol_enum(&self) -> Option<ProxyProtocol> {
        ProxyProtocol::from_str(&self.protocol)
    }

    /// Get the status enum
    pub fn status_enum(&self) -> Option<ProxyStatus> {
        ProxyStatus::from_str(&self.status)
    }

    /// Calculate success rate as percentage
    pub fn success_rate(&self) -> f64 {
        if self.requests == 0 {
            0.0
        } else {
            (self.successful_requests as f64 / self.requests as f64) * 100.0
        }
    }

    /// Check if proxy is usable
    pub fn is_usable(&self) -> bool {
        self.status_enum().map(|s| s.is_usable()).unwrap_or(false)
    }

    /// Check if proxy matches filter criteria
    pub fn matches_filter(&self, settings: &super::RotationSettings) -> bool {
        // Protocol filter
        if !settings.allowed_protocols.is_empty() {
            if let Some(proto) = self.protocol_enum() {
                if !settings
                    .allowed_protocols
                    .contains(&proto.as_str().to_string())
                {
                    return false;
                }
            }
        }

        // Max response time filter
        if settings.max_response_time > 0 && self.avg_response_time > settings.max_response_time {
            return false;
        }

        // Min success rate filter
        if settings.min_success_rate > 0.0 && self.success_rate() < settings.min_success_rate {
            return false;
        }

        true
    }

    /// Get proxy URL with optional authentication
    pub fn url(&self) -> String {
        let proto = self.protocol_enum().unwrap_or(ProxyProtocol::Http);
        let scheme = match proto {
            ProxyProtocol::Http | ProxyProtocol::Https => "http",
            ProxyProtocol::Socks4 => "socks4",
            ProxyProtocol::Socks4a => "socks4a",
            ProxyProtocol::Socks5 => "socks5",
        };

        match (&self.username, &self.password) {
            (Some(user), Some(pass)) => {
                format!("{}://{}:{}@{}", scheme, user, pass, self.address)
            }
            (Some(user), None) => {
                format!("{}://{}@{}", scheme, user, self.address)
            }
            _ => {
                format!("{}://{}", scheme, self.address)
            }
        }
    }
}

/// Proxy with calculated statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyWithStats {
    #[serde(flatten)]
    pub proxy: Proxy,
    pub success_rate: f64,
}

impl From<Proxy> for ProxyWithStats {
    fn from(proxy: Proxy) -> Self {
        let success_rate = proxy.success_rate();
        ProxyWithStats {
            proxy,
            success_rate,
        }
    }
}

/// Request to create a new proxy
#[derive(Debug, Clone, Deserialize)]
pub struct CreateProxyRequest {
    pub address: String,
    pub protocol: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub auto_delete_after_failed_seconds: Option<i32>,
}

/// Request to update an existing proxy
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateProxyRequest {
    pub address: Option<String>,
    pub protocol: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub status: Option<String>,
}

/// Archived proxy (automatically deleted and moved out of the active pool)
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DeletedProxy {
    pub id: i32,
    pub address: String,
    pub protocol: String,
    #[serde(skip_serializing)]
    pub username: Option<String>,
    #[serde(skip_serializing)]
    pub password: Option<String>,
    pub status: String,
    pub requests: i64,
    pub successful_requests: i64,
    pub failed_requests: i64,
    pub avg_response_time: i32,
    pub last_check: Option<DateTime<Utc>>,
    #[serde(skip_serializing)]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_delete_after_failed_seconds: Option<i32>,
    pub invalid_since: Option<DateTime<Utc>>,
    pub deleted_at: DateTime<Utc>,
    pub failure_reasons: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Deleted proxies list query parameters
#[derive(Debug, Clone, Deserialize, Default)]
pub struct DeletedProxyListParams {
    pub page: Option<i64>,
    pub limit: Option<i64>,
}

/// Bulk create proxies request
#[derive(Debug, Clone, Deserialize)]
pub struct BulkCreateProxiesRequest {
    pub proxies: Vec<CreateProxyRequest>,
}

/// Bulk delete proxies request
#[derive(Debug, Clone, Deserialize)]
pub struct BulkDeleteProxiesRequest {
    pub ids: Vec<i32>,
}

/// Proxy list query parameters
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProxyListParams {
    pub page: Option<i64>,
    pub limit: Option<i64>,
    pub search: Option<String>,
    pub status: Option<String>,
    pub protocol: Option<String>,
    pub sort_field: Option<String>,
    pub sort_order: Option<String>,
}

/// Paginated response wrapper
#[derive(Debug, Clone, Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub limit: i64,
    pub total_pages: i64,
}

impl<T> PaginatedResponse<T> {
    pub fn new(data: Vec<T>, total: i64, page: i64, limit: i64) -> Self {
        let total_pages = (total as f64 / limit as f64).ceil() as i64;
        PaginatedResponse {
            data,
            total,
            page,
            limit,
            total_pages,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RotationSettings;

    fn base_proxy() -> Proxy {
        Proxy {
            id: 1,
            address: "127.0.0.1:8080".to_string(),
            protocol: "http".to_string(),
            username: None,
            password: None,
            status: "idle".to_string(),
            requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            avg_response_time: 0,
            last_check: None,
            last_error: None,
            auto_delete_after_failed_seconds: None,
            invalid_since: None,
            failure_reasons: serde_json::Value::Array(Vec::new()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_proxy_protocol_parsing_and_helpers() {
        assert_eq!(ProxyProtocol::from_str("HTTP"), Some(ProxyProtocol::Http));
        assert_eq!(ProxyProtocol::from_str("https"), Some(ProxyProtocol::Https));
        assert_eq!(
            ProxyProtocol::from_str("SOCKS4A"),
            Some(ProxyProtocol::Socks4a)
        );
        assert_eq!(ProxyProtocol::from_str("unknown"), None);

        assert!(ProxyProtocol::Socks5.is_socks());
        assert!(!ProxyProtocol::Https.is_socks());
        assert!(ProxyProtocol::Https.is_http());
        assert!(!ProxyProtocol::Socks4.is_http());

        assert_eq!(ProxyProtocol::Socks4.to_string(), "socks4");
    }

    #[test]
    fn test_proxy_status_parsing_and_is_usable() {
        assert_eq!(ProxyStatus::from_str("idle"), Some(ProxyStatus::Idle));
        assert_eq!(ProxyStatus::from_str("ACTIVE"), Some(ProxyStatus::Active));
        assert_eq!(ProxyStatus::from_str("failed"), Some(ProxyStatus::Failed));
        assert_eq!(ProxyStatus::from_str("unknown"), None);

        assert!(ProxyStatus::Idle.is_usable());
        assert!(ProxyStatus::Active.is_usable());
        assert!(!ProxyStatus::Failed.is_usable());

        assert_eq!(ProxyStatus::Active.to_string(), "active");
    }

    #[test]
    fn test_proxy_success_rate_and_is_usable() {
        let mut proxy = base_proxy();
        assert_eq!(proxy.success_rate(), 0.0);
        assert!(proxy.is_usable());

        proxy.requests = 10;
        proxy.successful_requests = 7;
        assert!((proxy.success_rate() - 70.0).abs() < 1e-9);

        proxy.status = "failed".to_string();
        assert!(!proxy.is_usable());

        proxy.status = "invalid".to_string();
        assert!(!proxy.is_usable());
    }

    #[test]
    fn test_proxy_matches_filter() {
        let mut proxy = base_proxy();
        proxy.protocol = "http".to_string();
        proxy.avg_response_time = 200;
        proxy.requests = 10;
        proxy.successful_requests = 5;

        let mut settings = RotationSettings::default();

        settings.allowed_protocols = vec!["http".to_string()];
        assert!(proxy.matches_filter(&settings));

        settings.allowed_protocols = vec!["socks5".to_string()];
        assert!(!proxy.matches_filter(&settings));

        settings.allowed_protocols.clear();
        settings.max_response_time = 100;
        assert!(!proxy.matches_filter(&settings));

        settings.max_response_time = 0;
        settings.min_success_rate = 60.0;
        assert!(!proxy.matches_filter(&settings));
    }

    #[test]
    fn test_proxy_url_formats() {
        let mut proxy = base_proxy();
        proxy.address = "1.2.3.4:1234".to_string();

        proxy.protocol = "http".to_string();
        assert_eq!(proxy.url(), "http://1.2.3.4:1234");

        proxy.protocol = "https".to_string();
        assert_eq!(proxy.url(), "http://1.2.3.4:1234");

        proxy.protocol = "socks5".to_string();
        assert_eq!(proxy.url(), "socks5://1.2.3.4:1234");
    }

    #[test]
    fn test_proxy_url_with_auth() {
        let mut proxy = base_proxy();
        proxy.address = "1.2.3.4:1234".to_string();

        proxy.username = Some("user".to_string());
        proxy.password = Some("pass".to_string());
        assert_eq!(proxy.url(), "http://user:pass@1.2.3.4:1234");

        proxy.password = None;
        assert_eq!(proxy.url(), "http://user@1.2.3.4:1234");
    }

    #[test]
    fn test_paginated_response_total_pages() {
        let resp = PaginatedResponse::new(vec![1, 2, 3], 0, 1, 10);
        assert_eq!(resp.total_pages, 0);

        let resp = PaginatedResponse::new(vec![1], 1, 1, 10);
        assert_eq!(resp.total_pages, 1);

        let resp = PaginatedResponse::new(vec![1; 10], 10, 1, 10);
        assert_eq!(resp.total_pages, 1);

        let resp = PaginatedResponse::new(vec![1; 10], 11, 1, 10);
        assert_eq!(resp.total_pages, 2);
    }
}
