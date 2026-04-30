pub mod ground;
pub mod spawns;
pub mod timers;
pub mod world;

use std::collections::{HashMap, VecDeque};

use crate::filters::FilterSet;
use crate::map_reader::MapData;
use crate::protocol::Packet;
use ground::GroundStore;
use spawns::SpawnStore;
use timers::TimerStore;
use world::InGameTime;

const MAX_TRAIL_LEN: usize = 25;

/// Central in-memory state for the client, updated each tick.
#[derive(Default)]
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
pub fn apply_packet(data: &mut AppData, packet: Packet) {
    match packet {
        Packet::Zone { name } => {
            data.zone_name = name;
            data.spawns.clear();
            data.ground.clear();
            data.trails.clear();
            data.self_id = None;
            data.target_id = None;
        }
        Packet::Spawn(rec) => {
            let id = rec.id;
            let (new_x, new_y) = (rec.x, rec.y);
            if data.trails_enabled {
                if let Some(existing) = data.spawns.get(id) {
                    if (existing.x - new_x).abs() > 0.5 || (existing.y - new_y).abs() > 0.5 {
                        let trail = data.trails.entry(id).or_default();
                        trail.push_back((-existing.x, -existing.y));
                        while trail.len() > MAX_TRAIL_LEN {
                            trail.pop_front();
                        }
                    }
                }
            }
            let (spawns, filters) = (&mut data.spawns, &data.filters);
            spawns.upsert_with_filter(&rec, filters);
        }
        Packet::Self_(rec) => {
            data.self_id = Some(rec.id);
            let (spawns, filters) = (&mut data.spawns, &data.filters);
            spawns.upsert_with_filter(&rec, filters);
        }
        Packet::Target(rec) => {
            data.target_id = Some(rec.id);
        }
        Packet::Ground(rec) => data.ground.upsert(&rec),
        Packet::World(t) => data.world_time = t,
        Packet::Process { .. } | Packet::Unknown { .. } => {}
    }
}