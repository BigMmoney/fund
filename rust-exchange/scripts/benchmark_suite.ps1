# ============================================================
# Advanced Benchmark Suite — Real Matching Engine
# ============================================================
# Modes:
#   1. Quick Stress    — Short burst, variable concurrency
#   2. Soak Test       — Long-running (10/30 min), tail latency tracking
#   3. Concurrency Sweep — 1 / 4 / 8 / 16 / 32 concurrency levels
#   4. Hot Market      — Single-market blast, cancel storm, HF accounts
#
# Usage:
#   .\scripts\benchmark_suite.ps1                         # Quick stress (default)
#   .\scripts\benchmark_suite.ps1 -Mode Soak -DurationMin 10
#   .\scripts\benchmark_suite.ps1 -Mode ConcurrencySweep
#   .\scripts\benchmark_suite.ps1 -Mode HotMarket -Scenario CancelStorm
# ============================================================

param(
    [ValidateSet("Quick", "Soak", "ConcurrencySweep", "HotMarket")]
    [string]$Mode = "Quick",
    [int]$OrderCount = 500,
    [int]$Concurrency = 5,
    [int]$DurationMin = 10,
    [ValidateSet("SingleMarketBlast", "CancelStorm", "HighFreqAccounts")]
    [string]$Scenario = "SingleMarketBlast",
    [string]$BaseUri = "http://127.0.0.1:3030"
)

$ErrorActionPreference = "Stop"
$RunId = [Guid]::NewGuid().ToString("N").Substring(0, 8)
$StartTime = Get-Date

# ── HMAC helpers ──────────────────────────────────────────────
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

