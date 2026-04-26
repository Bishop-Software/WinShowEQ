mod config;
mod data;
mod mem_reader;
mod notifier;
mod scanner;

use config::IniReader;
use mem_reader::MemReader;
use scanner::EqGameScanner;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--scan" => {
                i += 1;
                let exe_path = args.get(i).map(String::as_str).unwrap_or("");
                run_scan(exe_path);
                return;
            }
            "--attach" => {
                run_attach();
                return;
            }
            _ => {}
        }
        i += 1;
    }

    println!("WinShowEQ starting...");
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

fn resolve_ini_path(name: &str) -> String {
    // GetPrivateProfileStringW requires an absolute path — relative paths resolve to
    // C:\Windows, not the current working directory.
    std::env::current_dir()
        .map(|d| d.join(name).to_string_lossy().into_owned())
        .unwrap_or_else(|_| name.to_string())
}

fn run_scan(exe_path: &str) {
    if exe_path.is_empty() {
        eprintln!("Usage: WinShowEQ --scan <path\\to\\eqgame.exe>");
        return;
    }

    let mut ir = IniReader::new();
    ir.open_config_file(&resolve_ini_path("config.ini"));

    // Load myseqserver.ini for port display / write-back; ignore if missing.
    let _ = ir.open_file(&resolve_ini_path("myseqserver.ini"));

    let current_offsets = ir.read_server_config_model()
        .map(|m| m.offsets)
        .unwrap_or_default();

    let scanner = EqGameScanner::new(exe_path);
    let result  = scanner.scan_executable(&ir, &current_offsets, false);
    print!("{}", result.output);
}