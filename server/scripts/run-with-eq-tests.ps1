param(
    [switch]$SkipBuild,
    [int]$StartupTimeoutSeconds = 8,
    [string]$ZoneShortName,
    [string]$TargetName,
    [string]$TargetCoords,
    [int]$CharacterLevel = -1,
    [string]$WorldDate,
    [string]$EqExePath,
    [switch]$ExpectGroundItems,
    [string]$ContextFile,
    [switch]$GenerateContextTemplate,
    [switch]$Force,
    [switch]$ListOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$serverDir = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$repoRoot = (Resolve-Path (Join-Path $serverDir "..")).Path
$exePath = Join-Path $repoRoot "target\debug\WinShowEQServer.exe"

. (Join-Path $PSScriptRoot "lib\test-common.ps1")

$reqPass = 0
$reqFail = 0
$reqSkip = 0
$optPass = 0
$optFail = 0
$optSkip = 0

function Run-Case {
    param(
        [string]$Id,
        [string]$Description,
        [scriptblock]$Body,
        [switch]$Optional,
        [string]$SkipReason = ""
    )

    if ($SkipReason.Length -gt 0) {
        Write-Host "[SKIP] $Id - $Description ($SkipReason)" -ForegroundColor Yellow
        if ($Optional) { $script:optSkip += 1 } else { $script:reqSkip += 1 }
        return
    }

    try {
        & $Body
        Write-Host "[PASS] $Id - $Description" -ForegroundColor Green
        if ($Optional) { $script:optPass += 1 } else { $script:reqPass += 1 }
    } catch {
        if ($Optional -and $_.Exception -is [System.TimeoutException]) {
            Write-Host "[SKIP] $Id - $Description ($($_.Exception.Message))" -ForegroundColor Yellow
            $script:optSkip += 1
            return
        }
        Write-Host "[FAIL] $Id - $Description" -ForegroundColor Red
        Write-Host "       $($_.Exception.Message)" -ForegroundColor Red
        if ($Optional) { $script:optFail += 1 } else { $script:reqFail += 1 }
    }
}

function Skip-OptionalIfTimedOut {
    param(
        [object]$Result,
        [int]$TimeoutSeconds,
        [string]$Reason
    )

    if ($Result.TimedOut) {
        throw [System.TimeoutException]::new("$Reason timed out after $TimeoutSeconds seconds")
    }
}

Write-Host "Preparing WinShowEQ EQ-running automation run..."

$plannedTests = @(
    "T21", "T40", "T41", "T42", "T43", "T44", "T45", "T46", "T47", "T48", "T49", "T52", "T53", "T61",
    "T50(optional)", "T51(optional)", "T54(optional)", "T55(optional)", "T56(optional)", "T57(optional)", "T63(optional)"
)

if ($ListOnly) {
    Write-Host "Planned tests:"
    $plannedTests | ForEach-Object { Write-Host " - $_" }
    Write-Host ""
    Write-Host "Optional test parameters:"
    Write-Host " -ZoneShortName <shortname>"
    Write-Host " -TargetName <spawnName>"
    Write-Host " -TargetCoords <X,Y,Z>"
    Write-Host " -CharacterLevel <int>"
    Write-Host " -WorldDate <mm/dd/yyyy>"
    Write-Host " -ExpectGroundItems"
    Write-Host " -EqExePath <path-to-eqgame.exe>"
    Write-Host " -ContextFile <path-to-json>"
    Write-Host " -GenerateContextTemplate"
    Write-Host " -Force"
    Write-Host ""
    Write-Host "Context JSON keys (optional):"
    Write-Host " - ZoneShortName"
    Write-Host " - TargetName"
    Write-Host " - TargetCoords"
    Write-Host " - CharacterLevel"
    Write-Host " - WorldDate"
    Write-Host " - EqExePath"
    Write-Host " - ExpectGroundItems"
    exit 0
}

if ($GenerateContextTemplate) {
    if (-not (Test-HasText -Value $ContextFile)) {
        throw "-GenerateContextTemplate requires -ContextFile <path-to-json>."
    }

    if ((Test-Path $ContextFile) -and -not $Force) {
        Write-Host "[ERROR] Context file already exists: $ContextFile" -ForegroundColor Red
        Write-Host "[ERROR] Re-run with -Force to overwrite the file." -ForegroundColor Red
        exit 1
    }

    $contextDir = Split-Path -Path $ContextFile -Parent
    if (Test-HasText -Value $contextDir -and -not (Test-Path $contextDir)) {
        $null = New-Item -ItemType Directory -Path $contextDir -Force
    }

    $template = [ordered]@{
        ZoneShortName = ""
        TargetName = ""
        TargetCoords = ""
        CharacterLevel = -1
        WorldDate = ""
        EqExePath = ""
        ExpectGroundItems = $false
    }

    $template | ConvertTo-Json | Set-Content -Path $ContextFile -Encoding utf8NoBOM
    Write-Host "Wrote EQ context template to: $ContextFile"
    exit 0
}

$context = $null
if (Test-HasText -Value $ContextFile) {
    if (-not (Test-Path $ContextFile)) {
        throw "Context file does not exist: $ContextFile"
    }
    try {
        $context = Get-Content -Path $ContextFile -Raw | ConvertFrom-Json
    } catch {
        throw "Failed to parse context JSON file '$ContextFile': $($_.Exception.Message)"
    }
    if ($null -eq $context) {
        throw "Context file '$ContextFile' is empty or invalid JSON."
    }

    $propZone = $context.PSObject.Properties["ZoneShortName"]
    $propTargetName = $context.PSObject.Properties["TargetName"]
    $propTargetCoords = $context.PSObject.Properties["TargetCoords"]
    $propCharacterLevel = $context.PSObject.Properties["CharacterLevel"]
    $propWorldDate = $context.PSObject.Properties["WorldDate"]
    $propEqExePath = $context.PSObject.Properties["EqExePath"]
    $propExpectGroundItems = $context.PSObject.Properties["ExpectGroundItems"]

    if (-not (Test-HasText -Value $ZoneShortName) -and $null -ne $propZone) { $ZoneShortName = [string]$propZone.Value }
    if (-not (Test-HasText -Value $TargetName) -and $null -ne $propTargetName) { $TargetName = [string]$propTargetName.Value }
    if (-not (Test-HasText -Value $TargetCoords) -and $null -ne $propTargetCoords) { $TargetCoords = [string]$propTargetCoords.Value }
    if (-not (Test-HasText -Value $WorldDate) -and $null -ne $propWorldDate) { $WorldDate = [string]$propWorldDate.Value }
    if (-not (Test-HasText -Value $EqExePath) -and $null -ne $propEqExePath) { $EqExePath = [string]$propEqExePath.Value }

    if ($CharacterLevel -lt 0 -and $null -ne $propCharacterLevel) {
        $parsedLevel = 0
        if ([int]::TryParse([string]$propCharacterLevel.Value, [ref]$parsedLevel)) {
            $CharacterLevel = $parsedLevel
        }
    }

    if (-not $ExpectGroundItems -and $null -ne $propExpectGroundItems) {
        $parsedExpectGround = $false
        if ([bool]::TryParse([string]$propExpectGroundItems.Value, [ref]$parsedExpectGround)) {
            $ExpectGroundItems = [System.Management.Automation.SwitchParameter]::new($parsedExpectGround)
        }
    }

    Write-Host "Loaded optional EQ context from: $ContextFile"
}

if (-not $SkipBuild) {
    Build-WinShowEqServer -RepoRoot $repoRoot
}

if (-not (Test-Path $exePath)) {
    throw "Binary not found at $exePath"
}

$eqRunning = Test-EqRunning
if (-not $eqRunning) {
    throw "eqgame.exe is not running. Start EverQuest with a character loaded, then rerun this script."
}

Run-Case "T21" "console mode attaches to EQ and stays alive" {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("console") -WorkingDirectory $serverDir -TimeoutSeconds $StartupTimeoutSeconds
    if (-not $r.TimedOut) {
        throw "console mode should continue running (timeout expected)"
    }
    Assert-Contains -Text $r.StdOut -Needle "[STATE] Attached to eqgame.exe PID=" -Message "console should attach to EQ"
    Assert-Matches -Text $r.StdOut -Pattern "Base=0x[1-9A-Fa-f][0-9A-Fa-f]*" -Message "console should report non-zero base"
}

Run-Case "T40" "debug mode attaches and loads offsets" {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "d`nx`n"
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "Attached to eqgame.exe  PID:" -Message "debug should attach"
    Assert-Contains -Text $r.StdOut -Needle "Debugger: Memory offsets read in." -Message "debug should load offsets"
    Assert-Matches -Text $r.StdOut -Pattern "pZone\s*=\s*0x[1-9A-Fa-f][0-9A-Fa-f]*" -Message "pZone should be non-zero"
}

Run-Case "T41" "spo sets primary offset by index" {
    $value = "0x1234567890ABCDEF"
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "spo 2 $value`nd`nx`n"
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "Primary offset #2 (pSelf) was set to $value" -Message "spo index should set pSelf"
    Assert-Contains -Text $r.StdOut -Needle "pSelf = $value" -Message "display should reflect pSelf override"
}

Run-Case "T42" "spo sets primary offset by name" {
    $value = "0xDEADBEEF00000000"
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "spo pTarget $value`nd`nx`n"
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "Primary offset #3 (pTarget) was set to $value" -Message "spo name should set pTarget"
    Assert-Contains -Text $r.StdOut -Needle "pTarget = $value" -Message "display should reflect pTarget override"
}

Run-Case "T43" "spo rejects invalid hex" {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "spo 0 notahex`nx`n"
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "Failed to parse hex value 'notahex'" -Message "invalid hex should be rejected"
}

Run-Case "T44" "sso sets secondary spawn offset by index" {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "r`nsso 0 0x1a0`nd`nx`n"
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "Secondary offset #0" -Message "sso should report success"
    Assert-Contains -Text $r.StdOut -Needle "0x1a0 (416)" -Message "display should show updated spawn offset"
}

Run-Case "T45" "r restores primary offsets after manual changes" {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "spo 0 0x0`nr`nd`nx`n"
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "Primary offset #0 (pZone) was set to 0x0" -Message "spo should set pZone to zero"
    Assert-Contains -Text $r.StdOut -Needle "Debugger: Memory offsets read in." -Message "reload should run"
    Assert-Matches -Text $r.StdOut -Pattern "pZone\s*=\s*0x[1-9A-Fa-f][0-9A-Fa-f]*" -Message "reload should restore pZone"
}

Run-Case "T46" "es handles invalid memory pointer gracefully" {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "spo pSelf 0x0`nes`nx`n" -TimeoutSeconds 20
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "Failed to obtain valid memory pointer" -Message "es should fail gracefully when pointer is invalid"
}

Run-Case "T47" "ps/pt display spawn fields or report invalid pointer gracefully" {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "ps`npt`nx`n" -TimeoutSeconds 30
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-ContainsAny -Text $r.StdOut -Needles @("NameOffset ->", "Failed to obtain valid memory pointer") -Message "ps/pt should print spawn fields or fail gracefully"
    Assert-ContainsAny -Text $r.StdOut -Needles @("XOffset ->", "Failed to obtain valid memory pointer") -Message "ps/pt should include coordinate field output when pointer is valid"
}

Run-Case "T48" "vs walk spawn list from pSelf" {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "vs`nx`n" -TimeoutSeconds 45
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "Walking spawnlist forward." -Message "vs output missing"
    Assert-Matches -Text $r.StdOut -Pattern "Discovered\s+[1-9][0-9]*\s+spawn entities during the walk\." -Message "spawn walk should discover at least one entity"
}

Run-Case "T49" "wt/vt handle invalid pTarget gracefully" {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "spo pTarget 0x0`nwt`nvt`nx`n" -TimeoutSeconds 20
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "Primary offset #3 (pTarget) was set to 0x0" -Message "T49 setup should force pTarget to zero"
    Assert-Matches -Text $r.StdOut -Pattern "Failed to obtain valid memory pointer" -Message "wt/vt should fail gracefully when pTarget is invalid"
}

Run-Case "T52" "sp lists eqgame process" {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "sp`nx`n"
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "eqgame.exe" -Message "sp should list eqgame.exe"
}

Run-Case "T53" "sp <name> filters process list" {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "sp notepad`nx`n"
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-ContainsAny -Text $r.StdOut -Needles @("No processes found matching 'notepad'.", "Exe: notepad") -Message "sp notepad should list matches or no-match message"
}

Run-Case "T61" "attach reports PID and base when EQ is running" {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("attach") -WorkingDirectory $serverDir
    Assert-ExitCode -Result $r -Expected 0 -Message "attach should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "Attached to eqgame.exe  PID:" -Message "attach should report PID"
    Assert-Matches -Text $r.StdOut -Pattern "Base:\s*0x[1-9A-Fa-f][0-9A-Fa-f]*" -Message "attach should report non-zero base"
}

$skipT50 = if (-not (Test-HasText -Value $ZoneShortName)) { "Provide -ZoneShortName" } else { "" }
Run-Case "T50" "fz finds zone name pointer" -Optional -SkipReason $skipT50 {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "fz $ZoneShortName`nx`n" -TimeoutSeconds 30
    Skip-OptionalIfTimedOut -Result $r -TimeoutSeconds 30 -Reason "fz zone-name scan"
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "Pointer match found" -Message "fz should find at least one pointer match"
}

$skipT51 = if (-not (Test-HasText -Value $TargetName)) { "Provide -TargetName" } else { "" }
Run-Case "T51" "ft/fs find spawn by name" -Optional -SkipReason $skipT51 {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "ft $TargetName`nx`n" -TimeoutSeconds 45
    Skip-OptionalIfTimedOut -Result $r -TimeoutSeconds 45 -Reason "ft target-name scan"
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "Pointer match found" -Message "ft should find at least one pointer match"
}

$skipT54 = if (-not (Test-HasText -Value $TargetCoords)) { "Provide -TargetCoords 'X,Y,Z'" } else { "" }
Run-Case "T54" "sft scans for target float coordinates" -Optional -SkipReason $skipT54 {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "sft $TargetCoords`nx`n" -TimeoutSeconds 30
    Skip-OptionalIfTimedOut -Result $r -TimeoutSeconds 30 -Reason "sft target-coordinate scan"
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "match found at offset" -Message "sft should find coordinate offsets"
}

$skipT55 = if ($CharacterLevel -lt 0) { "Provide -CharacterLevel" } else { "" }
Run-Case "T55" "sfu scans for UINT using pSelf" -Optional -SkipReason $skipT55 {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "sfu $CharacterLevel`nx`n" -TimeoutSeconds 30
    Skip-OptionalIfTimedOut -Result $r -TimeoutSeconds 30 -Reason "sfu self UINT scan"
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "$CharacterLevel  match found at offset" -Message "sfu should find at least one match"
}

$skipT56 = if (-not (Test-HasText -Value $WorldDate)) { "Provide -WorldDate mm/dd/yyyy" } else { "" }
Run-Case "T56" "sfw scans for world date" -Optional -SkipReason $skipT56 {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "sfw $WorldDate`nx`n" -TimeoutSeconds 90
    Skip-OptionalIfTimedOut -Result $r -TimeoutSeconds 90 -Reason "sfw world-date scan"
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "Date match found at offset" -Message "sfw should find at least one date match"
}

$skipT57 = if (-not $ExpectGroundItems) { "Pass -ExpectGroundItems after dropping an item" } else { "" }
Run-Case "T57" "sg scans for ground items" -Optional -SkipReason $skipT57 {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("debug") -WorkingDirectory $serverDir -InputText "sg`nx`n" -TimeoutSeconds 30
    Skip-OptionalIfTimedOut -Result $r -TimeoutSeconds 30 -Reason "sg ground-item scan"
    Assert-ExitCode -Result $r -Expected 0 -Message "debug loop should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "Pointer match found" -Message "sg should find at least one ground-item pointer"
}

$skipT63 = if (-not (Test-HasText -Value $EqExePath)) { "Provide -EqExePath to run scan" } elseif (-not (Test-Path $EqExePath)) { "EqExePath does not exist: $EqExePath" } else { "" }
Run-Case "T63" "scan resolves six primary addresses" -Optional -SkipReason $skipT63 {
    $r = Invoke-TestProcess -ExecutablePath $exePath -Arguments @("scan", $EqExePath) -WorkingDirectory $serverDir -TimeoutSeconds 40
    Skip-OptionalIfTimedOut -Result $r -TimeoutSeconds 40 -Reason "scan address-resolution pass"
    Assert-ExitCode -Result $r -Expected 0 -Message "scan should exit cleanly"
    Assert-Contains -Text $r.StdOut -Needle "ZoneAddr" -Message "scan output missing ZoneAddr"
    Assert-Contains -Text $r.StdOut -Needle "SpawnHeaderAddr" -Message "scan output missing SpawnHeaderAddr"
    Assert-Contains -Text $r.StdOut -Needle "CharInfo" -Message "scan output missing CharInfo"
    Assert-Contains -Text $r.StdOut -Needle "ItemsAddr" -Message "scan output missing ItemsAddr"
    Assert-Contains -Text $r.StdOut -Needle "TargetAddr" -Message "scan output missing TargetAddr"
    Assert-Contains -Text $r.StdOut -Needle "WorldAddr" -Message "scan output missing WorldAddr"
}

Write-Host ""
Write-Host "EQ-required results: required(pass=$reqPass fail=$reqFail skip=$reqSkip) optional(pass=$optPass fail=$optFail skip=$optSkip)"

if ($reqFail -gt 0 -or $optFail -gt 0) {
    exit 1
}

