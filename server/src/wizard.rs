// Offset Wizard — auto-discovers myseqserver.ini fields from a live eqgame.exe.
// Runs as a background thread; communicates with the GUI via Arc<Mutex<WizardShared>>.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::{IniReader, PrimaryOffsets};
use crate::mem_reader::MemReader;
use crate::scanner::EqGameScanner;
use crate::server_logic::{read_f32_at, read_u32_at, read_u64_at};

/// Bytes to read from the spawn struct per tick (matches debug loop `es` command).
const STRUCT_SIZE: usize = 0x1800;

/// Bytes to read from a ground item struct.
const ITEM_SIZE: usize = 0x200;

/// Minimum float delta to count as "changed".
const FLOAT_DELTA: f32 = 0.5;

/// How long to wait for a stable baseline before starting movement detection.
const STILL_WINDOW: Duration = Duration::from_millis(2500);

/// How long to collect movement samples.
const WALK_WINDOW: Duration = Duration::from_millis(5000);

/// How long to collect turning samples.
const TURN_WINDOW: Duration = Duration::from_millis(3000);

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Debug)]
pub enum WizardPhase {
    Idle,
    ScanningExe,
    Attaching,
    EnterName,  // user enters character name for exact-match discovery
    Static,     // name, lastname, next, prev from first struct read
    StandStill, // establish stable float baseline
    Walking,    // detect position/speed/heading candidates
    Stopped,    // identify speed offset (drops to 0)
    Turning,    // identify heading offset (changes without position change)
    WaitInvis,  // user casts invis → detect HideOffset byte flip
    WaitPet,    // user summons pet → detect OwnerIDOffset
    WaitItem,   // user drops item → detect GroundItem offsets
    Verify,     // live memory readback — user confirms before writing to INI
    Complete,
    Cancelled,
    Failed,
}

impl WizardPhase {
    pub fn instruction(&self) -> &'static str {
        match self {
            Self::Idle => "Click 'Start Wizard' to begin.",
            Self::ScanningExe => "Scanning eqgame.exe — please wait...",
            Self::Attaching => "Attaching to eqgame.exe — please wait...",
            Self::EnterName => {
                "Enter your character name below, then click 'Confirm Name' (or Skip to use heuristic)."
            }
            Self::Static => "Reading spawn struct...",
            Self::StandStill => "Stand completely still for a moment...",
            Self::Walking => "Walk around continuously for a few seconds...",
            Self::Stopped => "Stop moving completely...",
            Self::Turning => "Turn in place (do not move forward or backward)...",
            Self::WaitInvis => "Cast invisibility, then click 'I cast invis'.",
            Self::WaitPet => "Summon a pet or hire a mercenary, then click 'I have a pet'.",
            Self::WaitItem => "Drop any item on the ground, then click 'Item dropped'.",
            Self::Verify => {
                "Confirm the values below look correct, then click 'Accept & Write to INI'."
            }
            Self::Complete => "Discovery complete. Review results and click 'Write to INI'.",
            Self::Cancelled => "Cancelled.",
            Self::Failed => "Wizard failed — see log for details.",
        }
    }
}

/// Live readings taken during WizardPhase::Verify.
#[derive(Clone, Default)]
pub struct VerifyReadings {
    pub name: String,
    pub name_ok: Option<bool>, // None = char_name was skipped (no exact-match basis)
    pub zone: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub pos_ok: bool, // all three within ±15_000.0
    pub heading: f32,
    pub heading_ok: bool, // 0.0..=512.0
    pub level: u8,
    pub level_ok: bool, // 1..=120
    pub spawn_count: usize,
    pub spawn_count_ok: bool, // >= 1
}

#[derive(Clone, PartialEq)]
pub enum WizardCommand {
    None,
    ActionDone, // user clicked phase action button (cast invis / have pet / dropped item)
    SkipStep,
    Cancel,
}

#[derive(Clone, Default)]
pub struct WizardResults {
    // SpawnInfo fields
    pub name: Option<usize>,
    pub last_name: Option<usize>,
    pub next: Option<usize>,
    pub prev: Option<usize>,
    pub x: Option<usize>,
    pub y: Option<usize>,
    pub z: Option<usize>,
    pub heading: Option<usize>,
    pub speed: Option<usize>,
    pub hidden: Option<usize>,
    pub owner: Option<usize>,
    // GroundItem fields
    pub item_prev: Option<usize>,
    pub item_next: Option<usize>,
    pub item_id: Option<usize>,
    pub item_drop_id: Option<usize>,
    pub item_x: Option<usize>,
    pub item_y: Option<usize>,
    pub item_z: Option<usize>,
    pub item_name: Option<usize>,
    pub name_confirmed: bool,
}

pub struct WizardShared {
    pub phase: WizardPhase,
    pub log: Vec<String>,
    pub results: WizardResults,
    pub command: WizardCommand,
    // filled after ScanningExe phase
    pub char_info_addr: u64,
    pub spawn_header_addr: u64,
    pub ground_addr: u64,
    pub spawn_id_offset: usize,
    // filled after Static phase
    pub player_spawn_id: u32,
    // entered by user during EnterName phase
    pub char_name: String,
    pub char_last_name: String,
    // populated from scan before wizard starts; written to INI on "Write to INI"
    pub scan_primary: PrimaryOffsets,
    pub scan_secondary: Vec<(String, u64)>,
    pub scan_file_info: Vec<(String, String)>,
    // populated during Verify phase
    pub verify: Option<VerifyReadings>,
}

