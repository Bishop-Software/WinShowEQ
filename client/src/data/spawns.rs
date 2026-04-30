use std::collections::HashMap;

use common::SpawnRecord;

/// Con color assigned to a mob based on level delta relative to the player.
/// Matches the color sequence shown in the C# MySEQ client (SpawnColors.cs).
/// Verify exact thresholds against SpawnColors.cs when implementing C4 rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConColor {
    Gray,
    Green,
    LightBlue,
    Blue,
    White,
    Yellow,
    Red,
}

/// Return the con color for `mob_level` when the player is at `player_level`.
/// Approximates the EQ Live con system. See SpawnColors.cs for the authoritative table.
pub fn con_color(player_level: u8, mob_level: u8) -> ConColor {
    let diff = mob_level as i32 - player_level as i32;
    let gray = gray_gap(player_level) as i32;

    if diff <= -gray {
        ConColor::Gray
    } else if diff <= -4 {
        ConColor::Green
    } else if diff <= -2 {
        ConColor::LightBlue
    } else if diff == -1 {
        ConColor::Blue
    } else if diff == 0 {
        ConColor::White
    } else if diff <= 3 {
        ConColor::Yellow
    } else {
        ConColor::Red
    }
}

/// Gap (in levels) below the player at which a mob cons gray.
/// Thresholds approximate the standard EQ Live scaling.
fn gray_gap(player_level: u8) -> u8 {
    match player_level {
        0..=4 => 3,
        5..=9 => 4,
        10..=18 => 5,
        19..=29 => 6,
        30..=39 => 8,
        40..=49 => 10,
        50..=59 => 12,
        60..=64 => 13,
        _ => 14,
    }
}

/// Data for a single EverQuest spawn, decoded from a SpawnRecord.
/// Filter classification fields (is_hunt, etc.) are added in C5.
#[derive(Debug, Clone)]
pub struct SpawnInfo {
    pub id: u32,
    pub name: String,
    pub last_name: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub heading: f32,
    pub speed: f32,
    pub owner_id: u32,
    pub spawn_type: u8,
    pub class: u8,
    pub race: u32,
    pub level: u8,
    pub hidden: u8,
    pub primary: u32,
    pub offhand: u32,
}

impl SpawnInfo {
    pub fn from_record(rec: &SpawnRecord) -> Self {
        // Copy packed fields to locals to avoid misaligned reads.
        let (id, x, y, z, heading, speed, owner, class, race, level, hidden, primary, offhand, spawn_type) = (
            rec.id, rec.x, rec.y, rec.z, rec.heading, rec.speed,
            rec.owner, rec.class, rec.race, rec.level, rec.hidden,
            rec.primary, rec.offhand, rec.spawn_type,
        );
        Self {
            id,
            name: parse_cstr(&rec.name),
            last_name: parse_cstr(&rec.last_name),
            x,
            y,
            z,
            heading,
            speed,
            owner_id: owner,
            spawn_type,
            class,
            race,
            level,
            hidden,
            primary,
            offhand,
        }
    }

    /// Distance from this spawn to `(ox, oy)` in the XY plane.
    pub fn distance_2d(&self, ox: f32, oy: f32) -> f32 {
        let dx = self.x - ox;
        let dy = self.y - oy;
        (dx * dx + dy * dy).sqrt()
    }
}

/// Active spawn list keyed by spawn id.
#[derive(Debug, Default)]
pub struct SpawnStore {
    spawns: HashMap<u32, SpawnInfo>,
}

