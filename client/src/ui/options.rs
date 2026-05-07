use crate::config::ClientConfig;
use crate::logger::LogLevel;

const ALERT_MODES: &[&str] = &["none", "beep", "speech", "sound"];
const AUDIO_EXTENSIONS: &[&str] = &["wav", "flac", "ogg", "mp3"];

/// Returns the parent directory of a sound file path, or "." if unset/invalid.
fn sound_dir(path: &str) -> std::path::PathBuf {
    if path.is_empty() {
        return std::path::PathBuf::from(".");
    }
    std::path::Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

pub struct OptionsDialog {
    pub open: bool,
    pub trails_enabled: bool,
    /// Full config snapshot — fields not shown in the UI are preserved via struct-update.
    base_config: ClientConfig,
    // Connection
    server_ip: String,
    server_port: String,
    update_delay: String,
    auto_connect: bool,
    // Directories
    cfg_dir: String,
    timer_dir: String,
    annotations_dir: String,
    log_dir: String,
    filter_dir: String,
    map_dir: String,
    // Alerts
    danger_mode: String,
    caution_mode: String,
    hunt_mode: String,
    alert_mode: String,
    danger_sound: String,
    caution_sound: String,
    hunt_sound: String,
    alert_sound: String,
    // Discord
    discord_webhook: String,
    discord_on_danger: bool,
    discord_on_hunt: bool,
    // EQ
    eq_path: String,
    // Logging
    log_enabled: bool,
    log_level: String,
}

impl OptionsDialog {
    pub fn new(cfg: &ClientConfig, trails_enabled: bool) -> Self {
        Self {
            open: false,
            trails_enabled,
            base_config: cfg.clone(),
            server_ip: cfg.server_ip.clone(),
            server_port: cfg.server_port.to_string(),
            update_delay: cfg.update_delay_ms.to_string(),
            auto_connect: cfg.auto_connect,
            cfg_dir: cfg.cfg_dir.clone(),
            timer_dir: cfg.timer_dir.clone(),
            annotations_dir: cfg.annotations_dir.clone(),
            log_dir: cfg.log_dir.clone(),
            filter_dir: cfg.filter_dir.clone(),
            map_dir: cfg.map_dir.clone(),
            danger_mode: cfg.alert_danger_mode.clone(),
            caution_mode: cfg.alert_caution_mode.clone(),
            hunt_mode: cfg.alert_hunt_mode.clone(),
            alert_mode: cfg.alert_rare_mode.clone(),
            danger_sound: cfg.alert_danger_sound.clone(),
            caution_sound: cfg.alert_caution_sound.clone(),
            hunt_sound: cfg.alert_hunt_sound.clone(),
            alert_sound: cfg.alert_rare_sound.clone(),
            discord_webhook: cfg.discord_webhook.clone(),
            discord_on_danger: cfg.discord_on_danger,
            discord_on_hunt: cfg.discord_on_hunt,
            eq_path: cfg.eq_path.clone(),
            log_enabled: cfg.log_enabled,
            log_level: cfg.log_level.clone(),
        }
    }

    pub fn sync_from(&mut self, cfg: &ClientConfig, trails_enabled: bool) {
        self.base_config = cfg.clone();
        self.server_ip = cfg.server_ip.clone();
        self.server_port = cfg.server_port.to_string();
        self.update_delay = cfg.update_delay_ms.to_string();
        self.auto_connect = cfg.auto_connect;
        self.cfg_dir = cfg.cfg_dir.clone();
        self.timer_dir = cfg.timer_dir.clone();
        self.annotations_dir = cfg.annotations_dir.clone();
        self.log_dir = cfg.log_dir.clone();
        self.filter_dir = cfg.filter_dir.clone();
        self.map_dir = cfg.map_dir.clone();
        self.danger_mode = cfg.alert_danger_mode.clone();
        self.caution_mode = cfg.alert_caution_mode.clone();
        self.hunt_mode = cfg.alert_hunt_mode.clone();
        self.alert_mode = cfg.alert_rare_mode.clone();
        self.danger_sound = cfg.alert_danger_sound.clone();
        self.caution_sound = cfg.alert_caution_sound.clone();
        self.hunt_sound = cfg.alert_hunt_sound.clone();
        self.alert_sound = cfg.alert_rare_sound.clone();
        self.discord_webhook = cfg.discord_webhook.clone();
        self.discord_on_danger = cfg.discord_on_danger;
        self.discord_on_hunt = cfg.discord_on_hunt;
        self.eq_path = cfg.eq_path.clone();
        self.log_enabled = cfg.log_enabled;
        self.log_level = cfg.log_level.clone();
        self.trails_enabled = trails_enabled;
    }

    /// Returns `Some(config)` when the user clicks Save.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<ClientConfig> {
        if !self.open {
            return None;
        }
        let mut result = None;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("options_dialog"),
            egui::ViewportBuilder::default()
                .with_title("Options")
                .with_inner_size([550.0, 750.0])
                .with_resizable(false),
            |ctx, _class| {
                #[allow(deprecated)]
                egui::Panel::bottom("options_buttons").show(ctx, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            result = Some(ClientConfig {
                                server_ip: self.server_ip.clone(),
                                server_port: self.server_port.parse().unwrap_or(5555),
                                update_delay_ms: self.update_delay.parse().unwrap_or(250),
                                cfg_dir: self.cfg_dir.clone(),
                                timer_dir: self.timer_dir.clone(),
                                annotations_dir: self.annotations_dir.clone(),
                                log_dir: self.log_dir.clone(),
                                filter_dir: self.filter_dir.clone(),
                                map_dir: self.map_dir.clone(),
                                alert_danger_mode: self.danger_mode.clone(),
                                alert_danger_sound: self.danger_sound.clone(),
                                alert_caution_mode: self.caution_mode.clone(),
                                alert_caution_sound: self.caution_sound.clone(),
                                alert_hunt_mode: self.hunt_mode.clone(),
                                alert_hunt_sound: self.hunt_sound.clone(),
                                alert_rare_mode: self.alert_mode.clone(),
                                alert_rare_sound: self.alert_sound.clone(),
                                discord_webhook: self.discord_webhook.clone(),
                                discord_on_danger: self.discord_on_danger,
                                discord_on_hunt: self.discord_on_hunt,
                                eq_path: self.eq_path.clone(),
                                log_enabled: self.log_enabled,
                                log_level: self.log_level.clone(),
                                auto_connect: self.auto_connect,
                            });
                            self.open = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.open = false;
                        }
                    });
                    ui.add_space(4.0);
                });

                #[allow(deprecated)]
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::Grid::new("options_grid")
                        .num_columns(2)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            // ── Connection ──────────────────────────────────
                            ui.strong("Server IP:");
                            ui.text_edit_singleline(&mut self.server_ip);
                            ui.end_row();

                            ui.strong("Port:");
                            ui.text_edit_singleline(&mut self.server_port);
                            ui.end_row();

                            ui.strong("Update delay (ms):");
                            ui.text_edit_singleline(&mut self.update_delay);
                            ui.end_row();

                            ui.strong("Auto-connect on startup:");
                            ui.checkbox(&mut self.auto_connect, "");
                            ui.end_row();

                            // ── Directories ──────────────────────────────────
                            ui.strong("Config path:");
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.cfg_dir);
                                if ui.button("Browse…").clicked()
                                    && let Some(path) = rfd::FileDialog::new().set_directory(&self.cfg_dir).pick_folder() {
                                        self.cfg_dir = path.display().to_string();
                                    }
                            });
                            ui.end_row();

                            ui.strong("Timer path:");
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.timer_dir);
                                if ui.button("Browse…").clicked()
                                    && let Some(path) = rfd::FileDialog::new().set_directory(&self.timer_dir).pick_folder() {
                                        self.timer_dir = path.display().to_string();
                                    }
                            });
                            ui.end_row();

                            ui.strong("Annotations path:");
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.annotations_dir);
                                if ui.button("Browse…").clicked()
                                    && let Some(path) = rfd::FileDialog::new().set_directory(&self.annotations_dir).pick_folder() {
                                        self.annotations_dir = path.display().to_string();
                                    }
                            });
                            ui.end_row();

                            ui.strong("Log path:");
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.log_dir);
                                if ui.button("Browse…").clicked()
                                    && let Some(path) = rfd::FileDialog::new().set_directory(&self.log_dir).pick_folder() {
                                        self.log_dir = path.display().to_string();
                                    }
                            });
                            ui.end_row();

                            ui.strong("Filter path:");
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.filter_dir);
                                if ui.button("Browse…").clicked()
                                    && let Some(path) = rfd::FileDialog::new().set_directory(&self.filter_dir).pick_folder() {
                                        self.filter_dir = path.display().to_string();
                                    }
                            });
                            ui.end_row();

                            ui.strong("Map path:");
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.map_dir);
                                if ui.button("Browse…").clicked()
                                    && let Some(path) = rfd::FileDialog::new().set_directory(&self.map_dir).pick_folder() {
                                        self.map_dir = path.display().to_string();
                                    }
                            });
                            ui.end_row();

                            // ── Rendering ────────────────────────────────────
                            ui.strong("Mob trails:");
                            ui.checkbox(&mut self.trails_enabled, "");
                            ui.end_row();

                            // ── Alerts ───────────────────────────────────────
                            ui.separator();
                            ui.end_row();
                            ui.strong("Alerts");
                            ui.label("");
                            ui.end_row();

                            ui.strong("Danger mode:");
                            egui::ComboBox::from_id_salt("danger_mode")
                                .selected_text(self.danger_mode.as_str())
                                .show_ui(ui, |ui| {
                                    for mode in ALERT_MODES {
                                        ui.selectable_value(&mut self.danger_mode, mode.to_string(), *mode);
                                    }
                                });
                            ui.end_row();

                            ui.strong("Danger sound file:");
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.danger_sound);
                                if ui.button("Browse…").clicked() {
                                    let start = sound_dir(&self.danger_sound);
                                    let mut dialog = rfd::FileDialog::new().set_title("Select sound file").set_directory(start);
                                    for ext in AUDIO_EXTENSIONS { dialog = dialog.add_filter(*ext, &[*ext]); }
                                    if let Some(path) = dialog.pick_file() {
                                        self.danger_sound = path.display().to_string();
                                    }
                                }
                            });
                            ui.end_row();

                            ui.strong("Caution mode:");
                            egui::ComboBox::from_id_salt("caution_mode")
                                .selected_text(self.caution_mode.as_str())
                                .show_ui(ui, |ui| {
                                    for mode in ALERT_MODES {
                                        ui.selectable_value(&mut self.caution_mode, mode.to_string(), *mode);
                                    }
                                });
                            ui.end_row();

                            ui.strong("Caution sound file:");
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.caution_sound);
                                if ui.button("Browse…").clicked() {
                                    let start = sound_dir(&self.caution_sound);
                                    let mut dialog = rfd::FileDialog::new().set_title("Select sound file").set_directory(start);
                                    for ext in AUDIO_EXTENSIONS { dialog = dialog.add_filter(*ext, &[*ext]); }
                                    if let Some(path) = dialog.pick_file() {
                                        self.caution_sound = path.display().to_string();
                                    }
                                }
                            });
                            ui.end_row();

                            ui.strong("Hunt mode:");
                            egui::ComboBox::from_id_salt("hunt_mode")
                                .selected_text(self.hunt_mode.as_str())
                                .show_ui(ui, |ui| {
                                    for mode in ALERT_MODES {
                                        ui.selectable_value(&mut self.hunt_mode, mode.to_string(), *mode);
                                    }
                                });
                            ui.end_row();

                            ui.strong("Hunt sound file:");
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.hunt_sound);
                                if ui.button("Browse…").clicked() {
                                    let start = sound_dir(&self.hunt_sound);
                                    let mut dialog = rfd::FileDialog::new().set_title("Select sound file").set_directory(start);
                                    for ext in AUDIO_EXTENSIONS { dialog = dialog.add_filter(*ext, &[*ext]); }
                                    if let Some(path) = dialog.pick_file() {
                                        self.hunt_sound = path.display().to_string();
                                    }
                                }
                            });
                            ui.end_row();

                            ui.strong("Rare mode:");
                            egui::ComboBox::from_id_salt("alert_mode")
                                .selected_text(self.alert_mode.as_str())
                                .show_ui(ui, |ui| {
                                    for mode in ALERT_MODES {
                                        ui.selectable_value(&mut self.alert_mode, mode.to_string(), *mode);
                                    }
                                });
                            ui.end_row();

                            ui.strong("Rare sound file:");
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.alert_sound);
                                if ui.button("Browse…").clicked() {
                                    let start = sound_dir(&self.alert_sound);
                                    let mut dialog = rfd::FileDialog::new().set_title("Select sound file").set_directory(start);
                                    for ext in AUDIO_EXTENSIONS { dialog = dialog.add_filter(*ext, &[*ext]); }
                                    if let Some(path) = dialog.pick_file() {
                                        self.alert_sound = path.display().to_string();
                                    }
                                }
                            });
                            ui.end_row();

                            // ── Discord ──────────────────────────────────────
                            ui.separator();
                            ui.end_row();
                            ui.strong("Discord webhook URL:");
                            ui.text_edit_singleline(&mut self.discord_webhook);
                            ui.end_row();

                            ui.strong("Discord on danger:");
                            ui.checkbox(&mut self.discord_on_danger, "");
                            ui.end_row();

                            ui.strong("Discord on hunt:");
                            ui.checkbox(&mut self.discord_on_hunt, "");
                            ui.end_row();

                            // ── Logging ──────────────────────────────────────
                            ui.separator();
                            ui.end_row();
                            ui.strong("Logging");
                            ui.label("");
                            ui.end_row();

                            ui.strong("Enable logging:");
                            ui.checkbox(&mut self.log_enabled, "");
                            ui.end_row();

                            ui.strong("Log level:");
                            egui::ComboBox::from_id_salt("log_level")
                                .selected_text(self.log_level.as_str())
                                .show_ui(ui, |ui| {
                                    for level in LogLevel::all() {
                                        ui.selectable_value(
                                            &mut self.log_level,
                                            level.as_str().to_owned(),
                                            level.as_str(),
                                        );
                                    }
                                });
                            ui.end_row();

                            // ── EverQuest ─────────────────────────────────────
                            ui.separator();
                            ui.end_row();
                            ui.strong("EQ install path:");
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.eq_path);
                                if ui.button("Browse…").clicked()
                                    && let Some(path) = rfd::FileDialog::new().set_directory(&self.eq_path).pick_folder() {
                                        self.eq_path = path.display().to_string();
                                    }
                            });
                            ui.end_row();
                        });
                });

                if ctx.input(|i| i.viewport().close_requested()) {
                    self.open = false;
                }
            },
        );
        result
    }
}