use std::fmt::Write as FmtWrite;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use crate::config::{IniReader, PrimaryOffsets};

pub const SCAN_WINDOW: usize = 0x20000;

const DOS_SIGNATURE: u16 = 0x5A4D; // "MZ"
const PE_SIGNATURE: u32 = 0x0000_4550; // "PE\0\0"
const OPT_HDR64_MAGIC: u16 = 0x020B; // PE32+

pub struct PeSectionInfo {
    #[allow(dead_code)]
    pub name: [u8; 9],
    pub virtual_address: u32,
    pub pointer_to_raw_data: u32,
    pub size_of_raw_data: u32,
}

pub struct PeImageInfo {
    pub image_base: u64,
    pub time_date_stamp: u32,
    pub sections: Vec<PeSectionInfo>,
}

pub struct ScanResult {
    pub reload: bool,
    pub has_address_mismatch: bool,
    pub output: String,
    pub primary: PrimaryOffsets,
}

/// Which PrimaryOffsets field a primary scan entry corresponds to.
/// Mirrors NetworkServer::offset_types in NetworkServer.h.
#[derive(Clone, Copy)]
enum OffsetKind {
    ZoneName,
    SpawnList,
    SelfAddr,
    Target,
    Ground,
    World,
}

impl OffsetKind {
    fn get(self, o: &PrimaryOffsets) -> u64 {
        match self {
            OffsetKind::ZoneName => o.zone_name,
            OffsetKind::SpawnList => o.spawn_list,
            OffsetKind::SelfAddr => o.self_addr,
            OffsetKind::Target => o.target,
            OffsetKind::Ground => o.ground,
            OffsetKind::World => o.world,
        }
    }

    fn set(self, o: &mut PrimaryOffsets, val: u64) {
        match self {
            OffsetKind::ZoneName => o.zone_name = val,
            OffsetKind::SpawnList => o.spawn_list = val,
            OffsetKind::SelfAddr => o.self_addr = val,
            OffsetKind::Target => o.target = val,
            OffsetKind::Ground => o.ground = val,
            OffsetKind::World => o.world = val,
        }
    }
}

struct PrimaryPatternEntry {
    ini_section: &'static str,
    ini_write_key: &'static str,
    output_label: &'static str,
    kind: OffsetKind,
}

struct SecondaryPatternEntry {
    ini_section: &'static str,
    output_label: &'static str,
}

/// Mirrors kPrimaryScans in EQGameScanner.cpp::ScanExecutable.
static PRIMARY_SCANS: &[PrimaryPatternEntry] = &[
    PrimaryPatternEntry {
        ini_section: "ZoneAddr",
        ini_write_key: "ZoneAddr",
        output_label: "ZoneAddr",
        kind: OffsetKind::ZoneName,
    },
    PrimaryPatternEntry {
        ini_section: "SpawnHeaderAddr",
        ini_write_key: "SpawnHeaderAddr",
        output_label: "SpawnHeaderAddr",
        kind: OffsetKind::SpawnList,
    },
    PrimaryPatternEntry {
        ini_section: "CharInfo",
        ini_write_key: "CharInfo",
        output_label: "CharInfo",
        kind: OffsetKind::SelfAddr,
    },
    PrimaryPatternEntry {
        ini_section: "ItemsAddr",
        ini_write_key: "ItemsAddr",
        output_label: "ItemsAddr",
        kind: OffsetKind::Ground,
    },
    PrimaryPatternEntry {
        ini_section: "TargetAddr",
        ini_write_key: "TargetAddr",
        output_label: "TargetAddr",
        kind: OffsetKind::Target,
    },
    PrimaryPatternEntry {
        ini_section: "WorldAddr",
        ini_write_key: "WorldAddr",
        output_label: "WorldAddr",
        kind: OffsetKind::World,
    },
];

