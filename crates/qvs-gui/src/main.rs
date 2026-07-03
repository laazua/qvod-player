// On Windows, mark the executable as a GUI subsystem application so that
// Windows does NOT create a console window when launching the GUI player.
// (Applied unconditionally on Windows — both debug and release builds.)
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use clap::{Parser, Subcommand};

use qvs_gui::app::QvodApp;

#[derive(Parser)]
#[command(name = "qvs", version, about = "Qvod P2SP Player")]
struct Cli {
    /// Remote QVOD server URL (e.g. `http://192.168.1.100:8621`).
    /// When set, the GUI acts as a thin client connecting to the remote server.
    /// Can also be set via `QVS_SERVER_URL` environment variable or baked in at compile time.
    #[arg(long)]
    server_url: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Play a qvod:// URI
    Play { uri: String },
    /// Show engine status
    Status,
    /// List active streams
    List,
    /// Manage cache
    Cache { clean: bool, size: Option<u64> },
    /// Open settings
    Settings,
}

fn main() -> Result<(), eframe::Error> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    // Check env var if --server-url wasn't provided on CLI
    let server_url = cli
        .server_url
        .or_else(|| std::env::var("QVS_SERVER_URL").ok());

    let startup_uri = match &cli.command {
        Some(Commands::Play { uri }) => Some(uri.clone()),
        // NOTE: CLI subcommands (Status/List/Cache) are stubs here because the
        // real CLI lives in the `qvs-cli` crate.  On Windows the binary is
        // marked as a GUI subsystem application, so when running those
        // subcommands from a console the output will not be visible — use
        // `qvs-cli` instead.
        Some(Commands::Status) => {
            println!("Status: not yet implemented");
            return Ok(());
        }
        Some(Commands::List) => {
            println!("Playlist: not yet implemented");
            return Ok(());
        }
        Some(Commands::Cache { .. }) => {
            println!("Cache: not yet implemented");
            return Ok(());
        }
        Some(Commands::Settings) | None => None,
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([960.0, 600.0])
            .with_decorations(false),
        ..Default::default()
    };

    eframe::run_native(
        "QVOD Player",
        options,
        Box::new(move |cc| {
            // Set up CJK font fallback so Chinese text renders correctly on Windows
            qvs_gui::fonts::setup_cjk_fonts(&cc.egui_ctx);
            Box::new(QvodApp::new(startup_uri, server_url))
        }),
    )
}
