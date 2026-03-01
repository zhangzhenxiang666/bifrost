use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("LLM Map service starting...");
    info!("Version: {}", env!("CARGO_PKG_VERSION"));
    
    println!("LLM Map service is ready!");
}