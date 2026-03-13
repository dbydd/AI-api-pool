mod config;
mod health_check;
mod load_balancer;
mod providers;
mod server;

use anyhow::Result;
use tracing_subscriber::fmt::format::FmtSpan;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_span_events(FmtSpan::CLOSE)
        .with_target(false)
        .init();

    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.yaml".to_string());

    tracing::info!("Loading configuration from: {}", config_path);

    let config = config::Config::load(&config_path)?;
    
    tracing::info!("Loaded {} models", config.models.len());
    
    for (name, model) in &config.models {
        tracing::info!("Model '{}' has {} providers", name, model.providers.len());
    }

    server::start_server(config).await?;

    Ok(())
}
