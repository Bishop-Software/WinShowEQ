# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

See @README.md for a high-level overview of the project, its goals, and current status.

## Repository structure
```text
WinShowEQ/
├── Cargo.lock                    # Workspace dependency lockfile
├── Cargo.toml                    # Workspace manifest
├── CLAUDE.md                     # Project-specific agent/developer guidance
├── LICENSE                       # License text
├── README.md                     # Top-level project documentation
├── client/                       # Client crate (overlay app; C1–C8 complete)
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── config.rs             # ClientConfig — client.ini load/save
│       ├── net.rs                # ServerConnection — TCP tick loop
│       ├── protocol.rs           # decode_packet — OPT_* dispatch
│       ├── map_reader.rs         # Native EQ .map file parser (L/P lines, base + 3 layers)
│       ├── map_canvas.rs         # MapCon — egui map canvas rendering
│       ├── filters.rs            # FilterSet — hunt/caution/danger/rare XML filters
│       ├── alerts.rs             # AlertEngine — TTS, sound, Discord webhook
│       ├── logger.rs             # Logger — dated log files
│       ├── game_data.rs          # GameData — race name lookup (dbstr_us.txt) + class name lookup (cfg/classes.json)
│       ├── data/
│       │   ├── mod.rs            # AppData aggregate, apply_packet
│       │   ├── spawns.rs         # SpawnInfo, SpawnStore, SpawnCategory, con colors
│       │   ├── ground.rs         # GroundItem, GroundStore (Vec; replaced per tick)
│       │   ├── timers.rs         # SpawnTimer, TimerStore, SpawnObserver, SpawnObservation — respawn tracking, auto-learning, persistence
│       │   ├── world.rs          # InGameTime
│       │   └── annotations.rs   # AnnotationStore — per-zone map notes
│       └── ui/
│           ├── main_window.rs   # MainApp: eframe::App — egui_dock docking layout
│           ├── map_pane.rs      # MapPane — Z-filter + map canvas host
│           ├── spawn_list.rs    # Sortable spawn table
│           ├── timer_list.rs    # Timer countdown table
│           ├── ground_list.rs   # Ground item table (name, X, Y, Z)
│           ├── options.rs       # Settings dialog with folder browse buttons (rfd)
│           ├── login.rs         # Connect dialog
│           ├── search_dialog.rs # Ctrl+F spawn search with live highlighting
│           ├── spawn_filter.rs  # [planned] cascading race/class/level filter for spawn list
│           ├── about.rs         # About dialog (version, credits, icon attribution)
│           ├── help.rs          # Help window (keyboard shortcuts, usage guide)
│           └── mod.rs
├── common/                       # Shared protocol/types crate
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── protocol.rs           # Wire protocol structs/constants
│       └── world.rs              # Shared world-time/data types
├── installer/                    # Windows installer assets/scripts (non-crate)
│   ├── build-installer.ps1
│   ├── README.md
│   └── WinShowEQ.iss
└── server/                       # Server crate (EQ memory reader + TCP server)
    ├── Cargo.toml
    ├── config.ini                # Startup preferences (StartMinimized, EQGamePath)
    ├── myseqserver.ini           # Runtime offsets + network config
    ├── patterns.ini              # EQ memory scanner byte patterns (separated from config.ini)
    ├── scripts/                  # PowerShell test automation
    │   ├── eq-test-context.sample.json
    │   ├── README.md
    │   ├── run-no-eq-tests.ps1
    │   ├── run-tests.ps1
    │   ├── run-with-eq-tests.ps1
    │   └── lib/
    │       └── test-common.ps1
    └── src/
        ├── config.rs
        ├── debug.rs
        ├── main.rs
        ├── mem_reader.rs
        ├── network.rs
        ├── notifier.rs
        ├── scanner.rs
        ├── server_logic.rs
        ├── session.rs
        ├── data/
        │   ├── item.rs
        │   ├── mod.rs
        │   ├── spawn_offsets.rs
        │   ├── spawn.rs
        │   └── world.rs
        └── gui/
            ├── app.rs
            └── mod.rs
```

## Migration status

### Server — [WinShowEQ Rust Server Migration](https://github.com/Bishop-Software/WinShowEQ/milestone/1)

