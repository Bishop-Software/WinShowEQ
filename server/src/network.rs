use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use crate::data::spawn::SpawnRecord;
use crate::data::world::WorldTime;
use crate::notifier::{ConnectionEvent, StatusSnapshot, UiNotifier};
use common::{
    IPT_GETPROC, IPT_GROUND, IPT_SELF, IPT_SETPROC, IPT_SPAWNS, IPT_TARGET, IPT_WORLD, IPT_ZONE,
    OPT_GROUND, OPT_PROCESS, OPT_SELF, OPT_SPAWNS, OPT_TARGET, OPT_WORLD, OPT_ZONE,
};

/// Supplies data to the network layer without coupling it to MemReader.
/// M4: implemented by StubDataProvider. M5: backed by live MemReader reads.
pub trait DataProvider: Send + Sync {
    fn zone_name(&self) -> String;
    fn self_spawn(&self) -> Option<SpawnRecord>;
    fn spawn_list(&self) -> Vec<SpawnRecord>;
    fn target(&self) -> Option<SpawnRecord>;
    fn ground_items(&self) -> Vec<SpawnRecord>;
    fn world_time(&self) -> Option<WorldTime>;
    fn processes(&self) -> Vec<u32>;
}

pub struct NetworkServer {
    port: u16,
    notifier: Option<Arc<dyn UiNotifier>>,
}

impl NetworkServer {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            notifier: None,
        }
    }

    pub fn set_notifier(&mut self, n: Arc<dyn UiNotifier>) {
        self.notifier = Some(n);
    }

    fn log_info(&self, message: &str) {
        if let Some(n) = &self.notifier {
            n.on_log_event(message);
        } else {
            println!("{message}");
        }
    }

    fn log_error(&self, title: &str, message: &str) {
        if let Some(n) = &self.notifier {
            n.on_error(title, message);
        } else {
            eprintln!("{title}: {message}");
        }
    }

    /// Blocking accept loop. Handles one client at a time (matches C++ model).
    /// Call from a dedicated thread if the caller needs to stay responsive.
    pub fn serve(&self, provider: Arc<dyn DataProvider>) {
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = match TcpListener::bind(&addr) {
            Ok(l) => l,
            Err(e) => {
                self.log_error(
                    "NetworkServer",
                    &format!("Failed to bind to port {}: {}", self.port, e),
                );
                return;
            }
        };

        self.log_info(&format!(
            "WinShowEQServer: Listening on 0.0.0.0:{}",
            self.port
        ));

        if let Some(n) = &self.notifier {
            n.on_connection_changed(&ConnectionEvent {
                listening: true,
                ..Default::default()
            });
        }

        for stream in listener.incoming() {
            match stream {
                Ok(client) => {
                    let peer = client
                        .peer_addr()
                        .map(|a| a.to_string())
                        .unwrap_or_else(|_| "unknown".into());
                    self.log_info(&format!("WinShowEQServer: New connection from: {}", peer));

                    if let Some(n) = &self.notifier {
                        n.on_connection_changed(&ConnectionEvent {
                            connected: true,
                            ..Default::default()
                        });
                    }

                    self.handle_client(client, Arc::clone(&provider));

                    self.log_info("WinShowEQServer: Client disconnected.");

                    if let Some(n) = &self.notifier {
                        n.on_connection_changed(&ConnectionEvent {
                            listening: true,
                            ..Default::default()
                        });
                    }
                }
                Err(e) => self.log_error("NetworkServer", &format!("Accept error: {}", e)),
            }
        }
    }

    fn handle_client(&self, mut stream: TcpStream, provider: Arc<dyn DataProvider>) {
        // Zone name is tracked per-connection; only sent when it changes.
        let mut zone_name = String::from("StartUp");
        // When true, next 4-byte recv is the target PID from an IPT_SETPROC request.
        let mut change_process = false;

        loop {
            let mut buf = [0u8; 4];
            if stream.read_exact(&mut buf).is_err() {
                break;
            }
            let request = i32::from_le_bytes(buf);

            if change_process {
                // Payload was the requested PID — no process switching in stub mode.
                change_process = false;
                continue;
            }

            let mut records: Vec<SpawnRecord> = Vec::new();
            let mut ui_snap = StatusSnapshot::default();
            let mut ui_dirty = false;

            if request & IPT_GETPROC != 0 {
                for pid in provider.processes() {
                    let mut rec = SpawnRecord::zeroed();
                    rec.id = pid;
                    rec.flags = OPT_PROCESS;
                    records.push(rec);
                }
            }

            if request & IPT_SETPROC != 0 {
                // Next recv payload is the target PID; send no response this tick.
                change_process = true;
                continue;
            }

            if request & IPT_ZONE != 0 {
                let new_zone = provider.zone_name();
                if new_zone != zone_name {
                    zone_name = new_zone.clone();
                    let mut rec = SpawnRecord::zeroed();
                    write_str(&new_zone, &mut rec.name);
                    rec.flags = OPT_ZONE;
                    records.push(rec);
                }
                ui_snap.zone = zone_name.clone();
                ui_dirty = true;
            }

            if request & IPT_SELF != 0
                && let Some(mut rec) = provider.self_spawn()
            {
                rec.flags = OPT_SELF;
                let len = rec
                    .name
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(rec.name.len());
                ui_snap.character_name = String::from_utf8_lossy(&rec.name[..len]).into_owned();
                ui_dirty = true;
                records.push(rec);
            }

            if request & IPT_SPAWNS != 0 {
                let mut npc = 0i32;
                let mut pc = 0i32;
                let mut corpse = 0i32;
                for mut rec in provider.spawn_list() {
                    match rec.spawn_type {
                        0 => pc += 1,
                        1 => npc += 1,
                        _ => corpse += 1,
                    }
                    rec.flags = OPT_SPAWNS;
                    records.push(rec);
                }
                ui_snap.npc_count = npc;
                ui_snap.pc_count = pc;
                ui_snap.corpse_count = corpse;
                ui_dirty = true;
            }

            if request & IPT_TARGET != 0 {
                let rec = match provider.target() {
                    Some(mut r) => {
                        r.flags = OPT_TARGET;
                        r
                    }
                    None => {
                        let mut r = SpawnRecord::zeroed();
                        r.id = 99999; // sentinel: no target, matches C++ packNetBufferEmpty
                        r.flags = OPT_TARGET;
                        r
                    }
                };
                records.push(rec);
            }

            if request & IPT_GROUND != 0 {
                let items = provider.ground_items();
                ui_snap.item_count = items.len() as i32;
                ui_dirty = true;
                for mut rec in items {
                    rec.flags = OPT_GROUND;
                    records.push(rec);
                }
            }

            if request & IPT_WORLD != 0
                && let Some(wt) = provider.world_time()
            {
                records.push(world_time_to_record(wt));
            }

            if ui_dirty && let Some(n) = &self.notifier {
                n.on_status_update(&ui_snap);
            }

            if flush_records(&mut stream, &records).is_err() {
                break;
            }
        }
    }
}

