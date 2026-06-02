use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::Write as _;
use std::path::Path;

use chrono::{DateTime, Duration, NaiveDate, Utc};

use super::spawns::{SpawnCategory, SpawnStore};

const MAX_TIMERS: usize = 200;
const MIN_INTERVAL_SECS: i64 = 10;
const MAX_INTERVALS: usize = 10;
const MAX_ALL_NAMES: usize = 10;

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
        || matches!(race, 141 | 376 | 533) // boats and other non-mob races
}

/// Returns true if `name` looks like a named/boss mob (starts uppercase or '#').
fn is_named_mob(name: &str) -> bool {
    name.starts_with(|c: char| c.is_uppercase()) || name.starts_with('#')
}

/// From a list of observed names, return the best display name:
/// first named-mob name (uppercase/# prefix), otherwise `fallback`.
fn best_display_name<'a>(all_names: &'a [String], fallback: &'a str) -> &'a str {
    all_names
        .iter()
        .find(|n| is_named_mob(n))
        .map(|s| s.as_str())
        .unwrap_or(fallback)
}

/// A tracked respawn timer for a named EQ mob.
#[derive(Debug, Clone)]
pub struct SpawnTimer {
    /// Best display name (named/boss mob promoted here).
    pub name: String,
    /// "y.yyy,x.xxx" location key — primary key.
    pub spawn_loc: String,
    /// All mob names observed at this spawn location.
    pub all_names: Vec<String>,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// UTC instant the mob was killed (timer start). None if not recorded.
    pub killed_at: Option<DateTime<Utc>>,
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
    /// Sticky timers survive zone changes and are not cleared until "Clear All" is used.
    pub sticky: bool,
}

impl SpawnTimer {
    pub fn new(name: impl Into<String>, x: f32, y: f32, z: f32, respawn_secs: i64) -> Self {
        Self {
            name: name.into(),
            spawn_loc: loc_key(x, y),
            all_names: Vec::new(),
            x,
            y,
            z,
            killed_at: Some(Utc::now()),
            respawn_secs,
            is_auto: false,
            spawn_count: 0,
            spawn_time: None,
            zone: String::new(),
            sticky: false,
        }
    }

    pub fn next_spawn_at(&self) -> Option<DateTime<Utc>> {
        self.killed_at
            .map(|kt| kt + Duration::seconds(self.respawn_secs))
    }

    /// Seconds remaining until expected respawn. Negative means already past window.
    /// Returns `i64::MIN` when kill time is unknown — treated as already spawned.
    pub fn secs_remaining(&self) -> i64 {
        match self.next_spawn_at() {
            Some(t) => (t - Utc::now()).num_seconds(),
            None => i64::MIN,
        }
    }

    pub fn is_spawned(&self) -> bool {
        self.secs_remaining() <= 0
    }

    /// True when the mob is known to be alive (spawned but not yet killed).
    pub fn is_alive(&self) -> bool {
        self.killed_at.is_none()
    }

    pub fn countdown_str(&self) -> String {
        if self.killed_at.is_none() {
            return "ALIVE".to_owned();
        }
        let secs = self.secs_remaining();
        if secs <= 0 {
            return "UNKNOWN".to_owned();
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
    /// Add or replace a timer. Keyed on `spawn_loc` when non-empty, else falls back to `name`.
    pub fn add(&mut self, timer: SpawnTimer) {
        let existing = if !timer.spawn_loc.is_empty() {
            self.timers
                .iter_mut()
                .find(|t| t.spawn_loc == timer.spawn_loc)
        } else {
            self.timers.iter_mut().find(|t| t.name == timer.name)
        };
        if let Some(existing) = existing {
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
    /// Migrates old Rust and C# MySEQ formats on first load and immediately re-saves.
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
        let mut migrated = false;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((t, was_migrated)) = parse_line(line, zone) {
                store.timers.push_back(t);
                if was_migrated {
                    migrated = true;
                }
            }
        }
        if migrated {
            let _ = store.save(zone, dir);
        }
        store
    }

    /// Save timers for `zone` to `{dir}/spawns-{zone}.txt` using the new hybrid format:
    /// `{spawn_loc};{spawn_count};{respawn_secs};{spawn_time_unix};{kill_time_unix};{next_spawn_unix};{name};{all_names};{x};{y};{z}`
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
            let spawn_time_unix = t.spawn_time.map(|dt| dt.timestamp()).unwrap_or(0);
            let kill_time_unix = t.killed_at.map(|dt| dt.timestamp()).unwrap_or(0);
            let next_spawn_unix = t
                .killed_at
                .map(|kt| (kt + Duration::seconds(t.respawn_secs)).timestamp())
                .unwrap_or(0);
            let all_names = t.all_names.join(",");
            writeln!(
                f,
                "{};{};{};{};{};{};{};{};{};{};{};{}",
                t.spawn_loc,
                t.spawn_count,
                t.respawn_secs,
                spawn_time_unix,
                kill_time_unix,
                next_spawn_unix,
                t.name,
                all_names,
                t.x,
                t.y,
                t.z,
                t.sticky as u8,
            )?;
        }
        Ok(())
    }
}

