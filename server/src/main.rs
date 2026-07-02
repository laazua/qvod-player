use clap::Parser;
use qvs_local_server::{LocalServer, LocalServerConfig};
use qvs_stream::{EngineConfig, QvodEngine};
use tokio::signal;
use tracing::info;

#[derive(Parser)]
#[command(name = "qvs-server", version, about = "QVOD P2SP Headless Server")]
struct Cli {
    #[arg(short, long, default_value = "")]
    config: String,

    #[arg(short, long, default_value_t = 8621)]
    port: u16,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let mut engine_config = if cli.config.is_empty() {
        EngineConfig::default()
    } else {
        EngineConfig::load(&cli.config).unwrap_or_default()
    };
    engine_config.listen_port = cli.port;

    let engine = QvodEngine::new(engine_config).await;
    let server_config = LocalServerConfig::new(cli.port);

    info!("Starting QVOD server on port {}...", cli.port);
    let mut server = LocalServer::new(&server_config, engine)
        .await
        .expect("Failed to start local server");
    let actual_port = server.port();
    info!("Local HTTP server started on port {actual_port}");

    info!("QVOD server running. Press Ctrl+C to stop.");
    signal::ctrl_c().await.expect("failed to listen for signal");
    info!("Shutdown signal received, stopping server...");

    server.stop();
    info!("Server stopped.");
}
