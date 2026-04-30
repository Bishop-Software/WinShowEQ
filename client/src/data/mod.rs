pub mod ground;
pub mod spawns;
pub mod world;

use crate::map_reader::MapData;
use crate::protocol::Packet;
use ground::GroundStore;
use spawns::SpawnStore;
use world::InGameTime;

/// Central in-memory state for the client, updated each tick.
#[derive(Default)]
pub struct AppData {
    pub spawns: SpawnStore,
    pub ground: GroundStore,
    pub map: MapData,
    pub world_time: InGameTime,
    pub zone_name: String,
    pub target_id: Option<u32>,
    pub self_id: Option<u32>,
}

impl AppData {
    /// Level of the player character; 1 if not yet known.
    pub fn self_level(&self) -> u8 {
        self.self_id
            .and_then(|id| self.spawns.get(id))
            .map(|s| s.level)
            .unwrap_or(1)
    }
}

/// Apply a decoded packet to the shared app state.
pub fn apply_packet(data: &mut AppData, packet: Packet) {
    match packet {
        Packet::Zone { name } => {
            data.zone_name = name;
            data.spawns.clear();
            data.ground.clear();
            data.self_id = None;
            data.target_id = None;
        }
        Packet::Spawn(rec) => data.spawns.upsert(&rec),
        Packet::Self_(rec) => {
            let id = rec.id;
            data.self_id = Some(id);
            data.spawns.upsert(&rec);
        }
        Packet::Target(rec) => {
            data.target_id = Some(rec.id);
        }
        Packet::Ground(rec) => data.ground.upsert(&rec),
        Packet::World(t) => data.world_time = t,
        Packet::Process { .. } | Packet::Unknown { .. } => {}
    }
}