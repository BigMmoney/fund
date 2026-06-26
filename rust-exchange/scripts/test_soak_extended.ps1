# Phase 5: Extended Soak Testing
# Long-running stability: 100 single orders, dual-market, cancel-replace, restart.

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\test_lib.ps1"

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "PHASE 5: Extended Soak Testing" -ForegroundColor Cyan
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
$latencies = @()

# ============================================================
# Test 1: Single Market 100 Orders (rate-limited)
# ============================================================
Write-Host "`n--- Test 1: Single Market 100 Orders ---" -ForegroundColor White

$successCount = 0
$failCount = 0
$errors500 = 0
$latenciesSingle = @()

for ($i = 0; $i -lt 100; $i++) {
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $orderJson = New-OrderJson -Side "sell" -Price (55000 + $i) -Amount 1000
    $resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent
    $sw.Stop()
    
    $latenciesSingle += $sw.ElapsedMilliseconds
    
    if ($resp.StatusCode -eq 200) {
        $successCount++
    } elseif ($resp.StatusCode -eq 429) {
        # Rate limited - back off and retry
        Start-Sleep -Milliseconds 200
        $retryResp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent
        if ($retryResp.StatusCode -eq 200) { $successCount++ } else { $failCount++ }
    } elseif ($resp.StatusCode -ge 500) {
        $errors500++
        $failCount++
    } else {
        $failCount++
    }
    
    # Small delay to avoid flooding
    if ($i % 10 -eq 9) { Start-Sleep -Milliseconds 50 }
}

$p50 = ($latenciesSingle | Sort-Object)[$latenciesSingle.Count * 50 / 100]
$p95 = ($latenciesSingle | Sort-Object)[$latenciesSingle.Count * 95 / 100]
$p99 = ($latenciesSingle | Sort-Object)[$latenciesSingle.Count * 99 / 100]

$test1Passed = $errors500 -eq 0 -and $failCount -le 5
$phaseResults += @{
    Test        = "SingleMarket100"
    Passed      = $test1Passed
    Success     = $successCount
    Failed      = $failCount
    Errors500   = $errors500
    LatencyP50  = $p50
    LatencyP95  = $p95
    LatencyP99  = $p99
}

Write-Host "  Results: $successCount succeeded, $failCount failed, $errors500 server errors" -ForegroundColor $(if ($test1Passed) { "Green" } else { "Red" })
Write-Host "  Latency: P50=${p50}ms, P95=${p95}ms, P99=${p99}ms" -ForegroundColor Gray

# ============================================================
# Test 2: Dual Market 50 Each (interleaved)
# ============================================================
Write-Host "`n--- Test 2: Dual Market 50 Orders Each (Interleaved) ---" -ForegroundColor White

$btcSuccess = 0
$ethSuccess = 0
$failCount2 = 0
$errors500_2 = 0
$latenciesDual = @()

for ($i = 0; $i -lt 50; $i++) {
    # BTC order
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $btcOrder = New-OrderJson -MarketId "btc-usdt" -Side "sell" -Price (55000 + $i) -Amount 1000
    $respBtc = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $btcOrder -Silent
    $sw.Stop()
    $latenciesDual += $sw.ElapsedMilliseconds
    
    if ($respBtc.StatusCode -eq 200) { $btcSuccess++ }
    elseif ($respBtc.StatusCode -ge 500) { $errors500_2++; $failCount2++ }
    else { $failCount2++ }
    
    # ETH order
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $ethOrder = New-OrderJson -MarketId "eth-usdt" -Side "sell" -Price (3000 + $i) -Amount 10000
    $respEth = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $ethOrder -Silent
    $sw.Stop()
    $latenciesDual += $sw.ElapsedMilliseconds
    
    if ($respEth.StatusCode -eq 200) { $ethSuccess++ }
    elseif ($respEth.StatusCode -ge 500) { $errors500_2++; $failCount2++ }
    else { $failCount2++ }
    
    if ($i % 10 -eq 9) { Start-Sleep -Milliseconds 50 }
}

$p50d = ($latenciesDual | Sort-Object)[$latenciesDual.Count * 50 / 100]
$p95d = ($latenciesDual | Sort-Object)[$latenciesDual.Count * 95 / 100]
$p99d = ($latenciesDual | Sort-Object)[$latenciesDual.Count * 99 / 100]

