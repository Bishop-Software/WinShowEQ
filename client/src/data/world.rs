use common::{OPT_WORLD, SpawnRecord};

/// In-game Norrath time decoded from an OPT_WORLD packet.
/// Field repurposing mirrors Spawn::packNetBufferWorld in C++:
///   spawn_type=hour, class=minute, level=day, hidden=month, race=year.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InGameTime {
    pub hour: u8,
    pub minute: u8,
    pub day: u8,
    pub month: u8,
    pub year: u32,
}

impl InGameTime {
    /// Decode from an OPT_WORLD SpawnRecord. Returns None if the record is
    /// not flagged as OPT_WORLD.
    pub fn from_record(rec: &SpawnRecord) -> Option<Self> {
        let (flags, spawn_type, class, level, hidden, race) = (
            rec.flags,
            rec.spawn_type,
            rec.class,
            rec.level,
            rec.hidden,
            rec.race,
        );
        if flags != OPT_WORLD {
            return None;
        }
        Some(Self {
            hour: spawn_type,
            minute: class,
            day: level,
            month: hidden,
            year: race,
        })
    }

    /// Format as a human-readable string matching the EQ `/time` output style.
    pub fn display(&self) -> String {
        let ampm = if self.hour < 12 { "AM" } else { "PM" };
        let h = match self.hour % 12 {
            0 => 12,
            h => h,
        };
        format!(
            "{}/{}/{} {:02}:{:02} {}",
            self.month, self.day, self.year, h, self.minute, ampm
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_world_record(h: u8, min: u8, d: u8, mo: u8, y: u32) -> SpawnRecord {
        let mut rec = SpawnRecord::zeroed();
        rec.spawn_type = h;
        rec.class = min;
        rec.level = d;
        rec.hidden = mo;
        rec.race = y;
        rec.flags = OPT_WORLD;
        rec
    }

    #[test]
    fn round_trips_world_time() {
        let rec = make_world_record(14, 30, 15, 6, 3245);
        let t = InGameTime::from_record(&rec).unwrap();
        assert_eq!(t.hour, 14);
        assert_eq!(t.minute, 30);
        assert_eq!(t.day, 15);
        assert_eq!(t.month, 6);
        assert_eq!(t.year, 3245);
    }

    #[test]
    fn rejects_non_world_record() {
        let mut rec = SpawnRecord::zeroed();
        rec.flags = common::OPT_SPAWNS;
        assert!(InGameTime::from_record(&rec).is_none());
    }

    #[test]
    fn display_formats_pm_hour() {
        let rec = make_world_record(14, 5, 1, 3, 3100);
        let t = InGameTime::from_record(&rec).unwrap();
        assert_eq!(t.display(), "3/1/3100 02:05 PM");
    }
}