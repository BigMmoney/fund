param(
    [string]$OutputDir = "",
    [switch]$SkipBuild,
    [switch]$SkipTest,
    [switch]$SkipE2E,
    [switch]$SkipReplay,
    [switch]$SkipRestart,
    [switch]$SkipBackup
)

# One-shot P0 wrapper: runs Steps 1-6 in order, captures per-step logs, and
# emits an aggregated p0_summary.json. Exit 0 only if every executed step
# exited 0; any non-skipped failure short-circuits to exit 1.
#
# Steps:
#   01_build               — cargo build --release --bin api
#   02_test                — cargo test --workspace
#   03_e2e                 — e2e_trading_test.ps1 (wrapper manages api lifecycle)
#   04_wal_recovery        — test_wal_recovery.ps1 (self-managed lifecycle)
#   05_restart_after_errors — test_restart_after_errors.ps1 (self-managed lifecycle)
#   06_wal_backup_restore  — wal_backup.ps1 + run_wal_restore_drill.ps1
#
# Use -Skip<Step> flags to omit a step (useful for dev iteration).

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\test_lib.ps1"

$rustRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $ts = Get-Date -Format "yyyyMMdd_HHmmss"
    $OutputDir = Join-Path $rustRoot ".." "artifacts" "p0_run_$ts"
}
if (-not [System.IO.Path]::IsPathRooted($OutputDir)) {
    $OutputDir = Join-Path $rustRoot $OutputDir
}
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
$OutputDir = (Resolve-Path $OutputDir).Path

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "P0 Full Run — output: $OutputDir"          -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

$results = New-Object System.Collections.Generic.List[object]

function Record-Step {
    param(
        [string]$Name,
        [int]$ExitCode,
        [double]$ElapsedSeconds,
        [string]$LogPath,
        $ReportPath = $null,
        [string]$Note = ""
    )
    $entry = [ordered]@{
        name             = $Name
        exit_code        = $ExitCode
        elapsed_seconds  = [Math]::Round($ElapsedSeconds, 3)
        log              = $LogPath
        report           = $ReportPath
        note             = $Note
        passed           = ($ExitCode -eq 0)
    }
    $results.Add([pscustomobject]$entry) | Out-Null
    $color = if ($ExitCode -eq 0) { 'Green' } else { 'Red' }
    Write-Host ("  [{0}] {1}  ({2}s)" -f ($(if ($ExitCode -eq 0) {'PASS'} else {'FAIL'})), $Name, [Math]::Round($ElapsedSeconds, 1)) -ForegroundColor $color
}

function Invoke-CargoStep {
    param([string]$Name, [string[]]$Args, [string]$LogPath)
    $started = Get-Date
    & cargo @Args 2>&1 | Tee-Object -FilePath $LogPath | Out-Null
    $code = $LASTEXITCODE
    $elapsed = ((Get-Date) - $started).TotalSeconds
    Record-Step -Name $Name -ExitCode $code -ElapsedSeconds $elapsed -LogPath $LogPath
    return $code
}

function Invoke-PowerShellStep {
    param([string]$Name, [string]$ScriptPath, [string[]]$Args, [string]$LogPath, [string]$ReportPath = $null)
    $started = Get-Date
    $argList = @("-ExecutionPolicy", "Bypass", "-File", $ScriptPath) + $Args
    & powershell @argList *>&1 | Tee-Object -FilePath $LogPath | Out-Null
    $code = $LASTEXITCODE
    $elapsed = ((Get-Date) - $started).TotalSeconds
    Record-Step -Name $Name -ExitCode $code -ElapsedSeconds $elapsed -LogPath $LogPath -ReportPath $ReportPath
    return $code
}