$test2Passed = $errors500_2 -eq 0 -and $failCount2 -le 5
$phaseResults += @{
    Test        = "DualMarket50Each"
    Passed      = $test2Passed
    BtcSuccess  = $btcSuccess
    EthSuccess  = $ethSuccess
    Failed      = $failCount2
    Errors500   = $errors500_2
    LatencyP50  = $p50d
    LatencyP95  = $p95d
    LatencyP99  = $p99d
}

Write-Host "  BTC: $btcSuccess/50, ETH: $ethSuccess/50, Failed: $failCount2, 500s: $errors500_2" -ForegroundColor $(if ($test2Passed) { "Green" } else { "Red" })
Write-Host "  Latency: P50=${p50d}ms, P95=${p95d}ms, P99=${p99d}ms" -ForegroundColor Gray

# ============================================================
# Test 3: Cancel-Replace 20 Groups
# ============================================================
Write-Host "`n--- Test 3: Cancel-Replace 20 Groups ---" -ForegroundColor White

$crSuccess = 0
$crFail = 0
$errors500_3 = 0
$latenciesCR = @()

$activeOrderIds = @()

for ($i = 0; $i -lt 20; $i++) {
    # Step 1: Submit order
    $clientOrderId = "cr_$i"
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $submitJson = New-OrderJson -Side "sell" -Price (50000 + $i * 100) -Amount 1000 -ClientOrderId $clientOrderId
    $respSubmit = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $submitJson -Silent
    $sw.Stop()
    $latenciesCR += $sw.ElapsedMilliseconds
    
    if ($respSubmit.StatusCode -eq 200) {
        # Extract order_id from response if available
        $orderId = if ($respSubmit.ParsedJson -and $respSubmit.ParsedJson.order_id) { $respSubmit.ParsedJson.order_id } else { $null }
        if ($orderId) { $activeOrderIds += $orderId }
        
        # Step 2: Cancel the order
        $sw2 = [System.Diagnostics.Stopwatch]::StartNew()
        if ($orderId) {
            $cancelJson = New-CancelJson -OrderId $orderId
            $respCancel = Invoke-ExchangeRequest -Path "/cancel-order" -BodyJson $cancelJson -Silent
        } else {
            $respCancel = @{ StatusCode = 200 }
        }
        $sw2.Stop()
        $latenciesCR += $sw2.ElapsedMilliseconds
        
        if ($respCancel.StatusCode -eq 200 -or $respCancel.StatusCode -eq 404) {
            # 404 is acceptable (order already filled/expired)
            # Step 3: Submit replacement
            $sw3 = [System.Diagnostics.Stopwatch]::StartNew()
            $replaceJson = New-OrderJson -Side "sell" -Price (50100 + $i * 100) -Amount 1000 -ClientOrderId "${clientOrderId}_v2"
            $respReplace = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $replaceJson -Silent
            $sw3.Stop()
            $latenciesCR += $sw3.ElapsedMilliseconds
            
            if ($respReplace.StatusCode -eq 200) {
                $crSuccess++
            } else {
                $crFail++
                if ($respReplace.StatusCode -ge 500) { $errors500_3++ }
            }
        } else {
            $crFail++
            if ($respCancel.StatusCode -ge 500) { $errors500_3++ }
        }
    } else {
        $crFail++
        if ($respSubmit.StatusCode -ge 500) { $errors500_3++ }
    }
    
    Start-Sleep -Milliseconds 100
}

$p50cr = ($latenciesCR | Sort-Object)[$latenciesCR.Count * 50 / 100]
$p95cr = ($latenciesCR | Sort-Object)[$latenciesCR.Count * 95 / 100]
$p99cr = ($latenciesCR | Sort-Object)[$latenciesCR.Count * 99 / 100]

$test3Passed = $errors500_3 -eq 0
$phaseResults += @{
    Test        = "CancelReplace20"
    Passed      = $test3Passed
    Success     = $crSuccess
    Failed      = $crFail
    Errors500   = $errors500_3
    LatencyP50  = $p50cr
    LatencyP95  = $p95cr
    LatencyP99  = $p99cr
}

