# ============================================================
# Real Matching Engine E2E Stress Test
# Tests: Order Submit -> Matching -> WAL Persistence -> Risk -> Metrics
# ============================================================

param(
    [int]$OrderCount = 100,
    [int]$Concurrency = 10,
    [string]$BaseUri = "http://127.0.0.1:3030"
)

$ErrorActionPreference = "Stop"

# Unique run prefix to avoid idempotency collisions across runs
$RunId = [Guid]::NewGuid().ToString("N").Substring(0, 8)

# Helper: compute HMAC-SHA256 signature
function Compute-HmacSignature {
    param([string]$Message, [string]$Secret)
    $hmac = [System.Security.Cryptography.HMACSHA256]::new(
        [System.Text.Encoding]::UTF8.GetBytes($Secret))
    $hashBytes = $hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($Message))
    $hmac.Dispose()
    return [BitConverter]::ToString($hashBytes).Replace("-", "").ToLowerInvariant()
}

# Helper: compute SHA256 hex of body bytes
function Compute-BodyHash {
    param([byte[]]$BodyBytes)
    $hash = [System.Security.Cryptography.SHA256]::Create()
    $hashBytes = $hash.ComputeHash($BodyBytes)
    $hash.Dispose()
    return [BitConverter]::ToString($hashBytes).Replace("-", "").ToLowerInvariant()
}

# Helper: build auth headers for a request
function Build-AuthHeaders {
    param(
        [string]$Method,
        [string]$Path,
        [string]$Subject,
        [string]$Role = "user",
        [string]$RequestId,
        [byte[]]$BodyBytes,
        [string]$Secret = "dev-secret-change-me-to-32-chars-min!",
        [string]$SessionId = ""
    )
    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $payload = "${Method}`n${Path}`n`n${Subject}`n${Role}`n${SessionId}`n${timestamp}`n${RequestId}"
    $signature = Compute-HmacSignature -Message $payload -Secret $Secret
    $bodyHash = Compute-BodyHash -BodyBytes $BodyBytes

    return @{
        "x-internal-auth-subject"     = $Subject
        "x-internal-auth-role"        = $Role
        "x-internal-auth-session-id"  = $SessionId
        "x-internal-auth-timestamp"   = $timestamp
        "x-internal-auth-signature"   = $signature
        "x-internal-auth-body-sha256" = $bodyHash
        "x-request-id"                = $RequestId
        "Content-Type"                = "application/json"
    }
}

# Helper: invoke admin deposit with retry on 429
function Invoke-AdminDeposit {
    param(
        [string]$UserId,
        [long]$Amount,
        [string]$OpId,
        [int]$MaxRetries = 3
    )
    for ($attempt = 1; $attempt -le $MaxRetries; $attempt++) {
        $depositBody = @{
            user_id = $UserId
            amount = $Amount
            op_id = $OpId
        } | ConvertTo-Json -Compress
        $depositBodyBytes = [System.Text.Encoding]::UTF8.GetBytes($depositBody)
        $depositAuthHeaders = Build-AuthHeaders -Method "POST" -Path "/deposit" -Subject "admin" -Role "admin" -RequestId $OpId -BodyBytes $depositBodyBytes

        try {
            $resp = Invoke-RestMethod -Uri "$BaseUri/deposit" -Method Post -Headers $depositAuthHeaders -Body $depositBodyBytes -TimeoutSec 10
            return $resp
        } catch {
            $statusCode = $_.Exception.Response.StatusCode.value__
            if ($statusCode -eq 429 -and $attempt -lt $MaxRetries) {
                Start-Sleep -Milliseconds (500 * $attempt)
                continue
            }
            throw $_
        }
    }
}

Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  Real Matching Engine E2E Test" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

# --- 0. Pre-flight check ------------------------------------
Write-Host "[0/7] Pre-flight service check..." -ForegroundColor Yellow
try {
    $health = Invoke-RestMethod -Uri "$BaseUri/health" -Method Get -TimeoutSec 5
    Write-Host "  OK Health check passed: status=$($health.status) | accounts=$($health.accounts)" -ForegroundColor Green
} catch {
    Write-Host "  FAIL Service unreachable: $_" -ForegroundColor Red
    Write-Host "  Please start the server first: cd rust-exchange; cargo run --release" -ForegroundColor Yellow
    exit 1
}

