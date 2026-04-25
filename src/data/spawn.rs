use std::mem::size_of;

/// EQ spawn type stored in SpawnRecord::spawn_type.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnType {
    Npc = 0,
    Pc = 1,
    Corpse = 2,
}

/// Wire-format spawn record sent to the C# client.
/// Layout must match netBuffer_t (#pragma pack(1)) in Spawn.h exactly.
/// Items and world time are also packed into this struct (see Spawn::packNetBufferFrom /
/// packNetBufferWorld in Spawn.cpp) — the flags field identifies the record type using
/// the OPT_* constants in NetworkServer.h.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct SpawnRecord {
    pub name: [u8; 30],
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub heading: f32,
    pub speed: f32,
    pub id: u32,
    pub owner: u32,
    pub spawn_type: u8,
    pub class: u8,
    pub race: u32,
    pub level: u8,
    pub hidden: u8,
    pub primary: u32,
    pub offhand: u32,
    pub last_name: [u8; 22],
    pub flags: u32,
}

impl SpawnRecord {
    pub fn zeroed() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_record_wire_size() {
        // netBuffer_t under #pragma pack(1) is 100 bytes.
        // The RUST_MIGRATION_PLAN.md states 116 — that figure is incorrect;
        // the authoritative struct definition in Spawn.h gives 100.
        assert_eq!(size_of::<SpawnRecord>(), 100);
    }
}