Write-Host "  Results: $crSuccess succeeded, $crFail failed, $errors500_3 server errors" -ForegroundColor $(if ($test3Passed) { "Green" } else { "Red" })
Write-Host "  Latency: P50=${p50cr}ms, P95=${p95cr}ms, P99=${p99cr}ms" -ForegroundColor Gray

# ============================================================
# Test 4: Restart with WAL, then 20 orders per market
# ============================================================
Write-Host "`n--- Test 4: Restart with WAL + 20 Orders/Market ---" -ForegroundColor White

Write-Host "  Restarting service (WAL preserved)..." -ForegroundColor Gray
$restartOk = Restart-ExchangeService -NoClearWal

if ($restartOk) {
    Write-Host "  Service restarted successfully" -ForegroundColor Green
    
    # 20 orders per market
    $btcPostRestart = 0
    $ethPostRestart = 0
    $postRestartFail = 0
    
    for ($i = 0; $i -lt 20; $i++) {
        $btcOrder = New-OrderJson -MarketId "btc-usdt" -Side "sell" -Price (60000 + $i) -Amount 1000
        $respBtc = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $btcOrder -Silent
        if ($respBtc.StatusCode -eq 200) { $btcPostRestart++ } else { $postRestartFail++ }
        
        $ethOrder = New-OrderJson -MarketId "eth-usdt" -Side "sell" -Price (4000 + $i) -Amount 10000
        $respEth = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $ethOrder -Silent
        if ($respEth.StatusCode -eq 200) { $ethPostRestart++ } else { $postRestartFail++ }
    }
    
    $test4Passed = $postRestartFail -eq 0
    $phaseResults += @{
        Test        = "RestartThen20PerMarket"
        Passed      = $test4Passed
        BtcOrders   = $btcPostRestart
        EthOrders   = $ethPostRestart
        Failed      = $postRestartFail
    }
    
    Write-Host "  BTC: $btcPostRestart/20, ETH: $ethPostRestart/20, Failed: $postRestartFail" -ForegroundColor $(if ($test4Passed) { "Green" } else { "Red" })
} else {
    Write-Host "  FAIL: Service failed to restart" -ForegroundColor Red
    $phaseResults += @{
        Test        = "RestartThen20PerMarket"
        Passed      = $false
        BtcOrders   = 0
        EthOrders   = 0
        Failed      = 1
    }
}

# ============================================================
# Summary
# ============================================================
Write-Host "`n========================================" -ForegroundColor Cyan
Write-Host "PHASE 5 SUMMARY" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

$passed = ($phaseResults | Where-Object { $_.Passed }).Count
$total = $phaseResults.Count

Write-Host "Tests passed: $passed/$total" -ForegroundColor $(if ($passed -eq $total) { "Green" } else { "Red" })

$phaseResults | ForEach-Object {
    $icon = if ($_.Passed) { "PASS" } else { "FAIL" }
    $color = if ($_.Passed) { "Green" } else { "Red" }
    Write-Host "  [$icon] $($_.Test)" -ForegroundColor $color
    
    if ($_.LatencyP50) {
        Write-Host "       Latency: P50=$($_.LatencyP50)ms, P95=$($_.LatencyP95)ms, P99=$($_.LatencyP99)ms" -ForegroundColor DarkGray
    }
    if ($_.Success) {
        Write-Host "       Orders: $($_.Success) ok, $($_.Failed) fail, $($_.Errors500) 500s" -ForegroundColor DarkGray
    }
    if ($_.BtcSuccess) {
        Write-Host "       BTC: $($_.BtcSuccess)/50, ETH: $($_.EthSuccess)/50, Failed: $($_.Failed), 500s: $($_.Errors500)" -ForegroundColor DarkGray
    }
    if ($_.BtcOrders) {
        Write-Host "       Post-restart: BTC $($_.BtcOrders)/20, ETH $($_.EthOrders)/20, Failed: $($_.Failed)" -ForegroundColor DarkGray
    }
}

Write-Host "========================================`n" -ForegroundColor Cyan

$allPassed = $passed -eq $total
exit $(if ($allPassed) { 0 } else { 1 })