Milestones M1–M7 complete (closed issues #9–#15).

### Client — [WinShowEQ Rust Client Migration](https://github.com/Bishop-Software/WinShowEQ/milestone/2)

Milestones C1–C8 complete (closed issues #1–#8, #16–#23; 119/119 tests passing).

### Enhancements — [WinShowEQ Enhancements](https://github.com/Bishop-Software/WinShowEQ/milestone/3)

Open issues #36, #43–#46. Closed: #37–#42, #47–#50.

**Auto-learning spawn timers (#37–#42) — Complete:**
- Each network tick, `AppData::on_tick_end()` diffs `curr_tick_npc_ids` against `SpawnObserver.prev_tick_ids`
- Disappeared NPC IDs → pending kill recorded at `"y.yyy,x.xxx"` location key
- New NPC at pending kill location → interval calculated; `SpawnObservation` updated
- Auto-promotes to `TimerStore` once `spawn_count > 1` and interval ≥ 10 sec
- Exclusions: non-NPC spawn categories, `owner_id != 0` (pets/mercs), `_`-prefix names, races 141/376/533
- Void zones excluded from tracking, save, and load: `bazaar`, `clz`, `default`, `nexus`, `poknowledge`, any `guild*`
- `SpawnObserver::save/load` persists raw observations to `obs-{zone}.txt` in the timer directory (zone enter/exit and app close)
- Timer list: Count column, `[A]` indicator for auto-learned timers, Clear All (resets observer + deletes both `spawns-{zone}.txt` and `obs-{zone}.txt`)
- `timers_dirty` flag on `AppData`; auto-save fires every 60 seconds when set

**Map overlay visibility and label toggles (#47–#49) — Complete:**
- `MapOverlaySettings` struct in `config.rs` with 7 bool fields: `show_npcs`, `show_players`, `show_corpses`, `show_pets`, `show_npc_names`, `show_npc_levels`, `show_player_names`
- Persisted under `[MapOverlay]` in `client.ini`; threaded as `&MapOverlaySettings` from `MainApp` → `WinSeqTabViewer` → `MapPane` → `MapCon` each frame
- `draw_spawns` filters by category before rendering; draws name/level labels (10px proportional, clipped to canvas) when toggles are on
- Corpse rendering: PC corpse (name has no `_`, doesn't start with `"a "` / `"an "`) → hollow yellow square; NPC corpse → cyan crosshair (matches C# MySEQ `DrawRectangle`/`DrawLine` behavior)
- Map menu "Show ▶" submenu with checkbox items for all 7 toggles; each click calls `save_config()`

**UI state persistence (#50) — Complete:**
- `persist_window: true` in `NativeOptions` — eframe saves/restores window geometry automatically
- `Tab` enum derives `Serialize`/`Deserialize`; `DockState<Tab>` written to eframe storage key `dock_state` in `App::save()` and restored from `cc.storage` in `new()`
- Spawn/timer/ground column widths written to eframe storage keys `spawn_col_widths`, `timer_col_widths`, `ground_col_widths`; restored into `AppData` on startup (length-checked against defaults before applying)
- Storage file: `%AppData%\WinShowEQ Client\app.ron`

**Spawn list filter UI (#43–#46) — Planned:**
- `SpawnFilterUI` in `ui/spawn_filter.rs` — cascading ComboBox filter: race → class → level range → spawn type
- Options rebuilt from live spawn data each tick (only when spawn list changes)
- Filter bar rendered above spawn list column headers; returns `HashSet<u32>` of visible IDs
- Active filter also hides matching spawns on the map canvas


## C++ source reference

| C++ file                                   | Rust equivalent (planned)                                                          |
|--------------------------------------------|------------------------------------------------------------------------------------|
| `MemReader.cpp`                            | `server/src/mem_reader.rs`                                                         |
| `EQGameScanner.cpp` + `EQGamePatterns.h`   | `server/src/scanner.rs`                                                            |
| `IniReader.cpp`                            | `server/src/config.rs`                                                             |
| `NetworkServer.cpp`                        | `server/src/network.rs`                                                            |
| `ServerLogic.cpp`                          | `server/src/server_logic.rs`                                                       |
| `ServerSessionRunner.cpp`                  | `server/src/session.rs`                                                            |
| `IServerUiNotifier.h`                      | `server/src/notifier.rs` (trait)                                                   |
| `Spawn.h` / `Item.h` / `World.h`           | `common/src/protocol.rs`, `server/src/data/`                                       |
| `MySEQ.server.cpp` (service/console/debug) | `server/src/debug.rs`, `server/src/main.rs` (service mode not implemented)         |
| `Win32ServerUiNotifier.h`                  | `server/src/gui/mod.rs` (`EguiNotifier`), `server/src/gui/app.rs` (`WinShowEQApp`) |

## Key facts

- Wire protocol: 100-byte packed spawn record (`netBuffer_t` in C++ `Spawn.h`).
  `size_of::<SpawnRecord>() == 100` asserted in `common` tests.
- EQ module base is hardcoded to `0x140000000` (known issue — matches C++ behavior).
- TCP port default: 5555 (configurable in `myseqserver.ini`).
- INI files: `myseqserver.ini` (runtime offsets, port), `config.ini` (startup prefs such as
  `StartMinimized` and persisted `EQGamePath`), and `patterns.ini` (EQ memory scanner byte
  patterns — separated from `config.ini`). Path resolution order: (1) workspace-local
  `server\<name>` during checkout runs, (2) `%ProgramData%\WinShowEQ\<name>` when that
  directory exists (installer layout), (3) current working directory fallback. Override with
  `-f <file>` (myseqserver.ini only).
  Note: step (1) is planned; the current code implements steps (2) and (3).
- `myseqserver.ini` `[File Info]` is auto-populated by `EqGameScanner::scan_executable`:
  `PatchDate` from PE `TimeDateStamp`; `ClientHash` as SHA1 of the exe file; `BuildString`
  from a binary scan of PE sections combined with the timestamp converted to Pacific time
  (PST/PDT determined from US DST rules). `config.ini` and `patterns.ini` are never
  auto-created — `patterns.ini` must be seeded; `config.ini` is created on first write.
- No `tokio` — use `std::net` blocking sockets to match the C++ threading model.
- All `unsafe` Win32 calls must be contained within `mem_reader.rs` and `service.rs` only.

## Client coordinate system (spawns, map rendering)

**Wire protocol (spawn.X, spawn.Y from server):**
- `SpawnRecord` struct fields are named opposite to EQ convention: `rec.x` (offset 34) = EQ east-west,
  `rec.y` (offset 30) = EQ north-south. After `SpawnInfo::from_record` swap: `SpawnInfo.x = rec.y`,
  `SpawnInfo.y = rec.x`.
- Wire values: spawn.X = -file_x, spawn.Y = -file_y (opposite sign from native EQ map file coords).

**Map file parsing (`map_reader.rs`):**
- Native EQ format: `L x1,y1,z1,x2,y2,z2,r,g,b` (line) and `P x,y,z,...` (point)
- Negates Y only: `MapLine.x = file_x`, `MapLine.y = -file_y`
- Loads base layer `{zone}.txt` first, then layers `{zone}_1.txt`, `{zone}_2.txt`, `{zone}_3.txt`
- Black lines/labels (0,0,0) are rendered as (100,100,100) dark gray (visible on black background)

**Map canvas rendering (`map_canvas.rs`, `eq_to_map`):**
- `eq_to_map`: negates X only: `mx = -SpawnInfo.x`, `my = SpawnInfo.y`
- `to_screen`: `screen_x = center_x + (mx - focus_x) * zoom`, `screen_y = center_y - (my - focus_y) * zoom`
- Result: spawns align with map lines; north = up, east = right

**Player heading arrow:**
- EQ heading: 0 = north, increases counter-clockwise (128 = west, 256 = south, 384 = east)
- Formula: `tip = (pos.x - sin(heading_rad) * len, pos.y - cos(heading_rad) * len)`
- Matches C# MySEQ xSin/xCos lookup table convention

## Network protocol summary

Client sends a 4-byte request bitmask (`inc_packet_types`); server responds with packed
struct arrays. Request flags: `IPT_zone=0x01`, `IPT_self=0x02`, `IPT_target=0x04`,
`IPT_spawns=0x08`, `IPT_ground=0x10`, `IPT_getproc=0x20`, `IPT_setproc=0x40`,
`IPT_world=0x80`. Response type byte precedes each payload.

## Runtime modes (CLI flags)

| Flag        | Mode                                                                    |
|-------------|-------------------------------------------------------------------------|
| *(none)*    | Interactive GUI (`eframe` window; `SessionRunner` on background thread) |
| `console`   | Headless console                                                        |
| `debug`     | Console + interactive debug command loop                                |
| `-f <file>` | Use alternate INI file path                                             |

Note: Windows Service mode (`-i`/`-d`/`-k`) is intentionally not implemented. MySEQ
is always launched manually by a logged-in user; SCM integration adds no value here.


## Commands

```bash
# Build all crates
cargo build

# Build release
cargo build --release

# Build a specific crate
cargo build -p winshoweq-server
cargo build -p winshoweq-client

# Run the server
cargo run -p winshoweq-server

# Run the client
cargo run -p winshoweq-client

# Test all crates
cargo test

# Test a specific crate
cargo test -p common
cargo test -p winshoweq-server

# Run a single test
cargo test -p <crate> <test_name>

# Format
cargo fmt

# Lint
cargo clippy
```
## Before spawning any subagent
Only spawn subagents for complex, multistep tasks.
For simple tasks, handle directly without calling route_task.
When you do need a subagent:
Call route_task(task, files, directory) first. Always.
- REUSE → call get_context(agent_id), check stale_files, re-read any that changed
- CREATE_NEW → check existing_agents in response before spawning
## For code search
Prefer cocoindex.search() over Grep for semantic/exploratory queries.
Use Grep only for exact string matches.
## Memory
claude-mem auto-captures observations. Use search() → get_observations()
for progressive retrieval (don't load everything).