use crate::config::ClientConfig;
use crate::logger::LogLevel;

pub struct OptionsDialog {
    pub open: bool,
    pub trails_enabled: bool,
    /// Full config snapshot — fields not shown in the UI are preserved via struct-update.
    base_config: ClientConfig,
    // Connection
    server_ip: String,
    server_port: String,
    update_delay: String,
    // Directories
    cfg_dir: String,
    timer_dir: String,
    log_dir: String,
    filter_dir: String,
    // Alerts
    danger_mode: String,
    caution_mode: String,
    hunt_mode: String,
    alert_mode: String,
    danger_sound: String,
    hunt_sound: String,
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
            cfg_dir: cfg.cfg_dir.clone(),
            timer_dir: cfg.timer_dir.clone(),
            log_dir: cfg.log_dir.clone(),
            filter_dir: cfg.filter_dir.clone(),
            danger_mode: cfg.alert_danger_mode.clone(),
            caution_mode: cfg.alert_caution_mode.clone(),
            hunt_mode: cfg.alert_hunt_mode.clone(),
            alert_mode: cfg.alert_alert_mode.clone(),
            danger_sound: cfg.alert_danger_sound.clone(),
            hunt_sound: cfg.alert_hunt_sound.clone(),
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
        self.cfg_dir = cfg.cfg_dir.clone();
        self.timer_dir = cfg.timer_dir.clone();
        self.log_dir = cfg.log_dir.clone();
        self.filter_dir = cfg.filter_dir.clone();
        self.danger_mode = cfg.alert_danger_mode.clone();
        self.caution_mode = cfg.alert_caution_mode.clone();
        self.hunt_mode = cfg.alert_hunt_mode.clone();
        self.alert_mode = cfg.alert_alert_mode.clone();
        self.danger_sound = cfg.alert_danger_sound.clone();
        self.hunt_sound = cfg.alert_hunt_sound.clone();
        self.discord_webhook = cfg.discord_webhook.clone();
        self.discord_on_danger = cfg.discord_on_danger;
        self.discord_on_hunt = cfg.discord_on_hunt;
        self.eq_path = cfg.eq_path.clone();
        self.log_enabled = cfg.log_enabled;
        self.log_level = cfg.log_level.clone();
        self.trails_enabled = trails_enabled;
    }

    /// Returns `Some(config)` when the user clicks OK.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<ClientConfig> {
        if !self.open {
            return None;
        }
        let mut result = None;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("options_dialog"),
            egui::ViewportBuilder::default()
                .with_title("Options")
                .with_inner_size([510.0, 700.0])
                .with_resizable(false),
            |ctx, _class| {
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
                                log_dir: self.log_dir.clone(),
                                filter_dir: self.filter_dir.clone(),
                                alert_danger_mode: self.danger_mode.clone(),
                                alert_danger_sound: self.danger_sound.clone(),
                                alert_caution_mode: self.caution_mode.clone(),
                                alert_hunt_mode: self.hunt_mode.clone(),
                                alert_hunt_sound: self.hunt_sound.clone(),
                                alert_alert_mode: self.alert_mode.clone(),
                                discord_webhook: self.discord_webhook.clone(),
                                discord_on_danger: self.discord_on_danger,
                                discord_on_hunt: self.discord_on_hunt,
                                eq_path: self.eq_path.clone(),
                                log_enabled: self.log_enabled,
                                log_level: self.log_level.clone(),
                                ..self.base_config.clone()
                            });
                            self.open = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.open = false;
                        }
                    });
                    ui.add_space(4.0);
                });

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

                            // ── Directories ──────────────────────────────────
                            ui.strong("Config path:");
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.cfg_dir);
                                if ui.button("Browse…").clicked() {
                                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                        self.cfg_dir = path.display().to_string();
                                    }
                                }
                            });
                            ui.end_row();

                            ui.strong("Timer path:");
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.timer_dir);
                                if ui.button("Browse…").clicked() {
                                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                        self.timer_dir = path.display().to_string();
                                    }
                                }
                            });
                            ui.end_row();

                            ui.strong("Log path:");
                            ui.horizontal(|ui| {
                                ui.text_edit_singleline(&mut self.log_dir);
                                if ui.button("Browse…").clicked() {
                                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                        self.log_dir = path.display().to_string();
                                    }
                                }
                            });
                            ui.end_row();

                            ui.strong("Filter path:");
                            ui.text_edit_singleline(&mut self.filter_dir);
                            ui.end_row();

                            // ── Rendering ────────────────────────────────────
                            ui.strong("Mob trails:");
                            ui.checkbox(&mut self.trails_enabled, "");
                            ui.end_row();

                            // ── Alerts ───────────────────────────────────────
                            ui.separator();
                            ui.end_row();
                            ui.strong("Alerts");
                            ui.label("(none | beep | speech | sound)");
                            ui.end_row();

                            ui.strong("Danger mode:");
                            ui.text_edit_singleline(&mut self.danger_mode);
                            ui.end_row();

                            ui.strong("Danger sound file:");
                            ui.text_edit_singleline(&mut self.danger_sound);
                            ui.end_row();

                            ui.strong("Caution mode:");
                            ui.text_edit_singleline(&mut self.caution_mode);
                            ui.end_row();

                            ui.strong("Hunt mode:");
                            ui.text_edit_singleline(&mut self.hunt_mode);
                            ui.end_row();

                            ui.strong("Hunt sound file:");
                            ui.text_edit_singleline(&mut self.hunt_sound);
                            ui.end_row();

                            ui.strong("Alert mode:");
                            ui.text_edit_singleline(&mut self.alert_mode);
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
                                if ui.button("Browse…").clicked() {
                                    if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                        self.eq_path = path.display().to_string();
                                    }
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