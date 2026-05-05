use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{IPT_GROUND, IPT_SELF, IPT_SPAWNS, IPT_TARGET, IPT_WORLD, IPT_ZONE, OPT_GROUND};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};

use crate::config::ClientConfig;
use crate::data::annotations::AnnotationStore;
use crate::data::timers::{SpawnTimer, TimerStore};
use crate::map_reader;
use crate::data::{apply_packet, configure_alerts, AppData};
use crate::game_data::GameData;
use crate::logger::{LogLevel, Logger};
use crate::net::ServerConnection;
use crate::protocol::decode_packet;
use crate::ui::about::AboutDialog;
use crate::ui::ground_list;
use crate::ui::login::LoginDialog;
use crate::ui::map_pane::MapPane;
use crate::ui::options::OptionsDialog;
use crate::ui::search_dialog::SearchDialog;
use crate::ui::spawn_list::{self, SpawnAction};
use crate::ui::timer_list;

const TICK_REQUEST: i32 =
    IPT_ZONE | IPT_SELF | IPT_TARGET | IPT_SPAWNS | IPT_GROUND | IPT_WORLD;
const TICK_DELAY_MS: u64 = 250;
const RECONNECT_DELAY_SECS: u64 = 2;

/// Identifies each dockable panel.
#[derive(Debug, Clone, PartialEq)]
enum Tab {
    Spawns,
    Timers,
    Ground,
    Map,
}

/// State for the "Add Note" floating dialog.
#[derive(Default)]
struct AddNoteDialog {
    open: bool,
    text: String,
    color_idx: usize,
    /// When set, the note is placed at this position instead of the player's position.
    override_pos: Option<(f32, f32, f32)>,
}

/// State for the "Add Timer" floating dialog.
#[derive(Default)]
struct AddTimerDialog {
    open: bool,
    name: String,
    x: f32,
    y: f32,
    z: f32,
    /// Respawn time in minutes, as user-editable text.
    respawn_input: String,
}

const NOTE_COLORS: &[[u8; 3]] = &[
    [255, 255, 255],
    [255, 255, 0],
    [255, 100, 100],
    [100, 255, 100],
    [100, 200, 255],
];
const NOTE_COLOR_NAMES: &[&str] = &["White", "Yellow", "Red", "Green", "Cyan"];

pub struct MainApp {
    data: Arc<Mutex<AppData>>,
    config: ClientConfig,
    config_path: PathBuf,
    logger: Logger,
    map_pane: MapPane,
    login: LoginDialog,
    options: OptionsDialog,
    about: AboutDialog,
    dock_state: DockState<Tab>,
    add_note: AddNoteDialog,
    add_timer: AddTimerDialog,
    pending_spawn_action: Option<SpawnAction>,
    stop: Arc<AtomicBool>,
    server_addr: Arc<Mutex<Option<SocketAddr>>>,
    prev_zone: String,
    spawn_sort_column: Option<usize>,
    spawn_sort_ascending: bool,
    timer_sort_column: Option<usize>,
    timer_sort_ascending: bool,
    ground_sort_column: Option<usize>,
    ground_sort_ascending: bool,
    search: SearchDialog,
}

/// Build the initial dock layout: Spawns (top-left) + Timers (middle-left) + Ground (bottom-left) + Map (right).
fn build_dock_state() -> DockState<Tab> {
    let mut state = DockState::new(vec![Tab::Map]);
    let surface = state.main_surface_mut();
    let [_, spawns] = surface.split_left(NodeIndex::root(), 0.35, vec![Tab::Spawns]);
    let [_, timers] = surface.split_below(spawns, 0.40, vec![Tab::Timers]);
    surface.split_below(timers, 0.50, vec![Tab::Ground]);
    state
}

