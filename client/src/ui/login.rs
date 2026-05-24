use std::net::SocketAddr;

pub struct LoginDialog {
    pub open: bool,
    pub server_str: String,
}

impl LoginDialog {
    pub fn new(current_addr: Option<SocketAddr>) -> Self {
        Self {
            open: false,
            server_str: current_addr
                .map(|a| a.to_string())
                .unwrap_or_else(|| "127.0.0.1:5555".to_owned()),
        }
    }

    /// Returns `Some(addr)` when the user clicks Connect with a valid address.
    pub fn show(&mut self, ctx: &egui::Context) -> Option<SocketAddr> {
        if !self.open {
            return None;
        }
        let mut result = None;
        egui::Window::new("Connect to Server")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                egui::Grid::new("login_grid")
                    .num_columns(2)
                    .spacing([6.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Server address:");
                        ui.text_edit_singleline(&mut self.server_str);
                        ui.end_row();
                    });

                let parsed = self.server_str.parse::<SocketAddr>();
                if parsed.is_err() {
                    ui.colored_label(egui::Color32::RED, "Invalid address (e.g. 127.0.0.1:5555)");
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let ok = ui.add_enabled(parsed.is_ok(), egui::Button::new("Connect"));
                    if ok.clicked() {
                        result = parsed.ok();
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
