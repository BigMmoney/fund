# Phase 1: Business Error Mapping Coverage
# Tests that every business error returns correct HTTP status (not 500)
# with stable JSON body and trace_id.

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\test_lib.ps1"

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "PHASE 1: Business Error Mapping Coverage" -ForegroundColor Cyan
Write-Host "========================================`n" -ForegroundColor Cyan

# Ensure service is running (restart for clean state)
Write-Host "Restarting service for clean state..." -ForegroundColor Yellow
Stop-ExchangeService
Start-Sleep -Milliseconds 500

# Clear persisted state files
$dataDir = Resolve-Path (Join-Path $PSScriptRoot "..\data")
Write-Host "  Clearing persisted state..." -ForegroundColor Gray
$filesToRemove = @(
    "matching.snapshot.jsonl",
    "sequencer.wal.jsonl",
    "ledger.wal.jsonl",
    "trade_journal.wal.jsonl",
    "trade_settlement.wal.jsonl",
    "transfers.wal.jsonl",
    "withdrawals.wal.jsonl",
    "stop_orders.wal.jsonl",
    "position.cost.events.jsonl",
    "position.cost.state.jsonl",
    "replay_guard.jsonl"
)
foreach ($fileName in $filesToRemove) {
    $filePath = Join-Path $dataDir $fileName
    if (Test-Path $filePath) {
        Remove-Item -Path $filePath -Force -ErrorAction SilentlyContinue
    }
}

Start-ExchangeService

# Seed test account with funds
Write-Host "`nSeeding test account..." -ForegroundColor Yellow
$depositOk = Test-Deposit -UserId $Script:Subject -Amount 10000000 -OpId "seed-phase1-$([guid]::NewGuid().ToString().Substring(0,8))"
if ($depositOk) {
    Write-Host "  Cash deposit: OK" -ForegroundColor Green
} else {
    Write-Host "  Cash deposit: FAILED (may already exist)" -ForegroundColor Yellow
}
$posDepositOk = Test-PositionDeposit -UserId $Script:Subject -MarketId "btc-usdt" -Outcome 0 -Amount 10000 -OpId "seed-pos-phase1-$([guid]::NewGuid().ToString().Substring(0,8))"
if ($posDepositOk) {
    Write-Host "  Position deposit: OK" -ForegroundColor Green
} else {
    Write-Host "  Position deposit: FAILED (may already exist)" -ForegroundColor Yellow
}

# ============================================================
# Scenario 1: Insufficient Funds (buy with zero balance)
# ============================================================
Write-Host "`n--- Scenario 1: Insufficient Funds ---" -ForegroundColor White
# Use an unfunded user for this test
$origSubject = $Script:Subject
$Script:Subject = "user-unfunded-$(Get-Random)"
$orderJson = New-OrderJson -MarketId "btc-usdt" -Side "buy" -Price 50000 -Amount 10
$resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent
$Script:Subject = $origSubject

$traceId = if ($resp.ParsedJson -and $resp.ParsedJson.trace_id) { $resp.ParsedJson.trace_id } else { "" }
$msg = if ($resp.ParsedJson -and $resp.ParsedJson.message) { $resp.ParsedJson.message } elseif ($resp.ParsedJson -and $resp.ParsedJson.code) { $resp.ParsedJson.code } else { $resp.Body.Substring(0, [Math]::Min(100, $resp.Body.Length)) }

Log-Result -Phase "Phase1" -Scenario "InsufficientFunds" -StatusCode $resp.StatusCode -ExpectedStatus "400" -HasValidJson $resp.HasValidJson -Message $msg -TraceId $traceId

# ============================================================
# Scenario 2: Market Not Found
# ============================================================
Write-Host "`n--- Scenario 2: Market Not Found ---" -ForegroundColor White
$orderJson = New-OrderJson -MarketId "fake-market-xyz" -Side "buy" -Price 100 -Amount 1
$resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent

$traceId = if ($resp.ParsedJson -and $resp.ParsedJson.trace_id) { $resp.ParsedJson.trace_id } else { "" }
$msg = if ($resp.ParsedJson -and $resp.ParsedJson.message) { $resp.ParsedJson.message } elseif ($resp.ParsedJson -and $resp.ParsedJson.code) { $resp.ParsedJson.code } else { $resp.Body.Substring(0, [Math]::Min(100, $resp.Body.Length)) }

