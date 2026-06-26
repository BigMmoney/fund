# Phase 4: Batch Scenario Re-validation
# Progressive batch sizes: 5, 10, 20. Verify 200 or 429 (never 500).

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\test_lib.ps1"

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "PHASE 4: Batch Progressive Validation" -ForegroundColor Cyan
Write-Host "========================================`n" -ForegroundColor Cyan

# Ensure service is running
Write-Host "Checking service health..." -ForegroundColor Yellow
try {
    $healthResp = Invoke-ExchangeRequest -Method "GET" -Path "/health" -Silent
    if ($healthResp.StatusCode -ne 200) {
        Start-ExchangeService
    }
} catch {
    Start-ExchangeService
}

$phaseResults = @()

function Test-BatchSize {
    param(
        [int]$BatchSize,
        [string]$Endpoint = "/submit-batch"
    )
    
    Write-Host "`n--- Batch Size: $BatchSize ---" -ForegroundColor White
    
    # Build batch orders
    $orders = @()
    for ($i = 0; $i -lt $BatchSize; $i++) {
        $orders += @(New-OrderJson -Side "sell" -Price (65000 + $i) -Amount 1000)
    }
    $batchJson = "[$($orders -join ',')]"
    
    # Submit batch
    $resp = Invoke-ExchangeRequest -Path $Endpoint -BodyJson $batchJson -Silent
    
    $acceptable = $resp.StatusCode -eq 200 -or $resp.StatusCode -eq 429
    $is500 = $resp.StatusCode -ge 500
    
    $detail = "HTTP $($resp.StatusCode)"
    if ($resp.HasValidJson -and $resp.ParsedJson) {
        if ($resp.ParsedJson.results) {
            $accepted = ($resp.ParsedJson.results | Where-Object { $_.status -eq "accepted" -or $_.status -eq "ok" }).Count
            $rejected = ($resp.ParsedJson.results | Where-Object { $_.status -ne "accepted" -and $_.status -ne "ok" }).Count
            $detail += " | Accepted: $accepted, Rejected: $rejected"
        } elseif ($resp.ParsedJson.message) {
            $detail += " | $($resp.ParsedJson.message)"
        }
    }
    
    $phaseResults += @{
        BatchSize  = $BatchSize
        StatusCode = $resp.StatusCode
        Acceptable = $acceptable
        Is500      = $is500
        ValidJson  = $resp.HasValidJson
        Detail     = $detail
    }
    
    if ($is500) {
        Write-Host "  FAIL: HTTP $($resp.StatusCode) - Server error!" -ForegroundColor Red
    } elseif ($acceptable) {
        $color = if ($resp.StatusCode -eq 200) { "Green" } else { "Yellow" }
        Write-Host "  PASS: HTTP $($resp.StatusCode) - $detail" -ForegroundColor $color
    } else {
        Write-Host "  WARN: HTTP $($resp.StatusCode) - Unexpected status" -ForegroundColor Yellow
    }
    
    return $acceptable
}

# ============================================================
# Test 1: Batch 5
# ============================================================
Test-BatchSize -BatchSize 5

# ============================================================
# Test 2: Batch 10
# ============================================================
Test-BatchSize -BatchSize 10

# ============================================================
# Test 3: Batch 20
# ============================================================
Test-BatchSize -BatchSize 20

# ============================================================
# Test 4: Post-batch service health
# ============================================================
Write-Host "`n--- Post-Batch Health Check ---" -ForegroundColor White
Write-Host "  Sending 10 normal orders after batches..." -ForegroundColor Gray

$healthOk = Test-ServiceHealth -OrderCount 10
$phaseResults += @{
    BatchSize  = "PostBatchHealth"
    StatusCode = $(if ($healthOk) { 200 } else { 500 })
    Acceptable = $healthOk
    Is500      = $false
    ValidJson  = $true
    Detail     = $(if ($healthOk) { "Service healthy after batches" } else { "Service degraded after batches" })
}

if ($healthOk) {
    Write-Host "  PASS: Service healthy after batch operations" -ForegroundColor Green
} else {
    Write-Host "  FAIL: Service degraded after batch operations" -ForegroundColor Red
}

# ============================================================
# Test 5: Restart after batches (WAL preserved)
# ============================================================
Write-Host "`n--- Restart After Batches (WAL Preserved) ---" -ForegroundColor White
Write-Host "  Restarting service..." -ForegroundColor Gray
$restartOk = Restart-ExchangeService -NoClearWal

if ($restartOk) {
    Write-Host "  Service restarted successfully" -ForegroundColor Green
    
    # Send orders after restart
    $postRestartOk = Test-ServiceHealth -OrderCount 5
    $phaseResults += @{
        BatchSize  = "PostRestartHealth"
        StatusCode = $(if ($postRestartOk) { 200 } else { 500 })
        Acceptable = $postRestartOk
        Is500      = $false
        ValidJson  = $true
        Detail     = $(if ($postRestartOk) { "Healthy after restart" } else { "Unhealthy after restart" })
    }
    
    if ($postRestartOk) {
        Write-Host "  PASS: Service healthy after restart" -ForegroundColor Green
    } else {
        Write-Host "  FAIL: Service unhealthy after restart" -ForegroundColor Red
    }
} else {
    Write-Host "  FAIL: Service failed to restart" -ForegroundColor Red
    $phaseResults += @{
        BatchSize  = "PostRestartHealth"
        StatusCode = 500
        Acceptable = $false
        Is500      = $true
        ValidJson  = $false
        Detail     = "Restart failed"
    }
}

# ============================================================
# Summary
# ============================================================
Write-Host "`n========================================" -ForegroundColor Cyan
Write-Host "PHASE 4 SUMMARY" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

$passed = ($phaseResults | Where-Object { $_.Acceptable }).Count
$total = $phaseResults.Count
$has500 = ($phaseResults | Where-Object { $_.Is500 }).Count

Write-Host "Tests passed: $passed/$total" -ForegroundColor $(if ($has500 -eq 0 -and $passed -eq $total) { "Green" } else { "Red" })
if ($has500 -gt 0) {
    Write-Host "WARNING: $has500 server error(s) detected!" -ForegroundColor Red
}

$phaseResults | ForEach-Object {
    $icon = if ($_.Acceptable) { "PASS" } else { "FAIL" }
    $color = if ($_.Acceptable) { "Green" } else { "Red" }
    Write-Host "  [$icon] Batch $($_.BatchSize) -> HTTP $($_.StatusCode) | $($_.Detail)" -ForegroundColor $color
}

Write-Host "========================================`n" -ForegroundColor Cyan

$allPassed = $passed -eq $total -and $has500 -eq 0
exit $(if ($allPassed) { 0 } else { 1 })
