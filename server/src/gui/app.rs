use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, RichText};
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

use crate::config::IniReader;
use crate::scanner::EqGameScanner;
use crate::session::SessionState;
use super::GuiState;

const WINDOW_PADDING: i8 = 10;
const SIDE_BY_SIDE_MIN_WIDTH: f32 = 560.0;

// ── Offset Finder ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum ScanKind {
    Primary,
    Secondary,
    Both,
}

struct OffsetFinderState {
    open: bool,
    exe_path: String,
    scanning: bool,
    last_kind: Option<ScanKind>,
    pending: Arc<Mutex<Option<String>>>,
    display_text: String,
}

impl Default for OffsetFinderState {
    fn default() -> Self {
        Self {
            open: false,
            exe_path: String::new(),
            scanning: false,
            last_kind: None,
            pending: Arc::new(Mutex::new(None)),
            display_text: String::new(),
        }
    }
}

impl OffsetFinderState {
    fn with_exe_path(exe_path: String) -> Self {
        Self { exe_path, ..Self::default() }
    }
}

// ── Main app ─────────────────────────────────────────────────────────────────

pub struct WinShowEQApp {
    state: Arc<Mutex<GuiState>>,
    ini_path: String,
    config_ini_path: String,
    patterns_ini_path: String,
    reload_flag: Arc<AtomicBool>,
    _tray: TrayIcon,
    menu_open: MenuItem,
    menu_start_min: CheckMenuItem,
    menu_exit: MenuItem,
    window_visible: bool,
    offset_finder: OffsetFinderState,
}

impl WinShowEQApp {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        state: Arc<Mutex<GuiState>>,
        ini_path: String,
        config_ini_path: String,
        patterns_ini_path: String,
        reload_flag: Arc<AtomicBool>,
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

        let mut ir = IniReader::new();
        ir.open_config_file(&config_ini_path);
        let saved_exe_path = ir.read_eq_game_path();

