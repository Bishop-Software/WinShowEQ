Set-StrictMode -Version Latest

function Invoke-TestProcess {
    param(
        [string]$ExecutablePath,
        [string[]]$Arguments,
        [string]$WorkingDirectory,
        [string]$InputText = "",
        [int]$TimeoutSeconds = 20
    )

    if (-not (Test-Path $ExecutablePath)) {
        throw "Executable not found: $ExecutablePath"
    }

    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = $ExecutablePath
    foreach ($arg in $Arguments) {
        [void]$psi.ArgumentList.Add($arg)
    }
    $psi.WorkingDirectory = $WorkingDirectory
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.RedirectStandardInput = $true
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true

    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $psi

    if (-not $process.Start()) {
        throw "Failed to start process: $($psi.FileName)"
    }

    if (-not [string]::IsNullOrEmpty($InputText)) {
        $process.StandardInput.Write($InputText)
    }
    $process.StandardInput.Close()

    $timedOut = -not $process.WaitForExit($TimeoutSeconds * 1000)
    if ($timedOut) {
        try { $process.Kill($true) } catch {}
        $null = $process.WaitForExit()
    }

    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()

    [pscustomobject]@{
        Arguments = ($Arguments -join " ")
        ExitCode = if ($timedOut) { $null } else { $process.ExitCode }
        TimedOut = $timedOut
        StdOut = $stdout
        StdErr = $stderr
    }
}

function Assert-Contains {
    param(
        [string]$Text,
        [string]$Needle,
        [string]$Message
    )

    if (-not $Text.Contains($Needle)) {
        throw "$Message`nExpected to find: $Needle"
    }
}

function Assert-NotContains {
    param(
        [string]$Text,
        [string]$Needle,
        [string]$Message
    )

    if ($Text.Contains($Needle)) {
        throw "$Message`nUnexpectedly found: $Needle"
    }
}

function Assert-Matches {
    param(
        [string]$Text,
        [string]$Pattern,
        [string]$Message
    )

    if (-not ($Text -match $Pattern)) {
        throw "$Message`nExpected to match regex: $Pattern"
    }
}

function Assert-ContainsAny {
    param(
        [string]$Text,
        [string[]]$Needles,
        [string]$Message
    )

    foreach ($needle in $Needles) {
        if ($Text.Contains($needle)) {
            return
        }
    }

    throw "$Message`nExpected one of: $($Needles -join '; ')"
}

function Assert-ExitCode {
    param(
        [object]$Result,
        [int]$Expected,
        [string]$Message
    )

    if ($Result.TimedOut) {
        throw "$Message`nProcess timed out before exit."
    }
    if ($Result.ExitCode -ne $Expected) {
        throw "$Message`nExpected exit code $Expected but got $($Result.ExitCode)."
    }
}

function Build-WinShowEqServer {
    param([string]$RepoRoot)

    Push-Location $RepoRoot
    try {
        cargo build -p winshoweq-server | Out-Host
    } finally {
        Pop-Location
    }
}

function Test-EqRunning {
    $null -ne (Get-Process -Name "eqgame" -ErrorAction SilentlyContinue)
}

function Test-HasText {
    param([AllowNull()][string]$Value)

    -not [string]::IsNullOrWhiteSpace($Value)
}

