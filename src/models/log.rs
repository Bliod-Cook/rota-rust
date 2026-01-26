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
