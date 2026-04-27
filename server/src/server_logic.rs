use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use common::{OPT_GROUND, OPT_SELF, OPT_SPAWNS, OPT_TARGET, SpawnRecord, WorldTime};

use crate::config::{IniReader, PrimaryOffsets, ServerConfigModel};
use crate::data::spawn_offsets::{ItemOffsets, SpawnOffsets, WorldOffsets};
use crate::mem_reader::MemReader;
use crate::network::DataProvider;

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
    rec.x       = read_f32_at(buf, offs.x);
    rec.y       = read_f32_at(buf, offs.y);
    rec.z       = read_f32_at(buf, offs.z);
    rec.heading = read_f32_at(buf, offs.heading);
    rec.speed   = read_f32_at(buf, offs.speed);
    rec.spawn_type = read_u8_at(buf, offs.type_);
    rec.class      = read_u8_at(buf, offs.class);
    rec.level      = read_u8_at(buf, offs.level);
    rec.hidden     = read_u8_at(buf, offs.hidden);
    if offs.race8 {
        rec.race    = read_u8_at(buf, offs.race) as u32;
        rec.id      = read_u16_at(buf, offs.id) as u32;
        rec.owner   = read_u16_at(buf, offs.owner) as u32;
        rec.primary = read_u16_at(buf, offs.primary) as u32;
        rec.offhand = read_u16_at(buf, offs.offhand) as u32;
    } else {
        rec.race    = read_u32_at(buf, offs.race);
        rec.id      = read_u32_at(buf, offs.id);
        rec.owner   = read_u32_at(buf, offs.owner);
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
    rec.x    = read_f32_at(buf, offs.x);
    rec.y    = read_f32_at(buf, offs.y);
    rec.z    = read_f32_at(buf, offs.z);
    rec.id   = read_u32_at(buf, offs.id);
    rec.flags = OPT_GROUND;
    rec
}

/// Extract WorldTime from a raw world buffer.
/// Mirrors World::packWorldBuffer() in World.cpp.
fn extract_world_time(buf: &[u8], offs: &WorldOffsets) -> WorldTime {
    WorldTime {
        hour:   read_u8_at(buf, offs.hour),
        minute: read_u8_at(buf, offs.minute),
        day:    read_u8_at(buf, offs.day),
        month:  read_u8_at(buf, offs.month),
        year:   if offs.race8 {
            read_u16_at(buf, offs.year) as u32
        } else {
            read_u32_at(buf, offs.year)
        },
    }
}

// --------------------------------------------------------------------------
// MemDataProvider — live DataProvider backed by ReadProcessMemory
// --------------------------------------------------------------------------

pub struct MemDataProvider {
    mem:       Arc<Mutex<MemReader>>,
    primary:   PrimaryOffsets,
    spawn_off: SpawnOffsets,
    item_off:  ItemOffsets,
    world_off: WorldOffsets,
    /// Throttle counter for reattach attempts (retries on value % 10 == 2).
    check_ctr: AtomicU32,
}

impl MemDataProvider {
    pub fn new(
        mem: Arc<Mutex<MemReader>>,
        primary: PrimaryOffsets,
        spawn_off: SpawnOffsets,
        item_off: ItemOffsets,
        world_off: WorldOffsets,
    ) -> Self {
        Self {
            mem,
            primary,
            spawn_off,
            item_off,
            world_off,
            check_ctr: AtomicU32::new(0),
        }
    }
}

impl DataProvider for MemDataProvider {
    fn zone_name(&self) -> String {
        let mut mem = self.mem.lock().unwrap();
        if !try_attach(&mut mem, &self.check_ctr) {
            return "StartUp".to_string();
        }
        let addr = self.primary.zone_name;
        if addr == 0 {
            return "StartUp".to_string();
        }
        let remapped = mem.canonical_to_actual(addr);
        mem.read_string(remapped, 64).unwrap_or_else(|_| "StartUp".to_string())
    }

    fn self_spawn(&self) -> Option<SpawnRecord> {
        let mut mem = self.mem.lock().unwrap();
        if !try_attach(&mut mem, &self.check_ctr) {
            return None;
        }
        let addr = self.primary.self_addr;
        if addr == 0 {
            return None;
        }
        let ptr = mem.read_raw_pointer(addr).ok()?;
        if ptr == 0 {
            return None;
        }
        let buf = mem.read_bytes(ptr, self.spawn_off.buf_size).ok()?;
        Some(extract_spawn_record(&buf, &self.spawn_off, OPT_SELF))
    }

