<#
.SYNOPSIS
    HTTP对照实验: Benchmark /submit-order endpoint with granular_timing extraction.
    
.DESCRIPTION
    Runs 4 benchmark scenarios against the API server, measuring both:
    - Wall-clock latency (client-perceived P50/P95/P99)
    - Server-side pipeline breakdown (queue_wait_us, risk_us, matching_core_us, settlement_persist_us, post_match_us)
    
    Each scenario runs warmup rounds followed by formal measurement rounds.
    
.PARAMETER BaseUrl
    API server base URL. Default: http://localhost:3030
    
.PARAMETER Secret
    HMAC auth secret. Default: dev-secret-change-me
    
.PARAMETER WarmupRounds
    Number of warmup rounds per scenario. Default: 5
    
.PARAMETER FormalRounds
    Number of formal measurement rounds per scenario. Default: 20
#>

param(
    [string]$BaseUrl = "http://localhost:3030",
    [string]$Secret = "dev-secret-change-me-to-32-chars-min!",
    [int]$WarmupRounds = 5,
    [int]$FormalRounds = 20
)

$ErrorActionPreference = "Stop"

# ============================================================
# HMAC-SHA256 Signing
# ============================================================
function Sign-Request {
    param([string]$Method, [string]$Path, [string]$Timestamp, [string]$RequestId, [string]$Subject = "admin", [string]$Role = "admin", [string]$SessionId = "")
    
    # Signature payload format: Method\nPath\n\nSubject\nRole\nSessionId\ntimestamp\nRequestId
    $payload = "${Method}`n${Path}`n`n${Subject}`n${Role}`n${SessionId}`n${Timestamp}`n${RequestId}"
    $keyBytes = [System.Text.Encoding]::UTF8.GetBytes($Secret)
    $payloadBytes = [System.Text.Encoding]::UTF8.GetBytes($payload)
    
    $hmac = [System.Security.Cryptography.HMACSHA256]::new($keyBytes)
    $hashBytes = $hmac.ComputeHash($payloadBytes)
    $signature = [BitConverter]::ToString($hashBytes).Replace("-", "").ToLower()
    $hmac.Dispose()
    
    return $signature
}

# ============================================================
# SHA256 Body Hash
# ============================================================
function Compute-BodyHash {
    param([byte[]]$BodyBytes)
    $hash = [System.Security.Cryptography.SHA256]::Create()
    $hashBytes = $hash.ComputeHash($BodyBytes)
    $hash.Dispose()
    return [BitConverter]::ToString($hashBytes).Replace("-", "").ToLowerInvariant()
}

# ============================================================
# HTTP Client Setup
# ============================================================

# Pre-warm connection
try {
    $null = Invoke-WebRequest -Uri "$BaseUrl/health" -Method GET -UseBasicParsing -TimeoutSec 5 -ErrorAction Stop
    Write-Host "[INFO] Server health check OK"
} catch {
    Write-Host "[ERROR] Cannot reach server at $BaseUrl. Is it running?" -ForegroundColor Red
    Write-Host "[ERROR] Start with: cargo run --package api --release" -ForegroundColor Red
    exit 1
}

# ============================================================
# Submit Order Helper
# ============================================================
$script:RequestCounter = 0

function Submit-Order {
    param(
        [hashtable]$Order,
        [ref]$WallClockUs,
        [ref]$GranularTiming
    )
    
    # Inject unique client_order_id to avoid idempotency conflicts
    $Order["client_order_id"] = [Guid]::NewGuid().ToString("N")
    
    $bodyJson = $Order | ConvertTo-Json -Compress
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($bodyJson)
    $bodyHash = Compute-BodyHash -BodyBytes $bodyBytes
    
    $script:RequestCounter++
    $requestId = "bench-$($script:RequestCounter)-$(Get-Date -Format 'yyyyMMddHHmmssfff')"
    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $signature = Sign-Request -Method "POST" -Path "/submit-order" -Timestamp $timestamp -RequestId $requestId -Subject "admin" -Role "admin"
    
    $headers = @{
        "Content-Type" = "application/json"
        "x-internal-auth-subject" = "admin"
        "x-internal-auth-role" = "admin"
        "x-internal-auth-session-id" = ""
        "x-internal-auth-timestamp" = $timestamp
        "x-internal-auth-signature" = $signature
        "x-internal-auth-body-sha256" = $bodyHash
        "x-request-id" = $requestId
    }
    
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $response = Invoke-WebRequest -Uri "$BaseUrl/submit-order" -Method POST -Headers $headers -Body $bodyBytes -UseBasicParsing -TimeoutSec 30 -ErrorAction Stop
        $sw.Stop()
        
        $wallClockUs.Value = $sw.ElapsedTicks * 1000000 / [System.Diagnostics.Stopwatch]::Frequency
        
        if ($response.StatusCode -eq 200) {
            $respJson = $response.Content | ConvertFrom-Json
            if ($respJson.granular_timing) {
                $GranularTiming.Value = $respJson.granular_timing
            }
            return $true
        } else {
            $displayLen = [Math]::Min(300, $response.Content.Length)
            Write-Host "[WARN] HTTP $($response.StatusCode): $($response.Content.Substring(0, $displayLen))" -ForegroundColor Yellow
            return $false
        }
    } catch {
        $sw.Stop()
        $wallClockUs.Value = $sw.ElapsedTicks * 1000000 / [System.Diagnostics.Stopwatch]::Frequency
        if ($_.Exception.Response) {
            try {
                $reader = [System.IO.StreamReader]::new($_.Exception.Response.GetResponseStream())
                $respBody = $reader.ReadToEnd()
                $reader.Close()
                $displayLen = [Math]::Min(300, $respBody.Length)
                Write-Host "[WARN] HTTP $($_.Exception.Response.StatusCode): $($respBody.Substring(0, $displayLen))" -ForegroundColor Yellow
            } catch {
                Write-Host "[WARN] HTTP $($_.Exception.Response.StatusCode)" -ForegroundColor Yellow
            }
        } else {
            Write-Host "[ERROR] Request failed: $_" -ForegroundColor Red
        }
        return $false
    }
}

