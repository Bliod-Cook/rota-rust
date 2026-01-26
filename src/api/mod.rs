//! API server implementation
//!
//! Provides REST API and WebSocket endpoints for managing the proxy system.

pub mod handlers;
pub mod middleware;
pub mod routes;
pub mod server;
pub mod websocket;

pub use server::ApiServer;
