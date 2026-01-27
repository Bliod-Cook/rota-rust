//! Rota Proxy Server - Entry Point
//!
//! Starts both the proxy server and API server with graceful shutdown support.

use std::sync::Arc;

use tokio::signal;
use tokio::sync::{broadcast, watch};
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod config;
mod database;
mod error;
mod models;
mod proxy;
mod repository;
mod services;

use api::ApiServer;
use config::Config;
use database::Database;
use proxy::health::{HealthChecker, HealthCheckerConfig, HealthCheckerHandle};
use proxy::rotation::{create_selector, RotationStrategy};
use proxy::server::ProxyServer;
use services::{LogCleanupConfig, LogCleanupHandle, LogCleanupService};

#[tokio::main]
async fn main() -> error::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rota=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting Rota Proxy Server");

    // Load configuration
    let config = Config::from_env()?;
    info!("Configuration loaded");

    // Connect to database
    let db = Database::new(&config).await?;
    info!("Connected to database");

    // Run migrations
    db.run_migrations().await?;
    info!("Database migrations complete");

    // Initialize TimescaleDB if available
    if let Err(e) = database::timescale::setup_timescaledb(db.pool()).await {
        info!(
            "TimescaleDB setup skipped or failed: {} (this is OK if not using TimescaleDB)",
            e
        );
    }

    // Create log broadcast channel (bounded to prevent memory leaks)
    let (log_sender, _) = broadcast::channel::<models::RequestRecord>(1024);

    // Create proxy selector
    let strategy = RotationStrategy::from_str(&config.proxy.rotation_strategy);
    let selector: Arc<dyn proxy::rotation::ProxySelector> = Arc::from(create_selector(strategy));
    info!("Using rotation strategy: {}", strategy.as_str());

    // Load initial proxies into selector
    let proxy_repo = repository::ProxyRepository::new(db.pool().clone());
    let proxies = proxy_repo.get_all_usable().await?;
    selector.refresh(proxies).await?;
    info!("Loaded {} proxies", selector.available_count());

    // Create shutdown channels
    let (shutdown_tx, _) = watch::channel(false);

    // Start health checker
    let (health_handle, health_shutdown) = HealthCheckerHandle::new();
    let health_checker =
        HealthChecker::new(db.clone(), HealthCheckerConfig::default(), selector.clone());
    let health_task = tokio::spawn(async move {
        health_checker.run(health_shutdown).await;
    });

    // Start log cleanup service
    let (cleanup_handle, cleanup_shutdown) = LogCleanupHandle::new();
    let cleanup_service = LogCleanupService::new(db.clone(), LogCleanupConfig::default());
    let cleanup_task = tokio::spawn(async move {
        cleanup_service.run(cleanup_shutdown).await;
    });

    // Create proxy server
    let proxy_server = ProxyServer::new(
        config.proxy.clone(),
        selector.clone(),
        db.pool().clone(),
        Some(log_sender.clone()),
    );

    // Create API server
    let api_server = ApiServer::new(
        config.api.clone(),
        config.clone(),
        db.clone(),
        selector.clone(),
        log_sender.clone(),
    );

    // Start servers
    let proxy_shutdown = shutdown_tx.subscribe();
    let api_shutdown = shutdown_tx.subscribe();

    let proxy_task = tokio::spawn(async move {
        if let Err(e) = proxy_server.run(proxy_shutdown).await {
            error!("Proxy server error: {}", e);
        }
    });

    let api_task = tokio::spawn(async move {
        if let Err(e) = api_server.run(api_shutdown).await {
            error!("API server error: {}", e);
        }
    });

    info!(
        "Servers started - Proxy: {}:{}, API: {}:{}",
        config.proxy.host, config.proxy.port, config.api.host, config.api.port
    );

    // Wait for shutdown signal
    shutdown_signal().await;
    info!("Shutdown signal received");

    // Send shutdown signal to all services
    let _ = shutdown_tx.send(true);
    health_handle.shutdown();
    cleanup_handle.shutdown();

    // Wait for all tasks to complete
    let _ = tokio::join!(proxy_task, api_task, health_task, cleanup_task);

    info!("Rota Proxy Server stopped");
    Ok(())
}

/// Wait for shutdown signal (Ctrl+C or SIGTERM)
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