static SECONDARY_SCANS: &[SecondaryPatternEntry] = &[
    SecondaryPatternEntry {
        ini_section: "SpawnInfoTypeOffset",
        output_label: "TypeOffset",
    },
    SecondaryPatternEntry {
        ini_section: "SpawnInfoSpawnIDOffset",
        output_label: "SpawnIDOffset",
    },
    SecondaryPatternEntry {
        ini_section: "SpawnInfoLevelOffset",
        output_label: "LevelOffset",
    },
    SecondaryPatternEntry {
        ini_section: "SpawnInfoRaceOffset",
        output_label: "RaceOffset",
    },
    SecondaryPatternEntry {
        ini_section: "SpawnInfoClassOffset",
        output_label: "ClassOffset",
    },
    SecondaryPatternEntry {
        ini_section: "SpawnInfoPrimaryOffset",
        output_label: "PrimaryOffset",
    },
    SecondaryPatternEntry {
        ini_section: "SpawnInfoOffhandOffset",
        output_label: "OffhandOffset",
    },
];

// ---------------------------------------------------------------------------
// Core scan logic (operates on &[u8] — no file I/O, fully testable)
// ---------------------------------------------------------------------------

/// Returns true if `data` matches `byte_mask` at positions where `char_mask` is 'x', 'o', or 'r'.
/// All other mask characters are wildcards.
/// Mirrors EQGameScanner::compareData in EQGameScanner.cpp.
pub fn compare_data(data: &[u8], byte_mask: &[u8], char_mask: &[u8]) -> bool {
    for ((&d, &b), &c) in data.iter().zip(byte_mask.iter()).zip(char_mask.iter()) {
        if (c == b'x' || c == b'o' || c == b'r') && d != b {
            return false;
        }
    }
    true
}

/// Convert a file byte offset to a virtual address using the loaded PE section table.
/// Returns 0 if the offset falls outside all known sections.
fn file_offset_to_va(file_offset: u64, pe: &PeImageInfo) -> u64 {
    for s in &pe.sections {
        if s.pointer_to_raw_data == 0 || s.size_of_raw_data == 0 {
            continue;
        }
        let start = s.pointer_to_raw_data as u64;
        let end = start + s.size_of_raw_data as u64;
        if file_offset >= start && file_offset < end {
            return pe.image_base + s.virtual_address as u64 + (file_offset - start);
        }
    }
    0
}

fn read_extracted(buffer: &[u8], base: usize, t_offset: usize, type_len: usize) -> u32 {
    let pos = base + t_offset;
    match type_len {
        1 => buffer
            .get(pos)
            .copied()
            .map(|b| b as u32)
            .unwrap_or(0xFFFF_FFFF),
        2 => buffer
            .get(pos..pos + 2)
            .and_then(|s| s.try_into().ok())
            .map(u16::from_le_bytes)
            .map(|v| v as u32)
            .unwrap_or(0xFFFF_FFFF),
        _ => buffer
            .get(pos..pos + 4)
            .and_then(|s| s.try_into().ok())
            .map(u32::from_le_bytes)
            .unwrap_or(0xFFFF_FFFF),
    }
}

/// Scan `buffer` for `byte_mask`/`char_mask` and return the extracted value.
///
/// - `file_start`: file byte offset where `buffer` begins (used only for RIP resolution).
/// - `pe_info`: required when the mask contains `'r'` (RIP-relative mode).
///
/// Returns 0 when nothing is found.
/// Mirrors EQGameScanner::findEQPointerOffset (core loop) in EQGameScanner.cpp.
pub fn scan_buffer_for_pointer(
    buffer: &[u8],
    byte_mask: &[u8],
    char_mask: &[u8],
    file_start: u64,
    pe_info: Option<&PeImageInfo>,
) -> u64 {
    if char_mask.is_empty() || byte_mask.len() < char_mask.len() {
        return 0;
    }

    let mask_len = char_mask.len();
    let rip_relative = char_mask.contains(&b'r');

    // Locate the 't' run which defines the value field to extract.
    let t_first = char_mask.iter().position(|&c| c == b't');
    let t_last = char_mask.iter().rposition(|&c| c == b't');
    let type_len = match (t_first, t_last) {
        (Some(f), Some(l)) => (l - f + 1).max(1),
        _ => 4,
    };
    let t_offset = t_first.unwrap_or(0);

    for i in 0..buffer.len().saturating_sub(mask_len - 1) {
        if !compare_data(&buffer[i..], byte_mask, char_mask) {
            continue;
        }

        let extracted = read_extracted(buffer, i, t_offset, type_len);

        if rip_relative {
            let Some(pe) = pe_info else { return 0 };
            if type_len != 4 {
                return 0;
            }
            // RIP points to the byte immediately after the 4-byte displacement field.
            let next_file_offset = file_start + i as u64 + t_offset as u64 + 4;
            let next_va = file_offset_to_va(next_file_offset, pe);
            if next_va == 0 {
                return 0;
            }
            let resolved = next_va as i64 + extracted as i32 as i64;
            if resolved < 0 {
                return 0;
            }
            return resolved as u64;
        }

        // Non-RIP: only accept values below 0x20000000 (pointer plausibility guard).
        if extracted < 0x2000_0000 {
            return extracted as u64;
        }
    }

    0
}