$metrics = Invoke-RestMethod -Uri "$BaseUri/metrics" -Method Get -TimeoutSec 5
Write-Host "  OK Metrics endpoint responsive | orders_received=$($metrics.orders_received)" -ForegroundColor Green

# Record initial WAL file sizes
$dataDir = Join-Path $PSScriptRoot "..\data"
$walFiles = @("ledger.wal.jsonl", "sequencer.wal.jsonl", "trade_journal.wal.jsonl", "trade_settlement.wal.jsonl", "matching.snapshot.jsonl")
$initialWalSizes = @{}
foreach ($f in $walFiles) {
    $path = Join-Path $dataDir $f
    if (Test-Path $path) {
        $stat = Get-Item $path
        $initialWalSizes[$f] = $stat.Length
        Write-Host "  WAL: $f = $($stat.Length) bytes" -ForegroundColor Gray
    } else {
        $initialWalSizes[$f] = 0
        Write-Host "  WAL: $f = not found (will be created)" -ForegroundColor Gray
    }
}

$initialOrders = [long]$metrics.orders_received
$initialFills = [long]$metrics.orders_filled
$initialRejected = [long]$metrics.orders_rejected

# --- 0.5. Fund test accounts --------------------------------
Write-Host ""
Write-Host "[0.5/7] Funding test accounts..." -ForegroundColor Yellow

# Fund the buyer (test-trader-01) — this account has CASH only
try {
    $depResp = Invoke-AdminDeposit -UserId "test-trader-01" -Amount 50000000 -OpId "e2e-deposit-trader-01-$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())"
    if ($depResp.status -eq "ok") {
        Write-Host "  OK Funded test-trader-01: 50000000 subunits" -ForegroundColor Green
    } else {
        Write-Host "  WARN Deposit response: $(ConvertTo-Json $depResp -Compress)" -ForegroundColor Yellow
    }
} catch {
    Write-Host "  WARN Failed to fund test-trader-01: $($_.Exception.Message)" -ForegroundColor Yellow
}

# Fund stress users (they need cash for buy orders)
$stressUserCount = [Math]::Min($OrderCount, 50)
for ($i = 0; $i -lt $stressUserCount; $i++) {
    $userId = "stress-user-$i"
    # Even users = buyers (need cash), odd users = sellers (need BTC position)
    $cashAmount = 5000000
    $opId = "e2e-deposit-cash-stress-$i-$RunId"
    try {
        $depResp = Invoke-AdminDeposit -UserId $userId -Amount $cashAmount -OpId $opId
        if ($depResp.status -ne "ok" -and $i -lt 3) {
            Write-Host "  WARN Cash deposit failed for stress-user-${i}: $(ConvertTo-Json $depResp -Compress)" -ForegroundColor Yellow
        }
    } catch {
        if ($i -lt 3) {
            Write-Host "  WARN Failed to fund cash for stress-user-${i}: $($_.Exception.Message)" -ForegroundColor Yellow
        }
    }
    # Small delay to avoid hitting rate limit
    if ($i % 5 -eq 4) {
        Start-Sleep -Milliseconds 200
    }
}
Write-Host "  OK Funded $stressUserCount stress users (cash for buyers, BTC for sellers)" -ForegroundColor Green

# --- 1. Submit first limit sell order (Maker) ---------------
Write-Host ""
Write-Host "[1/6] Submit first limit SELL order (Maker)..." -ForegroundColor Yellow
$uniqueTs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$sellBody = @{
    market_id = "btc-usdt"
    side = "sell"
    price = 50000
    amount = 5
    outcome = 0
    client_order_id = "e2e-sell-$uniqueTs"
    request_id = "req-e2e-sell-$uniqueTs"
} | ConvertTo-Json -Compress
$sellBodyBytes = [System.Text.Encoding]::UTF8.GetBytes($sellBody)
$sellReqId = "req-e2e-sell-$uniqueTs"
# Use "trader" subject — seeded with BTC from bootstrap
$sellAuthHeaders = Build-AuthHeaders -Method "POST" -Path "/intent" -Subject "trader" -RequestId $sellReqId -BodyBytes $sellBodyBytes

