param([switch]$Quiet)

# Self-test for bench_compare.ps1.
# Runs the comparator against fixture pairs and asserts the expected exit code
# and verdict. Returns exit 0 on all-pass, 1 on any failure.

$ErrorActionPreference = "Stop"
$here = $PSScriptRoot
$compare = (Resolve-Path (Join-Path $here "..\..\bench_compare.ps1")).Path
$baseline = Join-Path $here "baseline_example.json"

$cases = @(
    @{ name = "current_pass";                 file = "current_pass.json";                 expected_exit = 0; expected_verdict = "PASS" }
    @{ name = "current_within_threshold";     file = "current_within_threshold.json";     expected_exit = 0; expected_verdict = "PASS" }
    @{ name = "current_throughput_regress";   file = "current_throughput_regress.json";   expected_exit = 1; expected_verdict = "FAIL" }
    @{ name = "current_rpo_breach";           file = "current_rpo_breach.json";           expected_exit = 1; expected_verdict = "FAIL" }
)

$results = @()
$failed = 0
foreach ($c in $cases) {
    $current = Join-Path $here $c.file
    $tmpReport = New-TemporaryFile
    & powershell -NoProfile -ExecutionPolicy Bypass -File $compare -Baseline $baseline -Current $current -Output $tmpReport.FullName *> $null
    $code = $LASTEXITCODE
    $verdict = if ($code -eq 0) { "PASS" } else { "FAIL" }
    $ok = ($code -eq $c.expected_exit) -and ($verdict -eq $c.expected_verdict)
    $results += [pscustomobject]@{
        case             = $c.name
        expected_exit    = $c.expected_exit
        actual_exit      = $code
        expected_verdict = $c.expected_verdict
        actual_verdict   = $verdict
        passed           = $ok
        report_excerpt   = (Get-Content $tmpReport.FullName -Raw)
    }
    if (-not $ok) { $failed++ }
    Remove-Item $tmpReport -Force -ErrorAction SilentlyContinue
}

if (-not $Quiet) {
    Write-Host "`n=========================================" -ForegroundColor Cyan
    Write-Host "bench_compare.ps1 self-test"                  -ForegroundColor Cyan
    Write-Host "=========================================" -ForegroundColor Cyan
    foreach ($r in $results) {
        $color = if ($r.passed) { 'Green' } else { 'Red' }
        $tag = if ($r.passed) { 'PASS' } else { 'FAIL' }
        Write-Host ("  [{0}] {1}: expected exit={2}/{3}, got exit={4}/{5}" -f `
            $tag, $r.case, $r.expected_exit, $r.expected_verdict, $r.actual_exit, $r.actual_verdict) -ForegroundColor $color
    }
    Write-Host ("Total: {0}/{1} passed" -f ($results.Count - $failed), $results.Count) -ForegroundColor $(if ($failed -eq 0) {'Green'} else {'Red'})
    Write-Host "=========================================" -ForegroundColor Cyan
}

if ($failed -eq 0) { exit 0 } else { exit 1 }
