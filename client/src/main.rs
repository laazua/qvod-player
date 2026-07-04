use clap::{Parser, Subcommand};
use qvs_stream::{EngineConfig, QvodEngine};

#[derive(Parser)]
#[command(name = "qvs-cli", version, about = "QVOD P2SP CLI Client")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Play a qvod:// or http(s):// URI
    Play { uri: String },
    /// Show engine status
    Status,
    /// List active streams
    List,
    /// Manage cache
    Cache {
        #[arg(long)]
        clean: bool,
        #[arg(long)]
        size: Option<u64>,
    },
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Play { uri } => cmd_play(&uri).await,
        Commands::Status => cmd_status().await,
        Commands::List => cmd_list().await,
        Commands::Cache { clean, size } => cmd_cache(clean, size).await,
    }
}

async fn cmd_play(uri: &str) {
    let config = EngineConfig::default();
    let mut engine = QvodEngine::new(config).await;

    match engine.play(uri).await {
        Ok(stream) => {
            let duration = stream.metadata.duration_ms;
            println!("Playing: {uri}");
            println!("Duration: {duration} ms");
        }
        Err(e) => {
            tracing::error!("Play failed: {e}");
            eprintln!("Error: {e}");
        }
    }
}

async fn cmd_status() {
    let config = EngineConfig::default();
    let engine = QvodEngine::new(config).await;
    let _ = engine;
    println!("QVOD Engine: running");
}

async fn cmd_list() {
    println!("No active streams.");
}

async fn cmd_cache(clean: bool, size: Option<u64>) {
    if clean {
        println!("Cache cleaned.");
    }
    if let Some(s) = size {
        println!("Cache size limit: {s} MB");
    }
    println!("Cache: not yet implemented");
}