// ---------------------------------------------------------------------------
// PE header parsing helpers
// ---------------------------------------------------------------------------

fn read_u16_le(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
}
fn read_u32_le(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
}
fn read_u64_le(b: &[u8], off: usize) -> Option<u64> {
    b.get(off..off + 8)
        .and_then(|s| s.try_into().ok())
        .map(u64::from_le_bytes)
}
fn read_i32_le(b: &[u8], off: usize) -> Option<i32> {
    b.get(off..off + 4)
        .and_then(|s| s.try_into().ok())
        .map(i32::from_le_bytes)
}

// ---------------------------------------------------------------------------
// EqGameScanner
// ---------------------------------------------------------------------------

pub struct EqGameScanner {
    pub exe_path: String,
}

impl EqGameScanner {
    pub fn new(exe_path: impl Into<String>) -> Self {
        Self {
            exe_path: exe_path.into(),
        }
    }

    pub fn executable_exists(&self) -> bool {
        std::path::Path::new(&self.exe_path).exists()
    }

    /// Read `size` bytes from the exe at file offset `start`.
    fn read_scan_window(&self, start: u64, size: usize) -> std::io::Result<Vec<u8>> {
        let mut file = File::open(&self.exe_path)?;
        file.seek(SeekFrom::Start(start))?;
        let mut buf = vec![0u8; size];
        let n = file.read(&mut buf)?;
        buf.truncate(n);
        Ok(buf)
    }

    /// Parse the PE optional header and section table from the exe.
    /// Mirrors EQGameScanner::parsePEHeaders in EQGameScanner.cpp.
    pub fn parse_pe_headers(&self) -> Option<PeImageInfo> {
        let mut file = File::open(&self.exe_path).ok()?;
        // 4 KB covers DOS header + PE signature + file header + optional header + ~8 sections.
        let mut bytes = vec![0u8; 4096];
        let n = file.read(&mut bytes).ok()?;
        bytes.truncate(n);
        let b = &bytes;

        if read_u16_le(b, 0)? != DOS_SIGNATURE {
            return None;
        }
        let e_lfanew = read_i32_le(b, 60)? as usize;

        if read_u32_le(b, e_lfanew)? != PE_SIGNATURE {
            return None;
        }

        // IMAGE_FILE_HEADER at e_lfanew + 4
        let fh = e_lfanew + 4;
        let num_sections = read_u16_le(b, fh + 2)? as usize;
        let time_date_stamp = read_u32_le(b, fh + 4)?;
        let opt_header_size = read_u16_le(b, fh + 16)? as usize;

        // IMAGE_OPTIONAL_HEADER64 at fh + 20
        let oh = fh + 20;
        if read_u16_le(b, oh)? != OPT_HDR64_MAGIC {
            return None;
        }
        let image_base = read_u64_le(b, oh + 24)?;

        // Section table follows the optional header
        let st = oh + opt_header_size;
        let mut sections = Vec::with_capacity(num_sections);
        for i in 0..num_sections {
            let sh = st + i * 40;
            if sh + 40 > b.len() {
                break;
            }
            let mut name = [0u8; 9];
            name[..8].copy_from_slice(&b[sh..sh + 8]);
            let virtual_address = read_u32_le(b, sh + 12)?;
            let size_of_raw_data = read_u32_le(b, sh + 16)?;
            let pointer_to_raw_data = read_u32_le(b, sh + 20)?;
            sections.push(PeSectionInfo {
                name,
                virtual_address,
                pointer_to_raw_data,
                size_of_raw_data,
            });
        }

        Some(PeImageInfo {
            image_base,
            time_date_stamp,
            sections,
        })
    }

