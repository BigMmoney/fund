param(
    [Parameter(Mandatory = $true)]
    [string]$Baseline,
    [Parameter(Mandatory = $true)]
    [string]$Current,
    [string]$Output = "",
    [double]$ThresholdPct = -1
)

# Benchmark baseline comparator.
#
# Reads a baseline JSON (with per-metric value + direction) and a current
# benchmark JSON (with per-metric raw values) and emits a regression report.
#
# Schemas:
#   baseline.json:
#     {
#       "schema_version": 1,
#       "regression_threshold_pct": 30,
#       "scenarios": {
#         "<scenario_name>": {
#           "metrics": {
#             "<metric_name>": {
#               "value": <baseline_value>,
#               "direction": "higher_is_better" | "lower_is_better",
#               "absolute_max": <optional, hard cap regardless of threshold>,
#               "absolute_min": <optional, hard floor regardless of threshold>
#             }
#           }
#         }
#       }
#     }
#
#   current.json:
#     {
#       "schema_version": 1,
#       "scenarios": {
#         "<scenario_name>": {
#           "metrics": {
#             "<metric_name>": <number>
#           }
#         }
#       }
#     }
#
# Exit codes:
#   0 — no regression
#   1 — at least one metric regressed beyond threshold (or absolute bound)
#   2 — schema/file error

$ErrorActionPreference = "Stop"

if (-not (Test-Path $Baseline)) {
    Write-Host "baseline file not found: $Baseline" -ForegroundColor Red
    exit 2
}
if (-not (Test-Path $Current)) {
    Write-Host "current file not found: $Current" -ForegroundColor Red
    exit 2
}

$baselineDoc = Get-Content $Baseline -Raw | ConvertFrom-Json
$currentDoc  = Get-Content $Current  -Raw | ConvertFrom-Json

if ($baselineDoc.schema_version -ne 1) {
    Write-Host "baseline schema_version unsupported: $($baselineDoc.schema_version)" -ForegroundColor Red
    exit 2
}
if ($currentDoc.schema_version -ne 1) {
    Write-Host "current schema_version unsupported: $($currentDoc.schema_version)" -ForegroundColor Red
    exit 2
}

if ($ThresholdPct -lt 0) {
    if ($baselineDoc.PSObject.Properties.Name -contains 'regression_threshold_pct' -and $baselineDoc.regression_threshold_pct) {
        $ThresholdPct = [double]$baselineDoc.regression_threshold_pct
    } else {
        $ThresholdPct = 30.0
    }
}

function Get-Metric {
    param($Doc, [string]$Scenario, [string]$Metric)
    if (-not ($Doc.scenarios.PSObject.Properties.Name -contains $Scenario)) { return $null }
    $sc = $Doc.scenarios.$Scenario
    if (-not ($sc.PSObject.Properties.Name -contains 'metrics')) { return $null }
    $metrics = $sc.metrics
    if (-not ($metrics.PSObject.Properties.Name -contains $Metric)) { return $null }
    return $metrics.$Metric
}

$results = New-Object System.Collections.Generic.List[object]

