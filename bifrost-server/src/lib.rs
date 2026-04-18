//! Bifrost - A mapping service for LLM providers
//!
//! This library provides adapters for multiple LLM providers
//! and routes for handling mapping requests.

pub mod adapter;
pub mod error;
pub mod middleware;
pub mod model;
pub mod provider;
pub mod routes;
pub mod state;
pub mod util;

// Re-export config from bifrost-config
pub use bifrost_config::Config;
pub use bifrost_config::types;

use crate::middleware::request_logger;
use crate::provider::registry::ProviderRegistry;
use crate::routes::{
    anthropic::messages, anthropic::messages_v1, openai::chat_completions,
    openai::chat_completions_v1, openai::responses, openai::responses_v1, status::status,
};
use crate::state::{AppState, get_global_state, set_global_state};
use bifrost_config::usage::cleanup_old_usage_files;

use axum::Router;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

pub fn run_server(config: Config) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("failed to create Tokio runtime");

    runtime.block_on(server(config))
}

async fn server(config: Config) -> anyhow::Result<()> {
    info!("Bifrost service starting...");
    info!("Version: {}", env!("CARGO_PKG_VERSION"));

    cleanup_old_usage_files(90);

    let port = config.server.port;
    info!("Starting server on port {}", port);

    let registry = ProviderRegistry::from_config(&config);
    let proxy = config.server.proxy.clone();
    set_global_state(AppState::new(registry, proxy));

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers(Any);

    let llm_router = Router::new()
        .route(
            "/openai/chat/completions",
            axum::routing::post(chat_completions),
        )
        .route(
            "/openai/v1/chat/completions",
            axum::routing::post(chat_completions_v1),
        )
        .route("/openai/responses", axum::routing::post(responses))
        .route("/openai/v1/responses", axum::routing::post(responses_v1))
        .route("/anthropic/messages", axum::routing::post(messages))
        .route("/anthropic/v1/messages", axum::routing::post(messages_v1))
        .layer(axum::middleware::from_fn(request_logger));

    let main_router = Router::new()
        .route("/status", axum::routing::get(status))
        .merge(llm_router);

    let app = main_router
        .with_state(get_global_state().clone())
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
