# Phase 6: HTTP Performance Benchmarks with Subsystem Timing
# Measures wall-clock + subsystem breakdown: queue_wait_us, risk_us, matching_core_us, settlement_persist_us
# 4 scenarios: single market, two markets, batch 10, cancel-replace

$ErrorActionPreference = "Stop"

# ============================================================
# Setup: Load HttpClient for sub-ms precision
# ============================================================
Add-Type -AssemblyName "System.Net.Http" | Out-Null

$BaseUri = "http://127.0.0.1:3031"
$Secret = "dev-secret-change-me-to-32-chars-min!"
$Subject = "user-test-123"
$Role = "user"
$SessionId = ""

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "PHASE 6: HTTP Performance Benchmarks" -ForegroundColor Cyan
Write-Host "========================================`n" -ForegroundColor Cyan

# ============================================================
# HMAC Helpers
# ============================================================
function Compute-HmacSignature {
    param([string]$Message, [string]$Secret)
    $hmac = [System.Security.Cryptography.HMACSHA256]::new([System.Text.Encoding]::UTF8.GetBytes($Secret))
    $hashBytes = $hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($Message))
    $hmac.Dispose()
    return [BitConverter]::ToString($hashBytes).Replace("-", "").ToLowerInvariant()
}

function Compute-BodyHash {
    param([byte[]]$BodyBytes)
    $hash = [System.Security.Cryptography.SHA256]::Create()
    $hashBytes = $hash.ComputeHash($BodyBytes)
    $hash.Dispose()
    return [BitConverter]::ToString($hashBytes).Replace("-", "").ToLowerInvariant()
}

# ============================================================
# HttpClient Factory (connection pooling)
# ============================================================
function New-BenchClient {
    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.UseCookies = $false
    $client = [System.Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromSeconds(10)
    return $client
}

# ============================================================
# Timed POST — returns wall-clock ms + parsed response
# ============================================================
function Invoke-TimedPost {
    param(
        [string]$Path,
        [string]$BodyJson,
        [System.Net.Http.HttpClient]$Client,
        [string]$Subject = $Script:Subject,
        [string]$Role = $Script:Role
    )
    $RequestId = [guid]::NewGuid().ToString()
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($BodyJson)
    $bodyHash = Compute-BodyHash -BodyBytes $bodyBytes
    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $payload = "POST`n${Path}`n`n${Subject}`n${Role}`n`n${timestamp}`n${RequestId}"
    $signature = Compute-HmacSignature -Message $payload -Secret $Secret

    $content = [System.Net.Http.ByteArrayContent]::new($bodyBytes)
    $content.Headers.ContentType = [System.Net.Http.Headers.MediaTypeHeaderValue]::new("application/json")
    $content.Headers.TryAddWithoutValidation("x-internal-auth-subject", $Subject) | Out-Null
    $content.Headers.TryAddWithoutValidation("x-internal-auth-role", $Role) | Out-Null
    $content.Headers.TryAddWithoutValidation("x-internal-auth-session-id", "") | Out-Null
    $content.Headers.TryAddWithoutValidation("x-internal-auth-timestamp", $timestamp) | Out-Null
    $content.Headers.TryAddWithoutValidation("x-internal-auth-signature", $signature) | Out-Null
    $content.Headers.TryAddWithoutValidation("x-internal-auth-body-sha256", $bodyHash) | Out-Null
    $content.Headers.TryAddWithoutValidation("x-request-id", $RequestId) | Out-Null

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Post, "${BaseUri}${Path}")
        $request.Content = $content
        $response = $Client.SendAsync($request).GetAwaiter().GetResult()
        $sw.Stop()

        $respBody = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
        $parsed = $null
        try { $parsed = $respBody | ConvertFrom-Json -ErrorAction SilentlyContinue } catch {}

        return @{
            wall_clock_ms = [int]$sw.ElapsedMilliseconds
            status_code   = [int]$response.StatusCode
            parsed        = $parsed
            raw           = $respBody
            request_id    = $RequestId
        }
    } catch {
        $sw.Stop()
        return @{
            wall_clock_ms = [int]$sw.ElapsedMilliseconds
            status_code   = 0
            parsed        = $null
            raw           = $_.Exception.Message
            request_id    = $RequestId
            error         = $_.Exception.Message
        }
    } finally {
        $content.Dispose()
    }
}

