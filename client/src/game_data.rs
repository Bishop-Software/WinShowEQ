use std::collections::HashMap;
use std::path::Path;

/// EQ string database parsed from dbstr_us.txt.
/// Race entries follow the pattern: `race_id^11^RaceName^0^`
pub struct GameData {
    races: HashMap<u32, String>,
}

/// Common EQ install locations to probe when no path is configured.
static EQ_CANDIDATE_DIRS: &[&str] = &[
    r"C:\Users\Public\Daybreak Game Company\Installed Games\EverQuest",
    r"C:\Users\Public\Sony Online Entertainment\Installed Games\EverQuest",
    r"C:\Program Files (x86)\Sony Online Entertainment\EverQuest",
    r"C:\Program Files\EverQuest",
];

impl GameData {
    /// Load from an EQ installation directory.
    /// If `eq_dir` is empty or the file is missing there, tries common install locations.
    /// Returns `None` if `dbstr_us.txt` cannot be found anywhere.
    pub fn load(eq_dir: &str) -> Option<Self> {
        // If the user pointed at eqgame.exe rather than the directory, use its parent.
        let resolved_dir;
        let eq_dir = if !eq_dir.is_empty() {
            let p = Path::new(eq_dir);
            if p.is_file() {
                resolved_dir = p.parent()?.to_string_lossy().into_owned();
                resolved_dir.as_str()
            } else {
                eq_dir
            }
        } else {
            eq_dir
        };

        let candidates: Vec<&str> = if eq_dir.is_empty() {
            EQ_CANDIDATE_DIRS.to_vec()
        } else {
            std::iter::once(eq_dir)
                .chain(EQ_CANDIDATE_DIRS.iter().copied())
                .collect()
        };

        for dir in candidates {
            let path = Path::new(dir).join("dbstr_us.txt");
            if let Ok(content) = std::fs::read_to_string(&path) {
                return Some(Self::parse(&content));
            }
        }
        None
    }

    fn parse(content: &str) -> Self {
        let mut races = HashMap::new();
        for line in content.lines() {
            // Format: race_id^type^text  (type 11 = singular race name)
            let mut parts = line.splitn(4, '^');
            let Some(race_id_str) = parts.next() else { continue };
            let Some(type_str) = parts.next() else { continue };
            let Some(text) = parts.next() else { continue };
            if type_str != "11" {
                continue;
            }
            let Ok(race_id) = race_id_str.trim().parse::<u32>() else { continue };
            if !text.is_empty() {
                races.insert(race_id, text.to_owned());
            }
        }
        Self { races }
    }

    pub fn race_name(&self, id: u32) -> &str {
        self.races
            .get(&id)
            .map(|s| s.as_str())
            .unwrap_or(FALLBACK_RACES.get(id as usize).copied().unwrap_or("---"))
    }
}

impl Default for GameData {
    fn default() -> Self {
        Self { races: HashMap::new() }
    }
}

/// Minimal fallback table for when dbstr_us.txt is not available.
/// Covers playable PC races only (indices match spawn race IDs).
static FALLBACK_RACES: &[&str] = &[
    "Unknown",    // 0
    "Human",      // 1
    "Barbarian",  // 2
    "Erudite",    // 3
    "Wood Elf",   // 4
    "High Elf",   // 5
    "Dark Elf",   // 6
    "Half Elf",   // 7
    "Dwarf",      // 8
    "Troll",      // 9
    "Ogre",       // 10
    "Halfling",   // 11
    "Gnome",      // 12
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_race_entries() {
        let input = "1^11^Human^0^\n2^11^Barbarian^0^\n3^8^Some lore text^0^\n3^11^Erudite^0^\n";
        let gd = GameData::parse(input);
        assert_eq!(gd.race_name(1), "Human");
        assert_eq!(gd.race_name(2), "Barbarian");
        assert_eq!(gd.race_name(3), "Erudite");
    }

    #[test]
    fn skips_non_11_entries() {
        let input = "5^8^Some lore^0^\n5^12^Plural^0^\n";
        let gd = GameData::parse(input);
        // race 5 not in map → falls back to FALLBACK_RACES[5] = "High Elf"
        assert_eq!(gd.race_name(5), "High Elf");
    }

    #[test]
    fn fallback_for_unknown_npc_race() {
        let gd = GameData::default();
        // race 999 not in fallback table
        assert_eq!(gd.race_name(999), "---");
    }
}