//! WebSocket handlers
//!
//! FIXED: Uses bounded channels with try_send to prevent memory leaks.
//! The Go implementation used unbounded 10000-buffer channels without backpressure.

pub mod dashboard;
pub mod logs;

/// Maximum number of messages to buffer per WebSocket connection
pub const WS_BUFFER_SIZE: usize = 256;