# ============================================================
# Extract subsystem timings from response
# ============================================================
function Extract-Timings {
    param($Parsed)
    if (-not $Parsed) { return @{ queue_wait_us = 0; risk_us = 0; matching_core_us = 0; settlement_persist_us = 0 } }

    return @{
        queue_wait_us       = if ($Parsed.queue_wait_us) { [int]$Parsed.queue_wait_us } elseif ($Parsed.timings -and $Parsed.timings.queue_wait_us) { [int]$Parsed.timings.queue_wait_us } else { 0 }
        risk_us             = if ($Parsed.risk_us) { [int]$Parsed.risk_us } elseif ($Parsed.timings -and $Parsed.timings.risk_us) { [int]$Parsed.timings.risk_us } else { 0 }
        matching_core_us    = if ($Parsed.matching_core_us) { [int]$Parsed.matching_core_us } elseif ($Parsed.timings -and $Parsed.timings.matching_core_us) { [int]$Parsed.timings.matching_core_us } else { 0 }
        settlement_persist_us = if ($Parsed.settlement_persist_us) { [int]$Parsed.settlement_persist_us } elseif ($Parsed.timings -and $Parsed.timings.settlement_persist_us) { [int]$Parsed.timings.settlement_persist_us } else { 0 }
    }
}

# ============================================================
# Stats helpers
# ============================================================
function Calc-Percentile {
    param([double[]]$Values, [double]$Pct)
    if ($Values.Count -eq 0) { return 0 }
    $sorted = $Values | Sort-Object
    $idx = [Math]::Ceiling($sorted.Count * $Pct / 100.0) - 1
    if ($idx -lt 0) { $idx = 0 }
    if ($idx -ge $sorted.Count) { $idx = $sorted.Count - 1 }
    return [int]$sorted[$idx]
}

function Calc-Avg {
    param([double[]]$Values)
    if ($Values.Count -eq 0) { return 0 }
    return [int]($Values | Measure-Object -Average).Average
}

# ============================================================
# Benchmark Runner
# ============================================================
$AllResults = @{}