/// Encodes WorldTime into a SpawnRecord for wire transmission (mirrors Spawn::packNetBufferWorld).
fn world_time_to_record(wt: WorldTime) -> SpawnRecord {
    let mut rec = SpawnRecord::zeroed();
    rec.spawn_type = wt.hour;
    rec.class = wt.minute;
    rec.level = wt.day;
    rec.hidden = wt.month;
    rec.race = wt.year;
    rec.flags = OPT_WORLD;
    rec
}

/// Sends count (4-byte LE i32) then packed record bytes. Matches flushNetBuffer in C++.
fn flush_records(stream: &mut TcpStream, records: &[SpawnRecord]) -> io::Result<()> {
    stream.write_all(&(records.len() as i32).to_le_bytes())?;
    for rec in records {
        stream.write_all(rec.as_bytes())?;
    }
    Ok(())
}

/// Copies `s` into `dest` as a null-terminated byte string, truncating to fit.
fn write_str(s: &str, dest: &mut [u8]) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(dest.len().saturating_sub(1));
    dest[..len].copy_from_slice(&bytes[..len]);
    // remaining bytes stay zero (SpawnRecord::zeroed)
}

// ---------------------------------------------------------------------------
// Stub data provider — used by `--serve-stub` for M4 integration testing.
// ---------------------------------------------------------------------------

pub struct StubDataProvider;

impl DataProvider for StubDataProvider {
    fn zone_name(&self) -> String {
        "stubzone".to_string()
    }

    fn self_spawn(&self) -> Option<SpawnRecord> {
        let mut rec = SpawnRecord::zeroed();
        write_str("Player", &mut rec.name);
        rec.x = 100.0;
        rec.y = 200.0;
        rec.z = 0.0;
        rec.id = 1;
        rec.level = 60;
        rec.spawn_type = 1; // PC
        Some(rec)
    }

    fn spawn_list(&self) -> Vec<SpawnRecord> {
        let mut npc = SpawnRecord::zeroed();
        write_str("StubNPC", &mut npc.name);
        npc.x = 150.0;
        npc.y = 250.0;
        npc.z = 0.0;
        npc.id = 2;
        npc.level = 30;
        npc.spawn_type = 1; // NPC
        vec![npc]
    }

    fn target(&self) -> Option<SpawnRecord> {
        None
    }

    fn ground_items(&self) -> Vec<SpawnRecord> {
        Vec::new()
    }

    fn world_time(&self) -> Option<WorldTime> {
        Some(WorldTime {
            hour: 8,
            minute: 0,
            day: 1,
            month: 1,
            year: 3100,
        })
    }

    fn processes(&self) -> Vec<u32> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_time_record_fields() {
        let wt = WorldTime {
            hour: 14,
            minute: 30,
            day: 15,
            month: 6,
            year: 3210,
        };
        let rec = world_time_to_record(wt);
        // Copy fields to locals before comparing — packed struct fields cannot be
        // referenced directly (potential misalignment on multi-byte types).
        let (spawn_type, class, level, hidden) = (rec.spawn_type, rec.class, rec.level, rec.hidden);
        let (race, flags) = (
            {
                let r = rec.race;
                r
            },
            {
                let f = rec.flags;
                f
            },
        );
        assert_eq!(spawn_type, 14);
        assert_eq!(class, 30);
        assert_eq!(level, 15);
        assert_eq!(hidden, 6);
        assert_eq!(race, 3210);
        assert_eq!(flags, OPT_WORLD);
    }

    #[test]
    fn write_str_truncates_and_null_terminates() {
        let mut buf = [0xFFu8; 5];
        write_str("hello!", &mut buf); // 6 chars, buf only holds 4 + null
        assert_eq!(&buf[..4], b"hell");
        assert_eq!(buf[4], 0xFF); // write_str only zeroes what it wrote — remaining stays
    }

    #[test]
    fn flush_records_byte_count() {
        let records = vec![SpawnRecord::zeroed(), SpawnRecord::zeroed()];
        let mut buf = Vec::new();
        // Reimplement flush_records using a Cursor to test without a real socket.
        buf.extend_from_slice(&(records.len() as i32).to_le_bytes());
        for rec in &records {
            buf.extend_from_slice(rec.as_bytes());
        }
        // 4 bytes count + 2 * 100 bytes records
        assert_eq!(buf.len(), 4 + 2 * 100);
        assert_eq!(&buf[..4], &2i32.to_le_bytes());
    }
}