impl Default for WizardShared {
    fn default() -> Self {
        Self {
            phase: WizardPhase::Idle,
            log: Vec::new(),
            results: WizardResults::default(),
            command: WizardCommand::None,
            char_info_addr: 0,
            spawn_header_addr: 0,
            ground_addr: 0,
            spawn_id_offset: 0,
            player_spawn_id: 0,
            char_name: String::new(),
            char_last_name: String::new(),
            scan_primary: PrimaryOffsets::default(),
            scan_secondary: Vec::new(),
            scan_file_info: Vec::new(),
            verify: None,
        }
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

pub fn start_wizard(
    ini_path: String,
    config_ini_path: String,
    patterns_ini_path: String,
    exe_path: String,
    shared: Arc<Mutex<WizardShared>>,
    skip_scan: bool,
) {
    std::thread::spawn(move || {
        run_wizard(
            ini_path,
            config_ini_path,
            patterns_ini_path,
            exe_path,
            shared,
            skip_scan,
        );
    });
}

/// Write all discovered results back to myseqserver.ini.
/// Includes primary addresses and secondary offsets from the scan, plus wizard fields.
/// Returns a summary of what was written.
pub fn write_wizard_results(
    results: &WizardResults,
    scan_primary: &PrimaryOffsets,
    scan_secondary: &[(String, u64)],
    scan_file_info: &[(String, String)],
    ini_path: &str,
    config_ini_path: &str,
) -> String {
    let mut ir = IniReader::new();
    ir.open_config_file(config_ini_path);
    let _ = ir.open_file(ini_path);

    let mut written = Vec::new();
    let mut skipped = Vec::new();

    let fmt_addr = |v: u64| format!("0x{:x}", v);

    // File info from scan (PatchDate, ClientHash, BuildString)
    for (key, val) in scan_file_info {
        if !val.is_empty() {
            if ir.write_string_entry("File Info", key, val, false) {
                written.push(format!("File Info.{} = {}", key, val));
            } else {
                skipped.push(format!("File Info.{}", key));
            }
        }
    }

    // [Port] — preserve existing value or default to 5555
    let port = {
        let existing = ir.read_integer_entry("Port", "Port", false) as u16;
        if existing != 0 { existing } else { 5555 }
    };
    ir.write_string_entry("Port", "Port", &port.to_string(), false);

    let mut write_mem = |key: &str, val: u64| {
        if val != 0 {
            if ir.write_string_entry("Memory Offsets", key, &fmt_addr(val), false) {
                written.push(format!("{} = 0x{:x}", key, val));
            } else {
                skipped.push(key.to_string());
            }
        }
    };

    // [Memory Offsets] — primary addresses from scan
    write_mem("ZoneAddr", scan_primary.zone_name);
    write_mem("SpawnHeaderAddr", scan_primary.spawn_list);
    write_mem("CharInfo", scan_primary.self_addr);
    write_mem("ItemsAddr", scan_primary.ground);
    write_mem("TargetAddr", scan_primary.target);
    write_mem("WorldAddr", scan_primary.world);

    // [WorldInfo Offsets] — fixed constants, never change across patches
    let world_info: &[(&str, &str)] = &[
        ("WorldHourOffset", "8"),
        ("WorldMinuteOffset", "9"),
        ("WorldDayOffset", "10"),
        ("WorldMonthOffset", "11"),
        ("WorldYearOffset", "12"),
    ];
    for (key, val) in world_info {
        if ir.write_string_entry("WorldInfo Offsets", key, val, false) {
            written.push(format!("WorldInfo.{} = {}", key, val));
        } else {
            skipped.push(format!("WorldInfo.{}", key));
        }
    }

    // [SpawnInfo Offsets] — secondary offsets from scan
    for (key, val) in scan_secondary {
        if *val != 0 {
            if ir.write_string_entry("SpawnInfo Offsets", key, &fmt_addr(*val), false) {
                written.push(format!("{} = 0x{:x}", key, val));
            } else {
                skipped.push(key.clone());
            }
        }
    }

    let mut write_spawn = |key: &str, val: Option<usize>| {
        if let Some(v) = val {
            let s = format!("0x{:x}", v);
            if ir.write_string_entry("SpawnInfo Offsets", key, &s, false) {
                written.push(format!("{} = 0x{:x}", key, v));
            } else {
                skipped.push(key.to_string());
            }
        }
    };

    // [SpawnInfo Offsets] — wizard-discovered fields:
    //   NextOffset/PrevOffset      — doubly-linked list invariant (spawn.next.prev == spawn)
    //   NameOffset/LastnameOffset  — exact match on user-provided character name
    //   XOffset/YOffset/ZOffset    — consecutive 4-byte float cluster during movement
    //   HeadingOffset              — float in [0,512] changing only during turning
    //   HideOffset                 — single byte flipping 0→1 after casting invisibility
    //   OwnerIDOffset              — u32 matching the player's known spawn ID
    //
    // Excluded (heuristics too error-prone):
    //   NameOffset/LastnameOffset — when name not confirmed by user (heuristic only)
    write_spawn("NextOffset", results.next);
    write_spawn("PrevOffset", results.prev);
    if results.name_confirmed {
        write_spawn("NameOffset", results.name);
        write_spawn("LastnameOffset", results.last_name);
    }
    write_spawn("XOffset", results.x);
    write_spawn("YOffset", results.y);
    write_spawn("ZOffset", results.z);
    write_spawn("SpeedOffset", results.speed);
    write_spawn("HeadingOffset", results.heading);
    write_spawn("HideOffset", results.hidden);
    write_spawn("OwnerIDOffset", results.owner);

    // [GroundItem Offsets] — wizard-discovered item fields
    let mut write_item = |key: &str, val: Option<usize>| {
        if let Some(v) = val {
            let s = format!("0x{:x}", v);
            if ir.write_string_entry("GroundItem Offsets", key, &s, false) {
                written.push(format!("{} = 0x{:x}", key, v));
            } else {
                skipped.push(key.to_string());
            }
        }
    };

    write_item("PrevOffset", results.item_prev);
    write_item("NextOffset", results.item_next);
    write_item("IdOffset", results.item_id);
    write_item("DropIdOffset", results.item_drop_id);
    write_item("XOffset", results.item_x);
    write_item("YOffset", results.item_y);
    write_item("ZOffset", results.item_z);
    write_item("NameOffset", results.item_name);

    let mut out = String::new();
    if !written.is_empty() {
        out.push_str(&format!("Written ({}):\r\n", written.len()));
        for w in &written {
            out.push_str(&format!("  {}\r\n", w));
        }
    }
    if !skipped.is_empty() {
        out.push_str(&format!("Write failed ({}):\r\n", skipped.len()));
        for s in &skipped {
            out.push_str(&format!("  {}\r\n", s));
        }
    }
    if written.is_empty() && skipped.is_empty() {
        out.push_str("Nothing to write (no fields discovered).\r\n");
    }
    out
}

// ── Background worker ─────────────────────────────────────────────────────────

fn run_wizard(
    ini_path: String,
    config_ini_path: String,
    patterns_ini_path: String,
    exe_path: String,
    shared: Arc<Mutex<WizardShared>>,
    skip_scan: bool,
) {
    macro_rules! log {
        ($msg:expr) => {
            if let Ok(mut s) = shared.lock() { s.log.push($msg.to_string()); }
        };
        ($fmt:literal, $($arg:tt)*) => {
            if let Ok(mut s) = shared.lock() { s.log.push(format!($fmt, $($arg)*)); }
        };
    }
    macro_rules! set_phase {
        ($p:expr) => {
            if let Ok(mut s) = shared.lock() {
                s.phase = $p;
            }
        };
    }
    macro_rules! check_cancel {
        () => {
            if let Ok(mut s) = shared.lock() {
                if s.command == WizardCommand::Cancel {
                    s.command = WizardCommand::None;
                    s.phase = WizardPhase::Cancelled;
                    return;
                }
            }
        };
    }
    macro_rules! take_command {
        () => {{
            let mut cmd = WizardCommand::None;
            if let Ok(mut s) = shared.lock() {
                cmd = s.command.clone();
                if cmd != WizardCommand::None {
                    s.command = WizardCommand::None;
                }
            }
            cmd
        }};
    }

    // ── Phase 1: scan the exe file (skipped when launched from combined run) ──
    let mut ir = IniReader::new();
    ir.open_config_file(&config_ini_path);
    ir.open_patterns_file(&patterns_ini_path);
    let _ = ir.open_file(&ini_path);
    let current_offsets = ir
        .read_server_config_model()
        .map(|m| m.offsets)
        .unwrap_or_default();

    if !skip_scan {
        set_phase!(WizardPhase::ScanningExe);
        log!("Scanning {}...", exe_path);

        let scanner = EqGameScanner::new(&exe_path);
        if !scanner.executable_exists() {
            log!("Error: exe not found — {}", exe_path);
            set_phase!(WizardPhase::Failed);
            return;
        }

        let scan_result = scanner.scan_executable(&ir, &current_offsets, false);
        for line in scan_result.output.lines() {
            let t = line.trim();
            if !t.is_empty() {
                log!("{}", t);
            }
        }

        let secondary = scanner.scan_secondary(&ir, current_offsets.self_addr, false);
        for line in secondary.lines() {
            let t = line.trim();
            if !t.is_empty() {
                log!("{}", t);
            }
        }
    }

    let (char_info_addr, spawn_header_addr, ground_addr, spawn_id_offset) = {
        let s = shared.lock().unwrap();
        let cid = if s.scan_primary.self_addr != 0 {
            s.scan_primary.self_addr
        } else {
            current_offsets.self_addr
        };
        let shl = if s.scan_primary.spawn_list != 0 {
            s.scan_primary.spawn_list
        } else {
            current_offsets.spawn_list
        };
        let gnd = if s.scan_primary.ground != 0 {
            s.scan_primary.ground
        } else {
            current_offsets.ground
        };
        let sio = s
            .scan_secondary
            .iter()
            .find(|(k, _)| k == "SpawnIDOffset")
            .map(|(_, v)| *v as usize)
            .unwrap_or_else(|| {
                ir.read_integer_entry("SpawnInfo Offsets", "SpawnIDOffset", false) as usize
            });
        (cid, shl, gnd, sio)
    };

    if char_info_addr == 0 || spawn_header_addr == 0 {
        log!(
            "Error: primary addresses not found — run Update Offsets first to populate Memory Offsets."
        );
        set_phase!(WizardPhase::Failed);
        return;
    }

    if let Ok(mut s) = shared.lock() {
        s.char_info_addr = char_info_addr;
        s.spawn_header_addr = spawn_header_addr;
        s.ground_addr = ground_addr;
        s.spawn_id_offset = spawn_id_offset;
    }

    check_cancel!();

    // ── Phase 2: attach to live process ─────────────────────────────────────
    set_phase!(WizardPhase::Attaching);
    log!("Attaching to eqgame.exe...");

    MemReader::enable_debug_privileges();
    let mut mem = MemReader::new();
    let attach_deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if Instant::now() > attach_deadline {
            log!("Error: could not attach within 30s — is EQ running?");
            set_phase!(WizardPhase::Failed);
            return;
        }
        check_cancel!();
        if let Some(pid) = MemReader::find_process("eqgame.exe")
            && mem.open(pid).is_ok()
        {
            log!(
                "Attached — PID {} Base 0x{:X}",
                mem.pid(),
                mem.base_address()
            );
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    let pself = match get_pself(&mem, char_info_addr) {
        Some(p) if p != 0 => p,
        _ => {
            log!("Error: pSelf is null — is the player logged in?");
            set_phase!(WizardPhase::Failed);
            return;
        }
    };
    log!("pSelf = 0x{:X}", pself);

    // ── Phase 2b: enter character name ──────────────────────────────────────
    set_phase!(WizardPhase::EnterName);
    log!(
        "Enter your character name and optionally your surname, then click 'Confirm Name' (or Skip to use heuristic)."
    );

    loop {
        check_cancel!();
        std::thread::sleep(Duration::from_millis(200));
        match take_command!() {
            WizardCommand::ActionDone | WizardCommand::SkipStep => break,
            WizardCommand::Cancel => {
                set_phase!(WizardPhase::Cancelled);
                return;
            }
            _ => {}
        }
    }

    // ── Phase 3: static discovery ────────────────────────────────────────────
    set_phase!(WizardPhase::Static);

    let buf = match mem.read_bytes(pself, STRUCT_SIZE) {
        Ok(b) => b,
        Err(e) => {
            log!("Error reading struct: {}", e);
            set_phase!(WizardPhase::Failed);
            return;
        }
    };

    // Name / Lastname — exact match on user-provided names, else heuristic
    let (char_name, char_last_name) = shared
        .lock()
        .map(|s| (s.char_name.clone(), s.char_last_name.clone()))
        .unwrap_or_default();

    if !char_name.is_empty() {
        match find_name_by_value(&buf, &char_name) {
            Some(off) => {
                log!("Name → 0x{:x} (\"{}\")", off, char_name);
                if let Ok(mut s) = shared.lock() {
                    s.results.name = Some(off);
                    s.results.name_confirmed = true;
                }
            }
            None => log!(
                "Name — \"{}\" not found in struct (is player logged in?)",
                char_name
            ),
        }
        if !char_last_name.is_empty() {
            match find_name_by_value(&buf, &char_last_name) {
                Some(off) => {
                    log!("Lastname → 0x{:x} (\"{}\")", off, char_last_name);
                    if let Ok(mut s) = shared.lock() {
                        s.results.last_name = Some(off);
                    }
                }
                None => log!("Lastname — \"{}\" not found in struct", char_last_name),
            }
        } else {
            log!("Lastname — skipped (no surname entered)");
        }
    } else {
        // No name provided — fall back to heuristic
        let name_candidates = find_name_candidates(&buf);
        match name_candidates.len() {
            0 => log!("Name/Lastname — not found (is player logged in and named?)"),
            1 => {
                if let Ok(mut s) = shared.lock() {
                    s.results.name = Some(name_candidates[0].0);
                }
                log!(
                    "Name → 0x{:x} (\"{}\")",
                    name_candidates[0].0,
                    name_candidates[0].1
                );
                log!("Lastname — not found");
            }
            _ => {
                if let Ok(mut s) = shared.lock() {
                    s.results.name = Some(name_candidates[0].0);
                    s.results.last_name = Some(name_candidates[1].0);
                }
                log!(
                    "Name → 0x{:x} (\"{}\")",
                    name_candidates[0].0,
                    name_candidates[0].1
                );
                log!(
                    "Lastname → 0x{:x} (\"{}\")",
                    name_candidates[1].0,
                    name_candidates[1].1
                );
            }
        }
    }

    // Next / Prev — validated via doubly-linked list invariant:
    // spawn.next.prev == spawn_addr
    match find_next_prev_offsets(&mem, pself) {
        Ok((next_off, prev_off)) => {
            log!("Next → 0x{:x}  Prev → 0x{:x}", next_off, prev_off);
            if let Ok(mut s) = shared.lock() {
                s.results.next = Some(next_off);
                s.results.prev = Some(prev_off);
            }
        }
        Err(diag) => log!("Next/Prev — not found: {}", diag),
    }

    // Player SpawnID (needed later for OwnerIDOffset discovery)
    if spawn_id_offset > 0 && spawn_id_offset + 4 <= buf.len() {
        let sid = read_u32_at(&buf, spawn_id_offset);
        if let Ok(mut s) = shared.lock() {
            s.player_spawn_id = sid;
        }
        log!("Player SpawnID = {}", sid);
    }

    check_cancel!();

    // ── Phase 4: stand still → establish float baseline ──────────────────────
    set_phase!(WizardPhase::StandStill);
    log!("Stand still completely, then click 'I'm standing still' to begin baseline collection.");

    let mut still_skipped = false;
    loop {
        check_cancel!();
        std::thread::sleep(Duration::from_millis(200));
        match take_command!() {
            WizardCommand::ActionDone => break,
            WizardCommand::SkipStep => {
                still_skipped = true;
                break;
            }
            WizardCommand::Cancel => {
                set_phase!(WizardPhase::Cancelled);
                return;
            }
            _ => {}
        }
    }

    let mut baseline = buf.clone();
    let mut stable_ticks = 0u32;
    if !still_skipped {
        log!("Collecting baseline for {}ms...", STILL_WINDOW.as_millis());
        let still_start = Instant::now();
        let mut prev_buf = buf;
        while still_start.elapsed() < STILL_WINDOW {
            check_cancel!();
            std::thread::sleep(Duration::from_millis(250));
            let Some(ps) = get_pself(&mem, char_info_addr) else {
                break;
            };
            let Ok(curr) = mem.read_bytes(ps, STRUCT_SIZE) else {
                break;
            };
            let changed = find_changed_floats(&prev_buf, &curr, -20000.0, 20000.0, FLOAT_DELTA);
            if changed.is_empty() {
                stable_ticks += 1;
                if stable_ticks >= 3 {
                    baseline = curr.clone();
                }
            } else {
                stable_ticks = 0;
            }
            prev_buf = curr;
        }
        log!("Baseline established ({} stable ticks).", stable_ticks);
    } else {
        drop(buf);
        log!("StandStill — skipped.");
    }
    check_cancel!();

    // ── Phase 5: walk → collect movement candidates ──────────────────────────
    set_phase!(WizardPhase::Walking);
    log!("Click 'Start walking', then walk around continuously for a few seconds.");

    let mut walk_skipped = false;
    loop {
        check_cancel!();
        std::thread::sleep(Duration::from_millis(200));
        match take_command!() {
            WizardCommand::ActionDone => break,
            WizardCommand::SkipStep => {
                walk_skipped = true;
                break;
            }
            WizardCommand::Cancel => {
                set_phase!(WizardPhase::Cancelled);
                return;
            }
            _ => {}
        }
    }

    let mut movement_union: Vec<usize> = Vec::new();
    let mut last_walking_buf: Vec<u8> = Vec::new();
    if !walk_skipped {
        log!(
            "Collecting movement data for {}ms...",
            WALK_WINDOW.as_millis()
        );
        let walk_start = Instant::now();
        while walk_start.elapsed() < WALK_WINDOW {
            check_cancel!();
            std::thread::sleep(Duration::from_millis(250));
            let Some(ps) = get_pself(&mem, char_info_addr) else {
                break;
            };
            let Ok(curr) = mem.read_bytes(ps, STRUCT_SIZE) else {
                break;
            };
            let changed = find_changed_floats(&baseline, &curr, -20000.0, 20000.0, FLOAT_DELTA);
            for off in changed {
                if !movement_union.contains(&off) {
                    movement_union.push(off);
                }
            }
            last_walking_buf = curr;
        }
        movement_union.sort();
        log!("Movement candidates: {}", movement_union.len());
    } else {
        log!("Walking — skipped.");
    }
    check_cancel!();

    // ── Phase 6: stop → find speed (drops back to ~0) ────────────────────────
    set_phase!(WizardPhase::Stopped);
    log!("Stop moving completely, then click 'I'm stopped'.");

    let mut stop_skipped = false;
    loop {
        check_cancel!();
        std::thread::sleep(Duration::from_millis(200));
        match take_command!() {
            WizardCommand::ActionDone => break,
            WizardCommand::SkipStep => {
                stop_skipped = true;
                break;
            }
            WizardCommand::Cancel => {
                set_phase!(WizardPhase::Cancelled);
                return;
            }
            _ => {}
        }
    }

    let Some(ps) = get_pself(&mem, char_info_addr) else {
        log!("Lost pSelf");
        set_phase!(WizardPhase::Failed);
        return;
    };
    let Ok(stopped_buf) = mem.read_bytes(ps, STRUCT_SIZE) else {
        log!("Read error");
        set_phase!(WizardPhase::Failed);
        return;
    };

    if stop_skipped {
        log!("Stopped — skipped.");
    }
    // Speed detection is deferred until after X/Y/Z and Heading are known
    // so we can exclude those offsets from the candidate set.
    check_cancel!();

    // ── Phase 7: turn → find heading ─────────────────────────────────────────
    set_phase!(WizardPhase::Turning);
    log!("Click 'Start turning', then turn in place (do not move forward or backward).");

    let mut turn_skipped = false;
    loop {
        check_cancel!();
        std::thread::sleep(Duration::from_millis(200));
        match take_command!() {
            WizardCommand::ActionDone => break,
            WizardCommand::SkipStep => {
                turn_skipped = true;
                break;
            }
            WizardCommand::Cancel => {
                set_phase!(WizardPhase::Cancelled);
                return;
            }
            _ => {}
        }
    }

    let mut heading_changed: Vec<usize> = Vec::new();
    if !turn_skipped {
        log!(
            "Collecting turning data for {}ms...",
            TURN_WINDOW.as_millis()
        );
        let turn_start = Instant::now();
        let mut turn_prev = stopped_buf.clone();
        while turn_start.elapsed() < TURN_WINDOW {
            check_cancel!();
            std::thread::sleep(Duration::from_millis(250));
            let Some(ps) = get_pself(&mem, char_info_addr) else {
                break;
            };
            let Ok(curr) = mem.read_bytes(ps, STRUCT_SIZE) else {
                break;
            };
            for off in find_changed_floats(&turn_prev, &curr, 0.0, 512.0, FLOAT_DELTA * 0.5) {
                if !heading_changed.contains(&off) {
                    heading_changed.push(off);
                }
            }
            turn_prev = curr;
        }
    } else {
        log!("Turning — skipped.");
    }

    // Heading must also have changed during walking (was in movement_union)
    let heading_candidates: Vec<usize> = heading_changed
        .iter()
        .filter(|&&o| movement_union.contains(&o))
        .copied()
        .collect();

    let heading_offset = heading_candidates
        .first()
        .or_else(|| heading_changed.first())
        .copied();

    if let Some(off) = heading_offset {
        log!("Heading → 0x{:x}", off);
        if let Ok(mut s) = shared.lock() {
            s.results.heading = Some(off);
        }
        movement_union.retain(|&o| o != off);
    } else {
        log!("Heading — not found");
    }

    // Remaining movement_union candidates → X, Y, Z.
    // Z often does not change on flat terrain and may be absent from movement_union,
    // causing a 3-cluster search to skip X/Y/Z and land on a later spurious cluster.
    // Find the earliest consecutive pair (X, Y) and infer Z = Y + 4.
    let pair = find_consecutive_cluster(&movement_union, 2, 4);
    let (x_off, y_off, z_off) = if pair.len() >= 2 {
        let inferred_z = pair[1] + 4;
        if !movement_union.contains(&inferred_z) {
            log!(
                "Z not in movement candidates (flat terrain?); inferring Z = 0x{:x}",
                inferred_z
            );
        }
        (pair[0], pair[1], inferred_z)
    } else {
        log!(
            "Position — only {} movement candidates, no consecutive pair found",
            movement_union.len()
        );
        if let Some(&a) = movement_union.first() {
            (a, 0, 0)
        } else {
            (0, 0, 0)
        }
    };

    if x_off != 0 {
        log!("X → 0x{:x}  Y → 0x{:x}  Z → 0x{:x}", x_off, y_off, z_off);
        if let Ok(mut s) = shared.lock() {
            s.results.x = Some(x_off);
            if y_off != 0 {
                s.results.y = Some(y_off);
            }
            if z_off != 0 {
                s.results.z = Some(z_off);
            }
        }
    }
    // ── Speed detection (deferred): now that X/Y/Z and Heading are known ────────
    if !stop_skipped {
        let (known, heading_off_opt): (std::collections::HashSet<usize>, Option<usize>) = {
            let s = shared.lock().ok();
            let heading = s.as_ref().and_then(|s| s.results.heading);
            let known = [
                s.as_ref().and_then(|s| s.results.x),
                s.as_ref().and_then(|s| s.results.y),
                s.as_ref().and_then(|s| s.results.z),
                heading,
            ]
            .into_iter()
            .flatten()
            .collect();
            (known, heading)
        };

        // Primary: SpeedOffset is structurally always at HeadingOffset - 4 in EQ's spawn struct.
        // Verify it with the stopped snapshot to confirm it really does drop to ~0.
        let structural = heading_off_opt
            .and_then(|h| h.checked_sub(4))
            .filter(|&off| read_f32_at(&stopped_buf, off).abs() < 0.1);

        let speed_off = if let Some(off) = structural {
            log!("Speed → 0x{:x} (structural: heading - 4)", off);
            Some(off)
        } else {
            // Fallback: find movement_union candidates that dropped to ~0 when stopped,
            // were clearly nonzero while walking, and aren't a known offset.
            let mut candidates: Vec<usize> = movement_union
                .iter()
                .filter(|&&off| {
                    !known.contains(&off)
                        && read_f32_at(&stopped_buf, off).abs() < 0.1
                        && (last_walking_buf.is_empty()
                            || read_f32_at(&last_walking_buf, off).abs() > 0.5)
                })
                .copied()
                .collect();
            candidates.sort_by(|&a, &b| {
                read_f32_at(&last_walking_buf, b)
                    .abs()
                    .partial_cmp(&read_f32_at(&last_walking_buf, a).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            if let Some(&off) = candidates.first() {
                log!("Speed → 0x{:x} (heuristic fallback)", off);
                Some(off)
            } else {
                log!("Speed — not found");
                None
            }
        };

        if let Some(off) = speed_off
            && let Ok(mut s) = shared.lock()
        {
            s.results.speed = Some(off);
        }
    }
    check_cancel!();

    // ── Phase 8: wait for invis → HideOffset ─────────────────────────────────
    set_phase!(WizardPhase::WaitInvis);
    log!("Cast invisibility, then click 'I cast invis' (or Skip).");

    let pself = get_pself(&mem, char_info_addr).unwrap_or(0);
    let pre_invis = if pself != 0 {
        mem.read_bytes(pself, STRUCT_SIZE).unwrap_or_default()
    } else {
        vec![]
    };

    loop {
        check_cancel!();
        std::thread::sleep(Duration::from_millis(200));
        match take_command!() {
            WizardCommand::ActionDone => {
                let ps = get_pself(&mem, char_info_addr).unwrap_or(0);
                if ps != 0 && !pre_invis.is_empty() {
                    let post = mem.read_bytes(ps, STRUCT_SIZE).unwrap_or_default();
                    let flipped = find_byte_flip(&pre_invis, &post, 0, 1);
                    match flipped.len() {
                        0 => log!("Hide — no byte changed 0→1 (cast invis first?)"),
                        1 => {
                            log!("Hide → 0x{:x}", flipped[0]);
                            if let Ok(mut s) = shared.lock() {
                                s.results.hidden = Some(flipped[0]);
                            }
                        }
                        n => {
                            log!("Hide — {} bytes flipped, ambiguous:", n);
                            for off in flipped.iter().take(8) {
                                log!("  0x{:x}", off);
                            }
                        }
                    }
                }
                break;
            }
            WizardCommand::SkipStep => {
                log!("Hide — skipped");
                break;
            }
            WizardCommand::Cancel => {
                set_phase!(WizardPhase::Cancelled);
                return;
            }
            _ => {}
        }
    }
    check_cancel!();

    // ── Phase 9: wait for pet → OwnerIDOffset ────────────────────────────────
    set_phase!(WizardPhase::WaitPet);
    log!("Summon a pet or hire a mercenary, then click 'I have a pet' (or Skip).");

    let player_spawn_id = shared.lock().map(|s| s.player_spawn_id).unwrap_or(0);
    // Read Next/Prev from the ini rather than using heuristic discoveries —
    // the pointer scan is unreliable and wrong values break spawn list traversal.
    let next_off = {
        let v = ir.read_integer_entry("SpawnInfo Offsets", "NextOffset", false) as usize;
        if v > 0 { v } else { 0x8 }
    };
    let prev_off = {
        let v = ir.read_integer_entry("SpawnInfo Offsets", "PrevOffset", false) as usize;
        if v > 0 { v } else { 0x10 }
    };

    loop {
        check_cancel!();
        std::thread::sleep(Duration::from_millis(200));
        match take_command!() {
            WizardCommand::ActionDone => {
                if spawn_header_addr == 0 {
                    log!("Owner — cannot search: SpawnHeaderAddr not found by scan");
                } else {
                    if player_spawn_id != 0 {
                        log!("Owner search — player SpawnID = {}", player_spawn_id);
                    } else {
                        log!(
                            "Owner search — player SpawnID unknown, proceeding by spawn-list statistics"
                        );
                    }
                    let pself_buf = get_pself(&mem, char_info_addr)
                        .and_then(|ps| mem.read_bytes(ps, STRUCT_SIZE).ok());
                    match find_owner_offset(
                        &mem,
                        spawn_header_addr,
                        player_spawn_id,
                        next_off,
                        prev_off,
                        pself_buf.as_deref(),
                    ) {
                        Some((off, diag)) => {
                            log!("Owner candidates: {}", diag);
                            log!("Owner → 0x{:x}", off);
                            if let Ok(mut s) = shared.lock() {
                                s.results.owner = Some(off);
                            }
                        }
                        None => log!(
                            "Owner — not found in spawn list (is pet/merc active and in range?)"
                        ),
                    }
                }
                break;
            }
            WizardCommand::SkipStep => {
                log!("Owner — skipped");
                break;
            }
            WizardCommand::Cancel => {
                set_phase!(WizardPhase::Cancelled);
                return;
            }
            _ => {}
        }
    }
    check_cancel!();

    // ── Phase 10: wait for ground item ───────────────────────────────────────
    set_phase!(WizardPhase::WaitItem);
    log!("Drop any item on the ground, then click 'Item dropped' (or Skip).");

    loop {
        check_cancel!();
        std::thread::sleep(Duration::from_millis(200));
        match take_command!() {
            WizardCommand::ActionDone => {
                if ground_addr == 0 {
                    log!("GroundItem — ItemsAddr not found by scan");
                } else {
                    match discover_item_offsets(&mem, ground_addr) {
                        Some(r) => {
                            log!("GroundItem.Name  → 0x{:x}", r.item_name.unwrap_or(0));
                            log!(
                                "GroundItem.X/Y/Z → 0x{:x}/0x{:x}/0x{:x}",
                                r.item_x.unwrap_or(0),
                                r.item_y.unwrap_or(0),
                                r.item_z.unwrap_or(0)
                            );
                            log!(
                                "GroundItem.Prev/Next → 0x{:x}/0x{:x}",
                                r.item_prev.unwrap_or(0),
                                r.item_next.unwrap_or(0)
                            );
                            if let Ok(mut s) = shared.lock() {
                                s.results.item_name = r.item_name;
                                s.results.item_x = r.item_x;
                                s.results.item_y = r.item_y;
                                s.results.item_z = r.item_z;
                                s.results.item_prev = r.item_prev;
                                s.results.item_next = r.item_next;
                                s.results.item_id = r.item_id;
                                s.results.item_drop_id = r.item_drop_id;
                            }
                        }
                        None => log!(
                            "GroundItem — could not read item struct (is item on ground nearby?)"
                        ),
                    }
                }
                break;
            }
            WizardCommand::SkipStep => {
                log!("GroundItem — skipped");
                break;
            }
            WizardCommand::Cancel => {
                set_phase!(WizardPhase::Cancelled);
                return;
            }
            _ => {}
        }
    }

    // ── Phase 11: verify — live readback before user commits to INI ─────────
    set_phase!(WizardPhase::Verify);
    log!("Verify: reading live values — confirm they look correct before writing.");

    let results_snapshot = shared.lock().map(|s| s.results.clone()).unwrap_or_default();

    loop {
        check_cancel!();
        let readings = read_verify_readings(&mem, &shared, &results_snapshot);
        if let Ok(mut s) = shared.lock() {
            s.verify = Some(readings);
        }
        std::thread::sleep(Duration::from_millis(500));
        match take_command!() {
            WizardCommand::ActionDone => break, // user clicked Accept & Write to INI
            WizardCommand::Cancel => {
                set_phase!(WizardPhase::Cancelled);
                return;
            }
            _ => {}
        }
    }

    set_phase!(WizardPhase::Complete);
    log!("Discovery complete. Click 'Write to INI' to save results.");
}

// ── Memory helpers ────────────────────────────────────────────────────────────

fn get_pself(mem: &MemReader, char_info_canonical: u64) -> Option<u64> {
    if char_info_canonical == 0 {
        return None;
    }
    let ptr = mem.read_raw_pointer(char_info_canonical).ok()?;
    if ptr == 0 { None } else { Some(ptr) }
}

// ── Detection algorithms ──────────────────────────────────────────────────────

/// Scan buf for null-terminated ASCII name-like strings (A-Za-z'-space, len 3–30).
/// Returns (offset, string) pairs sorted by descending length (longest first = most likely name).
fn find_name_candidates(buf: &[u8]) -> Vec<(usize, String)> {
    let mut results: Vec<(usize, String)> = Vec::new();
    let mut i = 0;
    while i < buf.len() {
        if is_name_char(buf[i]) {
            let start = i;
            while i < buf.len() && is_name_char(buf[i]) {
                i += 1;
            }
            let len = i - start;
            if (3..=30).contains(&len) && i < buf.len() && buf[i] == 0 {
                // Must start with uppercase letter
                if buf[start].is_ascii_uppercase() {
                    let s = String::from_utf8_lossy(&buf[start..i]).into_owned();
                    results.push((start, s));
                }
            }
        } else {
            i += 1;
        }
    }
    // Sort by length descending, then by offset ascending (prefer lower offsets on tie)
    results.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    // Deduplicate: drop any entry whose string is a substring of an earlier entry
    let mut deduped: Vec<(usize, String)> = Vec::new();
    'outer: for (off, s) in results {
        for (_, existing) in &deduped {
            if existing.contains(&*s) {
                continue 'outer;
            }
        }
        deduped.push((off, s));
        if deduped.len() >= 4 {
            break;
        }
    }
    deduped
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'\'' || b == b' ' || b == b'-'
}

/// Scan buf for an exact null-terminated match of `name`. Returns the byte offset if found.
fn find_name_by_value(buf: &[u8], name: &str) -> Option<usize> {
    if name.is_empty() {
        return None;
    }
    let needle = name.as_bytes();
    buf.windows(needle.len() + 1)
        .position(|w| w[..needle.len()] == *needle && w[needle.len()] == 0)
}

/// Scan the first POINTER_SCAN_RANGE bytes of buf for 8-byte values that look like
/// heap pointers in the same memory region as pself (same top 3 bytes, non-zero, != pself).
/// Discover NextOffset and PrevOffset using the doubly-linked list invariant:
/// `spawn.next.prev == spawn_addr`. Uses pSelf (a known spawn struct address)
/// rather than the spawn list head, which may be a manager/header struct.
/// Returns Ok on success or Err with a diagnostic message.
fn find_next_prev_offsets(mem: &MemReader, pself: u64) -> Result<(usize, usize), String> {
    let spawn_addr = pself;
    if spawn_addr == 0 {
        return Err("pself is null".to_string());
    }

    let buf = mem
        .read_bytes(spawn_addr, 0x80)
        .map_err(|e| format!("read_bytes(pself=0x{:x}) failed: {}", spawn_addr, e))?;

    let spawn_region = spawn_addr >> 40;

    // Collect 8-byte-aligned offsets whose values look like pointers into the
    // same allocation region (same upper bytes). Self-referential pointers are
    // allowed: in a single-element list next == prev == spawn_addr, and the
    // invariant spawn.next.prev == spawn_addr still holds.
    let candidates: Vec<usize> = (0..0x80usize.min(buf.len().saturating_sub(8)))
        .step_by(8)
        .filter(|&off| {
            let v = read_u64_at(&buf, off);
            v != 0 && (v >> 40) == spawn_region
        })
        .collect();

    if candidates.is_empty() {
        // Dump what we actually found so the caller can diagnose region mismatches
        let found: Vec<String> = (0..0x80usize.min(buf.len().saturating_sub(8)))
            .step_by(8)
            .filter_map(|off| {
                let v = read_u64_at(&buf, off);
                if v != 0 {
                    Some(format!(
                        "  [0x{:x}] = 0x{:x} (region 0x{:x})",
                        off,
                        v,
                        v >> 40
                    ))
                } else {
                    None
                }
            })
            .collect();
        return Err(format!(
            "no pointer candidates at spawn_addr=0x{:x} (region=0x{:x}); non-zero 8B slots:\n{}",
            spawn_addr,
            spawn_region,
            if found.is_empty() {
                "  (none)".to_string()
            } else {
                found.join("\n")
            }
        ));
    }

    // For each candidate next_off: follow the pointer into the next struct and
    // check if candidate prev_off there points back to spawn_addr.
    let mut diag: Vec<String> = Vec::new();
    for &next_off in &candidates {
        let next_ptr = read_u64_at(&buf, next_off);
        match mem.read_bytes(next_ptr, 0x80) {
            Err(e) => {
                diag.push(format!(
                    "  next_off=0x{:x} ptr=0x{:x} read_bytes FAILED: {}",
                    next_off, next_ptr, e
                ));
            }
            Ok(next_buf) => {
                // Search ALL 8-byte-aligned offsets in the target struct for the back-pointer,
                // not just pself's candidates — pself.prev may be 0 (head of list) and therefore
                // absent from candidates, which would miss the correct prev_off entirely.
                for prev_off in (0..0x80usize.min(next_buf.len().saturating_sub(8))).step_by(8) {
                    if prev_off == next_off {
                        continue;
                    }
                    if read_u64_at(&next_buf, prev_off) == spawn_addr {
                        // Enforce smaller offset = NextOffset (EQ layout: Next=0x8 < Prev=0x10).
                        let (n, p) = if next_off < prev_off {
                            (next_off, prev_off)
                        } else {
                            (prev_off, next_off)
                        };
                        return Ok((n, p));
                    }
                }
            }
        }
    }

    Err(format!(
        "invariant not satisfied for spawn_addr=0x{:x}; candidates: {:?}\ndetail:\n{}",
        spawn_addr,
        candidates
            .iter()
            .map(|&o| format!("0x{:x}=0x{:x}", o, read_u64_at(&buf, o)))
            .collect::<Vec<_>>(),
        diag.join("\n")
    ))
}

/// Return all 4-byte-aligned offsets in buf where the f32 value changed between
/// prev and curr by more than `threshold`, and the curr value is within [min, max].
fn find_changed_floats(prev: &[u8], curr: &[u8], min: f32, max: f32, threshold: f32) -> Vec<usize> {
    let len = prev.len().min(curr.len()).saturating_sub(4);
    let mut results = Vec::new();
    for off in (0..len).step_by(4) {
        let pv = read_f32_at(prev, off);
        let cv = read_f32_at(curr, off);
        if pv.is_finite() && cv.is_finite() && cv >= min && cv <= max && (cv - pv).abs() > threshold
        {
            results.push(off);
        }
    }
    results
}

/// Find offsets where buf[off] changed from `from_val` to `to_val`.
fn find_byte_flip(prev: &[u8], curr: &[u8], from_val: u8, to_val: u8) -> Vec<usize> {
    prev.iter()
        .zip(curr.iter())
        .enumerate()
        .filter(|(_, (p, c))| **p == from_val && **c == to_val)
        .map(|(i, _)| i)
        .collect()
}

/// From a sorted list of offsets, find the first run of `count` values that are
/// exactly `step` apart (e.g., 3 consecutive 4-byte-aligned floats).
fn find_consecutive_cluster(offsets: &[usize], count: usize, step: usize) -> Vec<usize> {
    if offsets.len() < count {
        return Vec::new();
    }
    for i in 0..=offsets.len().saturating_sub(count) {
        let window = &offsets[i..i + count];
        let consecutive = window.windows(2).all(|w| w[1] == w[0] + step);
        if consecutive {
            return window.to_vec();
        }
    }
    Vec::new()
}

/// Walk the spawn list and find the offset used to store the owner/master entity ID.
///
/// Strategy: don't search for a specific value. Instead, find offsets that are:
///   - 0 in the player's own struct (player is not owned by anyone)
///   - 0 in the vast majority of spawns (NPCs/PCs have no owner)
///   - non-zero in a small number of spawns (just the pet/merc)
///   - non-zero value looks like a valid entity ID (1..=0xFFFF)
///
/// This works even when OwnerIDOffset stores an entity ID rather than a spawn ID,
/// so the player's spawn_id cannot be used as a search value.
fn find_owner_offset(
    mem: &MemReader,
    spawn_header_canonical: u64,
    _player_spawn_id: u32, // used only at call site for logging
    next_off: usize,
    prev_off: usize,
    player_buf: Option<&[u8]>,
) -> Option<(usize, String)> {
    let pb = player_buf?;

    let header_ptr = mem.read_raw_pointer(spawn_header_canonical).ok()?;
    if header_ptr == 0 {
        return None;
    }

    // Walk backward to head.
    let mut ptr = header_ptr;
    for _ in 0..2000 {
        let buf = match mem.read_bytes(ptr, STRUCT_SIZE) {
            Ok(b) => b,
            Err(_) => break,
        };
        let prev = read_u64_at(&buf, prev_off);
        if prev == 0 {
            break;
        }
        ptr = prev;
    }

    // Per-offset accumulators across all visited spawns.
    let slots = STRUCT_SIZE / 4;
    let mut zero_counts = vec![0u32; slots];
    let mut nonzero_counts = vec![0u32; slots];
    let mut sample_nonzero = vec![0u32; slots]; // one representative non-zero value
    let mut total_spawns = 0u32;

    let mut visited = 0u32;
    loop {
        if ptr == 0 || visited > 2000 {
            break;
        }
        visited += 1;
        total_spawns += 1;
        let buf = match mem.read_bytes(ptr, STRUCT_SIZE) {
            Ok(b) => b,
            Err(_) => break,
        };
        for off in (0..STRUCT_SIZE.saturating_sub(4)).step_by(4) {
            let val = read_u32_at(&buf, off);
            let slot = off / 4;
            if val == 0 {
                zero_counts[slot] += 1;
            } else {
                nonzero_counts[slot] += 1;
                sample_nonzero[slot] = val;
            }
        }
        let next = read_u64_at(&buf, next_off);
        if next == 0 || next == ptr {
            break;
        }
        ptr = next;
    }

    if total_spawns == 0 {
        return None;
    }

    // Allow up to ~5 % of spawns to be non-zero (handles zones with multiple pets).
    let nonzero_threshold = (total_spawns / 20).max(2);

    // Build candidates: offsets where player==0, few non-zero spawns, value looks like an ID.
    let mut candidates: Vec<(usize, u32, u32)> = (0..slots)
        .filter_map(|slot| {
            let off = slot * 4;
            // Skip the pointer/header region at the start of the struct.
            if off < 0x80 {
                return None;
            }
            let nz = nonzero_counts[slot];
            if nz == 0 || nz > nonzero_threshold {
                return None;
            }
            // Player's own struct must be 0 here.
            if pb.get(off..off + 4).map(|b| read_u32_at(b, 0)).unwrap_or(1) != 0 {
                return None;
            }
            // Non-zero value must look like a valid entity ID, not a pointer or float.
            let sample = sample_nonzero[slot];
            if sample == 0 || sample > 0xFFFF {
                return None;
            }
            Some((off, zero_counts[slot], nz))
        })
        .collect();

    if candidates.is_empty() {
        return None;
    }

    // Rank: highest zero_count first (most spawns have 0 = most "owner-like").
    // Break ties by lowest nonzero_count, then lowest offset.
    candidates.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)).then(a.0.cmp(&b.0)));

    let diag = candidates
        .iter()
        .take(5)
        .map(|(off, z, nz)| format!("0x{:x} (zeros={}/{}, owned={})", off, z, total_spawns, nz))
        .collect::<Vec<_>>()
        .join(", ");

    Some((candidates[0].0, diag))
}

/// Read the ground item struct and identify field offsets.
fn discover_item_offsets(mem: &MemReader, ground_addr_canonical: u64) -> Option<WizardResults> {
    // Resolve item struct pointer (mirrors server_logic ground item logic)
    let base_ptr = mem.read_raw_pointer(ground_addr_canonical).ok()?;
    if base_ptr == 0 {
        return None;
    }

    let name_check = mem.read_string(base_ptr + 0x38, 4).unwrap_or_default();
    let item_ptr = if name_check.starts_with("IT") {
        base_ptr
    } else {
        mem.read_pointer(base_ptr).ok().filter(|&p| p != 0)?
    };

    let buf = mem.read_bytes(item_ptr, ITEM_SIZE).ok()?;

    // Name: scan for "IT"-prefixed ASCII string
    let name_off = find_item_name_offset(&buf)?;

    // Pointers: first two 8-byte pointer-like values (Prev, Next)
    let item_region = item_ptr >> 40;
    let ptr_offsets: Vec<usize> = (0..0x30usize.min(buf.len().saturating_sub(8)))
        .step_by(8)
        .filter(|&off| {
            let v = read_u64_at(&buf, off);
            v == 0 || (v >> 40) == item_region // allow null for Prev (head of list)
        })
        .collect();
    // Take first two (Prev is often 0 at head, Next may be non-null)
    let item_prev = ptr_offsets.first().copied();
    let item_next = ptr_offsets.get(1).copied();

    // ID / DropID: small u32 values after the pointer block (typically 0x10, 0x18)
    let item_id = Some(0x10usize); // stable across builds in practice
    let item_drop_id = Some(0x18usize);

    // X/Y/Z: derive from name offset using the fixed intra-struct relationship.
    // EQ's GroundItem struct has X/Y/Z at name_off + 0x54/0x58/0x5c (e.g. 0x38
    // + 0x54 = 0x8c). Float-matching heuristics fail because adjacent fields
    // can coincidentally score better than the true cluster.
    let item_x = Some(name_off + 0x54);
    let item_y = Some(name_off + 0x58);
    let item_z = Some(name_off + 0x5c);

    Some(WizardResults {
        item_name: Some(name_off),
        item_prev,
        item_next,
        item_id,
        item_drop_id,
        item_x,
        item_y,
        item_z,
        ..Default::default()
    })
}

fn find_item_name_offset(buf: &[u8]) -> Option<usize> {
    for i in 0..buf.len().saturating_sub(4) {
        if buf[i] == b'I' && buf[i + 1] == b'T' && buf[i + 2].is_ascii_alphanumeric() {
            // Verify it's a null-terminated string of reasonable length
            let end = buf[i..].iter().position(|&b| b == 0).unwrap_or(0);
            if (4..=64).contains(&end) {
                return Some(i);
            }
        }
    }
    None
}

// ── Verify phase helper ───────────────────────────────────────────────────────

/// Read live memory using the just-discovered offsets and return a snapshot for
/// the Verify phase UI. Never panics — missing or unreadable fields are left at
/// their Default values so the GUI can show partial results gracefully.
fn read_verify_readings(
    mem: &MemReader,
    shared: &Arc<Mutex<WizardShared>>,
    results: &WizardResults,
) -> VerifyReadings {
    let (
        char_info_addr,
        spawn_header_addr,
        char_name,
        zone_canonical,
        level_off,
        next_off,
        prev_off,
    ) = {
        let s = shared.lock().unwrap();
        let level_off = s
            .scan_secondary
            .iter()
            .find(|(k, _)| k == "LevelOffset")
            .map(|(_, v)| *v as usize)
            .unwrap_or(0);
        let next_off = s
            .scan_secondary
            .iter()
            .find(|(k, _)| k == "NextOffset")
            .map(|(_, v)| *v as usize)
            .unwrap_or(0x8);
        let prev_off = s
            .scan_secondary
            .iter()
            .find(|(k, _)| k == "PrevOffset")
            .map(|(_, v)| *v as usize)
            .unwrap_or(0x10);
        (
            s.char_info_addr,
            s.spawn_header_addr,
            s.char_name.clone(),
            s.scan_primary.zone_name,
            level_off,
            next_off,
            prev_off,
        )
    };

    let mut r = VerifyReadings::default();

    // pSelf
    let pself = match mem.read_raw_pointer(char_info_addr) {
        Ok(p) if p != 0 => p,
        _ => return r,
    };

    let buf = match mem.read_bytes(pself, STRUCT_SIZE) {
        Ok(b) => b,
        Err(_) => return r,
    };

    // Name
    if let Some(name_off) = results.name {
        r.name = mem
            .read_string(pself + name_off as u64, 64)
            .unwrap_or_default();
        r.name_ok = if !char_name.is_empty() {
            Some(r.name == char_name)
        } else {
            None
        };
    }

    // Zone — canonical address remapped to actual points directly to the string
    if zone_canonical != 0 {
        r.zone = mem
            .read_string(mem.canonical_to_actual(zone_canonical), 64)
            .unwrap_or_default();
    }

    // Position
    if let (Some(xo), Some(yo), Some(zo)) = (results.x, results.y, results.z)
        && xo + 4 <= buf.len()
        && yo + 4 <= buf.len()
        && zo + 4 <= buf.len()
    {
        r.x = read_f32_at(&buf, xo);
        r.y = read_f32_at(&buf, yo);
        r.z = read_f32_at(&buf, zo);
        r.pos_ok = r.x.is_finite()
            && r.y.is_finite()
            && r.z.is_finite()
            && r.x.abs() <= 15_000.0
            && r.y.abs() <= 15_000.0
            && r.z.abs() <= 15_000.0;
    }

    // Heading
    if let Some(ho) = results.heading
        && ho + 4 <= buf.len()
    {
        r.heading = read_f32_at(&buf, ho);
        r.heading_ok = (0.0..=512.0).contains(&r.heading);
    }

    // Level (byte field; offset comes from secondary scan, not wizard results)
    if level_off > 0 && level_off < buf.len() {
        r.level = buf[level_off];
        r.level_ok = (1..=120).contains(&r.level);
    }

    // Spawn count — walk backward to head, then count forward; cap each at 2000.
    // read_raw_pointer returns the current/last-inserted node, not the head.
    if spawn_header_addr != 0
        && let Ok(header_ptr) = mem.read_raw_pointer(spawn_header_addr)
        && header_ptr != 0
    {
        let node_buf_size = next_off.max(prev_off) + 8;

        // Seek head (where prev == 0)
        let mut ptr = header_ptr;
        for _ in 0..2000 {
            let Ok(node) = mem.read_bytes(ptr, node_buf_size) else {
                break;
            };
            let prev = read_u64_at(&node, prev_off);
            if prev == 0 || prev == ptr {
                break;
            }
            ptr = prev;
        }

        // Count forward from head
        let mut count = 0usize;
        for _ in 0..2000 {
            if ptr == 0 {
                break;
            }
            count += 1;
            let Ok(node) = mem.read_bytes(ptr, node_buf_size) else {
                break;
            };
            let next = read_u64_at(&node, next_off);
            if next == 0 || next == ptr {
                break;
            }
            ptr = next;
        }
        r.spawn_count = count;
        r.spawn_count_ok = count >= 1;
    }

    r
}
