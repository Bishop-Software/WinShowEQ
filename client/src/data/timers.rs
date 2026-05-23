use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::Write as _;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};

use super::spawns::{SpawnCategory, SpawnStore};

const MAX_TIMERS: usize = 200;
const MIN_INTERVAL_SECS: i64 = 10;
const MAX_INTERVALS: usize = 10;

static VOID_ZONES: &[&str] = &["bazaar", "clz", "default", "nexus", "poknowledge"];

fn is_void_zone(zone: &str) -> bool {
    let z = zone.to_lowercase();
    VOID_ZONES.contains(&z.as_str()) || z.starts_with("guild")
}

fn loc_key(x: f32, y: f32) -> String {
    format!("{:.3},{:.3}", y, x)
}

/// Returns true for spawns that should never generate auto-timers.
/// Matches C# `SpawnObserver` exclusion rules.
fn is_excluded_spawn(name: &str, race: u32, owner_id: u32) -> bool {
    owner_id != 0                           // pets, mercs, familiars, mounts
        || name.starts_with('_')            // internal/placeholder names
        || matches!(race, 141 | 376 | 533)  // boats and other non-mob races
}

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
    /// True if this timer was auto-learned by SpawnObserver; false if manually entered.
    pub is_auto: bool,
    /// Number of confirmed respawn cycles observed (auto timers only).
    pub spawn_count: u32,
    /// Wall-clock time the mob last respawned (None for manually added timers).
    pub spawn_time: Option<DateTime<Utc>>,
    /// Zone name where this timer was learned.
    pub zone: String,
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
            is_auto: false,
            spawn_count: 0,
            spawn_time: None,
            zone: String::new(),
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

    pub fn clear_all(&mut self) {
        self.timers.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = &SpawnTimer> {
        self.timers.iter()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.timers.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.timers.is_empty()
    }

    /// Load timers for `zone` from `{dir}/spawns-{zone}.txt`.
    /// Returns an empty store if the file doesn't exist, can't be read, or zone is void.
    pub fn load(zone: &str, dir: &str) -> Self {
        if is_void_zone(zone) {
            return Self::default();
        }
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
        if self.timers.is_empty() || is_void_zone(zone) {
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
                "{};{};{};{};{};{};{};{};{};{}",
                t.name,
                t.x,
                t.y,
                t.z,
                t.killed_at.timestamp(),
                t.respawn_secs,
                if t.is_auto { 1 } else { 0 },
                t.spawn_count,
                t.spawn_time.map(|dt| dt.timestamp()).unwrap_or(0),
                t.zone,
            )?;
        }
        Ok(())
    }
}

fn timer_path(zone: &str, dir: &str) -> std::path::PathBuf {
    Path::new(dir).join(format!("spawns-{zone}.txt"))
}

fn parse_line(line: &str) -> Option<SpawnTimer> {
    let mut parts = line.splitn(10, ';');
    let name = parts.next()?.to_owned();
    let x: f32 = parts.next()?.parse().ok()?;
    let y: f32 = parts.next()?.parse().ok()?;
    let z: f32 = parts.next()?.parse().ok()?;
    let killed_unix: i64 = parts.next()?.parse().ok()?;
    let respawn_secs: i64 = parts.next()?.parse().ok()?;
    let is_auto = parts.next().and_then(|s| s.parse::<u8>().ok()).map(|v| v != 0).unwrap_or(false);
    let spawn_count = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let spawn_time = parts.next()
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|&ts| ts > 0)
        .and_then(|ts| DateTime::from_timestamp(ts, 0));
    let zone = parts.next().unwrap_or("").to_owned();
    let killed_at = DateTime::from_timestamp(killed_unix, 0).unwrap_or_else(Utc::now);
    Some(SpawnTimer { name, x, y, z, killed_at, respawn_secs, is_auto, spawn_count, spawn_time, zone })
}

// ---------------------------------------------------------------------------
// SpawnObserver — auto-learning respawn timer engine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct PendingKill {
    name: String,
    x: f32,
    y: f32,
    z: f32,
    killed_at: DateTime<Utc>,
}