function Run-Benchmark {
    param(
        [string]$Name,
        [scriptblock]$Workload,
        [int]$SampleCount
    )
    Write-Host "`n--- Benchmark: $Name ($SampleCount samples) ---" -ForegroundColor White

    $client = New-BenchClient
    $wallClocks = @()
    $queueWaits = @()
    $riskTimes = @()
    $matchingTimes = @()
    $settlementTimes = @()
    $successCount = 0
    $errorCount = 0
    $errors500 = 0

    for ($i = 0; $i -lt $SampleCount; $i++) {
        $result = & $Workload -Client $client -Index $i
        $wallClocks += $result.wall_clock_ms

        if ($result.status_code -ge 200 -and $result.status_code -lt 300) {
            $successCount++
        } else {
            $errorCount++
            if ($result.status_code -ge 500) { $errors500++ }
        }

        $timings = Extract-Timings -Parsed $result.parsed
        $queueWaits += $timings.queue_wait_us
        $riskTimes += $timings.risk_us
        $matchingTimes += $timings.matching_core_us
        $settlementTimes += $timings.settlement_persist_us
    }

    $client.Dispose()

    $apiOverhead_us = @()
    for ($i = 0; $i -lt $wallClocks.Count; $i++) {
        $subsystem_total_us = $queueWaits[$i] + $riskTimes[$i] + $matchingTimes[$i] + $settlementTimes[$i]
        $overhead = ($wallClocks[$i] * 1000) - $subsystem_total_us
        $api_overhead_us += $overhead
    }

    $scenarioResult = @{
        Name              = $Name
        SampleCount       = $SampleCount
        SuccessCount      = $successCount
        ErrorCount        = $errorCount
        Errors500         = $errors500
        WallClock_P50_ms  = Calc-Percentile -Values $wallClocks -Pct 50
        WallClock_P95_ms  = Calc-Percentile -Values $wallClocks -Pct 95
        WallClock_P99_ms  = Calc-Percentile -Values $wallClocks -Pct 99
        WallClock_Avg_ms  = Calc-Avg -Values $wallClocks
        QueueWait_P50_us  = Calc-Percentile -Values $queueWaits -Pct 50
        QueueWait_P95_us  = Calc-Percentile -Values $queueWaits -Pct 95
        Risk_P50_us       = Calc-Percentile -Values $riskTimes -Pct 50
        Risk_P95_us       = Calc-Percentile -Values $riskTimes -Pct 95
        Matching_P50_us   = Calc-Percentile -Values $matchingTimes -Pct 50
        Matching_P95_us   = Calc-Percentile -Values $matchingTimes -Pct 95
        Settlement_P50_us = Calc-Percentile -Values $settlementTimes -Pct 50
        Settlement_P95_us = Calc-Percentile -Values $settlementTimes -Pct 95
        ApiOverhead_P50_us = if ($api_overhead_us.Count -gt 0) { Calc-Percentile -Values $api_overhead_us -Pct 50 } else { 0 }
        ApiOverhead_P95_us = if ($api_overhead_us.Count -gt 0) { Calc-Percentile -Values $api_overhead_us -Pct 95 } else { 0 }
    }

    $AllResults[$Name] = $scenarioResult

    # Print results
    Write-Host "  Wall Clock:  P50=$($scenarioResult.WallClock_P50_ms)ms  P95=$($scenarioResult.WallClock_P95_ms)ms  P99=$($scenarioResult.WallClock_P99_ms)ms  Avg=$($scenarioResult.WallClock_Avg_ms)ms" -ForegroundColor Yellow
    Write-Host "  Queue Wait:  P50=$($scenarioResult.QueueWait_P50_us)µs  P95=$($scenarioResult.QueueWait_P95_us)µs" -ForegroundColor Gray
    Write-Host "  Risk Check:  P50=$($scenarioResult.Risk_P50_us)µs  P95=$($scenarioResult.Risk_P95_us)µs" -ForegroundColor Gray
    Write-Host "  Matching:    P50=$($scenarioResult.Matching_P50_us)µs  P95=$($scenarioResult.Matching_P95_us)µs" -ForegroundColor Gray
    Write-Host "  Settlement:  P50=$($scenarioResult.Settlement_P50_us)µs  P95=$($scenarioResult.Settlement_P95_us)µs" -ForegroundColor Gray
    Write-Host "  API Overhead: P50=$($scenarioResult.ApiOverhead_P50_us)µs  P95=$($scenarioResult.ApiOverhead_P95_us)µs" -ForegroundColor DarkGray
    Write-Host "  Success: $successCount/$SampleCount | Errors: $errorCount (500s: $errors500)" -ForegroundColor $(if ($errors500 -eq 0) { "Green" } else { "Red" })

    return $scenarioResult
}

# ============================================================
# Check service health first
# ============================================================
Write-Host "Checking service health..." -ForegroundColor Yellow
try {
    $testClient = New-BenchClient
    $healthResult = Invoke-TimedPost -Path "/health" -BodyJson "{}" -Client $testClient
    $testClient.Dispose()
    if ($healthResult.status_code -ne 200) {
        Write-Host "Service not healthy (HTTP $($healthResult.status_code)). Please start it first." -ForegroundColor Red
        exit 1
    }
    Write-Host "Service ready.`n" -ForegroundColor Green
} catch {
    Write-Host "Cannot connect to service at $BaseUri. Please start it first." -ForegroundColor Red
    exit 1
}

# ============================================================
# Scenario 1: Single Market Orders (n=50)
# ============================================================
$workload1 = {
    param($Client, $Index)
    $orderObj = [ordered]@{
        market_id       = "btc-usdt"
        side            = "sell"
        order_type      = "limit"
        price           = (55000 + $Index)
        amount          = 0.001
        outcome         = 0
        time_in_force   = "gtc"
        client_order_id = "bench1_$Index"
    }
    $json = $orderObj | ConvertTo-Json -Compress
    return Invoke-TimedPost -Path "/submit-order" -BodyJson $json -Client $Client
}

