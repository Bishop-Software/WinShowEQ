use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{IPT_GROUND, IPT_SELF, IPT_SPAWNS, IPT_TARGET, IPT_WORLD, IPT_ZONE, OPT_GROUND};
use egui_dock::{DockArea, DockState, NodeIndex, TabViewer};

use crate::config::{ClientConfig, MapOverlaySettings};
use crate::data::annotations::AnnotationStore;
use crate::data::timers::{SpawnObserver, SpawnTimer, TimerStore};
use crate::map_reader;
use crate::data::{apply_packet, configure_alerts, AppData};
use crate::game_data::GameData;
use crate::logger::{LogLevel, Logger};
use crate::net::ServerConnection;
use crate::protocol::decode_packet;
use crate::ui::about::AboutDialog;
use crate::ui::ground_list;
use crate::ui::help::HelpDialog;
use crate::ui::login::LoginDialog;
use crate::ui::map_pane::MapPane;
use crate::ui::options::OptionsDialog;
use crate::ui::search_dialog::SearchDialog;
use crate::filters::FilterCategory;
use crate::map_canvas::MapAction;
use crate::ui::spawn_list::{self, SpawnAction};
use crate::ui::timer_list;

const TICK_REQUEST: i32 =
    IPT_ZONE | IPT_SELF | IPT_TARGET | IPT_SPAWNS | IPT_GROUND | IPT_WORLD;
const TICK_DELAY_MS: u64 = 250;
const RECONNECT_DELAY_SECS: u64 = 2;

/// Identifies each dockable panel.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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

/// State for the "Add to Filter — choose scope" floating dialog.
#[derive(Default)]
struct AddFilterScopeDialog {
    open: bool,
    focus_requested: bool,
    name: String,
    category: Option<FilterCategory>,
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
    help: HelpDialog,
    dock_state: DockState<Tab>,
    hidden_panels: HashSet<Tab>,
    add_note: AddNoteDialog,
    add_timer: AddTimerDialog,
    pending_spawn_action: Option<SpawnAction>,
    pending_map_action: Option<MapAction>,
    add_filter_scope: AddFilterScopeDialog,
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
    pending_clear_timers: bool,
    last_timer_autosave: std::time::Instant,
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
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let config = ClientConfig::load(&config_path);
        let server_addr = server_addr.or_else(|| {
            if config.auto_connect {
                format!("{}:{}", config.server_ip, config.server_port).parse().ok()
            } else {
                None
            }
        });
        let logger = Logger::new(&config.log_dir);
        logger.set_enabled(config.log_enabled);
        logger.set_level(LogLevel::from_str(&config.log_level));
        // Restore persisted UI state (dock layout + column widths).
        let dock_state: Option<DockState<Tab>> = cc.storage
            .and_then(|s| eframe::get_value(s, "dock_state"));
        let spawn_col_widths: Option<Vec<f32>> = cc.storage
            .and_then(|s| eframe::get_value(s, "spawn_col_widths"));
        let timer_col_widths: Option<Vec<f32>> = cc.storage
            .and_then(|s| eframe::get_value(s, "timer_col_widths"));
        let ground_col_widths: Option<Vec<f32>> = cc.storage
            .and_then(|s| eframe::get_value(s, "ground_col_widths"));