function Build-AuthHeaders {
    param(
        [string]$Method, [string]$Path, [string]$Subject,
        [string]$Role = "user", [string]$RequestId,
        [byte[]]$BodyBytes, [string]$Secret = "dev-secret-change-me",
        [string]$SessionId = ""
    )
    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $payload = "${Method}`n${Path}`n`n${Subject}`n${Role}`n${SessionId}`n${timestamp}`n${RequestId}"
    $signature = Compute-HmacSignature -Message $payload -Secret $Secret
    $bodyHash = Compute-BodyHash -BodyBytes $bodyBytes
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

function Invoke-AdminDeposit {
    param([string]$UserId, [long]$Amount, [string]$OpId, [int]$MaxRetries = 3)
    for ($attempt = 1; $attempt -le $MaxRetries; $attempt++) {
        $depositBody = @{ user_id = $UserId; amount = $Amount; op_id = $OpId } | ConvertTo-Json -Compress
        $depositBodyBytes = [System.Text.Encoding]::UTF8.GetBytes($depositBody)
        $depositAuthHeaders = Build-AuthHeaders -Method "POST" -Path "/deposit" -Subject "admin" -Role "admin" -RequestId $OpId -BodyBytes $depositBodyBytes
        try {
            return Invoke-RestMethod -Uri "$BaseUri/deposit" -Method Post -Headers $depositAuthHeaders -Body $depositBodyBytes -TimeoutSec 10
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

function Invoke-AdminPositionDeposit {
    param([string]$UserId, [string]$MarketId, [int]$Outcome, [long]$Amount, [string]$OpId, [int]$MaxRetries = 3)
    for ($attempt = 1; $attempt -le $MaxRetries; $attempt++) {
        $depositBody = @{ user_id = $UserId; market_id = $MarketId; outcome = $Outcome; amount = $Amount; op_id = $OpId } | ConvertTo-Json -Compress
        $depositBodyBytes = [System.Text.Encoding]::UTF8.GetBytes($depositBody)
        $depositAuthHeaders = Build-AuthHeaders -Method "POST" -Path "/position-deposit" -Subject "admin" -Role "admin" -RequestId $OpId -BodyBytes $depositBodyBytes
        try {
            return Invoke-RestMethod -Uri "$BaseUri/position-deposit" -Method Post -Headers $depositAuthHeaders -Body $depositBodyBytes -TimeoutSec 10
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

# ── Snapshot metrics ─────────────────────────────────────────
function Get-MetricsSnapshot {
    try {
        return Invoke-RestMethod -Uri "$BaseUri/metrics" -Method Get -TimeoutSec 5
    } catch {
        return $null
    }
}

function Get-LatencyBreakdown {
    param($metrics)
    if (-not $metrics -or -not $metrics.latency) { return $null }
    $lat = $metrics.latency

    function Get-Pct($obj, $field) {
        if ($obj -and $obj.$field) { return $obj.$field }
        return 0
    }

    return @{
        match_e2e_p50    = Get-Pct $lat.match_e2e_us "p50_us"
        match_e2e_p95    = Get-Pct $lat.match_e2e_us "p95_us"
        match_e2e_p99    = Get-Pct $lat.match_e2e_us "p99_us"
        queue_wait_p50   = Get-Pct $lat.queue_wait_us "p50_us"
        queue_wait_p95   = Get-Pct $lat.queue_wait_us "p95_us"
        queue_wait_p99   = Get-Pct $lat.queue_wait_us "p99_us"
        match_exec_p50   = Get-Pct $lat.match_execution_us "p50_us"
        match_exec_p95   = Get-Pct $lat.match_execution_us "p95_us"
        match_exec_p99   = Get-Pct $lat.match_execution_us "p99_us"
        wal_append_p50   = Get-Pct $lat.wal_append_us "p50_us"
        wal_append_p95   = Get-Pct $lat.wal_append_us "p95_us"
        wal_append_p99   = Get-Pct $lat.wal_append_us "p99_us"
    }
}

# ── WAL snapshot ─────────────────────────────────────────────
function Get-WalSnapshot {
    $dataDir = Join-Path $PSScriptRoot "..\data"
    $walFiles = @("ledger.wal.jsonl", "sequencer.wal.jsonl", "trade_journal.wal.jsonl", "trade_settlement.wal.jsonl", "matching.snapshot.jsonl")
    $result = @{}
    foreach ($f in $walFiles) {
        $path = Join-Path $dataDir $f
        if (Test-Path $path) {
            $stat = Get-Item $path
            $lines = (Get-Content $path).Count
            $result[$f] = @{ size = $stat.Length; entries = $lines }
        } else {
            $result[$f] = @{ size = 0; entries = 0 }
        }
    }
    return $result
}

# ── Pre-flight ───────────────────────────────────────────────
Write-Host ""
Write-Host "===================================================" -ForegroundColor Cyan
Write-Host "  Advanced Benchmark Suite — Mode: $Mode" -ForegroundColor Cyan
Write-Host "  Run ID: $RunId | Started: $StartTime" -ForegroundColor Cyan
Write-Host "===================================================" -ForegroundColor Cyan
Write-Host ""

Write-Host "[PRE-FLIGHT] Checking server health..." -ForegroundColor Yellow
$health = Invoke-RestMethod -Uri "$BaseUri/health" -Method Get -TimeoutSec 5
Write-Host "  ✓ Server alive | status=$($health.status) | accounts=$($health.accounts)" -ForegroundColor Green

$initialMetrics = Get-MetricsSnapshot
$initialWal = Get-WalSnapshot
$initialLatency = Get-LatencyBreakdown $initialMetrics

Write-Host "[PRE-FLIGHT] Baseline metrics captured" -ForegroundColor Green
if ($initialLatency) {
    Write-Host "  Match E2E  p50=$($initialLatency.match_e2e_p50)μs  p99=$($initialLatency.match_e2e_p99)μs" -ForegroundColor DarkGray
    Write-Host "  Queue Wait p50=$($initialLatency.queue_wait_p50)μs  p99=$($initialLatency.queue_wait_p99)μs" -ForegroundColor DarkGray
    Write-Host "  Match Exec p50=$($initialLatency.match_exec_p50)μs  p99=$($initialLatency.match_exec_p99)μs" -ForegroundColor DarkGray
    Write-Host "  WAL Append p50=$($initialLatency.wal_append_p50)μs  p99=$($initialLatency.wal_append_p99)μs" -ForegroundColor DarkGray
}

# ── Fund test accounts ───────────────────────────────────────
function Fund-Accounts {
    param([int]$Count, [string]$Prefix = "bench")
    Write-Host "[FUNDING] Provisioning $Count accounts..." -ForegroundColor Yellow
    for ($i = 0; $i -lt $Count; $i++) {
        $userId = "${Prefix}-$i"
        # Fund cash for buying
        $cashAmount = 10000000  # 10M subunits
        $cashOpId = "bench-deposit-${Prefix}-$i-$RunId"
        try {
            $resp = Invoke-AdminDeposit -UserId $userId -Amount $cashAmount -OpId $cashOpId
        } catch {
            # Ignore deposit errors for existing accounts
        }
        # Fund position for selling (outcome 0)
        $posAmount = 1000  # 1000 units
        $posOpId = "bench-pos-deposit-${Prefix}-$i-$RunId"
        try {
            $resp = Invoke-AdminPositionDeposit -UserId $userId -MarketId "btc-usdt" -Outcome 0 -Amount $posAmount -OpId $posOpId
        } catch {
            # Ignore position deposit errors for existing accounts
        }
        if ($i % 10 -eq 9) { Start-Sleep -Milliseconds 100 }
    }
    Write-Host "  ✓ Funded $Count accounts (cash + positions)" -ForegroundColor Green
}

# ── Stress job scriptblock ───────────────────────────────────
$StressJobScript = {
    param($BaseUri, $i, $Side, $Price, $Amount, $Secret, $RunId, $MarketId, $UserId, $Period)

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

    $requestId = "req-${RunId}-p${Period}-${i}"
    $body = @{
        market_id = $MarketId
        side = $Side
        price = $Price
        amount = $Amount
        outcome = 0
        client_order_id = "bench-${RunId}-$i"
        request_id = $requestId
    } | ConvertTo-Json -Compress
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($body)

    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $payload = "POST`n/intent`n`n${UserId}`nuser`n`n${timestamp}`n${requestId}"
    $signature = Compute-HmacSignature -Message $payload -Secret $Secret
    $bodyHash = Compute-BodyHash -BodyBytes $bodyBytes

    $authHeaders = @{
        "x-internal-auth-subject"     = $UserId
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
        $fills = 0; if ($resp.fills) { $fills = $resp.fills }
        $state = "unknown"; if ($resp.order_state) { $state = $resp.order_state }
        return @{ ok = $true; ms = $sw.ElapsedMilliseconds; fills = $fills; state = $state }
    } catch {
        $sw.Stop()
        return @{ ok = $false; ms = $sw.ElapsedMilliseconds; error = $_.Exception.Message }
    }
}

# ── Cancel job scriptblock ───────────────────────────────────
$CancelJobScript = {
    param($BaseUri, $OrderId, $UserId, $Secret, $RunId)

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

    $requestId = "cancel-${RunId}-${OrderId}"
    $body = @{ order_id = $OrderId; request_id = $requestId } | ConvertTo-Json -Compress
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($body)

    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $payload = "POST`n/cancel`n`n${UserId}`nuser`n`n${timestamp}`n${requestId}"
    $signature = Compute-HmacSignature -Message $payload -Secret $Secret
    $bodyHash = Compute-BodyHash -BodyBytes $bodyBytes

    $authHeaders = @{
        "x-internal-auth-subject"     = $UserId
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
        $resp = Invoke-RestMethod -Uri "$BaseUri/cancel" -Method Post -Headers $authHeaders -Body $bodyBytes -TimeoutSec 10
        $sw.Stop()
        return @{ ok = $true; ms = $sw.ElapsedMilliseconds }
    } catch {
        $sw.Stop()
        return @{ ok = $false; ms = $sw.ElapsedMilliseconds; error = $_.Exception.Message }
    }
}

# ── Run batched stress test ──────────────────────────────────
function Run-StressBatch {
    param(
        [int]$OrderCount,
        [int]$Concurrency,
        [string]$MarketId = "btc-usdt",
        [bool]$BalancedSides = $true,
        [hashtable]$PriceLevels = $null,
        [int]$AccountCount = 50,
        [int]$Period = 0,
        [switch]$Silent
    )

    $accountCount = [Math]::Min($OrderCount, $AccountCount)
    Fund-Accounts -Count $accountCount -Prefix "bench"

    # Price level distribution
    if (-not $PriceLevels) {
        # Default: multi-layer around 50000
        $PriceLevels = @{
            buy  = @(49500, 49600, 49700, 49800, 49900, 50000, 50100)
            sell = @(50000, 50100, 50200, 50300, 50400, 50500, 50600)
        }
    }

    $allLatencies = @()
    $successCount = 0
    $failCount = 0
    $fillCount = 0
    $orderIds = @()  # Track for cancel storm

    $batches = [Math]::Ceiling($OrderCount / $Concurrency)
    $sampleFailures = @()

    for ($batch = 0; $batch -lt $batches; $batch++) {
        $tasks = @()
        $startIdx = $batch * $Concurrency
        $endIdx = [Math]::Min($startIdx + $Concurrency, $OrderCount)

        for ($i = $startIdx; $i -lt $endIdx; $i++) {
            # 50/50 buy/sell split
            if ($BalancedSides) {
                $side = if ($i % 2 -eq 0) { "buy" } else { "sell" }
            } else {
                $side = "buy"
            }

            $userId = "bench-$($i % $accountCount)"
            $priceArr = $PriceLevels[$side]
            $price = $priceArr[$i % $priceArr.Count]
            $amount = 1 + ($i % 3)

            $tasks += Start-Job -ScriptBlock $StressJobScript -ArgumentList @(
                $BaseUri, $i, $side, $price, $amount, "dev-secret-change-me", $RunId, $MarketId, $userId, $period
            )
        }

        $tasks | Wait-Job | ForEach-Object {
            $result = Receive-Job $_
            Remove-Job $_
            $allLatencies += $result.ms
            if ($result.ok) {
                $successCount++
                if ($result.fills -gt 0) { $fillCount += $result.fills }
            } else {
                $failCount++
                if ($sampleFailures.Count -lt 5) {
                    $sampleFailures += $result.error
                }
            }
        }

        if (-not $Silent) {
            $pct = [Math]::Round(($endIdx / $OrderCount) * 100)
            Write-Host "  Progress: $endIdx/$OrderCount (${pct}%)" -ForegroundColor DarkGray
        }
    }

    $sorted = $allLatencies | Sort-Object
    $p50 = if ($sorted.Count -gt 0) { $sorted[[Math]::Floor($sorted.Count * 0.50)] } else { 0 }
    $p95 = if ($sorted.Count -gt 0) { $sorted[[Math]::Floor($sorted.Count * 0.95)] } else { 0 }
    $p99 = if ($sorted.Count -gt 0) { $sorted[[Math]::Floor($sorted.Count * 0.99)] } else { 0 }
    $avg = if ($allLatencies.Count -gt 0) { [Math]::Round(($allLatencies | Measure-Object -Average).Average) } else { 0 }
    $min = if ($allLatencies.Count -gt 0) { ($allLatencies | Measure-Object -Minimum).Minimum } else { 0 }
    $max = if ($allLatencies.Count -gt 0) { ($allLatencies | Measure-Object -Maximum).Maximum } else { 0 }

    return @{
        latencies  = $allLatencies
        success    = $successCount
        failed     = $failCount
        fills      = $fillCount
        p50        = $p50
        p95        = $p95
        p99        = $p99
        avg        = $avg
        min        = $min
        max        = $max
        sampleErrors = $sampleFailures
    }
}

# ===========================================================
# MODE DISPATCH
# ===========================================================

switch ($Mode) {

    # ── Quick Stress ──────────────────────────────────────────
    "Quick" {
        Write-Host "`n[QUICK STRESS] $OrderCount orders @ concurrency=$Concurrency" -ForegroundColor Yellow
        $result = Run-StressBatch -OrderCount $OrderCount -Concurrency $Concurrency -BalancedSides $true

        Write-Host "`n  ── Results ──" -ForegroundColor Cyan
        Write-Host "  Success=$($result.success) | Failed=$($result.failed) | Fills=$($result.fills)" -ForegroundColor White
        Write-Host "  Latency (ms): P50=$($result.p50) | P95=$($result.p95) | P99=$($result.p99) | Avg=$($result.avg) | [$($result.min)-$($result.max)]" -ForegroundColor White

        if ($result.sampleErrors.Count -gt 0) {
            Write-Host "  Sample errors:" -ForegroundColor Red
            foreach ($err in $result.sampleErrors) {
                Write-Host "    - $err" -ForegroundColor DarkGray
            }
        }
    }

    # ── Soak Test ─────────────────────────────────────────────
    "Soak" {
        $totalSeconds = $DurationMin * 60
        $endTime = (Get-Date).AddSeconds($totalSeconds)
        Write-Host "`n[SOAK TEST] Running for ${DurationMin} minutes ($totalSeconds seconds)" -ForegroundColor Yellow
        Write-Host "  Target: $Concurrency concurrent, continuous 50/50 buy/sell" -ForegroundColor DarkGray

        $periodSeconds = 30  # Report every 30s
        $ordersPerPeriod = $Concurrency * 4  # ~4 batches per period
        $periodResults = @()
        $totalSuccess = 0
        $totalFailed = 0
        $totalFills = 0
        $allLatencies = @()

        Fund-Accounts -Count 50 -Prefix "soak"

        $iteration = 0
        while ((Get-Date) -lt $endTime) {
            $iteration++
            $remaining = [Math]::Round(($endTime - (Get-Date)).TotalMinutes, 1)
            Write-Host "`n  [Period $iteration] ${remaining}min remaining..." -ForegroundColor DarkGray

            $periodResult = Run-StressBatch -OrderCount $ordersPerPeriod -Concurrency $Concurrency -BalancedSides $true -Period $iteration -Silent
            $periodResults += @{
                period   = $iteration
                success  = $periodResult.success
                failed   = $periodResult.failed
                fills    = $periodResult.fills
                p50      = $periodResult.p50
                p95      = $periodResult.p95
                p99      = $periodResult.p99
            }

            $totalSuccess += $periodResult.success
            $totalFailed += $periodResult.failed
            $totalFills += $periodResult.fills
            $allLatencies += $periodResult.latencies

            # Capture latency snapshot
            $snap = Get-MetricsSnapshot
            if ($snap -and $snap.latency) {
                $lat = $snap.latency
                Write-Host "    Queue Wait p99=$($lat.queue_wait_us.p99)μs | Match Exec p99=$($lat.match_execution_us.p99)μs | WAL p99=$($lat.wal_append_us.p99)μs" -ForegroundColor DarkGray
            }
        }

        # Final summary
        $sorted = $allLatencies | Sort-Object
        $overallP50 = if ($sorted.Count -gt 0) { $sorted[[Math]::Floor($sorted.Count * 0.50)] } else { 0 }
        $overallP95 = if ($sorted.Count -gt 0) { $sorted[[Math]::Floor($sorted.Count * 0.95)] } else { 0 }
        $overallP99 = if ($sorted.Count -gt 0) { $sorted[[Math]::Floor($sorted.Count * 0.99)] } else { 0 }

        Write-Host "`n  ===========================================" -ForegroundColor Cyan
        Write-Host "  SOAK TEST SUMMARY (${DurationMin} min)" -ForegroundColor Cyan
        Write-Host "  ===========================================" -ForegroundColor Cyan
        Write-Host "  Total: Success=${totalSuccess} | Failed=${totalFailed} | Fills=${totalFills}" -ForegroundColor White
        Write-Host "  Overall Latency: P50=${overallP50}ms | P95=${overallP95}ms | P99=${overallP99}ms" -ForegroundColor White

        # Tail latency degradation analysis
        Write-Host "`n  ── Tail Latency Trend (P99 per period) ──" -ForegroundColor Yellow
        foreach ($pr in $periodResults) {
            Write-Host "    Period $($pr.period): P50=$($pr.p50)ms | P95=$($pr.p95)ms | P99=$($pr.p99)ms" -ForegroundColor DarkGray
        }

        # Check for degradation
        if ($periodResults.Count -ge 2) {
            $firstP99 = $periodResults[0].p99
            $lastP99 = $periodResults[-1].p99
            if ($lastP99 -gt ($firstP99 * 2)) {
                Write-Host "  ⚠ TAIL DEGRADATION DETECTED: P99 ${firstP99}ms → ${lastP99}ms ($([Math]::Round($lastP99/$firstP99))x)" -ForegroundColor Red
            } else {
                Write-Host "  ✓ Tail latency stable: P99 ${firstP99}ms → ${lastP99}ms" -ForegroundColor Green
            }
        }
    }

    # ── Concurrency Sweep ─────────────────────────────────────
    "ConcurrencySweep" {
        $levels = @(1, 4, 8, 16, 32)
        Write-Host "`n[CONCURRENCY SWEEP] Testing levels: $($levels -join ', ')" -ForegroundColor Yellow
        Write-Host "  Orders per level: $OrderCount | Balanced 50/50 buy/sell" -ForegroundColor DarkGray

        $sweepResults = @()

        foreach ($level in $levels) {
            Write-Host "`n  ── Concurrency: $level ──" -ForegroundColor Yellow

            # Reset metrics between runs
            $beforeMetrics = Get-MetricsSnapshot

            $result = Run-StressBatch -OrderCount $OrderCount -Concurrency $level -BalancedSides $true -Silent

            $afterMetrics = Get-MetricsSnapshot
            $deltaOrders = [long]$afterMetrics.orders_received - [long]$beforeMetrics.orders_received
            $deltaFills = [long]$afterMetrics.orders_filled - [long]$beforeMetrics.orders_filled

            $sweepResults += @{
                concurrency = $level
                success     = $result.success
                failed      = $result.failed
                fills       = $result.fills
                p50         = $result.p50
                p95         = $result.p95
                p99         = $result.p99
                avg         = $result.avg
                throughput  = if ($result.avg -gt 0) { [Math]::Round(1000 / $result.avg, 1) } else { 0 }
                deltaOrders = $deltaOrders
                deltaFills  = $deltaFills
            }

            Write-Host "    Success=$($result.success)/$OrderCount | Fills=$($result.fills)" -ForegroundColor White
            Write-Host "    Latency: P50=$($result.p50)ms | P95=$($result.p95)ms | P99=$($result.p99)ms" -ForegroundColor White
            Write-Host "    Est. throughput: $(if ($result.avg -gt 0) { [Math]::Round(1000 / $result.avg, 1) } else { 'N/A' }) orders/sec" -ForegroundColor DarkGray

            # Capture latency breakdown
            $latBreakdown = Get-LatencyBreakdown $afterMetrics
            if ($latBreakdown) {
                Write-Host "    Queue Wait p99=$($latBreakdown.queue_wait_p99)μs | Match Exec p99=$($latBreakdown.match_exec_p99)μs | WAL p99=$($latBreakdown.wal_append_p99)μs" -ForegroundColor DarkGray
            }
        }

        # Summary table
        Write-Host "`n  ===============================================================================" -ForegroundColor Cyan
        Write-Host "  CONCURRENCY SWEEP SUMMARY" -ForegroundColor Cyan
        Write-Host "  ===============================================================================" -ForegroundColor Cyan
        Write-Host ("  {0,-12} {1,8} {2,8} {3,8} {4,8} {5,8} {6,10} {7,10}" -f "Concurrency", "Success", "Fills", "P50(ms)", "P95(ms)", "P99(ms)", "Thr(op/s)", "Fill Rate") -ForegroundColor White
        Write-Host "  ---------------------------------------------------------------------------" -ForegroundColor DarkGray
        foreach ($r in $sweepResults) {
            $fillRate = if ($r.success -gt 0) { [Math]::Round($r.fills / $r.success * 100, 1) } else { 0 }
            Write-Host ("  {0,-12} {1,8} {2,8} {3,8} {4,8} {5,8} {6,10} {7,9}%" -f $r.concurrency, $r.success, $r.fills, $r.p50, $r.p95, $r.p99, $r.throughput, $fillRate) -ForegroundColor White
        }
    }

    # ── Hot Market ────────────────────────────────────────────
    "HotMarket" {
        Write-Host "`n[HOT MARKET] Scenario: $Scenario" -ForegroundColor Yellow

        switch ($Scenario) {
            "SingleMarketBlast" {
                Write-Host "  Target: btc-usdt | 50/50 buy/sell | Multi-price layers" -ForegroundColor DarkGray
                Fund-Accounts -Count 100 -Prefix "hot"

                $priceLevels = @{
                    buy  = @(49000, 49200, 49400, 49600, 49800, 49900, 50000)
                    sell = @(50000, 50100, 50200, 50400, 50600, 50800, 51000)
                }

                $result = Run-StressBatch `
                    -OrderCount $OrderCount `
                    -Concurrency $Concurrency `
                    -MarketId "btc-usdt" `
                    -BalancedSides $true `
                    -PriceLevels $priceLevels `
                    -AccountCount 100

                Write-Host "`n  ── Results ──" -ForegroundColor Cyan
                Write-Host "  Success=$($result.success) | Failed=$($result.failed) | Fills=$($result.fills)" -ForegroundColor White
                Write-Host "  Latency: P50=$($result.p50)ms | P95=$($result.p95)ms | P99=$($result.p99)ms" -ForegroundColor White
            }

            "CancelStorm" {
                Write-Host "  Phase 1: Place orders | Phase 2: Mass cancel" -ForegroundColor DarkGray
                Fund-Accounts -Count 50 -Prefix "cancel"

                # Phase 1: Place orders
                Write-Host "`n  [Phase 1] Placing $OrderCount orders to build book..." -ForegroundColor Yellow
                $placeResult = Run-StressBatch -OrderCount $OrderCount -Concurrency $Concurrency -BalancedSides $true -Silent
                Write-Host "  Placed: Success=$($placeResult.success) | Fills=$($placeResult.fills)" -ForegroundColor DarkGray

                # Phase 2: Cancel storm
                Write-Host "`n  [Phase 2] Cancel storm: $Concurrency concurrent cancels..." -ForegroundColor Yellow
                $cancelLatencies = @()
                $cancelSuccess = 0
                $cancelFailed = 0

                $cancelBatches = [Math]::Ceiling($OrderCount / $Concurrency)
                for ($batch = 0; $batch -lt $cancelBatches; $batch++) {
                    $tasks = @()
                    $startIdx = $batch * $Concurrency
                    $endIdx = [Math]::Min($startIdx + $Concurrency, $OrderCount)

                    for ($i = $startIdx; $i -lt $endIdx; $i++) {
                        $orderId = "bench-${RunId}-$i"
                        $userId = "cancel-$($i % 50)"
                        $tasks += Start-Job -ScriptBlock $CancelJobScript -ArgumentList @(
                            $BaseUri, $orderId, $userId, "dev-secret-change-me", $RunId
                        )
                    }

                    $tasks | Wait-Job | ForEach-Object {
                        $result = Receive-Job $_
                        Remove-Job $_
                        $cancelLatencies += $result.ms
                        if ($result.ok) { $cancelSuccess++ } else { $cancelFailed++ }
                    }
                }

                $sortedCancel = $cancelLatencies | Sort-Object
                $cancelP50 = if ($sortedCancel.Count -gt 0) { $sortedCancel[[Math]::Floor($sortedCancel.Count * 0.50)] } else { 0 }
                $cancelP99 = if ($sortedCancel.Count -gt 0) { $sortedCancel[[Math]::Floor($sortedCancel.Count * 0.99)] } else { 0 }

                Write-Host "`n  ── Cancel Storm Results ──" -ForegroundColor Cyan
                Write-Host "  Cancelled: $cancelSuccess | Failed: $cancelFailed" -ForegroundColor White
                Write-Host "  Cancel Latency: P50=$cancelP50ms | P99=$cancelP99ms" -ForegroundColor White
            }

            "HighFreqAccounts" {
                Write-Host "  5 hyper-active accounts, rapid-fire orders" -ForegroundColor DarkGray
                Fund-Accounts -Count 5 -Prefix "hf"

                $hfLatencies = @()
                $hfSuccess = 0
                $hfFailed = 0
                $hfFills = 0

                # Each of 5 accounts sends OrderCount/5 orders sequentially
                for ($acct = 0; $acct -lt 5; $acct++) {
                    $userId = "hf-$acct"
                    $ordersPerAcct = [Math]::Ceiling($OrderCount / 5)
                    Write-Host "  Account hf-${acct}: ${ordersPerAcct} orders..." -ForegroundColor DarkGray

                    for ($i = 0; $i -lt $ordersPerAcct; $i++) {
                        $side = if (($acct + $i) % 2 -eq 0) { "buy" } else { "sell" }
                        $price = if ($side -eq "buy") { 49500 + ($i % 200) } else { 50100 + ($i % 200) }
                        $amount = 1 + ($i % 2)
                        $requestId = "hf-${RunId}-${acct}-${i}"

                        $body = @{
                            market_id = "btc-usdt"
                            side = $side
                            price = $price
                            amount = $amount
                            outcome = 0
                            client_order_id = "hf-${acct}-${i}"
                            request_id = $requestId
                        } | ConvertTo-Json -Compress
                        $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($body)
                        $authHeaders = Build-AuthHeaders -Method "POST" -Path "/intent" -Subject $userId -RequestId $requestId -BodyBytes $bodyBytes

                        $sw = [System.Diagnostics.Stopwatch]::StartNew()
                        try {
                            $resp = Invoke-RestMethod -Uri "$BaseUri/intent" -Method Post -Headers $authHeaders -Body $bodyBytes -TimeoutSec 10
                            $sw.Stop()
                            $hfLatencies += $sw.ElapsedMilliseconds
                            $hfSuccess++
                            if ($resp.fills -gt 0) { $hfFills += $resp.fills }
                        } catch {
                            $sw.Stop()
                            $hfLatencies += $sw.ElapsedMilliseconds
                            $hfFailed++
                        }
                    }
                }

                $sortedHf = $hfLatencies | Sort-Object
                $hfP50 = if ($sortedHf.Count -gt 0) { $sortedHf[[Math]::Floor($sortedHf.Count * 0.50)] } else { 0 }
                $hfP95 = if ($sortedHf.Count -gt 0) { $sortedHf[[Math]::Floor($sortedHf.Count * 0.95)] } else { 0 }
                $hfP99 = if ($sortedHf.Count -gt 0) { $sortedHf[[Math]::Floor($sortedHf.Count * 0.99)] } else { 0 }

                Write-Host "`n  ── High Frequency Account Results ──" -ForegroundColor Cyan
                Write-Host "  Success=${hfSuccess} | Failed=${hfFailed} | Fills=${hfFills}" -ForegroundColor White
                Write-Host "  Latency: P50=${hfP50}ms | P95=${hfP95}ms | P99=${hfP99}ms" -ForegroundColor White
            }
        }
    }
}

# ===========================================================
# FINAL REPORT
# ===========================================================

Write-Host "`n===================================================" -ForegroundColor Cyan
Write-Host "  FINAL REPORT" -ForegroundColor Cyan
Write-Host "===================================================" -ForegroundColor Cyan

$finalMetrics = Get-MetricsSnapshot
$finalWal = Get-WalSnapshot
$finalLatency = Get-LatencyBreakdown $finalMetrics

if ($initialMetrics -and $finalMetrics) {
    $deltaOrders = [long]$finalMetrics.orders_received - [long]$initialMetrics.orders_received
    $deltaFills = [long]$finalMetrics.orders_filled - [long]$initialMetrics.orders_filled
    $deltaRejected = [long]$finalMetrics.orders_rejected - [long]$initialMetrics.orders_rejected

    Write-Host "`n  ── Metrics Delta ──" -ForegroundColor Yellow
        Write-Host "  Orders Received:  +${deltaOrders}" -ForegroundColor White
        Write-Host "  Orders Filled:    +${deltaFills}" -ForegroundColor White
        Write-Host "  Orders Rejected:  +${deltaRejected}" -ForegroundColor White
}

if ($initialLatency -and $finalLatency) {
    Write-Host "`n  ── Latency Breakdown (Server-Side) ──" -ForegroundColor Yellow
    Write-Host ("  {0,-16} {1,10} {2,10} {3,10}" -f "Dimension", "P50 (μs)", "P95 (μs)", "P99 (μs)") -ForegroundColor White
    Write-Host "  ------------------------------------------------" -ForegroundColor DarkGray
    Write-Host ("  {0,-16} {1,10} {2,10} {3,10}" -f "Match E2E", $finalLatency.match_e2e_p50, $finalLatency.match_e2e_p95, $finalLatency.match_e2e_p99) -ForegroundColor White
    Write-Host ("  {0,-16} {1,10} {2,10} {3,10}" -f "Queue Wait", $finalLatency.queue_wait_p50, $finalLatency.queue_wait_p95, $finalLatency.queue_wait_p99) -ForegroundColor White
    Write-Host ("  {0,-16} {1,10} {2,10} {3,10}" -f "Match Exec", $finalLatency.match_exec_p50, $finalLatency.match_exec_p95, $finalLatency.match_exec_p99) -ForegroundColor White
    Write-Host ("  {0,-16} {1,10} {2,10} {3,10}" -f "WAL Append", $finalLatency.wal_append_p50, $finalLatency.wal_append_p95, $finalLatency.wal_append_p99) -ForegroundColor White
}

Write-Host "`n  ── WAL Growth ──" -ForegroundColor Yellow
foreach ($f in $finalWal.Keys) {
        $initSize = if ($initialWal.ContainsKey($f)) { $initialWal[$f].size } else { 0 }
        $deltaSize = $finalWal[$f].size - $initSize
        $indicator = if ($deltaSize -gt 0) { "+" } else { "" }
        Write-Host "  ${f} : $($finalWal[$f].entries) entries | ${indicator}${deltaSize} bytes" -ForegroundColor White
}

$elapsed = (Get-Date) - $StartTime
Write-Host "`n  Total elapsed: $([Math]::Round($elapsed.TotalSeconds, 1))s" -ForegroundColor DarkGray
Write-Host ""
