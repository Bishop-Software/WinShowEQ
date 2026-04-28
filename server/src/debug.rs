use std::io::{self, Write as _};

use crate::config::IniReader;
use crate::data::spawn_offsets::{ItemOffsets, SpawnOffsets, WorldOffsets};
use crate::mem_reader::{MemReader, parse_string_bytes};
use crate::server_logic::{extract_spawn_record, read_u64_at};

const FALLBACK_BASE: u64 = 0x140000000;

const OT_ZONE: usize = 0;
const OT_SPAWNS: usize = 1;
const OT_SELF: usize = 2;
const OT_TARGET: usize = 3;
const OT_GROUND: usize = 4;
const OT_WORLD: usize = 5;
const OT_MAX: usize = 6;

static PRIMARY_NAMES: [&str; OT_MAX] = ["pZone", "pSpawns", "pSelf", "pTarget", "pItems", "pWorld"];

/// Interactive debug command loop. Mirrors Debugger in C++.
pub struct DebugLoop {
    primary_addrs: [u64; OT_MAX],
    spawn_off: SpawnOffsets,
    item_off: ItemOffsets,
    world_off: WorldOffsets,
}

impl Default for DebugLoop {
    fn default() -> Self {
        Self::new()
    }
}

impl DebugLoop {
    pub fn new() -> Self {
        Self {
            primary_addrs: [0; OT_MAX],
            spawn_off: SpawnOffsets::default(),
            item_off: ItemOffsets::default(),
            world_off: WorldOffsets::default(),
        }
    }

    fn init(&mut self, ir: &IniReader) {
        self.spawn_off = SpawnOffsets::from_ini(ir);
        self.item_off = ItemOffsets::from_ini(ir);
        self.world_off = WorldOffsets::from_ini(ir);
        if let Ok(model) = ir.read_server_config_model() {
            let o = &model.offsets;
            self.primary_addrs[OT_ZONE] = o.zone_name;
            self.primary_addrs[OT_SPAWNS] = o.spawn_list;
            self.primary_addrs[OT_SELF] = o.self_addr;
            self.primary_addrs[OT_TARGET] = o.target;
            self.primary_addrs[OT_GROUND] = o.ground;
            self.primary_addrs[OT_WORLD] = o.world;
        }
        println!("Debugger: Memory offsets read in.");
    }

    fn print_menu(&self) {
        println!();
        println!("   (d)isplay / (r)eload offsets");
        println!("  spo) set a primary offset   (index/name) (hex value)");
        println!("  sso) set a secondary offset (index/name) (hex value)");
        println!("   ez) examine data using pZone (et) pTarget (ew) pWorld");
        println!("   es) examine raw data using pSelf");
        println!("   fz) find zonename using pZone (zonename)");
        println!("   ft) find spawnname using pTarget (fs) pSelf (spawnname)");
        println!("   ps) display spawn info using pSelf (pt) pTarget");
        println!("   sp) scan process names (process name)");
        println!("  sft) scan for float using pTarget (sfs) pSelf (X,Y,Z)");
        println!("  sfa) scan for floating point using Address (X,Y,Z,Address)");
        println!("  sfu) scan for UINT using pSelf (int)");
        println!("  sbt) scan for BYTE using pTarget (sbs) pSelf (byte)");
        println!("  sfw) scan for world using Game Date (mm/dd/yyyy) from /time");
        println!("   sg) scan for ground items");
        println!("   ws) walk the spawnlist (reverse) using pSelf (wt) pTarget");
        println!("   vs) walk the spawnlist (forward) using pSelf (vt) pTarget");
        println!("    x) exit debugger");
        println!();
    }