# ============================================================
# Statistics Helpers
# ============================================================
function Get-Percentile {
    param([double[]]$Values, [double]$Percentile)
    if ($Values.Count -eq 0) { return 0 }
    $sorted = $Values | Sort-Object
    $index = [Math]::Ceiling($Percentile / 100.0 * $sorted.Count) - 1
    if ($index -lt 0) { $index = 0 }
    if ($index -ge $sorted.Count) { $index = $sorted.Count - 1 }
    return $sorted[$index]
}

function Format-Us {
    param([double]$Us)
    if ($Us -ge 1000000) { return "{0:F2}s" -f ($Us / 1000000) }
    if ($Us -ge 1000) { return "{0:F2}ms" -f ($Us / 1000) }
    return "{0:F2}µs" -f $Us
}

function Print-Stats {
    param([string]$Label, [double[]]$Values)
    if ($Values.Count -eq 0) { return }
    $p50 = Get-Percentile -Values $Values -Percentile 50
    $p95 = Get-Percentile -Values $Values -Percentile 95
    $p99 = Get-Percentile -Values $Values -Percentile 99
    $mean = ($Values | Measure-Object -Average).Average
    $min = ($Values | Measure-Object -Minimum).Minimum
    $max = ($Values | Measure-Object -Maximum).Maximum
    
    Write-Host "  $Label :" -ForegroundColor Cyan
    Write-Host "    Min=$(Format-Us $min)  P50=$(Format-Us $p50)  P95=$(Format-Us $p95)  P99=$(Format-Us $p99)  Max=$(Format-Us $max)  Mean=$(Format-Us $mean)"
}

# ============================================================
# Scenario Definitions
# ============================================================
$scenarios = @(
    @{
        Name = "single_market"
        Description = "BTC-USDT only, sequential buy orders (no position required)"
        Orders = @(
            @{ market_id = "btc-usdt"; side = "buy"; order_type = "limit"; price = 49000; amount = 1; outcome = 1; time_in_force = "gtc" },
            @{ market_id = "btc-usdt"; side = "buy"; order_type = "limit"; price = 48900; amount = 1; outcome = 1; time_in_force = "gtc" },
            @{ market_id = "btc-usdt"; side = "buy"; order_type = "limit"; price = 48800; amount = 1; outcome = 1; time_in_force = "gtc" },
            @{ market_id = "btc-usdt"; side = "buy"; order_type = "limit"; price = 48700; amount = 1; outcome = 1; time_in_force = "gtc" },
            @{ market_id = "btc-usdt"; side = "buy"; order_type = "limit"; price = 48600; amount = 1; outcome = 1; time_in_force = "gtc" }
        )
    },
    @{
        Name = "two_markets"
        Description = "BTC-USDT / ETH-USDT alternating, buy orders only"
        Orders = @(
            @{ market_id = "btc-usdt"; side = "buy"; order_type = "limit"; price = 49000; amount = 1; outcome = 1; time_in_force = "gtc" },
            @{ market_id = "eth-usdt"; side = "buy"; order_type = "limit"; price = 2900; amount = 10; outcome = 1; time_in_force = "gtc" },
            @{ market_id = "btc-usdt"; side = "buy"; order_type = "limit"; price = 48900; amount = 1; outcome = 1; time_in_force = "gtc" },
            @{ market_id = "eth-usdt"; side = "buy"; order_type = "limit"; price = 2890; amount = 10; outcome = 1; time_in_force = "gtc" },
            @{ market_id = "btc-usdt"; side = "buy"; order_type = "limit"; price = 48800; amount = 1; outcome = 1; time_in_force = "gtc" }
        )
    },
    @{
        Name = "batch_50"
        Description = "50 buy orders in rapid succession (staggered to avoid rate limit)"
        Orders = @(for ($i = 0; $i -lt 50; $i++) {
            $price = 48000 - $i * 10
            @{ market_id = "btc-usdt"; side = "buy"; order_type = "limit"; price = $price; amount = 1; outcome = 1; time_in_force = "gtc" }
        })
        StaggerMs = 500  # ms between requests to avoid rate limiting
    },
    @{
        Name = "cancel_replace"
        Description = "Submit -> Cancel -> Resubmit cycle"
        Orders = @(
            @{ market_id = "btc-usdt"; side = "buy"; order_type = "limit"; price = 47000; amount = 1; outcome = 1; time_in_force = "gtc" }
        )
        # Special handling: submit first, then cancel, then resubmit
    }
)