fn timer_path(zone: &str, dir: &str) -> std::path::PathBuf {
    Path::new(dir).join(format!("spawns-{zone}.txt"))
}

/// Parse a single timer file line. Returns `(timer, was_migrated)` or None on error.
///
/// Format detection:
/// - field[0] has comma AND field[3] parses as i64 → new hybrid format
/// - field[0] has comma AND field[3] is not an i64  → C# MySEQ format (migrate)
/// - field[0] has no comma                          → old Rust format (migrate)
fn parse_line(line: &str, zone: &str) -> Option<(SpawnTimer, bool)> {
    let parts: Vec<&str> = line.splitn(13, ';').collect();
    if parts.is_empty() {
        return None;
    }
    if parts[0].contains(',') {
        if parts.len() < 11 {
            return None;
        }
        if parts[3].parse::<i64>().is_ok() {
            parse_new_format(&parts, zone).map(|t| (t, false))
        } else {
            parse_cs_format(&parts, zone).map(|t| (t, true))
        }
    } else {
        parse_old_format(&parts, zone).map(|t| (t, true))
    }
}

/// New hybrid format (11 fields):
/// `{spawn_loc};{spawn_count};{respawn_secs};{spawn_time_unix};{kill_time_unix};{next_spawn_unix};{name};{all_names};{x};{y};{z}`
fn parse_new_format(parts: &[&str], zone: &str) -> Option<SpawnTimer> {
    let spawn_loc = parts[0].to_owned();
    let spawn_count: u32 = parts[1].parse().ok()?;
    let respawn_secs: i64 = parts[2].parse().ok()?;
    let spawn_time_unix: i64 = parts[3].parse().ok()?;
    let kill_time_unix: i64 = parts[4].parse().ok()?;
    // parts[5] = next_spawn_unix — ignored; recomputed from killed_at + respawn_secs
    let name = parts[6].to_owned();
    let all_names_str = parts[7];
    let all_names: Vec<String> = if all_names_str.is_empty() {
        Vec::new()
    } else {
        all_names_str.split(',').map(|s| s.to_owned()).collect()
    };
    let x: f32 = parts[8].parse().ok()?;
    let y: f32 = parts[9].parse().ok()?;
    let z: f32 = parts[10].parse().ok()?;
    let sticky = parts
        .get(11)
        .and_then(|s| s.trim().parse::<u8>().ok())
        .map(|v| v != 0)
        .unwrap_or(false);

    let killed_at = (kill_time_unix > 0)
        .then(|| DateTime::from_timestamp(kill_time_unix, 0))
        .flatten();
    let spawn_time = (spawn_time_unix > 0)
        .then(|| DateTime::from_timestamp(spawn_time_unix, 0))
        .flatten();
    let is_auto = spawn_count >= 2;

    Some(SpawnTimer {
        name,
        spawn_loc,
        all_names,
        x,
        y,
        z,
        killed_at,
        respawn_secs,
        is_auto,
        spawn_count,
        spawn_time,
        zone: zone.to_owned(),
        sticky,
    })
}

