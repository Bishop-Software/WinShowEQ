use std::collections::VecDeque;
use std::fs;
use std::io::Write as _;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};

const MAX_TIMERS: usize = 200;

/// A tracked respawn timer for a named EQ mob.
#[derive(Debug, Clone)]
pub struct SpawnTimer {
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// UTC instant the mob was killed (timer start).
    pub killed_at: DateTime<Utc>,
    /// Expected respawn interval in seconds.
    pub respawn_secs: i64,
}

impl SpawnTimer {
    pub fn new(
        name: impl Into<String>,
        x: f32,
        y: f32,
        z: f32,
        respawn_secs: i64,
    ) -> Self {
        Self {
            name: name.into(),
            x,
            y,
            z,
            killed_at: Utc::now(),
            respawn_secs,
        }
    }

    pub fn next_spawn_at(&self) -> DateTime<Utc> {
        self.killed_at + Duration::seconds(self.respawn_secs)
    }

    /// Seconds remaining until expected respawn. Negative means already past window.
    pub fn secs_remaining(&self) -> i64 {
        (self.next_spawn_at() - Utc::now()).num_seconds()
    }

    pub fn is_spawned(&self) -> bool {
        self.secs_remaining() <= 0
    }

    pub fn countdown_str(&self) -> String {
        let secs = self.secs_remaining();
        if secs <= 0 {
            return "SPAWNED".to_owned();
        }
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        let s = secs % 60;
        if h > 0 {
            format!("{h}:{m:02}:{s:02}")
        } else {
            format!("{m}:{s:02}")
        }
    }
}

/// Timer list for the current zone.
#[derive(Debug, Default)]
pub struct TimerStore {
    pub timers: VecDeque<SpawnTimer>,
}

impl TimerStore {
    pub fn add(&mut self, timer: SpawnTimer) {
        // Replace existing entry for the same name if present.
        if let Some(existing) = self.timers.iter_mut().find(|t| t.name == timer.name) {
            *existing = timer;
            return;
        }
        self.timers.push_back(timer);
        if self.timers.len() > MAX_TIMERS {
            self.timers.pop_front();
        }
    }

    pub fn remove(&mut self, index: usize) {
        self.timers.remove(index);
    }

    pub fn iter(&self) -> impl Iterator<Item = &SpawnTimer> {
        self.timers.iter()
    }

    pub fn len(&self) -> usize {
        self.timers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.timers.is_empty()
    }

    /// Load timers for `zone` from `{dir}/spawns-{zone}.txt`.
    /// Returns an empty store if the file doesn't exist or can't be read.
    pub fn load(zone: &str, dir: &str) -> Self {
        let path = timer_path(zone, dir);
        let mut store = Self::default();
        let Ok(text) = fs::read_to_string(&path) else {
            return store;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(t) = parse_line(line) {
                store.timers.push_back(t);
            }
        }
        store
    }

    /// Save timers for `zone` to `{dir}/spawns-{zone}.txt`.
    pub fn save(&self, zone: &str, dir: &str) -> std::io::Result<()> {
        if self.timers.is_empty() {
            return Ok(());
        }
        let path = timer_path(zone, dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut f = fs::File::create(&path)?;
        for t in &self.timers {
            writeln!(
                f,
                "{};{};{};{};{};{}",
                t.name,
                t.x,
                t.y,
                t.z,
                t.killed_at.timestamp(),
                t.respawn_secs,
            )?;
        }
        Ok(())
    }
}

fn timer_path(zone: &str, dir: &str) -> std::path::PathBuf {
    Path::new(dir).join(format!("spawns-{zone}.txt"))
}

fn parse_line(line: &str) -> Option<SpawnTimer> {
    let mut parts = line.splitn(6, ';');
    let name = parts.next()?.to_owned();
    let x: f32 = parts.next()?.parse().ok()?;
    let y: f32 = parts.next()?.parse().ok()?;
    let z: f32 = parts.next()?.parse().ok()?;
    let killed_unix: i64 = parts.next()?.parse().ok()?;
    let respawn_secs: i64 = parts.next()?.parse().ok()?;
    let killed_at =
        DateTime::from_timestamp(killed_unix, 0).unwrap_or_else(Utc::now);
    Some(SpawnTimer { name, x, y, z, killed_at, respawn_secs })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_str_formats_correctly() {
        let mut t = SpawnTimer::new("Boss", 0.0, 0.0, 0.0, 3661);
        t.killed_at = Utc::now();
        let s = t.countdown_str();
        assert!(s.starts_with('1'), "expected 1:01:01 format, got {s}");
    }

    #[test]
    fn countdown_str_shows_spawned_when_expired() {
        let mut t = SpawnTimer::new("Boss", 0.0, 0.0, 0.0, 0);
        t.killed_at = Utc::now() - Duration::seconds(60);
        assert_eq!(t.countdown_str(), "SPAWNED");
    }

    #[test]
    fn add_replaces_same_name() {
        let mut store = TimerStore::default();
        store.add(SpawnTimer::new("Fippy", 0.0, 0.0, 0.0, 600));
        store.add(SpawnTimer::new("Fippy", 1.0, 2.0, 3.0, 900));
        assert_eq!(store.len(), 1);
        assert_eq!(store.timers[0].respawn_secs, 900);
    }

    #[test]
    fn remove_by_index() {
        let mut store = TimerStore::default();
        store.add(SpawnTimer::new("A", 0.0, 0.0, 0.0, 600));
        store.add(SpawnTimer::new("B", 0.0, 0.0, 0.0, 600));
        store.remove(0);
        assert_eq!(store.len(), 1);
        assert_eq!(store.timers[0].name, "B");
    }

    #[test]
    fn save_and_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_string_lossy().into_owned();

        let mut store = TimerStore::default();
        store.add(SpawnTimer::new("Lord Nagafen", 100.5, 200.0, -50.0, 1800));
        store.add(SpawnTimer::new("Fippy Darkpaw", 10.0, 20.0, 0.0, 600));
        store.save("najena", &dir_str).unwrap();

        let loaded = TimerStore::load("najena", &dir_str);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.timers[0].name, "Lord Nagafen");
        assert_eq!(loaded.timers[0].respawn_secs, 1800);
        assert!((loaded.timers[0].x - 100.5).abs() < 0.01);
        assert_eq!(loaded.timers[1].name, "Fippy Darkpaw");
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let store = TimerStore::load("unknownzone", "/nonexistent/dir");
        assert!(store.is_empty());
    }
}