    /// Scan the exe for a pointer/address using a byte+char mask.
    /// Mirrors EQGameScanner::findEQPointerOffset in EQGameScanner.cpp.
    pub fn find_eq_pointer_offset(
        &self,
        start_addr: u64,
        block_size: usize,
        byte_mask: &[u8],
        char_mask: &[u8],
    ) -> u64 {
        let pe_info = if char_mask.contains(&b'r') {
            self.parse_pe_headers()
        } else {
            None
        };
        let buffer = match self.read_scan_window(start_addr, block_size) {
            Ok(b) => b,
            Err(_) => return 0,
        };
        scan_buffer_for_pointer(&buffer, byte_mask, char_mask, start_addr, pe_info.as_ref())
    }

    /// Scan for a structure field offset by first patching the 'o' run with a known base address.
    /// Mirrors EQGameScanner::findEQStructureOffset in EQGameScanner.cpp.
    pub fn find_eq_structure_offset(
        &self,
        start_addr: u64,
        block_size: usize,
        byte_mask: &[u8],
        char_mask: &[u8],
        base_addr: u64,
    ) -> u64 {
        let o_pos = char_mask.iter().position(|&c| c == b'o');
        let Some(o_pos) = o_pos else {
            return self.find_eq_pointer_offset(start_addr, block_size, byte_mask, char_mask);
        };
        let o_len = char_mask[o_pos..]
            .iter()
            .take_while(|&&c| c == b'o')
            .count();

        let mut patched = byte_mask.to_vec();
        if o_len == 4 && o_pos + 4 <= patched.len() {
            patched[o_pos..o_pos + 4].copy_from_slice(&(base_addr as u32).to_le_bytes());
        } else if o_len == 8 && o_pos + 8 <= patched.len() {
            patched[o_pos..o_pos + 8].copy_from_slice(&base_addr.to_le_bytes());
        } else {
            return 0;
        }

        self.find_eq_pointer_offset(start_addr, block_size, &patched, char_mask)
    }

    fn run_primary_scan(
        &self,
        ir: &IniReader,
        current_offsets: &PrimaryOffsets,
        entry: &PrimaryPatternEntry,
        write_out: bool,
        reload: &mut bool,
        has_mismatch: &mut bool,
    ) -> (String, u64) {
        let start = ir.read_pattern_int(entry.ini_section, "Start");
        let pattern = ir.read_pattern_bytes(entry.ini_section, "Pattern");
        let mask_str = ir.read_pattern_string(entry.ini_section, "Mask");

        let match_addr =
            self.find_eq_pointer_offset(start, SCAN_WINDOW, &pattern, mask_str.as_bytes());

        let suffix = if match_addr != 0 {
            let current = entry.kind.get(current_offsets);
            if match_addr == current {
                " # Match\r\n"
            } else if write_out {
                let value_str = format!("0x{:x}", match_addr);
                if ir.write_string_entry("Memory Offsets", entry.ini_write_key, &value_str, false) {
                    *reload = true;
                    " # Written to ini file\r\n"
                } else {
                    " # Found - Write failed\r\n"
                }
            } else {
                *has_mismatch = true;
                " # Does not match ini file.\r\n"
            }
        } else {
            " # Not Found\r\n"
        };

        (
            format!("{}=0x{:x}{}", entry.output_label, match_addr, suffix),
            match_addr,
        )
    }

    fn run_secondary_scan(
        &self,
        ir: &IniReader,
        entry: &SecondaryPatternEntry,
        base_addr: u64,
        write_out: bool,
    ) -> String {
        let start = ir.read_pattern_int(entry.ini_section, "Start");
        let pattern = ir.read_pattern_bytes(entry.ini_section, "Pattern");
        let mask_str = ir.read_pattern_string(entry.ini_section, "Mask");

        let match_val = self.find_eq_structure_offset(
            start,
            SCAN_WINDOW,
            &pattern,
            mask_str.as_bytes(),
            base_addr,
        );

        let suffix = if match_val != 0 {
            let current =
                ir.read_integer_entry("SpawnInfo Offsets", entry.output_label, false) as u64;
            if match_val == current {
                " # Match\r\n"
            } else if write_out {
                let value_str = format!("0x{:x}", match_val);
                if ir.write_string_entry("SpawnInfo Offsets", entry.output_label, &value_str, false)
                {
                    " # Written to ini file\r\n"
                } else {
                    " # Found - Write failed\r\n"
                }
            } else {
                " # Found\r\n"
            }
        } else {
            " # Not Found\r\n"
        };

        format!("{}=0x{:x}{}", entry.output_label, match_val, suffix)
    }

