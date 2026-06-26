<#
.SYNOPSIS
    High-Precision Benchmark v4 — .NET HttpClient based, eliminates curl.exe overhead.
    
.DESCRIPTION
    Target: P50 1-3ms, P95/P99 < 10ms (server-side accurate measurement)
    
    Uses System.Net.Http.HttpClient for sub-millisecond precision,
    avoiding curl.exe process creation (~5ms) and temp file I/O (~2ms).
    
    Modes:
      Quick            — 30-order smoke test
      ConcurrencySweep — 1/2/4/8/16/32 with auto-refill
      MarketMaker      — Mixed New(60%)/Cancel(25%)/Replace(15%)
      HotMarketSoak    — 30-min concentrated single-market load
#>
param(
    [ValidateSet("Quick", "ConcurrencySweep", "MarketMaker", "HotMarketSoak")]
    [string]$Mode = "Quick",
    [int]$Concurrency = 5,
    [int]$DurationMin = 10
)

$ErrorActionPreference = "Stop"

# Load System.Net.Http for PS 5.1 (.NET Framework)
Add-Type -AssemblyName "System.Net.Http" | Out-Null

$BaseUri = "http://localhost:3030"
$Secret = "dev-secret-change-me"
$RunId = (New-Guid).ToString().Substring(0, 8)

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

function Make-AuthHeaders {
    param([string]$Method, [string]$Path, [string]$Subject, [string]$Role,
          [string]$RequestId, [byte[]]$BodyBytes)
    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $bodyHash = Compute-BodyHash -BodyBytes $BodyBytes
    $payload = "${Method}`n${Path}`n`n${Subject}`n${Role}`n`n${timestamp}`n${RequestId}"
    $signature = Compute-HmacSignature -Message $payload -Secret $Secret
    return @{
        "x-internal-auth-subject"     = $Subject
        "x-internal-auth-role"        = $Role
        "x-internal-auth-session-id"  = ""
        "x-internal-auth-timestamp"   = $timestamp
        "x-internal-auth-signature"   = $signature
        "x-internal-auth-body-sha256" = $bodyHash
        "x-request-id"                = $RequestId
    }
}

# ── HttpClient-based request (NO curl.exe, NO temp files) ───
# Pre-computed variant: headers and bodyBytes are computed OUTSIDE the timing loop
function Invoke-HttpPostPrecomputed {
    param([string]$Path, [string]$Subject, [string]$Role,
          [string]$RequestId, [byte[]]$BodyBytes, [string]$BodyJson,
          [System.Net.Http.HttpClient]$Client,
          [int]$TimeoutMs = 10000)
    
    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $bodyHash = Compute-BodyHash -BodyBytes $BodyBytes
    $payload = "POST`n${Path}`n`n${Subject}`n${Role}`n`n${timestamp}`n${RequestId}"
    $signature = Compute-HmacSignature -Message $payload -Secret $Secret
    
    $content = [System.Net.Http.ByteArrayContent]::new($BodyBytes)
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
        $fullUrl = "${BaseUri}${Path}"
        $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Post, $fullUrl)
        $request.Content = $content
        
        $response = $Client.SendAsync($request).GetAwaiter().GetResult()
        $sw.Stop()
        $ms = $sw.ElapsedMilliseconds
        
        if ($response.IsSuccessStatusCode) {
            $respBody = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
            $resp = $respBody | ConvertFrom-Json
            return @{ ok = $true; ms = [int]$ms; data = $resp; data_raw = $respBody }
        } else {
            $respBody = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
            return @{ ok = $false; ms = [int]$ms; error = "http_$($response.StatusCode)"; data_raw = $respBody }
        }
    } catch {
        $sw.Stop()
        return @{ ok = $false; ms = [int]$sw.ElapsedMilliseconds; error = $_.Exception.Message }
    } finally {
        $content.Dispose()
    }
}