/// Accumulated respawn observations for a single spawn location.
#[derive(Debug, Clone)]
pub struct SpawnObservation {
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub spawn_count: u32,
    pub intervals: Vec<i64>,
    /// All mob names observed at this location (placeholder + named variants).
    pub names: Vec<String>,
}

/// Detects kill and respawn events from consecutive tick NPC ID sets and
/// auto-promotes learned timers into a `TimerStore`.
#[derive(Debug, Default)]
pub struct SpawnObserver {
    prev_tick_ids: HashSet<u32>,
    pending_kills: HashMap<String, PendingKill>,
    pub observations: HashMap<String, SpawnObservation>,
}

impl SpawnObserver {
    /// Called when a zone change packet arrives — resets in-flight state.
    /// `observations` is replaced by the caller after loading the new zone file.
    pub fn on_zone_change(&mut self) {
        self.prev_tick_ids.clear();
        self.pending_kills.clear();
    }

    /// Diff the current tick's NPC ID set against the previous tick's, record
    /// kills and respawns, and auto-promote confident timers into `timers`.
    ///
    /// Returns `(promoted, log_messages)` where `promoted` is true if at least
    /// one timer was promoted this tick and `log_messages` holds lines for the
    /// caller to write to the log file.
    pub fn process_diff(
        &mut self,
        curr_ids: &HashSet<u32>,
        spawns: &SpawnStore,
        zone: &str,
        timers: &mut TimerStore,
    ) -> (bool, Vec<String>) {
        if zone.is_empty() || is_void_zone(zone) {
            self.prev_tick_ids = curr_ids.clone();
            return (false, Vec::new());
        }

        let now = Utc::now();
        let mut promoted = false;
        let mut log: Vec<String> = Vec::new();

        // Detect respawns: IDs new this tick that appear at a pending kill location.
        for &id in curr_ids {
            if self.prev_tick_ids.contains(&id) {
                continue;
            }
            if let Some(spawn) = spawns.get(id) {
                if spawn.spawn_category != SpawnCategory::Npc
                    || is_excluded_spawn(&spawn.name, spawn.race, spawn.owner_id)
                {
                    continue;
                }
                let key = loc_key(spawn.x, spawn.y);
                if let Some(kill) = self.pending_kills.remove(&key) {
                    let interval = (now - kill.killed_at).num_seconds();
                    if interval >= MIN_INTERVAL_SECS {
                        let obs = self.observations.entry(key).or_insert_with(|| SpawnObservation {
                            name: spawn.name.clone(),
                            x: spawn.x,
                            y: spawn.y,
                            z: spawn.z,
                            spawn_count: 0,
                            intervals: Vec::new(),
                            names: Vec::new(),
                        });
                        obs.spawn_count += 1;
                        obs.intervals.push(interval);
                        if obs.intervals.len() > MAX_INTERVALS {
                            obs.intervals.remove(0);
                        }
                        if !obs.names.contains(&spawn.name) {
                            obs.names.push(spawn.name.clone());
                        }
                        log.push(format!(
                            "[Timer] Respawn: {} — interval {}s (cycle {})",
                            spawn.name, interval, obs.spawn_count,
                        ));
                        if obs.spawn_count > 1 {
                            let avg = obs.intervals.iter().sum::<i64>() / obs.intervals.len() as i64;
                            timers.add(SpawnTimer {
                                name: kill.name.clone(),
                                x: kill.x,
                                y: kill.y,
                                z: kill.z,
                                killed_at: now,
                                respawn_secs: avg,
                                is_auto: true,
                                spawn_count: obs.spawn_count,
                                spawn_time: Some(now),
                                zone: zone.to_owned(),
                            });
                            log.push(format!(
                                "[Timer] Auto-timer promoted: {} — avg {}s over {} cycles",
                                kill.name, avg, obs.spawn_count,
                            ));
                            promoted = true;
                        }
                    } else {
                        log.push(format!(
                            "[Timer] Respawn skipped: {} — interval {}s below minimum {}s",
                            spawn.name, interval, MIN_INTERVAL_SECS,
                        ));
                    }
                }
            }
        }

        // Detect kills: IDs present last tick but absent this tick.
        // Note: do NOT re-check spawn_category here. An NPC kill causes the spawn to
        // transition to spawn_type=2 (Corpse) in the same tick, so spawns.get() may
        // return a Corpse even though the ID was an NPC in prev_tick_ids.
        for &id in &self.prev_tick_ids {
            if curr_ids.contains(&id) {
                continue;
            }
            if let Some(spawn) = spawns.get(id) {
                if is_excluded_spawn(&spawn.name, spawn.race, spawn.owner_id) {
                    continue;
                }
                // Use spawn_x/spawn_y (first-seen position = spawn point) rather than
                // current position, so kills on pulled mobs still match the respawn location.
                let key = loc_key(spawn.spawn_x, spawn.spawn_y);
                log.push(format!(
                    "[Timer] Kill detected: {} @ {}",
                    spawn.name, key,
                ));
                self.pending_kills.insert(key, PendingKill {
                    name: spawn.name.clone(),
                    x: spawn.spawn_x,
                    y: spawn.spawn_y,
                    z: spawn.z,
                    killed_at: now,
                });
            }
        }

        self.prev_tick_ids = curr_ids.clone();
        (promoted, log)
    }

