# Advanced Performance & Complex Market Behavior Test Suite
# Tests: P50/P55/P95/P99/P99.9 latency, full pipeline, complex market scenarios
# Usage: .\scripts\advanced_performance_test.ps1

$ConcurrencyLevels = @(1, 2, 5, 10, 20)
$OrdersPerLevel = 200
$SoakDurationMin = 10
$SkipSoak = $false
$SkipComplexMarket = $false

$ErrorActionPreference = "Stop"

# Import test library
. "$PSScriptRoot\test_lib.ps1"

# ============================================================
# Utility Functions
# ============================================================

function Get-Percentile {
    param([double[]]$Values, [double]$Percentile)
    if ($Values.Count -eq 0) { return 0 }
    $sorted = $Values | Sort-Object
    $index = [math]::Floor(($sorted.Count - 1) * $Percentile)
    return $sorted[$index]
}

function Measure-RequestLatency {
    param(
        [string]$Method = "GET",
        [string]$Path,
        [string]$BodyJson = "",
        [string]$Subject = $Script:Subject,
        [string]$Role = $Script:Role
    )
    
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        if ($Method -eq "GET") {
            $resp = Invoke-UserRequest -Method GET -Path $Path -Silent
        } else {
            $resp = Invoke-UserRequest -Method POST -Path $Path -BodyJson $BodyJson -Silent
        }
        $sw.Stop()
        return @{
            LatencyMs = $sw.Elapsed.TotalMilliseconds
            StatusCode = $resp.StatusCode
            Success = ($resp.StatusCode -ge 200 -and $resp.StatusCode -lt 300)
        }
    } catch {
        $sw.Stop()
        return @{
            LatencyMs = $sw.Elapsed.TotalMilliseconds
            StatusCode = 0
            Success = $false
            Error = $_.Exception.Message
        }
    }
}

function Start-ConcurrentWorkers {
    param(
        [int]$Concurrency,
        [int]$TotalOrders,
        [hashtable]$OrderTemplate,
        [string]$TestName
    )
    
    $latencies = @()
    $successCount = 0
    $failCount = 0
    $fillCount = 0
    
    Write-Host "  Running $TestName (concurrency=$Concurrency, orders=$TotalOrders)..." -ForegroundColor Cyan
    
    $jobs = @()
    $ordersPerWorker = [math]::Ceiling($TotalOrders / $Concurrency)
    
    for ($w = 0; $w -lt $Concurrency; $w++) {
        $workerId = $w
        $job = Start-Job -ScriptBlock {
            param($workerId, $ordersPerWorker, $baseUrl, $secret, $subject, $role, $sessionId, $orderTemplate)
            
            $results = @()
            for ($i = 0; $i -lt $ordersPerWorker; $i++) {
                $orderId = "perf-${workerId}-${i}-$(Get-Random)"
                $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
                $requestId = [guid]::NewGuid().ToString()
                
                # Build order JSON
                $orderJson = @"
{"market_id":"$($orderTemplate.MarketId)","outcome":$($orderTemplate.Outcome),"side":"$($orderTemplate.Side)","price":$($orderTemplate.Price),"amount":$($orderTemplate.Amount),"order_id":"$orderId","client_order_id":"$orderId"}
"@
                
                # HMAC signature
                $payload = "POST`n/order`n`n$subject`n$role`n$sessionId`n$timestamp`n$requestId"
                $hmac = New-Object System.Security.Cryptography.HMACSHA256
                $hmac.Key = [System.Text.Encoding]::UTF8.GetBytes($secret)
                $signature = [BitConverter]::ToString($hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($payload))).Replace("-", "").ToLowerInvariant()
                
                $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($orderJson)
                $bodyHash = [BitConverter]::ToString((New-Object System.Security.Cryptography.SHA256Managed).ComputeHash($bodyBytes)).Replace("-", "").ToLowerInvariant()
                
                $headers = @{
                    "Content-Type" = "application/json"
                    "x-request-id" = $requestId
                    "x-internal-auth-subject" = $subject
                    "x-internal-auth-role" = $role
                    "x-internal-auth-session-id" = $sessionId
                    "x-internal-auth-timestamp" = $timestamp
                    "x-internal-auth-signature" = $signature
                    "x-internal-auth-body-sha256" = $bodyHash
                }
                
                $sw = [System.Diagnostics.Stopwatch]::StartNew()
                try {
                    $resp = Invoke-WebRequest -Uri "$baseUrl/order" -Method POST -Headers $headers -Body $orderJson -UseBasicParsing -TimeoutSec 30
                    $sw.Stop()
                    $results += @{
                        LatencyMs = $sw.Elapsed.TotalMilliseconds
                        StatusCode = $resp.StatusCode
                        IsFill = ($resp.StatusCode -eq 200)
                    }
                } catch {
                    $sw.Stop()
                    $results += @{
                        LatencyMs = $sw.Elapsed.TotalMilliseconds
                        StatusCode = 0
                        IsFill = $false
                    }
                }
                
                Start-Sleep -Milliseconds 10  # Small delay between orders
            }
            return $results
        } -ArgumentList $workerId, $ordersPerWorker, $Script:ExchangeBaseUrl, $Script:Secret, $subject, $role, $sessionId, $orderTemplate
        
        $jobs += $job
    }
    
    # Wait for all jobs to complete
    $jobs | Wait-Job | Out-Null
    $allResults = $jobs | Receive-Job
    $jobs | Remove-Job
    
    # Calculate statistics
    $latencies = @($allResults | ForEach-Object { $_.LatencyMs })
    $successCount = ($allResults | Where-Object { $_.StatusCode -ge 200 -and $_.StatusCode -lt 300 }).Count
    $failCount = ($allResults | Where-Object { $_.StatusCode -ge 400 -or $_.StatusCode -eq 0 }).Count
    $fillCount = ($allResults | Where-Object { $_.IsFill }).Count
    
    return @{
        Latencies = $latencies
        SuccessCount = $successCount
        FailCount = $failCount
        FillCount = $fillCount
        TotalOrders = $allResults.Count
    }
}

