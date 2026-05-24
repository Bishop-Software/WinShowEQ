use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, RichText};
use tray_icon::{
    Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

use super::GuiState;
use crate::config::{IniReader, PrimaryOffsets};
use crate::scanner::EqGameScanner;
use crate::session::SessionState;
use crate::wizard::{
    VerifyReadings, WizardCommand, WizardPhase, WizardShared, start_wizard, write_wizard_results,
};

const WINDOW_PADDING: i8 = 10;
const SIDE_BY_SIDE_MIN_WIDTH: f32 = 560.0;

// ── Offset Finder ────────────────────────────────────────────────────────────

struct OffsetFinderState {
    open: bool,
    exe_path: String,
    scanning: bool,
    pending: Arc<Mutex<Option<String>>>,
    scan_log: String,
    auto_start_wizard: bool,
    wizard_running: bool,
    wizard_shared: Arc<Mutex<WizardShared>>,
    wizard_write_result: String,
    name_input: String,
    last_name_input: String,
    verify_readings: Option<VerifyReadings>,
}

impl Default for OffsetFinderState {
    fn default() -> Self {
        Self {
            open: false,
            exe_path: String::new(),
            scanning: false,
            pending: Arc::new(Mutex::new(None)),
            scan_log: String::new(),
            auto_start_wizard: false,
            wizard_running: false,
            wizard_shared: Arc::new(Mutex::new(WizardShared::default())),
            wizard_write_result: String::new(),
            name_input: String::new(),
            last_name_input: String::new(),
            verify_readings: None,
        }
    }
}

impl OffsetFinderState {
    fn with_exe_path(exe_path: String) -> Self {
        Self {
            exe_path,
            ..Self::default()
        }
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

    fn start_combined_run(&mut self) {
        self.offset_finder.scanning = true;
        self.offset_finder.auto_start_wizard = true;
        self.offset_finder.scan_log.clear();
        self.offset_finder.wizard_shared = Arc::new(Mutex::new(WizardShared::default()));
        self.offset_finder.wizard_write_result.clear();

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
            let scan_result = scanner.scan_executable(&ir, &current_offsets, false);
            let mut output = scan_result.output.clone();
            output.push_str("\n[Primary scan complete — not yet written]\n");

            // Use the just-scanned CharInfo address for the secondary scan;
            // fall back to the INI value if the pattern didn't match.
            let new_char_info = if scan_result.primary.self_addr != 0 {
                scan_result.primary.self_addr
            } else {
                current_offsets.self_addr
            };
            let secondary = scanner.scan_secondary(&ir, new_char_info, false);
            output.push_str(&secondary);
            output.push_str("\n[Secondary scan complete — not yet written]");

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

        // Seed WizardShared with addresses from the scan log so the wizard
        // doesn't need to read them from the INI (which hasn't been written yet).
        let (primary_addrs, secondary_offsets) = parse_scan_log(&self.offset_finder.scan_log);

        let get_addr = |key: &str| -> u64 {
            primary_addrs
                .iter()
                .find(|(k, _)| k == key)
                .and_then(|(_, v)| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
                .unwrap_or(0)
        };
        let scan_primary = PrimaryOffsets {
            zone_name: get_addr("ZoneAddr"),
            spawn_list: get_addr("SpawnHeaderAddr"),
            self_addr: get_addr("CharInfo"),
            ground: get_addr("ItemsAddr"),
            target: get_addr("TargetAddr"),
            world: get_addr("WorldAddr"),
        };
        let scan_secondary: Vec<(String, u64)> = secondary_offsets
            .iter()
            .filter_map(|(k, v)| {
                u64::from_str_radix(v.trim_start_matches("0x"), 16)
                    .ok()
                    .map(|n| (k.clone(), n))
            })
            .collect();

        let scan_file_info: Vec<(String, String)> = {
            let mut items = Vec::new();
            let mut in_section = false;
            for line in self.offset_finder.scan_log.lines() {
                let line = line.trim();
                if line.eq_ignore_ascii_case("[file info]") {
                    in_section = true;
                    continue;
                }
                if line.starts_with('[') {
                    in_section = false;
                    continue;
                }
                if in_section && let Some(eq) = line.find('=') {
                    let key = line[..eq].trim().to_owned();
                    let val = line[eq + 1..].trim().to_owned();
                    if !val.is_empty() {
                        items.push((key, val));
                    }
                }
            }
            items
        };

        if let Ok(mut s) = self.offset_finder.wizard_shared.lock() {
            s.scan_primary = scan_primary;
            s.scan_secondary = scan_secondary;
            s.scan_file_info = scan_file_info;
        }

        start_wizard(
            self.ini_path.clone(),
            self.config_ini_path.clone(),
            self.patterns_ini_path.clone(),
            self.offset_finder.exe_path.clone(),
            Arc::clone(&self.offset_finder.wizard_shared),
            true, // scan already done by start_combined_run
        );
    }

    fn render_offset_finder(&mut self, ctx: &egui::Context) {
        if !self.offset_finder.open {
            return;
        }

        // Poll for a completed background scan; auto-start wizard when ready.
        // Extract the result in its own scope so the MutexGuard drops before
        // start_wizard_thread() needs &mut self.
        let scan_result = if self.offset_finder.scanning {
            self.offset_finder
                .pending
                .lock()
                .ok()
                .and_then(|mut g| g.take())
        } else {
            None
        };
        if let Some(result) = scan_result {
            self.offset_finder.scan_log = result;
            self.offset_finder.scanning = false;
            if self.offset_finder.auto_start_wizard {
                self.offset_finder.auto_start_wizard = false;
                self.start_wizard_thread();
            }
        }

        let mut open = true;
        let mut do_browse = false;
        let mut do_start = false;
        let scanning = self.offset_finder.scanning;
        let has_path = !self.offset_finder.exe_path.trim().is_empty();
        let mut exe_path = std::mem::take(&mut self.offset_finder.exe_path);
        let mut name_input = std::mem::take(&mut self.offset_finder.name_input);
        let mut last_name_input = std::mem::take(&mut self.offset_finder.last_name_input);
        let scan_log = self.offset_finder.scan_log.clone();

        let (wizard_phase, wizard_log, wizard_results) = {
            match self.offset_finder.wizard_shared.lock() {
                Ok(s) => {
                    if s.phase == WizardPhase::Verify {
                        self.offset_finder.verify_readings = s.verify.clone();
                    }
                    (s.phase.clone(), s.log.clone(), s.results.clone())
                }
                Err(_) => (WizardPhase::Idle, vec![], Default::default()),
            }
        };
        if self.offset_finder.wizard_running
            && matches!(
                wizard_phase,
                WizardPhase::Complete | WizardPhase::Failed | WizardPhase::Cancelled
            )
        {
            self.offset_finder.wizard_running = false;
        }
        let wizard_running = self.offset_finder.wizard_running;
        let wizard_write_result = self.offset_finder.wizard_write_result.clone();

        let is_running = scanning || wizard_running;

        let status_text = if scanning {
            "Step 1 of 2: Scanning executable…".to_owned()
        } else if wizard_running {
            wizard_phase.instruction().to_owned()
        } else {
            match &wizard_phase {
                WizardPhase::Complete => "Complete — offsets discovered.".to_owned(),
                WizardPhase::Failed => "Failed.".to_owned(),
                WizardPhase::Cancelled => "Cancelled.".to_owned(),
                _ => String::new(),
            }
        };

        let mut wizard_cmd: Option<WizardCommand> = None;
        let mut do_write_ini = false;
        let mut do_confirm_name = false;

        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("offset_finder"),
            egui::ViewportBuilder::default()
                .with_title("Offset Finder")
                .with_inner_size(egui::vec2(900.0, 850.0))
                .with_min_inner_size(egui::vec2(900.0, 850.0))
                .with_resizable(false),
            |ctx, _class| {
                if ctx.input(|i| i.viewport().close_requested()) {
                    open = false;
                }

                #[allow(deprecated)]
                egui::CentralPanel::default().show(ctx, |ui| {
                    // Header — exe path + action row
                    egui::Panel::top("offset_finder_header")
                        .resizable(false)
                        .show_inside(ui, |ui| {
                            egui::Frame::NONE
                                .inner_margin(egui::Margin::same(WINDOW_PADDING))
                                .show(ui, |ui| {
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
                                        ui.add_enabled_ui(!is_running && has_path, |ui| {
                                            if ui.button("Update Offsets").clicked() {
                                                do_start = true;
                                            }
                                        });
                                        ui.add_enabled_ui(wizard_running, |ui| {
                                            if ui.button("Cancel").clicked() {
                                                wizard_cmd = Some(WizardCommand::Cancel);
                                            }
                                        });
                                        if is_running {
                                            ui.add(egui::Spinner::new());
                                        }
                                        if !status_text.is_empty() {
                                            ui.label(RichText::new(&status_text).italics());
                                        }
                                    });
                                });
                        });

                    // Central area — offsets left, log right
                    egui::Frame::NONE
                        .inner_margin(egui::Margin {
                            left: WINDOW_PADDING,
                            right: WINDOW_PADDING,
                            top: 4,
                            bottom: 4,
                        })
                        .show(ui, |ui| {
                            let (primary_addrs, secondary_offsets) = parse_scan_log(&scan_log);
                            let _has_wizard = wizard_results.name.is_some()
                                || wizard_results.x.is_some()
                                || wizard_results.item_name.is_some();

                            ui.horizontal_top(|ui| {
                                let total_w = ui.available_width();
                                let left_w = (total_w * 0.38).floor();
                                let avail_h = ui.available_height();

                                // ── Left: discovered offsets ─────────────────
                                ui.allocate_ui(egui::vec2(left_w, avail_h), |ui| {
                                    let avail_h = ui.available_height();
                                    egui::ScrollArea::vertical()
                                        .id_salt("results_scroll")
                                        .max_height(avail_h)
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            const PRIMARY_KEYS: &[&str] = &[
                                                "ZoneAddr",
                                                "SpawnHeaderAddr",
                                                "CharInfo",
                                                "ItemsAddr",
                                                "TargetAddr",
                                                "WorldAddr",
                                            ];
                                            const SECONDARY_KEYS: &[&str] = &[
                                                "TypeOffset",
                                                "SpawnIDOffset",
                                                "LevelOffset",
                                                "RaceOffset",
                                                "ClassOffset",
                                                "PrimaryOffset",
                                                "OffhandOffset",
                                            ];

                                            egui::Grid::new("offsets_grid")
                                                .num_columns(2)
                                                .spacing([12.0, 2.0])
                                                .show(ui, |ui| {
                                                    // ── Memory Offsets ──
                                                    ui.label(
                                                        RichText::new("Memory Offsets").strong(),
                                                    );
                                                    ui.label("");
                                                    ui.end_row();
                                                    for &key in PRIMARY_KEYS {
                                                        ui.label(key);
                                                        let val = primary_addrs
                                                            .iter()
                                                            .find(|(k, _)| k == key)
                                                            .map(|(_, v)| v.as_str())
                                                            .unwrap_or("—");
                                                        if val == "—" {
                                                            ui.label(RichText::new(val).weak());
                                                        } else {
                                                            ui.monospace(val);
                                                        }
                                                        ui.end_row();
                                                    }
                                                    ui.label("");
                                                    ui.label("");
                                                    ui.end_row();

                                                    // ── SpawnInfo Offsets ──
                                                    ui.label(
                                                        RichText::new("SpawnInfo Offsets").strong(),
                                                    );
                                                    ui.label("");
                                                    ui.end_row();
                                                    for &key in SECONDARY_KEYS {
                                                        ui.label(key);
                                                        let val = secondary_offsets
                                                            .iter()
                                                            .find(|(k, _)| k == key)
                                                            .map(|(_, v)| v.as_str())
                                                            .unwrap_or("—");
                                                        if val == "—" {
                                                            ui.label(RichText::new(val).weak());
                                                        } else {
                                                            ui.monospace(val);
                                                        }
                                                        ui.end_row();
                                                    }
                                                    ui.label("");
                                                    ui.label("");
                                                    ui.end_row();

                                                    // ── Wizard Results ──
                                                    ui.label(
                                                        RichText::new("Wizard Results").strong(),
                                                    );
                                                    ui.label("");
                                                    ui.end_row();
                                                    let mut row =
                                                        |label: &str, val: Option<usize>| {
                                                            ui.label(label);
                                                            match val {
                                                                Some(v) => {
                                                                    ui.monospace(format!(
                                                                        "0x{v:x}"
                                                                    ));
                                                                }
                                                                None => {
                                                                    ui.label(
                                                                        RichText::new("—").weak(),
                                                                    );
                                                                }
                                                            }
                                                            ui.end_row();
                                                        };
                                                    row("NameOffset", wizard_results.name);
                                                    row("LastNameOffset", wizard_results.last_name);
                                                    row("NextOffset", wizard_results.next);
                                                    row("PrevOffset", wizard_results.prev);
                                                    row("XOffset", wizard_results.x);
                                                    row("YOffset", wizard_results.y);
                                                    row("ZOffset", wizard_results.z);
                                                    row("HeadingOffset", wizard_results.heading);
                                                    row("SpeedOffset", wizard_results.speed);
                                                    row("HideOffset", wizard_results.hidden);
                                                    row("OwnerIDOffset", wizard_results.owner);
                                                    row(
                                                        "Item.NameOffset",
                                                        wizard_results.item_name,
                                                    );
                                                    row("Item.XOffset", wizard_results.item_x);
                                                    row("Item.YOffset", wizard_results.item_y);
                                                    row("Item.ZOffset", wizard_results.item_z);
                                                    row(
                                                        "Item.PrevOffset",
                                                        wizard_results.item_prev,
                                                    );
                                                    row(
                                                        "Item.NextOffset",
                                                        wizard_results.item_next,
                                                    );
                                                    row("Item.IdOffset", wizard_results.item_id);
                                                    row(
                                                        "Item.DropIdOffset",
                                                        wizard_results.item_drop_id,
                                                    );
                                                });
                                            ui.add_space(6.0);
                                        });
                                }); // allocate_ui (left column)

                                // ── Right: log ───────────────────────────────
                                ui.vertical(|ui| {
                                    ui.label(RichText::new("Log").strong());

                                    if wizard_running {
                                        let show_action = matches!(
                                            wizard_phase,
                                            WizardPhase::EnterName
                                                | WizardPhase::StandStill
                                                | WizardPhase::Walking
                                                | WizardPhase::Stopped
                                                | WizardPhase::Turning
                                                | WizardPhase::WaitInvis
                                                | WizardPhase::WaitPet
                                                | WizardPhase::WaitItem
                                        );
                                        if show_action {
                                            if matches!(wizard_phase, WizardPhase::EnterName) {
                                                ui.horizontal(|ui| {
                                                    ui.label("Name:");
                                                    ui.add(
                                                        egui::TextEdit::singleline(&mut name_input)
                                                            .desired_width(110.0)
                                                            .hint_text("Yourname"),
                                                    );
                                                    ui.label("Surname:");
                                                    ui.add(
                                                        egui::TextEdit::singleline(
                                                            &mut last_name_input,
                                                        )
                                                        .desired_width(110.0)
                                                        .hint_text("optional"),
                                                    );
                                                    if ui.button("Confirm Name").clicked() {
                                                        do_confirm_name = true;
                                                        wizard_cmd =
                                                            Some(WizardCommand::ActionDone);
                                                    }
                                                    if ui.button("Skip").clicked() {
                                                        wizard_cmd = Some(WizardCommand::SkipStep);
                                                    }
                                                });
                                            } else {
                                                ui.horizontal(|ui| {
                                                    let action_label = match wizard_phase {
                                                        WizardPhase::StandStill => {
                                                            "I'm standing still"
                                                        }
                                                        WizardPhase::Walking => "Start walking",
                                                        WizardPhase::Stopped => "I'm stopped",
                                                        WizardPhase::Turning => "Start turning",
                                                        WizardPhase::WaitInvis => "I cast invis",
                                                        WizardPhase::WaitPet => "I have a pet",
                                                        WizardPhase::WaitItem => "Item dropped",
                                                        _ => "Done",
                                                    };
                                                    if ui.button(action_label).clicked() {
                                                        wizard_cmd =
                                                            Some(WizardCommand::ActionDone);
                                                    }
                                                    if ui.button("Skip").clicked() {
                                                        wizard_cmd = Some(WizardCommand::SkipStep);
                                                    }
                                                });
                                            }
                                            ui.add_space(4.0);
                                        }
                                    }

                                    if matches!(wizard_phase, WizardPhase::Complete) {
                                        ui.horizontal(|ui| {
                                            if ui.button("Write to INI").clicked() {
                                                do_write_ini = true;
                                            }
                                        });
                                        ui.add_space(4.0);
                                    }

                                    let log_h = ui.available_height() - 4.0;
                                    egui::ScrollArea::vertical()
                                        .id_salt("log_scroll")
                                        .max_height(log_h)
                                        .auto_shrink([false, false])
                                        .stick_to_bottom(true)
                                        .show(ui, |ui| {
                                            if !scan_log.is_empty() {
                                                for line in scan_log.lines() {
                                                    ui.label(
                                                        RichText::new(line).monospace().size(11.0),
                                                    );
                                                }
                                                if !wizard_log.is_empty() {
                                                    ui.separator();
                                                }
                                            }
                                            for line in &wizard_log {
                                                ui.label(
                                                    RichText::new(line).monospace().size(11.0),
                                                );
                                            }
                                            if !wizard_write_result.is_empty() {
                                                if !scan_log.is_empty() || !wizard_log.is_empty() {
                                                    ui.separator();
                                                }
                                                for line in wizard_write_result.lines() {
                                                    ui.label(
                                                        RichText::new(line).monospace().size(11.0),
                                                    );
                                                }
                                            }
                                        });
                                }); // ui.vertical (right column)
                            }); // ui.horizontal_top
                        });
                });
            },
        );

        self.offset_finder.open = open;
        self.offset_finder.exe_path = exe_path;
        self.offset_finder.name_input = name_input.clone();
        self.offset_finder.last_name_input = last_name_input.clone();

        if do_browse && let Some(path) = browse_for_exe() {
            self.offset_finder.exe_path = path;
            self.save_exe_path();
        }

        if do_start {
            self.save_exe_path();
            self.start_combined_run();
        }

        if let Some(cmd) = wizard_cmd
            && let Ok(mut s) = self.offset_finder.wizard_shared.lock()
        {
            if do_confirm_name {
                s.char_name = name_input.trim().to_owned();
                s.char_last_name = last_name_input.trim().to_owned();
            }
            s.command = cmd;
        }
        if do_write_ini {
            let (scan_primary, scan_secondary, scan_file_info) =
                match self.offset_finder.wizard_shared.lock() {
                    Ok(s) => (
                        s.scan_primary.clone(),
                        s.scan_secondary.clone(),
                        s.scan_file_info.clone(),
                    ),
                    Err(_) => (PrimaryOffsets::default(), vec![], vec![]),
                };
            self.offset_finder.wizard_write_result = write_wizard_results(
                &wizard_results,
                &scan_primary,
                &scan_secondary,
                &scan_file_info,
                &self.ini_path,
                &self.config_ini_path,
            );
            self.reload_flag.store(true, Ordering::Relaxed);
        }
    }
}