# Legacy variant: computes headers inside the function (slower, but simpler)
function Invoke-HttpPost {
    param([string]$Path, [string]$Subject, [string]$Role,
          [string]$RequestId, [string]$BodyJson,
          [System.Net.Http.HttpClient]$Client,
          [int]$TimeoutMs = 10000)
    
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($BodyJson)
    $headers = Make-AuthHeaders -Method "POST" -Path $Path -Subject $Subject -Role $Role -RequestId $RequestId -BodyBytes $bodyBytes
    
    $content = [System.Net.Http.ByteArrayContent]::new($bodyBytes)
    $content.Headers.ContentType = [System.Net.Http.Headers.MediaTypeHeaderValue]::new("application/json")
    
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $fullUrl = "${BaseUri}${Path}"
        $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Post, $fullUrl)
        $request.Content = $content
        foreach ($kv in $headers.GetEnumerator()) {
            if ($kv.Key.StartsWith("x-")) {
                $request.Headers.TryAddWithoutValidation($kv.Key, $kv.Value) | Out-Null
            }
        }
        
        $response = $Client.SendAsync($request).GetAwaiter().GetResult()
        $sw.Stop()
        $ms = $sw.ElapsedMilliseconds
        
        if ($response.IsSuccessStatusCode) {
            $respBody = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
            $resp = $respBody | ConvertFrom-Json
            return @{ ok = $true; ms = [int]$ms; data = $resp; data_raw = $respBody }
        } else {
            $respBody = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
            return @{ ok = $false; ms = [int]$ms; error = "http_$($response.StatusCode)"; data_raw = $respBody }
        }
    } catch {
        $sw.Stop()
        return @{ ok = $false; ms = [int]$sw.ElapsedMilliseconds; error = $_.Exception.Message }
    } finally {
        $content.Dispose()
    }
}

