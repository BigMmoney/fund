# Phase 2: Post-Failure Service Health
# After each business error, send 5-10 normal orders to verify service stays healthy.

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\test_lib.ps1"

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "PHASE 2: Post-Failure Service Health" -ForegroundColor Cyan
Write-Host "========================================`n" -ForegroundColor Cyan

# Ensure service is running (restart to get clean state)
Write-Host "Restarting service for clean state..." -ForegroundColor Yellow
Stop-ExchangeService
Start-Sleep -Milliseconds 500
Start-ExchangeService

# Seed test account with cash
Write-Host "Seeding test account..." -ForegroundColor Yellow
Test-Deposit -Amount 10000000000  # 10B subunits to support many orders
Write-Host "  Cash deposit: OK" -ForegroundColor Green

$phaseResults = @()

function Test-PostFailureHealth {
    param(
        [string]$ScenarioName,
        [scriptblock]$TriggerError,
        [int]$FollowUpOrders = 10
    )
    
    Write-Host "`n--- Testing: $ScenarioName ---" -ForegroundColor White
    
    # Trigger the error
    Write-Host "  Triggering error..." -ForegroundColor Gray
    & $TriggerError
    
    # Send follow-up normal orders (use funded user, buy orders)
    Write-Host "  Sending $FollowUpOrders follow-up orders..." -ForegroundColor Gray
    $successCount = 0
    $failDetails = @()
    
    for ($i = 0; $i -lt $FollowUpOrders; $i++) {
        # Use unique client order IDs to avoid duplicates
        $clientId = "health-followup-${i}-$([guid]::NewGuid().ToString().Substring(0,6))"
        $orderJson = New-OrderJson -Side "buy" -Price (50000 + $i * 100) -Amount 1000 -ClientOrderId $clientId
        $resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent
        
        if ($resp.StatusCode -eq 200) {
            $successCount++
        } else {
            $failDetails += "Order ${i}: HTTP $($resp.StatusCode)"
        }
    }
    
    $passed = $successCount -ge ($FollowUpOrders / 2)  # Allow some failures due to balance limits
    $script:phaseResults += @{
        Scenario    = $ScenarioName
        Passed      = $passed
        SuccessRate = "$successCount/$FollowUpOrders"
        Failures    = $failDetails
    }
    
    if ($passed) {
        Write-Host "  PASS: $successCount/$FollowUpOrders orders succeeded" -ForegroundColor Green
    } else {
        Write-Host "  FAIL: Only $successCount/$FollowUpOrders succeeded" -ForegroundColor Red
        $failDetails | ForEach-Object { Write-Host "    $_" -ForegroundColor Red }
    }
    
    return $passed
}

# ============================================================
# Test 1: After InsufficientFunds
# ============================================================
Test-PostFailureHealth -ScenarioName "After InsufficientFunds" -TriggerError {
    # Use unfunded user to trigger InsufficientFunds
    $unfundedUser = "user-unfunded-$([guid]::NewGuid().ToString().Substring(0,8))"
    $orderJson = New-OrderJson -Side "buy" -Price 50000 -Amount 10000
    $resp = Invoke-ExchangeRequestAs -Path "/submit-order" -BodyJson $orderJson -Subject $unfundedUser -Silent
    Write-Host "  Error response: HTTP $($resp.StatusCode)" -ForegroundColor Gray
} -FollowUpOrders 10

# ============================================================
# Test 2: After MarketNotFound
# ============================================================
Test-PostFailureHealth -ScenarioName "After MarketNotFound" -TriggerError {
    $orderJson = New-OrderJson -MarketId "fake-market" -Side "buy" -Price 100 -Amount 1
    $resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent
    Write-Host "  Error response: HTTP $($resp.StatusCode)" -ForegroundColor Gray
} -FollowUpOrders 10

# ============================================================
# Test 3: After DuplicateOrderId
# ============================================================
Test-PostFailureHealth -ScenarioName "After DuplicateOrderId" -TriggerError {
    $dupId = "dup_health_$([guid]::NewGuid().ToString().Substring(0,8))"
    $orderJson1 = New-OrderJson -Side "buy" -Price 49999 -Amount 1000 -ClientOrderId $dupId
    Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson1 -Silent | Out-Null
    
    $orderJson2 = New-OrderJson -Side "buy" -Price 49998 -Amount 1000 -ClientOrderId $dupId
    $resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson2 -Silent
    Write-Host "  Error response: HTTP $($resp.StatusCode)" -ForegroundColor Gray
} -FollowUpOrders 10

# ============================================================
# Test 4: After QueueFull
# ============================================================
Test-PostFailureHealth -ScenarioName "After QueueFull" -TriggerError {
    Write-Host "  Flooding queue..." -ForegroundColor Gray
    for ($i = 0; $i -lt 60; $i++) {
        $clientId = "flood-health-$i-$([guid]::NewGuid().ToString().Substring(0,4))"
        $orderJson = New-OrderJson -Side "buy" -Price (40000 + $i) -Amount 1000 -ClientOrderId $clientId
        $resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent
        if ($resp.StatusCode -eq 429) {
            Write-Host "  Queue full detected at order $i" -ForegroundColor Gray
            break
        }
    }
    Write-Host "  Waiting 2s for queue to drain..." -ForegroundColor Gray
    Start-Sleep -Seconds 2
} -FollowUpOrders 10

