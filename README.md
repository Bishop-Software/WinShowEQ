# WinShowEQ

Rust rewrite of the [MySEQ](https://sourceforge.net/projects/seq/) EverQuest map overlay tool.
This repo contains both the **server** (reads EQ memory and streams data over TCP) and the
future **client** (planned Rust overlay) in a single workspace.

## Status

| Component | Milestone                                     | Status      |
|-----------|-----------------------------------------------|-------------|
| Server    | M1 — Data types, INI reader, notifier trait   | Complete    |
| Server    | M2 — EQ file scanner (pattern matching)       | Complete    |
| Server    | M3 — Memory reader (Win32 unsafe)             | Complete    |
| Server    | M4 — Network server + binary protocol         | Complete    |
| Server    | M5 — Server logic + console mode (end-to-end) | Complete    |
| Server    | M6 — Debug loop, full CLI (`clap`)            | Complete    |
| Server    | M7 — GUI (`egui` + `eframe`)                  | Partial     |
| Client    | Prereq — Cargo workspace restructure          | Complete    |
| Client    | C1 — Common crate + client foundation         | Scaffold only |
| Client    | C2–C8 — Network, map, rendering, filters…     | Not started |

## What it does

MySEQ is a map overlay tool for EverQuest. The server component today:
- Attaches to a running `eqgame.exe` via `ReadProcessMemory`
- Reads spawn positions, zone info, ground items, and player state from EQ memory
- Streams packed binary records over TCP (default port 5555) to a connected client
- Supports GUI mode (default), console mode, and debug mode

The Rust client component currently:
- Is scaffolded only (`client/src/main.rs` placeholder)
- Does not yet implement map rendering or server connectivity

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
      main.rs         # scaffold only — C1 not yet started
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

# Run tests
cargo test

# Build release binary
cargo build --release -p winshoweq-server
```

Automated/manual server test docs live in `server/scripts/README.md`.

## Configuration

By default, the server resolves INI files in this order:

1. workspace-local `server\<name>` when running from a checkout (for example via `cargo run`
   or `target\debug\WinShowEQServer.exe`)
2. `%ProgramData%\WinShowEQ\<name>` when that directory exists (installer layout)
3. current working directory fallback for ad-hoc local runs

Two INI files are expected in that resolved config location:

**`myseqserver.ini`** — runtime offsets and port:
```ini
[File Info]
PatchDate=MM/DD/YYYY

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

**`config.ini`** — scanner patterns and startup preferences:
```ini
[Server]
StartMinimized=0

[EQG_SpawnList]
Pattern=\x48\x8B...
; ... (byte patterns for scanning eqgame.exe)
```

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