// ── Scan log parser ───────────────────────────────────────────────────────────

type KvList = Vec<(String, String)>;

/// Parse the raw scan_log text into (primary_addresses, secondary_offsets).
/// Expects lines formatted as `Key=0xvalue # status` under `[Memory Offsets]`
/// and `[SpawnInfo Offsets]` section headers.
fn parse_scan_log(log: &str) -> (KvList, KvList) {
    let mut primary = Vec::new();
    let mut secondary = Vec::new();
    let mut in_primary = false;
    let mut in_secondary = false;
    for line in log.lines() {
        let line = line.trim();
        if line.eq_ignore_ascii_case("[memory offsets]") {
            in_primary = true;
            in_secondary = false;
            continue;
        }
        if line.eq_ignore_ascii_case("[spawninfo offsets]") {
            in_secondary = true;
            in_primary = false;
            continue;
        }
        if line.starts_with('[') {
            in_primary = false;
            in_secondary = false;
            continue;
        }
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim().to_owned();
            let rest = &line[eq + 1..];
            // Take everything up to the first space (the 0xvalue part)
            let val = rest.split_whitespace().next().unwrap_or("—").to_owned();
            if in_primary {
                primary.push((key, val));
            } else if in_secondary {
                secondary.push((key, val));
            }
        }
    }
    (primary, secondary)
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
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
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

                let zone = if snapshot.clear_zone_and_name {
                    ""
                } else {
                    snapshot.zone.as_str()
                };
                let character = if snapshot.clear_zone_and_name {
                    ""
                } else {
                    snapshot.character_name.as_str()
                };
                let port_str = if snapshot.port > 0 {
                    snapshot.port.to_string()
                } else {
                    String::new()
                };

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
    ui.label(if count < 0 {
        "—".into()
    } else {
        count.to_string()
    });
    ui.end_row();
}
