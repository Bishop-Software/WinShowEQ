use crate::config::ClientConfig;

pub struct OptionsDialog {
    pub open: bool,
    pub trails_enabled: bool,
    server_ip: String,
    server_port: String,
    update_delay: String,
    cfg_dir: String,
    timer_dir: String,
    log_dir: String,
    filter_dir: String,
}

impl OptionsDialog {
    pub fn new(cfg: &ClientConfig, trails_enabled: bool) -> Self {
        Self {
            open: false,
            trails_enabled,
            server_ip: cfg.server_ip.clone(),
            server_port: cfg.server_port.to_string(),
            update_delay: cfg.update_delay_ms.to_string(),
            cfg_dir: cfg.cfg_dir.clone(),
            timer_dir: cfg.timer_dir.clone(),
            log_dir: cfg.log_dir.clone(),
            filter_dir: cfg.filter_dir.clone(),
        }
    }

    pub fn sync_from(&mut self, cfg: &ClientConfig, trails_enabled: bool) {
        self.server_ip = cfg.server_ip.clone();
        self.server_port = cfg.server_port.to_string();
        self.update_delay = cfg.update_delay_ms.to_string();
        self.cfg_dir = cfg.cfg_dir.clone();
        self.timer_dir = cfg.timer_dir.clone();
        self.log_dir = cfg.log_dir.clone();
        self.filter_dir = cfg.filter_dir.clone();
        self.trails_enabled = trails_enabled;
    }

    /// Returns `Some(config)` when the user clicks OK.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<ClientConfig> {
        if !self.open {
            return None;
        }
        let mut result = None;
        egui::Window::new("Options")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                egui::Grid::new("options_grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.strong("Server IP:");
                        ui.text_edit_singleline(&mut self.server_ip);
                        ui.end_row();

                        ui.strong("Port:");
                        ui.text_edit_singleline(&mut self.server_port);
                        ui.end_row();

                        ui.strong("Update delay (ms):");
                        ui.text_edit_singleline(&mut self.update_delay);
                        ui.end_row();

                        ui.strong("Config dir:");
                        ui.text_edit_singleline(&mut self.cfg_dir);
                        ui.end_row();

                        ui.strong("Timer dir:");
                        ui.text_edit_singleline(&mut self.timer_dir);
                        ui.end_row();

                        ui.strong("Log dir:");
                        ui.text_edit_singleline(&mut self.log_dir);
                        ui.end_row();

                        ui.strong("Filter dir:");
                        ui.text_edit_singleline(&mut self.filter_dir);
                        ui.end_row();

                        ui.strong("Mob trails:");
                        ui.checkbox(&mut self.trails_enabled, "");
                        ui.end_row();
                    });

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        result = Some(ClientConfig {
                            server_ip: self.server_ip.clone(),
                            server_port: self.server_port.parse().unwrap_or(5555),
                            update_delay_ms: self.update_delay.parse().unwrap_or(250),
                            cfg_dir: self.cfg_dir.clone(),
                            timer_dir: self.timer_dir.clone(),
                            log_dir: self.log_dir.clone(),
                            filter_dir: self.filter_dir.clone(),
                        });
                        self.open = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.open = false;
                    }
                });
            });
        result
    }
}