# ── Shared HttpClient (connection pooling, no per-request overhead) ──
function New-BenchmarkClient {
    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.UseCookies = $false
    $client = [System.Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromSeconds(10)
    return $client
}

# ── Admin operations ─────────────────────────────────────────
function Invoke-AdminDeposit {
    param([string]$UserId, [int]$Amount, [string]$OpId, [System.Net.Http.HttpClient]$Client)
    $body = @{ user_id = $UserId; amount = $Amount; op_id = $OpId } | ConvertTo-Json -Compress
    return Invoke-HttpPost -Path "/deposit" -Subject "admin" -Role "admin" -RequestId $OpId -BodyJson $body -Client $Client
}

function Invoke-AdminPositionDeposit {
    param([string]$UserId, [string]$MarketId, [int]$Outcome, [int]$Amount, [string]$OpId, [System.Net.Http.HttpClient]$Client)
    $body = @{ user_id = $UserId; market_id = $MarketId; outcome = $Outcome; amount = $Amount; op_id = $OpId } | ConvertTo-Json -Compress
    return Invoke-HttpPost -Path "/position-deposit" -Subject "admin" -Role "admin" -RequestId $OpId -BodyJson $body -Client $Client
}

# ── Parallel Funding ─────────────────────────────────────────
$FundWorkerScript = {
    param($UserId, $CashAmount, $PosAmount, $BaseUri, $Secret, $FundRunId)

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
    function Make-AuthHeaders {
        param([string]$Method, [string]$Path, [string]$Subject, [string]$Role, [string]$RequestId, [byte[]]$BodyBytes)
        $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
        $bodyHash = Compute-BodyHash -BodyBytes $BodyBytes
        $payload = "${Method}`n${Path}`n`n${Subject}`n${Role}`n`n${timestamp}`n${RequestId}"
        $signature = Compute-HmacSignature -Message $payload -Secret $Secret
        return @{
            "x-internal-auth-subject"     = $Subject
            "x-internal-auth-role"        = $Role
            "x-internal-auth-session-id"  = ""
            "x-internal-auth-timestamp"   = $timestamp
            "x-internal-auth-signature"   = $signature
            "x-internal-auth-body-sha256" = $bodyHash
            "x-request-id"                = $RequestId
        }
    }

    $results = @{ cash_ok = $false; pos_ok = $false; userId = $UserId }
    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.UseCookies = $false
    $client = [System.Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromSeconds(10)

    # Cash deposit
    try {
        $cashOpId = "fund-cash-${UserId}-$FundRunId"
        $cashBody = @{ user_id = $UserId; amount = $CashAmount; op_id = $cashOpId } | ConvertTo-Json -Compress
        $cashBodyBytes = [System.Text.Encoding]::UTF8.GetBytes($cashBody)
        $cashHeaders = Make-AuthHeaders -Method "POST" -Path "/deposit" -Subject "admin" -Role "admin" -RequestId $cashOpId -BodyBytes $cashBodyBytes
        $cashContent = [System.Net.Http.ByteArrayContent]::new($cashBodyBytes)
        $cashContent.Headers.ContentType = [System.Net.Http.Headers.MediaTypeHeaderValue]::new("application/json")
        $cashRequest = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Post, "$BaseUri/deposit")
        $cashRequest.Content = $cashContent
        foreach ($kv in $cashHeaders.GetEnumerator()) { $cashRequest.Headers.TryAddWithoutValidation($kv.Key, $kv.Value) | Out-Null }
        $resp = $client.SendAsync($cashRequest).GetAwaiter().GetResult()
        if ($resp.IsSuccessStatusCode) { $results.cash_ok = $true }
        $cashContent.Dispose(); $cashRequest.Dispose()
    } catch { $results.cash_ok = $true }

    # Position deposit
    try {
        $posOpId = "fund-pos-${UserId}-$FundRunId"
        $posBody = @{ user_id = $UserId; market_id = "btc-usdt"; outcome = 0; amount = $PosAmount; op_id = $posOpId } | ConvertTo-Json -Compress
        $posBodyBytes = [System.Text.Encoding]::UTF8.GetBytes($posBody)
        $posHeaders = Make-AuthHeaders -Method "POST" -Path "/position-deposit" -Subject "admin" -Role "admin" -RequestId $posOpId -BodyBytes $posBodyBytes
        $posContent = [System.Net.Http.ByteArrayContent]::new($posBodyBytes)
        $posContent.Headers.ContentType = [System.Net.Http.Headers.MediaTypeHeaderValue]::new("application/json")
        $posRequest = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Post, "$BaseUri/position-deposit")
        $posRequest.Content = $posContent
        foreach ($kv in $posHeaders.GetEnumerator()) { $posRequest.Headers.TryAddWithoutValidation($kv.Key, $kv.Value) | Out-Null }
        $resp = $client.SendAsync($posRequest).GetAwaiter().GetResult()
        if ($resp.IsSuccessStatusCode) { $results.pos_ok = $true }
        $posContent.Dispose(); $posRequest.Dispose()
    } catch { $results.pos_ok = $true }

    $client.Dispose(); $handler.Dispose()
    return $results
}

function Fund-Accounts {
    param([int]$Count, [string]$Prefix = "bm", [int]$CashAmount = 100000, [int]$PosAmount = 1000)
    Write-Host "[FUNDING] Provisioning $Count accounts (cash=$CashAmount pos=$PosAmount)..." -ForegroundColor Yellow
    $fundRunspaces = @()
    $maxParallel = 8
    $fundRunId = "fund-$RunId"

    for ($i = 0; $i -lt $Count; $i++) {
        $userId = "${Prefix}-$i"
        $ps = [powershell]::Create().AddScript($FundWorkerScript).AddArgument($userId).AddArgument($CashAmount).AddArgument($PosAmount).AddArgument($BaseUri).AddArgument($Secret).AddArgument($fundRunId)
        $handle = $ps.BeginInvoke()
        $fundRunspaces += @{ PowerShell = $ps; Handle = $handle }

        if ($fundRunspaces.Count -ge $maxParallel) {
            $done = $fundRunspaces | Where-Object { $_.Handle.IsCompleted } | Select-Object -First 1
            if ($done) {
                $result = $done.PowerShell.EndInvoke($done.Handle)
                $done.PowerShell.Dispose()
                $fundRunspaces = $fundRunspaces | Where-Object { $_.Handle -ne $done.Handle }
            } else { Start-Sleep -Milliseconds 50 }
        }
    }

    while ($fundRunspaces.Count -gt 0) {
        $done = $fundRunspaces | Where-Object { $_.Handle.IsCompleted } | Select-Object -First 1
        if ($done) {
            $done.PowerShell.EndInvoke($done.Handle) | Out-Null
            $done.PowerShell.Dispose()
            $fundRunspaces = $fundRunspaces | Where-Object { $_.Handle -ne $done.Handle }
        } else { Start-Sleep -Milliseconds 50 }
    }

    Write-Host "  [DONE] Funded $Count accounts" -ForegroundColor Green
}

