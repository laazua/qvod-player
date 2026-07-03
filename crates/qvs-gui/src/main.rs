use clap::{Parser, Subcommand};

use qvs_gui::app::QvodApp;

#[derive(Parser)]
#[command(name = "qvs", version, about = "Qvod P2SP Player")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Play { uri: String },
    Status,
    List,
    Cache { clean: bool, size: Option<u64> },
    Settings,
}

fn main() -> Result<(), eframe::Error> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let startup_uri = match &cli.command {
        Some(Commands::Play { uri }) => Some(uri.clone()),
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
        Box::new(move |_cc| Box::new(QvodApp::new(startup_uri))),
    )
}
