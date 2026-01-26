//! Rota - Proxy Rotation System
//!
//! A high-performance proxy rotation system written in Rust.
//!
//! ## Features
//!
//! - Multiple proxy rotation strategies (random, round-robin, least-connections, time-based)
//! - HTTP, HTTPS, SOCKS4, SOCKS4a, and SOCKS5 proxy support
//! - Health checking for upstream proxies
//! - Rate limiting and authentication
//! - Real-time dashboard with WebSocket updates
//! - Log management with configurable retention
//! - PostgreSQL with TimescaleDB support

pub mod api;
pub mod config;
pub mod database;
pub mod error;
pub mod models;
pub mod proxy;
pub mod repository;
pub mod services;

pub use config::Config;
pub use database::Database;
pub use error::{Result, RotaError};