Run-Benchmark -Name "SingleMarket" -Workload $workload1 -SampleCount 50

# ============================================================
# Scenario 2: Two Markets Interleaved (n=25 each = 50 total)
# ============================================================
$workload2 = {
    param($Client, $Index)
    $marketId = if ($Index % 2 -eq 0) { "btc-usdt" } else { "eth-usdt" }
    $price = if ($marketId -eq "btc-usdt") { (55000 + $Index) } else { (3000 + $Index) }
    $amount = if ($marketId -eq "btc-usdt") { 0.001 } else { 0.01 }

    $orderObj = [ordered]@{
        market_id       = $marketId
        side            = "sell"
        order_type      = "limit"
        price           = $price
        amount          = $amount
        outcome         = 0
        time_in_force   = "gtc"
        client_order_id = "bench2_$Index"
    }
    $json = $orderObj | ConvertTo-Json -Compress
    return Invoke-TimedPost -Path "/submit-order" -BodyJson $json -Client $Client
}

Run-Benchmark -Name "TwoMarkets" -Workload $workload2 -SampleCount 50

# ============================================================
# Scenario 3: Batch 10 Orders (n=10 batches = 100 orders)
# ============================================================
$workload3 = {
    param($Client, $Index)
    $orders = @()
    for ($j = 0; $j -lt 10; $j++) {
        $orderObj = [ordered]@{
            market_id       = "btc-usdt"
            side            = "sell"
            order_type      = "limit"
            price           = (55000 + $Index * 10 + $j)
            amount          = 0.001
            outcome         = 0
            time_in_force   = "gtc"
            client_order_id = "bench3_${Index}_$j"
        }
        $orders += ($orderObj | ConvertTo-Json -Compress)
    }
    $batchJson = "[$($orders -join ',')]"
    return Invoke-TimedPost -Path "/submit-batch" -BodyJson $batchJson -Client $Client
}

Run-Benchmark -Name "Batch10" -Workload $workload3 -SampleCount 10

# ============================================================
# Scenario 4: Cancel-Replace Cycles (n=20)
# ============================================================
$activeOrderIds = @{}

$workload4_submit = {
    param($Client, $Index)
    $clientOrderId = "bench4_$Index"
    $orderObj = [ordered]@{
        market_id       = "btc-usdt"
        side            = "sell"
        order_type      = "limit"
        price           = (50000 + $Index * 100)
        amount          = 0.001
        outcome         = 0
        time_in_force   = "gtc"
        client_order_id = $clientOrderId
    }
    $json = $orderObj | ConvertTo-Json -Compress
    $result = Invoke-TimedPost -Path "/submit-order" -BodyJson $json -Client $Client
    if ($result.parsed -and $result.parsed.order_id) {
        $activeOrderIds[$Index] = $result.parsed.order_id
    }
    return $result
}

$workload4_cancel = {
    param($Client, $Index)
    $orderId = $activeOrderIds[$Index]
    if (-not $orderId) {
        return @{ wall_clock_ms = 0; status_code = 200; parsed = $null; raw = ""; request_id = "skip" }
    }
    $cancelObj = @{ market_id = "btc-usdt"; order_id = $orderId } | ConvertTo-Json -Compress
    return Invoke-TimedPost -Path "/cancel-order" -BodyJson $cancelObj -Client $Client
}

Write-Host "`n--- Benchmark: CancelReplace (20 cycles) ---" -ForegroundColor White

$clientCR = New-BenchClient
$crWallClocks = @()
$crQueueWaits = @()
$crRiskTimes = @()
$crMatchingTimes = @()
$crSettlementTimes = @()
$crSuccessCount = 0
$crErrorCount = 0
$crErrors500 = 0