    /// Fully reset observer state for the current zone (called by Clear All Timers).
    pub fn reset_zone(&mut self) {
        self.prev_tick_ids.clear();
        self.pending_kills.clear();
        self.observations.clear();
    }

    /// Load observation data for `zone` from `{dir}/obs-{zone}.txt`.
    pub fn load(zone: &str, dir: &str) -> HashMap<String, SpawnObservation> {
        if is_void_zone(zone) {
            return HashMap::new();
        }
        let path = obs_path(zone, dir);
        let Ok(text) = fs::read_to_string(&path) else {
            return HashMap::new();
        };
        let mut map = HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, obs)) = parse_obs_line(line) {
                map.insert(key, obs);
            }
        }
        map
    }

    /// Save observation data for `zone` to `{dir}/obs-{zone}.txt`.
    pub fn save(observations: &HashMap<String, SpawnObservation>, zone: &str, dir: &str) -> std::io::Result<()> {
        if observations.is_empty() || is_void_zone(zone) {
            return Ok(());
        }
        let path = obs_path(zone, dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut f = fs::File::create(&path)?;
        for (key, obs) in observations {
            let intervals: Vec<String> = obs.intervals.iter().map(|i| i.to_string()).collect();
            writeln!(
                f,
                "{};{};{};{};{};{};{};{}",
                key,
                obs.name,
                obs.x,
                obs.y,
                obs.z,
                obs.spawn_count,
                intervals.join(","),
                obs.names.join("|"),
            )?;
        }
        Ok(())
    }
}

fn obs_path(zone: &str, dir: &str) -> std::path::PathBuf {
    Path::new(dir).join(format!("obs-{zone}.txt"))
}