foreach ($scProp in $baselineDoc.scenarios.PSObject.Properties) {
    $scenarioName = $scProp.Name
    $scenarioBaseline = $scProp.Value
    if (-not ($scenarioBaseline.PSObject.Properties.Name -contains 'metrics')) { continue }

    foreach ($mProp in $scenarioBaseline.metrics.PSObject.Properties) {
        $metricName = $mProp.Name
        $baselineEntry = $mProp.Value

        $direction = if ($baselineEntry.PSObject.Properties.Name -contains 'direction') { $baselineEntry.direction } else { 'higher_is_better' }
        $baselineValue = [double]$baselineEntry.value
        $absMax = if ($baselineEntry.PSObject.Properties.Name -contains 'absolute_max' -and $null -ne $baselineEntry.absolute_max) { [double]$baselineEntry.absolute_max } else { $null }
        $absMin = if ($baselineEntry.PSObject.Properties.Name -contains 'absolute_min' -and $null -ne $baselineEntry.absolute_min) { [double]$baselineEntry.absolute_min } else { $null }

        $rawCurrent = Get-Metric -Doc $currentDoc -Scenario $scenarioName -Metric $metricName
        if ($null -eq $rawCurrent) {
            $results.Add([pscustomobject][ordered]@{
                scenario   = $scenarioName
                metric     = $metricName
                baseline   = $baselineValue
                current    = $null
                direction  = $direction
                passed     = $false
                reason     = "metric missing in current report"
            }) | Out-Null
            continue
        }
        $currentValue = [double]$rawCurrent

        $passed = $true
        $reason = ""
        $minAllowed = $null
        $maxAllowed = $null

        switch ($direction) {
            'higher_is_better' {
                $minAllowed = [Math]::Round($baselineValue * (1.0 - $ThresholdPct / 100.0), 6)
                if ($currentValue -lt $minAllowed) {
                    $passed = $false
                    $reason = ("regression: current={0} < min_allowed={1} (baseline={2}, threshold={3}%)" -f $currentValue, $minAllowed, $baselineValue, $ThresholdPct)
                }
            }
            'lower_is_better' {
                $maxAllowed = [Math]::Round($baselineValue * (1.0 + $ThresholdPct / 100.0), 6)
                if ($currentValue -gt $maxAllowed) {
                    $passed = $false
                    $reason = ("regression: current={0} > max_allowed={1} (baseline={2}, threshold={3}%)" -f $currentValue, $maxAllowed, $baselineValue, $ThresholdPct)
                }
            }
            default {
                $passed = $false
                $reason = "unknown direction '$direction'"
            }
        }

        if ($passed -and $null -ne $absMax -and $currentValue -gt $absMax) {
            $passed = $false
            $reason = ("absolute_max breach: current={0} > absolute_max={1}" -f $currentValue, $absMax)
        }
        if ($passed -and $null -ne $absMin -and $currentValue -lt $absMin) {
            $passed = $false
            $reason = ("absolute_min breach: current={0} < absolute_min={1}" -f $currentValue, $absMin)
        }

        $results.Add([pscustomobject][ordered]@{
            scenario     = $scenarioName
            metric       = $metricName
            baseline     = $baselineValue
            current      = $currentValue
            direction    = $direction
            min_allowed  = $minAllowed
            max_allowed  = $maxAllowed
            absolute_max = $absMax
            absolute_min = $absMin
            passed       = $passed
            reason       = $reason
        }) | Out-Null
    }
}

$regressions = @($results | Where-Object { -not $_.passed })
$allPassed = $regressions.Count -eq 0

$report = [ordered]@{
    schema_version           = 1
    evaluated_at_utc         = (Get-Date).ToUniversalTime().ToString("o")
    baseline_path            = (Resolve-Path $Baseline).Path
    current_path             = (Resolve-Path $Current).Path
    regression_threshold_pct = $ThresholdPct
    metrics_evaluated        = $results.Count
    regressions_count        = $regressions.Count
    results                  = $results
    passed                   = $allPassed
}
$rendered = $report | ConvertTo-Json -Depth 8

if ($Output) {
    $outPath = if ([System.IO.Path]::IsPathRooted($Output)) { $Output } else { Join-Path (Get-Location) $Output }
    $outDir = Split-Path -Parent $outPath
    if ($outDir) { New-Item -ItemType Directory -Path $outDir -Force | Out-Null }
    Set-Content -Path $outPath -Value $rendered -Encoding UTF8
    Write-Host "Report written to $outPath" -ForegroundColor Green
}

Write-Host "`n=========================================" -ForegroundColor Cyan
Write-Host "BENCH COMPARE — threshold=${ThresholdPct}%" -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan
foreach ($r in $results) {
    $color = if ($r.passed) { 'Green' } else { 'Red' }
    $status = if ($r.passed) { 'PASS' } else { 'FAIL' }
    Write-Host ("  [{0}] {1}.{2}  baseline={3}  current={4}" -f $status, $r.scenario, $r.metric, $r.baseline, $r.current) -ForegroundColor $color
    if (-not $r.passed) { Write-Host ("       reason: {0}" -f $r.reason) -ForegroundColor Red }
}
Write-Host ("Metrics evaluated: {0}  Regressions: {1}" -f $report.metrics_evaluated, $report.regressions_count) -ForegroundColor $(if ($allPassed) {'Green'} else {'Red'})
Write-Host "=========================================" -ForegroundColor Cyan

if ($allPassed) { exit 0 } else { exit 1 }
