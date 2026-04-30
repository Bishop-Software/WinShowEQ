param(
    [switch]$SkipBuild,
    [switch]$IncludeClient,
    [switch]$StageOnly,
    [string]$Version,
    [string]$InnoSetupCompilerPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$installerDir = $PSScriptRoot
$repoRoot = (Resolve-Path (Join-Path $installerDir "..")).Path
$targetDir = Join-Path $repoRoot "target"
$stageRoot = Join-Path $targetDir "installer-stage"
$outputRoot = Join-Path $targetDir "installer-output"
$issPath = Join-Path $installerDir "WinShowEQ.iss"
$serverCargoToml = Join-Path $repoRoot "server\Cargo.toml"
$serverExe = Join-Path $repoRoot "target\release\WinShowEQServer.exe"
$clientExe = Join-Path $repoRoot "target\release\WinShowEQClient.exe"

function Get-PackageVersion {
    param([string]$CargoTomlPath)

    $lines = Get-Content -Path $CargoTomlPath
    foreach ($line in $lines) {
        if ($line -match '^version\s*=\s*"([^"]+)"\s*$') {
            return $Matches[1]
        }
    }

    throw "Could not determine package version from $CargoTomlPath"
}

function Invoke-NativeCommand {
    param(
        [string]$FilePath,
        [string[]]$Arguments,
        [string]$WorkingDirectory
    )

    Write-Host "`n> $FilePath $($Arguments -join ' ')" -ForegroundColor Cyan
    $process = Start-Process -FilePath $FilePath -ArgumentList $Arguments -WorkingDirectory $WorkingDirectory -NoNewWindow -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "Command failed with exit code $($process.ExitCode): $FilePath"
    }
}

function New-CleanDirectory {
    param([string]$Path)

    if (Test-Path $Path) {
        Remove-Item -Path $Path -Recurse -Force
    }

    $null = New-Item -ItemType Directory -Path $Path
}

function Resolve-InnoSetupCompiler {
    param([string]$ExplicitPath)

    if ($ExplicitPath) {
        if (-not (Test-Path $ExplicitPath)) {
            throw "Inno Setup compiler not found at $ExplicitPath"
        }

        return (Resolve-Path $ExplicitPath).Path
    }

    $candidates = @(
        (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
        (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe")
    )

    foreach ($candidate in $candidates) {
        if ($candidate -and (Test-Path $candidate)) {
            return (Resolve-Path $candidate).Path
        }
    }

    $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }

    throw "Could not find ISCC.exe. Install Inno Setup 6 or pass -InnoSetupCompilerPath."
}

if (-not $Version) {
    $Version = Get-PackageVersion -CargoTomlPath $serverCargoToml
}

Write-Host "Preparing WinShowEQ installer assets..." -ForegroundColor Green
Write-Host " - Repo root: $repoRoot"
Write-Host " - Version:   $Version"
Write-Host " - Client:    $IncludeClient"
Write-Host " - Stage dir: $stageRoot"
Write-Host " - Output:    $outputRoot"

if (-not $SkipBuild) {
    Invoke-NativeCommand -FilePath "cargo" -Arguments @("build", "--release", "-p", "winshoweq-server") -WorkingDirectory $repoRoot
    if ($IncludeClient) {
        Invoke-NativeCommand -FilePath "cargo" -Arguments @("build", "--release", "-p", "winshoweq-client") -WorkingDirectory $repoRoot
    }
}

if (-not (Test-Path $serverExe)) {
    throw "Missing server binary: $serverExe"
}

if ($IncludeClient -and -not (Test-Path $clientExe)) {
    throw "Missing client binary: $clientExe"
}

New-CleanDirectory -Path $stageRoot
$null = New-Item -ItemType Directory -Path (Join-Path $stageRoot "bin")
$null = New-Item -ItemType Directory -Path (Join-Path $stageRoot "config")
$null = New-Item -ItemType Directory -Path (Join-Path $stageRoot "docs")
$null = New-Item -ItemType Directory -Path $outputRoot -Force

Copy-Item -Path $serverExe -Destination (Join-Path $stageRoot "bin\WinShowEQServer.exe")
Copy-Item -Path (Join-Path $repoRoot "server\myseqserver.ini") -Destination (Join-Path $stageRoot "config\myseqserver.ini")
Copy-Item -Path (Join-Path $repoRoot "server\patterns.ini") -Destination (Join-Path $stageRoot "config\patterns.ini")
Copy-Item -Path (Join-Path $repoRoot "README.md") -Destination (Join-Path $stageRoot "docs\README.md")
Copy-Item -Path (Join-Path $repoRoot "LICENSE") -Destination (Join-Path $stageRoot "docs\LICENSE.txt")
Copy-Item -Path (Join-Path $installerDir "README.md") -Destination (Join-Path $stageRoot "docs\INSTALLER-README.md")

if ($IncludeClient) {
    Copy-Item -Path $clientExe -Destination (Join-Path $stageRoot "bin\WinShowEQClient.exe")
}

Write-Host "`nStaging complete." -ForegroundColor Green
Write-Host " - Server exe: $(Join-Path $stageRoot 'bin\WinShowEQServer.exe')"
Write-Host " - Config dir: $(Join-Path $stageRoot 'config')"
if ($IncludeClient) {
    Write-Host " - Client exe: $(Join-Path $stageRoot 'bin\WinShowEQClient.exe')"
}

if ($StageOnly) {
    Write-Host "`n-StageOnly specified; skipping Inno Setup compilation." -ForegroundColor Yellow
    return
}

$compilerPath = Resolve-InnoSetupCompiler -ExplicitPath $InnoSetupCompilerPath

Push-Location $installerDir
try {
    $compilerArgs = @("/DAppVersion=$Version")
    if ($IncludeClient) {
        $compilerArgs += "/DIncludeClient=1"
    }
    $compilerArgs += "WinShowEQ.iss"

    Invoke-NativeCommand -FilePath $compilerPath -Arguments $compilerArgs -WorkingDirectory $installerDir
}
finally {
    Pop-Location
}

Write-Host "`nInstaller build complete." -ForegroundColor Green
Write-Host "Output directory: $outputRoot"