# ============================================================
# Test 5: After KillSwitch (enable, error, disable, recover)
# ============================================================
Test-PostFailureHealth -ScenarioName "After KillSwitch" -TriggerError {
    # Enable kill switch via governance flow
    $ksJson = '{"enabled":true}'
    $respCreate = Invoke-AdminRequest -Path "/admin/kill-switch" -BodyJson $ksJson -Silent
    $actionId = if ($respCreate.ParsedJson -and $respCreate.ParsedJson.approval -and $respCreate.ParsedJson.approval.action_id) {
        $respCreate.ParsedJson.approval.action_id
    } else { $null }
    
    if ($actionId) {
        # Approve with admin2 + admin3
        $approvePath = "/admin/risk/governance/actions/$actionId/approve"
        Invoke-AdminRequest -Path $approvePath -BodyJson '{}' -Subject $Script:AdminSubject2 -Role $Script:AdminRole2 -Silent | Out-Null
        Invoke-AdminRequest -Path $approvePath -BodyJson '{}' -Subject $Script:AdminSubject3 -Role $Script:AdminRole3 -Silent | Out-Null
        Start-Sleep -Milliseconds 500
    }
    
    # Try to submit (should fail)
    $orderJson = New-OrderJson -Side "buy" -Price 49999 -Amount 1000
    $resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent
    Write-Host "  Kill switch error response: HTTP $($resp.StatusCode)" -ForegroundColor Gray
    
    # Disable kill switch
    $ksOffJson = '{"enabled":false}'
    $respCreateOff = Invoke-AdminRequest -Path "/admin/kill-switch" -BodyJson $ksOffJson -Silent
    $offActionId = if ($respCreateOff.ParsedJson -and $respCreateOff.ParsedJson.approval -and $respCreateOff.ParsedJson.approval.action_id) {
        $respCreateOff.ParsedJson.approval.action_id
    } else { $null }
    if ($offActionId) {
        $offApprovePath = "/admin/risk/governance/actions/$offActionId/approve"
        Invoke-AdminRequest -Path $offApprovePath -BodyJson '{}' -Subject $Script:AdminSubject2 -Role $Script:AdminRole2 -Silent | Out-Null
        Invoke-AdminRequest -Path $offApprovePath -BodyJson '{}' -Subject $Script:AdminSubject3 -Role $Script:AdminRole3 -Silent | Out-Null
        Start-Sleep -Milliseconds 500
    }
    Write-Host "  Kill switch disabled" -ForegroundColor Gray
} -FollowUpOrders 10

# ============================================================
# Test 6: Cross-market health (different market unaffected)
# ============================================================
Write-Host "`n--- Cross-Market Health Check ---" -ForegroundColor White
Write-Host "  Testing eth-usdt after btc-usdt errors..." -ForegroundColor Gray

# Trigger error on btc-usdt
$errorOrder = New-OrderJson -MarketId "btc-usdt" -Side "buy" -Price 50000 -Amount 100000
Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $errorOrder -Silent | Out-Null

# Send orders to eth-usdt
$ethSuccess = 0
for ($i = 0; $i -lt 5; $i++) {
    $clientId = "eth-health-${i}-$([guid]::NewGuid().ToString().Substring(0,6))"
    $orderJson = New-OrderJson -MarketId "eth-usdt" -Side "buy" -Price (4000 + $i * 100) -Amount 10000 -ClientOrderId $clientId
    $resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent
    if ($resp.StatusCode -eq 200) { $ethSuccess++ }
}

if ($ethSuccess -ge 2) {
    Write-Host "  PASS: eth-usdt unaffected ($ethSuccess/5 succeeded)" -ForegroundColor Green
} else {
    Write-Host "  FAIL: eth-usdt affected ($ethSuccess/5 succeeded)" -ForegroundColor Red
}

# ============================================================
# Summary
# ============================================================
Write-Host "`n========================================" -ForegroundColor Cyan
Write-Host "PHASE 2 SUMMARY" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

$passed = ($phaseResults | Where-Object { $_.Passed }).Count
$total = $phaseResults.Count

Write-Host "Scenarios passed: $passed/$total" -ForegroundColor $(if ($passed -eq $total) { "Green" } else { "Red" })

$phaseResults | ForEach-Object {
    $icon = if ($_.Passed) { "PASS" } else { "FAIL" }
    $color = if ($_.Passed) { "Green" } else { "Red" }
    Write-Host "  [$icon] $($_.Scenario) - $($_.SuccessRate)" -ForegroundColor $color
    if ($_.Failures.Count -gt 0) {
        $_.Failures | ForEach-Object { Write-Host "    $_" -ForegroundColor Red }
    }
}

Write-Host "========================================`n" -ForegroundColor Cyan

# Exit with appropriate code
if ($passed -eq $total) {
    exit 0
} else {
    exit 1
}