    fn display_offsets(&self) {
        println!();
        println!("     Primary Offsets");
        println!("=========================");
        for (i, (name, addr)) in PRIMARY_NAMES
            .iter()
            .zip(self.primary_addrs.iter())
            .enumerate()
        {
            println!("{:>10}) {} = 0x{:X}", i, name, addr);
        }

        println!();
        println!("    Secondary Spawn Offsets");
        println!("===============================");
        for (i, (name, val)) in self.spawn_off.named_values().iter().enumerate() {
            println!("{:>10}) {} = 0x{:03x} ({})", i, name, val, val);
        }

        println!();
        println!("    Secondary Ground Offsets");
        println!("===============================");
        for (i, (name, val)) in self.item_off.named_values().iter().enumerate() {
            println!("{:>10}) {} = 0x{:03x} ({})", i, name, val, val);
        }

        println!();
        println!("    Secondary World Offsets");
        println!("===============================");
        for (i, (name, val)) in self.world_off.named_values().iter().enumerate() {
            println!("{:>10}) {} = 0x{:03x} ({})", i, name, val, val);
        }
        println!();
    }

    fn set_primary_offset(&mut self, args: &str) {
        let tokens: Vec<&str> = args.split_whitespace().collect();
        if tokens.len() < 2 {
            println!(" Failed to set primary offset (usage: spo <index|name> <hex>)");
            return;
        }
        let value = match u64::from_str_radix(
            tokens[1].trim_start_matches("0x").trim_start_matches("0X"),
            16,
        ) {
            Ok(v) => v,
            Err(_) => {
                println!(" Failed to parse hex value '{}'", tokens[1]);
                return;
            }
        };
        // Try as numeric index first, then by name
        if let Ok(idx) = tokens[0].parse::<usize>() {
            if idx < OT_MAX {
                self.primary_addrs[idx] = value;
                println!(
                    " Primary offset #{} ({}) was set to 0x{:X}",
                    idx, PRIMARY_NAMES[idx], value
                );
            } else {
                println!(" Failed to set primary offset");
            }
        } else {
            let name_lower = tokens[0].to_lowercase();
            if let Some(idx) = PRIMARY_NAMES
                .iter()
                .position(|n| n.to_lowercase() == name_lower)
            {
                self.primary_addrs[idx] = value;
                println!(
                    " Primary offset #{} ({}) was set to 0x{:X}",
                    idx, PRIMARY_NAMES[idx], value
                );
            } else {
                println!(" Failed to set primary offset");
            }
        }
    }

    fn set_secondary_offset(&mut self, args: &str) {
        let tokens: Vec<&str> = args.split_whitespace().collect();
        if tokens.len() < 2 {
            println!(" Failed to set secondary offset (usage: sso <index|name> <hex>)");
            return;
        }
        let value = match usize::from_str_radix(
            tokens[1].trim_start_matches("0x").trim_start_matches("0X"),
            16,
        ) {
            Ok(v) => v,
            Err(_) => {
                println!(" Failed to parse hex value '{}'", tokens[1]);
                return;
            }
        };
        if let Ok(idx) = tokens[0].parse::<usize>() {
            let named = self.spawn_off.named_values();
            if self.spawn_off.set_by_index(idx, value) {
                println!(
                    " Secondary offset #{} ({}) was set to 0x{:X}",
                    idx, named[idx].0, value
                );
            } else {
                println!(" Failed to set secondary offset");
            }
        } else {
            if let Some(idx) = self.spawn_off.set_by_name(tokens[0], value) {
                let named = self.spawn_off.named_values();
                println!(
                    " Secondary offset #{} ({}) was set to 0x{:X}",
                    idx, named[idx].0, value
                );
            } else {
                println!(" Failed to set secondary offset");
            }
        }
    }

    // Resolve the live pointer for an offset type.
    // Zone uses a direct canonical→actual remap; all others dereference a pointer.
    fn resolve_ptr(&self, mem: &MemReader, ot: usize) -> Option<u64> {
        let canonical = self.primary_addrs[ot];
        if canonical == 0 {
            return None;
        }
        if ot == OT_ZONE {
            Some(mem.canonical_to_actual(canonical))
        } else {
            match mem.read_raw_pointer(canonical) {
                Ok(0) | Err(_) => None,
                Ok(ptr) => Some(ptr),
            }
        }
    }