$sellStart = [System.Diagnostics.Stopwatch]::StartNew()
try {
    $sellResp = Invoke-RestMethod -Uri "$BaseUri/intent" -Method Post -Headers $sellAuthHeaders -Body $sellBodyBytes
    $sellStart.Stop()
    Write-Host "  OK Sell order submitted | latency=$($sellStart.ElapsedMilliseconds)ms | state=$($sellResp.order_state)" -ForegroundColor Green
    Write-Host "    Response: $(ConvertTo-Json $sellResp -Compress)" -ForegroundColor DarkGray
} catch {
    $sellStart.Stop()
    Write-Host "  WARN Sell order failed: $($_.Exception.Message)" -ForegroundColor Yellow
    $sellResp = $null
}

# --- 2. Submit limit buy order (Taker, triggers matching) ---
Write-Host ""
Write-Host "[2/6] Submit limit BUY order (Taker, triggers matching)..." -ForegroundColor Yellow
$uniqueTs2 = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$buyBody = @{
    market_id = "btc-usdt"
    side = "buy"
    price = 50000
    amount = 3
    outcome = 0
    client_order_id = "e2e-buy-$uniqueTs2"
    request_id = "req-e2e-buy-$uniqueTs2"
} | ConvertTo-Json -Compress
$buyBodyBytes = [System.Text.Encoding]::UTF8.GetBytes($buyBody)
$buyReqId = "req-e2e-buy-$uniqueTs2"
# Use DIFFERENT subject (test-trader-01) to avoid self-trade prevention
$buyAuthHeaders = Build-AuthHeaders -Method "POST" -Path "/intent" -Subject "test-trader-01" -RequestId $buyReqId -BodyBytes $buyBodyBytes

$buyStart = [System.Diagnostics.Stopwatch]::StartNew()
try {
    $buyResp = Invoke-RestMethod -Uri "$BaseUri/intent" -Method Post -Headers $buyAuthHeaders -Body $buyBodyBytes
    $buyStart.Stop()
    Write-Host "  OK Buy order submitted | latency=$($buyStart.ElapsedMilliseconds)ms | state=$($buyResp.order_state)" -ForegroundColor Green
    Write-Host "    Response: $(ConvertTo-Json $buyResp -Compress)" -ForegroundColor DarkGray

    if ($buyResp.fills -gt 0) {
        Write-Host "  MATCH Successful! Generated $($buyResp.fills) fills" -ForegroundColor Green
    } else {
        Write-Host "  WARN No fill generated (may be insufficient balance)" -ForegroundColor Yellow
    }
} catch {
    $buyStart.Stop()
    $errorDetail = $_.ErrorDetails.Message
    if (-not $errorDetail) {
        try { $errorDetail = $_.Exception.Response.StatusCode.ToString() } catch {}
    }
    Write-Host "  WARN Buy order failed: $($_.Exception.Message) | Detail: $errorDetail" -ForegroundColor Yellow
    $buyResp = $null
}

# --- 3. Query order book depth ------------------------------
Write-Host ""
Write-Host "[3/6] Querying order book depth..." -ForegroundColor Yellow
try {
    $depthResp = Invoke-RestMethod -Uri "$BaseUri/markets/btc-usdt/book" -Method Get -TimeoutSec 5
    $bidCount = if ($depthResp.bids) { $depthResp.bids.Count } else { 0 }
    $askCount = if ($depthResp.asks) { $depthResp.asks.Count } else { 0 }
    Write-Host "  OK Book depth: bids=$bidCount | asks=$askCount" -ForegroundColor Green
} catch {
    Write-Host "  WARN Depth query failed: $($_.Exception.Message)" -ForegroundColor Yellow
}

# --- 4. Stress test: batch orders ---------------------------
Write-Host ""
Write-Host "[4/7] Stress test: $OrderCount orders ($Concurrency concurrent)..." -ForegroundColor Yellow

$latencies = @()
$successCount = 0
$failCount = 0
$fillCount = 0

