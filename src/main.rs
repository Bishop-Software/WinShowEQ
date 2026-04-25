mod config;
mod data;
mod notifier;
mod scanner;

use config::{IniReader, PrimaryOffsets};
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
            _ => {}
        }
        i += 1;
    }

    println!("WinShowEQ starting...");
}

fn run_scan(exe_path: &str) {
    if exe_path.is_empty() {
        eprintln!("Usage: winshowed --scan <path\\to\\eqgame.exe>");
        return;
    }

    let mut ir = IniReader::new();
    ir.open_config_file("config.ini");

    // Load myseqserver.ini for port display / write-back; ignore if missing.
    let _ = ir.open_file("myseqserver.ini");

    let scanner = EqGameScanner::new(exe_path);
    let result  = scanner.scan_executable(&ir, &PrimaryOffsets::default(), false);
    print!("{}", result.output);
}