    fn examine_raw_memory(&self, mem: &MemReader, ot: usize) {
        const BUF_SIZE: usize = 6144;
        let Some(p_mem) = self.resolve_ptr(mem, ot) else {
            println!(
                " Failed to obtain valid memory pointer for offset {}",
                PRIMARY_NAMES[ot]
            );
            return;
        };
        let canonical = mem.actual_to_canonical(p_mem);
        println!(
            " Display Raw Memory from 0x{:X} to 0x{:X}",
            canonical,
            canonical + BUF_SIZE as u64
        );
        let buf = match mem.read_bytes(p_mem, BUF_SIZE) {
            Ok(b) => b,
            Err(_) => {
                println!(" Failed to read memory at address 0x{:X}", canonical);
                return;
            }
        };
        for r in 0..(BUF_SIZE / 16) {
            print!("0x{:02x}) ", r * 16);
            for c in 0..16_usize {
                let idx = r * 16 + c;
                print!(" {:02x}", buf[idx]);
                if c % 4 == 3 {
                    print!(" ");
                }
            }
            print!("  ");
            for c in 0..16_usize {
                let b = buf[r * 16 + c];
                if b.is_ascii_alphanumeric() {
                    print!("{}", b as char);
                } else {
                    print!(".");
                }
            }
            println!();
        }
    }

    fn process_spawn(&self, mem: &MemReader, ot: usize) {
        let Some(p_mem) = self.resolve_ptr(mem, ot) else {
            println!(
                " Failed to obtain valid memory pointer for offset {}",
                PRIMARY_NAMES[ot]
            );
            return;
        };
        let buf = match mem.read_bytes(p_mem, self.spawn_off.buf_size) {
            Ok(b) => b,
            Err(_) => {
                println!(
                    " Failed to read memory at address 0x{:X}",
                    mem.actual_to_canonical(p_mem)
                );
                return;
            }
        };
        let rec = extract_spawn_record(&buf, &self.spawn_off, 0);
        // Copy packed fields to locals before printing — packed fields can't be referenced.
        let (id, owner, race, x, y, z, heading, speed, primary, offhand) = (
            rec.id,
            rec.owner,
            rec.race,
            rec.x,
            rec.y,
            rec.z,
            rec.heading,
            rec.speed,
            rec.primary,
            rec.offhand,
        );
        println!(
            " {} = 0x{:X}",
            PRIMARY_NAMES[ot],
            mem.actual_to_canonical(p_mem)
        );
        println!("    NameOffset -> {}", parse_string_bytes(&rec.name));
        println!(
            "    LastNameOffset -> {}",
            parse_string_bytes(&rec.last_name)
        );
        println!("    SpawnIDOffset -> {}", id);
        println!("    OwnerIDOffset -> {}", owner);
        println!("    LevelOffset -> {}", rec.level);
        println!("    RaceOffset -> {}", race);
        println!("    ClassOffset -> {}", rec.class);
        println!("    XOffset -> {}", x);
        println!("    YOffset -> {}", y);
        println!("    ZOffset -> {}", z);
        println!("    HeadingOffset -> {}", heading);
        println!("    SpeedOffset -> {}", speed);
        println!("    TypeOffset -> {}", rec.spawn_type);
        println!("    HideOffset -> {}", rec.hidden);
        println!("    PrimaryOffset -> {}", primary);
        println!("    OffhandOffset -> {}", offhand);
    }