for ($i = 0; $i -lt 20; $i++) {
    # Submit
    $subResult = & $workload4_submit -Client $clientCR -Index $i
    $crWallClocks += $subResult.wall_clock_ms
    $timings = Extract-Timings -Parsed $subResult.parsed
    $crQueueWaits += $timings.queue_wait_us
    $crRiskTimes += $timings.risk_us
    $crMatchingTimes += $timings.matching_core_us
    $crSettlementTimes += $timings.settlement_persist_us

    if ($subResult.status_code -ge 200 -and $subResult.status_code -lt 300) { $crSuccessCount++ } else { $crErrorCount++; if ($subResult.status_code -ge 500) { $crErrors500++ } }

    # Cancel
    Start-Sleep -Milliseconds 50
    $canResult = & $workload4_cancel -Client $clientCR -Index $i
    $crWallClocks += $canResult.wall_clock_ms
    if ($canResult.status_code -ge 200 -and $canResult.status_code -lt 300) { $crSuccessCount++ } else { $crErrorCount++; if ($canResult.status_code -ge 500) { $crErrors500++ } }
}

$clientCR.Dispose()

$crApiOverhead_us = @()
for ($i = 0; $i -lt $crWallClocks.Count; $i++) {
    $subsystem_total_us = $crQueueWaits[$i] + $crRiskTimes[$i] + $crMatchingTimes[$i] + $crSettlementTimes[$i]
    $overhead = ($crWallClocks[$i] * 1000) - $subsystem_total_us
    $crApiOverhead_us += $overhead
}

$AllResults["CancelReplace"] = @{
    Name              = "CancelReplace"
    SampleCount       = 40  # 20 submit + 20 cancel
    SuccessCount      = $crSuccessCount
    ErrorCount        = $crErrorCount
    Errors500         = $crErrors500
    WallClock_P50_ms  = Calc-Percentile -Values $crWallClocks -Pct 50
    WallClock_P95_ms  = Calc-Percentile -Values $crWallClocks -Pct 95
    WallClock_P99_ms  = Calc-Percentile -Values $crWallClocks -Pct 99
    WallClock_Avg_ms  = Calc-Avg -Values $crWallClocks
    QueueWait_P50_us  = Calc-Percentile -Values $crQueueWaits -Pct 50
    QueueWait_P95_us  = Calc-Percentile -Values $crQueueWaits -Pct 95
    Risk_P50_us       = Calc-Percentile -Values $crRiskTimes -Pct 50
    Risk_P95_us       = Calc-Percentile -Values $crRiskTimes -Pct 95
    Matching_P50_us   = Calc-Percentile -Values $crMatchingTimes -Pct 50
    Matching_P95_us   = Calc-Percentile -Values $crMatchingTimes -Pct 95
    Settlement_P50_us = Calc-Percentile -Values $crSettlementTimes -Pct 50
    Settlement_P95_us = Calc-Percentile -Values $crSettlementTimes -Pct 95
    ApiOverhead_P50_us = if ($crApiOverhead_us.Count -gt 0) { Calc-Percentile -Values $crApiOverhead_us -Pct 50 } else { 0 }
    ApiOverhead_P95_us = if ($crApiOverhead_us.Count -gt 0) { Calc-Percentile -Values $crApiOverhead_us -Pct 95 } else { 0 }
}

$r = $AllResults["CancelReplace"]
Write-Host "  Wall Clock:  P50=$($r.WallClock_P50_ms)ms  P95=$($r.WallClock_P95_ms)ms  P99=$($r.WallClock_P99_ms)ms  Avg=$($r.WallClock_Avg_ms)ms" -ForegroundColor Yellow
Write-Host "  Queue Wait:  P50=$($r.QueueWait_P50_us)µs  P95=$($r.QueueWait_P95_us)µs" -ForegroundColor Gray
Write-Host "  Risk Check:  P50=$($r.Risk_P50_us)µs  P95=$($r.Risk_P95_us)µs" -ForegroundColor Gray
Write-Host "  Matching:    P50=$($r.Matching_P50_us)µs  P95=$($r.Matching_P95_us)µs" -ForegroundColor Gray
Write-Host "  Settlement:  P50=$($r.Settlement_P50_us)µs  P95=$($r.Settlement_P95_us)µs" -ForegroundColor Gray
Write-Host "  API Overhead: P50=$($r.ApiOverhead_P50_us)µs  P95=$($r.ApiOverhead_P95_us)µs" -ForegroundColor DarkGray
Write-Host "  Success: $($r.SuccessCount)/$($r.SampleCount) | Errors: $($r.ErrorCount) (500s: $($r.Errors500))" -ForegroundColor $(if ($r.Errors500 -eq 0) { "Green" } else { "Red" })