    fn spawn_list(&self) -> Vec<SpawnRecord> {
        let mut mem = self.mem.lock().unwrap();
        if !try_attach(&mut mem, &self.check_ctr) {
            return Vec::new();
        }
        let addr = self.primary.spawn_list;
        if addr == 0 {
            return Vec::new();
        }
        let Ok(mut ptr) = mem.read_raw_pointer(addr) else {
            return Vec::new();
        };
        if ptr == 0 {
            return Vec::new();
        }

        // Walk backward to the true head of the list. After TSS shrouds/hover, the
        // INI pointer may land mid-list — matches C++ handleSpawnList() back-walk.
        let buf_size = self.spawn_off.buf_size;
        let prev_off = self.spawn_off.prev;
        for _ in 0..2000 {
            let Ok(buf) = mem.read_bytes(ptr, buf_size) else { break; };
            let prev = read_u64_at(&buf, prev_off);
            if prev == 0 {
                break;
            }
            ptr = prev;
        }

        // Walk forward collecting records.
        let next_off = self.spawn_off.next;
        let mut records = Vec::new();
        loop {
            if ptr == 0 {
                break;
            }
            let Ok(buf) = mem.read_bytes(ptr, buf_size) else { break; };
            records.push(extract_spawn_record(&buf, &self.spawn_off, OPT_SPAWNS));
            let next = read_u64_at(&buf, next_off);
            if next == 0 || next == ptr {
                break;
            }
            ptr = next;
        }
        records
    }

    fn target(&self) -> Option<SpawnRecord> {
        let mut mem = self.mem.lock().unwrap();
        if !try_attach(&mut mem, &self.check_ctr) {
            return None;
        }
        let addr = self.primary.target;
        if addr == 0 {
            return None;
        }
        let ptr = mem.read_raw_pointer(addr).ok()?;
        if ptr == 0 {
            return None;
        }
        let buf = mem.read_bytes(ptr, self.spawn_off.buf_size).ok()?;
        Some(extract_spawn_record(&buf, &self.spawn_off, OPT_TARGET))
    }

    fn ground_items(&self) -> Vec<SpawnRecord> {
        let mut mem = self.mem.lock().unwrap();
        if !try_attach(&mut mem, &self.check_ctr) {
            return Vec::new();
        }
        let addr = self.primary.ground;
        if addr == 0 {
            return Vec::new();
        }
        let Ok(base_ptr) = mem.read_raw_pointer(addr) else {
            return Vec::new();
        };
        if base_ptr == 0 {
            return Vec::new();
        }

        // If the name at base_ptr+nameOff starts with "IT", base_ptr is a direct item.
        // Otherwise base_ptr is a container that holds a pointer to the first item.
        // Mirrors NetworkServer::handleGroundItems() pointer disambiguation.
        let name_check_addr = base_ptr + self.item_off.name as u64;
        let first_name = mem.read_string(name_check_addr, 4).unwrap_or_default();
        let mut ptr = if first_name.starts_with("IT") {
            base_ptr
        } else {
            mem.read_pointer(base_ptr).unwrap_or(0)
        };

        let buf_size = self.item_off.buf_size;
        let next_off = self.item_off.next;
        let mut records = Vec::new();
        let mut count = 0u32;
        loop {
            if ptr == 0 || count >= 300 {
                break;
            }
            let Ok(buf) = mem.read_bytes(ptr, buf_size) else { break; };
            records.push(pack_item_as_spawn(&buf, &self.item_off));
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
        let mut mem = self.mem.lock().unwrap();
        if !try_attach(&mut mem, &self.check_ctr) {
            return None;
        }
        let addr = self.primary.world;
        if addr == 0 {
            return None;
        }
        let ptr = mem.read_raw_pointer(addr).ok()?;
        if ptr == 0 {
            return None;
        }
        let buf = mem.read_bytes(ptr, self.world_off.buf_size).ok()?;
        Some(extract_world_time(&buf, &self.world_off))
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
fn try_attach(mem: &mut MemReader, counter: &AtomicU32) -> bool {
    if mem.is_valid() {
        return true;
    }
    let n = counter.fetch_add(1, Ordering::Relaxed);
    if n % 10 == 2 {
        if let Some(pid) = MemReader::find_process("eqgame.exe") {
            if mem.open(pid).is_ok() {
                println!("[STATE] Attached to eqgame.exe PID={pid}");
            }
        }
    }
    mem.pid() != 0
}

// --------------------------------------------------------------------------
// ServerLogic — config loading and offset reload
// --------------------------------------------------------------------------

pub struct ServerLogic {
    ini_path:        String,
    config_ini_path: String,
}

impl ServerLogic {
    pub fn new(ini_path: String, config_ini_path: String) -> Self {
        Self { ini_path, config_ini_path }
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

    /// Reload offsets from files; return the patch date on success.
    /// Mirrors ServerLogic::reloadOffsets() in C++.
    pub fn reload_offsets(&self) -> Result<String, String> {
        let (ir, _model) = self.load_config()?;
        Ok(ir.patch_date.clone())
    }
}