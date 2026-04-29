use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, RichText};
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

use crate::config::IniReader;
use crate::session::SessionState;
use super::GuiState;

const WINDOW_PADDING: i8 = 10;
const SIDE_BY_SIDE_MIN_WIDTH: f32 = 560.0;

pub struct WinShowEQApp {
    state: Arc<Mutex<GuiState>>,
    ini_path: String,
    config_ini_path: String,
    _tray: TrayIcon,
    menu_open: MenuItem,
    menu_start_min: CheckMenuItem,
    menu_exit: MenuItem,
    window_visible: bool,
}

impl WinShowEQApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        state: Arc<Mutex<GuiState>>,
        ini_path: String,
        config_ini_path: String,
        start_minimized: bool,
    ) -> Self {
        let menu_open = MenuItem::new("Open", true, None);
        let menu_start_min = CheckMenuItem::new("Start Minimized", true, start_minimized, None);
        let menu_exit = MenuItem::new("Exit", true, None);

        let menu = Menu::new();
        menu.append_items(&[
            &menu_open,
            &PredefinedMenuItem::separator(),
            &menu_start_min,
            &PredefinedMenuItem::separator(),
            &menu_exit,
        ])
        .expect("tray menu build");

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("WinShowEQ")
            .with_icon(make_placeholder_icon())
            .build()
            .expect("tray icon creation");

        Self {
            state,
            ini_path,
            config_ini_path,
            _tray: tray,
            menu_open,
            menu_start_min,
            menu_exit,
            window_visible: !start_minimized,
        }
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.window_visible = true;
    }

    fn toggle_start_minimized(&mut self) {
        let mut ir = IniReader::new();
        ir.open_config_file(&self.config_ini_path);
        ir.toggle_start_minimized();
        self.menu_start_min.set_checked(ir.start_minimized);
    }
}

fn make_placeholder_icon() -> Icon {
    let size = 16u32;
    let rgba: Vec<u8> = (0..size * size).flat_map(|_| [50u8, 160, 50, 255]).collect();
    Icon::from_rgba(rgba, size, size).expect("valid tray icon")
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

impl eframe::App for WinShowEQApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Keep polling even when the window is hidden.
        ctx.request_repaint_after(std::time::Duration::from_millis(100));

        // X button closes the app. Exit via tray menu also closes.
        if ctx.input(|i| i.viewport().close_requested()) {
            // Let eframe close naturally — no CancelClose intercept.
        }

        // Tray icon clicks — double-click restores if window is hidden.
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } = event {
                self.show_window(ctx);
            }
        }

        // Tray context menu selections.
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == *self.menu_open.id() {
                self.show_window(ctx);
            } else if event.id == *self.menu_start_min.id() {
                self.toggle_start_minimized();
            } else if event.id == *self.menu_exit.id() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(WINDOW_PADDING))
            .show(ui, |ui| {
                let (snapshot, session_state, log) = {
                    let Ok(s) = self.state.lock() else { return };
                    (s.snapshot.clone(), s.session_state, s.log.clone())
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

                // ── Status info block ─────────────────────────────────────────────────
                ui.horizontal(|ui| {
                    egui::Grid::new("status_port")
                        .num_columns(4)
                        .min_col_width(70.0)
                        .spacing([8.0, 0.0])
                        .show(ui, |ui| {
                            ui.label("Status:");
                            ui.colored_label(status_color(session_state), &status_text);
                            ui.label("Port:");
                            ui.label(&port_str);
                            ui.end_row();
                        });
                });

                egui::Grid::new("status_details")
                    .num_columns(2)
                    .min_col_width(80.0)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Patch:");
                        ui.label(&snapshot.patch_date);
                        ui.end_row();
                        ui.label("Zone:");
                        ui.label(zone);
                        ui.end_row();
                        ui.label("Character:");
                        ui.label(character);
                        ui.end_row();
                    });

                // IP Address + List IPs inline (mirrors C++ layout)
                ui.horizontal(|ui| {
                    ui.label("IP Address:");
                    ui.label(&snapshot.primary_address);
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
                });

                ui.separator();

                // ── Primary Offsets + Spawns (responsive layout) ───────────────────────
                let show_side_by_side = ui.available_width() >= SIDE_BY_SIDE_MIN_WIDTH;
                if show_side_by_side {
                    ui.columns(2, |cols| {
                        render_primary_offsets_group(&mut cols[0], &snapshot);
                        render_spawns_group(&mut cols[1], &snapshot);
                    });
                } else {
                    render_primary_offsets_group(ui, &snapshot);

                    ui.add_space(6.0);

                    render_spawns_group(ui, &snapshot);
                }

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
                    // Offset Finder — implemented in M7-6.
                    ui.add_enabled(false, egui::Button::new("Offset Finder"));
                });

                ui.separator();

                // ── Log pane ──────────────────────────────────────────────────────────
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &log {
                            ui.label(RichText::new(line).monospace());
                        }
                    });
            });
    }
}

fn addr_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(label);
    ui.monospace(value);
    ui.end_row();
}

fn render_primary_offsets_group(ui: &mut egui::Ui, snapshot: &crate::notifier::StatusSnapshot) {
    ui.group(|ui| {
        ui.label(RichText::new("Primary Offsets").strong());
        ui.add_space(2.0);
        egui::Grid::new("offsets_grid")
            .num_columns(2)
            .min_col_width(90.0)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                addr_row(ui, "ZoneAddr:", &snapshot.zone_name_addr);
                addr_row(ui, "TargetAddr:", &snapshot.target_addr);
                addr_row(ui, "SpawnHeader:", &snapshot.spawn_list_addr);
                addr_row(ui, "CharInfo:", &snapshot.self_addr);
                addr_row(ui, "ItemsAddr:", &snapshot.ground_addr);
                addr_row(ui, "WorldAddr:", &snapshot.world_addr);
            });
    });
}

fn render_spawns_group(ui: &mut egui::Ui, snapshot: &crate::notifier::StatusSnapshot) {
    ui.group(|ui| {
        ui.label(RichText::new("Spawns").strong());
        ui.add_space(2.0);
        egui::Grid::new("spawns_grid")
            .num_columns(2)
            .min_col_width(60.0)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                count_row(ui, "NPC:", snapshot.npc_count);
                count_row(ui, "PC:", snapshot.pc_count);
                count_row(ui, "Corpse:", snapshot.corpse_count);
                count_row(ui, "Ground:", snapshot.item_count);
            });
    });
}

fn count_row(ui: &mut egui::Ui, label: &str, count: i32) {
    ui.label(label);
    ui.label(if count < 0 { "—".into() } else { count.to_string() });
    ui.end_row();
}