# Accept both 404 (ideal) and 400 (valid client error) - key requirement is NOT 500
$expectedStatus = if ($resp.StatusCode -eq 404 -or $resp.StatusCode -eq 400) { $resp.StatusCode.ToString() } else { "404" }
Log-Result -Phase "Phase1" -Scenario "MarketNotFound" -StatusCode $resp.StatusCode -ExpectedStatus $expectedStatus -HasValidJson $resp.HasValidJson -Message $msg -TraceId $traceId

# ============================================================
# Scenario 3: Duplicate client_order_id
# ============================================================
Write-Host "`n--- Scenario 3: Duplicate client_order_id ---" -ForegroundColor White
# First order (should succeed or fail for other reasons)
$orderJson1 = New-OrderJson -MarketId "btc-usdt" -Side "sell" -Price 99000 -Amount 100 -ClientOrderId $dupClientOrderId
$resp1 = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson1 -Silent

# Second order with same client_order_id (should get 409)
$orderJson2 = New-OrderJson -MarketId "btc-usdt" -Side "sell" -Price 99000 -Amount 100 -ClientOrderId $dupClientOrderId
$resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson2 -Silent

$traceId = if ($resp.ParsedJson -and $resp.ParsedJson.trace_id) { $resp.ParsedJson.trace_id } else { "" }
$msg = if ($resp.ParsedJson -and $resp.ParsedJson.message) { $resp.ParsedJson.message } elseif ($resp.ParsedJson -and $resp.ParsedJson.code) { $resp.ParsedJson.code } else { $resp.Body.Substring(0, [Math]::Min(100, $resp.Body.Length)) }

Log-Result -Phase "Phase1" -Scenario "DuplicateClientId" -StatusCode $resp.StatusCode -ExpectedStatus "409" -HasValidJson $resp.HasValidJson -Message $msg -TraceId $traceId
# ============================================================
# Scenario 4: Order Not Found (cancel non-existent order)
# ============================================================
Write-Host "`n--- Scenario 4: Order Not Found (cancel) ---" -ForegroundColor White
$cancelJson = New-CancelJson -MarketId "btc-usdt" -OrderId "nonexistent-order-id-12345"
$resp = Invoke-ExchangeRequest -Path "/cancel-order" -BodyJson $cancelJson -Silent

$traceId = if ($resp.ParsedJson -and $resp.ParsedJson.trace_id) { $resp.ParsedJson.trace_id } else { "" }
$msg = if ($resp.ParsedJson -and $resp.ParsedJson.message) { $resp.ParsedJson.message } elseif ($resp.ParsedJson -and $resp.ParsedJson.code) { $resp.ParsedJson.code } else { $resp.Body.Substring(0, [Math]::Min(100, $resp.Body.Length)) }

Log-Result -Phase "Phase1" -Scenario "OrderNotFound" -StatusCode $resp.StatusCode -ExpectedStatus "404" -HasValidJson $resp.HasValidJson -Message $msg -TraceId $traceId

# ============================================================
# ============================================================
# Scenario 5: Kill Switch Active
# ============================================================
Write-Host "`n--- Scenario 5: Kill Switch Active ---" -ForegroundColor White

# Step 1: Create kill switch governance action (admin1)
$killSwitchJson = '{"enabled":true}'
$respCreate = Invoke-AdminRequest -Path "/admin/kill-switch" -BodyJson $killSwitchJson -Silent
Write-Host "  Kill switch create response: $($respCreate.StatusCode)" -ForegroundColor Gray

$actionId = if ($respCreate.ParsedJson -and $respCreate.ParsedJson.approval -and $respCreate.ParsedJson.approval.action_id) {
    $respCreate.ParsedJson.approval.action_id
} else { $null }

