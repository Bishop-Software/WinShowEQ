use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32};

use crate::session::SessionState;
use super::GuiState;

pub struct WinShowEQApp {
    state: Arc<Mutex<GuiState>>,
    ini_path: String,
}

impl WinShowEQApp {
    pub fn new(_cc: &eframe::CreationContext<'_>, state: Arc<Mutex<GuiState>>, ini_path: String) -> Self {
        Self { state, ini_path }
    }
}

fn status_color(state: SessionState) -> Color32 {
    match state {
        SessionState::Connected => Color32::from_rgb(80, 200, 80),
        SessionState::Listening | SessionState::Starting => Color32::YELLOW,
        SessionState::Error => Color32::RED,
        SessionState::Paused => Color32::from_rgb(100, 180, 255),
        _ => Color32::GRAY,
    }
}

fn list_local_ips() -> Vec<String> {
    use std::net::ToSocketAddrs;
    let hostname = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "localhost".into());
    match (hostname.as_str(), 0u16).to_socket_addrs() {
        Ok(addrs) => {
            let mut ips: Vec<String> = addrs
                .filter(|a| !a.ip().is_loopback())
                .map(|a| a.ip().to_string())
                .collect();
            ips.dedup();
            ips
        }
        Err(_) => vec![],
    }
}

// Two-column label/value grid with a fixed minimum label column width.
fn status_grid<'a>(id: &'static str, label_min_width: f32) -> egui::Grid {
    egui::Grid::new(id)
        .num_columns(2)
        .min_col_width(label_min_width)
        .spacing([8.0, 4.0])
}

impl eframe::App for WinShowEQApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));

        let (snapshot, session_state) = {
            let Ok(s) = self.state.lock() else { return };
            (s.snapshot.clone(), s.session_state)
        };

        let status_text = if snapshot.status_text.is_empty() {
            match session_state {
                SessionState::Idle | SessionState::Stopping => "Idle",
                SessionState::Starting => "Starting",
                SessionState::Listening => "Listening",
                SessionState::Connected => "Connected",
                SessionState::Error => "Error",
                SessionState::Paused => "Paused",
            }
            .to_owned()
        } else {
            snapshot.status_text.clone()
        };

        let zone = if snapshot.clear_zone_and_name { "" } else { snapshot.zone.as_str() };
        let character = if snapshot.clear_zone_and_name { "" } else { snapshot.character_name.as_str() };
        let port_str = if snapshot.port > 0 { snapshot.port.to_string() } else { String::new() };

        // ── Status / connection info — two side-by-side grids ─────────────────
        ui.columns(2, |cols| {
            status_grid("status_left", 80.0).show(&mut cols[0], |ui| {
                ui.label("Status:");
                ui.colored_label(status_color(session_state), &status_text);
                ui.end_row();
                ui.label("Port:");
                ui.label(&port_str);
                ui.end_row();
                ui.label("Zone:");
                ui.label(zone);
                ui.end_row();
            });

            status_grid("status_right", 80.0).show(&mut cols[1], |ui| {
                ui.label("Primary:");
                ui.label(&snapshot.primary_address);
                ui.end_row();
                ui.label("Patch Date:");
                ui.label(&snapshot.patch_date);
                ui.end_row();
                ui.label("Character:");
                ui.label(character);
                ui.end_row();
            });
        });

        ui.separator();

        // ── Spawn counts ──────────────────────────────────────────────────────
        ui.columns(4, |cols| {
            count_col(&mut cols[0], "NPCs", snapshot.npc_count);
            count_col(&mut cols[1], "PCs", snapshot.pc_count);
            count_col(&mut cols[2], "Corpses", snapshot.corpse_count);
            count_col(&mut cols[3], "Items", snapshot.item_count);
        });

        ui.separator();

        // ── Memory addresses — two side-by-side grids ─────────────────────────
        ui.columns(2, |cols| {
            status_grid("addr_left", 100.0).show(&mut cols[0], |ui| {
                ui.label("SpawnHeader:");
                ui.monospace(&snapshot.spawn_list_addr);
                ui.end_row();
                ui.label("Target:");
                ui.monospace(&snapshot.target_addr);
                ui.end_row();
                ui.label("Items Addr:");
                ui.monospace(&snapshot.ground_addr);
                ui.end_row();
            });

            status_grid("addr_right", 80.0).show(&mut cols[1], |ui| {
                ui.label("CharInfo:");
                ui.monospace(&snapshot.self_addr);
                ui.end_row();
                ui.label("Zone Addr:");
                ui.monospace(&snapshot.zone_name_addr);
                ui.end_row();
                ui.label("World:");
                ui.monospace(&snapshot.world_addr);
                ui.end_row();
            });
        });

        ui.separator();

        // ── Buttons ───────────────────────────────────────────────────────────
        ui.horizontal(|ui| {
            if ui.button("Edit INI").clicked() {
                std::process::Command::new("notepad.exe")
                    .arg(&self.ini_path)
                    .spawn()
                    .ok();
            }

            if ui.button("Reload Offsets").clicked() {
                if let Ok(mut s) = self.state.lock() {
                    s.push_log("Reload Offsets: not yet wired to server thread");
                }
            }

            if ui.button("List IPs").clicked() {
                let ips = list_local_ips();
                if let Ok(mut s) = self.state.lock() {
                    if ips.is_empty() {
                        s.push_log("List IPs: no non-loopback addresses found");
                    } else {
                        for ip in &ips {
                            s.push_log(&format!("Local IP: {ip}"));
                        }
                    }
                }
            }

            // Offset Finder — implemented in M7-6.
            ui.add_enabled(false, egui::Button::new("Offset Finder"));
        });

        ui.separator();

        // ── Log pane (M7-4) ───────────────────────────────────────────────────
        ui.label("(log — M7-4)");
    }
}

fn count_col(ui: &mut egui::Ui, label: &str, count: i32) {
    ui.vertical_centered(|ui| {
        ui.label(label);
        ui.label(if count < 0 { "—".into() } else { count.to_string() });
    });
}