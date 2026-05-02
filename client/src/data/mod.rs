pub mod annotations;
pub mod ground;
pub mod spawns;
pub mod timers;
pub mod world;

use std::collections::{HashMap, VecDeque};

use crate::alerts::AlertEngine;
use crate::filters::FilterSet;
use crate::game_data::GameData;
use crate::map_reader::MapData;
use crate::protocol::Packet;
use annotations::AnnotationStore;
use ground::GroundStore;
use spawns::{SpawnInfo, SpawnStore};
use timers::TimerStore;
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
    pub filters: FilterSet,
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
            filters: FilterSet::default(),
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
                80.0,  // Loc
                100.0, // Countdown
            ],
            ground_list_column_widths: vec![
                150.0, // Item
                60.0,  // X
                60.0,  // Y
                60.0,  // Z
            ],
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
}

/// Apply a decoded packet to the shared app state.
/// Returns an optional log message (alerts) for the caller to record.
pub fn apply_packet(data: &mut AppData, packet: Packet) -> Option<String> {
    match packet {
        Packet::Zone { name } => {
            data.zone_name = name;
            data.spawns.clear();
            data.ground.clear();
            data.trails.clear();
            data.self_id = None;
            data.target_id = None;
            data.alert_engine.on_zone_change();
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
            data.spawns.upsert_info(info);
            return log_msg;
        }
        Packet::Self_(rec) => {
            data.self_id = Some(rec.id);
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