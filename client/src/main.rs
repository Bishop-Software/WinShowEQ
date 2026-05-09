#![windows_subsystem = "windows"]

mod alerts;
mod config;
mod data;
mod game_data;
mod filters;
mod logger;
mod map_canvas;
mod map_reader;
mod net;
mod protocol;
mod ui;

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;

use ui::main_window::MainApp;

#[derive(Parser)]
#[command(name = "WinShowEQClient", about = "WinShowEQ Rust client")]
struct Cli {
    /// Connect to a WinShowEQ server (e.g. 127.0.0.1:5555)
    #[arg(long, value_name = "IP:PORT")]
    connect: Option<SocketAddr>,
}

fn load_icon() -> egui::IconData {
    let bytes = include_bytes!("../../assets/WinShowEQ.png");
    let img = image::load_from_memory(bytes)
        .expect("valid PNG icon")
        .into_rgba8();
    let (width, height) = img.dimensions();
    egui::IconData { rgba: img.into_raw(), width, height }
}

fn main() -> eframe::Result {
    let cli = Cli::parse();
    let config_path = resolve_ini_path("client.ini");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("WinShowEQ Client")
            .with_inner_size([900.0, 700.0])
            .with_icon(std::sync::Arc::new(load_icon())),
        persist_window: true,
        ..Default::default()
    };

    eframe::run_native(
        "WinShowEQ Client",
        options,
        Box::new(move |cc| Ok(Box::new(MainApp::new(cc, cli.connect, config_path)))),
    )
}

fn resolve_ini_path(name: &str) -> PathBuf {
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
    current_dir: Option<PathBuf>,
    current_exe: Option<PathBuf>,
) -> PathBuf {
    // 1. Walk ancestors of current_exe looking for workspace/client/<name>
    if let Some(p) = current_exe
        .as_deref()
        .and_then(|p| resolve_workspace_ini_path(name, p.parent()))
    {
        return p;
    }

    // 2. Walk ancestors of current_dir looking for workspace/client/<name>
    if let Some(p) = resolve_workspace_ini_path(name, current_dir.as_deref()) {
        return p;
    }

    // 3. %ProgramData%\WinShowEQ\<name> when that directory exists (installer layout)
    if let Some(pd) = program_data {
        let dir = PathBuf::from(pd).join("WinShowEQ");
        if dir.exists() {
            return dir.join(name);
        }
    }

    // 4. Fallback: absolute path in current dir (bare name may resolve to C:\Windows)
    current_dir
        .map(|d| d.join(name))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn resolve_workspace_ini_path(name: &str, start_dir: Option<&std::path::Path>) -> Option<PathBuf> {
    for ancestor in start_dir?.ancestors() {
        let candidate = ancestor.join("client").join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::resolve_ini_path_with;
    use std::path::PathBuf;

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        path
    }

    #[test]
    fn resolve_ini_prefers_workspace_client_ini_from_current_exe() {
        let base = unique_temp_dir("winshoweq-client-exe-workspace");
        let workspace = base.join("workspace");
        let client_dir = workspace.join("client");
        let target_dir = workspace.join("target").join("debug");
        let fake_exe = target_dir.join("WinShowEQClient.exe");

        std::fs::create_dir_all(&client_dir).unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(client_dir.join("client.ini"), "[WinShowEQ]").unwrap();

        let resolved = resolve_ini_path_with("client.ini", None, None, Some(fake_exe));
        assert_eq!(resolved, client_dir.join("client.ini"));
    }

    #[test]
    fn resolve_ini_prefers_workspace_client_ini_from_current_dir() {
        let base = unique_temp_dir("winshoweq-client-cwd-workspace");
        let workspace = base.join("workspace");
        let client_dir = workspace.join("client");
        let cwd = workspace.join("tools");

        std::fs::create_dir_all(&client_dir).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(client_dir.join("client.ini"), "[WinShowEQ]").unwrap();

        let resolved = resolve_ini_path_with("client.ini", None, Some(cwd), None);
        assert_eq!(resolved, client_dir.join("client.ini"));
    }

    #[test]
    fn resolve_ini_prefers_programdata_winshoweq_dir_when_present() {
        let base = unique_temp_dir("winshoweq-client-programdata");
        let program_data = base.join("program-data");
        let winshoweq_dir = program_data.join("WinShowEQ");
        let cwd = base.join("cwd");

        std::fs::create_dir_all(&winshoweq_dir).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();

        let resolved =
            resolve_ini_path_with("client.ini", Some(program_data.into()), Some(cwd), None);
        assert_eq!(resolved, winshoweq_dir.join("client.ini"));
    }

    #[test]
    fn resolve_ini_falls_back_to_current_dir_when_programdata_not_ready() {
        let base = unique_temp_dir("winshoweq-client-fallback");
        let program_data = base.join("program-data"); // does NOT contain WinShowEQ subdir
        let cwd = base.join("cwd");
        std::fs::create_dir_all(&cwd).unwrap();

        let resolved =
            resolve_ini_path_with("client.ini", Some(program_data.into()), Some(cwd.clone()), None);
        assert_eq!(resolved, cwd.join("client.ini"));
    }
}