param(
    [switch]$SkipBuild,
    [switch]$NoEqOnly,
    [switch]$WithEqOnly,
    [switch]$RequireEq,
    [switch]$WithEqListOnly,
    [int]$NoEqStartupTimeoutSeconds = 6,
    [int]$WithEqStartupTimeoutSeconds = 8,
    [string]$ZoneShortName,
    [string]$TargetName,
    [string]$TargetCoords,
    [int]$CharacterLevel = -1,
    [string]$WorldDate,
    [string]$EqExePath,
    [switch]$ExpectGroundItems,
    [string]$ContextFile,
    [switch]$GenerateContextTemplate,
    [switch]$Force
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$serverDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$repoRoot = (Resolve-Path (Join-Path $serverDir "..")).Path
$noEqScript = Join-Path $PSScriptRoot "run-no-eq-tests.ps1"
$withEqScript = Join-Path $PSScriptRoot "run-with-eq-tests.ps1"

. (Join-Path $PSScriptRoot "lib\test-common.ps1")

if ($NoEqOnly -and $WithEqOnly) {
    throw "-NoEqOnly and -WithEqOnly cannot be used together."
}

if (-not (Test-Path $noEqScript)) {
    throw "Missing script: $noEqScript"
}
if (-not (Test-Path $withEqScript)) {
    throw "Missing script: $withEqScript"
}

function Invoke-ChildScript {
    param(
        [string]$Path,
        [object[]]$Arguments,
        [string]$Label
    )

    Write-Host ""
    Write-Host "=== Unified runner: running $Label ===" -ForegroundColor Cyan

    $childArgs = @("-File", $Path) + $Arguments
    $process = Start-Process -FilePath "pwsh" -ArgumentList $childArgs -NoNewWindow -Wait -PassThru
    $exitCode = $process.ExitCode

    if ($exitCode -eq 0) {
        Write-Host "=== Unified runner: $Label completed (exit 0) ===" -ForegroundColor Green
    } else {
        Write-Host "=== Unified runner: $Label failed (exit $exitCode) ===" -ForegroundColor Red
    }

    return [int]$exitCode
}

Write-Host "Preparing WinShowEQ unified runner test run..."

if (-not $SkipBuild) {
    Write-Host "Building winshoweq-server once before running suites..."
    Build-WinShowEqServer -RepoRoot $repoRoot
}

$overallExit = 0
$ranNoEq = $false
$ranWithEq = $false
$skippedWithEq = $false

$eqRunning = Test-EqRunning

if (-not $WithEqOnly) {
    $noEqArgs = @("-SkipBuild", "-StartupTimeoutSeconds", "$NoEqStartupTimeoutSeconds")
    $noEqExit = Invoke-ChildScript -Path $noEqScript -Arguments $noEqArgs -Label "No-EQ suite"
    $ranNoEq = $true
    if ($noEqExit -ne 0) {
        $overallExit = $noEqExit
    }
}

if (-not $NoEqOnly) {
    if (-not $eqRunning -and -not $WithEqListOnly -and -not $GenerateContextTemplate) {
        $skippedWithEq = $true
        Write-Host ""
        Write-Host "Skipping EQ-required suite because eqgame.exe is not running." -ForegroundColor Yellow
        if ($RequireEq) {
            Write-Host "-RequireEq is set, so this is treated as a failure." -ForegroundColor Red
            if ($overallExit -eq 0) {
                $overallExit = 1
            }
        }
    } else {
        $withEqArgs = @("-SkipBuild", "-StartupTimeoutSeconds", "$WithEqStartupTimeoutSeconds")

        if ($WithEqListOnly) { $withEqArgs += "-ListOnly" }
        if (Test-HasText -Value $ZoneShortName) { $withEqArgs += @("-ZoneShortName", $ZoneShortName) }
        if (Test-HasText -Value $TargetName) { $withEqArgs += @("-TargetName", $TargetName) }
        if (Test-HasText -Value $TargetCoords) { $withEqArgs += @("-TargetCoords", $TargetCoords) }
        if ($CharacterLevel -ge 0) { $withEqArgs += @("-CharacterLevel", "$CharacterLevel") }
        if (Test-HasText -Value $WorldDate) { $withEqArgs += @("-WorldDate", $WorldDate) }
        if (Test-HasText -Value $EqExePath) { $withEqArgs += @("-EqExePath", $EqExePath) }
        if ($ExpectGroundItems) { $withEqArgs += "-ExpectGroundItems" }
        if (Test-HasText -Value $ContextFile) { $withEqArgs += @("-ContextFile", $ContextFile) }
        if ($GenerateContextTemplate) { $withEqArgs += "-GenerateContextTemplate" }
        if ($Force) { $withEqArgs += "-Force" }

        $withEqExit = Invoke-ChildScript -Path $withEqScript -Arguments $withEqArgs -Label "EQ-required suite"
        $ranWithEq = $true
        if ($withEqExit -ne 0 -and $overallExit -eq 0) {
            $overallExit = $withEqExit
        }
    }
}

Write-Host ""
Write-Host "Unified runner result:" -ForegroundColor Cyan
Write-Host " - Ran no-EQ suite: $ranNoEq"
Write-Host " - Ran EQ suite: $ranWithEq"
Write-Host " - Skipped EQ suite: $skippedWithEq"
Write-Host " - Final exit code: $overallExit"

exit $overallExit