impl MainApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        server_addr: Option<SocketAddr>,
        config_path: PathBuf,
    ) -> Self {
        // Configure Arial font for better Unicode support (arrows, international text)
        let mut fonts = egui::FontDefinitions::default();
        let font_data = egui::FontData::from_static(include_bytes!("../../assets/Arial.ttf"));
        fonts.font_data.insert("arial".to_owned(), Arc::new(font_data));
        fonts.families.get_mut(&egui::FontFamily::Proportional)
            .unwrap()
            .insert(0, "arial".to_owned());
        cc.egui_ctx.set_fonts(fonts);

        let config = ClientConfig::load(&config_path);
        let logger = Logger::new(&config.log_dir);
        logger.set_enabled(config.log_enabled);
        logger.set_level(LogLevel::from_str(&config.log_level));
        let data = Arc::new(Mutex::new(AppData::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let addr_cell: Arc<Mutex<Option<SocketAddr>>> = Arc::new(Mutex::new(server_addr));

        {
            let mut d = data.lock().unwrap();
            configure_alerts(&mut d, &config);
            if let Some(gd) = GameData::load(&config.eq_path) {
                d.game_data = gd;
            }
            d.filters = crate::filters::FilterSet::load(
                &std::path::Path::new(&config.filter_dir).join("seqfilters.xml"),
            );
            let filters = d.filters.clone();
            d.spawns.reclassify_all(&filters);
        }

        start_network_thread(
            Arc::clone(&addr_cell),
            Arc::clone(&data),
            Arc::clone(&stop),
            logger.clone(),
        );

        let login = LoginDialog::new(server_addr);
        let options = OptionsDialog::new(&config, false);
        let about = AboutDialog::new();

        Self {
            data,
            config,
            config_path,
            logger,
            map_pane: MapPane::default(),
            login,
            options,
            about,
            dock_state: build_dock_state(),
            add_note: AddNoteDialog::default(),
            add_timer: AddTimerDialog::default(),
            pending_spawn_action: None,
            stop,
            server_addr: addr_cell,
            prev_zone: String::new(),
            spawn_sort_column: None,
            spawn_sort_ascending: true,
            timer_sort_column: None,
            timer_sort_ascending: true,
            ground_sort_column: None,
            ground_sort_ascending: true,
            search: SearchDialog::default(),
        }
    }

    fn save_config(&self) {
        let _ = self.config.save(&self.config_path);
    }

    fn handle_zone_change(&mut self, new_zone: String) {
        if !self.prev_zone.is_empty() {
            let data = self.data.lock().unwrap();
            let _ = data.timers.save(&self.prev_zone, &self.config.timer_dir);
            let _ = data.annotations.save(&self.prev_zone, &self.config.cfg_dir);
        }
        let new_timers = TimerStore::load(&new_zone, &self.config.timer_dir);
        let new_annotations = AnnotationStore::load(&new_zone, &self.config.cfg_dir);
        // Zone names from EQ are lowercase short names; map files use the same convention.
        let new_map = map_reader::load_zone(
            std::path::Path::new(&self.config.map_dir),
            &new_zone.to_lowercase(),
        )
        .unwrap_or_default();
        let mut data = self.data.lock().unwrap();
        data.timers = new_timers;
        data.annotations = new_annotations;
        data.map = new_map;
        self.prev_zone = new_zone;
    }

    /// Dispatch a `SpawnAction` returned from the spawn list context menu.
    fn handle_spawn_action(&mut self, action: SpawnAction) {
        match action {
            SpawnAction::AddTimer { name, x, y, z } => {
                self.add_timer.open = true;
                self.add_timer.name = name;
                self.add_timer.x = x;
                self.add_timer.y = y;
                self.add_timer.z = z;
                self.add_timer.respawn_input = "30".to_owned();
            }
            SpawnAction::AddToFilter { name, category } => {
                let mut data = self.data.lock().unwrap();
                data.filters.add(category, name);
                let filters = data.filters.clone();
                data.spawns.reclassify_all(&filters);
                let _ = data.filters.save(
                    &std::path::Path::new(&self.config.filter_dir).join("seqfilters.xml"),
                );
            }
            SpawnAction::AddMapText { x, y, z } => {
                self.add_note.open = true;
                self.add_note.text.clear();
                self.add_note.override_pos = Some((x, y, z));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// TabViewer — renders each panel's content, created fresh each frame.
// ---------------------------------------------------------------------------

struct WinSeqTabViewer<'a> {
    data: &'a Arc<Mutex<AppData>>,
    map_pane: &'a mut MapPane,
    spawn_sort_column: &'a mut Option<usize>,
    spawn_sort_ascending: &'a mut bool,
    timer_sort_column: &'a mut Option<usize>,
    timer_sort_ascending: &'a mut bool,
    ground_sort_column: &'a mut Option<usize>,
    ground_sort_ascending: &'a mut bool,
    spawn_action: &'a mut Option<SpawnAction>,
}

impl<'a> TabViewer for WinSeqTabViewer<'a> {
    type Tab = Tab;

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        match tab {
            Tab::Spawns => "Spawns".into(),
            Tab::Timers => "Timers".into(),
            Tab::Ground => "Ground Items".into(),
            Tab::Map => "Map".into(),
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        match tab {
            Tab::Spawns => {
                let mut data = self.data.lock().unwrap();
                ui.label(format!("Spawns ({})", data.spawns.len()));
                ui.separator();
                *self.spawn_action = spawn_list::show(
                    ui,
                    &mut data,
                    self.spawn_sort_column,
                    self.spawn_sort_ascending,
                );
            }
            Tab::Timers => {
                let mut data = self.data.lock().unwrap();
                timer_list::show(ui, &mut data, &mut self.timer_sort_column, &mut self.timer_sort_ascending);
            }
            Tab::Ground => {
                let mut data = self.data.lock().unwrap();
                ground_list::show(ui, &mut data, &mut self.ground_sort_column, &mut self.ground_sort_ascending);
            }
            Tab::Map => {
                let data = self.data.lock().unwrap();
                self.map_pane.show(ui, &data);
            }
        }
    }

    fn is_closeable(&self, _tab: &Tab) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// eframe::App
// ---------------------------------------------------------------------------

impl eframe::App for MainApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(TICK_DELAY_MS));

        let zone_name = self.data.lock().unwrap().zone_name.clone();
        if !zone_name.is_empty() && zone_name != self.prev_zone {
            self.handle_zone_change(zone_name);
        }

        if ctx.input(|i| i.viewport().close_requested()) {
            if !self.prev_zone.is_empty() {
                let data = self.data.lock().unwrap();
                let _ = data.timers.save(&self.prev_zone, &self.config.timer_dir);
                let _ = data.annotations.save(&self.prev_zone, &self.config.cfg_dir);
            }
            self.save_config();
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Floating dialogs
        if let Some(new_addr) = self.login.show(&ctx) {
            *self.server_addr.lock().unwrap() = Some(new_addr);
        }
        if let Some(new_config) = self.options.show(&ctx) {
            let trails = self.options.trails_enabled;
            self.config = new_config;
            self.logger.set_enabled(self.config.log_enabled);
            self.logger.set_level(LogLevel::from_str(&self.config.log_level));
            {
                let mut data = self.data.lock().unwrap();
                data.trails_enabled = trails;
                configure_alerts(&mut data, &self.config);
                if let Some(gd) = GameData::load(&self.config.eq_path) {
                    data.game_data = gd;
                }
            }
            let _ = self.config.save(&self.config_path);
        }
        self.about.show(&ctx);

        // Ctrl+F opens spawn search
        if ctx.input(|i| i.key_pressed(egui::Key::F) && i.modifiers.ctrl) {
            self.search.open();
        }

        // Search dialog — runs outside the DockArea lock so it can mutate AppData directly
        {
            let mut data = self.data.lock().unwrap();
            if let Some(spawn_id) = self.search.show(&ctx, &mut data) {
                if let Some(s) = data.spawns.get(spawn_id) {
                    let (mx, my) = crate::map_con::eq_to_map_pub(s.x, s.y);
                    self.map_pane.state.pending_center = Some((mx, my));
                }
            }
        }

        // "Add Note" floating dialog
        if self.add_note.open {
            let mut open = true;
            egui::Window::new("Add Map Note")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(&ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Text:");
                        ui.text_edit_singleline(&mut self.add_note.text);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Color:");
                        for (i, name) in NOTE_COLOR_NAMES.iter().enumerate() {
                            let [r, g, b] = NOTE_COLORS[i];
                            let color = egui::Color32::from_rgb(r, g, b);
                            let selected = self.add_note.color_idx == i;
                            if ui
                                .add(egui::SelectableLabel::new(
                                    selected,
                                    egui::RichText::new(*name).color(color),
                                ))
                                .clicked()
                            {
                                self.add_note.color_idx = i;
                            }
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let can_add = !self.add_note.text.trim().is_empty();
                        if ui.add_enabled(can_add, egui::Button::new("Add")).clicked() {
                            let (x, y, z) = self
                                .add_note
                                .override_pos
                                .take()
                                .or_else(|| self.data.lock().unwrap().player_pos())
                                .unwrap_or((0.0, 0.0, 0.0));
                            let color = NOTE_COLORS[self.add_note.color_idx];
                            self.data.lock().unwrap().annotations.add(
                                self.add_note.text.trim().to_owned(),
                                x,
                                y,
                                z,
                                color,
                                12,
                            );
                            self.add_note.open = false;
                            self.add_note.text.clear();
                        }
                        if ui.button("Cancel").clicked() {
                            self.add_note.open = false;
                            self.add_note.override_pos = None;
                        }
                    });
                });
            if !open {
                self.add_note.open = false;
                self.add_note.override_pos = None;
            }
        }

        // "Add Timer" floating dialog
        if self.add_timer.open {
            let mut open = true;
            egui::Window::new("Add Timer")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(&ctx, |ui| {
                    ui.label(format!("Mob: {}", self.add_timer.name));
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Respawn (minutes):");
                        ui.text_edit_singleline(&mut self.add_timer.respawn_input);
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let minutes: Option<i64> = self
                            .add_timer
                            .respawn_input
                            .trim()
                            .parse::<i64>()
                            .ok()
                            .filter(|&m| m > 0);
                        if ui.add_enabled(minutes.is_some(), egui::Button::new("Add")).clicked() {
                            let timer = SpawnTimer::new(
                                self.add_timer.name.clone(),
                                self.add_timer.x,
                                self.add_timer.y,
                                self.add_timer.z,
                                minutes.unwrap() * 60,
                            );
                            self.data.lock().unwrap().timers.add(timer);
                            self.add_timer.open = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.add_timer.open = false;
                        }
                    });
                });
            if !open {
                self.add_timer.open = false;
            }
        }

        // Menu bar
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Connect…").clicked() {
                    self.login.open = true;
                    ui.close();
                }
                if ui.button("Options…").clicked() {
                    let trails = self.data.lock().unwrap().trails_enabled;
                    self.options.sync_from(&self.config, trails);
                    self.options.open = true;
                    ui.close();
                }
                ui.separator();
                if ui.button("Add Map Note…").clicked() {
                    self.add_note.open = true;
                    self.add_note.text.clear();
                    self.add_note.override_pos = None;
                    ui.close();
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.button("Find Spawn…  Ctrl+F").clicked() {
                    self.search.open();
                    ui.close();
                }
            });
            ui.menu_button("View", |ui| {
            });
            ui.menu_button("Map", |ui| {
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About…").clicked() {
                    self.about.open = true;
                    ui.close();
                }
            });
        });

        // Docked panel layout
        let mut viewer = WinSeqTabViewer {
            data: &self.data,
            map_pane: &mut self.map_pane,
            spawn_sort_column: &mut self.spawn_sort_column,
            spawn_sort_ascending: &mut self.spawn_sort_ascending,
            timer_sort_column: &mut self.timer_sort_column,
            timer_sort_ascending: &mut self.timer_sort_ascending,
            ground_sort_column: &mut self.ground_sort_column,
            ground_sort_ascending: &mut self.ground_sort_ascending,
            spawn_action: &mut self.pending_spawn_action,
        };
        DockArea::new(&mut self.dock_state).show_inside(ui, &mut viewer);

        // Handle any spawn context menu action from this frame
        if let Some(action) = self.pending_spawn_action.take() {
            self.handle_spawn_action(action);
        }
    }
}

