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
use crate::wizard::{WizardCommand, WizardPhase, WizardShared, start_wizard, write_wizard_results};
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

#[derive(Clone, Copy, PartialEq)]
enum OffsetFinderTab {
    Scanner,
    Wizard,
}

struct OffsetFinderState {
    open: bool,
    exe_path: String,
    scanning: bool,
    last_kind: Option<ScanKind>,
    pending: Arc<Mutex<Option<String>>>,
    display_text: String,
    // Wizard tab
    active_tab: OffsetFinderTab,
    wizard_running: bool,
    wizard_shared: Arc<Mutex<WizardShared>>,
    wizard_write_result: String,
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
            active_tab: OffsetFinderTab::Scanner,
            wizard_running: false,
            wizard_shared: Arc::new(Mutex::new(WizardShared::default())),
            wizard_write_result: String::new(),
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
    first_frame: bool,
    prev_log_len: usize,
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
            .with_icon(load_app_icon())
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
            first_frame: true,
            prev_log_len: 0,
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
                    scanner.scan_secondary(&ir, current_offsets.self_addr, write_out)
                }
                ScanKind::Both => {
                    let primary = scanner.scan_executable(&ir, &current_offsets, write_out);
                    let secondary = scanner.scan_secondary(&ir, current_offsets.self_addr, write_out);
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

    fn start_wizard_thread(&mut self) {
        // Reset shared state for a fresh run.
        self.offset_finder.wizard_shared = Arc::new(Mutex::new(WizardShared::default()));
        self.offset_finder.wizard_write_result.clear();
        self.offset_finder.wizard_running = true;

        start_wizard(
            self.ini_path.clone(),
            self.config_ini_path.clone(),
            self.patterns_ini_path.clone(),
            self.offset_finder.exe_path.clone(),
            Arc::clone(&self.offset_finder.wizard_shared),
        );
    }

    fn render_offset_finder(&mut self, ctx: &egui::Context) {
        if !self.offset_finder.open {
            return;
        }

        // Poll for a completed background scan each frame.
        if self.offset_finder.scanning
            && let Ok(mut guard) = self.offset_finder.pending.lock()
                && let Some(result) = guard.take() {
                    self.offset_finder.display_text = result;
                    self.offset_finder.scanning = false;
                }

        let mut open = true;
        let mut start_scan: Option<(ScanKind, bool)> = None;
        let mut do_browse = false;
        let mut tab_switch: Option<OffsetFinderTab> = None;
        let scanning = self.offset_finder.scanning;
        let has_path = !self.offset_finder.exe_path.trim().is_empty();
        let last_kind = self.offset_finder.last_kind;
        let display_empty = self.offset_finder.display_text.is_empty();
        let active_tab = self.offset_finder.active_tab;
        // Take exe_path out so we can move it into the closure without a borrow conflict.
        let mut exe_path = std::mem::take(&mut self.offset_finder.exe_path);
        let mut display_text = self.offset_finder.display_text.clone();

        // Extract wizard state into locals before the closure (can't call &mut self methods inside).
        let (wizard_phase, wizard_log, wizard_results) = {
            match self.offset_finder.wizard_shared.lock() {
                Ok(s) => (s.phase.clone(), s.log.clone(), s.results.clone()),
                Err(_) => (WizardPhase::Idle, vec![], Default::default()),
            }
        };
        if self.offset_finder.wizard_running
            && matches!(wizard_phase, WizardPhase::Complete | WizardPhase::Failed)
        {
            self.offset_finder.wizard_running = false;
        }
        let wizard_running = self.offset_finder.wizard_running;
        let wizard_write_result = self.offset_finder.wizard_write_result.clone();

        // Command flags set inside the closure, applied after.
        let mut do_start_wizard = false;
        let mut wizard_cmd: Option<WizardCommand> = None;
        let mut do_write_ini = false;

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

                    // Header — exe path + tab bar + scanner controls
                    egui::Panel::top("offset_finder_header")
                        .resizable(false)
                        .show_inside(ui, |ui| {
                            egui::Frame::NONE
                                .inner_margin(egui::Margin::same(WINDOW_PADDING))
                                .show(ui, |ui| {
                                    // Exe path row
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

                                    // Tab bar
                                    ui.horizontal(|ui| {
                                        let scanner_sel = active_tab == OffsetFinderTab::Scanner;
                                        if ui.selectable_label(scanner_sel, "Scanner").clicked() {
                                            tab_switch = Some(OffsetFinderTab::Scanner);
                                        }
                                        let wizard_sel = active_tab == OffsetFinderTab::Wizard;
                                        if ui.selectable_label(wizard_sel, "Wizard").clicked() {
                                            tab_switch = Some(OffsetFinderTab::Wizard);
                                        }
                                    });

                                    // Scanner controls (only shown on Scanner tab)
                                    if active_tab == OffsetFinderTab::Scanner {
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
                                    }
                                });
                        });

                    // Central area
                    egui::Frame::NONE
                        .inner_margin(egui::Margin {
                            left: WINDOW_PADDING,
                            right: WINDOW_PADDING,
                            top: 4,
                            bottom: 4,
                        })
                        .show(ui, |ui| {
                            if active_tab == OffsetFinderTab::Scanner {
                                let h = ui.available_height();
                                let w = ui.available_width();
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
                            } else {
                                // ── Wizard tab ────────────────────────────────────────────────
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.add_enabled_ui(!wizard_running && has_path, |ui| {
                                        if ui.button("Start Wizard").clicked() {
                                            do_start_wizard = true;
                                        }
                                    });
                                    ui.add_enabled_ui(wizard_running, |ui| {
                                        if ui.button("Cancel").clicked() {
                                            wizard_cmd = Some(WizardCommand::Cancel);
                                        }
                                    });
                                    if wizard_running {
                                        ui.add(egui::Spinner::new());
                                    }
                                    ui.label(egui::RichText::new(wizard_phase.instruction()).italics());
                                });

                                ui.separator();

                                // Phase action buttons (for prompted steps).
                                if wizard_running {
                                    let show_action = matches!(
                                        wizard_phase,
                                        WizardPhase::WaitInvis | WizardPhase::WaitPet | WizardPhase::WaitItem
                                    );
                                    if show_action {
                                        ui.horizontal(|ui| {
                                            let action_label = match wizard_phase {
                                                WizardPhase::WaitInvis => "I cast invis",
                                                WizardPhase::WaitPet   => "I have a pet",
                                                WizardPhase::WaitItem  => "Item dropped",
                                                _                      => "Done",
                                            };
                                            if ui.button(action_label).clicked() {
                                                wizard_cmd = Some(WizardCommand::ActionDone);
                                            }
                                            if ui.button("Skip").clicked() {
                                                wizard_cmd = Some(WizardCommand::SkipStep);
                                            }
                                        });
                                        ui.add_space(4.0);
                                    }
                                }

                                // Results summary.
                                if matches!(wizard_phase, WizardPhase::Complete) || wizard_results.name.is_some() {
                                    ui.collapsing("Discovered offsets", |ui| {
                                        egui::Grid::new("wizard_results")
                                            .num_columns(2)
                                            .spacing([12.0, 2.0])
                                            .show(ui, |ui| {
                                                let mut row = |label: &str, val: Option<usize>| {
                                                    ui.label(label);
                                                    match val {
                                                        Some(v) => { ui.monospace(format!("0x{v:x}")); }
                                                        None    => { ui.label(RichText::new("—").weak()); }
                                                    }
                                                    ui.end_row();
                                                };
                                                row("Name",        wizard_results.name);
                                                row("Lastname",    wizard_results.last_name);
                                                row("Next",        wizard_results.next);
                                                row("Prev",        wizard_results.prev);
                                                row("X",           wizard_results.x);
                                                row("Y",           wizard_results.y);
                                                row("Z",           wizard_results.z);
                                                row("Heading",     wizard_results.heading);
                                                row("Speed",       wizard_results.speed);
                                                row("Hide",        wizard_results.hidden);
                                                row("Owner",       wizard_results.owner);
                                                row("Item.Name",   wizard_results.item_name);
                                                row("Item.X",      wizard_results.item_x);
                                                row("Item.Y",      wizard_results.item_y);
                                                row("Item.Z",      wizard_results.item_z);
                                                row("Item.Prev",   wizard_results.item_prev);
                                                row("Item.Next",   wizard_results.item_next);
                                                row("Item.Id",     wizard_results.item_id);
                                                row("Item.DropId", wizard_results.item_drop_id);
                                            });
                                    });

                                    if matches!(wizard_phase, WizardPhase::Complete) {
                                        ui.horizontal(|ui| {
                                            if ui.button("Write to INI").clicked() {
                                                do_write_ini = true;
                                            }
                                        });
                                        if !wizard_write_result.is_empty() {
                                            ui.monospace(&wizard_write_result);
                                        }
                                    }

                                    ui.separator();
                                }

                                // Log.
                                ui.label(RichText::new("Log").strong());
                                let log_h = ui.available_height() - 4.0;
                                egui::ScrollArea::vertical()
                                    .max_height(log_h)
                                    .auto_shrink([false, false])
                                    .stick_to_bottom(true)
                                    .show(ui, |ui| {
                                        for line in &wizard_log {
                                            ui.label(RichText::new(line).monospace().size(11.0));
                                        }
                                    });
                            }
                        });
                });
            },
        );

        self.offset_finder.open = open;
        self.offset_finder.exe_path = exe_path;
        if let Some(tab) = tab_switch {
            self.offset_finder.active_tab = tab;
        }

        // Browse must run outside the egui closure so it can block for the dialog.
        if do_browse
            && let Some(path) = browse_for_exe() {
                self.offset_finder.exe_path = path;
                self.save_exe_path();
            }

        if let Some((kind, write_out)) = start_scan {
            self.save_exe_path();
            self.start_scan(kind, write_out);
        }

        // Apply wizard commands collected inside the closure.
        if do_start_wizard {
            self.start_wizard_thread();
        }
        if let Some(cmd) = wizard_cmd
            && let Ok(mut s) = self.offset_finder.wizard_shared.lock()
        {
            s.command = cmd;
        }
        if do_write_ini {
            self.offset_finder.wizard_write_result = write_wizard_results(
                &wizard_results,
                &self.ini_path,
                &self.config_ini_path,
            );
            self.reload_flag.store(true, Ordering::Relaxed);
        }
    }
}

// ── Native file dialog ────────────────────────────────────────────────────────

/// Opens a native file-open dialog and returns the chosen eqgame.exe path, or None
/// if the user canceled or an error occurred.
fn browse_for_exe() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("Executable Files", &["exe"])
        .pick_file()
        .map(|p| p.display().to_string())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn load_app_icon() -> Icon {
    let bytes = include_bytes!("../../../assets/WinShowEQ.png");
    let img = image::load_from_memory(bytes)
        .expect("valid PNG icon")
        .into_rgba8();
    let (width, height) = img.dimensions();
    Icon::from_rgba(img.into_raw(), width, height).expect("valid tray icon")
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

        // with_visible(false) in NativeOptions is unreliable on Windows; force-hide on first frame.
        if std::mem::replace(&mut self.first_frame, false) && !self.window_visible {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

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
                let new_log_entry = log.len() > self.prev_log_len;
                self.prev_log_len = log.len();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &log {
                            ui.label(RichText::new(line).monospace());
                        }
                        if new_log_entry {
                            ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
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