/// C# MySEQ format (11 fields):
/// `{SpawnLoc};{SpawnCount};{SpawnTimer};{SpawnTimeStr};{KillTimeStr};{NextSpawnStr};{LastSpawnName};{AllNames};{X};{Y};{Z}`
fn parse_cs_format(parts: &[&str], zone: &str) -> Option<SpawnTimer> {
    let spawn_loc = parts[0].to_owned();
    let spawn_count: u32 = parts[1].parse().ok()?;
    let respawn_secs: i64 = parts[2].parse().ok()?;
    let spawn_time = parse_cs_datetime(parts[3]);
    let killed_at = parse_cs_datetime(parts[4]);
    // parts[5] = NextSpawnStr — ignored
    let name = parts[6].to_owned();
    let all_names_str = parts[7];
    let all_names: Vec<String> = if all_names_str.is_empty() {
        Vec::new()
    } else {
        all_names_str.split(',').map(|s| s.to_owned()).collect()
    };
    let x: f32 = parts[8].parse().ok()?;
    let y: f32 = parts[9].parse().ok()?;
    let z: f32 = parts[10].parse().ok()?;
    let is_auto = spawn_count >= 2;

    Some(SpawnTimer {
        name,
        spawn_loc,
        all_names,
        x,
        y,
        z,
        killed_at,
        respawn_secs,
        is_auto,
        spawn_count,
        spawn_time,
        zone: zone.to_owned(),
        sticky: false,
    })
}

/// Old Rust format (10 fields):
/// `{name};{x};{y};{z};{killed_unix};{respawn_secs};{is_auto};{spawn_count};{spawn_time_unix};{zone}`
fn parse_old_format(parts: &[&str], zone: &str) -> Option<SpawnTimer> {
    if parts.len() < 6 {
        return None;
    }
    let name = parts[0].to_owned();
    let x: f32 = parts[1].parse().ok()?;
    let y: f32 = parts[2].parse().ok()?;
    let z: f32 = parts[3].parse().ok()?;
    let killed_unix: i64 = parts[4].parse().ok()?;
    let respawn_secs: i64 = parts[5].parse().ok()?;
    let is_auto = parts
        .get(6)
        .and_then(|s| s.parse::<u8>().ok())
        .map(|v| v != 0)
        .unwrap_or(false);
    let spawn_count: u32 = parts.get(7).and_then(|s| s.parse().ok()).unwrap_or(0);
    let spawn_time = parts
        .get(8)
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|&ts| ts > 0)
        .and_then(|ts| DateTime::from_timestamp(ts, 0));
    let killed_at = (killed_unix > 0)
        .then(|| DateTime::from_timestamp(killed_unix, 0))
        .flatten();
    let spawn_loc = loc_key(x, y);

    Some(SpawnTimer {
        name,
        spawn_loc,
        all_names: Vec::new(),
        x,
        y,
        z,
        killed_at,
        respawn_secs,
        is_auto,
        spawn_count,
        spawn_time,
        zone: zone.to_owned(),
        sticky: false,
    })
}

/// Parse a .NET `DateTime.ToString()` string (en-US: "M/D/YYYY H:MM:SS AM/PM") to UTC.
fn parse_cs_datetime(s: &str) -> Option<DateTime<Utc>> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let parts: Vec<&str> = s.splitn(3, ' ').collect();
    if parts.len() < 2 {
        return None;
    }
    let date_parts: Vec<&str> = parts[0].split('/').collect();
    if date_parts.len() != 3 {
        return None;
    }
    let month: u32 = date_parts[0].parse().ok()?;
    let day: u32 = date_parts[1].parse().ok()?;
    let year: i32 = date_parts[2].parse().ok()?;

    let time_parts: Vec<&str> = parts[1].split(':').collect();
    if time_parts.len() < 2 {
        return None;
    }
    let mut hour: u32 = time_parts[0].parse().ok()?;
    let min: u32 = time_parts[1].parse().ok()?;
    let sec: u32 = time_parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);

    let ampm = parts.get(2).map(|s| s.to_uppercase()).unwrap_or_default();
    if ampm == "PM" && hour < 12 {
        hour += 12;
    } else if ampm == "AM" && hour == 12 {
        hour = 0;
    }

    let naive = NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(hour, min, sec)?;
    Some(naive.and_utc())
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
    /// NPC ID → name recorded on the previous tick while the spawn was alive.
    prev_tick_npcs: HashMap<u32, String>,
    pending_kills: HashMap<String, PendingKill>,
    pub observations: HashMap<String, SpawnObservation>,
}

