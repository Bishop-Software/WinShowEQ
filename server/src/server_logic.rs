use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use common::{OPT_GROUND, OPT_SELF, OPT_SPAWNS, OPT_TARGET, SpawnRecord, WorldTime};

use crate::config::{IniReader, PrimaryOffsets, ServerConfigModel};
use crate::data::spawn_offsets::{ItemOffsets, SpawnOffsets, WorldOffsets};
use crate::mem_reader::MemReader;
use crate::network::DataProvider;
use crate::notifier::UiNotifier;

// --------------------------------------------------------------------------
// Raw-byte field extraction helpers
// Mirrors the extractRaw* family in Spawn.h / Item.h / World.h.
// --------------------------------------------------------------------------

pub(crate) fn read_u8_at(buf: &[u8], off: usize) -> u8 {
    buf.get(off).copied().unwrap_or(0)
}

pub(crate) fn read_u16_at(buf: &[u8], off: usize) -> u16 {
    buf.get(off..off + 2)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
        .unwrap_or(0)
}

pub(crate) fn read_u32_at(buf: &[u8], off: usize) -> u32 {
    buf.get(off..off + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
        .unwrap_or(0)
}

pub(crate) fn read_u64_at(buf: &[u8], off: usize) -> u64 {
    buf.get(off..off + 8)
        .and_then(|s| s.try_into().ok())
        .map(u64::from_le_bytes)
        .unwrap_or(0)
}

pub(crate) fn read_f32_at(buf: &[u8], off: usize) -> f32 {
    f32::from_bits(read_u32_at(buf, off))
}

/// Copy a null-terminated byte string from `buf[off..]` into `dest`, leaving a null terminator.
fn copy_str_into(buf: &[u8], off: usize, dest: &mut [u8]) {
    if off >= buf.len() || dest.is_empty() {
        return;
    }
    let src = &buf[off..];
    let len = src.iter().position(|&b| b == 0).unwrap_or(src.len());
    let copy = len.min(dest.len() - 1);
    dest[..copy].copy_from_slice(&src[..copy]);
}

// --------------------------------------------------------------------------
// Spawn record packing from a raw EQ memory buffer
// Mirrors Spawn::packNetBufferRaw() in Spawn.cpp.
// --------------------------------------------------------------------------

pub(crate) fn extract_spawn_record(buf: &[u8], offs: &SpawnOffsets, flags: u32) -> SpawnRecord {
    let mut rec = SpawnRecord::zeroed();
    copy_str_into(buf, offs.name, &mut rec.name);
    copy_str_into(buf, offs.last_name, &mut rec.last_name);
    rec.x = read_f32_at(buf, offs.x);
    rec.y = read_f32_at(buf, offs.y);
    rec.z = read_f32_at(buf, offs.z);
    rec.heading = read_f32_at(buf, offs.heading);
    rec.speed = read_f32_at(buf, offs.speed);
    rec.spawn_type = read_u8_at(buf, offs.type_);
    rec.class = read_u8_at(buf, offs.class);
    rec.level = read_u8_at(buf, offs.level);
    rec.hidden = read_u8_at(buf, offs.hidden);
    if offs.race8 {
        rec.race = read_u8_at(buf, offs.race) as u32;
        rec.id = read_u16_at(buf, offs.id) as u32;
        rec.owner = read_u16_at(buf, offs.owner) as u32;
        rec.primary = read_u16_at(buf, offs.primary) as u32;
        rec.offhand = read_u16_at(buf, offs.offhand) as u32;
    } else {
        rec.race = read_u32_at(buf, offs.race);
        rec.id = read_u32_at(buf, offs.id);
        rec.owner = read_u32_at(buf, offs.owner);
        rec.primary = read_u32_at(buf, offs.primary);
        rec.offhand = read_u32_at(buf, offs.offhand);
    }
    rec.flags = flags;
    rec
}

/// Pack a raw item buffer into a SpawnRecord for wire transmission.
/// Mirrors Spawn::packNetBufferFrom(Item) in Spawn.cpp.
fn pack_item_as_spawn(buf: &[u8], offs: &ItemOffsets) -> SpawnRecord {
    let mut rec = SpawnRecord::zeroed();
    copy_str_into(buf, offs.name, &mut rec.name);
    rec.x = read_f32_at(buf, offs.x);
    rec.y = read_f32_at(buf, offs.y);
    rec.z = read_f32_at(buf, offs.z);
    rec.id = read_u32_at(buf, offs.id);
    rec.flags = OPT_GROUND;
    rec
}

/// Extract WorldTime from a raw world buffer.
/// Mirrors World::packWorldBuffer() in World.cpp.
fn extract_world_time(buf: &[u8], offs: &WorldOffsets) -> WorldTime {
    WorldTime {
        hour: read_u8_at(buf, offs.hour),
        minute: read_u8_at(buf, offs.minute),
        day: read_u8_at(buf, offs.day),
        month: read_u8_at(buf, offs.month),
        year: if offs.race8 {
            read_u16_at(buf, offs.year) as u32
        } else {
            read_u32_at(buf, offs.year)
        },
    }
}

// --------------------------------------------------------------------------
// LiveOffsets — hot-reloadable offset bundle
// --------------------------------------------------------------------------

#[derive(Clone)]
pub struct LiveOffsets {
    pub primary: PrimaryOffsets,
    pub spawn_off: SpawnOffsets,
    pub item_off: ItemOffsets,
    pub world_off: WorldOffsets,
}

/// Read all offsets from the two INI files. Used at startup and on reload.
pub fn load_live_offsets(ini_path: &str, config_ini_path: &str) -> Result<LiveOffsets, String> {
    let mut ir = IniReader::new();
    ir.open_config_file(config_ini_path);
    ir.open_file(ini_path)?;
    let model = ir.read_server_config_model()?;
    Ok(LiveOffsets {
        primary: model.offsets,
        spawn_off: SpawnOffsets::from_ini(&ir),
        item_off: ItemOffsets::from_ini(&ir),
        world_off: WorldOffsets::from_ini(&ir),
    })
}

// --------------------------------------------------------------------------
// MemDataProvider — live DataProvider backed by ReadProcessMemory
// --------------------------------------------------------------------------

pub struct MemDataProvider {
    mem: Arc<Mutex<MemReader>>,
    offsets: Arc<Mutex<LiveOffsets>>,
    ini_path: String,
    config_ini_path: String,
    reload_flag: Arc<AtomicBool>,
    /// Throttle counter for reattach attempts (retries on value % 10 == 2).
    check_ctr: AtomicU32,
    notifier: Option<Arc<dyn UiNotifier>>,
}

impl MemDataProvider {
    pub fn new(
        mem: Arc<Mutex<MemReader>>,
        live: LiveOffsets,
        ini_path: String,
        config_ini_path: String,
        reload_flag: Arc<AtomicBool>,
        notifier: Option<Arc<dyn UiNotifier>>,
    ) -> Self {
        Self {
            mem,
            offsets: Arc::new(Mutex::new(live)),
            ini_path,
            config_ini_path,
            reload_flag,
            check_ctr: AtomicU32::new(0),
            notifier,
        }
    }

    fn check_and_reload(&self) {
        if !self.reload_flag.swap(false, Ordering::Relaxed) {
            return;
        }
        match load_live_offsets(&self.ini_path, &self.config_ini_path) {
            Ok(new_offs) => {
                *self.offsets.lock().unwrap() = new_offs;
                if let Some(n) = &self.notifier {
                    n.on_log_event("Offsets reloaded from INI.");
                }
            }
            Err(e) => {
                if let Some(n) = &self.notifier {
                    n.on_log_event(&format!("[ERROR] Reload offsets failed: {e}"));
                }
            }
        }
    }
}

impl DataProvider for MemDataProvider {
    fn zone_name(&self) -> String {
        self.check_and_reload();
        let zone_name_addr = self.offsets.lock().unwrap().primary.zone_name;
        let mut mem = self.mem.lock().unwrap();
        if !try_attach(&mut mem, &self.check_ctr, self.notifier.as_ref()) {
            return "StartUp".to_string();
        }
        if zone_name_addr == 0 {
            return "StartUp".to_string();
        }
        let remapped = mem.canonical_to_actual(zone_name_addr);
        mem.read_string(remapped, 64)
            .unwrap_or_else(|_| "StartUp".to_string())
    }

    fn self_spawn(&self) -> Option<SpawnRecord> {
        let (self_addr, spawn_off) = {
            let o = self.offsets.lock().unwrap();
            (o.primary.self_addr, o.spawn_off.clone())
        };
        let mut mem = self.mem.lock().unwrap();
        if !try_attach(&mut mem, &self.check_ctr, self.notifier.as_ref()) {
            return None;
        }
        if self_addr == 0 {
            return None;
        }
        let ptr = mem.read_raw_pointer(self_addr).ok()?;
        if ptr == 0 {
            return None;
        }
        let buf = mem.read_bytes(ptr, spawn_off.buf_size).ok()?;
        Some(extract_spawn_record(&buf, &spawn_off, OPT_SELF))
    }

    fn spawn_list(&self) -> Vec<SpawnRecord> {
        let (spawn_list_addr, spawn_off) = {
            let o = self.offsets.lock().unwrap();
            (o.primary.spawn_list, o.spawn_off.clone())
        };
        let mut mem = self.mem.lock().unwrap();
        if !try_attach(&mut mem, &self.check_ctr, self.notifier.as_ref()) {
            return Vec::new();
        }
        if spawn_list_addr == 0 {
            return Vec::new();
        }
        let Ok(mut ptr) = mem.read_raw_pointer(spawn_list_addr) else {
            return Vec::new();
        };
        if ptr == 0 {
            return Vec::new();
        }

        // Walk backward to the true head of the list. After TSS shrouds/hover, the
        // INI pointer may land mid-list — matches C++ handleSpawnList() back-walk.
        let buf_size = spawn_off.buf_size;
        let prev_off = spawn_off.prev;
        for _ in 0..2000 {
            let Ok(buf) = mem.read_bytes(ptr, buf_size) else {
                break;
            };
            let prev = read_u64_at(&buf, prev_off);
            if prev == 0 {
                break;
            }
            ptr = prev;
        }

        // Walk forward collecting records.
        let next_off = spawn_off.next;
        let mut records = Vec::new();
        loop {
            if ptr == 0 {
                break;
            }
            let Ok(buf) = mem.read_bytes(ptr, buf_size) else {
                break;
            };
            records.push(extract_spawn_record(&buf, &spawn_off, OPT_SPAWNS));
            let next = read_u64_at(&buf, next_off);
            if next == 0 || next == ptr {
                break;
            }
            ptr = next;
        }
        records
    }

    fn target(&self) -> Option<SpawnRecord> {
        let (target_addr, spawn_off) = {
            let o = self.offsets.lock().unwrap();
            (o.primary.target, o.spawn_off.clone())
        };
        let mut mem = self.mem.lock().unwrap();
        if !try_attach(&mut mem, &self.check_ctr, self.notifier.as_ref()) {
            return None;
        }
        if target_addr == 0 {
            return None;
        }
        let ptr = mem.read_raw_pointer(target_addr).ok()?;
        if ptr == 0 {
            return None;
        }
        let buf = mem.read_bytes(ptr, spawn_off.buf_size).ok()?;
        Some(extract_spawn_record(&buf, &spawn_off, OPT_TARGET))
    }

    fn ground_items(&self) -> Vec<SpawnRecord> {
        let (ground_addr, item_off) = {
            let o = self.offsets.lock().unwrap();
            (o.primary.ground, o.item_off.clone())
        };
        let mut mem = self.mem.lock().unwrap();
        if !try_attach(&mut mem, &self.check_ctr, self.notifier.as_ref()) {
            return Vec::new();
        }
        if ground_addr == 0 {
            return Vec::new();
        }
        let Ok(base_ptr) = mem.read_raw_pointer(ground_addr) else {
            return Vec::new();
        };
        if base_ptr == 0 {
            return Vec::new();
        }

        // If the name at base_ptr+nameOff starts with "IT", base_ptr is a direct item.
        // Otherwise base_ptr is a container that holds a pointer to the first item.
        // Mirrors NetworkServer::handleGroundItems() pointer disambiguation.
        let name_check_addr = base_ptr + item_off.name as u64;
        let first_name = mem.read_string(name_check_addr, 4).unwrap_or_default();
        let mut ptr = if first_name.starts_with("IT") {
            base_ptr
        } else {
            mem.read_pointer(base_ptr).unwrap_or(0)
        };

        let buf_size = item_off.buf_size;
        let next_off = item_off.next;
        let mut records = Vec::new();
        let mut count = 0u32;
        loop {
            if ptr == 0 || count >= 300 {
                break;
            }
            let Ok(buf) = mem.read_bytes(ptr, buf_size) else {
                break;
            };
            records.push(pack_item_as_spawn(&buf, &item_off));
            let next = read_u64_at(&buf, next_off);
            if next == 0 || next == ptr {
                break;
            }
            ptr = next;
            count += 1;
        }
        records
    }

    fn world_time(&self) -> Option<WorldTime> {
        let (world_addr, world_off) = {
            let o = self.offsets.lock().unwrap();
            (o.primary.world, o.world_off.clone())
        };
        let mut mem = self.mem.lock().unwrap();
        if !try_attach(&mut mem, &self.check_ctr, self.notifier.as_ref()) {
            return None;
        }
        if world_addr == 0 {
            return None;
        }
        let ptr = mem.read_raw_pointer(world_addr).ok()?;
        if ptr == 0 {
            return None;
        }
        let buf = mem.read_bytes(ptr, world_off.buf_size).ok()?;
        Some(extract_world_time(&buf, &world_off))
    }

    fn processes(&self) -> Vec<u32> {
        let mem = self.mem.lock().unwrap();
        let pid = mem.pid();
        if pid == 0 { Vec::new() } else { vec![pid] }
    }
}

/// Check if MemReader is attached; try to reattach if not.
/// Retries every ~10th call to avoid hammering the process list.
/// Mirrors the check_delay logic in ServerLogic::onDataReceived().
fn try_attach(
    mem: &mut MemReader,
    counter: &AtomicU32,
    notifier: Option<&Arc<dyn UiNotifier>>,
) -> bool {
    if mem.is_valid() {
        return true;
    }
    let n = counter.fetch_add(1, Ordering::Relaxed);
    if n % 10 == 2
        && let Some(pid) = MemReader::find_process("eqgame.exe")
        && mem.open(pid).is_ok()
    {
        if let Some(notifier) = notifier {
            notifier.on_log_event(&format!("Attached to eqgame.exe PID={pid}"));
        } else {
            println!("[STATE] Attached to eqgame.exe PID={pid}");
        }
    }
    mem.pid() != 0
}

// --------------------------------------------------------------------------
// ServerLogic — config loading and offset reload
// --------------------------------------------------------------------------

pub struct ServerLogic {
    ini_path: String,
    config_ini_path: String,
}

impl ServerLogic {
    pub fn new(ini_path: String, config_ini_path: String) -> Self {
        Self {
            ini_path,
            config_ini_path,
        }
    }

    /// Load both INI files; return a configured IniReader and ServerConfigModel.
    /// Mirrors ServerLogic::loadIniAndConfig() in C++.
    pub fn load_config(&self) -> Result<(IniReader, ServerConfigModel), String> {
        let mut ir = IniReader::new();
        ir.open_config_file(&self.config_ini_path);
        ir.open_file(&self.ini_path)?;
        let model = ir.read_server_config_model()?;
        Ok((ir, model))
    }
}