impl SpawnStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace the spawn described by `rec`.
    pub fn upsert(&mut self, rec: &SpawnRecord) {
        let info = SpawnInfo::from_record(rec);
        self.spawns.insert(info.id, info);
    }

    pub fn remove(&mut self, id: u32) {
        self.spawns.remove(&id);
    }

    pub fn clear(&mut self) {
        self.spawns.clear();
    }

    pub fn get(&self, id: u32) -> Option<&SpawnInfo> {
        self.spawns.get(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &SpawnInfo> {
        self.spawns.values()
    }

    pub fn len(&self) -> usize {
        self.spawns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spawns.is_empty()
    }
}

fn parse_cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::OPT_SPAWNS;

    fn spawn_at_level(level: u8) -> SpawnRecord {
        let mut rec = SpawnRecord::zeroed();
        rec.level = level;
        rec.flags = OPT_SPAWNS;
        rec
    }

    // ── con color ────────────────────────────────────────────────────────────

    #[test]
    fn same_level_is_white() {
        assert_eq!(con_color(30, 30), ConColor::White);
        assert_eq!(con_color(1, 1), ConColor::White);
        assert_eq!(con_color(60, 60), ConColor::White);
    }

    #[test]
    fn one_below_is_blue() {
        assert_eq!(con_color(30, 29), ConColor::Blue);
    }

    #[test]
    fn two_below_is_light_blue() {
        assert_eq!(con_color(30, 28), ConColor::LightBlue);
        assert_eq!(con_color(30, 27), ConColor::LightBlue);
    }

    #[test]
    fn four_below_is_green() {
        assert_eq!(con_color(30, 26), ConColor::Green);
        assert_eq!(con_color(30, 23), ConColor::Green);
    }

    #[test]
    fn gray_boundary_at_level_30() {
        // gray_gap(30) = 8 → mob at 30-8=22 is gray, mob at 23 is green
        assert_eq!(con_color(30, 22), ConColor::Gray);
        assert_eq!(con_color(30, 23), ConColor::Green);
    }

    #[test]
    fn gray_boundary_at_level_10() {
        // gray_gap(10) = 5 → mob at 10-5=5 is gray, mob at 6 is green
        assert_eq!(con_color(10, 5), ConColor::Gray);
        assert_eq!(con_color(10, 6), ConColor::Green);
    }

    #[test]
    fn yellow_range() {
        assert_eq!(con_color(30, 31), ConColor::Yellow);
        assert_eq!(con_color(30, 33), ConColor::Yellow);
    }

    #[test]
    fn four_above_is_red() {
        assert_eq!(con_color(30, 34), ConColor::Red);
        assert_eq!(con_color(30, 60), ConColor::Red);
    }

    // ── SpawnStore ───────────────────────────────────────────────────────────

    #[test]
    fn upsert_and_retrieve() {
        let mut store = SpawnStore::new();
        let mut rec = spawn_at_level(30);
        rec.id = 100;
        store.upsert(&rec);
        assert_eq!(store.len(), 1);
        assert_eq!(store.get(100).unwrap().level, 30);
    }

    #[test]
    fn upsert_overwrites_existing() {
        let mut store = SpawnStore::new();
        let mut rec = spawn_at_level(30);
        rec.id = 100;
        store.upsert(&rec);
        rec.level = 35;
        store.upsert(&rec);
        assert_eq!(store.get(100).unwrap().level, 35);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn remove_drops_entry() {
        let mut store = SpawnStore::new();
        let mut rec = spawn_at_level(1);
        rec.id = 7;
        store.upsert(&rec);
        store.remove(7);
        assert!(store.is_empty());
    }

    // ── SpawnInfo parsing ────────────────────────────────────────────────────

    #[test]
    fn parses_name_from_record() {
        let mut rec = SpawnRecord::zeroed();
        rec.flags = OPT_SPAWNS;
        let src = b"Fippy";
        rec.name[..src.len()].copy_from_slice(src);
        let info = SpawnInfo::from_record(&rec);
        assert_eq!(info.name, "Fippy");
    }

    #[test]
    fn distance_2d_correct() {
        let mut rec = SpawnRecord::zeroed();
        rec.x = 3.0;
        rec.y = 4.0;
        rec.flags = OPT_SPAWNS;
        let info = SpawnInfo::from_record(&rec);
        assert!((info.distance_2d(0.0, 0.0) - 5.0).abs() < 0.001);
    }
}