# Phase 3: Restart Recovery (WAL Integrity)
# Trigger business errors, restart WITHOUT clearing WAL, verify clean recovery.

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\test_lib.ps1"

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "PHASE 3: Restart Recovery (WAL Integrity)" -ForegroundColor Cyan
Write-Host "========================================`n" -ForegroundColor Cyan

$phaseResults = @()

function Test-RestartAfterError {
    param(
        [string]$ScenarioName,
        [scriptblock]$TriggerError,
        [int]$PostRestartOrders = 5
    )
    
    Write-Host "`n--- Testing: $ScenarioName ---" -ForegroundColor White
    
    # Step 1: Trigger the error
    Write-Host "  Step 1: Triggering business error..." -ForegroundColor Gray
    & $TriggerError
    
    # Step 2: Record WAL state (count lines)
    $walDir = Join-Path $PSScriptRoot "..\data"
    $walFiles = Get-ChildItem $walDir -Filter "*.wal*" -ErrorAction SilentlyContinue
    $walLineCounts = @{}
    foreach ($f in $walFiles) {
        $lines = (Get-Content $f.FullName -ErrorAction SilentlyContinue).Count
        $walLineCounts[$f.Name] = $lines
    }
    Write-Host "  Step 2: WAL files recorded ($($walFiles.Count) files)" -ForegroundColor Gray
    
    # Step 3: Restart WITHOUT clearing WAL
    Write-Host "  Step 3: Restarting service (WAL preserved)..." -ForegroundColor Gray
    $restartOk = Restart-ExchangeService -NoClearWal
    
    if (-not $restartOk) {
        Write-Host "  FAIL: Service failed to restart" -ForegroundColor Red
        $phaseResults += @{ Scenario = $ScenarioName; Passed = $false; Detail = "Restart failed" }
        return $false
    }
    
    # Step 4: Send normal orders after restart
    Write-Host "  Step 4: Sending $PostRestartOrders orders after restart..." -ForegroundColor Gray
    $successCount = 0
    for ($i = 0; $i -lt $PostRestartOrders; $i++) {
        $orderJson = New-OrderJson -Side "sell" -Price (90000 + $i) -Amount 1000
        $resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent
        if ($resp.StatusCode -eq 200) {
            $successCount++
        }
    }
    
    $passed = $successCount -eq $PostRestartOrders
    $phaseResults += @{
        Scenario = $ScenarioName
        Passed   = $passed
        Detail   = "$successCount/$PostRestartOrders orders succeeded after restart"
    }
    
    if ($passed) {
        Write-Host "  PASS: $successCount/$PostRestartOrders orders succeeded after restart" -ForegroundColor Green
    } else {
        Write-Host "  FAIL: Only $successCount/$PostRestartOrders succeeded" -ForegroundColor Red
    }
    
    return $passed
}

# ============================================================
# Test 1: Restart after InsufficientFunds
# ============================================================
Test-RestartAfterError -ScenarioName "After InsufficientFunds" -TriggerError {
    $orderJson = New-OrderJson -Side "buy" -Price 50000 -Amount 10
    $resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent
    Write-Host "  Error: HTTP $($resp.StatusCode)" -ForegroundColor Gray
} -PostRestartOrders 5

# ============================================================
# Test 2: Restart after QueueFull (partial orders in WAL)
# ============================================================
Test-RestartAfterError -ScenarioName "After QueueFull" -TriggerError {
    Write-Host "  Flooding queue..." -ForegroundColor Gray
    for ($i = 0; $i -lt 60; $i++) {
        $orderJson = New-OrderJson -Side "sell" -Price (80000 + $i) -Amount 1000
        $resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent
        if ($resp.StatusCode -eq 429) {
            Write-Host "  Queue full at order $i" -ForegroundColor Gray
            break
        }
    }
    Start-Sleep -Seconds 1
} -PostRestartOrders 5

# ============================================================
# Test 3: Restart after mixed success/failure batch
# ============================================================
Test-RestartAfterError -ScenarioName "After Mixed Success/Failure" -TriggerError {
    # Some valid orders
    for ($i = 0; $i -lt 3; $i++) {
        $orderJson = New-OrderJson -Side "sell" -Price (95000 + $i) -Amount 1000
        Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent | Out-Null
    }
    
    # Some invalid orders
    $badOrder = New-OrderJson -MarketId "fake-market" -Side "buy" -Price 100 -Amount 1
    Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $badOrder -Silent | Out-Null
    
    $dupId = "dup_mixed_$([guid]::NewGuid().ToString().Substring(0,8))"
    $orderJson1 = New-OrderJson -Side "sell" -Price 99999 -Amount 1000 -ClientOrderId $dupId
    Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson1 -Silent | Out-Null
    $orderJson2 = New-OrderJson -Side "sell" -Price 99998 -Amount 1000 -ClientOrderId $dupId
    Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson2 -Silent | Out-Null
    
    Write-Host "  Mixed batch sent (3 valid + 2 invalid)" -ForegroundColor Gray
} -PostRestartOrders 5

# ============================================================
# Test 4: Restart after cancel on non-existent order
# ============================================================
Test-RestartAfterError -ScenarioName "After CancelNonExistent" -TriggerError {
    $cancelJson = New-CancelJson -OrderId "ghost-order-xyz"
    $resp = Invoke-ExchangeRequest -Path "/cancel-order" -BodyJson $cancelJson -Silent
    Write-Host "  Error: HTTP $($resp.StatusCode)" -ForegroundColor Gray
} -PostRestartOrders 5

# ============================================================
# Summary
# ============================================================
Write-Host "`n========================================" -ForegroundColor Cyan
Write-Host "PHASE 3 SUMMARY" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

$passed = ($phaseResults | Where-Object { $_.Passed }).Count
$total = $phaseResults.Count

Write-Host "Scenarios passed: $passed/$total" -ForegroundColor $(if ($passed -eq $total) { "Green" } else { "Red" })

$phaseResults | ForEach-Object {
    $icon = if ($_.Passed) { "PASS" } else { "FAIL" }
    $color = if ($_.Passed) { "Green" } else { "Red" }
    Write-Host "  [$icon] $($_.Scenario) - $($_.Detail)" -ForegroundColor $color
}

Write-Host "========================================`n" -ForegroundColor Cyan

$allPassed = $passed -eq $total
exit $(if ($allPassed) { 0 } else { 1 })