        let data = Arc::new(Mutex::new(AppData::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let addr_cell: Arc<Mutex<Option<SocketAddr>>> = Arc::new(Mutex::new(server_addr));

        {
            let mut d = data.lock().unwrap();
            configure_alerts(&mut d, &config);
            if let Some(gd) = GameData::load(&config.eq_path) {
                d.game_data = gd;
            }
            d.game_data.load_classes(&std::path::Path::new(&config.cfg_dir).join("classes.json"));
            d.game_data.load_color_palette(&std::path::Path::new(&config.cfg_dir).join("colors.json"));
            d.game_data.load_spawn_colors(&std::path::Path::new(&config.cfg_dir).join("spawn_colors.json"));
            d.filter_dir = config.filter_dir.clone();
            d.filters_global = crate::filters::FilterSet::load(
                &std::path::Path::new(&config.filter_dir).join("filters_global.xml"),
            );
            d.recompute_filters();
            if let Some(w) = spawn_col_widths  && w.len() == d.spawn_list_column_widths.len()  { d.spawn_list_column_widths  = w; }
            if let Some(w) = timer_col_widths  && w.len() == d.timer_list_column_widths.len()  { d.timer_list_column_widths  = w; }
            if let Some(w) = ground_col_widths && w.len() == d.ground_list_column_widths.len() { d.ground_list_column_widths = w; }
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
        let help = HelpDialog::default();

        Self {
            data,
            config,
            config_path,
            logger,
            map_pane: MapPane::default(),
            login,
            options,
            about,
            help,
            dock_state: dock_state.unwrap_or_else(build_dock_state),
            hidden_panels: HashSet::new(),
            add_note: AddNoteDialog::default(),
            add_timer: AddTimerDialog::default(),
            pending_spawn_action: None,
            pending_map_action: None,
            add_filter_scope: AddFilterScopeDialog::default(),
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
            pending_clear_timers: false,
            last_timer_autosave: std::time::Instant::now(),
        }
    }

    fn save_config(&self) {
        let _ = self.config.save(&self.config_path);
    }

    fn handle_zone_change(&mut self, new_zone: String) {
        if !self.prev_zone.is_empty() {
            let data = self.data.lock().unwrap();
            let _ = data.timers.save(&self.prev_zone, &self.config.timer_dir);
            let _ = data.annotations.save(&self.prev_zone, &self.config.annotations_dir);
            let _ = SpawnObserver::save(&data.observer.observations, &self.prev_zone, &self.config.timer_dir);
        }
        let new_timers = TimerStore::load(&new_zone, &self.config.timer_dir);
        let new_annotations = AnnotationStore::load(&new_zone, &self.config.annotations_dir);
        let new_obs = SpawnObserver::load(&new_zone, &self.config.timer_dir);
        // Zone names from EQ are lowercase short names; map files use the same convention.
        let new_map = map_reader::load_zone(
            std::path::Path::new(&self.config.map_dir),
            &new_zone.to_lowercase(),
        )
        .unwrap_or_default();
        let mut data = self.data.lock().unwrap();
        data.timers = new_timers;
        data.annotations = new_annotations;
        data.observer.observations = new_obs;
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
                let zone_name = self.data.lock().unwrap().zone_name.clone();
                if zone_name.is_empty() {
                    self.write_filter_global(name, category);
                } else {
                    self.add_filter_scope.name = name.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end().to_string();
                    self.add_filter_scope.category = Some(category);
                    self.add_filter_scope.open = true;
                    self.add_filter_scope.focus_requested = false;
                }
            }
            SpawnAction::CenterMap { id, x, y } => {
                let (mx, my) = crate::map_canvas::eq_to_map_pub(x, y);
                self.map_pane.state.pending_center = Some((mx, my));
                let mut data = self.data.lock().unwrap();
                data.selected_id = Some(id);
            }
            SpawnAction::AddMapText { x, y, z } => {
                self.add_note.open = true;
                self.add_note.text.clear();
                self.add_note.override_pos = Some((x, y, z));
            }
        }
    }

    fn write_filter_global(&mut self, name: String, category: FilterCategory) {
        let path = std::path::Path::new(&self.config.filter_dir).join("filters_global.xml");
        let mut data = self.data.lock().unwrap();
        data.filters_global.add(category, name);
        let _ = data.filters_global.save(&path);
        data.recompute_filters();
    }

    fn write_filter_zone(&mut self, name: String, category: FilterCategory) {
        let zone_name = self.data.lock().unwrap().zone_name.clone();
        let path = std::path::Path::new(&self.config.filter_dir)
            .join(format!("filters_{}.xml", zone_name.to_lowercase()));
        let mut data = self.data.lock().unwrap();
        data.filters_zone.add(category, name);
        let _ = data.filters_zone.save(&path);
        data.recompute_filters();
    }