# ============================================================
# Benchmark Runner
# ============================================================
Write-Host "`n============================================================" -ForegroundColor Green
Write-Host "  HTTP BENCHMARK: /submit-order Endpoint" -ForegroundColor Green
Write-Host "  Server: $BaseUrl" -ForegroundColor Green
Write-Host "  Warmup: $WarmupRounds rounds | Formal: $FormalRounds rounds" -ForegroundColor Green
Write-Host "============================================================`n" -ForegroundColor Green

$allResults = @()

foreach ($scenario in $scenarios) {
    Write-Host "--- Scenario: $($scenario.Name) ---" -ForegroundColor Yellow
    Write-Host "    $($scenario.Description)" -ForegroundColor Gray
    
    $wallClockTimes = @()
    $serverQueueWaits = @()
    $serverRiskTimes = @()
    $serverMatchTimes = @()
    $serverSettlementTimes = @()
    $serverPostMatchTimes = @()
    $successCount = 0
    $totalOrders = 0
    
    foreach ($phase in @("warmup", "formal")) {
        $rounds = if ($phase -eq "warmup") { $WarmupRounds } else { $FormalRounds }
        $label = if ($phase -eq "warmup") { "Warmup" } else { "Formal" }
        
        Write-Host "    [$label] Running $rounds rounds..." -NoNewline
        
        for ($r = 0; $r -lt $rounds; $r++) {
            foreach ($order in $scenario.Orders) {
                $wc = 0; $gt = $null
                $wcRef = [ref]$wc; $gtRef = [ref]$gt
                
                $ok = Submit-Order -Order $order -WallClockUs $wcRef -GranularTiming $gtRef
                $totalOrders++

                # Apply stagger delay if configured (avoids rate limiting)
                if ($scenario.StaggerMs -gt 0) {
                    Start-Sleep -Milliseconds $scenario.StaggerMs
                }
                
                if ($ok) {
                    $successCount++
                    if ($phase -eq "formal") {
                        $wallClockTimes += $wc
                        if ($gt) {
                            if ($gt.queue_wait_us) { $serverQueueWaits += [double]$gt.queue_wait_us }
                            if ($gt.risk_us) { $serverRiskTimes += [double]$gt.risk_us }
                            if ($gt.matching_core_us) { $serverMatchTimes += [double]$gt.matching_core_us }
                            if ($gt.settlement_persist_us) { $serverSettlementTimes += [double]$gt.settlement_persist_us }
                            if ($gt.post_match_us) { $serverPostMatchTimes += [double]$gt.post_match_us }
                        }
                    }
                }
            }
        }
        
        Write-Host " Done ($($rounds * $scenario.Orders.Count) orders)" -ForegroundColor Green
    }
    
    Write-Host ""
    Write-Host "  === Results: $($scenario.Name) ===" -ForegroundColor White
    Write-Host "  Success Rate: $successCount/$totalOrders ($([math]::Round($successCount*100.0/[math]::Max($totalOrders,1), 1))%)" -ForegroundColor White
    Write-Host ""
    
    # Wall-clock stats
    Print-Stats -Label "Wall-Clock (client)" -Values $wallClockTimes
    
    # Server-side granular timing stats
    if ($serverQueueWaits.Count -gt 0) {
        Write-Host ""
        Write-Host "  Server Pipeline Breakdown:" -ForegroundColor Cyan
        Print-Stats -Label "queue_wait" -Values $serverQueueWaits
        Print-Stats -Label "risk_check" -Values $serverRiskTimes
        Print-Stats -Label "matching_core" -Values $serverMatchTimes
        Print-Stats -Label "settlement_persist" -Values $serverSettlementTimes
        Print-Stats -Label "post_match" -Values $serverPostMatchTimes
        
        # Total pipeline time (sum of stages)
        $totalPipeline = @()
        $minCount = [math]::Min([math]::Min([math]::Min($serverQueueWaits.Count, $serverRiskTimes.Count), $serverMatchTimes.Count), $serverSettlementTimes.Count)
        for ($i = 0; $i -lt $minCount; $i++) {
            $totalPipeline += $serverQueueWaits[$i] + $serverRiskTimes[$i] + $serverMatchTimes[$i] + $serverSettlementTimes[$i] + $serverPostMatchTimes[$i]
        }
        Write-Host ""
        Print-Stats -Label "total_pipeline (sum)" -Values $totalPipeline
        
        # API tax = wall-clock - pipeline_total
        $apiTax = @()
        $minTaxCount = [math]::Min($wallClockTimes.Count, $totalPipeline.Count)
        for ($i = 0; $i -lt $minTaxCount; $i++) {
            $tax = $wallClockTimes[$i] - $totalPipeline[$i]
            if ($tax -gt 0) { $apiTax += $tax }
        }
        if ($apiTax.Count -gt 0) {
            Write-Host ""
            Print-Stats -Label "API_tax (wall - pipeline)" -Values $apiTax
        }
    }
    
    Write-Host "`n"
    
    $allResults += @{
        Scenario = $scenario.Name
        WallClockP50 = if ($wallClockTimes.Count -gt 0) { Get-Percentile -Values $wallClockTimes -Percentile 50 } else { 0 }
        WallClockP95 = if ($wallClockTimes.Count -gt 0) { Get-Percentile -Values $wallClockTimes -Percentile 95 } else { 0 }
        WallClockP99 = if ($wallClockTimes.Count -gt 0) { Get-Percentile -Values $wallClockTimes -Percentile 99 } else { 0 }
        PipelineP50 = if ($totalPipeline.Count -gt 0) { Get-Percentile -Values $totalPipeline -Percentile 50 } else { 0 }
        QueueWaitP50 = if ($serverQueueWaits.Count -gt 0) { Get-Percentile -Values $serverQueueWaits -Percentile 50 } else { 0 }
        RiskP50 = if ($serverRiskTimes.Count -gt 0) { Get-Percentile -Values $serverRiskTimes -Percentile 50 } else { 0 }
        MatchP50 = if ($serverMatchTimes.Count -gt 0) { Get-Percentile -Values $serverMatchTimes -Percentile 50 } else { 0 }
        SettlementP50 = if ($serverSettlementTimes.Count -gt 0) { Get-Percentile -Values $serverSettlementTimes -Percentile 50 } else { 0 }
        SuccessRate = [math]::Round($successCount*100.0/[math]::Max($totalOrders,1), 1)
    }
}

