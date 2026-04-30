mod config;
mod data;
mod filters;
mod logger;
mod map_con;
mod map_reader;
mod net;
mod protocol;
mod ui;

use std::net::SocketAddr;

use clap::Parser;

use ui::main_window::MainApp;

#[derive(Parser)]
#[command(name = "WinShowEQClient", about = "WinShowEQ Rust client")]
struct Cli {
    /// Connect to a WinShowEQ server (e.g. 127.0.0.1:5555)
    #[arg(long, value_name = "IP:PORT")]
    connect: Option<SocketAddr>,
}

fn main() -> eframe::Result {
    let cli = Cli::parse();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("WinShowEQ Client")
            .with_inner_size([900.0, 700.0]),
        ..Default::default()
    };

    eframe::run_native(
        "WinShowEQ Client",
        options,
        Box::new(move |cc| Ok(Box::new(MainApp::new(cc, cli.connect)))),
    )
}