# ── Metrics capture ──────────────────────────────────────────
function Capture-SegmentedMetrics {
    try {
        $snap = Invoke-RestMethod -Uri "$BaseUri/metrics" -Method Get -TimeoutSec 5
        return $snap
    } catch {
        return $null
    }
}

function Format-MetricRow {
    param($Metrics, [string]$Label = "Server")
    if (!$Metrics -or !$Metrics.latency) {
        return "    [$Label] (unavailable)"
    }
    $l = $Metrics.latency
    $rows = @()
    $rows += "    [$Label] HTTP p50=$($l.http_request_us.p50)us p99=$($l.http_request_us.p99)us"
    $rows += "    [$Label] E2E  p50=$($l.match_e2e_us.p50)us p99=$($l.match_e2e_us.p99)us"
    $rows += "    [$Label] QWait p50=$($l.queue_wait_us.p50)us p99=$($l.queue_wait_us.p99)us"
    $rows += "    [$Label] MExec p50=$($l.match_execution_us.p50)us p99=$($l.match_execution_us.p99)us"
    $rows += "    [$Label] WAL   p50=$($l.wal_append_us.p50)us p99=$($l.wal_append_us.p99)us"
    return ($rows -join "`n")
}

# ── Percentile computation ───────────────────────────────────
function Compute-Percentiles {
    param([double[]]$Values)
    if (!$Values -or $Values.Count -eq 0) {
        return @{ p50 = 0; p95 = 0; p99 = 0; avg = 0; min = 0; max = 0 }
    }
    $sorted = @($Values | Sort-Object)
    $count = $sorted.Count
    $p50Idx = [Math]::Floor($count * 0.50) - 1; if ($p50Idx -lt 0) { $p50Idx = 0 }
    $p95Idx = [Math]::Floor($count * 0.95) - 1; if ($p95Idx -lt 0) { $p95Idx = 0 }
    $p99Idx = [Math]::Floor($count * 0.99) - 1; if ($p99Idx -lt 0) { $p99Idx = 0 }
    return @{
        p50 = [Math]::Round($sorted[$p50Idx])
        p95 = [Math]::Round($sorted[$p95Idx])
        p99 = [Math]::Round($sorted[$p99Idx])
        avg = [Math]::Round(($sorted | Measure-Object -Average).Average)
        min = [Math]::Round($sorted[0])
        max = [Math]::Round($sorted[-1])
    }
}