function Format-LatencyReport {
    param(
        [string]$TestName,
        [double[]]$Latencies,
        [int]$SuccessCount,
        [int]$FailCount,
        [int]$FillCount,
        [int]$TotalOrders
    )
    
    if ($Latencies.Count -eq 0) {
        Write-Host "  [WARN] No latency data collected for $TestName" -ForegroundColor Yellow
        return
    }
    
    $p50 = Get-Percentile -Values $Latencies -Percentile 0.50
    $p55 = Get-Percentile -Values $Latencies -Percentile 0.55
    $p95 = Get-Percentile -Values $Latencies -Percentile 0.95
    $p99 = Get-Percentile -Values $Latencies -Percentile 0.99
    $p999 = Get-Percentile -Values $Latencies -Percentile 0.999
    $min = ($Latencies | Measure-Object -Minimum).Minimum
    $max = ($Latencies | Measure-Object -Maximum).Maximum
    $avg = ($Latencies | Measure-Object -Average).Average
    
    Write-Host "  === $TestName ===" -ForegroundColor Green
    Write-Host "  Orders: $TotalOrders | Success: $SuccessCount | Failed: $FailCount | Fills: $FillCount" -ForegroundColor White
    Write-Host "  P50: $([math]::Round($p50, 2))ms | P55: $([math]::Round($p55, 2))ms | P95: $([math]::Round($p95, 2))ms | P99: $([math]::Round($p99, 2))ms | P99.9: $([math]::Round($p999, 2))ms" -ForegroundColor White
    Write-Host "  Min: $([math]::Round($min, 2))ms | Avg: $([math]::Round($avg, 2))ms | Max: $([math]::Round($max, 2))ms" -ForegroundColor White
    Write-Host ""
}

# ============================================================
# Test Phases
# ============================================================

Write-Host "========================================" -ForegroundColor Magenta
Write-Host "Advanced Performance & Complex Market Test Suite" -ForegroundColor Magenta
Write-Host "========================================" -ForegroundColor Magenta
Write-Host ""

