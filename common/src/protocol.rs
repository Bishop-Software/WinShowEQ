use std::mem::size_of;

// Request bitmask flags sent by the client (4-byte LE i32).
pub const IPT_ZONE: i32 = 0x01;
pub const IPT_SELF: i32 = 0x02;
pub const IPT_TARGET: i32 = 0x04;
pub const IPT_SPAWNS: i32 = 0x08;
pub const IPT_GROUND: i32 = 0x10;
pub const IPT_GETPROC: i32 = 0x20;
pub const IPT_SETPROC: i32 = 0x40;
pub const IPT_WORLD: i32 = 0x80;

// Outgoing packet type stored in SpawnRecord::flags (mirrors OPT_* in NetworkServer.h).
pub const OPT_SPAWNS: u32 = 0x00;
pub const OPT_TARGET: u32 = 0x01;
pub const OPT_ZONE: u32 = 0x04;
pub const OPT_GROUND: u32 = 0x05;
pub const OPT_PROCESS: u32 = 0x06;
pub const OPT_WORLD: u32 = 0x08;
pub const OPT_SELF: u32 = 0xFD;

/// EQ spawn category stored in SpawnRecord::spawn_type.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnType {
    Pc = 0,
    Npc = 1,
    Corpse = 2,
}

/// Wire-format record sent between server and client.
/// Layout matches netBuffer_t (#pragma pack(1)) in Spawn.h exactly.
/// Items and world time are also encoded here; the flags field carries the OPT_* type.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
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

    /// Returns the packed wire bytes for this record (100 bytes, no padding).
    pub fn as_bytes(&self) -> &[u8] {
        // SAFETY: SpawnRecord is #[repr(C, packed)] with alignment 1 and a
        // statically asserted size of 100. Every byte is initialized by zeroed().
        unsafe { std::slice::from_raw_parts(self as *const Self as *const u8, size_of::<Self>()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_record_wire_size() {
        assert_eq!(size_of::<SpawnRecord>(), 100);
    }
}
