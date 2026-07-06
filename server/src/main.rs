use std::net::IpAddr;

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

    /// IP address to bind the HTTP API server.
    /// Use 0.0.0.0 to accept remote connections (e.g. from GUI clients).
    #[arg(short = 'a', long, default_value = "127.0.0.1")]
    bind: IpAddr,

    /// Enable the experimental custom DHT network.
    #[arg(long)]
    dht: bool,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    info!("QVOD server starting");
    info!(
        "Configuration: config={:?}, port={}, bind={}, dht={}",
        if cli.config.is_empty() {
            "default"
        } else {
            &cli.config
        },
        cli.port,
        cli.bind,
        cli.dht
    );

    let mut engine_config = if cli.config.is_empty() {
        info!("Using default engine configuration");
        EngineConfig::default()
    } else {
        info!("Loading engine configuration from: {}", cli.config);
        let cfg = EngineConfig::load(&cli.config);
        match &cfg {
            Ok(_) => info!("Engine configuration loaded successfully"),
            Err(e) => tracing::warn!("Failed to load config, using defaults: {e}"),
        }
        cfg.unwrap_or_default()
    };
    engine_config.listen_port = cli.port;
    if cli.dht {
        info!("DHT mode enabled");
        engine_config.dht_enabled = true;
    }

    info!("Initializing QVOD engine...");
    let engine = QvodEngine::new(engine_config).await;
    info!("QVOD engine initialized");

    let server_config = LocalServerConfig::new(cli.port).with_bind_address(cli.bind);

    info!("Starting QVOD HTTP server on {}:{}...", cli.bind, cli.port);
    let mut server = LocalServer::new(&server_config, engine)
        .await
        .expect("Failed to start local server");
    let actual_port = server.port();
    info!(
        "QVOD HTTP server started on http://{}:{}/",
        cli.bind, actual_port
    );

    info!("QVOD server running. Press Ctrl+C to stop.");
    signal::ctrl_c().await.expect("failed to listen for signal");
    info!("Shutdown signal received, stopping server...");

    server.stop();
    info!("QVOD server stopped.");
}