    fn walk_spawn_list(&self, mem: &MemReader, ot: usize, reverse: bool) {
        let Some(mut ptr) = self.resolve_ptr(mem, ot) else {
            println!(
                " Failed to obtain valid memory pointer for offset {}",
                PRIMARY_NAMES[ot]
            );
            return;
        };
        if reverse {
            println!(" Walking spawnlist in reverse.");
        } else {
            println!(" Walking spawnlist forward.");
        }

        let buf_size = self
            .spawn_off
            .buf_size
            .max(self.spawn_off.prev + 8)
            .max(self.spawn_off.next + 8);

        let mut spawn_count = 0u32;
        let mut last_prev = 0u64;

        loop {
            let Ok(buf) = mem.read_bytes(ptr, buf_size) else {
                break;
            };
            let p_prev = read_u64_at(&buf, self.spawn_off.prev);
            let p_next = read_u64_at(&buf, self.spawn_off.next);
            last_prev = p_prev;
            spawn_count += 1;

            let rec = extract_spawn_record(&buf, &self.spawn_off, 0);
            let id = rec.id;
            println!("    -----------------------------------");
            println!("    NameOffset -> {}", parse_string_bytes(&rec.name));
            println!("    SpawnIDOffset -> {}", id);
            println!("    PrevOffset -> 0x{:X}", p_prev);
            println!("    NextOffset -> 0x{:X}", p_next);

            let step = if reverse { p_prev } else { p_next };
            let keep_going = step != 0 && step != ptr && spawn_count < 1000;
            if !keep_going {
                break;
            }

            if mem.read_bytes(step, buf_size).is_err() {
                break;
            }
            ptr = step;
        }

        println!(
            " Discovered {} spawn entities during the walk.",
            spawn_count
        );

        // If a reverse walk didn't reach the head, try scanning for the pointer.
        if reverse && last_prev != 0 {
            self.scan_for_ptr(mem, ptr, FALLBACK_BASE, 0x1800000);
        }
    }

    // Scan a canonical address range byte-by-byte for an 8-byte pointer value.
    // Reads in 4 KiB chunks for efficiency; mirrors scanForPtr in Debugger.cpp.
    fn scan_for_ptr(&self, mem: &MemReader, search: u64, start_canonical: u64, size: u64) {
        if start_canonical == 0 {
            return;
        }
        let start = mem.canonical_to_actual(start_canonical);
        println!(
            " Scanning for 0x{:X} from 0x{:X} to 0x{:X}",
            search,
            start_canonical,
            start_canonical + size
        );

        const CHUNK: usize = 4096;
        let mut pos = start;
        let end = start + size;
        while pos < end {
            let want = (CHUNK + 7).min((end - pos) as usize);
            let Ok(buf) = mem.read_bytes(pos, want) else {
                pos += CHUNK as u64;
                continue;
            };
            let scan_end = buf.len().saturating_sub(7);
            for i in 0..scan_end {
                if let Ok(bytes) = buf[i..i + 8].try_into() {
                    if u64::from_le_bytes(bytes) == search {
                        let found = mem.actual_to_canonical(pos + i as u64);
                        println!(" Pointer match found for 0x{:X} at 0x{:X}", search, found);
                    }
                }
            }
            pos += CHUNK as u64;
        }
    }

