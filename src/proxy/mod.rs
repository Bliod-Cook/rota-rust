//! Proxy server implementation
//!
//! This module provides the proxy server functionality including:
//! - HTTP/HTTPS proxy handling
//! - CONNECT tunnel for HTTPS
//! - Multiple proxy rotation strategies
//! - Health checking
//! - Request/response handling with retry logic

pub mod handler;
pub mod egress;
pub mod health;
pub mod middleware;
pub mod rotation;
pub mod server;
pub mod transport;
pub mod tunnel;

pub use handler::ProxyHandler;
pub use health::HealthChecker;
pub use rotation::{create_selector, ProxySelector, RotationStrategy};
pub use server::ProxyServer;
pub use transport::ProxyTransport;
pub use tunnel::TunnelHandler;
