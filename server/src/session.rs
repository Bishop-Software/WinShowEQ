use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::config::{IniReader, ServerConfigModel};
use crate::data::spawn_offsets::{ItemOffsets, SpawnOffsets, WorldOffsets};
use crate::mem_reader::MemReader;
use crate::network::NetworkServer;
use crate::notifier::{ConnectionEvent, LoggingNotifier, StatusSnapshot, UiNotifier};
use crate::server_logic::{LiveOffsets, MemDataProvider, ServerLogic};

/// Server session state machine.
/// Mirrors SessionState enum in ServerSessionRunner.h.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Starting,
    Listening,
    Connected,
    Stopping,
    Error,
    Paused,
}

/// Selects the server's runtime mode.
/// Mirrors SessionMode enum in ServerSessionRunner.h.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    Interactive,
    Console,
    WindowsService,
    Debug,
}

/// Manages the server session lifecycle.
/// Mirrors ServerSessionRunner in C++ — Start/Stop/Pause/Resume + per-mode loops.
pub struct SessionRunner {
    logic: ServerLogic,
    ini_path: String,
    config_ini_path: String,
    reload_flag: Arc<AtomicBool>,
    notifier: Arc<dyn UiNotifier>,
    state: SessionState,
    last_error: String,
}

impl SessionRunner {
    pub fn new(ini_path: String, config_ini_path: String) -> Self {
        Self {
            logic: ServerLogic::new(ini_path.clone(), config_ini_path.clone()),
            ini_path,
            config_ini_path,
            reload_flag: Arc::new(AtomicBool::new(false)),
            notifier: Arc::new(LoggingNotifier::new(true)),
            state: SessionState::Idle,
            last_error: String::new(),
        }
    }

    pub fn set_notifier(&mut self, notifier: Arc<dyn UiNotifier>) {
        self.notifier = notifier;
    }

    pub fn reload_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.reload_flag)
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn last_error(&self) -> &str {
        &self.last_error
    }

    fn transition_to(&mut self, new_state: SessionState) {
        self.state = new_state;
        let event = match new_state {
            SessionState::Listening => ConnectionEvent {
                listening: true,
                ..Default::default()
            },
            SessionState::Connected => ConnectionEvent {
                connected: true,
                ..Default::default()
            },
            SessionState::Paused => ConnectionEvent {
                paused: true,
                ..Default::default()
            },
            SessionState::Error => ConnectionEvent {
                error: true,
                error_message: self.last_error.clone(),
                ..Default::default()
            },
            _ => ConnectionEvent::default(),
        };
        self.notifier.on_connection_changed(&event);
    }

    /// Blocking console mode loop — load config, attach to EQ, serve clients indefinitely.
    /// Mirrors RunConsoleLoop() in ServerSessionRunner.cpp.
    pub fn run_console_loop(&mut self) {
        self.transition_to(SessionState::Starting);

        MemReader::enable_debug_privileges();

        let (ir, config) = match self.logic.load_config() {
            Ok(r) => r,
            Err(e) => {
                self.notifier.on_error("SessionRunner", &e);
                self.last_error = e;
                self.transition_to(SessionState::Error);
                return;
            }
        };

        self.notifier
            .on_info("SessionRunner", &format!("Patch date: {}", ir.patch_date));
        self.notifier
            .on_info("SessionRunner", &format!("Port: {}", config.port));

        self.notifier.on_status_update(&build_initial_snapshot(&ir, &config));

        let spawn_off = SpawnOffsets::from_ini(&ir);
        let item_off = ItemOffsets::from_ini(&ir);
        let world_off = WorldOffsets::from_ini(&ir);
        let live = LiveOffsets { primary: config.offsets.clone(), spawn_off: spawn_off.clone(), item_off, world_off };

        if spawn_off.buf_size <= 30 {
            self.notifier
                .on_log_event("WARN: SpawnInfo Offsets are all zero — check myseqserver.ini");
        }

        let mem = Arc::new(Mutex::new(MemReader::new()));

        // Attempt initial attach. DataProvider will retry automatically if EQ isn't running yet.
        if let Some(pid) = MemReader::find_process("eqgame.exe") {
            let mut r = mem.lock().unwrap();
            match r.open(pid) {
                Ok(()) => self.notifier.on_log_event(&format!(
                    "Attached to eqgame.exe PID={pid} Base=0x{:X}",
                    r.base_address()
                )),
                Err(e) => self
                    .notifier
                    .on_log_event(&format!("WARN: Could not attach to eqgame.exe: {e}")),
            }
        } else {
            self.notifier
                .on_log_event("WARN: eqgame.exe not running — will attach when found");
        }

        let provider = Arc::new(MemDataProvider::new(
            Arc::clone(&mem),
            live,
            self.ini_path.clone(),
            self.config_ini_path.clone(),
            Arc::clone(&self.reload_flag),
            Some(Arc::clone(&self.notifier)),
        ));

        let mut server = NetworkServer::new(config.port as u16);
        server.set_notifier(Arc::clone(&self.notifier) as Arc<dyn UiNotifier>);

        self.transition_to(SessionState::Listening);

        // Blocking accept loop — returns only if the listener socket errors.
        server.serve(provider);

        self.transition_to(SessionState::Idle);
    }
}

fn build_initial_snapshot(ir: &IniReader, config: &ServerConfigModel) -> StatusSnapshot {
    let o = &config.offsets;
    StatusSnapshot {
        patch_date: ir.patch_date.clone(),
        port: config.port,
        primary_address: primary_ip(),
        spawn_list_addr: fmt_addr(o.spawn_list),
        self_addr: fmt_addr(o.self_addr),
        target_addr: fmt_addr(o.target),
        zone_name_addr: fmt_addr(o.zone_name),
        ground_addr: fmt_addr(o.ground),
        world_addr: fmt_addr(o.world),
        npc_count: -1,
        pc_count: -1,
        corpse_count: -1,
        item_count: -1,
        ..StatusSnapshot::default()
    }
}

fn fmt_addr(addr: u64) -> String {
    if addr != 0 { format!("0x{addr:X}") } else { String::new() }
}

fn primary_ip() -> String {
    use std::net::{IpAddr, ToSocketAddrs};
    let hostname = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "localhost".into());
    let addrs: Vec<IpAddr> = (hostname.as_str(), 0u16)
        .to_socket_addrs()
        .map(|it| it.map(|a| a.ip()).filter(|ip| !ip.is_loopback()).collect())
        .unwrap_or_default();
    // Prefer a routable IPv4 address; fall back to any non-loopback.
    addrs.iter()
        .find(|ip| ip.is_ipv4())
        .or_else(|| addrs.iter().find(|ip| !matches!(ip, IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80)))
        .map(|ip| ip.to_string())
        .unwrap_or_default()
}