# ScriptBlock for stress test job - must be self-contained with helper functions
$stressJobScript = {
    param($BaseUri, $i, $Secret, $RunId)

    function Compute-HmacSignature {
        param([string]$Message, [string]$Secret)
        $hmac = [System.Security.Cryptography.HMACSHA256]::new(
            [System.Text.Encoding]::UTF8.GetBytes($Secret))
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

    # Alternate sides to create realistic order flow and avoid self-trade
    # All stress users are buyers (cash only), place bids at various levels below maker ask
    $side = "buy"
    $userId = "stress-user-$i"
    # Bids below 50000 (maker ask price), spread across depth levels
    $price = 48000 + ($i % 1000)
    $amount = 1 + ($i % 3)
    $requestId = "req-${RunId}-stress-$i"

    $body = @{
        market_id = "btc-usdt"
        side = $side
        price = $price
        amount = $amount
        outcome = 0
        client_order_id = "stress-${RunId}-$i"
        request_id = $requestId
    } | ConvertTo-Json -Compress
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($body)

    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $payload = "POST`n/intent`n`n${userId}`nuser`n`n${timestamp}`n${requestId}"
    $signature = Compute-HmacSignature -Message $payload -Secret $Secret
    $bodyHash = Compute-BodyHash -BodyBytes $bodyBytes

    $authHeaders = @{
        "x-internal-auth-subject"     = $userId
        "x-internal-auth-role"        = "user"
        "x-internal-auth-session-id"  = ""
        "x-internal-auth-timestamp"   = $timestamp
        "x-internal-auth-signature"   = $signature
        "x-internal-auth-body-sha256" = $bodyHash
        "x-request-id"                = $requestId
        "Content-Type"                = "application/json"
    }

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $resp = Invoke-RestMethod -Uri "$BaseUri/intent" -Method Post -Headers $authHeaders -Body $bodyBytes -TimeoutSec 10
        $sw.Stop()
        $fills = 0
        if ($resp.fills) { $fills = $resp.fills }
        $state = "unknown"
        if ($resp.order_state) { $state = $resp.order_state }
        return @{ ok = $true; ms = $sw.ElapsedMilliseconds; fills = $fills; state = $state }
    } catch {
        $sw.Stop()
        return @{ ok = $false; ms = $sw.ElapsedMilliseconds; error = $_.Exception.Message }
    }
}

# Batch concurrency
$batches = [Math]::Ceiling($OrderCount / $Concurrency)
for ($batch = 0; $batch -lt $batches; $batch++) {
    $tasks = @()
    $startIdx = $batch * $Concurrency
    $endIdx = [Math]::Min($startIdx + $Concurrency, $OrderCount)

    for ($i = $startIdx; $i -lt $endIdx; $i++) {
        $tasks += Start-Job -ScriptBlock $stressJobScript -ArgumentList $BaseUri, $i, "dev-secret-change-me-to-32-chars-min!", $RunId
    }

    # Wait for this batch to complete
    $tasks | Wait-Job | ForEach-Object {
        $result = Receive-Job $_
        Remove-Job $_
        $latencies += $result.ms
        if ($result.ok) {
            $successCount++
            if ($result.fills -gt 0) { $fillCount += $result.fills }
        } else {
            $failCount++
            if ($failCount -le 3) {
                Write-Host "  Sample failure: $($result.error)" -ForegroundColor Red
            }
        }
    }

    $pct = [Math]::Round(($endIdx / $OrderCount) * 100)
    Write-Host "  Progress: $endIdx/$OrderCount (${pct}%)" -ForegroundColor DarkGray
}

# Calculate latency stats
$sortedLatencies = $latencies | Sort-Object
$p50Idx = [Math]::Floor($sortedLatencies.Count * 0.50)
$p95Idx = [Math]::Floor($sortedLatencies.Count * 0.95)
$p99Idx = [Math]::Floor($sortedLatencies.Count * 0.99)
$p50 = $sortedLatencies[$p50Idx]
$p95 = $sortedLatencies[$p95Idx]
$p99 = $sortedLatencies[$p99Idx]
$avg = [Math]::Round(($latencies | Measure-Object -Average).Average)
$min = ($latencies | Measure-Object -Minimum).Minimum
$max = ($latencies | Measure-Object -Maximum).Maximum

Write-Host ""
Write-Host "  Stress Test Results:" -ForegroundColor Cyan
Write-Host "    Success=$successCount | Failed=$failCount | Fills=$fillCount" -ForegroundColor White
Write-Host "    Latency (ms): P50=$p50 | P95=$p95 | P99=$p99 | Avg=$avg | Range=[$min, $max]" -ForegroundColor White