if ($actionId) {
    # Step 2: First approval with admin2
    $approvePath = "/admin/risk/governance/actions/$actionId/approve"
    $respApprove1 = Invoke-AdminRequest -Path $approvePath -BodyJson '{}' -Subject $Script:AdminSubject2 -Role $Script:AdminRole2 -Silent
    Write-Host "  Kill switch approve (admin2) response: $($respApprove1.StatusCode)" -ForegroundColor Gray

    # Step 3: Second approval with admin3 (required_approvals = 2, this triggers execution)
    $respApprove2 = Invoke-AdminRequest -Path $approvePath -BodyJson '{}' -Subject $Script:AdminSubject3 -Role $Script:AdminRole3 -Silent
    Write-Host "  Kill switch approve (admin3) response: $($respApprove2.StatusCode)" -ForegroundColor Gray

    # Give it a moment to propagate
    Start-Sleep -Milliseconds 500

    # Step 4: Verify kill switch is active via health endpoint
    $healthResp = Invoke-WebRequest -Uri "http://127.0.0.1:3030/health" -UseBasicParsing
    $healthJson = $healthResp.Content | ConvertFrom-Json
    Write-Host "  Health kill_switch: $($healthJson.kill_switch)" -ForegroundColor Gray

    # Step 5: Try to submit order (should be rejected with 403)
    $orderJson = New-OrderJson -MarketId "btc-usdt" -Side "sell" -Price 99999 -Amount 1000
    $resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent

    $traceId = if ($resp.ParsedJson -and $resp.ParsedJson.trace_id) { $resp.ParsedJson.trace_id } else { "" }
    $msg = if ($resp.ParsedJson -and $resp.ParsedJson.message) { $resp.ParsedJson.message } elseif ($resp.ParsedJson -and $resp.ParsedJson.code) { $resp.ParsedJson.code } else { $resp.Body.Substring(0, [Math]::Min(100, $resp.Body.Length)) }

    Log-Result -Phase "Phase1" -Scenario "KillSwitchActive" -StatusCode $resp.StatusCode -ExpectedStatus "503" -HasValidJson $resp.HasValidJson -Message $msg -TraceId $traceId $traceId

    # Step 6: Disable kill switch (admin1 creates, admin2 + admin3 approve)
    $killSwitchOffJson = '{"enabled":false}'
    $respCreateOff = Invoke-AdminRequest -Path "/admin/kill-switch" -BodyJson $killSwitchOffJson -Silent
    $offActionId = if ($respCreateOff.ParsedJson -and $respCreateOff.ParsedJson.approval -and $respCreateOff.ParsedJson.approval.action_id) {
        $respCreateOff.ParsedJson.approval.action_id
    } else { $null }
    if ($offActionId) {
        $offApprovePath = "/admin/risk/governance/actions/$offActionId/approve"
        $respApproveOff1 = Invoke-AdminRequest -Path $offApprovePath -BodyJson '{}' -Subject $Script:AdminSubject2 -Role $Script:AdminRole2 -Silent
        $respApproveOff2 = Invoke-AdminRequest -Path $offApprovePath -BodyJson '{}' -Subject $Script:AdminSubject3 -Role $Script:AdminRole3 -Silent
        Write-Host "  Kill switch disable approvals: $($respApproveOff1.StatusCode)/$($respApproveOff2.StatusCode)" -ForegroundColor Gray
        Start-Sleep -Milliseconds 500
    }
} else {
    Write-Host "  WARNING: No action_id returned from kill switch creation" -ForegroundColor Red
    Log-Result -Phase "Phase1" -Scenario "KillSwitchActive" -StatusCode 0 -ExpectedStatus "403" -HasValidJson $false -Message "Failed to create governance action"
}

# ============================================================
# Scenario 6: Queue Full (flood with orders)
# ============================================================
Write-Host "`n--- Scenario 6: Queue Full ---" -ForegroundColor White
$queueFullDetected = $false
$floodCount = 5000

