use tracing::info;
use llm_map::utils::init_logging;

#[tokio::main]
async fn main() {
    // Initialize logging
    init_logging();

    info!("LLM Map service starting...");
    info!("Version: {}", env!("CARGO_PKG_VERSION"));
    
    println!("LLM Map service is ready!");
}