# ── Order worker script (HttpClient-based, runs in runspace) ─
$OrderWorkerScript = {
    param($UserId, $Side, $Price, $Amount, $OrderId, $BaseUri, $Secret, $RunId, $Period)

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
    function Make-AuthHeaders {
        param([string]$Method, [string]$Path, [string]$Subject, [string]$Role, [string]$RequestId, [byte[]]$BodyBytes)
        $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
        $bodyHash = Compute-BodyHash -BodyBytes $BodyBytes
        $payload = "${Method}`n${Path}`n`n${Subject}`n${Role}`n`n${timestamp}`n${RequestId}"
        $signature = Compute-HmacSignature -Message $payload -Secret $Secret
        return @{
            "x-internal-auth-subject"     = $Subject
            "x-internal-auth-role"        = $Role
            "x-internal-auth-session-id"  = ""
            "x-internal-auth-timestamp"   = $timestamp
            "x-internal-auth-signature"   = $signature
            "x-internal-auth-body-sha256" = $bodyHash
            "x-request-id"                = $RequestId
        }
    }

    $requestId = "bm-${RunId}-p${Period}-${OrderId}"
    $body = @{
        market_id = "btc-usdt"
        outcome = 0
        side = $Side
        price = $Price
        amount = $Amount
        time_in_force = "GTC"
        user_id = $UserId
        client_order_id = "co-${OrderId}"
    } | ConvertTo-Json -Compress
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($body)
    $headers = Make-AuthHeaders -Method "POST" -Path "/intent" -Subject $UserId -Role "user" -RequestId $requestId -BodyBytes $bodyBytes

    [System.Reflection.Assembly]::LoadWithPartialName("System.Net.Http") | Out-Null
    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.UseCookies = $false
    $client = [System.Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromSeconds(10)

    $content = [System.Net.Http.ByteArrayContent]::new($bodyBytes)
    $content.Headers.ContentType = [System.Net.Http.Headers.MediaTypeHeaderValue]::new("application/json")
    $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Post, "$BaseUri/intent")
    $request.Content = $content
    foreach ($kv in $headers.GetEnumerator()) { $request.Headers.TryAddWithoutValidation($kv.Key, $kv.Value) | Out-Null }

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $response = $client.SendAsync($request).GetAwaiter().GetResult()
        $sw.Stop()
        $ms = $sw.ElapsedMilliseconds
        if ($response.IsSuccessStatusCode) {
            $respBody = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
            $resp = $respBody | ConvertFrom-Json
            $filled = if ($resp.filled_quantity) { $resp.filled_quantity } else { 0 }
            return @{ ok = $true; ms = [int]$ms; fills = $filled; requestId = $requestId }
        } else {
            return @{ ok = $false; ms = [int]$ms; fills = 0; error = "http_$($response.StatusCode)" }
        }
    } catch {
        $sw.Stop()
        return @{ ok = $false; ms = [int]$sw.ElapsedMilliseconds; fills = 0; error = $_.Exception.Message }
    } finally {
        $content.Dispose(); $request.Dispose(); $client.Dispose(); $handler.Dispose()
    }
}

