pub mod annotations;
pub mod ground;
pub mod spawns;
pub mod timers;
pub mod world;

use std::collections::{HashMap, HashSet, VecDeque};

use crate::alerts::AlertEngine;
use crate::filters::FilterSet;
use crate::game_data::GameData;
use crate::map_reader::MapData;
use crate::protocol::Packet;
use annotations::AnnotationStore;
use ground::GroundStore;
use spawns::{SpawnInfo, SpawnStore};
use timers::{SpawnObserver, TimerStore};
use world::InGameTime;

const MAX_TRAIL_LEN: usize = 25;

/// Central in-memory state for the client, updated each tick.
pub struct AppData {
    pub spawns: SpawnStore,
    pub ground: GroundStore,
    pub timers: TimerStore,
    pub map: MapData,
    pub world_time: InGameTime,
    pub zone_name: String,
    pub target_id: Option<u32>,
    pub self_id: Option<u32>,
    /// Global filters loaded from `global.xml` — applies in every zone.
    pub filters_global: FilterSet,
    /// Zone-specific filters loaded from `{zone}.xml` — applies in the current zone only.
    pub filters_zone: FilterSet,
    /// Merged result of global + zone filters (computed, not persisted directly).
    pub filters: FilterSet,
    /// Directory where filter XML files are stored; used for zone-filter reloading on zone change.
    pub filter_dir: String,
    /// Mob trail positions (map coords — already X/Y negated). Updated each tick.
    pub trails: HashMap<u32, VecDeque<(f32, f32)>>,
    pub trails_enabled: bool,
    pub alert_engine: AlertEngine,
    pub annotations: AnnotationStore,
    pub game_data: GameData,
    /// Spawn list column widths (in pixels); 15 columns matching spawn_list headers.
    pub spawn_list_column_widths: Vec<f32>,
    /// Timer list column widths (in pixels); 3 columns: Name, Loc, Countdown
    pub timer_list_column_widths: Vec<f32>,
    /// Ground list column widths (in pixels); 4 columns: Item, X, Y, Z
    pub ground_list_column_widths: Vec<f32>,
    /// Spawn IDs currently highlighted by the search dialog.
    pub marked_ids: HashSet<u32>,
    /// Single spawn selected by clicking a row in the spawn list or a dot on the map canvas.
    pub selected_id: Option<u32>,
    /// When true, the spawn list should scroll to reveal `selected_id` on the next frame.
    pub scroll_to_selected: bool,
    /// Timer selected by clicking a row in the timer list or a crosshair on the map canvas.
    pub selected_timer_loc: Option<String>,
    /// When true, the timer list should scroll to reveal `selected_timer_loc` on the next frame.
    pub scroll_to_selected_timer: bool,
    /// Auto-learning respawn timer engine.
    pub observer: SpawnObserver,
    /// NPC spawn IDs seen in the current tick (scratch space for observer diff).
    pub curr_tick_npc_ids: HashSet<u32>,
    /// All spawn IDs (any type) seen in the current tick — used to prune stale spawns.
    pub curr_tick_all_ids: HashSet<u32>,
    /// Set when an auto-timer is promoted; cleared after periodic save.
    pub timers_dirty: bool,
    /// Set whenever the spawn list changes; cleared by the UI after rebuilding filter options.
    pub spawns_dirty: bool,
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            spawns: SpawnStore::default(),
            ground: GroundStore::default(),
            timers: TimerStore::default(),
            map: MapData::default(),
            world_time: InGameTime::default(),
            zone_name: String::new(),
            target_id: None,
            self_id: None,
            filters_global: FilterSet::default(),
            filters_zone: FilterSet::default(),
            filters: FilterSet::default(),
            filter_dir: String::new(),
            trails: HashMap::new(),
            trails_enabled: false,
            alert_engine: AlertEngine::default(),
            annotations: AnnotationStore::default(),
            game_data: GameData::default(),
            spawn_list_column_widths: vec![
                90.0, // Name
                70.0, // Last Name
                28.0, // Lvl
                36.0, // Class
                70.0, // Race
                36.0, // Type
                70.0, // Owner
                28.0, // Invis
                42.0, // Speed
                58.0, // X
                58.0, // Y
                58.0, // Z
                42.0, // Dist
                40.0, // ID
                62.0, // Time
            ],
            timer_list_column_widths: vec![
                120.0, // Name
                80.0,  // Remain
                65.0,  // Interval
                80.0,  // Zone
                55.0,  // X
                55.0,  // Y
                55.0,  // Z
                45.0,  // Count
                110.0, // Spawn Time
                110.0, // Kill Time
            ],
            ground_list_column_widths: vec![
                150.0, // Item
                60.0,  // X
                60.0,  // Y
                60.0,  // Z
            ],
            marked_ids: HashSet::new(),
            selected_id: None,
            scroll_to_selected: false,
            selected_timer_loc: None,
            scroll_to_selected_timer: false,
            observer: SpawnObserver::default(),
            curr_tick_npc_ids: HashSet::new(),
            curr_tick_all_ids: HashSet::new(),
            timers_dirty: false,
            spawns_dirty: false,
        }
    }
}

impl AppData {
    /// Level of the player character; 1 if not yet known.
    pub fn self_level(&self) -> u8 {
        self.self_id
            .and_then(|id| self.spawns.get(id))
            .map(|s| s.level)
            .unwrap_or(1)
    }

    /// Player world position (EQ coords), or None.
    pub fn player_pos(&self) -> Option<(f32, f32, f32)> {
        self.self_id
            .and_then(|id| self.spawns.get(id))
            .map(|s| (s.x, s.y, s.z))
    }