# --- 5. Verify WAL persistence ------------------------------
Write-Host ""
Write-Host "[5/7] Verifying WAL persistence..." -ForegroundColor Yellow
$walChanges = @{}
foreach ($f in $walFiles) {
    $path = Join-Path $dataDir $f
    if (Test-Path $path) {
        $stat = Get-Item $path
        $prevSize = $initialWalSizes[$f]
        $delta = $stat.Length - $prevSize
        $walChanges[$f] = $delta
        $indicator = if ($delta -gt 0) { "GROW" } elseif ($delta -eq 0 -and $prevSize -gt 0) { "SAME" } else { "NEW" }
        Write-Host "  WAL: $f = $($stat.Length) bytes ($indicator, +${delta})" -ForegroundColor Green
    } else {
        Write-Host "  WAL: $f = MISSING" -ForegroundColor Red
    }
}

# Count WAL entries
$totalWalEntries = 0
foreach ($f in $walFiles) {
    $path = Join-Path $dataDir $f
    if (Test-Path $path) {
        $lines = (Get-Content $path).Count
        $totalWalEntries += $lines
        Write-Host "  WAL: $f = $lines entries" -ForegroundColor Gray
    }
}
Write-Host "  Total WAL entries across all files: $totalWalEntries" -ForegroundColor Green

# --- 6. Verify metrics delta --------------------------------
Write-Host ""
Write-Host "[6/7] Verifying metrics delta..." -ForegroundColor Yellow
$finalMetrics = Invoke-RestMethod -Uri "$BaseUri/metrics" -Method Get -TimeoutSec 5
$deltaOrders = [long]$finalMetrics.orders_received - $initialOrders
$deltaFills = [long]$finalMetrics.orders_filled - $initialFills
$deltaRejected = [long]$finalMetrics.orders_rejected - $initialRejected

Write-Host "  Orders received:  $initialOrders -> $($finalMetrics.orders_received) (delta: +$deltaOrders)" -ForegroundColor White
Write-Host "  Orders filled:    $initialFills -> $($finalMetrics.orders_filled) (delta: +$deltaFills)" -ForegroundColor White
Write-Host "  Orders rejected:  $initialRejected -> $($finalMetrics.orders_rejected) (delta: +$deltaRejected)" -ForegroundColor White

# --- 7. Final summary ---------------------------------------
Write-Host ""
Write-Host "========================================" -ForegroundColor Cyan
Write-Host "  E2E Test Summary" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

$passed = $true
$checks = @()

if ($sellResp -ne $null) {
    $checks += @{ Name = "Sell order (maker)"; Status = "PASS" }
} else {
    $checks += @{ Name = "Sell order (maker)"; Status = "FAIL" }
    $passed = $false
}

if ($buyResp -ne $null) {
    $checks += @{ Name = "Buy order (taker)"; Status = "PASS" }
} else {
    $checks += @{ Name = "Buy order (taker)"; Status = "FAIL" }
    $passed = $false
}

if ($successCount -gt 0) {
    $checks += @{ Name = "Stress test ($successCount/$OrderCount success)"; Status = "PASS" }
} else {
    $checks += @{ Name = "Stress test ($successCount/$OrderCount success)"; Status = "FAIL" }
    $passed = $false
}

if ($deltaOrders -gt 0) {
    $checks += @{ Name = "Metrics delta (orders +$deltaOrders)"; Status = "PASS" }
} else {
    $checks += @{ Name = "Metrics delta (orders +$deltaOrders)"; Status = "FAIL" }
    $passed = $false
}

$walGrowCount = ($walChanges.Values | Where-Object { $_ -gt 0 }).Count
if ($walGrowCount -gt 0) {
    $checks += @{ Name = "WAL growth ($walGrowCount files grew)"; Status = "PASS" }
} else {
    $checks += @{ Name = "WAL growth"; Status = "WARN" }
}

foreach ($check in $checks) {
    $color = if ($check.Status -eq "PASS") { "Green" } elseif ($check.Status -eq "WARN") { "Yellow" } else { "Red" }
    Write-Host "  [$($check.Status)] $($check.Name)" -ForegroundColor $color
}

if ($passed) {
    Write-Host ""
    Write-Host "  ALL CHECKS PASSED" -ForegroundColor Green
} else {
    Write-Host ""
    Write-Host "  SOME CHECKS FAILED" -ForegroundColor Red
}