        Self {
            state,
            ini_path,
            config_ini_path,
            patterns_ini_path,
            reload_flag,
            _tray: tray,
            menu_open,
            menu_start_min,
            menu_exit,
            window_visible: !start_minimized,
            offset_finder: OffsetFinderState::with_exe_path(saved_exe_path),
        }
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.window_visible = true;
    }

    fn save_exe_path(&self) {
        let mut ir = IniReader::new();
        ir.open_config_file(&self.config_ini_path);
        ir.save_eq_game_path(&self.offset_finder.exe_path);
    }

    fn toggle_start_minimized(&mut self) {
        let mut ir = IniReader::new();
        ir.open_config_file(&self.config_ini_path);
        ir.toggle_start_minimized();
        self.menu_start_min.set_checked(ir.start_minimized);
    }

    fn start_scan(&mut self, kind: ScanKind, write_out: bool) {
        self.offset_finder.scanning = true;
        self.offset_finder.last_kind = Some(kind);

        let exe_path = self.offset_finder.exe_path.clone();
        let ini_path = self.ini_path.clone();
        let config_ini_path = self.config_ini_path.clone();
        let patterns_ini_path = self.patterns_ini_path.clone();
        let pending = Arc::clone(&self.offset_finder.pending);

        std::thread::spawn(move || {
            let mut ir = IniReader::new();
            ir.open_config_file(&config_ini_path);
            ir.open_patterns_file(&patterns_ini_path);
            let _ = ir.open_file(&ini_path);

            let current_offsets = ir
                .read_server_config_model()
                .map(|m| m.offsets)
                .unwrap_or_default();

            let scanner = EqGameScanner::new(&exe_path);

            let mut output = match kind {
                ScanKind::Primary => {
                    scanner.scan_executable(&ir, &current_offsets, write_out).output
                }
                ScanKind::Secondary => {
                    scanner.scan_secondary(&ir, current_offsets.self_addr)
                }
                ScanKind::Both => {
                    let primary = scanner.scan_executable(&ir, &current_offsets, write_out);
                    let secondary = scanner.scan_secondary(&ir, current_offsets.self_addr);
                    format!("{}\n{}", primary.output, secondary)
                }
            };

            if write_out {
                output.push_str("\n[Written to INI]");
            }

            if let Ok(mut guard) = pending.lock() {
                *guard = Some(output);
            }
        });
    }

    fn render_offset_finder(&mut self, ctx: &egui::Context) {
        if !self.offset_finder.open {
            return;
        }

        // Poll for a completed background scan each frame.
        if self.offset_finder.scanning {
            if let Ok(mut guard) = self.offset_finder.pending.lock() {
                if let Some(result) = guard.take() {
                    self.offset_finder.display_text = result;
                    self.offset_finder.scanning = false;
                }
            }
        }

        let mut open = true;
        let mut start_scan: Option<(ScanKind, bool)> = None;
        let mut do_browse = false;
        let scanning = self.offset_finder.scanning;
        let has_path = !self.offset_finder.exe_path.trim().is_empty();
        let last_kind = self.offset_finder.last_kind;
        let display_empty = self.offset_finder.display_text.is_empty();
        // Take exe_path out so we can move it into the closure without a borrow conflict.
        let mut exe_path = std::mem::take(&mut self.offset_finder.exe_path);
        let mut display_text = self.offset_finder.display_text.clone();

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("offset_finder"),
            egui::ViewportBuilder::default()
                .with_title("Offset Finder")
                .with_inner_size(egui::vec2(700.0, 600.0))
                .with_min_inner_size(egui::vec2(700.0, 400.0))
                .with_resizable(true),
            |ctx, _class| {
                if ctx.input(|i| i.viewport().close_requested()) {
                    open = false;
                }

                #[allow(deprecated)]
                egui::CentralPanel::default().show(ctx, |ui| {
                    let can_write = !scanning
                        && !display_empty
                        && matches!(last_kind, Some(ScanKind::Primary) | Some(ScanKind::Both));

                    // Footer — declared first, claims space from the bottom.
                    // Panel::bottom draws its own separator; no explicit one needed.
                    egui::Panel::bottom("offset_finder_footer")
                        .resizable(false)
                        .show_inside(ui, |ui| {
                            egui::Frame::NONE
                                .inner_margin(egui::Margin::same(WINDOW_PADDING))
                                .show(ui, |ui| {
                                    ui.add_enabled_ui(can_write, |ui| {
                                        if ui.button("Write to INI").clicked() {
                                            start_scan = Some((ScanKind::Primary, true));
                                        }
                                    });
                                });
                        });

                    // Header — claimed from the top after footer.
                    // Panel::top draws its own separator at the bottom edge.
                    egui::Panel::top("offset_finder_header")
                        .resizable(false)
                        .show_inside(ui, |ui| {
                            egui::Frame::NONE
                                .inner_margin(egui::Margin::same(WINDOW_PADDING))
                                .show(ui, |ui| {
                                    // Browse allocated before TextEdit so it is
                                    // never pushed off the right edge.
                                    ui.horizontal(|ui| {
                                        ui.label("EQ Executable:");
                                        if ui.button("Browse…").clicked() {
                                            do_browse = true;
                                        }
                                        ui.add(
                                            egui::TextEdit::singleline(&mut exe_path)
                                                .desired_width(f32::INFINITY),
                                        );
                                    });

                                    ui.add_space(4.0);

                                    ui.horizontal(|ui| {
                                        ui.add_enabled_ui(!scanning && has_path, |ui| {
                                            if ui.button("Scan Primary").clicked() {
                                                start_scan = Some((ScanKind::Primary, false));
                                            }
                                            if ui.button("Scan Secondary").clicked() {
                                                start_scan = Some((ScanKind::Secondary, false));
                                            }
                                            if ui.button("Scan Both").clicked() {
                                                start_scan = Some((ScanKind::Both, false));
                                            }
                                        });
                                        if scanning {
                                            ui.add(egui::Spinner::new());
                                        }
                                    });
                                });
                        });

                    // Central area — fill whatever space the two panels left.
                    egui::Frame::NONE
                        .inner_margin(egui::Margin {
                            left: WINDOW_PADDING,
                            right: WINDOW_PADDING,
                            top: 4,
                            bottom: 4,
                        })
                        .show(ui, |ui| {
                            let h = ui.available_height();
                            let w = ui.available_width();
                            // ScrollArea provides the hard height bound when content overflows.
                            // add_sized forces the TextEdit to fill h when content is short.
                            egui::ScrollArea::vertical()
                                .max_height(h)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.add_sized(
                                        [w, h],
                                        egui::TextEdit::multiline(&mut display_text)
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(f32::INFINITY)
                                            .interactive(false),
                                    );
                                });
                        });
                });
            },
        );

        self.offset_finder.open = open;
        self.offset_finder.exe_path = exe_path;

        // Browse must run outside the egui closure so it can block for the dialog.
        if do_browse {
            if let Some(path) = browse_for_exe() {
                self.offset_finder.exe_path = path;
                self.save_exe_path();
            }
        }

        if let Some((kind, write_out)) = start_scan {
            self.save_exe_path();
            self.start_scan(kind, write_out);
        }
    }
}

