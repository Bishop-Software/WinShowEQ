# WinShowEQ Windows Installer

This folder contains the first-pass Windows installer scaffold for `WinShowEQ`.

## What it builds

### Server
- Installs `WinShowEQServer.exe` to `%ProgramFiles%\WinShowEQ\bin`
- Seeds `myseqserver.ini` and `patterns.ini` into `%ProgramData%\WinShowEQ`
- Creates Start Menu shortcuts that launch the server GUI mode with `WorkingDir=%ProgramData%\WinShowEQ`
- Preserves user-edited INI files on upgrade and uninstall

### Client
- Installs `WinShowEQClient.exe` to `%ProgramFiles%\WinShowEQ\bin`
- Creates `%ProgramData%\WinShowEQ\client\` with subdirectories for filters, timers, annotations, maps, logs, cfg
- Seeds `client.ini` template with defaults (Host, Port, alert modes)
- Seeds `cfg\` with app data files (`classes.json`, `colors.json`)
- Creates Start Menu shortcuts to launch the client and open the config directory
- Preserves user-edited config files on upgrade and uninstall
- Note: User must populate map files in `%ProgramData%\WinShowEQ\client\maps\` manually or via the Options dialog

## Config path behavior

### Server

`WinShowEQServer` resolves INI paths in this order:

1. workspace-local `server\<name>` when running from a source checkout
2. `%ProgramData%\WinShowEQ\<name>` when `%ProgramData%\WinShowEQ` exists
3. current working directory fallback for ad-hoc local/dev runs

This lets installed launches work even when the EXE is started directly from `%ProgramFiles%`.
Shortcuts use `%ProgramData%\WinShowEQ` as their working directory for consistency.

### Client

`WinShowEQClient` expects configuration directories in:

- `%ProgramData%\WinShowEQ\client\filters\` — XML filter files
- `%ProgramData%\WinShowEQ\client\timers\` — zone-specific timer files
- `%ProgramData%\WinShowEQ\client\annotations\` — per-zone map notes
- `%ProgramData%\WinShowEQ\client\maps\` — EQ zone map files (user-provided)
- `%ProgramData%\WinShowEQ\client\logs\` — dated application logs
- `%ProgramData%\WinShowEQ\client\cfg\` — app data files (classes, colors)

Shortcuts are configured with `WorkingDir=%ProgramData%\WinShowEQ\client` for consistent file discovery.

## Prerequisites

- Rust toolchain available on `PATH`
- Windows x64 build environment for the workspace
- [Inno Setup 6](https://jrsoftware.org/isinfo.php) installed, or pass `-InnoSetupCompilerPath`

## Build the installer

From the repo root:

```powershell
# Build installer (server + client)
pwsh -File .\installer\build-installer.ps1

# Stage files only, without running Inno Setup compiler
pwsh -File .\installer\build-installer.ps1 -StageOnly

# Specify a custom Inno Setup compiler path
pwsh -File .\installer\build-installer.ps1 -InnoSetupCompilerPath "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"

# Override the version (default: read from server/Cargo.toml)
pwsh -File .\installer\build-installer.ps1 -Version "1.2.3"
```

## Output locations

The staging/build script writes under `target/`, so it stays alongside the rest of the generated
workspace artifacts:

- `target/installer-stage/` — files staged for packaging
- `target/installer-output/` — final installer output from Inno Setup

## Installer notes

### Server
- The default installer type is **Server + Client** (recommended); server-only is also available.
- `myseqserver.ini` and `patterns.ini` are installed with `onlyifdoesntexist` and
  `uninsneveruninstall`, so upgrades preserve user edits.
- `config.ini` is **not seeded** by the installer — it is auto-created on first launch when
  the user changes a setting (e.g. Start Minimized or Browse for eqgame.exe).
- The configuration folder shortcut opens `%ProgramData%\WinShowEQ` directly for manual edits.

### Client
- Included in the default install type alongside the server.
- `client.ini` is installed with `onlyifdoesntexist` and `uninsneveruninstall` flags.
- All client data directories (filters, timers, annotations, maps, logs, cfg) are created during install.
- `cfg\` files (`classes.json`, `colors.json`) are updated on upgrade (`ignoreversion`).
- Users must populate map files in the `maps\` directory manually or via the client Options dialog.
- The configuration folder shortcut opens `%ProgramData%\WinShowEQ\client` for direct access to config/data files.