    /// Recompute `filters` as global merged with zone, then reclassify all spawns.
    pub fn recompute_filters(&mut self) {
        let mut merged = self.filters_global.clone();
        merged.merge(self.filters_zone.clone());
        self.filters = merged;
        let filters = self.filters.clone();
        self.spawns.reclassify_all(&filters);
    }

    /// Run the spawn diff after each network tick to detect kills/respawns.
    /// Returns log messages for the caller to write (kill detections, respawns, promotions).
    pub fn on_tick_end(&mut self) -> Vec<String> {
        let curr_ids = self.curr_tick_npc_ids.clone();
        let (promoted, log) =
            self.observer
                .process_diff(&curr_ids, &self.spawns, &self.zone_name, &mut self.timers);
        if promoted {
            self.timers_dirty = true;
        }

        // Remove spawns that the server didn't send this tick (despawned/decayed).
        // Skip pruning if the tick was empty — the server may have sent nothing due to
        // a partial response or the player not being in a zone yet.
        if !self.curr_tick_all_ids.is_empty() {
            let stale: Vec<u32> = self
                .spawns
                .iter()
                .map(|s| s.id)
                .filter(|id| !self.curr_tick_all_ids.contains(id))
                .collect();
            for id in stale {
                self.spawns.remove(id);
                self.trails.remove(&id);
                self.marked_ids.remove(&id);
                if self.selected_id == Some(id) {
                    self.selected_id = None;
                }
                if self.target_id == Some(id) {
                    self.target_id = None;
                }
            }
        }
        self.curr_tick_all_ids.clear();
        self.spawns_dirty = true;
        log
    }

    /// Load `{zone}.xml` from `filter_dir`, recompute the merged filter set.
    pub fn reload_zone_filter(&mut self, zone: &str) {
        let path =
            std::path::Path::new(&self.filter_dir).join(format!("{}.xml", zone.to_lowercase()));
        self.filters_zone = FilterSet::load(&path);
        self.recompute_filters();
    }
}

/// Apply a decoded packet to the shared app state.
/// Returns an optional log message (alerts) for the caller to record.
pub fn apply_packet(data: &mut AppData, packet: Packet) -> Option<String> {
    match packet {
        Packet::Zone { name } => {
            data.spawns.clear();
            data.ground.clear();
            data.trails.clear();
            data.self_id = None;
            data.target_id = None;
            data.curr_tick_npc_ids.clear();
            data.curr_tick_all_ids.clear();
            data.spawns_dirty = true;
            data.selected_id = None;
            data.selected_timer_loc = None;
            data.observer.on_zone_change();
            data.alert_engine.on_zone_change();
            if !data.filter_dir.is_empty() {
                data.reload_zone_filter(&name);
            }
            data.zone_name = name;
        }
        Packet::Spawn(rec) => {
            let id = rec.id;
            if data.trails_enabled {
                let (new_x, new_y) = (rec.y, rec.x); // match SpawnInfo.x=east-west, .y=north-south
                let trail_pt = data
                    .spawns
                    .get(id)
                    .filter(|s| (s.x - new_x).abs() > 0.5 || (s.y - new_y).abs() > 0.5)
                    .map(|s| (-s.x, s.y));
                if let Some(pt) = trail_pt {
                    let trail = data.trails.entry(id).or_default();
                    trail.push_back(pt);
                    while trail.len() > MAX_TRAIL_LEN {
                        trail.pop_front();
                    }
                }
            }
            let mut info = SpawnInfo::from_record(&rec);
            info.apply_filters(&data.filters);
            let log_msg = data.alert_engine.check_spawn(&info);
            if info.spawn_category == spawns::SpawnCategory::Npc {
                data.curr_tick_npc_ids.insert(id);
            }
            data.curr_tick_all_ids.insert(id);
            data.spawns.upsert_info(info);
            return log_msg;
        }
        Packet::Self_(rec) => {
            data.self_id = Some(rec.id);
            data.curr_tick_all_ids.insert(rec.id);
            let (spawns, filters) = (&mut data.spawns, &data.filters);
            spawns.upsert_with_filter(&rec, filters);
        }
        Packet::Target(rec) => {
            data.target_id = Some(rec.id);
        }
        Packet::Ground(rec) => data.ground.push(&rec),
        Packet::World(t) => data.world_time = t,
        Packet::Process { .. } | Packet::Unknown { .. } => {}
    }
    None
}

/// Apply alert config from `ClientConfig` to the alert engine in `data`.
pub fn configure_alerts(data: &mut AppData, cfg: &crate::config::ClientConfig) {
    use crate::alerts::AlertMode;
    data.alert_engine.danger_mode =
        AlertMode::from_config(&cfg.alert_danger_mode, &cfg.alert_danger_sound);
    data.alert_engine.caution_mode =
        AlertMode::from_config(&cfg.alert_caution_mode, &cfg.alert_caution_sound);
    data.alert_engine.hunt_mode =
        AlertMode::from_config(&cfg.alert_hunt_mode, &cfg.alert_hunt_sound);
    data.alert_engine.rare_mode =
        AlertMode::from_config(&cfg.alert_rare_mode, &cfg.alert_rare_sound);
    data.alert_engine.discord_webhook = cfg.discord_webhook.clone();
    data.alert_engine.discord_on_danger = cfg.discord_on_danger;
    data.alert_engine.discord_on_hunt = cfg.discord_on_hunt;
}
