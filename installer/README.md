# WinShowEQ Windows Installer

This folder contains the first-pass Windows installer scaffold for `WinShowEQ`.

## What it builds

- Installs `WinShowEQServer.exe` to `%ProgramFiles%\WinShowEQ\bin`
- Seeds `myseqserver.ini` and `config.ini` into `%ProgramData%\WinShowEQ`
- Creates Start Menu shortcuts that launch the server with `WorkingDir=%ProgramData%\WinShowEQ`
- Preserves user-edited INI files on upgrade and uninstall
- Optionally includes `WinShowEQClient.exe` as a custom component

## Config path behavior

`WinShowEQServer` now resolves default INI paths in this order:

1. `%ProgramData%\WinShowEQ\<name>` when `%ProgramData%\WinShowEQ` exists
2. current working directory fallback for local/dev runs

This lets installed launches work even when the EXE is started directly from `%ProgramFiles%`.
Shortcuts still use `%ProgramData%\WinShowEQ` as their working directory for consistency.

## Prerequisites

- Rust toolchain available on `PATH`
- Windows x64 build environment for the workspace
- [Inno Setup 6](https://jrsoftware.org/isinfo.php) installed, or pass `-InnoSetupCompilerPath`

## Build the installer

From the repo root:

```powershell
pwsh -File .\installer\build-installer.ps1
```

Stage files only, without compiling the installer:

```powershell
pwsh -File .\installer\build-installer.ps1 -StageOnly
```

Include the placeholder client component in a custom installer build:

```powershell
pwsh -File .\installer\build-installer.ps1 -IncludeClient
```

Use a specific Inno Setup compiler path:

```powershell
pwsh -File .\installer\build-installer.ps1 -InnoSetupCompilerPath "C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
```

## Output locations

The staging/build script writes under `target/`, so it stays alongside the rest of the generated
workspace artifacts:

- `target/installer-stage/` — files staged for packaging
- `target/installer-output/` — final installer output from Inno Setup

## Installer notes

- The default installer type is **Server only**.
- The client component is optional because `client/src/main.rs` is still a placeholder.
- Config files are installed with `onlyifdoesntexist` and `uninsneveruninstall`, so upgrades do not
  overwrite user edits.
- The configuration folder shortcut opens `%ProgramData%\WinShowEQ` directly for manual edits.

