use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Dashboard statistics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DashboardStats {
    /// Number of active proxies
    pub active_proxies: i64,
    /// Total number of proxies
    pub total_proxies: i64,
    /// Total requests processed
    pub total_requests: i64,
    /// Average success rate (0-100)
    pub avg_success_rate: f64,
    /// Average response time in milliseconds
    pub avg_response_time: i32,
    /// Request count growth percentage (vs previous period)
    pub request_growth: f64,
    /// Success rate change (vs previous period)
    pub success_rate_growth: f64,
    /// Response time change in ms (vs previous period)
    pub response_time_delta: i32,
}

/// Chart data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartDataPoint {
    pub timestamp: DateTime<Utc>,
    pub value: f64,
}

/// Chart data response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartData {
    pub data: Vec<ChartDataPoint>,
    pub label: String,
}

/// Time range for chart queries
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ChartTimeRange {
    /// Time range: 1h, 6h, 24h, 7d, 30d
    pub range: Option<String>,
    /// Custom start time
    pub start: Option<DateTime<Utc>>,
    /// Custom end time
    pub end: Option<DateTime<Utc>>,
}

impl ChartTimeRange {
    /// Get the start time based on range or custom value
    pub fn start_time(&self) -> DateTime<Utc> {
        if let Some(start) = self.start {
            return start;
        }

        let now = Utc::now();
        let range = self.range.as_deref().unwrap_or("24h");

        match range {
            "1h" => now - chrono::Duration::hours(1),
            "6h" => now - chrono::Duration::hours(6),
            "24h" => now - chrono::Duration::hours(24),
            "7d" => now - chrono::Duration::days(7),
            "30d" => now - chrono::Duration::days(30),
            _ => now - chrono::Duration::hours(24),
        }
    }

    /// Get the end time
    pub fn end_time(&self) -> DateTime<Utc> {
        self.end.unwrap_or_else(Utc::now)
    }

    /// Get the aggregation interval for this range
    pub fn interval(&self) -> &'static str {
        let range = self.range.as_deref().unwrap_or("24h");

        match range {
            "1h" => "1 minute",
            "6h" => "5 minutes",
            "24h" => "1 hour",
            "7d" => "6 hours",
            "30d" => "1 day",
            _ => "1 hour",
        }
    }
}

/// System metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    /// CPU usage percentage
    pub cpu_usage: f64,
    /// Memory usage percentage
    pub memory_usage: f64,
    /// Total memory in bytes
    pub memory_total: u64,
    /// Used memory in bytes
    pub memory_used: u64,
    /// Uptime in seconds
    pub uptime: u64,
    /// Number of active connections
    pub active_connections: u64,
}

/// Database health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseHealth {
    pub connected: bool,
    pub latency_ms: i32,
    pub pool_size: u32,
    pub pool_idle: u32,
}

/// Overall health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub version: String,
    pub uptime: u64,
    pub database: DatabaseHealth,
}