    /// Show or hide a panel tab. The Map tab is never hidden.
    fn toggle_panel(&mut self, tab: Tab) {
        if tab == Tab::Map {
            return;
        }
        if let Some(path) = self.dock_state.find_tab(&tab) {
            self.dock_state.remove_tab(path);
            self.hidden_panels.insert(tab);
        } else {
            self.hidden_panels.remove(&tab);
            self.dock_state.push_to_first_leaf(tab);
        }
    }
}

// ---------------------------------------------------------------------------
// TabViewer — renders each panel's content, created fresh each frame.
// ---------------------------------------------------------------------------

struct WinSeqTabViewer<'a> {
    data: &'a Arc<Mutex<AppData>>,
    map_pane: &'a mut MapPane,
    overlay: &'a MapOverlaySettings,
    spawn_sort_column: &'a mut Option<usize>,
    spawn_sort_ascending: &'a mut bool,
    timer_sort_column: &'a mut Option<usize>,
    timer_sort_ascending: &'a mut bool,
    ground_sort_column: &'a mut Option<usize>,
    ground_sort_ascending: &'a mut bool,
    spawn_action: &'a mut Option<SpawnAction>,
    map_action: &'a mut Option<MapAction>,
    pending_clear_timers: &'a mut bool,
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
                if timer_list::show(ui, &mut data, self.timer_sort_column, self.timer_sort_ascending) {
                    *self.pending_clear_timers = true;
                }
            }
            Tab::Ground => {
                let mut data = self.data.lock().unwrap();
                ground_list::show(ui, &mut data, self.ground_sort_column, self.ground_sort_ascending);
            }
            Tab::Map => {
                let data = self.data.lock().unwrap();
                if let Some(action) = self.map_pane.show(ui, &data, self.overlay) {
                    *self.map_action = Some(action);
                }
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
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "dock_state", &self.dock_state);
        let data = self.data.lock().unwrap();
        eframe::set_value(storage, "spawn_col_widths",  &data.spawn_list_column_widths);
        eframe::set_value(storage, "timer_col_widths",  &data.timer_list_column_widths);
        eframe::set_value(storage, "ground_col_widths", &data.ground_list_column_widths);
    }

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
                let _ = data.annotations.save(&self.prev_zone, &self.config.annotations_dir);
                let _ = SpawnObserver::save(&data.observer.observations, &self.prev_zone, &self.config.timer_dir);
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
                data.game_data.load_classes(&std::path::Path::new(&self.config.cfg_dir).join("classes.json"));
                data.game_data.load_color_palette(&std::path::Path::new(&self.config.cfg_dir).join("colors.json"));
                data.game_data.load_spawn_colors(&std::path::Path::new(&self.config.cfg_dir).join("spawn_colors.json"));
                data.filter_dir = self.config.filter_dir.clone();
                data.filters_global = crate::filters::FilterSet::load(
                    &std::path::Path::new(&self.config.filter_dir).join("filters_global.xml"),
                );
                data.recompute_filters();
            }
            let _ = self.config.save(&self.config_path);
        }
        self.about.show(&ctx);
        self.help.show(&ctx);

        // Ctrl+F opens spawn search; F1 opens help
        if ctx.input(|i| i.key_pressed(egui::Key::F) && i.modifiers.ctrl) {
            self.search.open();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.help.open = true;
        }

        // Global keyboard shortcuts
        let (zoom_in, zoom_out, center_player, f5, f6, f7, toggle_trails) = ctx.input(|i| (
            (i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals)) && !i.modifiers.any(),
            i.key_pressed(egui::Key::Minus) && !i.modifiers.any(),
            i.key_pressed(egui::Key::Home),
            i.key_pressed(egui::Key::F5),
            i.key_pressed(egui::Key::F6),
            i.key_pressed(egui::Key::F7),
            i.key_pressed(egui::Key::T) && !i.modifiers.any(),
        ));
        if zoom_in {
            self.map_pane.state.zoom = (self.map_pane.state.zoom * crate::map_canvas::ZOOM_STEP)
                .clamp(crate::map_canvas::ZOOM_MIN, crate::map_canvas::ZOOM_MAX);
        }
        if zoom_out {
            self.map_pane.state.zoom = (self.map_pane.state.zoom / crate::map_canvas::ZOOM_STEP)
                .clamp(crate::map_canvas::ZOOM_MIN, crate::map_canvas::ZOOM_MAX);
        }
        if center_player {
            self.map_pane.state.pan = egui::Vec2::ZERO;
        }
        if f5 { self.toggle_panel(Tab::Spawns); }
        if f6 { self.toggle_panel(Tab::Timers); }
        if f7 { self.toggle_panel(Tab::Ground); }
        if toggle_trails {
            let mut data = self.data.lock().unwrap();
            data.trails_enabled = !data.trails_enabled;
            self.options.trails_enabled = data.trails_enabled;
        }

        // Search dialog — runs outside the DockArea lock so it can mutate AppData directly
        {
            let mut data = self.data.lock().unwrap();
            if let Some(spawn_id) = self.search.show(&ctx, &mut data)
                && let Some(s) = data.spawns.get(spawn_id) {
                    let (mx, my) = crate::map_canvas::eq_to_map_pub(s.x, s.y);
                    self.map_pane.state.pending_center = Some((mx, my));
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
                                .add(egui::Button::selectable(
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

        // "Add to Filter — scope" dialog
        if self.add_filter_scope.open && let Some(category) = self.add_filter_scope.category {
            let zone_name = self.data.lock().unwrap().zone_name.clone();
            let mut chosen: Option<bool> = None; // true = global, false = zone
            let mut cancel = false;
            egui::Window::new("Add to Filter")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(&ctx, |ui| {
                    ui.label("Filter name:");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.add_filter_scope.name)
                            .min_size(egui::vec2(220.0, 0.0))
                            .hint_text("Enter filter text"),
                    );
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        chosen = Some(false); // Enter defaults to Zone
                    }
                    if !self.add_filter_scope.focus_requested {
                        response.request_focus();
                        self.add_filter_scope.focus_requested = true;
                    }
                    ui.add_space(4.0);
                    ui.label("Add to which filter scope?");
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        let name_empty = self.add_filter_scope.name.trim().is_empty();
                        ui.add_enabled_ui(!name_empty, |ui| {
                            if ui.button("Global").clicked() {
                                chosen = Some(true);
                            }
                            if ui.button(format!("Zone: {zone_name}")).clicked() {
                                chosen = Some(false);
                            }
                        });
                        if ui.button("Cancel").clicked() {
                            cancel = true;
                        }
                    });
                });
            if cancel {
                self.add_filter_scope.open = false;
            }
            if let Some(is_global) = chosen {
                let name = self.add_filter_scope.name.trim().to_string();
                self.add_filter_scope.open = false;
                if !name.is_empty() {
                    if is_global {
                        self.write_filter_global(name, category);
                    } else {
                        self.write_filter_zone(name, category);
                    }
                }
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
                let mut spawns_visible = !self.hidden_panels.contains(&Tab::Spawns);
                if ui.checkbox(&mut spawns_visible, "Spawns  F5").clicked() {
                    self.toggle_panel(Tab::Spawns);
                    ui.close();
                }
                let mut timers_visible = !self.hidden_panels.contains(&Tab::Timers);
                if ui.checkbox(&mut timers_visible, "Timers  F6").clicked() {
                    self.toggle_panel(Tab::Timers);
                    ui.close();
                }
                let mut ground_visible = !self.hidden_panels.contains(&Tab::Ground);
                if ui.checkbox(&mut ground_visible, "Ground Items  F7").clicked() {
                    self.toggle_panel(Tab::Ground);
                    ui.close();
                }
            });
            ui.menu_button("Map", |ui| {
                if ui.button("Center on Player  Home").clicked() {
                    self.map_pane.state.pan = egui::Vec2::ZERO;
                    ui.close();
                }
                if ui.button("Zoom In  +").clicked() {
                    self.map_pane.state.zoom = (self.map_pane.state.zoom * crate::map_canvas::ZOOM_STEP)
                        .clamp(crate::map_canvas::ZOOM_MIN, crate::map_canvas::ZOOM_MAX);
                    ui.close();
                }
                if ui.button("Zoom Out  −").clicked() {
                    self.map_pane.state.zoom = (self.map_pane.state.zoom / crate::map_canvas::ZOOM_STEP)
                        .clamp(crate::map_canvas::ZOOM_MIN, crate::map_canvas::ZOOM_MAX);
                    ui.close();
                }
                ui.separator();
                let mut trails_on = self.data.lock().unwrap().trails_enabled;
                if ui.checkbox(&mut trails_on, "Mob Trails  T").clicked() {
                    let mut data = self.data.lock().unwrap();
                    data.trails_enabled = !data.trails_enabled;
                    self.options.trails_enabled = data.trails_enabled;
                    ui.close();
                }
                ui.separator();
                ui.menu_button("Show", |ui| {
                    if ui.checkbox(&mut self.config.map_overlay.show_npcs, "NPCs").clicked() {
                        self.save_config();
                    }
                    if ui.checkbox(&mut self.config.map_overlay.show_players, "Players").clicked() {
                        self.save_config();
                    }
                    if ui.checkbox(&mut self.config.map_overlay.show_corpses, "Corpses").clicked() {
                        self.save_config();
                    }
                    if ui.checkbox(&mut self.config.map_overlay.show_pets, "Pets / Mercs").clicked() {
                        self.save_config();
                    }
                    ui.separator();
                    if ui.checkbox(&mut self.config.map_overlay.show_npc_names, "NPC Names").clicked() {
                        self.save_config();
                    }
                    if ui.checkbox(&mut self.config.map_overlay.show_npc_levels, "NPC Levels").clicked() {
                        self.save_config();
                    }
                    if ui.checkbox(&mut self.config.map_overlay.show_player_names, "Player Names").clicked() {
                        self.save_config();
                    }
                    ui.separator();
                    if ui.checkbox(&mut self.config.map_overlay.show_zone_text, "Zone Text").clicked() {
                        self.save_config();
                    }
                    if ui.checkbox(&mut self.config.map_overlay.show_layer1, "Layer 1").clicked() {
                        self.save_config();
                    }
                    if ui.checkbox(&mut self.config.map_overlay.show_layer2, "Layer 2").clicked() {
                        self.save_config();
                    }
                    if ui.checkbox(&mut self.config.map_overlay.show_layer3, "Layer 3").clicked() {
                        self.save_config();
                    }
                });
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Help  F1").clicked() {
                    self.help.open = true;
                    ui.close();
                }
                ui.separator();
                if ui.button("About…").clicked() {
                    self.about.open = true;
                    ui.close();
                }
            });
        });

        // Toolbar — one-click access to common actions
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            let connected = self.server_addr.lock().unwrap().is_some();
            if connected {
                let img = egui::Image::new(egui::include_image!("../../assets/connected.png"))
                    .fit_to_exact_size(egui::vec2(24.0, 24.0));
                if ui.add(egui::Button::image(img)).on_hover_text("Disconnect").clicked() {
                    *self.server_addr.lock().unwrap() = None;
                }
            } else {
                let img = egui::Image::new(egui::include_image!("../../assets/disconnected.png"))
                    .fit_to_exact_size(egui::vec2(24.0, 24.0));
                let tooltip = format!("Connect to {}:{}", self.config.server_ip, self.config.server_port);
                if ui.add(egui::Button::image(img)).on_hover_text(tooltip).clicked()
                    && let Ok(addr) = format!("{}:{}", self.config.server_ip, self.config.server_port).parse() {
                    *self.server_addr.lock().unwrap() = Some(addr);
                }
            }
            ui.separator();
            let find_img = egui::Image::new(egui::include_image!("../../assets/find.png"))
                .fit_to_exact_size(egui::vec2(24.0, 24.0));
            if ui.add(egui::Button::image(find_img)).on_hover_text("Find Spawn").clicked() {
                self.search.open();
            }
            let trails_on = self.data.lock().unwrap().trails_enabled;
            let trail_img = egui::Image::new(egui::include_image!("../../assets/trail.png"))
                .fit_to_exact_size(egui::vec2(24.0, 24.0));
            if ui.add(egui::Button::image(trail_img).selected(trails_on)).on_hover_text("Mob Trails").clicked() {
                let mut data = self.data.lock().unwrap();
                data.trails_enabled = !data.trails_enabled;
                self.options.trails_enabled = data.trails_enabled;
            }
            let tool_img = egui::Image::new(egui::include_image!("../../assets/tool.png"))
                .fit_to_exact_size(egui::vec2(24.0, 24.0));
            if ui.add(egui::Button::image(tool_img)).on_hover_text("Options").clicked() {
                let trails = self.data.lock().unwrap().trails_enabled;
                self.options.sync_from(&self.config, trails);
                self.options.open = true;
            }
        });
        ui.separator();

        // Docked panel layout
        let mut viewer = WinSeqTabViewer {
            data: &self.data,
            map_pane: &mut self.map_pane,
            overlay: &self.config.map_overlay,
            spawn_sort_column: &mut self.spawn_sort_column,
            spawn_sort_ascending: &mut self.spawn_sort_ascending,
            timer_sort_column: &mut self.timer_sort_column,
            timer_sort_ascending: &mut self.timer_sort_ascending,
            ground_sort_column: &mut self.ground_sort_column,
            ground_sort_ascending: &mut self.ground_sort_ascending,
            spawn_action: &mut self.pending_spawn_action,
            map_action: &mut self.pending_map_action,
            pending_clear_timers: &mut self.pending_clear_timers,
        };
        DockArea::new(&mut self.dock_state).show_inside(ui, &mut viewer);

        // Handle any spawn context menu action from this frame
        if let Some(action) = self.pending_spawn_action.take() {
            self.handle_spawn_action(action);
        }

        // Handle "Clear all timers" — delete both the timer and obs files so they don't reload
        if std::mem::take(&mut self.pending_clear_timers) && !self.prev_zone.is_empty() {
            let dir = std::path::Path::new(&self.config.timer_dir);
            let _ = std::fs::remove_file(dir.join(format!("spawns-{}.txt", self.prev_zone)));
            let _ = std::fs::remove_file(dir.join(format!("obs-{}.txt", self.prev_zone)));
        }

        // Periodic auto-save of timers when auto-promotion has dirtied them
        {
            let mut data = self.data.lock().unwrap();
            if data.timers_dirty
                && self.last_timer_autosave.elapsed() >= Duration::from_secs(60)
                && !self.prev_zone.is_empty()
            {
                let _ = data.timers.save(&self.prev_zone, &self.config.timer_dir);
                data.timers_dirty = false;
                self.last_timer_autosave = std::time::Instant::now();
            }
        }

        // Handle any map context menu action from this frame
        if let Some(action) = self.pending_map_action.take() {
            match action {
                MapAction::AddNoteAt { eq_x, eq_y } => {
                    let eq_z = self.data.lock().unwrap().player_pos().map(|(_, _, z)| z).unwrap_or(0.0);
                    self.add_note.open = true;
                    self.add_note.text.clear();
                    self.add_note.override_pos = Some((eq_x, eq_y, eq_z));
                }
            }
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
                                    d.curr_tick_npc_ids.clear();
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
                                    d.on_tick_end();
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