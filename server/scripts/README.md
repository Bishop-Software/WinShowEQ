# WinShowEQ Server — Test Automation & Manual Test Guide

## Overview

This directory contains automated test scripts and comprehensive manual test specifications for the WinShowEQ server. The scripts automate many of the manual test cases, while this guide documents all expected behaviors.

**Contents:**
- Automated test scripts (PowerShell)
- Manual test specifications (7 sections, 63+ test cases)
- Build instructions and usage examples

---

## Table of Contents

1. [Quick Start](#quick-start)
2. [Automated Test Scripts](#automated-test-scripts)
   - [No-EQ Tests](#no-eq-manual-test-automation)
   - [EQ-Running Tests](#eq-running-manual-test-automation)
   - [Unified Runner](#unified-runner)
3. [Manual Test Guide](#winshoweq-server--manual-test-guide)
   - [Test Specifications](#test-specifications)

---

## Quick Start

Build the binary first:
```powershell
cd server
cargo build -p winshoweq-server
```

Run the automated test suite:
```powershell
pwsh -File .\scripts\run-tests.ps1
```

Binary location after build: `target\debug\WinShowEQServer.exe`

---

## Automated Test Scripts

### No-EQ Manual Test Automation

`run-no-eq-tests.ps1` automates the manual test cases (see the Manual Test Guide section below)
that do not require a running EverQuest client.

Covered tests currently include:

- CLI dispatch checks (`T01` to `T06`)
- Alternate INI path checks (`T10` to `T12`)
- Console startup without EQ (`T20`, skipped automatically if `eqgame.exe` is running)
- Debug loop checks without EQ (`T30` to `T35`, plus `T58`)
- Hidden dev subcommand startup checks (`T60`, `T62`)

### Usage

Run from the `server` folder:

```powershell
pwsh -File .\scripts\run-no-eq-tests.ps1
```

Skip build if you already compiled `WinShowEQServer`:

```powershell
pwsh -File .\scripts\run-no-eq-tests.ps1 -SkipBuild
```

Adjust startup timeout for long-running modes (`console`, `serve-stub`):

```powershell
pwsh -File .\scripts\run-no-eq-tests.ps1 -StartupTimeoutSeconds 10
```

## EQ-Running Manual Test Automation

`run-with-eq-tests.ps1` automates the manual tests that require a running `eqgame.exe` 
(see the Manual Test Guide section below for complete test specifications).

Required automated checks include:

- Console attach startup (`T21`)
- Debug attach, memory/spawn inspection, and offset mutation/reload checks (`T40` to `T49`)
- Process enumeration (`T52`, `T53`)
- Attach subcommand (`T61`)

Optional checks are enabled by parameters for live context-dependent values:

- `T50` with `-ZoneShortName`
- `T51` with `-TargetName`
- `T54` with `-TargetCoords`
- `T55` with `-CharacterLevel`
- `T56` with `-WorldDate`
- `T57` with `-ExpectGroundItems`
- `T63` with `-EqExePath`

`-TargetCoords` syntax: pass a single comma-separated string (`X,Y,Z`), for example
`"123.4,-55.0,7.2"`. The debug scanner also accepts `X` or `X,Y`, but `T54` is typically
run with full `X,Y,Z`.

You can also supply optional EQ inputs from a JSON file via `-ContextFile` (CLI flags still override JSON values).

If an optional live scan exceeds its timeout, it is reported as `[SKIP]` with a reason instead of
failing the whole run.

Generate a starter JSON template at any path:

```powershell
pwsh -File .\scripts\run-with-eq-tests.ps1 -GenerateContextTemplate -ContextFile C:\Temp\eq-test-context.json
```

Generated templates are skip-safe by default (empty strings, `CharacterLevel=-1`,
`ExpectGroundItems=false`), so optional checks remain skipped until you fill in real values.

By default, template generation refuses to overwrite an existing file; pass `-Force` to overwrite:

```powershell
pwsh -File .\scripts\run-with-eq-tests.ps1 -GenerateContextTemplate -ContextFile C:\Temp\eq-test-context.json -Force
```

List planned test cases without running:

```powershell
pwsh -File .\scripts\run-with-eq-tests.ps1 -ListOnly
```

Run required EQ tests:

```powershell
pwsh -File .\scripts\run-with-eq-tests.ps1
```

Run required + optional tests when you have supporting in-game values:

```powershell
pwsh -File .\scripts\run-with-eq-tests.ps1 `
  -ZoneShortName qeytoqrg `
  -TargetName OrcPawn `
  -TargetCoords "123.4,-55.0,7.2" `
  -CharacterLevel 60 `
  -WorldDate 06/15/3017 `
  -ExpectGroundItems
```

Run with a JSON context file:

```powershell
pwsh -File .\scripts\run-with-eq-tests.ps1 -ContextFile .\scripts\eq-test-context.sample.json
```

In JSON context files, set `TargetCoords` as a string in the same `X,Y,Z` format.

## Unified Runner

`run-tests.ps1` is the canonical single entry point. It routes to existing no-EQ and EQ
suites, keeping the current logic:

- Always runs the no-EQ suite unless `-WithEqOnly` is used
- Runs the EQ suite when `eqgame.exe` is running
- Auto-skips EQ suite when `eqgame.exe` is not running
- Fails when EQ is missing only if `-RequireEq` is used

Run both suites (EQ suite auto-skips if `eqgame.exe` is not running):

```powershell
pwsh -File .\scripts\run-tests.ps1
```

Run only no-EQ tests:

```powershell
pwsh -File .\scripts\run-tests.ps1 -NoEqOnly
```

Require EQ suite to run (fail if `eqgame.exe` is not running):

```powershell
pwsh -File .\scripts\run-tests.ps1 -RequireEq
```

Preview EQ-suite test list through the Unified Runner without launching EQ:

```powershell
pwsh -File .\scripts\run-tests.ps1 -WithEqOnly -WithEqListOnly -SkipBuild
```

Run the Unified Runner with a JSON context file for EQ optional checks:

```powershell
pwsh -File .\scripts\run-tests.ps1 -ContextFile .\scripts\eq-test-context.sample.json
```

Generate a starter JSON template via the Unified Runner:

```powershell
pwsh -File .\scripts\run-tests.ps1 -WithEqOnly -GenerateContextTemplate -ContextFile C:\Temp\eq-test-context.json
```

Overwrite an existing template file via the Unified Runner:

```powershell
pwsh -File .\scripts\run-tests.ps1 -WithEqOnly -GenerateContextTemplate -ContextFile C:\Temp\eq-test-context.json -Force
```

If you run from the workspace root, prefix commands with `server\` (for example,
`pwsh -File .\server\scripts\run-tests.ps1`).

### Direct Script Access

If you prefer to run scripts directly instead of through the unified runner:

```powershell
.\scripts\run-no-eq-tests.ps1    # No-EQ tests only
.\scripts\run-with-eq-tests.ps1  # EQ-dependent tests only
```

---

# WinShowEQ Server — Manual Test Guide

## Test Specifications

**Milestone 6 Coverage:** CLI dispatch + debug loop. M7 will include GUI-specific tests.

### Prerequisites Legend
- `[EQ]` — Requires running `eqgame.exe` with a loaded zone
- `[INI]` — Requires valid `myseqserver.ini` and `config.ini` (typically in the binary directory)
- `[CS]` — Requires C# MySEQ client connection

### Test Categories
The following sections contain automated and manual test cases:

---

## 1. CLI Dispatch (`T01`–`T06`)

**Prerequisites:** None

### T01 — `--help` output
```
WinShowEQServer --help
```
**Expect:**
- Program name and `about` string printed
- `-f <FILE>` flag documented
- `console` and `debug` subcommands listed with their descriptions
- `scan`, `attach`, `serve-stub` are **not** shown (hidden)

### T02 — unknown flag rejected
```
WinShowEQServer --bogus
```
**Expect:** clap error message, non-zero exit code, no panic.

### T03 — unknown subcommand rejected
```
WinShowEQServer blarg
```
**Expect:** clap error message, non-zero exit code, no panic.

### T04 — no args routes to console mode
```
WinShowEQServer
```
**Expect:** same startup output as `WinShowEQServer console` (see Section 3).

### T05 — `console` subcommand explicit
```
WinShowEQServer console
```
**Expect:** same startup output as no-args (T04).

### T06 — `debug` subcommand enters debug loop
```
WinShowEQServer debug
```
**Expect:** debug loop menu printed, prompt `>` displayed, `x` exits cleanly.

---

## 2. Alternate INI Path (`T10`–`T12`)

**Prerequisites:** `[INI]`

### T10 — `-f` with valid alternate INI, console mode `[INI]`
Copy `myseqserver.ini` to a temp path (e.g., `C:\Temp\alt.ini`). Change the `Port` value to
something distinct (e.g., `5556`).
```
WinShowEQServer -f C:\Temp\alt.ini console
```
**Expect:** startup log shows `Port: 5556` — confirms the alternate file was read.

### T11 — `-f` with non-existent path, console mode
```
WinShowEQServer -f C:\Temp\does_not_exist.ini console
```
**Expect:** server starts (no crash). Offsets will be zero; warn logged about zero spawn offsets.
Port falls back to default (5555).

### T12 — `-f` with valid alternate INI, debug mode `[INI]`
```
WinShowEQServer -f C:\Temp\alt.ini debug
```
**Expect:** debug loop loads offsets from `alt.ini`. Type `d` to confirm primary offsets are
non-zero and match the values in `alt.ini`.

---

## 3. Console Mode (`T20`–`T24`)

**Prerequisites:** `[INI]` for most tests; `[EQ]` for T21+

### T20 — startup without EQ running `[INI]`
```
WinShowEQServer console
```
**Expect:**
- `[INFO] Patch date: …` and `[INFO] Port: 5555` printed
- `[WARN] eqgame.exe not running — will attach when found` printed
- Server does **not** crash; keeps listening on port 5555

### T21 — startup with EQ running `[EQ] [INI]`
```
WinShowEQServer console
```
**Expect:**
- `[STATE] Attached to eqgame.exe PID=XXXXX  Base=0x…` printed
- Base address is non-zero (actual loaded base, not the 0x140000000 fallback)

### T22 — C# client receives live data `[EQ] [INI] [CS]`
Start the server (`WinShowEQServer console`), then connect the C# MySEQ client.
**Expect:**
- Client connects (shown in server log as `[NET] Client connected`)
- Zone name populates in the client
- Character name and X/Y/Z coordinates visible and updating
- NPC/PC spawns appear on the map

### T23 — client disconnect, server keeps listening `[EQ] [INI] [CS]`
With the server running and the C# client connected, disconnect the client.
**Expect:**
- Server logs a disconnect event
- Server does **not** crash or exit
- Reconnecting the C# client resumes normal operation

### T24 — zone change re-syncs client `[EQ] [INI] [CS]`
With server + client running, zone in EverQuest (`/zone …` or gate out).
**Expect:**
- Client clears old spawns and re-populates with the new zone's data
- Zone name updates in the client

---

## 4. Debug Mode — Without EQ (`T30`–`T35`)

**Prerequisites:** None (EQ not running is expected)

### T30 — graceful start without EQ
```
WinShowEQServer debug
```
**Expect:**
- `Warning: eqgame.exe not found — memory commands will fail` printed to stderr
- Debug menu printed immediately after
- Prompt `>` displayed

### T31 — `?` reprints menu
At the `>` prompt, type `?`.
**Expect:** full command menu reprinted.

### T32 — `d` displays offsets (all zero without INI)
```
WinShowEQServer debug   (run from a directory with no myseqserver.ini)
> d
```
**Expect:** primary offsets all `0x0`; secondary offsets all `0x000`.

### T33 — `r` reloads offsets `[INI]`
Run from the directory containing `myseqserver.ini`. At the prompt:
```
> r
```
**Expect:** `Debugger: Memory offsets read in.` printed; `d` afterwards shows non-zero offsets.

### T34 — bad command prints error
```
> zzz
```
**Expect:** `Invalid selection. Please try again.`

### T35 — `x` exits
```
> x
```
**Expect:** debug loop exits, process terminates with code 0.

---

## 5. Debug Mode — With EQ Running (`T40`–`T58`)

**Prerequisites:** `[EQ] [INI]`

### T40 — startup attaches and loads offsets
```
WinShowEQServer debug
```
**Expect:**
- `Attached to eqgame.exe  PID: XXXXX  Base: 0x…` printed
- `Debugger: Memory offsets read in.` printed
- `d` shows non-zero primary offsets matching `myseqserver.ini`

### T41 — `spo` sets a primary offset by index
```
> spo 2 0x1234567890ABCDEF
> d
```
**Expect:** `pSelf = 0x1234567890ABCDEF` shown after `d`.

### T42 — `spo` sets a primary offset by name
```
> spo pTarget 0xDEADBEEF00000000
> d
```
**Expect:** `pTarget = 0xDEADBEEF00000000` shown.

### T43 — `spo` rejects invalid hex
```
> spo 0 notahex
```
**Expect:** `Failed to parse hex value 'notahex'` error.

### T44 — `sso` sets a secondary spawn offset by index
```
> r
> sso 0 0x1a0
> d
```
**Expect:** first spawn offset entry changed to `0x1a0 (416)`.

### T45 — `r` restores offsets after manual changes
```
> spo 0 0x0
> r
> d
```
**Expect:** `pZone` returns to the value in `myseqserver.ini`.

### T46 — `es` / `et` / `ez` / `ew` — examine raw memory
```
> es
```
**Expect:** 6144-byte hex dump printed with address, hex columns, ASCII sidebar. No crash even
if the pointer resolves to zero (prints "Failed to obtain valid memory pointer" message).

### T47 — `ps` / `pt` — display spawn info
```
> ps
```
**Expect:** named fields printed (`NameOffset`, `SpawnIDOffset`, `XOffset`, etc.). All fields
are present; coordinates are plausible floats for the current zone.

### T48 — `ws` / `vs` — walk spawn list from pSelf
```
> ws
```
**Expect:**
- "Walking spawnlist in reverse." printed
- One block per spawn with name and ID
- "Discovered N spawn entities during the walk." at the end
- N matches approximately the number of spawns visible in the zone
- No crash on null/end-of-list termination

### T49 — `wt` / `vt` — walk spawn list from pTarget
```
> wt
```
**Expect:** same structure as T48; starts from the target entity.

### T50 — `fz` — find zone name
```
> fz <current_zone_short_name>
```
(e.g., `fz qeytoqrg`)
**Expect:** at least one "Pointer match found at 0x…" line. Address should be near `pZone`.

### T51 — `ft` / `fs` — find spawn by name
```
> ft <target_name>
```
**Expect:** "Pointer match found at 0x…" for the targeted NPC/PC.

### T52 — `sp` — show processes
```
> sp
```
**Expect:** at minimum `eqgame.exe` listed with its PID.

### T53 — `sp <name>` — filter by process name
```
> sp notepad
```
**Expect:** lists running Notepad processes, or prints "No processes found matching 'notepad'."

### T54 — `sft X,Y,Z` — scan for float coordinates using pTarget
Stand next to a known NPC. Note your approximate X, Y, Z from a `/loc` command.
```
> sft X,Y,Z
```
**Expect:** one or more "X,Y,Z match found at offset 0x…" lines near offset 0 of the target struct.

### T55 — `sfu <int>` — scan for UINT from pSelf
```
> sfu <your_character_level>
```
**Expect:** at least one `match found at offset 0x…` near the level offset.

### T56 — `sfw mm/dd/yyyy` — scan for world date
Get the in-game date from `/time`.
```
> sfw 06/15/3017
```
**Expect:** "Date match found at offset 0x…" for a valid world-data pointer.

### T57 — `sg` — scan for ground items
Drop an item on the ground before running.
```
> sg
```
**Expect:** "Pointer match found at 0x…. Full string is IT…" for each ground item.

### T58 — `sfw` with malformed date rejects gracefully
```
> sfw 13/99/2024
```
**Expect:** "Bad Date" error message, no crash.

---

## 6. Hidden Dev Subcommands (`T60`–`T63`)

**Prerequisites:** `[CS]` for T60; `[EQ]` for T61; `[INI]` for T63

### T60 — `serve-stub` starts stub server `[CS]`
```
WinShowEQServer serve-stub
```
**Expect:** "Starting stub server on port 5555…" printed. The C# MySEQ client can connect and
receives stub spawn data (hardcoded values from `StubDataProvider`).

### T61 — `attach` finds EQ process `[EQ]`
```
WinShowEQServer attach
```
**Expect:** `Attached to eqgame.exe  PID: XXXXX  Base: 0x…` printed, process exits.

### T62 — `attach` without EQ running
```
WinShowEQServer attach
```
**Expect:** `eqgame.exe not found` printed to stderr, process exits.

### T63 — `scan <path>` resolves addresses `[INI]`
```
WinShowEQServer scan "C:\path\to\eqgame.exe"
```
**Expect:** six addresses printed (ZoneAddr, SpawnHeaderAddr, CharInfo, ItemsAddr, TargetAddr,
WorldAddr). Values should be non-zero and consistent with the current EQ client patch.

---

## 7. Regression Testing — C# Client Drop-in Replacement (`R1`–`R10`)

**Prerequisites:** `[EQ] [INI] [CS]`

Replace `serverx64.exe` with `WinShowEQServer.exe` (or run via `cargo run`). Connect the
existing C# MySEQ client to `127.0.0.1:5555`.

| # | Check | Pass criteria |
|---|-------|---------------|
| R1 | Client connects | No timeout; connection event logged |
| R2 | Zone name | Correct short name shown in client status bar |
| R3 | Character position | X/Y/Z coordinates update as you move |
| R4 | NPC spawns | All nearby NPCs appear with correct names and positions |
| R5 | PC spawns | Other players appear with correct class/level |
| R6 | Target updates | Target name and ID update on `/target` change |
| R7 | Ground items | Dropped items appear on the map |
| R8 | World time | In-game date/time matches `/time` output |
| R9 | Zone change | Client clears and reloads on zone transition |
| R10 | Multi-reconnect | Client can disconnect and reconnect multiple times without server restart |
