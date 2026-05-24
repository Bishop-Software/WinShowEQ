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

    // [SpawnInfo Offsets] — wizard-discovered fields (only reliable methods):
    //   NameOffset/LastNameOffset  — exact match on user-provided character name
    //   XOffset/YOffset/ZOffset    — consecutive 4-byte float cluster during movement
    //   HeadingOffset              — float in [0,512] changing only during turning
    //   HideOffset                 — single byte flipping 0→1 after casting invisibility
    //   OwnerIDOffset              — u32 matching the player's known spawn ID
    //
    // Excluded (heuristics too error-prone):
    //   NameOffset/LastNameOffset — when name not confirmed by user (heuristic only)
    //   SpeedOffset               — "drops near 0 when stopped" matches multiple floats
    //   NextOffset/PrevOffset     — pointer scan picks wrong heap pointer too often
    if results.name_confirmed {
        write_spawn("NameOffset", results.name);
        write_spawn("LastNameOffset", results.last_name);
    }
    write_spawn("XOffset", results.x);
    write_spawn("YOffset", results.y);
    write_spawn("ZOffset", results.z);
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

    // Next / Prev pointers
    let ptrs = find_pointer_candidates(&buf, pself);
    match ptrs.len() {
        0 => log!("Next/Prev — not found"),
        1 => {
            if let Ok(mut s) = shared.lock() {
                s.results.next = Some(ptrs[0]);
            }
            log!("One pointer @ 0x{:x} — need two for Next/Prev", ptrs[0]);
        }
        _ => {
            if let Ok(mut s) = shared.lock() {
                s.results.next = Some(ptrs[0]);
                s.results.prev = Some(ptrs[1]);
            }
            log!("Next → 0x{:x}  Prev → 0x{:x}", ptrs[0], ptrs[1]);
        }
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

    if !stop_skipped {
        let mut speed_candidates: Vec<usize> = movement_union
            .iter()
            .filter(|&&off| {
                let v = read_f32_at(&stopped_buf, off);
                v.is_finite() && v.abs() < 0.1
            })
            .copied()
            .collect();
        speed_candidates.sort_by(|&a, &b| {
            read_f32_at(&stopped_buf, a)
                .abs()
                .partial_cmp(&read_f32_at(&stopped_buf, b).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let speed_offset = speed_candidates.first().copied();
        if let Some(off) = speed_offset {
            log!("Speed → 0x{:x}", off);
            if let Ok(mut s) = shared.lock() {
                s.results.speed = Some(off);
            }
            movement_union.retain(|&o| o != off);
        } else {
            log!("Speed — not found among movement candidates");
        }
    } else {
        log!("Stopped — skipped.");
    }
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
    // They should be consecutive 4-byte-aligned offsets.
    let pos = find_consecutive_cluster(&movement_union, 3, 4);
    let (x_off, y_off, z_off) = if pos.len() >= 3 {
        (pos[0], pos[1], pos[2])
    } else if movement_union.len() >= 3 {
        (movement_union[0], movement_union[1], movement_union[2])
    } else {
        log!(
            "Position — only {} candidates remain (need 3)",
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
                if player_spawn_id == 0 {
                    log!("Owner — cannot search: player SpawnID unknown");
                } else if spawn_header_addr == 0 {
                    log!("Owner — cannot search: SpawnHeaderAddr not found by scan");
                } else {
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
                        Some(off) => {
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

    // Current player position (for cross-referencing item coordinates)
    let player_pos: Option<(f32, f32, f32)> = {
        let s = shared.lock().ok();
        let xo = s.as_ref().and_then(|s| s.results.x).unwrap_or(0);
        let yo = s.as_ref().and_then(|s| s.results.y).unwrap_or(0);
        let zo = s.as_ref().and_then(|s| s.results.z).unwrap_or(0);
        if xo > 0 {
            get_pself(&mem, char_info_addr)
                .and_then(|ps| mem.read_bytes(ps, STRUCT_SIZE).ok())
                .map(|buf| {
                    (
                        read_f32_at(&buf, xo),
                        read_f32_at(&buf, yo),
                        read_f32_at(&buf, zo),
                    )
                })
        } else {
            None
        }
    };

    loop {
        check_cancel!();
        std::thread::sleep(Duration::from_millis(200));
        match take_command!() {
            WizardCommand::ActionDone => {
                if ground_addr == 0 {
                    log!("GroundItem — ItemsAddr not found by scan");
                } else {
                    match discover_item_offsets(&mem, ground_addr, player_pos) {
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
fn find_pointer_candidates(buf: &[u8], pself: u64) -> Vec<usize> {
    const POINTER_SCAN_RANGE: usize = 0x80;
    let self_region = pself >> 40;
    let mut results = Vec::new();
    for off in (0..POINTER_SCAN_RANGE.min(buf.len().saturating_sub(8))).step_by(8) {
        let val = read_u64_at(buf, off);
        if val != 0 && val != pself && (val >> 40) == self_region {
            results.push(off);
        }
    }
    results
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

/// Walk the spawn list and find the offset in a non-player spawn where a u32
/// equals `player_spawn_id`. Verifies the same offset is 0 in the player's own struct.
fn find_owner_offset(
    mem: &MemReader,
    spawn_header_canonical: u64,
    player_spawn_id: u32,
    next_off: usize,
    prev_off: usize,
    player_buf: Option<&[u8]>,
) -> Option<usize> {
    let header_ptr = mem.read_raw_pointer(spawn_header_canonical).ok()?;
    if header_ptr == 0 {
        return None;
    }

    // Walk backward to head; skip unreadable nodes rather than aborting.
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

    // Walk forward looking for player_spawn_id in a non-player spawn
    let mut visited = 0u32;
    loop {
        if ptr == 0 || visited > 2000 {
            break;
        }
        visited += 1;
        let buf = match mem.read_bytes(ptr, STRUCT_SIZE) {
            Ok(b) => b,
            Err(_) => {
                // Skip unreadable node; try to advance via next pointer if possible.
                break;
            }
        };

        // Scan for player_spawn_id as a u32 at any 4-byte-aligned offset
        for off in (0..STRUCT_SIZE.saturating_sub(4)).step_by(4) {
            if read_u32_at(&buf, off) == player_spawn_id {
                // Verify the same offset is 0 in the player's own struct (player's owner = 0).
                // If player_buf is unavailable, reject the match rather than accepting blindly.
                let player_is_zero = player_buf
                    .and_then(|pb| pb.get(off..off + 4))
                    .map(|b| read_u32_at(b, 0) == 0)
                    .unwrap_or(false);
                if player_is_zero {
                    return Some(off);
                }
            }
        }

        let next = read_u64_at(&buf, next_off);
        if next == 0 || next == ptr {
            break;
        }
        ptr = next;
    }
    None
}

/// Read the ground item struct and identify field offsets.
fn discover_item_offsets(
    mem: &MemReader,
    ground_addr_canonical: u64,
    player_pos: Option<(f32, f32, f32)>,
) -> Option<WizardResults> {
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

    // X/Y/Z: floats near player position
    let (item_x, item_y, item_z) = if let Some((px, py, pz)) = player_pos {
        find_item_position_offsets(&buf, px, py, pz)
    } else {
        (None, None, None)
    };

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

fn find_item_position_offsets(
    buf: &[u8],
    px: f32,
    py: f32,
    pz: f32,
) -> (Option<usize>, Option<usize>, Option<usize>) {
    // Items are dropped near the player; search for floats within 50 units
    let candidates: Vec<usize> = (0..buf.len().saturating_sub(4))
        .step_by(4)
        .filter(|&off| {
            let v = read_f32_at(buf, off);
            v.is_finite()
                && ((v - px).abs() < 50.0 || (v - py).abs() < 50.0 || (v - pz).abs() < 50.0)
        })
        .collect();

    // Look for 3 consecutive float offsets
    let cluster = find_consecutive_cluster(&candidates, 3, 4);
    if cluster.len() >= 3 {
        (Some(cluster[0]), Some(cluster[1]), Some(cluster[2]))
    } else if candidates.len() >= 3 {
        (
            Some(candidates[0]),
            Some(candidates[1]),
            Some(candidates[2]),
        )
    } else {
        (
            candidates.first().copied(),
            candidates.get(1).copied(),
            candidates.get(2).copied(),
        )
    }
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
    let (char_info_addr, spawn_header_addr, char_name, zone_canonical, level_off, next_off) = {
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
        (
            s.char_info_addr,
            s.spawn_header_addr,
            s.char_name.clone(),
            s.scan_primary.zone_name,
            level_off,
            next_off,
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
    if let (Some(xo), Some(yo), Some(zo)) = (results.x, results.y, results.z) {
        if xo + 4 <= buf.len() && yo + 4 <= buf.len() && zo + 4 <= buf.len() {
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
    }

    // Heading
    if let Some(ho) = results.heading {
        if ho + 4 <= buf.len() {
            r.heading = read_f32_at(&buf, ho);
            r.heading_ok = (0.0..=512.0).contains(&r.heading);
        }
    }

    // Level (byte field; offset comes from secondary scan, not wizard results)
    if level_off > 0 && level_off < buf.len() {
        r.level = buf[level_off];
        r.level_ok = (1..=120).contains(&r.level);
    }

    // Spawn count — walk forward from spawn header pointer, cap at 500
    if spawn_header_addr != 0 {
        if let Ok(header_ptr) = mem.read_raw_pointer(spawn_header_addr) {
            if header_ptr != 0 {
                let mut ptr = header_ptr;
                let mut count = 0usize;
                for _ in 0..500 {
                    if ptr == 0 {
                        break;
                    }
                    count += 1;
                    let Ok(node) = mem.read_bytes(ptr, next_off + 8) else {
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
        }
    }

    r
}