# Use rapid sequential requests without delays
for ($i = 0; $i -lt $floodCount; $i++) {
    # Use varying prices to avoid duplicate rejection; high prices to avoid fills
    $orderJson = New-OrderJson -MarketId "btc-usdt" -Side "sell" -Price (90000 + ($i % 100)) -Amount 100
    $resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent
    
    if ($resp.StatusCode -eq 429) {
        $queueFullDetected = $true
        $traceId = if ($resp.ParsedJson -and $resp.ParsedJson.trace_id) { $resp.ParsedJson.trace_id } else { "" }
        $msg = if ($resp.ParsedJson -and $resp.ParsedJson.message) { $resp.ParsedJson.message } elseif ($resp.ParsedJson -and $resp.ParsedJson.code) { $resp.ParsedJson.code } else { "Queue full detected at order $i" }
        
        Log-Result -Phase "Phase1" -Scenario "QueueFull" -StatusCode $resp.StatusCode -ExpectedStatus "429" -HasValidJson $resp.HasValidJson -Message $msg -TraceId $traceId
        break
    }
    
    # Progress indicator every 500 orders
    if (($i + 1) % 500 -eq 0) {
        Write-Host "  Flooded $($i + 1)/$floodCount orders..." -ForegroundColor DarkGray
    }
}

if (-not $queueFullDetected) {
    Log-Result -Phase "Phase1" -Scenario "QueueFull" -StatusCode 200 -ExpectedStatus "429" -HasValidJson $false -Message "Queue did not fill after $floodCount orders"
}

# Wait for queue to drain (may take longer with 5000 orders)
Start-Sleep -Seconds 5

# ============================================================
# Scenario 7: Risk Rejected (large order violating limits)
# ============================================================
Write-Host "`n--- Scenario 7: Risk Rejected ---" -ForegroundColor White
# Submit an order that should violate risk limits (extremely large amount)
$orderJson = New-OrderJson -MarketId "btc-usdt" -Side "buy" -Price 50000 -Amount 1000000
$resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent

$traceId = if ($resp.ParsedJson -and $resp.ParsedJson.trace_id) { $resp.ParsedJson.trace_id } else { "" }
$msg = if ($resp.ParsedJson -and $resp.ParsedJson.message) { $resp.ParsedJson.message } elseif ($resp.ParsedJson -and $resp.ParsedJson.code) { $resp.ParsedJson.code } else { $resp.Body.Substring(0, [Math]::Min(100, $resp.Body.Length)) }

# Could be 400 (bad request) or 403 (risk rejected) - both are valid non-500
$expectedStatus = if ($resp.StatusCode -eq 400 -or $resp.StatusCode -eq 403) { $resp.StatusCode.ToString() } else { "400" }
Log-Result -Phase "Phase1" -Scenario "RiskRejected" -StatusCode $resp.StatusCode -ExpectedStatus $expectedStatus -HasValidJson $resp.HasValidJson -Message $msg -TraceId $traceId

# ============================================================
# Scenario 8: Position Insufficient (close position with no holdings)
# ============================================================
Write-Host "`n--- Scenario 8: Position Insufficient ---" -ForegroundColor White
# Try to sell something we don't own
# Use amount exceeding seeded position (10000) to trigger position insufficient
# Use high sell price to avoid crossing resting buy orders from prior scenarios
$orderJson = New-OrderJson -MarketId "btc-usdt" -Side "sell" -Price 999999 -Amount 999999
$resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent

$traceId = if ($resp.ParsedJson -and $resp.ParsedJson.trace_id) { $resp.ParsedJson.trace_id } else { "" }
$msg = if ($resp.ParsedJson -and $resp.ParsedJson.message) { $resp.ParsedJson.message } elseif ($resp.ParsedJson -and $resp.ParsedJson.code) { $resp.ParsedJson.code } else { $resp.Body.Substring(0, [Math]::Min(100, $resp.Body.Length)) }

Log-Result -Phase "Phase1" -Scenario "PositionInsufficient" -StatusCode $resp.StatusCode -ExpectedStatus "400" -HasValidJson $resp.HasValidJson -Message $msg -TraceId $traceId

# ============================================================
# Summary
# ============================================================
$allPassed = Show-TestSummary

if ($allPassed) {
    Write-Host "PHASE 1 PASSED - All error mappings correct!" -ForegroundColor Green
} else {
    Write-Host "PHASE 1 FAILED - Review error mappings above." -ForegroundColor Red
}

exit $(if ($allPassed) { 0 } else { 1 })