# ============================================================
# Final Summary Table
# ============================================================
Write-Host "`n========================================" -ForegroundColor Cyan
Write-Host "PHASE 6: PERFORMANCE SUMMARY" -ForegroundColor Cyan
Write-Host "========================================`n" -ForegroundColor Cyan

$scenarios = @("SingleMarket", "TwoMarkets", "Batch10", "CancelReplace")

# Header
$header = "{0,-16} {1,8} {2,8} {3,8} {4,10} {5,10} {6,10} {7,10} {8,10} {9,8} {10,8}" -f "Scenario", "Samples", "WC P50", "WC P95", "Queue P50", "Risk P50", "Match P50", "Settle P50", "API OH P50", "Success", "500s"
Write-Host $header -ForegroundColor White
Write-Host ("-" * $header.Length) -ForegroundColor DarkGray

foreach ($name in $scenarios) {
    $r = $AllResults[$name]
    if (-not $r) { continue }

    $line = "{0,-16} {1,8} {2,7}ms {3,7}ms {4,8}µs {5,8}µs {6,8}µs {7,8}µs {8,8}µs {9,7}/{10} {11,8}" -f `
        $r.Name, `
        $r.SampleCount, `
        $r.WallClock_P50_ms, `
        $r.WallClock_P95_ms, `
        $r.QueueWait_P50_us, `
        $r.Risk_P50_us, `
        $r.Matching_P50_us, `
        $r.Settlement_P50_us, `
        $r.ApiOverhead_P50_us, `
        $r.SuccessCount, `
        $r.SampleCount, `
        $r.Errors500

    $color = if ($r.Errors500 -gt 0) { "Red" } else { "Green" }
    Write-Host $line -ForegroundColor $color
}

Write-Host "`n========================================" -ForegroundColor Cyan
Write-Host "Subsystem Cost Breakdown (P50 µs):" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

$totalQueue = 0; $totalRisk = 0; $totalMatch = 0; $totalSettle = 0; $totalApi = 0; $count = 0
foreach ($name in $scenarios) {
    $r = $AllResults[$name]
    if (-not $r) { continue }
    $totalQueue += $r.QueueWait_P50_us
    $totalRisk += $r.Risk_P50_us
    $totalMatch += $r.Matching_P50_us
    $totalSettle += $r.Settlement_P50_us
    $totalApi += $r.ApiOverhead_P50_us
    $count++
}

if ($count -gt 0) {
    $grandTotal = $totalQueue + $totalRisk + $totalMatch + $totalSettle + $totalApi
    Write-Host ("  Queue Wait:    {0,6} µs  ({1,5}%)" -f ($totalQueue / $count), [int]($totalQueue / $grandTotal * 100)) -ForegroundColor Gray
    Write-Host ("  Risk Check:    {0,6} µs  ({1,5}%)" -f ($totalRisk / $count), [int]($totalRisk / $grandTotal * 100)) -ForegroundColor Gray
    Write-Host ("  Matching:      {0,6} µs  ({1,5}%)" -f ($totalMatch / $count), [int]($totalMatch / $grandTotal * 100)) -ForegroundColor Gray
    Write-Host ("  Settlement:    {0,6} µs  ({1,5}%)" -f ($totalSettle / $count), [int]($totalSettle / $grandTotal * 100)) -ForegroundColor Gray
    Write-Host ("  API Overhead:  {0,6} µs  ({1,5}%)" -f ($totalApi / $count), [int]($totalApi / $grandTotal * 100)) -ForegroundColor DarkGray
    Write-Host ("  ─────────────────────────────────" ) -ForegroundColor DarkGray
    Write-Host ("  Total avg:     {0,6} µs" -f ($grandTotal / $count)) -ForegroundColor Yellow
}

Write-Host "`n========================================`n" -ForegroundColor Cyan

# Exit code: fail if any 500s
$total500s = ($AllResults.Values | Where-Object { $_.Errors500 -gt 0 }).Count
exit $(if ($total500s -eq 0) { 0 } else { 1 })