    /// Run the primary (pointer address) scan against the exe.
    /// Mirrors EQGameScanner::ScanExecutable in EQGameScanner.cpp.
    pub fn scan_executable(
        &self,
        ir: &IniReader,
        current_offsets: &PrimaryOffsets,
        write_out: bool,
    ) -> ScanResult {
        let mut result = ScanResult {
            reload: false,
            has_address_mismatch: false,
            output: String::new(),
            primary: PrimaryOffsets::default(),
        };

        if !self.executable_exists() {
            result.output = "Error: Could not locate the specified executable file.".to_string();
            return result;
        }

        let mut out = String::new();

        if let Some(pe) = self.parse_pe_headers() {
            let patch_date = unix_to_date(pe.time_date_stamp as u64);
            let client_hash = self.compute_client_hash().unwrap_or_default();
            let build_string = self
                .scan_build_string(&pe)
                .map(|raw| {
                    // Binary stores the short form "Release Client #NNN)\n" (newline before null).
                    // Trim trailing whitespace then ')' before appending time/date from the PE
                    // TimeDateStamp (UTC — may differ from local build-machine time by timezone).
                    let base = raw.trim_end().trim_end_matches(')');
                    let (time_str, date_str) = unix_to_compile_time_date(pe.time_date_stamp as u64);
                    format!("{} {} {}", base, time_str, date_str)
                })
                .unwrap_or_default();

            if write_out {
                ir.write_string_entry("File Info", "PatchDate", &patch_date, false);
                if !client_hash.is_empty() {
                    ir.write_string_entry("File Info", "ClientHash", &client_hash, false);
                }
                if !build_string.is_empty() {
                    ir.write_string_entry("File Info", "BuildString", &build_string, false);
                }
            }
            let _ = write!(out, "[File Info]\r\nPatchDate={}\r\n", patch_date);
            if !client_hash.is_empty() {
                let _ = write!(out, "ClientHash={}\r\n", client_hash);
            }
            if !build_string.is_empty() {
                let _ = write!(out, "BuildString={}\r\n", build_string);
            }
            out.push_str("\r\n");
        }

        let port = ir.read_integer_entry("Port", "Port", false);
        let _ = write!(out, "[Port]\r\nPort={}\r\n\r\n[Memory Offsets]\r\n", port);

        for entry in PRIMARY_SCANS {
            let (line, addr) = self.run_primary_scan(
                ir,
                current_offsets,
                entry,
                write_out,
                &mut result.reload,
                &mut result.has_address_mismatch,
            );
            out.push_str(&line);
            if addr != 0 {
                entry.kind.set(&mut result.primary, addr);
            }
        }

        result.output = out;
        result
    }

