use common::{
    SpawnRecord, OPT_GROUND, OPT_PROCESS, OPT_SELF, OPT_SPAWNS, OPT_TARGET, OPT_WORLD, OPT_ZONE,
};

use crate::data::world::InGameTime;

/// Decoded form of a wire SpawnRecord, dispatched by the `flags` field.
#[derive(Debug)]
#[allow(dead_code)]
pub enum Packet {
    /// Regular NPC or PC spawn update.
    Spawn(SpawnRecord),
    /// Player's own character.
    Self_(SpawnRecord),
    /// Current target.
    Target(SpawnRecord),
    /// Zone change — name in the `name` field.
    Zone { name: String },
    /// Ground item.
    Ground(SpawnRecord),
    /// In-game Norrath time.
    World(InGameTime),
    /// EQ process PID returned by IPT_GETPROC.
    Process { pid: u32 },
    /// Unrecognised packet type.
    Unknown { flags: u32 },
}

pub fn decode_packet(rec: SpawnRecord) -> Packet {
    let flags = { rec.flags };
    match flags {
        OPT_SPAWNS => Packet::Spawn(rec),
        OPT_SELF => Packet::Self_(rec),
        OPT_TARGET => Packet::Target(rec),
        OPT_ZONE => Packet::Zone { name: name_from_bytes(&rec.name) },
        OPT_GROUND => Packet::Ground(rec),
        OPT_WORLD => Packet::World(InGameTime::from_record(&rec).unwrap_or_default()),
        OPT_PROCESS => Packet::Process { pid: { rec.id } },
        _ => Packet::Unknown { flags },
    }
}

/// Extract a null-terminated string from a 30-byte name field.
pub fn name_from_bytes(bytes: &[u8; 30]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Extract a null-terminated string from a 22-byte last_name field.
#[allow(dead_code)]
pub fn last_name_from_bytes(bytes: &[u8; 22]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{OPT_SPAWNS, OPT_SELF, OPT_TARGET, OPT_ZONE, OPT_GROUND, OPT_WORLD, OPT_PROCESS};

    fn rec_with_flags(flags: u32) -> SpawnRecord {
        let mut r = SpawnRecord::zeroed();
        r.flags = flags;
        r
    }

    #[test]
    fn decode_spawn() {
        let mut r = rec_with_flags(OPT_SPAWNS);
        r.id = 10;
        assert!(matches!(decode_packet(r), Packet::Spawn(_)));
    }

    #[test]
    fn decode_self() {
        assert!(matches!(decode_packet(rec_with_flags(OPT_SELF)), Packet::Self_(_)));
    }

    #[test]
    fn decode_target() {
        assert!(matches!(decode_packet(rec_with_flags(OPT_TARGET)), Packet::Target(_)));
    }

    #[test]
    fn decode_zone_extracts_name() {
        let mut r = rec_with_flags(OPT_ZONE);
        r.name[..6].copy_from_slice(b"qeynos");
        // null terminator at index 6 (rest are zero from zeroed())
        match decode_packet(r) {
            Packet::Zone { name } => assert_eq!(name, "qeynos"),
            _ => panic!("expected Zone"),
        }
    }

    #[test]
    fn decode_zone_full_field_no_null() {
        let mut r = rec_with_flags(OPT_ZONE);
        r.name = [b'a'; 30];
        match decode_packet(r) {
            Packet::Zone { name } => assert_eq!(name.len(), 30),
            _ => panic!("expected Zone"),
        }
    }

    #[test]
    fn decode_ground() {
        assert!(matches!(decode_packet(rec_with_flags(OPT_GROUND)), Packet::Ground(_)));
    }

    #[test]
    fn decode_world_maps_fields() {
        let mut r = rec_with_flags(OPT_WORLD);
        r.spawn_type = 14; // hour
        r.class = 30;      // minute
        r.level = 15;      // day
        r.hidden = 6;      // month
        r.race = 3245;     // year
        match decode_packet(r) {
            Packet::World(t) => {
                assert_eq!(t.hour, 14);
                assert_eq!(t.minute, 30);
                assert_eq!(t.year, 3245);
            }
            _ => panic!("expected World"),
        }
    }

    #[test]
    fn decode_process() {
        let mut r = rec_with_flags(OPT_PROCESS);
        r.id = 12345;
        assert!(matches!(decode_packet(r), Packet::Process { pid: 12345 }));
    }

    #[test]
    fn decode_unknown() {
        assert!(matches!(decode_packet(rec_with_flags(0x99)), Packet::Unknown { flags: 0x99 }));
    }

    #[test]
    fn name_from_bytes_null_terminated() {
        let mut b = [0u8; 30];
        b[..4].copy_from_slice(b"Test");
        assert_eq!(name_from_bytes(&b), "Test");
    }

    #[test]
    fn name_from_bytes_no_null() {
        let b = [b'x'; 30];
        assert_eq!(name_from_bytes(&b).len(), 30);
    }
}