# Phase 0: Service startup
Write-Host "[Phase 0] Service startup & cleanup..." -ForegroundColor Cyan
Stop-ExchangeService
Start-Sleep -Milliseconds 1000

# Clear WAL files
$walDir = Join-Path $PSScriptRoot "..\data"
if (Test-Path $walDir) {
    Get-ChildItem $walDir -Filter "*.wal*" | Remove-Item -Force -ErrorAction SilentlyContinue
    Get-ChildItem $walDir -Filter "*.jsonl" | Remove-Item -Force -ErrorAction SilentlyContinue
}

if (-not (Start-ExchangeService -NoClearWal)) {
    Write-Host "Service startup failed!" -ForegroundColor Red
    exit 1
}

Start-Sleep -Milliseconds 2000

# Phase 1: Baseline latency measurement
Write-Host "[Phase 1] Baseline Latency Measurement (Sequential)..." -ForegroundColor Cyan
$baselineLatencies = @()
$baselineTests = @(
    @{ Name = "HealthCheck"; Method = "GET"; Path = "/health" },
    @{ Name = "MarketsList"; Method = "GET"; Path = "/markets" },
    @{ Name = "BalanceQuery"; Method = "GET"; Path = "/balance" },
    @{ Name = "PositionQuery"; Method = "GET"; Path = "/positions" }
)

foreach ($test in $baselineTests) {
    $testLatencies = @()
    for ($i = 0; $i -lt 50; $i++) {
        $result = Measure-RequestLatency -Method $test.Method -Path $test.Path
        $testLatencies += $result.LatencyMs
        Start-Sleep -Milliseconds 20
    }
    Format-LatencyReport -TestName $test.Name -Latencies $testLatencies -SuccessCount ($testLatencies.Count) -FailCount 0 -FillCount 0 -TotalOrders $testLatencies.Count
    $baselineLatencies += $testLatencies
}

# Phase 2: Single-threaded order submission latency
Write-Host "[Phase 2] Single-Threaded Order Latency..." -ForegroundColor Cyan
$orderLatencies = @()
$successCount = 0
$failCount = 0

# Seed accounts first
Test-CashDeposit -UserId $Script:Subject -Amount 100000 -OpId "seed-perf-1" | Out-Null
Test-PositionDeposit -UserId $Script:Subject -MarketId "btc-usdt" -Outcome 0 -Amount 10000 -OpId "seed-pos-perf-1" | Out-Null

for ($i = 0; $i -lt 100; $i++) {
    $orderId = "seq-order-$i-$(Get-Random)"
    $side = if ($i % 2 -eq 0) { "buy" } else { "sell" }
    $price = if ($side -eq "buy") { 49900 + ($i % 10) } else { 50100 - ($i % 10) }
    
    $orderJson = @"
{"market_id":"btc-usdt","outcome":0,"side":"$side","price":$price,"amount":10,"order_id":"$orderId","client_order_id":"$orderId"}
"@
    
    $result = Measure-RequestLatency -Method POST -Path "/order" -BodyJson $orderJson
    $orderLatencies += $result.LatencyMs
    if ($result.Success) { $successCount++ } else { $failCount++ }
    Start-Sleep -Milliseconds 10
}

Format-LatencyReport -TestName "SequentialOrders" -Latencies $orderLatencies -SuccessCount $successCount -FailCount $failCount -FillCount 0 -TotalOrders 100

# Phase 3: Concurrent order submission (P50/P55/P95/P99/P99.9)
Write-Host "[Phase 3] Concurrent Order Submission (P50/P55/P95/P99/P99.9)..." -ForegroundColor Cyan

foreach ($conc in $ConcurrencyLevels) {
    $orderTemplate = @{
        MarketId = "btc-usdt"
        Outcome = 0
        Side = "buy"
        Price = 49900
        Amount = 10
    }
    
    $result = Start-ConcurrentWorkers -Concurrency $conc -TotalOrders $OrdersPerLevel -OrderTemplate $orderTemplate -TestName "Concurrent-$conc"
    
    Format-LatencyReport -TestName "Concurrent-$conc" -Latencies $result.Latencies -SuccessCount $result.SuccessCount -FailCount $result.FailCount -FillCount $result.FillCount -TotalOrders $result.TotalOrders
}