    /// Compute SHA1 of the entire exe. Returns lowercase hex or None on I/O error.
    pub fn compute_client_hash(&self) -> Option<String> {
        let mut file = File::open(&self.exe_path).ok()?;
        let mut hasher = sha1_smol::Sha1::new();
        let mut buf = vec![0u8; 65536];
        loop {
            let n = file.read(&mut buf).ok()?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Some(hasher.digest().to_string())
    }

    /// Scan all PE sections for the null-terminated EQ build string.
    /// EQ embeds a string like "Release Client #630 10:45:28 Apr 14 2026".
    /// Multiple occurrences exist across sections; we return the longest
    /// non-format-string match (skipping any containing '%').
    pub fn scan_build_string(&self, pe: &PeImageInfo) -> Option<String> {
        const PREFIX: &[u8] = b"Release Client #";
        let mut file = File::open(&self.exe_path).ok()?;
        let mut best: Option<String> = None;

        for section in &pe.sections {
            file.seek(SeekFrom::Start(section.pointer_to_raw_data as u64))
                .ok()?;
            let mut data = vec![0u8; section.size_of_raw_data as usize];
            let n = file.read(&mut data).ok()?;
            data.truncate(n);

            let mut from = 0usize;
            while from + PREFIX.len() <= data.len() {
                let Some(rel) = data[from..].windows(PREFIX.len()).position(|w| w == PREFIX) else {
                    break;
                };
                let pos = from + rel;
                let end = data[pos..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|p| pos + p)
                    .unwrap_or(data.len());
                let s = String::from_utf8_lossy(&data[pos..end]).into_owned();
                if !s.contains('%')
                    && best
                        .as_ref()
                        .map(|b: &String| s.len() > b.len())
                        .unwrap_or(true)
                {
                    best = Some(s);
                }
                from = pos + 1;
            }
        }
        best
    }

    /// Run the secondary (structure field offset) scan against the exe.
    /// When `write_out` is true, found values are written to `[SpawnInfo Offsets]`
    /// in `myseqserver.ini`.
    /// Mirrors EQGameScanner::ScanSecondary in EQGameScanner.cpp.
    pub fn scan_secondary(
        &self,
        ir: &IniReader,
        fallback_char_info: u64,
        write_out: bool,
    ) -> String {
        if !self.executable_exists() {
            return "Error: Could not locate the specified executable file.".to_string();
        }

        let char_info_base = {
            let start = ir.read_pattern_int("CharInfo", "Start");
            let pattern = ir.read_pattern_bytes("CharInfo", "Pattern");
            let mask = ir.read_pattern_string("CharInfo", "Mask");
            let found = self.find_eq_pointer_offset(start, SCAN_WINDOW, &pattern, mask.as_bytes());
            if found != 0 {
                found
            } else {
                fallback_char_info
            }
        };

        let mut out = String::from("[SpawnInfo Offsets]\r\n");
        for entry in SECONDARY_SCANS {
            out.push_str(&self.run_secondary_scan(ir, entry, char_info_base, write_out));
        }
        out
    }
}

/// Hinnant civil_from_days: days since Unix epoch → (year, month 1-based, day).
fn civil_from_days(days: i64) -> (i64, u64, u64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Hinnant days_from_civil: (year, month 1-based, day) → days since Unix epoch.
fn civil_to_days(y: i64, m: u64, d: u64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m_adj = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * m_adj + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

/// Day of week from days since epoch: 0=Sun, 1=Mon, …, 6=Sat.
fn weekday_from_days(days: i64) -> u64 {
    (days + 4).rem_euclid(7) as u64
}

/// Day-of-month of the nth occurrence of `weekday` (0=Sun) in year/month.
fn nth_weekday_of_month(year: i64, month: u64, weekday: u64, n: u64) -> u64 {
    let first_dow = weekday_from_days(civil_to_days(year, month, 1));
    let offset = (weekday + 7 - first_dow) % 7;
    1 + offset + (n - 1) * 7
}

/// Pacific UTC offset in hours: -7 (PDT) or -8 (PST).
/// US DST: 2nd Sunday of March 02:00 PST (= 10:00 UTC) → 1st Sunday of November 02:00 PDT (= 09:00 UTC).
fn pacific_utc_offset_hours(utc_secs: u64) -> i64 {
    let (year, _, _) = civil_from_days((utc_secs / 86400) as i64);
    let mar_sun2 = nth_weekday_of_month(year, 3, 0, 2);
    let nov_sun1 = nth_weekday_of_month(year, 11, 0, 1);
    let dst_start = civil_to_days(year, 3, mar_sun2) as u64 * 86400 + 10 * 3600;
    let dst_end = civil_to_days(year, 11, nov_sun1) as u64 * 86400 + 9 * 3600;
    if utc_secs >= dst_start && utc_secs < dst_end {
        -7
    } else {
        -8
    }
}

/// Convert a UTC Unix timestamp to ("HH:MM:SS", "Mon DD YYYY") in Pacific time,
/// matching the format of C's __TIME__ / __DATE__ macros used in EQ build strings.
fn unix_to_compile_time_date(utc_secs: u64) -> (String, String) {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let local_secs = (utc_secs as i64 + pacific_utc_offset_hours(utc_secs) * 3600) as u64;
    let tod = local_secs % 86400;
    let time_str = format!("{:02}:{:02}:{:02}", tod / 3600, (tod % 3600) / 60, tod % 60);
    let (y, m, d) = civil_from_days((local_secs / 86400) as i64);
    // C __DATE__: single-digit days are space-padded ("Apr  1 2026")
    let date_str = format!("{} {:2} {}", MONTHS[(m - 1) as usize], d, y);
    (time_str, date_str)
}

/// Convert a Unix timestamp to M/D/YYYY using the Hinnant civil calendar algorithm.
fn unix_to_date(secs: u64) -> String {
    let z = (secs / 86400) as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:02}/{:02}/{}", m, d, y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse_escape_bytes;

    #[test]
    fn compare_data_exact_and_wildcard() {
        let byte_mask = &[0x8Bu8, 0xAA, 0xFF];
        let char_mask = b"xxt";
        assert!(compare_data(&[0x8B, 0xAA, 0x00], byte_mask, char_mask));
        assert!(!compare_data(&[0x8B, 0xAB, 0x00], byte_mask, char_mask));
        assert!(!compare_data(&[0x00, 0xAA, 0x00], byte_mask, char_mask));
    }

    #[test]
    fn compare_data_o_and_r_also_require_match() {
        let byte_mask = &[0x11u8, 0x22, 0x33];
        assert!(compare_data(&[0x11, 0x22, 0x33], byte_mask, b"oxr"));
        assert!(!compare_data(&[0x11, 0xFF, 0x33], byte_mask, b"oxr"));
    }

    #[test]
    fn scan_buffer_finds_4byte_value() {
        // Pattern at offset 2: [0x8B, _, _, _, _, 0xFF]
        // char_mask "xttttx" -> extract 4 bytes at t_offset=1 -> [0x78,0x56,0x34,0x12] = 0x12345678
        let buffer = &[0x00u8, 0x00, 0x8B, 0x78, 0x56, 0x34, 0x12, 0xFF, 0x00];
        let byte_mask = &[0x8Bu8, 0xAA, 0xBB, 0xCC, 0xDD, 0xFF];
        let char_mask = b"xttttx";
        assert_eq!(
            scan_buffer_for_pointer(buffer, byte_mask, char_mask, 0, None),
            0x12345678
        );
    }

    #[test]
    fn scan_buffer_threshold_rejects_large_value() {
        // Extracted 0x30000000 > 0x20000000 -> not returned
        let buffer = &[0x8Bu8, 0x00, 0x00, 0x00, 0x30, 0xFF];
        let byte_mask = &[0x8Bu8, 0xAA, 0xBB, 0xCC, 0xDD, 0xFF];
        let char_mask = b"xttttx";
        assert_eq!(
            scan_buffer_for_pointer(buffer, byte_mask, char_mask, 0, None),
            0
        );
    }

    #[test]
    fn scan_buffer_no_match_returns_zero() {
        let buffer = &[0x00u8; 16];
        let byte_mask = &[0xFFu8, 0xFF];
        let char_mask = b"xx";
        assert_eq!(
            scan_buffer_for_pointer(buffer, byte_mask, char_mask, 0, None),
            0
        );
    }

    #[test]
    fn parse_escape_bytes_decodes_hex_sequences() {
        let bytes = parse_escape_bytes(r"\x6A\x20\x8B");
        assert_eq!(bytes, &[0x6A, 0x20, 0x8B]);
    }

    #[test]
    fn parse_escape_bytes_ignores_non_escape_chars() {
        // Only \xNN sequences contribute; other chars are discarded
        let bytes = parse_escape_bytes("hello\\x41world\\x42");
        assert_eq!(bytes, &[0x41, 0x42]);
    }

    #[test]
    fn unix_to_date_epoch() {
        assert_eq!(unix_to_date(0), "01/01/1970");
    }

    #[test]
    fn unix_to_date_known_date() {
        // 2009-04-15 = 14_350 days from epoch = 1_239_753_600 seconds
        assert_eq!(unix_to_date(1_239_753_600), "04/15/2009");
    }
}
