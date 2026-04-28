param(
    [switch]$SkipBuild,
    [int]$StartupTimeoutSeconds = 6
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$serverDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$repoRoot = (Resolve-Path (Join-Path $serverDir "..")).Path
$exePath = Join-Path $repoRoot "target\debug\WinShowEQServer.exe"

. (Join-Path $PSScriptRoot "lib\test-common.ps1")

$passCount = 0
$failCount = 0
$skipCount = 0

function Run-Case {
    param(
        [string]$Id,
        [string]$Description,
        [scriptblock]$Body,
        [switch]$SkipWhenEqRunning
    )

    if ($SkipWhenEqRunning -and $script:eqRunning) {
        Write-Host "[SKIP] $Id - $Description (eqgame.exe is currently running)" -ForegroundColor Yellow
        $script:skipCount += 1
        return
    }

    try {
        & $Body
        Write-Host "[PASS] $Id - $Description" -ForegroundColor Green
        $script:passCount += 1
    } catch {
        Write-Host "[FAIL] $Id - $Description" -ForegroundColor Red
        Write-Host "       $($_.Exception.Message)" -ForegroundColor Red
        $script:failCount += 1
    }
}

Write-Host "Preparing WinShowEQ no-EQ automation run..."

if (-not $SkipBuild) {
    Build-WinShowEqServer -RepoRoot $repoRoot
}

if (-not (Test-Path $exePath)) {
    throw "Binary not found at $exePath"
}

$eqRunning = Test-EqRunning
if ($eqRunning) {
    Write-Host "eqgame.exe is running. Tests that require no active EQ process will be skipped." -ForegroundColor Yellow
}

$tempRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("winshoweq-no-eq-" + [Guid]::NewGuid().ToString("N"))
$null = New-Item -ItemType Directory -Path $tempRoot
$tempIni = Join-Path $tempRoot "alt.ini"
Copy-Item (Join-Path $serverDir "myseqserver.ini") $tempIni

$iniText = Get-Content $tempIni -Raw
$iniText = [Regex]::Replace($iniText, '(?im)^\s*port\s*=\s*\d+\s*$', 'port=5556')
Set-Content -Path $tempIni -Value $iniText -NoNewline

try {
    Run-Case "T01" "--help output includes expected visible commands" {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("--help") -WorkingDirectory $serverDir
        Assert-ExitCode -Result $r -Expected 0 -Message "--help should exit cleanly"
        Assert-Contains -Text $r.StdOut -Needle "-f <FILE>" -Message "help should document -f"
        Assert-Contains -Text $r.StdOut -Needle "console" -Message "help should list console"
        Assert-Contains -Text $r.StdOut -Needle "debug" -Message "help should list debug"
        Assert-NotContains -Text $r.StdOut -Needle "serve-stub" -Message "help should hide serve-stub"
        Assert-NotContains -Text $r.StdOut -Needle "attach" -Message "help should hide attach"
        Assert-NotContains -Text $r.StdOut -Needle "scan" -Message "help should hide scan"
    }

    Run-Case "T02" "unknown flag is rejected" {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("--bogus") -WorkingDirectory $serverDir
        if ($r.TimedOut) { throw "--bogus unexpectedly timed out" }
        if ($r.ExitCode -eq 0) { throw "--bogus should return non-zero" }
        Assert-Contains -Text $r.StdErr -Needle "--bogus" -Message "error should include invalid flag"
    }

    Run-Case "T03" "unknown subcommand is rejected" {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("blarg") -WorkingDirectory $serverDir
        if ($r.TimedOut) { throw "blarg unexpectedly timed out" }
        if ($r.ExitCode -eq 0) { throw "unknown subcommand should return non-zero" }
        Assert-Contains -Text $r.StdErr -Needle "blarg" -Message "error should include unknown subcommand"
    }

    Run-Case "T04" "no args routes to console startup" {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @() -WorkingDirectory $serverDir -TimeoutSeconds $StartupTimeoutSeconds
        if (-not $r.TimedOut) { throw "console mode should continue running (timeout expected)" }
        Assert-Contains -Text $r.StdOut -Needle "[INFO] Patch date:" -Message "console startup should print patch date"
        Assert-Contains -Text $r.StdOut -Needle "[INFO] Port:" -Message "console startup should print port"
    }

    Run-Case "T05" "console subcommand routes to console startup" {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("console") -WorkingDirectory $serverDir -TimeoutSeconds $StartupTimeoutSeconds
        if (-not $r.TimedOut) { throw "console mode should continue running (timeout expected)" }
        Assert-Contains -Text $r.StdOut -Needle "[INFO] Patch date:" -Message "console startup should print patch date"
        Assert-Contains -Text $r.StdOut -Needle "[INFO] Port:" -Message "console startup should print port"
    }

    Run-Case "T06" "debug subcommand enters loop and exits with x" {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "x`n"
        Assert-ExitCode -Result $r -Expected 0 -Message "debug x should exit cleanly"
        Assert-Contains -Text $r.StdOut -Needle "x) exit debugger" -Message "debug menu should be printed"
    }

    Run-Case "T10" "-f valid alternate INI is honored in console mode" {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("-f", $tempIni, "console") -WorkingDirectory $serverDir -TimeoutSeconds $StartupTimeoutSeconds
        if (-not $r.TimedOut) { throw "console mode should continue running (timeout expected)" }
        Assert-Contains -Text $r.StdOut -Needle "[INFO] Port: 5556" -Message "alternate INI port should be used"
    }

    Run-Case "T11" "-f missing INI path falls back safely in console mode" {
        $missingIni = Join-Path $tempRoot "does_not_exist.ini"
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("-f", $missingIni, "console") -WorkingDirectory $serverDir -TimeoutSeconds $StartupTimeoutSeconds
        if ($r.TimedOut) {
            Assert-Contains -Text $r.StdOut -Needle "[INFO] Port: 5555" -Message "missing INI should fall back to default port"
            Assert-Contains -Text $r.StdErr -Needle "SpawnInfo Offsets are all zero" -Message "missing INI should warn about zero spawn offsets"
        } else {
            Assert-ExitCode -Result $r -Expected 0 -Message "missing INI should fail gracefully without crashing"
            Assert-ContainsAny -Text ($r.StdOut + "`n" + $r.StdErr) -Needles @("Invalid INI file", "SpawnInfo Offsets are all zero") -Message "missing INI should emit a clear diagnostic"
        }
    }

    Run-Case "T12" "-f valid alternate INI is honored in debug mode" {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("-f", $tempIni, "debug") -WorkingDirectory $serverDir -InputText "d`nx`n"
        Assert-ExitCode -Result $r -Expected 0 -Message "debug with alternate INI should exit cleanly"
        Assert-Contains -Text $r.StdOut -Needle "pZone = 0x" -Message "debug display should include primary offsets"
    }

    Run-Case "T20" "console startup without EQ shows waiting warning" -SkipWhenEqRunning {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("console") -WorkingDirectory $serverDir -TimeoutSeconds $StartupTimeoutSeconds
        if (-not $r.TimedOut) { throw "console mode should continue running (timeout expected)" }
        Assert-Contains -Text $r.StdOut -Needle "eqgame.exe not running" -Message "should warn when EQ is not running"
    }

    Run-Case "T30" "debug startup without EQ prints warning" -SkipWhenEqRunning {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "x`n"
        Assert-ExitCode -Result $r -Expected 0 -Message "debug should exit cleanly"
        Assert-Contains -Text $r.StdErr -Needle "eqgame.exe not found" -Message "missing EQ warning should be printed"
    }

    Run-Case "T31" "debug ? command reprints menu" {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "?`nx`n"
        Assert-ExitCode -Result $r -Expected 0 -Message "debug should exit cleanly"
        Assert-Contains -Text $r.StdOut -Needle "(d)isplay / (r)eload offsets" -Message "menu text should appear"
    }

    Run-Case "T32" "debug d shows zero offsets without local INI" {
        $emptyDir = Join-Path $tempRoot "no-ini"
        $null = New-Item -ItemType Directory -Path $emptyDir
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $emptyDir -InputText "d`nx`n"
        Assert-ExitCode -Result $r -Expected 0 -Message "debug should exit cleanly"
        Assert-Contains -Text $r.StdOut -Needle "pZone = 0x0" -Message "offsets should default to zero without INI"
    }

    Run-Case "T33" "debug r reload command prints confirmation" {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "r`nx`n"
        Assert-ExitCode -Result $r -Expected 0 -Message "debug should exit cleanly"
        Assert-Contains -Text $r.StdOut -Needle "Debugger: Memory offsets read in." -Message "reload confirmation should be printed"
    }

    Run-Case "T34" "invalid debug command prints error" {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "zzz`nx`n"
        Assert-ExitCode -Result $r -Expected 0 -Message "debug should exit cleanly"
        Assert-Contains -Text $r.StdOut -Needle "Invalid selection. Please try again." -Message "invalid command message should be printed"
    }

    Run-Case "T35" "debug x exits with code 0" {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "x`n"
        Assert-ExitCode -Result $r -Expected 0 -Message "x should exit debug loop"
    }

    Run-Case "T58" "sfw malformed date is rejected gracefully" {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "spo 5 0x1`nsfw 13/99/2024`nx`n"
        Assert-ExitCode -Result $r -Expected 0 -Message "debug should exit cleanly"
        Assert-Contains -Text $r.StdOut -Needle "Bad Date" -Message "malformed date should be rejected"
    }

    Run-Case "T60" "serve-stub starts and stays alive" {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("serve-stub") -WorkingDirectory $serverDir -TimeoutSeconds $StartupTimeoutSeconds
        if (-not $r.TimedOut) { throw "serve-stub should keep running (timeout expected)" }
        Assert-Contains -Text $r.StdOut -Needle "Starting stub server on port 5555" -Message "serve-stub banner should be printed"
    }

    Run-Case "T62" "attach without EQ prints not found" -SkipWhenEqRunning {
        $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("attach") -WorkingDirectory $serverDir
        Assert-ExitCode -Result $r -Expected 0 -Message "attach should exit cleanly"
        Assert-Contains -Text $r.StdErr -Needle "eqgame.exe not found" -Message "attach should report missing eqgame"
    }
}
finally {
    Remove-Item -Path $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host ""
Write-Host "No-EQ automation results: passed=$passCount failed=$failCount skipped=$skipCount"
if ($failCount -gt 0) {
    exit 1
}


