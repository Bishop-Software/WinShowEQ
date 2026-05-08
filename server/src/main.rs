mod config;
mod data;
mod debug;
mod gui;
mod mem_reader;
mod network;
mod notifier;
mod scanner;
mod server_logic;
mod session;

use std::sync::{Arc, Mutex};

use clap::{Parser, Subcommand};
use config::IniReader;
use eframe::egui;
use debug::DebugLoop;
use mem_reader::MemReader;
use network::{NetworkServer, StubDataProvider};
use notifier::{LoggingNotifier, UiNotifier};
use scanner::EqGameScanner;
use session::SessionRunner;

const GUI_DEFAULT_WIDTH: f32 = 660.0;
const GUI_DEFAULT_HEIGHT: f32 = 560.0;
const GUI_MIN_WIDTH: f32 = 660.0;
const GUI_MIN_HEIGHT: f32 = 560.0;

#[derive(Parser)]
#[command(
    name = "WinShowEQServer",
    about = "WinShowEQ — EverQuest map overlay server"
)]
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
        None => run_gui(ini),
        Some(Command::Console) => run_console(ini),
        Some(Command::Debug) => run_debug(ini),
        Some(Command::Scan { exe_path }) => run_scan(&exe_path, ini),
        Some(Command::Attach) => run_attach(),
        Some(Command::ServeStub) => run_serve_stub(),
    }
}

fn load_icon() -> egui::IconData {
    let bytes = include_bytes!("../../assets/WinShowEQ.png");
    let img = image::load_from_memory(bytes)
        .expect("valid PNG icon")
        .into_rgba8();
    let (width, height) = img.dimensions();
    egui::IconData { rgba: img.into_raw(), width, height }
}