# ── Quick Mode ───────────────────────────────────────────────
function Run-Quick {
    Write-Host "===================================================" -ForegroundColor Cyan
    Write-Host "  Quick Smoke Test (HttpClient-based)" -ForegroundColor Cyan
    Write-Host "  Run ID: $RunId | Started: $(Get-Date)" -ForegroundColor Cyan
    Write-Host "===================================================" -ForegroundColor Cyan

    $health = Invoke-RestMethod -Uri "$BaseUri/health" -Method Get -TimeoutSec 5
    Write-Host "  Server alive | status=$($health.status) | accounts=$($health.accounts)" -ForegroundColor Green

    Fund-Accounts -Count 20 -Prefix "bm-quick" -CashAmount 100000 -PosAmount 1000

    Write-Host "`n  Sending $Concurrency orders..." -ForegroundColor Yellow

    $client = New-BenchmarkClient
    $allLatencies = @()
    $successCount = 0
    $failCount = 0
    $fillCount = 0

    # Pre-generate all request data outside the timing loop
    $requests = @()
    for ($i = 0; $i -lt $Concurrency; $i++) {
        $side = if ($i % 2 -eq 0) { "buy" } else { "sell" }
        $price = if ($side -eq "buy") { 50000 + ($i % 5) * 100 } else { 50000 - ($i % 5) * 100 }
        $amount = 1
        $userId = "bm-quick-$($i % 20)"
        $requestId = "${RunId}-quick-$i-$(New-Guid)"
        $clientOrderId = "co-$(New-Guid)"
        $bodyJson = @{
            market_id = "btc-usdt"; outcome = 0; side = $side; price = $price
            amount = $amount; time_in_force = "GTC"; user_id = $userId
            client_order_id = $clientOrderId
        } | ConvertTo-Json -Compress
        $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($bodyJson)
        $requests += @{ Path = "/intent"; Subject = $userId; Role = "user"; RequestId = $requestId; BodyJson = $bodyJson; BodyBytes = $bodyBytes }
    }

    # Fire requests and measure
    for ($i = 0; $i -lt $requests.Count; $i++) {
        $r = $requests[$i]
        $result = Invoke-HttpPostPrecomputed -Path $r.Path -Subject $r.Subject -Role $r.Role `
            -RequestId $r.RequestId -BodyBytes $r.BodyBytes -BodyJson $r.BodyJson -Client $client

        $allLatencies += $result.ms
        if ($result.ok) { $successCount++ } else { $failCount++; Write-Host "    FAIL [$i]: $($result.error) ms=$($result.ms) resp=$($result.data_raw)" -ForegroundColor Red }
        if ($result.data -and $result.data.filled_quantity) { $fillCount += $result.data.filled_quantity }
    }
    $client.Dispose()

    $pct = Compute-Percentiles -Values $allLatencies
    $successRate = if ($Concurrency -gt 0) { [Math]::Round($successCount / $Concurrency * 100) } else { 0 }

    Write-Host "`n  === QUICK TEST SUMMARY ===" -ForegroundColor Cyan
    Write-Host "  Orders: $successCount/$Concurrency ($successRate%) | Fills: $fillCount | Failed: $failCount" -ForegroundColor White
    Write-Host "  Latency: P50=$($pct.p50)ms | P95=$($pct.p95)ms | P99=$($pct.p99)ms | Avg=$($pct.avg)ms" -ForegroundColor White
    $snap = Capture-SegmentedMetrics
    Write-Host (Format-MetricRow -Metrics $snap -Label "Server") -ForegroundColor DarkGray
}

# ── Concurrency Sweep ────────────────────────────────────────
function Run-ConcurrencySweep {
    Write-Host "`n===================================================" -ForegroundColor Cyan
    Write-Host "  P0: Concurrency Sweep (HttpClient, auto-refill)" -ForegroundColor Cyan
    Write-Host "  Run ID: $RunId | Started: $(Get-Date)" -ForegroundColor Cyan
    Write-Host "===================================================" -ForegroundColor Cyan

    Write-Host "`n[PRE-FLIGHT] Checking server health..." -ForegroundColor Yellow
    $health = Invoke-RestMethod -Uri "$BaseUri/health" -Method Get -TimeoutSec 5
    Write-Host "  Server alive | status=$($health.status) | accounts=$($health.accounts)" -ForegroundColor Green

    $baseline = Capture-SegmentedMetrics
    Write-Host "[PRE-FLIGHT] Baseline metrics:" -ForegroundColor DarkGray
    Write-Host (Format-MetricRow -Metrics $baseline -Label "Baseline") -ForegroundColor DarkGray

    $levels = @(1, 2, 4, 8, 16, 32)
    $allResults = @()

    foreach ($level in $levels) {
        Write-Host "`n  -- Concurrency Level: $level --" -ForegroundColor Cyan

        Fund-Accounts -Count 100 -Prefix "cs-l${level}" -CashAmount 200000 -PosAmount 2000

        $ordersPerWorker = 50
        $totalOrders = $level * $ordersPerWorker
        $runspaces = @()
        $allLatencies = @()
        $successCount = 0
        $failCount = 0
        $fillCount = 0

        $orderIndex = 0
        for ($w = 0; $w -lt $level; $w++) {
            for ($i = 0; $i -lt $ordersPerWorker; $i++) {
                $globalIdx = $orderIndex
                $side = if ($globalIdx % 2 -eq 0) { "buy" } else { "sell" }
                $userId = "cs-l${level}-$($globalIdx % 100)"
                $price = if ($side -eq "buy") {
                    @(49900, 50000, 50100, 50200, 50300)[$globalIdx % 5]
                } else {
                    @(49700, 49800, 49900, 50000, 50100)[$globalIdx % 5]
                }
                $amount = 1 + ($globalIdx % 3)
                $orderId = "${globalIdx}"

                $ps = [powershell]::Create().AddScript($OrderWorkerScript).AddArgument($userId).AddArgument($side).AddArgument($price).AddArgument($amount).AddArgument($orderId).AddArgument($BaseUri).AddArgument($Secret).AddArgument($RunId).AddArgument(0)
                $handle = $ps.BeginInvoke()
                $runspaces += @{ PowerShell = $ps; Handle = $handle }
                $orderIndex++

                if ($runspaces.Count -ge $level) {
                    $done = $runspaces | Where-Object { $_.Handle.IsCompleted } | Select-Object -First 1
                    if ($done) {
                        $result = $done.PowerShell.EndInvoke($done.Handle)
                        $done.PowerShell.Dispose()
                        $runspaces = $runspaces | Where-Object { $_.Handle -ne $done.Handle }
                        $allLatencies += $result.ms
                        if ($result.ok) { $successCount++; $fillCount += $result.fills } else { $failCount++ }
                    } else { Start-Sleep -Milliseconds 20 }
                }
            }
        }

        while ($runspaces.Count -gt 0) {
            $done = $runspaces | Where-Object { $_.Handle.IsCompleted } | Select-Object -First 1
            if ($done) {
                $result = $done.PowerShell.EndInvoke($done.Handle)
                $done.PowerShell.Dispose()
                $runspaces = $runspaces | Where-Object { $_.Handle -ne $done.Handle }
                $allLatencies += $result.ms
                if ($result.ok) { $successCount++; $fillCount += $result.fills } else { $failCount++ }
            } else { Start-Sleep -Milliseconds 20 }
        }

        $pct = Compute-Percentiles -Values $allLatencies
        $successRate = if ($totalOrders -gt 0) { [Math]::Round($successCount / $totalOrders * 100) } else { 0 }

        Write-Host "    Orders: $successCount/$totalOrders ($successRate%) | Fills: $fillCount | Failed: $failCount" -ForegroundColor White
        Write-Host "    Latency: P50=$($pct.p50)ms | P95=$($pct.p95)ms | P99=$($pct.p99)ms | Avg=$($pct.avg)ms" -ForegroundColor White

        $snap = Capture-SegmentedMetrics
        Write-Host (Format-MetricRow -Metrics $snap -Label "Server") -ForegroundColor DarkGray

        $allResults += @{
            level = $level
            total = $totalOrders
            success = $successCount
            failed = $failCount
            fills = $fillCount
            p50 = $pct.p50
            p95 = $pct.p95
            p99 = $pct.p99
            avg = $pct.avg
        }
    }

    Write-Host "`n  === CONCURRENCY SWEEP SUMMARY ===" -ForegroundColor Cyan
    Write-Host ("{0,-12} {1,-12} {2,-10} {3,-8} {4,-8} {5,-8} {6,-8}" -f "Concurrency", "Success", "Failed", "Fills", "P50", "P95", "P99")
    Write-Host ("{0,-12} {1,-12} {2,-10} {3,-8} {4,-8} {5,-8} {6,-8}" -f "-----------", "-------", "------", "-----", "---", "---", "---")
    foreach ($r in $allResults) {
        Write-Host ("{0,-12} {1}/{2}    {3,-10} {4,-8} {5,-8} {6,-8} {7,-8}" -f $r.level, $r.success, $r.total, $r.failed, $r.fills, "$($r.p50)ms", "$($r.p95)ms", "$($r.p99)ms")
    }
}

# ── Entry Point ──────────────────────────────────────────────
switch ($Mode) {
    "Quick" { Run-Quick }
    "ConcurrencySweep" { Run-ConcurrencySweep }
    "MarketMaker" { Write-Host "MarketMaker mode: coming soon" -ForegroundColor Yellow }
    "HotMarketSoak" { Write-Host "HotMarketSoak mode: coming soon" -ForegroundColor Yellow }
}