// ── Native file dialog ────────────────────────────────────────────────────────

/// Opens a native Windows file-open dialog and returns the chosen path, or None
/// if the user cancelled or an error occurred.
fn browse_for_exe() -> Option<String> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{
        Common::COMDLG_FILTERSPEC, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
    };
    use windows::core::w;

    unsafe {
        // Initialize COM for this call; ignore S_FALSE (already initialized).
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let path = (|| -> Option<String> {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL).ok()?;

            let filters = [
                COMDLG_FILTERSPEC { pszName: w!("EverQuest Game"), pszSpec: w!("eqgame.exe") },
                COMDLG_FILTERSPEC { pszName: w!("Executable Files (*.exe)"), pszSpec: w!("*.exe") },
            ];
            let _ = dialog.SetFileTypes(&filters);
            let _ = dialog.SetFileTypeIndex(1); // default to eqgame.exe filter

            dialog.Show(None).ok()?;
            let item = dialog.GetResult().ok()?;
            let pwstr = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
            // Convert before freeing; always free regardless of conversion result.
            let s = pwstr.to_string().ok();
            windows::Win32::System::Com::CoTaskMemFree(Some(
                pwstr.0 as *const core::ffi::c_void,
            ));
            s
        })();

        CoUninitialize();
        path
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

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
    use std::net::{IpAddr, ToSocketAddrs};
    let hostname = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "localhost".into());
    match (hostname.as_str(), 0u16).to_socket_addrs() {
        Ok(addrs) => {
            let mut ips: Vec<String> = addrs
                .map(|a| a.ip())
                .filter(|ip| {
                    !ip.is_loopback()
                        && !matches!(ip, IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80)
                })
                .map(|ip| ip.to_string())
                .collect();
            ips.dedup();
            ips
        }
        Err(_) => vec![],
    }
}

// ── eframe::App ───────────────────────────────────────────────────────────────

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
                let mut list_ips_clicked = false;
                egui::Grid::new("status_info")
                    .num_columns(2)
                    .min_col_width(80.0)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Status:");
                        ui.colored_label(status_color(session_state), &status_text);
                        ui.end_row();
                        ui.label("Port:");
                        ui.label(&port_str);
                        ui.end_row();
                        ui.label("Patch:");
                        ui.label(&snapshot.patch_date);
                        ui.end_row();
                        ui.label("Zone:");
                        ui.label(zone);
                        ui.end_row();
                        ui.label("Character:");
                        ui.label(character);
                        ui.end_row();
                        ui.label("IP Address:");
                        ui.horizontal(|ui| {
                            ui.label(&snapshot.primary_address);
                            if ui.button("List IPs").clicked() {
                                list_ips_clicked = true;
                            }
                        });
                        ui.end_row();
                    });

                if list_ips_clicked {
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
                    let font_id = egui::TextStyle::Button.resolve(ui.style());
                    let pad = ui.spacing().button_padding.x;
                    let gap = ui.spacing().item_spacing.x;
                    let painter = ui.painter();
                    let btn_w = |text: &str| -> f32 {
                        painter
                            .layout_no_wrap(text.to_owned(), font_id.clone(), egui::Color32::WHITE)
                            .size()
                            .x
                            + 2.0 * pad
                    };
                    let total = btn_w("Edit INI")
                        + btn_w("Reload Offsets")
                        + btn_w("Offset Finder")
                        + 2.0 * gap;
                    ui.add_space(((ui.available_width() - total) * 0.5).max(0.0));
                    if ui.button("Edit INI").clicked() {
                        std::process::Command::new("notepad.exe")
                            .arg(&self.ini_path)
                            .spawn()
                            .ok();
                    }
                    if ui.button("Reload Offsets").clicked() {
                        self.reload_flag.store(true, Ordering::Relaxed);
                        if let Ok(mut s) = self.state.lock() {
                            s.push_log("Reload Offsets: requested — will apply on next tick");
                        }
                    }
                    if ui.button("Offset Finder").clicked() {
                        self.offset_finder.open = true;
                    }
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

                // ── Offset Finder (separate OS window via immediate viewport) ──────────
                let ctx = ui.ctx().clone();
                self.render_offset_finder(&ctx);
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