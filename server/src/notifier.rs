/// Snapshot of server status delivered to the UI layer.
/// Fields with sentinel values (-1 for counts, empty string for addresses) mean "not updated".
/// Mirrors ServerStatusSnapshot in IServerUiNotifier.h.
#[derive(Debug, Clone)]
pub struct StatusSnapshot {
    pub patch_date: String,
    pub status_text: String,
    pub zone: String,
    pub character_name: String,
    pub primary_address: String,
    /// When true, the UI should blank zone and character name controls.
    pub clear_zone_and_name: bool,
    pub port: u32,
    pub npc_count: i32, // -1 = not set
    pub pc_count: i32,
    pub corpse_count: i32,
    pub item_count: i32,
    pub spawn_list_addr: String,
    pub self_addr: String,
    pub target_addr: String,
    pub zone_name_addr: String,
    pub ground_addr: String,
    pub world_addr: String,
}

impl Default for StatusSnapshot {
    fn default() -> Self {
        Self {
            patch_date: String::new(),
            status_text: String::new(),
            zone: String::new(),
            character_name: String::new(),
            primary_address: String::new(),
            clear_zone_and_name: false,
            port: 0,
            npc_count: -1,
            pc_count: -1,
            corpse_count: -1,
            item_count: -1,
            spawn_list_addr: String::new(),
            self_addr: String::new(),
            target_addr: String::new(),
            zone_name_addr: String::new(),
            ground_addr: String::new(),
            world_addr: String::new(),
        }
    }
}

impl StatusSnapshot {
    /// Merge `other` into `self`, updating only fields that carry non-default values.
    pub fn merge_from(&mut self, other: &StatusSnapshot) {
        if !other.patch_date.is_empty() {
            self.patch_date = other.patch_date.clone();
        }
        if !other.status_text.is_empty() {
            self.status_text = other.status_text.clone();
        }
        if !other.primary_address.is_empty() {
            self.primary_address = other.primary_address.clone();
        }
        if other.port != 0 {
            self.port = other.port;
        }
        if !other.spawn_list_addr.is_empty() {
            self.spawn_list_addr = other.spawn_list_addr.clone();
        }
        if !other.self_addr.is_empty() {
            self.self_addr = other.self_addr.clone();
        }
        if !other.target_addr.is_empty() {
            self.target_addr = other.target_addr.clone();
        }
        if !other.zone_name_addr.is_empty() {
            self.zone_name_addr = other.zone_name_addr.clone();
        }
        if !other.ground_addr.is_empty() {
            self.ground_addr = other.ground_addr.clone();
        }
        if !other.world_addr.is_empty() {
            self.world_addr = other.world_addr.clone();
        }
        if other.npc_count >= 0 {
            self.npc_count = other.npc_count;
        }
        if other.pc_count >= 0 {
            self.pc_count = other.pc_count;
        }
        if other.corpse_count >= 0 {
            self.corpse_count = other.corpse_count;
        }
        if other.item_count >= 0 {
            self.item_count = other.item_count;
        }
        if other.clear_zone_and_name {
            self.clear_zone_and_name = true;
            self.zone = String::new();
            self.character_name = String::new();
        } else {
            self.clear_zone_and_name = false;
            if !other.zone.is_empty() {
                self.zone = other.zone.clone();
            }
            if !other.character_name.is_empty() {
                self.character_name = other.character_name.clone();
            }
        }
    }
}

/// Connection state change event delivered to the UI layer.
/// Mirrors ServerConnectionEvent in IServerUiNotifier.h.
#[derive(Debug, Clone, Default)]
pub struct ConnectionEvent {
    pub connected: bool,
    pub listening: bool,
    pub paused: bool,
    pub error: bool,
    pub error_message: String,
}

/// UI notification interface — Rust equivalent of IServerUiNotifier.
/// Send + Sync required so Arc<dyn UiNotifier> is Send (needed for the server background thread).
pub trait UiNotifier: Send + Sync {
    fn on_status_update(&self, snapshot: &StatusSnapshot);
    fn on_info(&self, title: &str, message: &str);
    fn on_error(&self, title: &str, message: &str);
    fn on_connection_changed(&self, event: &ConnectionEvent);
    fn on_log_event(&self, message: &str);
}

/// Headless notifier that optionally logs to stdout/stderr.
/// Mirrors LoggingServerUiNotifier in LoggingServerUiNotifier.h.
pub struct LoggingNotifier {
    pub log_to_stdout: bool,
}

impl LoggingNotifier {
    pub fn new(log_to_stdout: bool) -> Self {
        Self { log_to_stdout }
    }
}

impl UiNotifier for LoggingNotifier {
    fn on_status_update(&self, snapshot: &StatusSnapshot) {
        if !self.log_to_stdout {
            return;
        }
        if !snapshot.zone.is_empty() {
            println!("[Zone] {}", snapshot.zone);
        }
        if !snapshot.character_name.is_empty() {
            println!("[Char] {}", snapshot.character_name);
        }
        if snapshot.npc_count >= 0 || snapshot.pc_count >= 0 {
            println!(
                "[Counts] NPCs={} PCs={}",
                snapshot.npc_count, snapshot.pc_count
            );
        }
    }

    fn on_info(&self, title: &str, message: &str) {
        if self.log_to_stdout {
            println!("[INFO] {}: {}", title, message);
        }
    }

    fn on_error(&self, title: &str, message: &str) {
        if self.log_to_stdout {
            eprintln!("[ERROR] {}: {}", title, message);
        }
    }

    fn on_connection_changed(&self, event: &ConnectionEvent) {
        if !self.log_to_stdout {
            return;
        }
        if event.error {
            eprintln!("[ERROR] {}", event.error_message);
        } else if event.connected {
            println!("[STATE] Connected");
        } else if event.listening {
            println!("[STATE] Listening");
        } else if event.paused {
            println!("[STATE] Paused");
        }
    }

    fn on_log_event(&self, message: &str) {
        if self.log_to_stdout {
            println!("[LOG] {}", message);
        }
    }
}
