//! API server using Axum
//!
//! Provides REST API endpoints for managing proxies, settings, and viewing logs.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use tokio::sync::{broadcast, watch};
use tower_http::trace::TraceLayer;
use tracing::{info, instrument};

use crate::config::{ApiServerConfig, Config};
use crate::database::Database;
use crate::error::Result;
use crate::models::RequestRecord;
use crate::proxy::rotation::ProxySelector;

use super::middleware::{cors_layer, JwtAuth};
use super::routes;

/// Shared state for API handlers
#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub config: Config,
    pub jwt_auth: JwtAuth,
    pub started_at: Instant,
    pub selector: Arc<dyn ProxySelector>,
    pub log_sender: broadcast::Sender<RequestRecord>,
}

/// API server
pub struct ApiServer {
    config: ApiServerConfig,
    state: AppState,
}

impl ApiServer {
    /// Create a new API server
    pub fn new(
        api_config: ApiServerConfig,
        full_config: Config,
        db: Database,
        selector: Arc<dyn ProxySelector>,
        log_sender: broadcast::Sender<RequestRecord>,
    ) -> Self {
        let jwt_auth = JwtAuth::new(&api_config.jwt_secret);

        let state = AppState {
            db,
            config: full_config,
            jwt_auth,
            started_at: Instant::now(),
            selector,
            log_sender,
        };

        Self {
            config: api_config,
            state,
        }
    }

    /// Build the router
    fn build_router(&self) -> Router {
        let cors = cors_layer(&self.config.cors_origins);

        routes::create_router(self.state.clone())
            .layer(cors)
            .layer(TraceLayer::new_for_http())
    }

    /// Run the API server
    #[instrument(skip(self, shutdown))]
    pub async fn run(&self, mut shutdown: watch::Receiver<bool>) -> Result<()> {
        let addr: SocketAddr = format!("{}:{}", self.config.host, self.config.port)
            .parse()
            .expect("Invalid API server address");

        let router = self.build_router();

        info!("API server listening on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;

        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown.changed().await;
            })
            .await
            .map_err(|e| crate::error::RotaError::Internal(e.to_string()))?;

        info!("API server shut down");
        Ok(())
    }
}

/// Builder for creating an API server
pub struct ApiServerBuilder {
    api_config: ApiServerConfig,
    full_config: Option<Config>,
    db: Option<Database>,
    selector: Option<Arc<dyn ProxySelector>>,
    log_sender: Option<broadcast::Sender<RequestRecord>>,
}

impl ApiServerBuilder {
    pub fn new(api_config: ApiServerConfig) -> Self {
        Self {
            api_config,
            full_config: None,
            db: None,
            selector: None,
            log_sender: None,
        }
    }

    pub fn config(mut self, config: Config) -> Self {
        self.full_config = Some(config);
        self
    }

    pub fn database(mut self, db: Database) -> Self {
        self.db = Some(db);
        self
    }

    pub fn selector(mut self, selector: Arc<dyn ProxySelector>) -> Self {
        self.selector = Some(selector);
        self
    }

    pub fn log_sender(mut self, sender: broadcast::Sender<RequestRecord>) -> Self {
        self.log_sender = Some(sender);
        self
    }

    pub fn build(self) -> ApiServer {
        let full_config = self.full_config.expect("Config is required");
        let db = self.db.expect("Database is required");
        let selector = self.selector.expect("Proxy selector is required");
        let log_sender = self.log_sender.expect("Log sender is required");

        ApiServer::new(self.api_config, full_config, db, selector, log_sender)
    }
}
