mod config;
mod data;
mod debug;
mod mem_reader;
mod network;
mod notifier;
mod scanner;
mod server_logic;
mod session;

use std::sync::Arc;

use clap::{Parser, Subcommand};
use config::IniReader;
use debug::DebugLoop;
use mem_reader::MemReader;
use network::{NetworkServer, StubDataProvider};
use notifier::{LoggingNotifier, UiNotifier};
use scanner::EqGameScanner;
use session::SessionRunner;

#[derive(Parser)]
#[command(name = "WinShowEQServer", about = "WinShowEQ — EverQuest map overlay server")]
struct Cli {
    /// Use an alternate INI file path instead of myseqserver.ini
    #[arg(short = 'f', value_name = "FILE")]
    ini_file: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Headless console mode (default)
    Console,
    /// Interactive debug command loop for offset discovery
    Debug,
    // Dev/milestone utilities — hidden from help output.
    #[command(hide = true)]
    Scan { exe_path: String },
    #[command(hide = true)]
    Attach,
    #[command(hide = true)]
    ServeStub,
}

fn main() {
    let cli = Cli::parse();
    let ini = cli.ini_file.as_deref();

    match cli.command {
        None | Some(Command::Console) => run_console(ini),
        Some(Command::Debug) => run_debug(ini),
        Some(Command::Scan { exe_path }) => run_scan(&exe_path, ini),
        Some(Command::Attach) => run_attach(),
        Some(Command::ServeStub) => run_serve_stub(),
    }
}

fn run_console(ini_override: Option<&str>) {
    let ini_path = ini_override
        .map(str::to_owned)
        .unwrap_or_else(|| resolve_ini_path("myseqserver.ini"));
    let config_ini_path = resolve_ini_path("config.ini");
    let mut runner = SessionRunner::new(ini_path, config_ini_path);
    runner.run_console_loop();
}

fn run_debug(ini_override: Option<&str>) {
    let ini_path = ini_override
        .map(str::to_owned)
        .unwrap_or_else(|| resolve_ini_path("myseqserver.ini"));
    let config_ini_path = resolve_ini_path("config.ini");

    let mut ir = IniReader::new();
    let _ = ir.open_file(&ini_path);
    ir.open_config_file(&config_ini_path);

    MemReader::enable_debug_privileges();
    let mut mem = MemReader::new();

    match MemReader::find_process("eqgame.exe") {
        Some(pid) => match mem.open(pid) {
            Ok(()) => println!(
                "Attached to eqgame.exe  PID: {}  Base: 0x{:X}",
                mem.pid(),
                mem.base_address()
            ),
            Err(e) => eprintln!("Warning: could not attach to eqgame.exe — {e}"),
        },
        None => eprintln!("Warning: eqgame.exe not found — memory commands will fail"),
    }

    DebugLoop::new().enter_debug_loop(&mut mem, &mut ir);
}

fn run_attach() {
    MemReader::enable_debug_privileges();
    match MemReader::find_process("eqgame.exe") {
        Some(pid) => {
            let mut reader = MemReader::new();
            match reader.open(pid) {
                Ok(()) => println!(
                    "Attached to eqgame.exe  PID: {}  Base: 0x{:X}",
                    reader.pid(),
                    reader.base_address()
                ),
                Err(e) => eprintln!("Failed to open process: {e}"),
            }
        }
        None => eprintln!("eqgame.exe not found"),
    }
}

fn run_serve_stub() {
    let notifier = Arc::new(LoggingNotifier::new(true));
    let mut server = NetworkServer::new(5555);
    server.set_notifier(Arc::clone(&notifier) as Arc<dyn UiNotifier>);
    println!("WinShowEQ: Starting stub server on port 5555...");
    server.serve(Arc::new(StubDataProvider));
}

fn run_scan(exe_path: &str, ini_override: Option<&str>) {
    if exe_path.is_empty() {
        eprintln!("Usage: WinShowEQServer scan <path\\to\\eqgame.exe>");
        return;
    }

    let ini_path = ini_override
        .map(str::to_owned)
        .unwrap_or_else(|| resolve_ini_path("myseqserver.ini"));
    let config_ini_path = resolve_ini_path("config.ini");

    let mut ir = IniReader::new();
    ir.open_config_file(&config_ini_path);
    let _ = ir.open_file(&ini_path);

    let current_offsets = ir
        .read_server_config_model()
        .map(|m| m.offsets)
        .unwrap_or_default();

    let scanner = EqGameScanner::new(exe_path);
    let result = scanner.scan_executable(&ir, &current_offsets, false);
    print!("{}", result.output);
}

fn resolve_ini_path(name: &str) -> String {
    // GetPrivateProfileStringW requires an absolute path — relative paths resolve to
    // C:\Windows, not the current working directory.
    std::env::current_dir()
        .map(|d| d.join(name).to_string_lossy().into_owned())
        .unwrap_or_else(|_| name.to_string())
}