impl SpawnObserver {
    /// Called when a zone change packet arrives — resets in-flight state.
    pub fn on_zone_change(&mut self) {
        self.prev_tick_npcs.clear();
        self.pending_kills.clear();
    }

    /// Diff the current tick's NPC ID set against the previous tick's, record
    /// kills and respawns, and auto-promote confident timers into `timers`.
    ///
    /// Returns `(promoted, log_messages)`.
    pub fn process_diff(
        &mut self,
        curr_ids: &HashSet<u32>,
        spawns: &SpawnStore,
        zone: &str,
        timers: &mut TimerStore,
    ) -> (bool, Vec<String>) {
        if zone.is_empty() || is_void_zone(zone) {
            self.prev_tick_npcs = curr_ids
                .iter()
                .filter_map(|&id| spawns.get(id).map(|s| (id, s.name.clone())))
                .collect();
            return (false, Vec::new());
        }

        let now = Utc::now();
        let mut promoted = false;
        let mut log: Vec<String> = Vec::new();

        // Detect respawns: IDs new this tick that appear at a pending kill location.
        for &id in curr_ids {
            if self.prev_tick_npcs.contains_key(&id) {
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
                        let obs = self.observations.entry(key.clone()).or_insert_with(|| {
                            SpawnObservation {
                                name: spawn.name.clone(),
                                x: spawn.x,
                                y: spawn.y,
                                z: spawn.z,
                                spawn_count: 0,
                                intervals: Vec::new(),
                                names: Vec::new(),
                            }
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
                            let avg =
                                obs.intervals.iter().sum::<i64>() / obs.intervals.len() as i64;

                            // Build all_names from all observed names at this location
                            let mut all_names = obs.names.clone();
                            if !all_names.contains(&kill.name) {
                                all_names.push(kill.name.clone());
                            }
                            all_names.truncate(MAX_ALL_NAMES);

                            // Prefer a named/boss mob (starts uppercase or '#') as display name
                            let display_name = best_display_name(&all_names, &kill.name).to_owned();

                            let existing_sticky = timers
                                .timers
                                .iter()
                                .find(|t| t.spawn_loc == key)
                                .map(|t| t.sticky)
                                .unwrap_or(false);
                            timers.add(SpawnTimer {
                                name: display_name,
                                spawn_loc: key,
                                all_names,
                                x: kill.x,
                                y: kill.y,
                                z: kill.z,
                                killed_at: None, // mob just spawned; countdown starts on kill
                                respawn_secs: avg,
                                is_auto: true,
                                spawn_count: obs.spawn_count,
                                spawn_time: Some(now),
                                zone: zone.to_owned(),
                                sticky: existing_sticky,
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
        for (&id, prev_name) in &self.prev_tick_npcs {
            if curr_ids.contains(&id) {
                continue;
            }
            let Some(spawn) = spawns.get(id) else {
                continue;
            };
            if is_excluded_spawn(prev_name, spawn.race, spawn.owner_id) {
                continue;
            }
            let key = loc_key(spawn.spawn_x, spawn.spawn_y);
            log.push(format!("[Timer] Kill detected: {} @ {}", prev_name, key));
            // Update the promoted timer's kill time so the countdown starts from now,
            // matching C# behavior where KillTimeDT and NextSpawnDT are set on kill.
            if let Some(t) = timers.timers.iter_mut().find(|t| t.spawn_loc == key) {
                t.killed_at = Some(now);
                promoted = true; // mark dirty so auto-save fires
            }
            self.pending_kills.insert(
                key,
                PendingKill {
                    name: prev_name.clone(),
                    x: spawn.spawn_x,
                    y: spawn.spawn_y,
                    z: spawn.z,
                    killed_at: now,
                },
            );
        }

        self.prev_tick_npcs = curr_ids
            .iter()
            .filter_map(|&id| spawns.get(id).map(|s| (id, s.name.clone())))
            .collect();
        (promoted, log)
    }

    /// Fully reset observer state for the current zone (called by Clear All Timers).
    pub fn reset_zone(&mut self) {
        self.prev_tick_npcs.clear();
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
    pub fn save(
        observations: &HashMap<String, SpawnObservation>,
        zone: &str,
        dir: &str,
    ) -> std::io::Result<()> {
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
        intervals_str
            .split(',')
            .filter_map(|s| s.parse().ok())
            .collect()
    };
    let names_str = parts.next().unwrap_or("");
    let names: Vec<String> = if names_str.is_empty() {
        Vec::new()
    } else {
        names_str.split('|').map(|s| s.to_owned()).collect()
    };
    Some((
        key,
        SpawnObservation {
            name,
            x,
            y,
            z,
            spawn_count,
            intervals,
            names,
        },
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{Local, Timelike};

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

    // --- is_named_mob / best_display_name ---

    #[test]
    fn named_mob_uppercase_promoted() {
        let names = vec!["a skeleton".to_owned(), "Bonecracker".to_owned()];
        assert_eq!(best_display_name(&names, "a skeleton"), "Bonecracker");
    }

    #[test]
    fn named_mob_hash_prefix_promoted() {
        let names = vec!["a rat".to_owned(), "#RareRat".to_owned()];
        assert_eq!(best_display_name(&names, "a rat"), "#RareRat");
    }

    #[test]
    fn best_display_name_falls_back_when_no_named_mob() {
        let names = vec!["a skeleton".to_owned(), "an orc".to_owned()];
        assert_eq!(best_display_name(&names, "a skeleton"), "a skeleton");
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

        let mut npc = make_npc(1, "Fippy Darkpaw", 100.0, 200.0);
        let store = make_store(vec![npc.clone()]);

        observer.process_diff(&ids(&[1]), &store, "blackburrow", &mut timers);

        npc.x = 300.0;
        npc.y = 400.0;
        let moved_store = make_store(vec![npc]);

        observer.process_diff(&ids(&[]), &moved_store, "blackburrow", &mut timers);

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

        observer.process_diff(&ids(&[1]), &store, "blackburrow", &mut timers);
        observer.process_diff(&ids(&[]), &store, "blackburrow", &mut timers);
        observer.pending_kills.get_mut(&key).unwrap().killed_at =
            Utc::now() - Duration::seconds(600);

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

        observer.process_diff(&ids(&[1]), &store, zone, &mut timers);
        observer.process_diff(&ids(&[]), &store, zone, &mut timers);
        observer.pending_kills.get_mut(&key).unwrap().killed_at =
            Utc::now() - Duration::seconds(600);
        let (promoted, _) = observer.process_diff(&ids(&[1]), &store, zone, &mut timers);
        assert!(!promoted);

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
        assert_eq!(t.spawn_loc, key);
        assert!(!t.all_names.is_empty());
    }

    #[test]
    fn process_diff_named_mob_promoted_over_placeholder() {
        let mut observer = SpawnObserver::default();
        let mut timers = TimerStore::default();
        let placeholder = make_npc(1, "a skeleton", 100.0, 200.0);
        let named = make_npc(2, "Bonecracker", 100.0, 200.0);
        let key = loc_key(100.0, 200.0);
        let zone = "unrest";

        let store_ph = make_store(vec![placeholder.clone()]);
        let store_nm = make_store(vec![named.clone()]);

        // Cycle 1: placeholder → gone; named mob respawns (spawn_count=1, no promotion yet)
        observer.process_diff(&ids(&[1]), &store_ph, zone, &mut timers);
        observer.process_diff(&ids(&[]), &store_ph, zone, &mut timers);
        observer.pending_kills.get_mut(&key).unwrap().killed_at =
            Utc::now() - Duration::seconds(600);
        observer.process_diff(&ids(&[2]), &store_nm, zone, &mut timers);

        // Cycle 2: named mob → gone; placeholder respawns (spawn_count=2, promoted)
        observer.process_diff(&ids(&[]), &store_nm, zone, &mut timers);
        observer.pending_kills.get_mut(&key).unwrap().killed_at =
            Utc::now() - Duration::seconds(600);
        let (promoted, _) = observer.process_diff(&ids(&[1]), &store_ph, zone, &mut timers);

        assert!(promoted);
        assert_eq!(timers.len(), 1);
        // Named mob "Bonecracker" should be promoted as display name even though
        // the placeholder respawned this cycle, because it's in all_names
        assert_eq!(timers.timers[0].name, "Bonecracker");
    }

    #[test]
    fn process_diff_skips_respawn_when_interval_too_short() {
        let mut observer = SpawnObserver::default();
        let mut timers = TimerStore::default();
        let npc = make_npc(1, "Fast Spawn", 100.0, 200.0);
        let store = make_store(vec![npc]);

        observer.process_diff(&ids(&[1]), &store, "crushbone", &mut timers);
        observer.process_diff(&ids(&[]), &store, "crushbone", &mut timers);
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

        observer.process_diff(&ids(&[1]), &store, zone, &mut timers);
        observer.process_diff(&ids(&[]), &store, zone, &mut timers);
        observer.pending_kills.get_mut(&key).unwrap().killed_at =
            Utc::now() - Duration::seconds(300);
        observer.process_diff(&ids(&[1]), &store, zone, &mut timers);

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
                x: 0.0,
                y: 0.0,
                z: 0.0,
                spawn_count: 1,
                intervals: vec![600],
                names: vec!["Orc".to_owned()],
            },
        );

        observer.on_zone_change();

        assert!(observer.pending_kills.is_empty());
        assert!(observer.prev_tick_npcs.is_empty());
        assert!(
            !observer.observations.is_empty(),
            "observations survive zone change"
        );
    }

    #[test]
    fn reset_zone_clears_everything_including_observations() {
        let mut observer = SpawnObserver::default();
        observer.observations.insert(
            "key".to_owned(),
            SpawnObservation {
                name: "NPC".to_owned(),
                x: 0.0,
                y: 0.0,
                z: 0.0,
                spawn_count: 3,
                intervals: vec![600, 610],
                names: vec!["NPC".to_owned()],
            },
        );
        observer.pending_kills.insert(
            "key".to_owned(),
            PendingKill {
                name: "NPC".to_owned(),
                x: 0.0,
                y: 0.0,
                z: 0.0,
                killed_at: Utc::now(),
            },
        );

        observer.reset_zone();

        assert!(observer.observations.is_empty());
        assert!(observer.pending_kills.is_empty());
        assert!(observer.prev_tick_npcs.is_empty());
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
                x: 100.0,
                y: 200.0,
                z: -5.0,
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
                name: "test".to_owned(),
                x: 0.0,
                y: 0.0,
                z: 0.0,
                spawn_count: 1,
                intervals: vec![600],
                names: vec![],
            },
        );

        SpawnObserver::save(&obs, "nexus", &dir_str).unwrap();

        assert!(
            !std::path::Path::new(&dir_str)
                .join("obs-nexus.txt")
                .exists()
        );
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
        t.all_names = vec!["Lord Nagafen".to_owned(), "a dragon".to_owned()];
        store.add(t);
        store.save("nagafen", &dir_str).unwrap();

        let loaded = TimerStore::load("nagafen", &dir_str);
        assert_eq!(loaded.len(), 1);
        let t = &loaded.timers[0];
        assert!(t.is_auto);
        assert_eq!(t.spawn_count, 5);
        assert_eq!(t.all_names, vec!["Lord Nagafen", "a dragon"]);
        assert_eq!(t.spawn_loc, loc_key(100.5, 200.0));
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

    #[test]
    fn timer_store_migrates_old_rust_format() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_string_lossy().into_owned();
        let path = dir.path().join("spawns-najena.txt");

        // Write old Rust format line
        let killed_unix = Utc::now().timestamp() - 900;
        let spawn_time_unix = Utc::now().timestamp() - 300;
        std::fs::write(
            &path,
            format!(
                "Lord Nagafen;100.5;200.0;-50.0;{killed_unix};1800;1;3;{spawn_time_unix};najena\n"
            ),
        )
        .unwrap();

        let loaded = TimerStore::load("najena", &dir_str);
        assert_eq!(loaded.len(), 1);
        let t = &loaded.timers[0];
        assert_eq!(t.name, "Lord Nagafen");
        assert_eq!(t.respawn_secs, 1800);
        assert_eq!(t.spawn_count, 3);
        assert!(t.is_auto); // spawn_count >= 2
        assert_eq!(t.spawn_loc, loc_key(100.5, 200.0));

        // Verify the file was re-saved in new format
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains(','),
            "new format spawn_loc should contain comma"
        );
    }

    #[test]
    fn timer_store_migrates_cs_format() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_string_lossy().into_owned();
        let path = dir.path().join("spawns-nagafen.txt");

        // Write C# format line (SpawnTimeStr is a datetime string, not integer)
        std::fs::write(
            &path,
            "200.000,100.500;3;1800;5/31/2026 3:00:00 PM;5/31/2026 2:00:00 PM;;Lord Nagafen;Lord Nagafen,a dragon;100.5;200.0;-50.0\n",
        )
        .unwrap();

        let loaded = TimerStore::load("nagafen", &dir_str);
        assert_eq!(loaded.len(), 1);
        let t = &loaded.timers[0];
        assert_eq!(t.name, "Lord Nagafen");
        assert_eq!(t.respawn_secs, 1800);
        assert_eq!(t.spawn_count, 3);
        assert!(t.is_auto);
        assert_eq!(t.all_names, vec!["Lord Nagafen", "a dragon"]);

        // Verify re-saved in new format
        let content = std::fs::read_to_string(&path).unwrap();
        // New format: field[3] is a unix timestamp (integer), not a date string
        let first_line = content.lines().next().unwrap();
        let fields: Vec<&str> = first_line.splitn(12, ';').collect();
        assert!(
            fields[3].parse::<i64>().is_ok(),
            "field[3] should be unix int after migration"
        );
    }

    // --- SpawnTimer helpers ---

    #[test]
    fn countdown_str_formats_correctly() {
        let mut t = SpawnTimer::new("Boss", 0.0, 0.0, 0.0, 3661);
        t.killed_at = Some(Utc::now());
        let s = t.countdown_str();
        assert!(s.starts_with('1'), "expected 1:01:01 format, got {s}");
    }

    #[test]
    fn countdown_str_shows_spawned_when_expired() {
        let mut t = SpawnTimer::new("Boss", 0.0, 0.0, 0.0, 0);
        t.killed_at = Some(Utc::now() - Duration::seconds(60));
        assert_eq!(t.countdown_str(), "UNKNOWN");
    }

    #[test]
    fn countdown_str_shows_alive_when_no_kill_time() {
        let mut t = SpawnTimer::new("Boss", 0.0, 0.0, 0.0, 1800);
        t.killed_at = None;
        assert_eq!(t.countdown_str(), "ALIVE");
    }

    #[test]
    fn add_replaces_existing_timer_at_same_location() {
        let mut store = TimerStore::default();
        store.add(SpawnTimer::new("Fippy", 0.0, 0.0, 0.0, 600));
        store.add(SpawnTimer::new("Fippy", 0.0, 0.0, 0.0, 900));
        assert_eq!(store.len(), 1);
        assert_eq!(store.timers[0].respawn_secs, 900);
    }

    #[test]
    fn add_keeps_timers_at_different_locations() {
        let mut store = TimerStore::default();
        store.add(SpawnTimer::new("Fippy", 0.0, 0.0, 0.0, 600));
        store.add(SpawnTimer::new("Fippy", 100.0, 200.0, 0.0, 900));
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn remove_by_index() {
        let mut store = TimerStore::default();
        store.add(SpawnTimer::new("A", 0.0, 0.0, 0.0, 600));
        store.add(SpawnTimer::new("B", 1.0, 0.0, 0.0, 600));
        store.remove(0);
        assert_eq!(store.len(), 1);
        assert_eq!(store.timers[0].name, "B");
    }

    #[test]
    fn clear_all_empties_the_store() {
        let mut store = TimerStore::default();
        store.add(SpawnTimer::new("A", 0.0, 0.0, 0.0, 600));
        store.add(SpawnTimer::new("B", 1.0, 0.0, 0.0, 900));
        store.clear_all();
        assert!(store.is_empty());
    }

    // --- parse_cs_datetime ---

    #[test]
    fn parse_cs_datetime_handles_pm() {
        let dt = parse_cs_datetime("5/31/2026 3:45:23 PM").unwrap();
        assert_eq!(dt.hour(), 15);
        assert_eq!(dt.minute(), 45);
        assert_eq!(dt.second(), 23);
    }

    #[test]
    fn parse_cs_datetime_handles_midnight() {
        let dt = parse_cs_datetime("1/1/2026 12:00:00 AM").unwrap();
        assert_eq!(dt.hour(), 0);
    }

    #[test]
    fn parse_cs_datetime_returns_none_for_empty() {
        assert!(parse_cs_datetime("").is_none());
    }
}