    fn scan_for_string(&self, mem: &MemReader, ot: usize, size: u64, search: &str) {
        if self.primary_addrs[ot] == 0 || size == 0 || search.is_empty() {
            println!(
                " Error: '{}' appears to be an invalid search string.",
                search
            );
            return;
        }
        let name_off: u64 = if ot == OT_GROUND {
            self.item_off.name as u64
        } else {
            self.spawn_off.name as u64
        };
        let p_start = mem.canonical_to_actual(self.primary_addrs[ot].saturating_sub(4 * size));
        let p_end = p_start + 8 * size;
        println!(
            " Scanning for '{}' from 0x{:X} to 0x{:X}",
            search, p_start, p_end
        );

        let mut p_mem = p_start;

        if ot == OT_ZONE {
            // Byte-by-byte scan for the first character, then verify the full string.
            let first_byte = search.as_bytes()[0];
            const CHUNK: usize = 4096;
            while p_mem < p_end {
                let want = CHUNK.min((p_end - p_mem) as usize);
                let Ok(buf) = mem.read_bytes(p_mem, want) else {
                    p_mem += CHUNK as u64;
                    continue;
                };
                for (i, &b) in buf.iter().enumerate() {
                    if b == first_byte {
                        let addr = p_mem + i as u64;
                        if let Ok(full) = mem.read_bytes(addr, 30) {
                            if parse_string_bytes(&full) == search {
                                println!(
                                    " Pointer match found at 0x{:X}",
                                    mem.actual_to_canonical(addr)
                                );
                            }
                        }
                    }
                }
                p_mem += CHUNK as u64;
            }
        } else if ot == OT_TARGET || ot == OT_SELF {
            while p_mem < p_end {
                if let Ok(p_deep) = mem.read_pointer(p_mem) {
                    if p_deep != 0 && p_deep < p_mem {
                        if let Ok(s) = mem.read_string(p_deep + name_off, 64) {
                            if s == search {
                                println!(
                                    " Pointer match found at 0x{:X}",
                                    mem.actual_to_canonical(p_mem)
                                );
                            }
                        }
                    }
                }
                p_mem += 8;
            }
        } else if ot == OT_GROUND {
            while p_mem < p_end {
                if let Ok(p_deep2) = mem.read_pointer(p_mem) {
                    if p_deep2 != 0 && p_deep2 < p_mem {
                        if let Ok(s) = mem.read_string(p_deep2 + name_off, 64) {
                            if s.starts_with(search) {
                                println!(
                                    " Pointer match found at 0x{:X}. Full string is {}",
                                    mem.actual_to_canonical(p_mem),
                                    s
                                );
                            }
                        }
                        if let Ok(p_deep) = mem.read_pointer(p_deep2) {
                            if p_deep != 0 && p_deep < p_mem {
                                if let Ok(s) = mem.read_string(p_deep + name_off, 64) {
                                    if s.starts_with(search) {
                                        println!(
                                            " Pointer match found at 0x{:X}. Full string is {}",
                                            mem.actual_to_canonical(p_mem),
                                            s
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
                p_mem += 8;
            }
        }
    }

    // Scan for X, Y, Z float coordinates within 4 KiB of p_start.
    // args is comma-separated: "X", "X,Y", or "X,Y,Z".
    // Mirrors scanForFloat(…, yankPstart=false) in Debugger.cpp.
    fn scan_for_float(&self, mem: &MemReader, args: &str, p_start: u64) {
        const INVALID: f32 = -100_000.0;
        if p_start == 0 {
            return;
        }

        let tokens: Vec<&str> = args.split(',').collect();
        let x_find: f32 = tokens
            .first()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(INVALID);
        let y_find: f32 = tokens
            .get(1)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(INVALID);
        let z_find: f32 = tokens
            .get(2)
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(INVALID);

        let (s_check, d_check, t_check) = match tokens.len() {
            1 => (true, false, false),
            2 => (false, true, false),
            _ => (false, false, true),
        };

        let p_end = p_start + 0x1000;
        let mut p_float = p_start;
        while p_float < p_end {
            let x_temp: f32 = mem.read(p_float).unwrap_or(INVALID);
            let x_match = (x_temp - x_find).abs() < 20.0;
            if s_check && x_match {
                println!("  X match found at offset 0x{:X}", p_float - p_start);
            }
            if d_check && x_match {
                let y_temp: f32 = mem.read(p_float + 4).unwrap_or(INVALID);
                if (y_temp - y_find).abs() < 20.0 {
                    println!(
                        "  X,Y match found at offset 0x{:X}, 0x{:X} ({},{})",
                        p_float - p_start,
                        p_float - p_start + 4,
                        x_temp,
                        y_temp
                    );
                }
            }
            if t_check && x_match {
                let y_temp: f32 = mem.read(p_float + 4).unwrap_or(INVALID);
                let z_temp: f32 = mem.read(p_float + 8).unwrap_or(INVALID);
                if (y_temp - y_find).abs() < 20.0 && (z_temp - z_find).abs() < 20.0 {
                    println!(
                        "  X,Y,Z match found at offset 0x{:X}, 0x{:X}, 0x{:X} ({},{},{})",
                        p_float - p_start,
                        p_float - p_start + 4,
                        p_float - p_start + 8,
                        x_temp,
                        y_temp,
                        z_temp
                    );
                }
            }
            p_float += 4;
        }
    }

    // Scan for an integer value (UINT=4 bytes or BYTE=2 bytes) starting at p_start.
    // Mirrors scanForUINT in Debugger.cpp.
    fn scan_for_uint(&self, mem: &MemReader, p_start: u64, size: u64, width: u32, args: &str) {
        if p_start == 0 || args.is_empty() {
            println!("    Error: '{}' appears to be an invalid value.", args);
            println!("    Proper usage - sfu number");
            return;
        }
        let find: u32 = match args.trim().parse() {
            Ok(v) => v,
            Err(_) => {
                println!("    Error: '{}' appears to be an invalid value.", args);
                return;
            }
        };
        println!(
            " Scanning for '{}' from 0x{:X} to 0x{:X}",
            args,
            p_start,
            p_start + 4 * size
        );

        let mut p_mem = p_start;
        let p_end = p_start + size * 2;
        while p_mem < p_end {
            let temp: u32 = if width == 4 {
                mem.read::<u32>(p_mem).unwrap_or(0)
            } else {
                mem.read::<u8>(p_mem).unwrap_or(0) as u32
            };
            if temp == find {
                println!("{}  match found at offset 0x{:X}", find, p_mem - p_start);
            }
            p_mem += 1;
        }
    }

    fn scan_for_world_from_date(&self, mem: &MemReader, ot: usize, size: u64, args: &str) {
        if self.primary_addrs[ot] == 0 || size == 0 || args.is_empty() {
            println!("    Error: '{}' appears to be an invalid date.", args);
            println!("    Proper usage - sfw mm/dd/yyyy");
            println!("    Get date from Game Time using /time");
            return;
        }

        let parts: Vec<&str> = args.split('/').collect();
        if parts.len() != 3 {
            println!("    Incomplete Date.  Proper usage - sfw mm/dd/yyyy");
            println!("    Get date from Game Time using /time");
            return;
        }
        let m_find: u8 = parts[0].trim().parse().unwrap_or(0);
        let d_find: u8 = parts[1].trim().parse().unwrap_or(0);
        let y_find: u32 = parts[2].trim().parse().unwrap_or(0);

        if m_find > 12 || d_find > 31 {
            println!("    Bad Date (mm/dd/yyyy): Limit mm to max of 12 and dd to max of 31");
            return;
        }

        let p_start = mem.canonical_to_actual(self.primary_addrs[ot].saturating_sub(2 * size));
        println!(
            " Scanning for '{}' from 0x{:X} to 0x{:X}",
            args,
            p_start,
            p_start + 4 * size
        );

        let day_off = self.world_off.day as u64;
        let month_off = self.world_off.month as u64;
        let year_off = self.world_off.year as u64;
        let race8 = self.world_off.race8;

        let mut p_mem = p_start;
        let p_end = p_start + size * 4;
        while p_mem < p_end {
            if let Ok(p_deep) = mem.read_pointer(p_mem) {
                if p_deep != 0 && p_deep < p_mem {
                    let d_temp: u8 = mem.read(p_deep + day_off).unwrap_or(0);
                    let m_temp: u8 = mem.read(p_deep + month_off).unwrap_or(0);
                    let y_temp: u32 = if race8 {
                        mem.read::<u16>(p_deep + year_off).unwrap_or(0) as u32
                    } else {
                        mem.read::<u32>(p_deep + year_off).unwrap_or(0)
                    };
                    if d_temp == d_find && m_temp == m_find && y_temp == y_find {
                        println!(
                            "  Date match found at offset 0x{:X} ({}/{}/{})",
                            mem.actual_to_canonical(p_mem),
                            m_temp,
                            d_temp,
                            y_temp
                        );
                    }
                }
            }
            p_mem += 4;
        }
    }

    fn show_processes(name: &str) {
        let search = if name.is_empty() { "eqgame" } else { name };
        let processes = MemReader::find_all_processes(search);
        if processes.is_empty() {
            println!(" No processes found matching '{}'.", search);
        } else {
            for (pid, exe) in &processes {
                println!("  PID: {:>6}  Exe: {}", pid, exe);
            }
        }
    }

    /// Parse one command line and execute the matching action.
    /// Returns false when the "x" (exit) command is received.
    pub fn dispatch_command(
        &mut self,
        input: &str,
        mem: &mut MemReader,
        ir: &mut IniReader,
    ) -> bool {
        let trimmed = input.trim();
        let (cmd, args) = match trimmed.find(char::is_whitespace) {
            Some(pos) => (
                trimmed[..pos].to_lowercase(),
                trimmed[pos + 1..].trim_start(),
            ),
            None => (trimmed.to_lowercase(), ""),
        };

        match cmd.as_str() {
            "?" => self.print_menu(),
            "d" => self.display_offsets(),
            "r" => self.init(ir),
            "spo" => self.set_primary_offset(args),
            "sso" => self.set_secondary_offset(args),
            "ez" => self.examine_raw_memory(mem, OT_ZONE),
            "et" => self.examine_raw_memory(mem, OT_TARGET),
            "es" => self.examine_raw_memory(mem, OT_SELF),
            "ew" => self.examine_raw_memory(mem, OT_WORLD),
            "fz" => self.scan_for_string(mem, OT_ZONE, 0x100_0000, args),
            "ft" => self.scan_for_string(mem, OT_TARGET, 0x100_0000, args),
            "fs" => self.scan_for_string(mem, OT_SELF, 0x100_0000, args),
            "ps" => self.process_spawn(mem, OT_SELF),
            "pt" => self.process_spawn(mem, OT_TARGET),
            "sp" => Self::show_processes(args),
            "sfa" => {
                // Parse X,Y,Z,Address — address is the 4th comma-separated token.
                let tokens: Vec<&str> = args.split(',').collect();
                let p_start = tokens
                    .get(3)
                    .and_then(|s| u64::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0);
                self.scan_for_float(mem, args, p_start);
            }
            "sft" => {
                let p = self.resolve_ptr(mem, OT_TARGET).unwrap_or(0);
                self.scan_for_float(mem, args, p);
            }
            "sfs" => {
                let p = self.resolve_ptr(mem, OT_SELF).unwrap_or(0);
                self.scan_for_float(mem, args, p);
            }
            "sfu" => {
                let p = self.resolve_ptr(mem, OT_SELF).unwrap_or(0);
                self.scan_for_uint(mem, p, 0x1000, 4, args);
            }
            "sbt" => {
                let p = self.resolve_ptr(mem, OT_TARGET).unwrap_or(0);
                self.scan_for_uint(mem, p, 0x1000, 2, args);
            }
            "sbs" => {
                let p = self.resolve_ptr(mem, OT_SELF).unwrap_or(0);
                self.scan_for_uint(mem, p, 0x1000, 2, args);
            }
            "sfw" => self.scan_for_world_from_date(mem, OT_WORLD, 0x100_0000, args),
            "sg" => self.scan_for_string(mem, OT_GROUND, 0x100_0000, "IT"),
            "ws" => self.walk_spawn_list(mem, OT_SELF, true),
            "wt" => self.walk_spawn_list(mem, OT_TARGET, true),
            "vs" => self.walk_spawn_list(mem, OT_SELF, false),
            "vt" => self.walk_spawn_list(mem, OT_TARGET, false),
            "x" => return false,
            "" => {}
            _ => println!(" Invalid selection. Please try again."),
        }
        true
    }

    /// Blocking interactive debug loop. Mirrors Debugger::enterDebugLoop in C++.
    pub fn enter_debug_loop(&mut self, mem: &mut MemReader, ir: &mut IniReader) {
        self.init(ir);
        self.print_menu();
        loop {
            print!(" > ");
            let _ = io::stdout().flush();
            let mut line = String::new();
            match io::stdin().read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let input = line
                .trim_end_matches('\n')
                .trim_end_matches('\r')
                .to_string();
            if !self.dispatch_command(&input, mem, ir) {
                break;
            }
            println!("    ?) display main menu");
        }
    }
}
