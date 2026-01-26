use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;

/// Log level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
    Success,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warning => "warning",
            LogLevel::Error => "error",
            LogLevel::Success => "success",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "debug" => Some(LogLevel::Debug),
            "info" => Some(LogLevel::Info),
            "warning" | "warn" => Some(LogLevel::Warning),
            "error" => Some(LogLevel::Error),
            "success" => Some(LogLevel::Success),
            _ => None,
        }
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Log entry
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Log {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub message: String,
    pub details: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

impl Log {
    /// Get the log level enum
    pub fn level_enum(&self) -> Option<LogLevel> {
        LogLevel::from_str(&self.level)
    }
}

/// Request to create a log entry
#[derive(Debug, Clone)]
pub struct CreateLogRequest {
    pub level: LogLevel,
    pub message: String,
    pub details: Option<String>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
}

impl CreateLogRequest {
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            level: LogLevel::Info,
            message: message.into(),
            details: None,
            metadata: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            level: LogLevel::Error,
            message: message.into(),
            details: None,
            metadata: None,
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            level: LogLevel::Warning,
            message: message.into(),
            details: None,
            metadata: None,
        }
    }

    pub fn success(message: impl Into<String>) -> Self {
        Self {
            level: LogLevel::Success,
            message: message.into(),
            details: None,
            metadata: None,
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.metadata
            .get_or_insert_with(HashMap::new)
            .insert(key.into(), value);
        self
    }
}

/// Log list query parameters
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LogListParams {
    pub page: Option<i64>,
    pub limit: Option<i64>,
    pub level: Option<String>,
    pub search: Option<String>,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
}

/// Request record for proxy usage tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestRecord {
    pub proxy_id: i32,
    pub proxy_address: String,
    pub requested_url: String,
    pub method: String,
    pub success: bool,
    pub response_time: i32,
    pub status_code: i32,
    pub error_message: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_parsing_and_display() {
        assert_eq!(LogLevel::from_str("debug"), Some(LogLevel::Debug));
        assert_eq!(LogLevel::from_str("INFO"), Some(LogLevel::Info));
        assert_eq!(LogLevel::from_str("warn"), Some(LogLevel::Warning));
        assert_eq!(LogLevel::from_str("warning"), Some(LogLevel::Warning));
        assert_eq!(LogLevel::from_str("error"), Some(LogLevel::Error));
        assert_eq!(LogLevel::from_str("success"), Some(LogLevel::Success));
        assert_eq!(LogLevel::from_str("unknown"), None);

        assert_eq!(LogLevel::Warning.to_string(), "warning");
    }

    #[test]
    fn test_create_log_request_builders() {
        let req = CreateLogRequest::info("hello");
        assert_eq!(req.level, LogLevel::Info);
        assert_eq!(req.message, "hello");
        assert!(req.details.is_none());
        assert!(req.metadata.is_none());

        let req = CreateLogRequest::error("oops").with_details("details");
        assert_eq!(req.level, LogLevel::Error);
        assert_eq!(req.details.as_deref(), Some("details"));

        let req = CreateLogRequest::success("ok")
            .with_metadata("proxy_id", serde_json::json!(123))
            .with_metadata("status", serde_json::json!("idle"));

        let metadata = req.metadata.unwrap();
        assert_eq!(metadata.get("proxy_id"), Some(&serde_json::json!(123)));
        assert_eq!(metadata.get("status"), Some(&serde_json::json!("idle")));
    }

    #[test]
    fn test_log_level_enum() {
        let log = Log {
            id: 1,
            timestamp: Utc::now(),
            level: "info".to_string(),
            message: "message".to_string(),
            details: None,
            metadata: None,
        };

        assert_eq!(log.level_enum(), Some(LogLevel::Info));
    }
}
