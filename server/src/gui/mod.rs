mod app;

pub use app::WinShowEQApp;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::notifier::{ConnectionEvent, StatusSnapshot, UiNotifier};
use crate::session::SessionState;

const LOG_CAP: usize = 200;

/// Shared state written by the server thread and read each egui frame.
pub struct GuiState {
    pub snapshot: StatusSnapshot,
    pub session_state: SessionState,
    /// Timestamped log lines, capped at LOG_CAP entries.
    pub log: VecDeque<String>,
}

impl Default for GuiState {
    fn default() -> Self {
        Self {
            snapshot: StatusSnapshot::default(),
            session_state: SessionState::Idle,
            log: VecDeque::new(),
        }
    }
}

impl GuiState {
    pub fn push_log(&mut self, message: &str) {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let ts = format!(
            "{:02}:{:02}:{:02}",
            (secs / 3600) % 24,
            (secs / 60) % 60,
            secs % 60
        );
        self.log.push_back(format!("[{ts}] {message}"));
        while self.log.len() > LOG_CAP {
            self.log.pop_front();
        }
    }
}

/// UiNotifier implementation that writes into a shared GuiState behind a Mutex.
pub struct EguiNotifier {
    state: Arc<Mutex<GuiState>>,
}

impl EguiNotifier {
    pub fn new(state: Arc<Mutex<GuiState>>) -> Self {
        Self { state }
    }

    fn append_log(&self, message: &str) {
        if let Ok(mut s) = self.state.lock() {
            s.push_log(message);
        }
    }
}

impl UiNotifier for EguiNotifier {
    fn on_status_update(&self, snapshot: &StatusSnapshot) {
        if let Ok(mut s) = self.state.lock() {
            s.snapshot.merge_from(snapshot);
        }
    }

    fn on_info(&self, _title: &str, message: &str) {
        self.append_log(message);
    }

    fn on_error(&self, _title: &str, message: &str) {
        self.append_log(&format!("[ERROR] {message}"));
    }

    fn on_connection_changed(&self, event: &ConnectionEvent) {
        if let Ok(mut s) = self.state.lock() {
            s.session_state = if event.error {
                SessionState::Error
            } else if event.connected {
                SessionState::Connected
            } else if event.listening {
                SessionState::Listening
            } else if event.paused {
                SessionState::Paused
            } else {
                SessionState::Idle
            };
        }
        if event.error && !event.error_message.is_empty() {
            self.append_log(&format!("[ERROR] {}", event.error_message));
        }
    }

    fn on_log_event(&self, message: &str) {
        self.append_log(message);
    }
}