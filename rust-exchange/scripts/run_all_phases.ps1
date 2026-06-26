# Master Test Runner: Executes all 6 phases in sequence
# Phases 1-5 must pass before Phase 6 runs.
# Usage: .\run_all_phases.ps1 [-SkipPhase6] [-Phases 1,2,3]

param(
    [switch]$SkipPhase6,
    [int[]]$Phases  # If specified, only run these phases (e.g. -Phases 1,3,5)
)

$ErrorActionPreference = "Stop"

$PhaseScripts = @{
    1 = "$PSScriptRoot\test_error_mapping_complete.ps1"
    2 = "$PSScriptRoot\test_post_failure_health.ps1"
    3 = "$PSScriptRoot\test_restart_after_errors.ps1"
    4 = "$PSScriptRoot\test_batch_progressive.ps1"
    5 = "$PSScriptRoot\test_soak_extended.ps1"
    6 = "$PSScriptRoot\benchmark_http_perf.ps1"
}

$PhaseNames = @{
    1 = "Business Error Mapping Coverage"
    2 = "Post-Failure Service Health"
    3 = "Restart Recovery (WAL Integrity)"
    4 = "Batch Progressive Validation"
    5 = "Extended Soak Testing"
    6 = "HTTP Performance Benchmarks"
}

# Determine which phases to run
$phasesToRun = if ($Phases) { $Phases } else { 1..6 }
if ($SkipPhase6) { $phasesToRun = $phasesToRun | Where-Object { $_ -ne 6 } }

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  Exchange Test Suite - Master Runner" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""
$phaseList = $phasesToRun -join ", "
Write-Host "Phases to run: $phaseList" -ForegroundColor Yellow
Write-Host ""

$PhaseResults = @{}
$AllPassed = $true

foreach ($phaseNum in ($phasesToRun | Sort-Object)) {
    $scriptPath = $PhaseScripts[$phaseNum]
    $phaseName = $PhaseNames[$phaseNum]

    if (-not (Test-Path $scriptPath)) {
        $skipMsg = "[SKIP] Phase $phaseNum : Script not found at $scriptPath"
        Write-Host $skipMsg -ForegroundColor DarkGray
        continue
    }

    Write-Host "----------------------------------------" -ForegroundColor Cyan
    $hdr = "PHASE $phaseNum : $phaseName"
    Write-Host $hdr -ForegroundColor Cyan
    Write-Host "----------------------------------------" -ForegroundColor Cyan
    Write-Host ""

    $startTime = Get-Date

    try {
        & $scriptPath
        $exitCode = $LASTEXITCODE
        $elapsed = (Get-Date) - $startTime

        $PhaseResults[$phaseNum] = @{
            Name     = $phaseName
            Passed   = $exitCode -eq 0
            Elapsed  = $elapsed
        }

        if ($exitCode -ne 0) {
            $AllPassed = $false
            $failMsg = "*** PHASE $phaseNum FAILED ***"
            Write-Host $failMsg -ForegroundColor Red
        } else {
            $secStr = $elapsed.TotalSeconds.ToString("F1")
            $passMsg = "*** PHASE $phaseNum PASSED (" + $secStr + "s) ***"
            Write-Host $passMsg -ForegroundColor Green
        }
    } catch {
        $elapsed = (Get-Date) - $startTime
        $PhaseResults[$phaseNum] = @{
            Name     = $phaseName
            Passed   = $false
            Elapsed  = $elapsed
            Error    = $_.Exception.Message
        }
        $AllPassed = $false
        $errMsg = "*** PHASE $phaseNum ERROR: " + $_.Exception.Message + " ***"
        Write-Host $errMsg -ForegroundColor Red
    }

    # Gate: Phase 6 only runs if 1-5 passed
    if ($phaseNum -lt 6 -and -not $PhaseResults[$phaseNum].Passed) {
        $gateMsg = "Phase $phaseNum failed. Skipping remaining phases."
        Write-Host $gateMsg -ForegroundColor Red
        break
    }

    Write-Host ""
}

# ============================================================
# Final Summary
# ============================================================
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  FINAL SUMMARY" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

$totalPassed = 0
$totalFailed = 0

foreach ($phaseNum in ($PhaseResults.Keys | Sort-Object)) {
    $r = $PhaseResults[$phaseNum]
    $icon = if ($r.Passed) { "PASS" } else { "FAIL" }
    $color = if ($r.Passed) { "Green" } else { "Red" }

    if ($r.Passed) { $totalPassed++ } else { $totalFailed++ }

    $secVal = $r.Elapsed.TotalSeconds.ToString("F1")
    $lineMsg = "  [$icon] Phase $phaseNum : $($r.Name) ($secVal s)"
    Write-Host $lineMsg -ForegroundColor $color

    if ($r.Error) {
        $errDetail = "         Error: " + $r.Error
        Write-Host $errDetail -ForegroundColor Red
    }
}

Write-Host "----------------------------------------" -ForegroundColor Cyan
$summaryMsg = "  Total: $totalPassed passed, $totalFailed failed"
$summaryColor = if ($AllPassed) { "Green" } else { "Red" }
Write-Host $summaryMsg -ForegroundColor $summaryColor
Write-Host "========================================" -ForegroundColor Cyan

exit $(if ($AllPassed) { 0 } else { 1 })