# ============================================================
# Summary Table
# ============================================================
Write-Host "============================================================" -ForegroundColor Green
Write-Host "  SUMMARY TABLE" -ForegroundColor Green
Write-Host "============================================================`n" -ForegroundColor Green

Write-Host ("{0,-20} {1,12} {2,12} {3,12} {4,12} {5,8}" -f "Scenario", "Wall P50", "Wall P95", "Wall P99", "Pipeline P50", "Success%")
Write-Host ("{0,-20} {1,12} {2,12} {3,12} {4,12} {5,8}" -f "--------", "----------", "----------", "----------", "------------", "--------")

foreach ($r in $allResults) {
    Write-Host ("{0,-20} {1,12} {2,12} {3,12} {4,12} {5,8}" -f $r.Scenario, (Format-Us $r.WallClockP50), (Format-Us $r.WallClockP95), (Format-Us $r.WallClockP99), (Format-Us $r.PipelineP50), "$($r.SuccessRate)%")
}

Write-Host "`n============================================================" -ForegroundColor Green
Write-Host "  Pipeline Stage P50 Breakdown (µs)" -ForegroundColor Green
Write-Host "============================================================`n" -ForegroundColor Green

Write-Host ("{0,-20} {1,12} {2,12} {3,12} {4,12}" -f "Scenario", "queue_wait", "risk_check", "matching_core", "settlement")
Write-Host ("{0,-20} {1,12} {2,12} {3,12} {4,12}" -f "--------", "----------", "----------", "-------------", "----------")

foreach ($r in $allResults) {
    Write-Host ("{0,-20} {1,12} {2,12} {3,12} {4,12}" -f $r.Scenario, (Format-Us $r.QueueWaitP50), (Format-Us $r.RiskP50), (Format-Us $r.MatchP50), (Format-Us $r.SettlementP50))
}

Write-Host "`nDone.`n" -ForegroundColor Green
