mod server;

use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};
use server::UppercutMcp;
use tracing_subscriber::{fmt, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!("uppercut-mcp starting (stdio)");
    let service = UppercutMcp::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