// ---------------------------------------------------------------------------
// Network thread
// ---------------------------------------------------------------------------

fn start_network_thread(
    addr_cell: Arc<Mutex<Option<SocketAddr>>>,
    data: Arc<Mutex<AppData>>,
    stop: Arc<AtomicBool>,
    logger: Logger,
) {
    std::thread::Builder::new()
        .name("wseq-network".to_owned())
        .spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let addr = *addr_cell.lock().unwrap();
                let Some(addr) = addr else {
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                };
                match ServerConnection::connect_with_timeout(addr, 5000) {
                    Err(e) => {
                        logger.warn(&format!("Connection failed: {e}"));
                        std::thread::sleep(Duration::from_secs(RECONNECT_DELAY_SECS));
                    }
                    Ok(mut conn) => {
                        logger.info(&format!("Connected to {addr}"));
                        loop {
                            if stop.load(Ordering::Relaxed) {
                                return;
                            }
                            if addr_cell.lock().unwrap().as_ref() != Some(&addr) {
                                break;
                            }
                            match conn.tick(TICK_REQUEST) {
                                Ok(records) => {
                                    let mut d = data.lock().unwrap();
                                    if records.iter().any(|r| r.flags == OPT_GROUND) {
                                        d.ground.clear();
                                    }
                                    for rec in records {
                                        let pkt = decode_packet(rec);
                                        if let crate::protocol::Packet::Zone { name: ref z } = pkt
                                        {
                                            logger.info(&format!("Zone: {z}"));
                                        }
                                        if let Some(msg) = apply_packet(&mut d, pkt) {
                                            logger.info(&msg);
                                        }
                                    }
                                }
                                Err(e) => {
                                    logger.warn(&format!("Disconnected: {e}"));
                                    break;
                                }
                            }
                            std::thread::sleep(Duration::from_millis(TICK_DELAY_MS));
                        }
                    }
                }
            }
        })
        .expect("failed to spawn network thread");
}