# Phase 4: Full Pipeline Test (Order -> Matching -> Settlement)
Write-Host "[Phase 4] Full Pipeline Test..." -ForegroundColor Cyan

# Reset state
Stop-ExchangeService
Start-Sleep -Milliseconds 1000
Start-ExchangeService -NoClearWal
Start-Sleep -Milliseconds 2000

# Setup accounts
Test-CashDeposit -UserId "pipeline-user-1" -Amount 100000 -OpId "pipe-seed-1" | Out-Null
Test-PositionDeposit -UserId "pipeline-user-1" -MarketId "eth-usdt" -Outcome 0 -Amount 5000 -OpId "pipe-pos-1" | Out-Null
Test-CashDeposit -UserId "pipeline-user-2" -Amount 100000 -OpId "pipe-seed-2" | Out-Null
Test-PositionDeposit -UserId "pipeline-user-2" -MarketId "eth-usdt" -Outcome 0 -Amount 5000 -OpId "pipe-pos-2" | Out-Null

$pipelineLatencies = @()
$matchCount = 0

# Place limit orders that will match
for ($i = 0; $i -lt 20; $i++) {
    $orderId1 = "pipe-sell-$i"
    $orderId2 = "pipe-buy-$i"
    
    # Sell order
    $sellJson = @"
{"market_id":"eth-usdt","outcome":0,"side":"sell","price":1800,"amount":10,"order_id":"$orderId1","client_order_id":"$orderId1"}
"@
    $result1 = Measure-RequestLatency -Method POST -Path "/order" -BodyJson $sellJson -Subject "pipeline-user-1" -Role "user"
    $pipelineLatencies += $result1.LatencyMs
    
    Start-Sleep -Milliseconds 50
    
    # Buy order (should match)
    $buyJson = @"
{"market_id":"eth-usdt","outcome":0,"side":"buy","price":1800,"amount":10,"order_id":"$orderId2","client_order_id":"$orderId2"}
"@
    $result2 = Measure-RequestLatency -Method POST -Path "/order" -BodyJson $buyJson -Subject "pipeline-user-2" -Role "user"
    $pipelineLatencies += $result2.LatencyMs
    if ($result2.Success) { $matchCount++ }
    
    Start-Sleep -Milliseconds 100
}

Format-LatencyReport -TestName "FullPipeline" -Latencies $pipelineLatencies -SuccessCount ($pipelineLatencies.Count) -FailCount 0 -FillCount $matchCount -TotalOrders $pipelineLatencies.Count

