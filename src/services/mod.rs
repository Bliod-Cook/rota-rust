//! Background services

pub mod log_cleanup;
pub mod proxy_auto_delete;

pub use log_cleanup::{LogCleanupConfig, LogCleanupHandle, LogCleanupService};
pub use proxy_auto_delete::{ProxyAutoDeleteConfig, ProxyAutoDeleteHandle, ProxyAutoDeleteService};
