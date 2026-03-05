use axum::Router;
use llm_map::config::Config;
use llm_map::provider::registry::ProviderRegistry;
use llm_map::routes::{AppState, chat_completions};
use llm_map::middleware::request_logger;
use llm_map::utils::init_logging;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    init_logging();

    info!("LLM Map service starting...");
    info!("Version: {}", env!("CARGO_PKG_VERSION"));

    // Load configuration from config.toml (supports LLM_MAP_CONFIG env var)
    let config_path = std::env::var("LLM_MAP_CONFIG").unwrap_or_else(|_| "config.toml".to_string());
    let config = Config::from_file(&config_path)
        .map_err(|e| anyhow::anyhow!("Failed to load config from '{}': {}", config_path, e))?;

    let port = config.server.port;
    info!("Starting server on port {}", port);

    // Create provider registry
    let registry = ProviderRegistry::from_config(&config);
    let state = AppState { registry };

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

    info!("Server listening on http://{}", addr);
    info!("proxy is: {:?}", config.server.proxy);
    info!("LLM Map service is ready on http://{}", addr);

    // Use into_make_service_with_connect_info to enable ConnectInfo extraction
    let app = app.into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, app).await?;

    Ok(())
}