# Phase 5: Complex Market Simulation
if (-not $SkipComplexMarket) {
    Write-Host "[Phase 5] Complex Market Simulation..." -ForegroundColor Cyan
    
    # Reset for complex test
    Stop-ExchangeService
    Start-Sleep -Milliseconds 1000
    Start-ExchangeService -NoClearWal
    Start-Sleep -Milliseconds 2000
    
    # Setup multiple markets
    $markets = @("btc-usdt", "eth-usdt", "sol-usdt")
    foreach ($market in $markets) {
        Test-CashDeposit -UserId "complex-trader" -Amount 500000 -OpId "complex-seed-$market" | Out-Null
        Test-PositionDeposit -UserId "complex-trader" -MarketId $market -Outcome 0 -Amount 10000 -OpId "complex-pos-$market" | Out-Null
    }
    
    $complexLatencies = @()
    
    # Simulate: 1) High volatility 2) Liquidity drain 3) Recovery
    Write-Host "  Scenario 1: High Volatility (rapid price changes)" -ForegroundColor Yellow
    for ($i = 0; $i -lt 50; $i++) {
        $price = 50000 + (Get-Random -Minimum -5000 -Maximum 5000)
        $side = if ($i % 2 -eq 0) { "buy" } else { "sell" }
        $orderId = "vol-order-$i"
        
        $orderJson = @"
{"market_id":"btc-usdt","outcome":0,"side":"$side","price":$price,"amount":5,"order_id":"$orderId","client_order_id":"$orderId"}
"@
        $result = Measure-RequestLatency -Method POST -Path "/order" -BodyJson $orderJson -Subject "complex-trader" -Role "user"
        $complexLatencies += $result.LatencyMs
        Start-Sleep -Milliseconds 20
    }
    
    Write-Host "  Scenario 2: Multi-Market Activity" -ForegroundColor Yellow
    foreach ($market in $markets) {
        for ($i = 0; $i -lt 20; $i++) {
            $price = if ($market -eq "btc-usdt") { 50000 } elseif ($market -eq "eth-usdt") { 1800 } else { 100 }
            $side = if ($i % 2 -eq 0) { "buy" } else { "sell" }
            $orderId = "multi-$market-$i"
            
            $orderJson = @"
{"market_id":"$market","outcome":0,"side":"$side","price":$price,"amount":10,"order_id":"$orderId","client_order_id":"$orderId"}
"@
            $result = Measure-RequestLatency -Method POST -Path "/order" -BodyJson $orderJson -Subject "complex-trader" -Role "user"
            $complexLatencies += $result.LatencyMs
            Start-Sleep -Milliseconds 15
        }
    }
    
    Write-Host "  Scenario 3: Order Book Depth Test (multiple price levels)" -ForegroundColor Yellow
    for ($i = 0; $i -lt 30; $i++) {
        $price = 50000 + ($i * 100)
        $orderId = "depth-order-$i"
        
        $orderJson = @"
{"market_id":"btc-usdt","outcome":0,"side":"sell","price":$price,"amount":1,"order_id":"$orderId","client_order_id":"$orderId"}
"@
        $result = Measure-RequestLatency -Method POST -Path "/order" -BodyJson $orderJson -Subject "complex-trader" -Role "user"
        $complexLatencies += $result.LatencyMs
        Start-Sleep -Milliseconds 10
    }
    
    Format-LatencyReport -TestName "ComplexMarket" -Latencies $complexLatencies -SuccessCount ($complexLatencies.Count) -FailCount 0 -FillCount 0 -TotalOrders $complexLatencies.Count
}

# Phase 6: Soak Test (if not skipped)
if (-not $SkipSoak) {
    Write-Host "[Phase 6] Soak Test (${SoakDurationMin} minutes)..." -ForegroundColor Cyan
    
    $soakLatencies = @()
    $soakSuccess = 0
    $soakFail = 0
    $startTime = Get-Date
    $duration = [TimeSpan]::FromMinutes($SoakDurationMin)
    
    $i = 0
    while ((Get-Date) - $startTime -lt $duration) {
        $orderId = "soak-order-$i"
        $side = if ($i % 2 -eq 0) { "buy" } else { "sell" }
        $price = if ($side -eq "buy") { 49900 } else { 50100 }
        
        $orderJson = @"
{"market_id":"btc-usdt","outcome":0,"side":"$side","price":$price,"amount":1,"order_id":"$orderId","client_order_id":"$orderId"}
"@
        $result = Measure-RequestLatency -Method POST -Path "/order" -BodyJson $orderJson -Subject "soak-user" -Role "user"
        $soakLatencies += $result.LatencyMs
        if ($result.Success) { $soakSuccess++ } else { $soakFail++ }
        
        $i++
        Start-Sleep -Milliseconds 100
        
        # Report progress every minute
        if ($i % 600 -eq 0) {
            $elapsed = (Get-Date) - $startTime
            Write-Host "  Soak progress: $([math]::Round($elapsed.TotalMinutes, 1))/$SoakDurationMin min ($i orders)" -ForegroundColor DarkGray
        }
    }
    
    Format-LatencyReport -TestName "SoakTest" -Latencies $soakLatencies -SuccessCount $soakSuccess -FailCount $soakFail -FillCount 0 -TotalOrders $soakLatencies.Count
}

# Cleanup
Write-Host "[Cleanup] Stopping service..." -ForegroundColor Yellow
Stop-ExchangeService

Write-Host "========================================" -ForegroundColor Magenta
Write-Host "TEST COMPLETE" -ForegroundColor Magenta
Write-Host "========================================" -ForegroundColor Magenta
