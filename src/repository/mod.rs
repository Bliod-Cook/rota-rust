pub mod dashboard;
pub mod deleted_proxy;
pub mod log;
pub mod proxy;
pub mod settings;

pub use dashboard::DashboardRepository;
pub use deleted_proxy::DeletedProxyRepository;
pub use log::LogRepository;
pub use proxy::ProxyRepository;
pub use settings::SettingsRepository;