fn run_gui(ini_override: Option<&str>) {
    let ini_path = ini_override
        .map(str::to_owned)
        .unwrap_or_else(|| resolve_ini_path("myseqserver.ini"));
    let config_ini_path = resolve_ini_path("config.ini");
    let patterns_ini_path = resolve_ini_path("patterns.ini");

    let start_minimized = {
        let mut ir = IniReader::new();
        ir.open_config_file(&config_ini_path);
        ir.start_minimized
    };

    let gui_state = Arc::new(Mutex::new(gui::GuiState::default()));
    let notifier = Arc::new(gui::EguiNotifier::new(Arc::clone(&gui_state)));

    let ini_path_for_gui = ini_path.clone();
    let config_ini_path_for_gui = config_ini_path.clone();
    let patterns_ini_path_for_gui = patterns_ini_path.clone();
    let mut runner = SessionRunner::new(ini_path, config_ini_path);
    runner.set_notifier(notifier as Arc<dyn UiNotifier>);
    let reload_flag = runner.reload_flag();

    std::thread::spawn(move || {
        runner.run_console_loop();
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([GUI_DEFAULT_WIDTH, GUI_DEFAULT_HEIGHT])
            .with_min_inner_size([GUI_MIN_WIDTH, GUI_MIN_HEIGHT])
            .with_visible(!start_minimized)
            .with_icon(std::sync::Arc::new(load_icon())),
        ..Default::default()
    };
    eframe::run_native(
        "WinShowEQ",
        options,
        Box::new(move |cc| {
            Ok(Box::new(gui::WinShowEQApp::new(
                cc,
                gui_state,
                ini_path_for_gui,
                config_ini_path_for_gui,
                patterns_ini_path_for_gui,
                reload_flag,
                start_minimized,
            )))
        }),
    )
    .expect("eframe failed to start");
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
    let patterns_ini_path = resolve_ini_path("patterns.ini");

    let mut ir = IniReader::new();
    let _ = ir.open_file(&ini_path);
    ir.open_config_file(&config_ini_path);
    ir.open_patterns_file(&patterns_ini_path);

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
    let patterns_ini_path = resolve_ini_path("patterns.ini");

    let mut ir = IniReader::new();
    ir.open_config_file(&config_ini_path);
    ir.open_patterns_file(&patterns_ini_path);
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
    resolve_ini_path_with(
        name,
        std::env::var_os("PROGRAMDATA"),
        std::env::current_dir().ok(),
        std::env::current_exe().ok(),
    )
}

fn resolve_ini_path_with(
    name: &str,
    program_data: Option<std::ffi::OsString>,
    current_dir: Option<std::path::PathBuf>,
    current_exe: Option<std::path::PathBuf>,
) -> String {
    if let Some(workspace_ini_path) = current_exe
        .as_deref()
        .and_then(|path| resolve_workspace_ini_path(name, path.parent()))
    {
        return workspace_ini_path.to_string_lossy().into_owned();
    }

    if let Some(workspace_ini_path) = resolve_workspace_ini_path(name, current_dir.as_deref()) {
        return workspace_ini_path.to_string_lossy().into_owned();
    }

    if let Some(program_data) = program_data {
        let winshoweq_dir = std::path::PathBuf::from(program_data).join("WinShowEQ");
        let program_data_path = winshoweq_dir.join(name);
        if winshoweq_dir.exists() {
            return program_data_path.to_string_lossy().into_owned();
        }
    }

    // GetPrivateProfileStringW requires an absolute path — relative paths resolve to
    // C:\Windows, not the current working directory.
    current_dir
        .map(|d| d.join(name).to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string())
}

fn resolve_workspace_ini_path(
    name: &str,
    start_dir: Option<&std::path::Path>,
) -> Option<std::path::PathBuf> {
    for ancestor in start_dir?.ancestors() {
        let candidate = ancestor.join("server").join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::resolve_ini_path_with;

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        let nonce = format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        path.push(nonce);
        path
    }

    #[test]
    fn resolve_ini_prefers_programdata_winshoweq_dir_when_present() {
        let base = unique_temp_dir("winshoweq-main-programdata");
        let program_data = base.join("program-data");
        let winshoweq_dir = program_data.join("WinShowEQ");
        let cwd = base.join("cwd");

        std::fs::create_dir_all(&winshoweq_dir).expect("create ProgramData WinShowEQ dir");
        std::fs::create_dir_all(&cwd).expect("create cwd");

        let resolved = resolve_ini_path_with(
            "myseqserver.ini",
            Some(program_data.into_os_string()),
            Some(cwd),
            None,
        );

        assert_eq!(
            resolved,
            winshoweq_dir
                .join("myseqserver.ini")
                .to_string_lossy()
                .into_owned()
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn resolve_ini_falls_back_to_current_dir_when_programdata_not_ready() {
        let base = unique_temp_dir("winshoweq-main-cwd");
        let program_data = base.join("program-data");
        let cwd = base.join("cwd");

        std::fs::create_dir_all(&cwd).expect("create cwd");

        let resolved = resolve_ini_path_with(
            "config.ini",
            Some(program_data.into_os_string()),
            Some(cwd.clone()),
            None,
        );

        assert_eq!(
            resolved,
            cwd.join("config.ini").to_string_lossy().into_owned()
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn resolve_ini_falls_back_to_bare_name_without_known_dirs() {
        let resolved = resolve_ini_path_with("config.ini", None, None, None);
        assert_eq!(resolved, "config.ini");
    }

    #[test]
    fn resolve_ini_prefers_workspace_server_ini_from_current_exe() {
        let base = unique_temp_dir("winshoweq-main-exe-workspace");
        let workspace = base.join("workspace");
        let server_dir = workspace.join("server");
        let target_dir = workspace.join("target").join("debug");
        let program_data = base.join("program-data");
        let winshoweq_dir = program_data.join("WinShowEQ");

        std::fs::create_dir_all(&server_dir).expect("create workspace server dir");
        std::fs::create_dir_all(&target_dir).expect("create target dir");
        std::fs::create_dir_all(&winshoweq_dir).expect("create ProgramData WinShowEQ dir");
        std::fs::write(server_dir.join("myseqserver.ini"), "[File Info]\nPatchDate=01/01/2000\n")
            .expect("write workspace ini");

        let resolved = resolve_ini_path_with(
            "myseqserver.ini",
            Some(program_data.into_os_string()),
            Some(base.join("other-cwd")),
            Some(target_dir.join("WinShowEQServer.exe")),
        );

        assert_eq!(
            resolved,
            server_dir
                .join("myseqserver.ini")
                .to_string_lossy()
                .into_owned()
        );

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn resolve_ini_prefers_workspace_server_ini_from_current_dir() {
        let base = unique_temp_dir("winshoweq-main-cwd-workspace");
        let workspace = base.join("workspace");
        let server_dir = workspace.join("server");
        let cwd = workspace.join("tools");

        std::fs::create_dir_all(&server_dir).expect("create workspace server dir");
        std::fs::create_dir_all(&cwd).expect("create cwd");
        std::fs::write(server_dir.join("config.ini"), "[Server]\nStartMinimized=0\n")
            .expect("write workspace config ini");

        let resolved = resolve_ini_path_with("config.ini", None, Some(cwd), None);

        assert_eq!(
            resolved,
            server_dir.join("config.ini").to_string_lossy().into_owned()
        );

        let _ = std::fs::remove_dir_all(base);
    }
}