fn parse_obs_line(line: &str) -> Option<(String, SpawnObservation)> {
    let mut parts = line.splitn(8, ';');
    let key = parts.next()?.to_owned();
    let name = parts.next()?.to_owned();
    let x: f32 = parts.next()?.parse().ok()?;
    let y: f32 = parts.next()?.parse().ok()?;
    let z: f32 = parts.next()?.parse().ok()?;
    let spawn_count: u32 = parts.next()?.parse().ok()?;
    let intervals_str = parts.next().unwrap_or("");
    let intervals: Vec<i64> = if intervals_str.is_empty() {
        Vec::new()
    } else {
        intervals_str.split(',').filter_map(|s| s.parse().ok()).collect()
    };
    let names_str = parts.next().unwrap_or("");
    let names: Vec<String> = if names_str.is_empty() {
        Vec::new()
    } else {
        names_str.split('|').map(|s| s.to_owned()).collect()
    };
    Some((key, SpawnObservation { name, x, y, z, spawn_count, intervals, names }))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::Local;

    use super::super::spawns::{SpawnCategory, SpawnInfo, SpawnStore};
    use super::*;

    fn make_npc(id: u32, name: &str, x: f32, y: f32) -> SpawnInfo {
        SpawnInfo {
            id,
            name: name.to_owned(),
            last_name: String::new(),
            x,
            y,
            z: 0.0,
            heading: 0.0,
            speed: 0.0,
            owner_id: 0,
            spawn_type: 1,
            class: 1,
            race: 1,
            level: 1,
            hidden: 0,
            primary: 0,
            offhand: 0,
            spawn_category: SpawnCategory::Npc,
            first_seen: Local::now(),
            spawn_x: x,
            spawn_y: y,
            is_hunt: false,
            is_caution: false,
            is_danger: false,
            is_rare: false,
        }
    }

    fn make_store(spawns: Vec<SpawnInfo>) -> SpawnStore {
        let mut store = SpawnStore::default();
        for s in spawns {
            store.upsert_info(s);
        }
        store
    }

    fn ids(v: &[u32]) -> HashSet<u32> {
        v.iter().copied().collect()
    }

    // --- is_void_zone ---

    #[test]
    fn void_zone_false_for_normal_zones() {
        assert!(!is_void_zone("najena"));
        assert!(!is_void_zone("gfaydark"));
        assert!(!is_void_zone("blackburrow"));
    }

    #[test]
    fn void_zone_true_for_known_void_zones() {
        assert!(is_void_zone("bazaar"));
        assert!(is_void_zone("nexus"));
        assert!(is_void_zone("poknowledge"));
        assert!(is_void_zone("clz"));
        assert!(is_void_zone("default"));
    }

    #[test]
    fn void_zone_true_for_guild_prefix() {
        assert!(is_void_zone("guildlobby"));
        assert!(is_void_zone("guildhall"));
        assert!(is_void_zone("GUILDLOBBY")); // case-insensitive
    }

    // --- is_excluded_spawn ---

    #[test]
    fn excluded_owner_id_nonzero() {
        assert!(is_excluded_spawn("Warder", 1, 1234));
    }

    #[test]
    fn excluded_underscore_prefix() {
        assert!(is_excluded_spawn("_placeholder", 1, 0));
    }

    #[test]
    fn excluded_boat_races() {
        assert!(is_excluded_spawn("Boat", 141, 0));
        assert!(is_excluded_spawn("Boat", 376, 0));
        assert!(is_excluded_spawn("Boat", 533, 0));
    }

    #[test]
    fn not_excluded_normal_npc() {
        assert!(!is_excluded_spawn("Fippy Darkpaw", 1, 0));
    }

    // --- loc_key ---

    #[test]
    fn loc_key_formats_y_then_x_three_decimal_places() {
        assert_eq!(loc_key(100.0, 200.0), "200.000,100.000");
        assert_eq!(loc_key(-50.5, 75.123), "75.123,-50.500");
    }

    // --- SpawnObserver::process_diff: kill detection ---

    #[test]
    fn process_diff_records_pending_kill_when_npc_disappears() {
        let mut observer = SpawnObserver::default();
        let mut timers = TimerStore::default();
        let store = make_store(vec![make_npc(1, "Fippy Darkpaw", 100.0, 200.0)]);

        observer.process_diff(&ids(&[1]), &store, "blackburrow", &mut timers);
        observer.process_diff(&ids(&[]), &store, "blackburrow", &mut timers);

        assert!(observer.pending_kills.contains_key(&loc_key(100.0, 200.0)));
    }

    #[test]
    fn process_diff_no_kill_recorded_in_void_zone() {
        let mut observer = SpawnObserver::default();
        let mut timers = TimerStore::default();
        let store = make_store(vec![make_npc(1, "Merchant", 100.0, 200.0)]);

        observer.process_diff(&ids(&[1]), &store, "bazaar", &mut timers);
        observer.process_diff(&ids(&[]), &store, "bazaar", &mut timers);

        assert!(observer.pending_kills.is_empty());
    }

    #[test]
    fn process_diff_no_kill_recorded_in_empty_zone() {
        let mut observer = SpawnObserver::default();
        let mut timers = TimerStore::default();
        let store = make_store(vec![make_npc(1, "NPC", 100.0, 200.0)]);

        observer.process_diff(&ids(&[1]), &store, "", &mut timers);
        observer.process_diff(&ids(&[]), &store, "", &mut timers);

        assert!(observer.pending_kills.is_empty());
    }

    #[test]
    fn process_diff_no_kill_for_pet() {
        let mut observer = SpawnObserver::default();
        let mut timers = TimerStore::default();
        let mut pet = make_npc(1, "Warder", 100.0, 200.0);
        pet.owner_id = 999;
        let store = make_store(vec![pet]);

        observer.process_diff(&ids(&[1]), &store, "crushbone", &mut timers);
        observer.process_diff(&ids(&[]), &store, "crushbone", &mut timers);

        assert!(observer.pending_kills.is_empty());
    }

    #[test]
    fn process_diff_no_kill_for_underscore_name() {
        let mut observer = SpawnObserver::default();
        let mut timers = TimerStore::default();
        let store = make_store(vec![make_npc(1, "_placeholder", 100.0, 200.0)]);

        observer.process_diff(&ids(&[1]), &store, "crushbone", &mut timers);
        observer.process_diff(&ids(&[]), &store, "crushbone", &mut timers);

        assert!(observer.pending_kills.is_empty());
    }

    #[test]
    fn process_diff_kill_uses_spawn_point_not_death_position() {
        let mut observer = SpawnObserver::default();
        let mut timers = TimerStore::default();

        // Mob spawns at (100, 200), then moves to (300, 400) before being killed.
        let mut npc = make_npc(1, "Fippy Darkpaw", 100.0, 200.0);
        let store = make_store(vec![npc.clone()]);

        // Tick 1: mob seen at spawn point
        observer.process_diff(&ids(&[1]), &store, "blackburrow", &mut timers);

        // Mob walks to (300, 400) — update store with moved position, spawn_x/spawn_y preserved
        npc.x = 300.0;
        npc.y = 400.0;
        let moved_store = make_store(vec![npc]);

        // Tick 2: mob gone (killed at death position 300, 400)
        observer.process_diff(&ids(&[]), &moved_store, "blackburrow", &mut timers);

        // Kill should be keyed at spawn point (100, 200), not death position (300, 400)
        assert!(observer.pending_kills.contains_key(&loc_key(100.0, 200.0)));
        assert!(!observer.pending_kills.contains_key(&loc_key(300.0, 400.0)));
    }

    #[test]
    fn process_diff_no_kill_for_boat_race() {
        let mut observer = SpawnObserver::default();
        let mut timers = TimerStore::default();
        let mut boat = make_npc(1, "The Big Boat", 100.0, 200.0);
        boat.race = 141;
        let store = make_store(vec![boat]);

        observer.process_diff(&ids(&[1]), &store, "butcher", &mut timers);
        observer.process_diff(&ids(&[]), &store, "butcher", &mut timers);

        assert!(observer.pending_kills.is_empty());
    }

    // --- SpawnObserver::process_diff: respawn and promotion ---

    #[test]
    fn process_diff_no_promotion_on_first_respawn_cycle() {
        let mut observer = SpawnObserver::default();
        let mut timers = TimerStore::default();
        let npc = make_npc(1, "Fippy Darkpaw", 100.0, 200.0);
        let store = make_store(vec![npc]);
        let key = loc_key(100.0, 200.0);

        // Present → gone (kill recorded)
        observer.process_diff(&ids(&[1]), &store, "blackburrow", &mut timers);
        observer.process_diff(&ids(&[]), &store, "blackburrow", &mut timers);
        // Backdate so interval passes MIN_INTERVAL_SECS
        observer.pending_kills.get_mut(&key).unwrap().killed_at =
            Utc::now() - Duration::seconds(600);

        // Respawn — first cycle: spawn_count becomes 1, not > 1, no promotion
        let (promoted, _) = observer.process_diff(&ids(&[1]), &store, "blackburrow", &mut timers);

        assert!(!promoted);
        assert!(timers.is_empty());
        assert_eq!(observer.observations[&key].spawn_count, 1);
    }

    #[test]
    fn process_diff_promotes_timer_on_second_respawn_cycle() {
        let mut observer = SpawnObserver::default();
        let mut timers = TimerStore::default();
        let npc = make_npc(1, "Fippy Darkpaw", 100.0, 200.0);
        let store = make_store(vec![npc]);
        let key = loc_key(100.0, 200.0);
        let zone = "blackburrow";

        // Cycle 1: present → gone → respawn (spawn_count=1, no promotion)
        observer.process_diff(&ids(&[1]), &store, zone, &mut timers);
        observer.process_diff(&ids(&[]), &store, zone, &mut timers);
        observer.pending_kills.get_mut(&key).unwrap().killed_at =
            Utc::now() - Duration::seconds(600);
        let (promoted, _) = observer.process_diff(&ids(&[1]), &store, zone, &mut timers);
        assert!(!promoted);

        // Cycle 2: gone → respawn (spawn_count=2, promoted)
        observer.process_diff(&ids(&[]), &store, zone, &mut timers);
        observer.pending_kills.get_mut(&key).unwrap().killed_at =
            Utc::now() - Duration::seconds(600);
        let (promoted, _) = observer.process_diff(&ids(&[1]), &store, zone, &mut timers);

        assert!(promoted);
        assert_eq!(timers.len(), 1);
        let t = &timers.timers[0];
        assert_eq!(t.name, "Fippy Darkpaw");
        assert!(t.is_auto);
        assert_eq!(t.spawn_count, 2);
        assert!(t.respawn_secs >= MIN_INTERVAL_SECS);
    }

    #[test]
    fn process_diff_skips_respawn_when_interval_too_short() {
        let mut observer = SpawnObserver::default();
        let mut timers = TimerStore::default();
        let npc = make_npc(1, "Fast Spawn", 100.0, 200.0);
        let store = make_store(vec![npc]);

        // Present → gone (kill recorded with killed_at = now)
        observer.process_diff(&ids(&[1]), &store, "crushbone", &mut timers);
        observer.process_diff(&ids(&[]), &store, "crushbone", &mut timers);
        // Do NOT backdate — interval will be ~0 secs (< MIN_INTERVAL_SECS=10)
        let (promoted, _) = observer.process_diff(&ids(&[1]), &store, "crushbone", &mut timers);

        assert!(!promoted);
        assert!(observer.observations.is_empty());
    }

    #[test]
    fn process_diff_averages_intervals_over_multiple_cycles() {
        let mut observer = SpawnObserver::default();
        let mut timers = TimerStore::default();
        let npc = make_npc(1, "Lord Nagafen", 100.0, 200.0);
        let store = make_store(vec![npc]);
        let key = loc_key(100.0, 200.0);
        let zone = "soldungb";

        // Cycle 1: 300s interval
        observer.process_diff(&ids(&[1]), &store, zone, &mut timers);
        observer.process_diff(&ids(&[]), &store, zone, &mut timers);
        observer.pending_kills.get_mut(&key).unwrap().killed_at =
            Utc::now() - Duration::seconds(300);
        observer.process_diff(&ids(&[1]), &store, zone, &mut timers);

        // Cycle 2: 700s interval → avg should be ~500
        observer.process_diff(&ids(&[]), &store, zone, &mut timers);
        observer.pending_kills.get_mut(&key).unwrap().killed_at =
            Utc::now() - Duration::seconds(700);
        observer.process_diff(&ids(&[1]), &store, zone, &mut timers);

        assert_eq!(timers.len(), 1);
        // avg = (300 + 700) / 2 = 500; allow ±2s for clock jitter
        assert!((timers.timers[0].respawn_secs - 500).abs() <= 2);
    }

    // --- on_zone_change / reset_zone ---

    #[test]
    fn on_zone_change_clears_transient_state_preserves_observations() {
        let mut observer = SpawnObserver::default();
        let mut timers = TimerStore::default();
        let store = make_store(vec![make_npc(1, "Orc", 100.0, 200.0)]);

        observer.process_diff(&ids(&[1]), &store, "crushbone", &mut timers);
        observer.process_diff(&ids(&[]), &store, "crushbone", &mut timers);
        observer.observations.insert(
            "key".to_owned(),
            SpawnObservation {
                name: "Orc".to_owned(),
                x: 0.0, y: 0.0, z: 0.0,
                spawn_count: 1,
                intervals: vec![600],
                names: vec!["Orc".to_owned()],
            },
        );

        observer.on_zone_change();

        assert!(observer.pending_kills.is_empty());
        assert!(observer.prev_tick_ids.is_empty());
        assert!(!observer.observations.is_empty(), "observations survive zone change");
    }

    #[test]
    fn reset_zone_clears_everything_including_observations() {
        let mut observer = SpawnObserver::default();
        observer.observations.insert(
            "key".to_owned(),
            SpawnObservation {
                name: "NPC".to_owned(),
                x: 0.0, y: 0.0, z: 0.0,
                spawn_count: 3,
                intervals: vec![600, 610],
                names: vec!["NPC".to_owned()],
            },
        );
        observer.pending_kills.insert(
            "key".to_owned(),
            PendingKill { name: "NPC".to_owned(), x: 0.0, y: 0.0, z: 0.0, killed_at: Utc::now() },
        );

        observer.reset_zone();

        assert!(observer.observations.is_empty());
        assert!(observer.pending_kills.is_empty());
        assert!(observer.prev_tick_ids.is_empty());
    }

    // --- Observation persistence ---

    #[test]
    fn obs_save_and_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_string_lossy().into_owned();
        let key = loc_key(100.0, 200.0);

        let mut observations = HashMap::new();
        observations.insert(
            key.clone(),
            SpawnObservation {
                name: "Fippy Darkpaw".to_owned(),
                x: 100.0, y: 200.0, z: -5.0,
                spawn_count: 3,
                intervals: vec![600, 605, 595],
                names: vec!["Fippy Darkpaw".to_owned(), "_Fippy".to_owned()],
            },
        );

        SpawnObserver::save(&observations, "blackburrow", &dir_str).unwrap();
        let loaded = SpawnObserver::load("blackburrow", &dir_str);

        assert_eq!(loaded.len(), 1);
        let obs = loaded.get(&key).unwrap();
        assert_eq!(obs.name, "Fippy Darkpaw");
        assert_eq!(obs.spawn_count, 3);
        assert_eq!(obs.intervals, vec![600, 605, 595]);
        assert_eq!(obs.names, vec!["Fippy Darkpaw", "_Fippy"]);
        assert!((obs.x - 100.0).abs() < 0.01);
        assert!((obs.y - 200.0).abs() < 0.01);
    }

    #[test]
    fn obs_load_returns_empty_for_void_zone() {
        assert!(SpawnObserver::load("bazaar", "/nonexistent").is_empty());
    }

    #[test]
    fn obs_save_does_not_create_file_for_void_zone() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_string_lossy().into_owned();
        let mut obs = HashMap::new();
        obs.insert(
            "key".to_owned(),
            SpawnObservation {
                name: "test".to_owned(), x: 0.0, y: 0.0, z: 0.0,
                spawn_count: 1, intervals: vec![600], names: vec![],
            },
        );

        SpawnObserver::save(&obs, "nexus", &dir_str).unwrap();

        assert!(!std::path::Path::new(&dir_str).join("obs-nexus.txt").exists());
    }

    // --- TimerStore persistence ---

    #[test]
    fn timer_store_load_skips_void_zone() {
        assert!(TimerStore::load("guildlobby", "/nonexistent").is_empty());
    }

    #[test]
    fn timer_store_load_missing_file_returns_empty() {
        assert!(TimerStore::load("unknownzone", "/nonexistent/dir").is_empty());
    }

    #[test]
    fn timer_store_save_and_load_round_trips_auto_fields() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_string_lossy().into_owned();

        let mut store = TimerStore::default();
        let mut t = SpawnTimer::new("Lord Nagafen", 100.5, 200.0, -50.0, 1800);
        t.is_auto = true;
        t.spawn_count = 5;
        store.add(t);
        store.save("nagafen", &dir_str).unwrap();

        let loaded = TimerStore::load("nagafen", &dir_str);
        assert_eq!(loaded.len(), 1);
        assert!(loaded.timers[0].is_auto);
        assert_eq!(loaded.timers[0].spawn_count, 5);
    }

    #[test]
    fn timer_store_save_and_load_round_trips() {
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

    // --- SpawnTimer helpers ---

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
    fn add_replaces_existing_timer_with_same_name() {
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
    fn clear_all_empties_the_store() {
        let mut store = TimerStore::default();
        store.add(SpawnTimer::new("A", 0.0, 0.0, 0.0, 600));
        store.add(SpawnTimer::new("B", 0.0, 0.0, 0.0, 900));
        store.clear_all();
        assert!(store.is_empty());
    }
}