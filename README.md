# WinShowEQ

Rust rewrite of the [MySEQ](https://sourceforge.net/projects/seq/) EverQuest map overlay tool.
This repo contains both the **server** (reads EQ memory and streams data over TCP) and the
future **client** (planned Rust overlay) in a single workspace.

## Status

| Component | Milestone                                     | Status                 |
|-----------|-----------------------------------------------|------------------------|
| Server    | M1 — Data types, INI reader, notifier trait   | Complete               |
| Server    | M2 — EQ file scanner (pattern matching)       | Complete               |
| Server    | M3 — Memory reader (Win32 unsafe)             | Complete               |
| Server    | M4 — Network server + binary protocol         | Complete               |
| Server    | M5 — Server logic + console mode (end-to-end) | Complete               |
| Server    | M6 — Debug loop, full CLI (`clap`)            | Complete               |
| Server    | M7 — GUI (`egui` + `eframe`)                  | Partial (M7-7 pending) |
| Client    | Prereq — Cargo workspace restructure          | Complete               |
| Client    | C1 — Common crate + client foundation         | Complete               |
| Client    | C2 — Network client (TCP tick loop, decode)   | Complete               |
| Client    | C3 — Map file parser (native EQ format)       | Complete               |
| Client    | C4 — Core rendering (egui map canvas)         | Complete               |
| Client    | C5 — Spawn categories, filters, Z-filter      | Complete               |
| Client    | C6 — Panels, timers, persistence              | Complete               |
| Client    | C7 — Alerts and integrations                  | Complete               |
| Client    | C8 — Polish and parity                        | In Progress            |

### Project tracking

Complete migration plans and issue tracking are available on GitHub:

- [**Server Migration Milestone**](https://github.com/Bishop-Software/WinShowEQ/milestone/1): M1–M6 complete, M7 in progress (issues #9–#15)
- [**Client Migration Milestone**](https://github.com/Bishop-Software/WinShowEQ/milestone/2): C1–C7 complete, C8 in progress (issues #1–#8 and #23; #2 and #3 closed)

See [CLAUDE.md](CLAUDE.md) for developer guidance and technical details.

## What it does

MySEQ is a map overlay tool for EverQuest. The server component today:
- Attaches to a running `eqgame.exe` via `ReadProcessMemory`
- Reads spawn positions, zone info, ground items, and player state from EQ memory
- Streams packed binary records over TCP (default port 5555) to a connected client
- Supports GUI mode (default), console mode, and debug mode

The Rust client component (93/93 tests passing):
- Connects to the server over TCP and decodes all packet types
- Renders EQ zone maps (native `.txt` format) with spawn dots, ground items, mob trails, and annotations
- Loads all four map file layers: `{zone}.txt` (base) + `{zone}_1.txt`, `{zone}_2.txt`, `{zone}_3.txt` (numbered layers)
- Maintains spawn list, timer list, and ground item list panels in an `egui_dock` docking layout
- Supports filter categories (hunt/caution/danger/rare), Z-filter for vertical spawn filtering, alerts (TTS/speech/sound/Discord)
- Right-click context menu on spawns: add timer, add to filter, add map text
- Persists timers, annotations, filters, and config across sessions
- Map rendering: applies coordinate transforms to align spawns with map lines (north up, east right)
- Color-coded spawn dots by con level; mob trails with faded orange dots when enabled
- Named spawn color overrides via `cfg/spawn_colors.json` (maps spawn name → color key from `cfg/colors.json`)
- Shift+click on map draws a bearing/distance line from player to clicked point (distance in EQ units, degrees, cardinal direction); ESC or plain click clears it
- Ctrl+F (or Edit > Find Spawn) opens a search dialog: case-insensitive partial name match, results table (Name/Lvl/Class/X/Y/Z), click a result to jump the map to that spawn; all matches highlighted with a white ring on the map and a cyan accent bar in the spawn list; ESC or close clears highlights
- Left-click a row in the spawn list to select it: gold ring on the map dot and gold accent bar in the list; click the same row again to deselect
- Double-click a spawn row to center the map on that spawn
- Target indicator: orange right-edge bar in spawn list and orange ring on map for the current EQ target
- Keyboard shortcuts:

| Key           | Action                     |
|---------------|----------------------------|
| `+` / `=`     | Zoom in                    |
| `-`           | Zoom out                   |
| Scroll wheel  | Zoom in/out (map focused)  |
| `Home`        | Center map on player       |
| `F5`          | Toggle Spawns panel        |
| `F6`          | Toggle Timers panel        |
| `F7`          | Toggle Ground Items panel  |
| `T`           | Toggle mob trails          |
| `Ctrl+F`      | Find Spawn                 |
| `Shift+click` | Draw bearing/distance line |
| `ESC`         | Clear bearing line         |

- View menu: panel visibility toggles (Spawns/Timers/Ground Items) with checkbox indicators
- Map menu: Center on Player, Zoom In/Out, Mob Trails toggle
- Help menu with About dialog (version, credits, clickable library links)

## Workspace layout

```
WinShowEQ/
  Cargo.toml          # workspace root — members: common, server, client
  common/             # shared wire protocol types
    src/
      protocol.rs     # SpawnRecord (100-byte packed), IPT_*/OPT_* constants, SpawnType
      world.rs        # WorldTime
  server/             # server binary (winshoweq-server / WinShowEQServer.exe)
    src/
      main.rs
      config.rs       # IniReader (myseqserver.ini + config.ini)
      scanner.rs      # EQ executable pattern scanner
      mem_reader.rs   # ReadProcessMemory wrapper (all unsafe contained here)
      network.rs      # TCP server, DataProvider trait, wire protocol
      server_logic.rs # MemDataProvider (live memory reads), ServerLogic
      session.rs      # SessionRunner state machine, run_console_loop
      notifier.rs     # UiNotifier trait, LoggingNotifier
      gui/
        mod.rs        # GuiState, EguiNotifier (UiNotifier → Arc<Mutex<GuiState>>)
        app.rs        # WinShowEQApp: eframe::App — main window layout
      data/
        spawn.rs      # re-exports SpawnRecord from common
        item.rs       # GroundItem
        world.rs      # re-exports WorldTime from common
        spawn_offsets.rs  # SpawnOffsets, ItemOffsets, WorldOffsets (from INI)
  client/             # client binary (winshoweq-client / WinShowEQClient.exe)
    src/
      main.rs
      config.rs       # ClientConfig — client.ini load/save
      net.rs          # ServerConnection — TCP tick loop
      protocol.rs     # decode_packet — OPT_* dispatch
      map_reader.rs   # native EQ .map file parser (L/P lines, base + 3 layers)
      map_canvas.rs   # MapCon — egui map canvas rendering
      filters.rs      # FilterSet — hunt/caution/danger/rare XML filters
      alerts.rs       # AlertEngine — TTS, sound, Discord webhook
      logger.rs       # Logger — dated log files
      game_data.rs    # GameData — race name lookup (dbstr_us.txt) + class name lookup (cfg/classes.json)
      data/
        mod.rs        # AppData aggregate, apply_packet
        spawns.rs     # SpawnInfo, SpawnStore, SpawnCategory, con colors
        ground.rs     # GroundItem, GroundStore (Vec; replaced per tick)
        timers.rs     # SpawnTimer, TimerStore — respawn tracking + persistence
        world.rs      # InGameTime
        annotations.rs # AnnotationStore — per-zone map notes
      ui/
        main_window.rs # MainApp: eframe::App — egui_dock docking layout
        map_pane.rs   # MapPane — Z-filter + map canvas host
        spawn_list.rs # sortable spawn table
        timer_list.rs # timer countdown table
        ground_list.rs # ground item table (name, X, Y, Z)
        options.rs    # settings dialog with folder browse buttons (rfd)
        login.rs      # connect dialog
        search_dialog.rs # Ctrl+F spawn search with live highlighting
```

## Building and running

```powershell
# Build everything
cargo build

# Run the server (default: GUI mode)
cargo run -p winshoweq-server

# Run the server with an explicit mode
cargo run -p winshoweq-server -- console
cargo run -p winshoweq-server -- debug

# Use an alternate INI file
cargo run -p winshoweq-server -- -f path\to\myseqserver.ini

# Dev/diagnostic subcommands (hidden from --help)
cargo run -p winshoweq-server -- scan path\to\eqgame.exe   # scan for memory offsets
cargo run -p winshoweq-server -- attach                     # print PID + base address
cargo run -p winshoweq-server -- serve-stub                 # stub server (no EQ required)

# Run the client (default: GUI mode)
cargo run -p winshoweq-client

# Run the client with a specific server address
cargo run -p winshoweq-client -- --connect 127.0.0.1:5555

# Run tests
cargo test

# Build release binaries
cargo build --release -p winshoweq-server
cargo build --release -p winshoweq-client
```

Automated/manual server test docs live in `server/scripts/README.md`.

## Configuration

### Server

By default, the server resolves INI files in this order:

1. workspace-local `server\<name>` when running from a checkout (for example via `cargo run`
   or `target\debug\WinShowEQServer.exe`)
2. `%ProgramData%\WinShowEQ\<name>` when that directory exists (installer layout)
3. current working directory fallback for ad-hoc local runs

Three INI files are used in that resolved config location:

**`myseqserver.ini`** — runtime offsets and port. `[File Info]` fields are auto-populated
by the Offset Finder scan or the `scan` subcommand — manual edits are not required:
```ini
[File Info]
PatchDate=MM/DD/YYYY          ; derived from PE TimeDateStamp (compile date)
ClientHash=<sha1>             ; SHA1 of eqgame.exe — uniquely identifies the client build
BuildString=Release Client #N HH:MM:SS Mon DD YYYY  ; from binary scan + PE timestamp (Pacific time)

[Port]
Port=5555

[Memory Offsets]
SpawnHeaderAddr=0x...
CharInfo=0x...
TargetAddr=0x...
ZoneAddr=0x...
ItemsAddr=0x...
WorldAddr=0x...

[SpawnInfo Offsets]
NameOffset=...
; ... (field-level byte offsets within EverQuest's spawn struct)

[GroundItem Offsets]
; ...

[WorldInfo Offsets]
; ...
```

**`config.ini`** — startup preferences and persisted UI state:
```ini
[Server]
StartMinimized=0

[OffsetFinder]
EQGamePath=C:\path\to\eqgame.exe
```

**`patterns.ini`** — EQ memory scanner byte patterns:
```ini
[ZoneAddr]
Start=0x...
Pattern=\x41\xB8...
Mask=xxxxxxxxx...

[SpawnHeaderAddr]
Start=0x...
Pattern=\x40\x53...
Mask=xxxxxxx...

; ... (one section per scanned address)
```

### Client

**`client.ini`** — server connection, UI preferences, and alert settings:

```ini
[WinShowEQ]
Host=127.0.0.1
Port=5555
UpdateDelayMs=100

[Directories]
ConfigDir=...      ; filters, timers, annotations (OS-specific %AppData%)
MapDir=...         ; zone .txt files
LogDir=...         ; dated log files

[Alerts]
DangerMode=speech  ; none/beep/speech/sound
DangerSound=...    ; path to .wav/.mp3/etc
; ... (caution, hunt, rare modes and sounds)

[Discord]
WebhookUrl=...     ; Discord webhook URL
OnDanger=1         ; post to Discord on danger spawns
OnHunt=1           ; post to Discord on hunt spawns
```

The Options dialog provides a GUI to edit all settings; changes persist automatically.

## Wire protocol

The server speaks a simple binary protocol compatible with the original C# MySEQ client:

- **Client → server:** 4-byte LE `i32` request bitmask (`IPT_*` flags)
- **Server → client:** 4-byte LE `i32` count, then `count × 100` bytes of `SpawnRecord`

`SpawnRecord` is a 100-byte `#[repr(C, packed)]` struct (`netBuffer_t` in the C++ source).
The `flags` field carries the `OPT_*` packet type (zone, self, spawn, target, ground, world).

See `common/src/protocol.rs` and `server/src/network.rs` for the source-of-truth constants and
encoding behavior.

## Installer

A Windows installer is available via [Inno Setup 6](https://jrsoftware.org/isinfo.php).
It installs `WinShowEQServer.exe` to `%ProgramFiles%\WinShowEQ\bin` and seeds the default
INI files into `%ProgramData%\WinShowEQ` (preserved across upgrades and uninstalls).

```powershell
# Build release and package installer
pwsh -File .\installer\build-installer.ps1

# Stage files only (skip Inno Setup compilation)
pwsh -File .\installer\build-installer.ps1 -StageOnly
```

Output lands in `target/installer-output/`. See [`installer/README.md`](installer/README.md)
for full options including `-SkipBuild`, `-IncludeClient`, and `-InnoSetupCompilerPath`.

## License

GPL-3.0 — see [LICENSE](LICENSE).