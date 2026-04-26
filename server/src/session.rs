use std::sync::{Arc, Mutex};

use crate::data::spawn_offsets::{ItemOffsets, SpawnOffsets, WorldOffsets};
use crate::mem_reader::MemReader;
use crate::network::NetworkServer;
use crate::notifier::{ConnectionEvent, LoggingNotifier, UiNotifier};
use crate::server_logic::{MemDataProvider, ServerLogic};

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
    logic:      ServerLogic,
    notifier:   Arc<dyn UiNotifier>,
    state:      SessionState,
    last_error: String,
}

impl SessionRunner {
    pub fn new(ini_path: String, config_ini_path: String) -> Self {
        Self {
            logic: ServerLogic::new(ini_path, config_ini_path),
            notifier: Arc::new(LoggingNotifier::new(true)),
            state: SessionState::Idle,
            last_error: String::new(),
        }
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
            SessionState::Listening => ConnectionEvent { listening: true, ..Default::default() },
            SessionState::Connected => ConnectionEvent { connected: true, ..Default::default() },
            SessionState::Paused    => ConnectionEvent { paused: true,    ..Default::default() },
            SessionState::Error     => ConnectionEvent {
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
                eprintln!("[ERROR] {e}");
                self.last_error = e;
                self.transition_to(SessionState::Error);
                return;
            }
        };

        println!("[INFO] Patch date: {}", ir.patch_date);
        println!("[INFO] Port: {}", config.port);

        let spawn_off = SpawnOffsets::from_ini(&ir);
        let item_off  = ItemOffsets::from_ini(&ir);
        let world_off = WorldOffsets::from_ini(&ir);

        if spawn_off.buf_size <= 30 {
            eprintln!("[WARN] SpawnInfo Offsets are all zero — check myseqserver.ini");
        }

        let mem = Arc::new(Mutex::new(MemReader::new()));

        // Attempt initial attach. DataProvider will retry automatically if EQ isn't running yet.
        if let Some(pid) = MemReader::find_process("eqgame.exe") {
            let mut r = mem.lock().unwrap();
            match r.open(pid) {
                Ok(()) => println!("[STATE] Attached to eqgame.exe PID={pid}  Base=0x{:X}", r.base_address()),
                Err(e) => eprintln!("[WARN] Could not attach to eqgame.exe: {e}"),
            }
        } else {
            println!("[WARN] eqgame.exe not running — will attach when found");
        }

        let provider = Arc::new(MemDataProvider::new(
            Arc::clone(&mem),
            config.offsets,
            spawn_off,
            item_off,
            world_off,
        ));

        let mut server = NetworkServer::new(config.port as u16);
        server.set_notifier(Arc::clone(&self.notifier) as Arc<dyn UiNotifier>);

        self.transition_to(SessionState::Listening);

        // Blocking accept loop — returns only if the listener socket errors.
        server.serve(provider);

        self.transition_to(SessionState::Idle);
    }
}