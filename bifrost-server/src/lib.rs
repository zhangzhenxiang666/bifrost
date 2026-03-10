//! Bifrost - A mapping service for LLM providers
//!
//! This library provides adapters for multiple LLM providers
//! and routes for handling mapping requests.

pub mod adapter;
pub mod config;
pub mod error;
pub mod middleware;
pub mod model;
pub mod provider;
pub mod routes;
pub mod state;
pub mod util;

use crate::middleware::request_logger;
use crate::provider::registry::ProviderRegistry;
use crate::routes::{chat_completions, messages};
use crate::state::AppState;

use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

pub fn run_server(config: config::Config) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("failed to create Tokio runtime");

    runtime.block_on(server(config))
}

async fn server(config: config::Config) -> anyhow::Result<()> {
    // Logging should be initialized by the caller (CLI)
    // If not initialized, tracing macros will be no-ops

    info!("Bifrost service starting...");
    info!("Version: {}", env!("CARGO_PKG_VERSION"));

    let port = config.server.port;
    info!("Starting server on port {}", port);

    // Create provider registry
    let registry = ProviderRegistry::from_config(&config);
    let state = AppState {
        registry: Arc::new(registry),
    };

    // Configure CORS - allow all origins for API access
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers(Any);

    // Build router with middleware
    let app = Router::new()
        .route(
            "/openai/chat/completions",
            axum::routing::post(chat_completions),
        )
        .route("/anthropic/v1/messages", axum::routing::post(messages))
        .with_state(state)
        // Add request logging middleware
        .layer(axum::middleware::from_fn(request_logger))
        // Add CORS middleware (must be last)
        .layer(cors);

    // Bind and listen
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to bind to address {}: {}", addr, e))?;

    info!("proxy is: {:?}", config.server.proxy);
    info!("Bifrost service is ready on http://{}", addr);

    // Use into_make_service_with_connect_info to enable ConnectInfo extraction
    let app = app.into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, app).await?;

    Ok(())
}