Push-Location $rustRoot
$exitCodes = @()
try {
    # ── 01: build ──
    if (-not $SkipBuild) {
        Write-Host "`n[01] cargo build --release --bin api" -ForegroundColor Yellow
        $exitCodes += Invoke-CargoStep -Name "01_build" -Args @("build","--release","--bin","api","--message-format=short") -LogPath (Join-Path $OutputDir "01_build.log")
    }

    # ── 02: test ──
    if (-not $SkipTest) {
        Write-Host "`n[02] cargo test --workspace" -ForegroundColor Yellow
        $exitCodes += Invoke-CargoStep -Name "02_test" -Args @("test","--workspace") -LogPath (Join-Path $OutputDir "02_test.log")
    }

    # ── 03: E2E (wrapper manages api lifecycle since e2e_trading_test.ps1 expects an already-running server) ──
    if (-not $SkipE2E) {
        Write-Host "`n[03] e2e_trading_test.ps1" -ForegroundColor Yellow
        Stop-ExchangeService
        $serverLog = Join-Path $OutputDir "03_e2e_api_server.log"
        if (-not (Start-ExchangeService -StdoutLog $serverLog -StderrLog "$serverLog.err" -WaitTimeoutSeconds 30)) {
            Record-Step -Name "03_e2e" -ExitCode 99 -ElapsedSeconds 0 -LogPath $serverLog -Note "api startup failed"
            $exitCodes += 99
        } else {
            $logPath = Join-Path $OutputDir "03_e2e.log"
            $started = Get-Date
            & powershell -ExecutionPolicy Bypass -File "$PSScriptRoot\e2e_trading_test.ps1" *>&1 | Tee-Object -FilePath $logPath | Out-Null
            $code = $LASTEXITCODE
            $elapsed = ((Get-Date) - $started).TotalSeconds
            Stop-ExchangeService
            Record-Step -Name "03_e2e" -ExitCode $code -ElapsedSeconds $elapsed -LogPath $logPath
            $exitCodes += $code
        }
    }

    # ── 04: WAL replay recovery (self-managed) ──
    if (-not $SkipReplay) {
        Write-Host "`n[04] test_wal_recovery.ps1" -ForegroundColor Yellow
        $report = Join-Path $OutputDir "04_wal_recovery.json"
        $exitCodes += Invoke-PowerShellStep -Name "04_wal_recovery" -ScriptPath "$PSScriptRoot\test_wal_recovery.ps1" -Args @("-Output",$report) -LogPath (Join-Path $OutputDir "04_wal_recovery.log") -ReportPath $report
    }

    # ── 05: restart-after-errors (self-managed) ──
    if (-not $SkipRestart) {
        Write-Host "`n[05] test_restart_after_errors.ps1" -ForegroundColor Yellow
        $report = Join-Path $OutputDir "05_restart_after_errors.json"
        $exitCodes += Invoke-PowerShellStep -Name "05_restart_after_errors" -ScriptPath "$PSScriptRoot\test_restart_after_errors.ps1" -Args @("-Output",$report) -LogPath (Join-Path $OutputDir "05_restart_after_errors.log") -ReportPath $report
    }

    # ── 06: WAL backup → restore drill ──
    if (-not $SkipBackup) {
        Write-Host "`n[06a] wal_backup.ps1" -ForegroundColor Yellow
        $backupOutDir = Join-Path $OutputDir "wal-backups"
        $backupLog = Join-Path $OutputDir "06_wal_backup.log"
        $started = Get-Date
        & powershell -ExecutionPolicy Bypass -File "$PSScriptRoot\wal_backup.ps1" -OutputDir $backupOutDir *>&1 | Tee-Object -FilePath $backupLog | Out-Null
        $backupCode = $LASTEXITCODE
        $elapsed = ((Get-Date) - $started).TotalSeconds
        Record-Step -Name "06a_wal_backup" -ExitCode $backupCode -ElapsedSeconds $elapsed -LogPath $backupLog
        $exitCodes += $backupCode

        if ($backupCode -eq 0) {
            $latest = Get-ChildItem -Path $backupOutDir -Filter "wal-*.tar.gz" -ErrorAction SilentlyContinue | Sort-Object LastWriteTime -Descending | Select-Object -First 1
            if ($latest) {
                Write-Host "`n[06b] run_wal_restore_drill.ps1 ($($latest.Name))" -ForegroundColor Yellow
                $restoreLog = Join-Path $OutputDir "06_wal_restore.log"
                $started2 = Get-Date
                & powershell -ExecutionPolicy Bypass -File "$PSScriptRoot\run_wal_restore_drill.ps1" -BackupArchive $latest.FullName -CleanRestoreDir *>&1 | Tee-Object -FilePath $restoreLog | Out-Null
                $restoreCode = $LASTEXITCODE
                $elapsed2 = ((Get-Date) - $started2).TotalSeconds
                Record-Step -Name "06b_wal_restore_drill" -ExitCode $restoreCode -ElapsedSeconds $elapsed2 -LogPath $restoreLog
                $exitCodes += $restoreCode
            } else {
                Record-Step -Name "06b_wal_restore_drill" -ExitCode 99 -ElapsedSeconds 0 -LogPath "" -Note "no backup archive produced"
                $exitCodes += 99
            }
        }
    }
} finally {
    Pop-Location
}

# ── Aggregate summary ──
$allPassed = ($results | Where-Object { -not $_.passed } | Measure-Object).Count -eq 0
$summary = [ordered]@{
    generated_at_epoch = [int][double]::Parse((Get-Date -UFormat %s))
    output_dir         = $OutputDir
    steps              = $results
    passed             = $allPassed
}
$summaryPath = Join-Path $OutputDir "p0_summary.json"
$summary | ConvertTo-Json -Depth 6 | Set-Content -Path $summaryPath -Encoding UTF8
Write-Host "`nAggregated summary: $summaryPath" -ForegroundColor Green

Write-Host "`n=========================================" -ForegroundColor Cyan
Write-Host "P0 SUMMARY"                                  -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan
foreach ($r in $results) {
    $color = if ($r.passed) { 'Green' } else { 'Red' }
    Write-Host ("  [{0}] {1}  ({2}s)  exit={3}" -f ($(if ($r.passed) {'PASS'} else {'FAIL'})), $r.name, $r.elapsed_seconds, $r.exit_code) -ForegroundColor $color
}
$passCount = ($results | Where-Object { $_.passed } | Measure-Object).Count
$totalCount = $results.Count
Write-Host ("Passed: {0}/{1}" -f $passCount, $totalCount) -ForegroundColor $(if ($allPassed) { 'Green' } else { 'Red' })
Write-Host "=========================================" -ForegroundColor Cyan

if ($allPassed) { exit 0 } else { exit 1 }
