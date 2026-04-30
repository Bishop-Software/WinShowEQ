pub mod ground;
pub mod spawns;
pub mod world;

use crate::map_reader::MapData;
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