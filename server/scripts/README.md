# Scripts

## No-EQ Manual Test Automation

`run-no-eq-tests.ps1` automates the manual test cases from `design/MANUAL_TEST_GUIDE.md`
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

`run-with-eq-tests.ps1` automates the manual tests that require a running `eqgame.exe`.

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

If needed, you can still call the underlying scripts directly:

- `run-no-eq-tests.ps1`
- `run-with-eq-tests.ps1`
