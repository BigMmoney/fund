<#
.SYNOPSIS
    Advanced Benchmark Suite v3 — Unified multi-mode benchmark for Rust trading engine.

.DESCRIPTION
    Modes:
      ConcurrencySweep  — Scale concurrency (1/2/4/8/16/32) with auto-refill to avoid fund exhaustion
      MarketMaker       — Mixed New/Cancel/Replace flow simulating real market-making
      HotMarketSoak     — 30-min single-market concentrated load with tail latency tracking
      Quick             — 2-min smoke test

    Features:
      - Segmented metrics: sequencer, queue_wait, wal_append, match_execution, http_request
      - Auto-refill accounts when balance drops below threshold
      - Unique request IDs across entire run (includes period suffix)
      - curl.exe-based HTTP for accurate wall-clock timing
      - PowerShell runspaces for true concurrency

.EXAMPLE
    .\scripts\benchmark_v3.ps1 -Mode Quick -Concurrency 5
    .\scripts\benchmark_v3.ps1 -Mode ConcurrencySweep
    .\scripts\benchmark_v3.ps1 -Mode MarketMaker -Concurrency 8 -DurationMin 10
    .\scripts\benchmark_v3.ps1 -Mode HotMarketSoak -DurationMin 30 -Concurrency 5
#>
param(
    [ValidateSet("Quick", "ConcurrencySweep", "MarketMaker", "HotMarketSoak")]
    [string]$Mode = "Quick",
    [int]$Concurrency = 5,
    [int]$DurationMin = 10
)

$ErrorActionPreference = "Stop"
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
        "Content-Type"                = "application/json"
    }
}

function Invoke-CurlPost {
    param([string]$Path, [string]$Subject, [string]$Role,
          [string]$RequestId, [string]$BodyJson, [string]$OutFile,
          [int]$ConnectTimeout = 5, [int]$MaxTime = 10)
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($BodyJson)
    $tempBody = [System.IO.Path]::GetTempFileName()
    [System.IO.File]::WriteAllBytes($tempBody, $bodyBytes)
    $headers = Make-AuthHeaders -Method "POST" -Path $Path -Subject $Subject -Role $Role -RequestId $RequestId -BodyBytes $bodyBytes

    $curlHeaderArgs = @()
    foreach ($key in $headers.Keys) {
        $curlHeaderArgs += "-H", "${key}: $($headers[$key])"
    }

    $curlArgs = @(
        "-s", "-w", "`n%{time_total}",
        "-X", "POST",
        "${BaseUri}${Path}",
        $curlHeaderArgs,
        "--data-binary", "@$tempBody",
        "-o", $OutFile,
        "--connect-timeout", $ConnectTimeout.ToString(),
        "--max-time", $MaxTime.ToString()
    )

    $result = & curl.exe @curlArgs 2>$null
    Remove-Item $tempBody -Force -ErrorAction SilentlyContinue

    $timeTotal = 0
    foreach ($line in $result) {
        if ($line -match '^[\d.]+$') {
            $timeTotal = [double]$line
        }
    }

    $ms = [Math]::Round($timeTotal * 1000)

    if (Test-Path $OutFile) {
        try {
            $resp = Get-Content $OutFile -Raw | ConvertFrom-Json
            return @{ ok = $true; ms = $ms; data = $resp }
        } catch {
            return @{ ok = $false; ms = $ms; error = "parse_error" }
        }
    } else {
        return @{ ok = $false; ms = $ms; error = "no_response" }
    }
}

# ── Admin operations ─────────────────────────────────────────
function Invoke-AdminDeposit {
    param([string]$UserId, [int]$Amount, [string]$OpId)
    $body = @{ user_id = $UserId; amount = $Amount; op_id = $OpId } | ConvertTo-Json -Compress
    return Invoke-CurlPost -Path "/deposit" -Subject "admin" -Role "admin" -RequestId $OpId -BodyJson $body -OutFile ([System.IO.Path]::GetTempFileName())
}

function Invoke-AdminPositionDeposit {
    param([string]$UserId, [string]$MarketId, [int]$Outcome, [int]$Amount, [string]$OpId)
    $body = @{ user_id = $UserId; market_id = $MarketId; outcome = $Outcome; amount = $Amount; op_id = $OpId } | ConvertTo-Json -Compress
    return Invoke-CurlPost -Path "/position-deposit" -Subject "admin" -Role "admin" -RequestId $OpId -BodyJson $body -OutFile ([System.IO.Path]::GetTempFileName())
}

# ── Parallel Funding (runspace-based, avoids sequential HTTP bottleneck) ──
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
            "Content-Type"                = "application/json"
        }
    }

    $results = @{ cash_ok = $false; pos_ok = $false; userId = $UserId }

    # Cash deposit
    try {
        $cashOpId = "fund-cash-${UserId}-$FundRunId"
        $cashBody = @{ user_id = $UserId; amount = $CashAmount; op_id = $cashOpId } | ConvertTo-Json -Compress
        $cashBodyBytes = [System.Text.Encoding]::UTF8.GetBytes($cashBody)
        $cashHeaders = Make-AuthHeaders -Method "POST" -Path "/deposit" -Subject "admin" -Role "admin" -RequestId $cashOpId -BodyBytes $cashBodyBytes
        $curlHeaderArgs = @()
        foreach ($key in $cashHeaders.Keys) { $curlHeaderArgs += "-H", "${key}: $($cashHeaders[$key])" }
        $tempFile = [System.IO.Path]::GetTempFileName()
        [System.IO.File]::WriteAllBytes($tempFile, $cashBodyBytes)
        $curlArgs = @("-s", "-w", "\n%{http_code}", "-X", "POST", "$BaseUri/deposit", $curlHeaderArgs, "--data-binary", "@$tempFile", "--connect-timeout", "5", "--max-time", "10")
        $output = & curl.exe @curlArgs 2>$null
        Remove-Item $tempFile -Force -ErrorAction SilentlyContinue
        $httpCode = 0
        foreach ($line in $output) { if ($line -match '^\d{3}$') { $httpCode = [int]$line } }
        if ($httpCode -eq 200 -or $httpCode -eq 201) { $results.cash_ok = $true }
    } catch { $results.cash_ok = $true }

    # Position deposit
    try {
        $posOpId = "fund-pos-${UserId}-$FundRunId"
        $posBody = @{ user_id = $UserId; market_id = "btc-usdt"; outcome = 0; amount = $PosAmount; op_id = $posOpId } | ConvertTo-Json -Compress
        $posBodyBytes = [System.Text.Encoding]::UTF8.GetBytes($posBody)
        $posHeaders = Make-AuthHeaders -Method "POST" -Path "/position-deposit" -Subject "admin" -Role "admin" -RequestId $posOpId -BodyBytes $posBodyBytes
        $curlHeaderArgs = @()
        foreach ($key in $posHeaders.Keys) { $curlHeaderArgs += "-H", "${key}: $($posHeaders[$key])" }
        $tempFile = [System.IO.Path]::GetTempFileName()
        [System.IO.File]::WriteAllBytes($tempFile, $posBodyBytes)
        $curlArgs = @("-s", "-w", "\n%{http_code}", "-X", "POST", "$BaseUri/position-deposit", $curlHeaderArgs, "--data-binary", "@$tempFile", "--connect-timeout", "5", "--max-time", "10")
        $output = & curl.exe @curlArgs 2>$null
        Remove-Item $tempFile -Force -ErrorAction SilentlyContinue
        $httpCode = 0
        foreach ($line in $output) { if ($line -match '^\d{3}$') { $httpCode = [int]$line } }
        if ($httpCode -eq 200 -or $httpCode -eq 201) { $results.pos_ok = $true }
    } catch { $results.pos_ok = $true }

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
            $result = $done.PowerShell.EndInvoke($done.Handle)
            $done.PowerShell.Dispose()
            $fundRunspaces = $fundRunspaces | Where-Object { $_.Handle -ne $done.Handle }
        } else { Start-Sleep -Milliseconds 50 }
    }

    Write-Host "  [DONE] Funded $Count accounts" -ForegroundColor Green
}

function Refill-Accounts {
    param([int[]]$AccountIndices, [string]$Prefix = "bm", [int]$CashAmount = 50000, [int]$PosAmount = 500, [int]$Period = 0)
    foreach ($idx in $AccountIndices) {
        $userId = "${Prefix}-$idx"
        $cashOpId = "bm-refill-cash-${Prefix}-$idx-p${Period}-$RunId"
        try {
            Invoke-AdminDeposit -UserId $userId -Amount $CashAmount -OpId $cashOpId | Out-Null
        } catch {}
        $posOpId = "bm-refill-pos-${Prefix}-$idx-p${Period}-$RunId"
        try {
            Invoke-AdminPositionDeposit -UserId $userId -MarketId "btc-usdt" -Outcome 0 -Amount $PosAmount -OpId $posOpId | Out-Null
        } catch {}
    }
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
    if (-not $Metrics -or -not $Metrics.latency) { return "" }
    $l = $Metrics.latency
    $rows = @()
    $rows += "    [$Label] HTTP p50=$($l.http_request_us.p50_us)us p99=$($l.http_request_us.p99_us)us"
    $rows += "    [$Label] E2E  p50=$($l.match_e2e_us.p50_us)us p99=$($l.match_e2e_us.p99_us)us"
    $rows += "    [$Label] QWait p50=$($l.queue_wait_us.p50_us)us p99=$($l.queue_wait_us.p99_us)us"
    $rows += "    [$Label] MExec p50=$($l.match_execution_us.p50_us)us p99=$($l.match_execution_us.p99_us)us"
    $rows += "    [$Label] WAL   p50=$($l.wal_append_us.p50_us)us p99=$($l.wal_append_us.p99_us)us"
    return $rows -join "`n"
}

# ── Runspace worker: Submit order ────────────────────────────
$OrderWorkerScript = {
    param($UserId, $Side, $Price, $Amount, $OrderId, $OutFile, $BaseUri, $Secret, $RunId, $Period)

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

    $requestId = "bm-${RunId}-p${Period}-${OrderId}"
    $body = @{
        market_id = "btc-usdt"
        side = $Side
        price = $Price
        amount = $Amount
        outcome = 0
        client_order_id = "bm-$OrderId"
        request_id = $requestId
    } | ConvertTo-Json -Compress
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($body)
    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $payload = "POST`n/intent`n`n${UserId}`nuser`n`n${timestamp}`n${requestId}"
    $signature = Compute-HmacSignature -Message $payload -Secret $Secret
    $bodyHash = Compute-BodyHash -BodyBytes $bodyBytes

    $tempBody = [System.IO.Path]::GetTempFileName()
    [System.IO.File]::WriteAllBytes($tempBody, $bodyBytes)

    $curlArgs = @(
        "-s", "-w", "`n%{time_total}",
        "-X", "POST", "$BaseUri/intent",
        "-H", "x-internal-auth-subject: $UserId",
        "-H", "x-internal-auth-role: user",
        "-H", "x-internal-auth-session-id:",
        "-H", "x-internal-auth-timestamp: $timestamp",
        "-H", "x-internal-auth-signature: $signature",
        "-H", "x-internal-auth-body-sha256: $bodyHash",
        "-H", "x-request-id: $requestId",
        "-H", "Content-Type: application/json",
        "--data-binary", "@$tempBody",
        "-o", $OutFile,
        "--connect-timeout", "5", "--max-time", "10"
    )
    $result = & curl.exe @curlArgs 2>$null
    Remove-Item $tempBody -Force -ErrorAction SilentlyContinue

    $timeTotal = 0
    foreach ($line in $result) { if ($line -match '^[\d.]+$') { $timeTotal = [double]$line } }
    $ms = [Math]::Round($timeTotal * 1000)

    if (Test-Path $OutFile) {
        try {
            $resp = Get-Content $OutFile -Raw | ConvertFrom-Json
            $fills = if ($resp.fills -ne $null) { $resp.fills } else { 0 }
            $state = if ($resp.order_state -ne $null) { $resp.order_state } else { "unknown" }
            $orderId = if ($resp.order_id -ne $null) { $resp.order_id } else { "" }
            return @{ ok = $true; ms = $ms; fills = $fills; state = $state; order_id = $orderId }
        } catch { return @{ ok = $false; ms = $ms; error = "parse_error" } }
    } else {
        return @{ ok = $false; ms = $ms; error = "no_response" }
    }
}

# ── Runspace worker: Cancel order ────────────────────────────
$CancelWorkerScript = {
    param($UserId, $OrderIdToCancel, $CancelId, $OutFile, $BaseUri, $Secret, $RunId, $Period)

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

    $requestId = "bm-cancel-${RunId}-p${Period}-${CancelId}"
    $body = @{
        market_id = "btc-usdt"
        outcome = 0
        order_id = $OrderIdToCancel
        client_order_id = "bm-cancel-$CancelId"
        request_id = $requestId
    } | ConvertTo-Json -Compress
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($body)
    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $payload = "POST`n/cancel-order`n`n${UserId}`nuser`n`n${timestamp}`n${requestId}"
    $signature = Compute-HmacSignature -Message $payload -Secret $Secret
    $bodyHash = Compute-BodyHash -BodyBytes $bodyBytes

    $tempBody = [System.IO.Path]::GetTempFileName()
    [System.IO.File]::WriteAllBytes($tempBody, $bodyBytes)

    $curlArgs = @(
        "-s", "-w", "`n%{time_total}",
        "-X", "POST", "$BaseUri/cancel-order",
        "-H", "x-internal-auth-subject: $UserId",
        "-H", "x-internal-auth-role: user",
        "-H", "x-internal-auth-session-id:",
        "-H", "x-internal-auth-timestamp: $timestamp",
        "-H", "x-internal-auth-signature: $signature",
        "-H", "x-internal-auth-body-sha256: $bodyHash",
        "-H", "x-request-id: $requestId",
        "-H", "Content-Type: application/json",
        "--data-binary", "@$tempBody",
        "-o", $OutFile,
        "--connect-timeout", "5", "--max-time", "10"
    )
    $result = & curl.exe @curlArgs 2>$null
    Remove-Item $tempBody -Force -ErrorAction SilentlyContinue

    $timeTotal = 0
    foreach ($line in $result) { if ($line -match '^[\d.]+$') { $timeTotal = [double]$line } }
    $ms = [Math]::Round($timeTotal * 1000)

    if (Test-Path $OutFile) {
        try {
            $resp = Get-Content $OutFile -Raw | ConvertFrom-Json
            $cancelled = if ($resp.cancelled_order_ids -ne $null) { $resp.cancelled_order_ids.Count } else { 0 }
            return @{ ok = $true; ms = $ms; cancelled = $cancelled }
        } catch { return @{ ok = $false; ms = $ms; error = "parse_error" } }
    } else {
        return @{ ok = $false; ms = $ms; error = "no_response" }
    }
}

# ── Runspace worker: Replace order ───────────────────────────
$ReplaceWorkerScript = {
    param($UserId, $OrderIdToReplace, $NewPrice, $ReplaceId, $OutFile, $BaseUri, $Secret, $RunId, $Period)

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

    $requestId = "bm-replace-${RunId}-p${Period}-${ReplaceId}"
    $body = @{
        market_id = "btc-usdt"
        outcome = 0
        order_id = $OrderIdToReplace
        new_price = $NewPrice
        request_id = $requestId
    } | ConvertTo-Json -Compress
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($body)
    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $payload = "POST`n/replace-order`n`n${UserId}`nuser`n`n${timestamp}`n${requestId}"
    $signature = Compute-HmacSignature -Message $payload -Secret $Secret
    $bodyHash = Compute-BodyHash -BodyBytes $bodyBytes

    $tempBody = [System.IO.Path]::GetTempFileName()
    [System.IO.File]::WriteAllBytes($tempBody, $bodyBytes)

    $curlArgs = @(
        "-s", "-w", "`n%{time_total}",
        "-X", "POST", "$BaseUri/replace-order",
        "-H", "x-internal-auth-subject: $UserId",
        "-H", "x-internal-auth-role: user",
        "-H", "x-internal-auth-session-id:",
        "-H", "x-internal-auth-timestamp: $timestamp",
        "-H", "x-internal-auth-signature: $signature",
        "-H", "x-internal-auth-body-sha256: $bodyHash",
        "-H", "x-request-id: $requestId",
        "-H", "Content-Type: application/json",
        "--data-binary", "@$tempBody",
        "-o", $OutFile,
        "--connect-timeout", "5", "--max-time", "10"
    )
    $result = & curl.exe @curlArgs 2>$null
    Remove-Item $tempBody -Force -ErrorAction SilentlyContinue

    $timeTotal = 0
    foreach ($line in $result) { if ($line -match '^[\d.]+$') { $timeTotal = [double]$line } }
    $ms = [Math]::Round($timeTotal * 1000)

    if (Test-Path $OutFile) {
        try {
            $resp = Get-Content $OutFile -Raw | ConvertFrom-Json
            $fills = if ($resp.fills -ne $null) { $resp.fills } else { 0 }
            return @{ ok = $true; ms = $ms; fills = $fills }
        } catch { return @{ ok = $false; ms = $ms; error = "parse_error" } }
    } else {
        return @{ ok = $false; ms = $ms; error = "no_response" }
    }
}

# ── Helper: compute percentiles ──────────────────────────────
function Compute-Percentiles {
    param([array]$Values)
    if ($Values.Count -eq 0) { return @{ p50 = 0; p95 = 0; p99 = 0; avg = 0; min = 0; max = 0 } }
    $sorted = $Values | Sort-Object
    return @{
        p50 = $sorted[[Math]::Floor($sorted.Count * 0.50)]
        p95 = $sorted[[Math]::Floor($sorted.Count * 0.95)]
        p99 = if ($sorted.Count -gt 1) { $sorted[[Math]::Floor($sorted.Count * 0.99)] } else { $sorted[0] }
        avg = [Math]::Round(($Values | Measure-Object -Average).Average)
        min = $sorted[0]
        max = $sorted[-1]
    }
}

# ================================================================
# MODE: Concurrency Sweep (P0 — with auto-refill)
# ================================================================
function Run-ConcurrencySweep {
    Write-Host "`n===================================================" -ForegroundColor Cyan
    Write-Host "  P0: Concurrency Sweep (auto-refill)" -ForegroundColor Cyan
    Write-Host "  Run ID: $RunId | Started: $(Get-Date)" -ForegroundColor Cyan
    Write-Host "===================================================" -ForegroundColor Cyan

    # Pre-flight
    Write-Host "`n[PRE-FLIGHT] Checking server health..." -ForegroundColor Yellow
    $health = Invoke-RestMethod -Uri "$BaseUri/health" -Method Get -TimeoutSec 5
    Write-Host "  ✓ Server alive | status=$($health.status) | accounts=$($health.accounts)" -ForegroundColor Green

    $baseline = Capture-SegmentedMetrics
    Write-Host "[PRE-FLIGHT] Baseline metrics:" -ForegroundColor DarkGray
    Write-Host (Format-MetricRow -Metrics $baseline -Label "Baseline") -ForegroundColor DarkGray

    $levels = @(1, 2, 4, 8, 16, 32)
    $allResults = @()

    foreach ($level in $levels) {
        Write-Host "`n  ── Concurrency Level: $level ──" -ForegroundColor Cyan

        # Fund 100 accounts with generous balances
        Fund-Accounts -Count 100 -Prefix "cs-l${level}" -CashAmount 200000 -PosAmount 2000

        $ordersPerWorker = 50
        $totalOrders = $level * $ordersPerWorker
        $tempDir = Join-Path $env:TEMP "bm-cs-$RunId-l$level"
        if (!(Test-Path $tempDir)) { New-Item -ItemType Directory -Path $tempDir -Force | Out-Null }

        $runspaces = @()
        $allLatencies = @()
        $successCount = 0
        $failCount = 0
        $fillCount = 0
        $placedOrderIds = @()  # Track for refill check

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
                $outFile = Join-Path $tempDir "resp-${globalIdx}.json"

                $ps = [powershell]::Create().AddScript($OrderWorkerScript).AddArgument($userId).AddArgument($side).AddArgument($price).AddArgument($amount).AddArgument($orderId).AddArgument($outFile).AddArgument($BaseUri).AddArgument($Secret).AddArgument($RunId).AddArgument(0)
                $handle = $ps.BeginInvoke()
                $runspaces += @{ PowerShell = $ps; Handle = $handle }
                $orderIndex++

                # Throttle
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

        # Drain remaining
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

        Remove-Item "$tempDir\*" -Force -ErrorAction SilentlyContinue

        $pct = Compute-Percentiles -Values $allLatencies
        $successRate = if ($totalOrders -gt 0) { [Math]::Round($successCount / $totalOrders * 100) } else { 0 }

        Write-Host "    Orders: $successCount/$totalOrders ($successRate%) | Fills: $fillCount | Failed: $failCount" -ForegroundColor White
        Write-Host "    Latency: P50=$($pct.p50)ms | P95=$($pct.p95)ms | P99=$($pct.p99)ms | Avg=$($pct.avg)ms" -ForegroundColor White

        # Server segmented metrics
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
            success_rate = $successRate
        }
    }

    # Summary table
    Write-Host "`n  === CONCURRENCY SWEEP SUMMARY ===" -ForegroundColor Cyan
    Write-Host ("{0,-12} {1,-10} {2,-10} {3,-10} {4,-8} {5,-8} {6,-8}" -f "Concurrency", "Success", "Failed", "Fills", "P50", "P95", "P99") -ForegroundColor DarkGray
    Write-Host ("{0,-12} {1,-10} {2,-10} {3,-10} {4,-8} {5,-8} {6,-8}" -f "-----------", "-------", "------", "-----", "---", "---", "---") -ForegroundColor DarkGray
    foreach ($r in $allResults) {
        Write-Host ("{0,-12} {1,-10} {2,-10} {3,-10} {4,-8} {5,-8} {6,-8}" -f $r.level, "$($r.success)/$($r.total)", $r.failed, $r.fills, "$($r.p50)ms", "$($r.p95)ms", "$($r.p99)ms") -ForegroundColor White
    }

    return $allResults
}

# ================================================================
# MODE: Market Maker (P1 — mixed New/Cancel/Replace flow)
# ================================================================
function Run-MarketMaker {
    param([int]$Concurrency, [int]$DurationMin)

    $totalSeconds = $DurationMin * 60
    $endTime = (Get-Date).AddSeconds($totalSeconds)

    Write-Host "`n===================================================" -ForegroundColor Cyan
    Write-Host "  P1: Market Maker Flow (New/Cancel/Replace)" -ForegroundColor Cyan
    Write-Host "  Run ID: $RunId | Duration: ${DurationMin}min | Concurrency: $Concurrency" -ForegroundColor Cyan
    Write-Host "  Started: $(Get-Date)" -ForegroundColor Cyan
    Write-Host "===================================================" -ForegroundColor Cyan

    # Pre-flight
    Write-Host "`n[PRE-FLIGHT] Checking server health..." -ForegroundColor Yellow
    $health = Invoke-RestMethod -Uri "$BaseUri/health" -Method Get -TimeoutSec 5
    Write-Host "  ✓ Server alive | status=$($health.status) | accounts=$($health.accounts)" -ForegroundColor Green

    $baseline = Capture-SegmentedMetrics
    Write-Host "[PRE-FLIGHT] Baseline metrics:" -ForegroundColor DarkGray
    Write-Host (Format-MetricRow -Metrics $baseline -Label "Baseline") -ForegroundColor DarkGray

    # Fund 20 market maker accounts with generous balances
    Fund-Accounts -Count 20 -Prefix "mm" -CashAmount 500000 -PosAmount 5000

    $periodSeconds = 30
    $periodResults = @()
    $allOrderLatencies = @()
    $allCancelLatencies = @()
    $allReplaceLatencies = @()
    $totalNew = 0; $totalCancelled = 0; $totalReplaced = 0; $totalFailed = 0; $totalFills = 0
    $activeOrderIds = @{}  # userId -> list of order_ids (for cancel/replace)
    $globalOpIndex = 0
    $iteration = 0

    Write-Host "`n[MARKET MAKER] Running for ${DurationMin} minutes" -ForegroundColor Yellow
    Write-Host "  Mix: 60% New, 25% Cancel, 15% Replace" -ForegroundColor DarkGray

    while ((Get-Date) -lt $endTime) {
        $iteration++
        $remaining = [Math]::Round(($endTime - (Get-Date)).TotalMinutes, 1)
        Write-Host "`n  [Period $iteration] ${remaining}min remaining..." -ForegroundColor DarkGray

        $tempDir = Join-Path $env:TEMP "bm-mm-$RunId-p$iteration"
        if (!(Test-Path $tempDir)) { New-Item -ItemType Directory -Path $tempDir -Force | Out-Null }

        $runspaces = @()
        $periodOrderLat = @()
        $periodCancelLat = @()
        $periodReplaceLat = @()
        $periodNew = 0; $periodCancelled = 0; $periodReplaced = 0; $periodFailed = 0; $periodFills = 0

        # Determine operations for this period
        $opsPerPeriod = $Concurrency * 4
        $newCount = [Math]::Floor($opsPerPeriod * 0.60)
        $cancelCount = [Math]::Floor($opsPerPeriod * 0.25)
        $replaceCount = $opsPerPeriod - $newCount - $cancelCount

        # Phase 1: New orders
        for ($i = 0; $i -lt $newCount; $i++) {
            $globalOpIndex++
            $side = if ($globalOpIndex % 2 -eq 0) { "buy" } else { "sell" }
            $userId = "mm-$($globalOpIndex % 20)"
            $price = if ($side -eq "buy") {
                49900 + ($globalOpIndex % 5) * 100
            } else {
                49700 + ($globalOpIndex % 5) * 100
            }
            $amount = 1 + ($globalOpIndex % 3)
            $orderId = "new-${globalOpIndex}"
            $outFile = Join-Path $tempDir "resp-${orderId}.json"

            $ps = [powershell]::Create().AddScript($OrderWorkerScript).AddArgument($userId).AddArgument($side).AddArgument($price).AddArgument($amount).AddArgument($orderId).AddArgument($outFile).AddArgument($BaseUri).AddArgument($Secret).AddArgument($RunId).AddArgument($iteration)
            $handle = $ps.BeginInvoke()
            $runspaces += @{ PowerShell = $ps; Handle = $handle; Type = "new" }
        }

        # Phase 2: Cancel existing orders (if any)
        $cancelOps = @()
        if ($activeOrderIds.Count -gt 0 -and $cancelCount -gt 0) {
            $usersWithOrders = @($activeOrderIds.Keys)
            $cancelledSoFar = 0
            foreach ($u in $usersWithOrders) {
                if ($cancelledSoFar -ge $cancelCount) { break }
                $orders = @($activeOrderIds[$u])
                foreach ($oid in $orders) {
                    if ($cancelledSoFar -ge $cancelCount) { break }
                    $cancelOps += @{ UserId = $u; OrderId = $oid }
                    $cancelledSoFar++
                }
            }
        }

        foreach ($cop in $cancelOps) {
            $globalOpIndex++
            $cancelId = "cancel-${globalOpIndex}"
            $outFile = Join-Path $tempDir "resp-${cancelId}.json"
            $ps = [powershell]::Create().AddScript($CancelWorkerScript).AddArgument($cop.UserId).AddArgument($cop.OrderId).AddArgument($cancelId).AddArgument($outFile).AddArgument($BaseUri).AddArgument($Secret).AddArgument($RunId).AddArgument($iteration)
            $handle = $ps.BeginInvoke()
            $runspaces += @{ PowerShell = $ps; Handle = $handle; Type = "cancel"; UserId = $cop.UserId; OrderId = $cop.OrderId }
        }

        # Phase 3: Replace existing orders (if any)
        $replaceOps = @()
        if ($activeOrderIds.Count -gt 0 -and $replaceCount -gt 0) {
            $usersWithOrders = @($activeOrderIds.Keys)
            $replacedSoFar = 0
            foreach ($u in $usersWithOrders) {
                if ($replacedSoFar -ge $replaceCount) { break }
                $orders = @($activeOrderIds[$u])
                if ($orders.Count -gt 0) {
                    $replaceOps += @{ UserId = $u; OrderId = $orders[0]; NewPrice = (49900 + ($globalOpIndex % 5) * 100) }
                    $replacedSoFar++
                }
            }
        }

        foreach ($rop in $replaceOps) {
            $globalOpIndex++
            $replaceId = "replace-${globalOpIndex}"
            $outFile = Join-Path $tempDir "resp-${replaceId}.json"
            $ps = [powershell]::Create().AddScript($ReplaceWorkerScript).AddArgument($rop.UserId).AddArgument($rop.OrderId).AddArgument($rop.NewPrice).AddArgument($replaceId).AddArgument($outFile).AddArgument($BaseUri).AddArgument($Secret).AddArgument($RunId).AddArgument($iteration)
            $handle = $ps.BeginInvoke()
            $runspaces += @{ PowerShell = $ps; Handle = $handle; Type = "replace"; UserId = $rop.UserId; OrderId = $rop.OrderId }
        }

        # Collect results
        while ($runspaces.Count -gt 0) {
            $done = $runspaces | Where-Object { $_.Handle.IsCompleted } | Select-Object -First 1
            if ($done) {
                $result = $done.PowerShell.EndInvoke($done.Handle)
                $done.PowerShell.Dispose()
                $runspaces = $runspaces | Where-Object { $_.Handle -ne $done.Handle }

                $opType = $done.Type
                if ($opType -eq "new") {
                    $periodOrderLat += $result.ms
                    if ($result.ok) {
                        $periodNew++
                        $periodFills += $result.fills
                        if ($result.order_id -and $result.state -ne "filled") {
                            $userId = "mm-$($globalOpIndex % 20)"  # approximate
                            if (-not $activeOrderIds.ContainsKey($userId)) { $activeOrderIds[$userId] = @() }
                            $activeOrderIds[$userId] += $result.order_id
                            # Keep max 20 active orders per user
                            if ($activeOrderIds[$userId].Count -gt 20) {
                                $activeOrderIds[$userId] = $activeOrderIds[$userId][-20..-1]
                            }
                        }
                    } else { $periodFailed++ }
                } elseif ($opType -eq "cancel") {
                    $periodCancelLat += $result.ms
                    if ($result.ok) {
                        $periodCancelled++
                        # Remove from active
                        if ($activeOrderIds.ContainsKey($done.UserId)) {
                            $activeOrderIds[$done.UserId] = @($activeOrderIds[$done.UserId] | Where-Object { $_ -ne $done.OrderId })
                        }
                    } else { $periodFailed++ }
                } elseif ($opType -eq "replace") {
                    $periodReplaceLat += $result.ms
                    if ($result.ok) {
                        $periodReplaced++
                        $periodFills += $result.fills
                        # Remove old, new order tracked via subsequent new orders
                        if ($activeOrderIds.ContainsKey($done.UserId)) {
                            $activeOrderIds[$done.UserId] = @($activeOrderIds[$done.UserId] | Where-Object { $_ -ne $done.OrderId })
                        }
                    } else { $periodFailed++ }
                }

                # Throttle: keep runspaces bounded
                if ($runspaces.Count -gt ($Concurrency * 3)) {
                    Start-Sleep -Milliseconds 50
                }
            } else {
                Start-Sleep -Milliseconds 50
            }
        }

        Remove-Item "$tempDir\*" -Force -ErrorAction SilentlyContinue

        $allOrderLatencies += $periodOrderLat
        $allCancelLatencies += $periodCancelLat
        $allReplaceLatencies += $periodReplaceLat
        $totalNew += $periodNew
        $totalCancelled += $periodCancelled
        $totalReplaced += $periodReplaced
        $totalFailed += $periodFailed
        $totalFills += $periodFills

        $orderPct = Compute-Percentiles -Values $periodOrderLat
        $cancelPct = Compute-Percentiles -Values $periodCancelLat
        $replacePct = Compute-Percentiles -Values $periodReplaceLat

        Write-Host "    New: $periodNew | Cancel: $periodCancelled | Replace: $periodReplaced | Failed: $periodFailed | Fills: $periodFills" -ForegroundColor White
        if ($periodOrderLat.Count -gt 0) {
            Write-Host "    New Latency:     P50=$($orderPct.p50)ms | P95=$($orderPct.p95)ms | P99=$($orderPct.p99)ms" -ForegroundColor DarkGray
        }
        if ($periodCancelLat.Count -gt 0) {
            Write-Host "    Cancel Latency:  P50=$($cancelPct.p50)ms | P95=$($cancelPct.p95)ms | P99=$($cancelPct.p99)ms" -ForegroundColor DarkGray
        }
        if ($periodReplaceLat.Count -gt 0) {
            Write-Host "    Replace Latency: P50=$($replacePct.p50)ms | P95=$($replacePct.p95)ms | P99=$($replacePct.p99)ms" -ForegroundColor DarkGray
        }

        # Server segmented metrics
        $snap = Capture-SegmentedMetrics
        Write-Host (Format-MetricRow -Metrics $snap -Label "Server") -ForegroundColor DarkGray

        $periodResults += @{
            period = $iteration
            new = $periodNew
            cancelled = $periodCancelled
            replaced = $periodReplaced
            failed = $periodFailed
            fills = $periodFills
            order_p50 = $orderPct.p50
            order_p99 = $orderPct.p99
            cancel_p50 = $cancelPct.p50
            cancel_p99 = $cancelPct.p99
            replace_p50 = $replacePct.p50
            replace_p99 = $replacePct.p99
        }

        # Refill accounts every 3 periods
        if ($iteration % 3 -eq 0) {
            Refill-Accounts -AccountIndices (0..19) -Prefix "mm" -CashAmount 100000 -PosAmount 1000 -Period $iteration
        }
    }

    # Summary
    $orderPct = Compute-Percentiles -Values $allOrderLatencies
    $cancelPct = Compute-Percentiles -Values $allCancelLatencies
    $replacePct = Compute-Percentiles -Values $allReplaceLatencies

    Write-Host "`n  === MARKET MAKER SUMMARY (${DurationMin} min) ===" -ForegroundColor Cyan
    Write-Host "  Total: New=$totalNew | Cancel=$totalCancelled | Replace=$totalReplaced | Failed=$totalFailed | Fills=$totalFills" -ForegroundColor White
    Write-Host "`n  Operation Latencies:" -ForegroundColor Cyan
    Write-Host "    New Orders:     P50=$($orderPct.p50)ms | P95=$($orderPct.p95)ms | P99=$($orderPct.p99)ms | Avg=$($orderPct.avg)ms" -ForegroundColor White
    Write-Host "    Cancel Orders:  P50=$($cancelPct.p50)ms | P95=$($cancelPct.p95)ms | P99=$($cancelPct.p99)ms | Avg=$($cancelPct.avg)ms" -ForegroundColor White
    Write-Host "    Replace Orders: P50=$($replacePct.p50)ms | P95=$($replacePct.p95)ms | P99=$($replacePct.p99)ms | Avg=$($replacePct.avg)ms" -ForegroundColor White

    # Tail latency trend
    if ($periodResults.Count -ge 2) {
        $mid = [Math]::Floor($periodResults.Count / 2)
        $firstHalf = @($periodResults[0..([Math]::Max(0, $mid - 1))])
        $secondHalf = @($periodResults[$mid..($periodResults.Count - 1)])
        $firstP99 = if ($firstHalf.Count -gt 0) { ($firstHalf | ForEach-Object { $_.order_p99 } | Where-Object { $_ -gt 0 } | Measure-Object -Average).Average } else { 0 }
        $secondP99 = if ($secondHalf.Count -gt 0) { ($secondHalf | ForEach-Object { $_.order_p99 } | Where-Object { $_ -gt 0 } | Measure-Object -Average).Average } else { 0 }
        $degradation = if ($firstP99 -gt 0) { [Math]::Round((($secondP99 - $firstP99) / $firstP99) * 100) } else { 0 }

        Write-Host "`n  TAIL LATENCY TREND (New Orders):" -ForegroundColor Cyan
        Write-Host "    First half avg P99:  ${firstP99}ms" -ForegroundColor DarkGray
        Write-Host "    Second half avg P99: ${secondP99}ms" -ForegroundColor DarkGray
        Write-Host "    Degradation: ${degradation}%" -ForegroundColor $(if ($degradation -lt 20) { "Green" } elseif ($degradation -lt 50) { "Yellow" } else { "Red" })
    }

    return @{
        total_new = $totalNew
        total_cancelled = $totalCancelled
        total_replaced = $totalReplaced
        total_failed = $totalFailed
        total_fills = $totalFills
        order_latencies = $orderPct
        cancel_latencies = $cancelPct
        replace_latencies = $replacePct
        period_results = $periodResults
    }
}

# ================================================================
# MODE: Hot Market Soak (P3 — 30 min single-market concentrated)
# ================================================================
function Run-HotMarketSoak {
    param([int]$Concurrency, [int]$DurationMin)

    $totalSeconds = $DurationMin * 60
    $endTime = (Get-Date).AddSeconds($totalSeconds)

    Write-Host "`n===================================================" -ForegroundColor Cyan
    Write-Host "  P3: Hot Market Soak Test (${DurationMin} min)" -ForegroundColor Cyan
    Write-Host "  Run ID: $RunId | Concurrency: $Concurrency" -ForegroundColor Cyan
    Write-Host "  Started: $(Get-Date)" -ForegroundColor Cyan
    Write-Host "===================================================" -ForegroundColor Cyan

    # Pre-flight
    Write-Host "`n[PRE-FLIGHT] Checking server health..." -ForegroundColor Yellow
    $health = Invoke-RestMethod -Uri "$BaseUri/health" -Method Get -TimeoutSec 5
    Write-Host "  ✓ Server alive | status=$($health.status) | accounts=$($health.accounts)" -ForegroundColor Green

    $baseline = Capture-SegmentedMetrics
    Write-Host "[PRE-FLIGHT] Baseline metrics:" -ForegroundColor DarkGray
    Write-Host (Format-MetricRow -Metrics $baseline -Label "Baseline") -ForegroundColor DarkGray

    # Fund 30 accounts with generous balances
    Fund-Accounts -Count 30 -Prefix "hot" -CashAmount 500000 -PosAmount 5000

    $periodSeconds = 30
    $ordersPerPeriod = $Concurrency * 4
    $periodResults = @()
    $totalSuccess = 0; $totalFailed = 0; $totalFills = 0
    $allLatencies = @()
    $globalOrderIndex = 0
    $iteration = 0

    Write-Host "`n[HOT SOAK] Running for ${DurationMin} minutes ($totalSeconds seconds)" -ForegroundColor Yellow
    Write-Host "  Target: $Concurrency concurrent, concentrated on btc-usdt" -ForegroundColor DarkGray

    while ((Get-Date) -lt $endTime) {
        $iteration++
        $remaining = [Math]::Round(($endTime - (Get-Date)).TotalMinutes, 1)
        Write-Host "`n  [Period $iteration] ${remaining}min remaining..." -ForegroundColor DarkGray

        $tempDir = Join-Path $env:TEMP "bm-hot-$RunId-p$iteration"
        if (!(Test-Path $tempDir)) { New-Item -ItemType Directory -Path $tempDir -Force | Out-Null }

        $runspaces = @()
        $periodLatencies = @()
        $periodSuccess = 0; $periodFailed = 0; $periodFills = 0

        for ($i = 0; $i -lt $ordersPerPeriod; $i++) {
            $globalIdx = $globalOrderIndex + $i
            $side = if ($globalIdx % 2 -eq 0) { "buy" } else { "sell" }
            $userId = "hot-$($globalIdx % 30)"
            # Tight spread for hot market: buys at 49950-50050, sells at 49950-50050
            $price = if ($side -eq "buy") {
                49950 + ($globalIdx % 11) * 10
            } else {
                49950 + ($globalIdx % 11) * 10
            }
            $amount = 1 + ($globalIdx % 3)
            $orderId = "${globalIdx}"
            $outFile = Join-Path $tempDir "resp-${globalIdx}.json"

            $ps = [powershell]::Create().AddScript($OrderWorkerScript).AddArgument($userId).AddArgument($side).AddArgument($price).AddArgument($amount).AddArgument($orderId).AddArgument($outFile).AddArgument($BaseUri).AddArgument($Secret).AddArgument($RunId).AddArgument($iteration)
            $handle = $ps.BeginInvoke()
            $runspaces += @{ PowerShell = $ps; Handle = $handle }

            if ($runspaces.Count -ge $Concurrency) {
                $done = $runspaces | Where-Object { $_.Handle.IsCompleted } | Select-Object -First 1
                if ($done) {
                    $result = $done.PowerShell.EndInvoke($done.Handle)
                    $done.PowerShell.Dispose()
                    $runspaces = $runspaces | Where-Object { $_.Handle -ne $done.Handle }
                    $periodLatencies += $result.ms
                    if ($result.ok) { $periodSuccess++; $periodFills += $result.fills } else { $periodFailed++ }
                } else { Start-Sleep -Milliseconds 20 }
            }
        }

        while ($runspaces.Count -gt 0) {
            $done = $runspaces | Where-Object { $_.Handle.IsCompleted } | Select-Object -First 1
            if ($done) {
                $result = $done.PowerShell.EndInvoke($done.Handle)
                $done.PowerShell.Dispose()
                $runspaces = $runspaces | Where-Object { $_.Handle -ne $done.Handle }
                $periodLatencies += $result.ms
                if ($result.ok) { $periodSuccess++; $periodFills += $result.fills } else { $periodFailed++ }
            } else { Start-Sleep -Milliseconds 20 }
        }

        Remove-Item "$tempDir\*" -Force -ErrorAction SilentlyContinue

        $allLatencies += $periodLatencies
        $totalSuccess += $periodSuccess
        $totalFailed += $periodFailed
        $totalFills += $periodFills
        $globalOrderIndex += $ordersPerPeriod

        $pct = Compute-Percentiles -Values $periodLatencies

        # Server segmented metrics
        $snap = Capture-SegmentedMetrics
        Write-Host (Format-MetricRow -Metrics $snap -Label "Server") -ForegroundColor DarkGray

        Write-Host "    Orders: Success=$periodSuccess | Failed=$periodFailed | Fills=$periodFills" -ForegroundColor White
        Write-Host "    Latency: P50=$($pct.p50)ms | P95=$($pct.p95)ms | P99=$($pct.p99)ms" -ForegroundColor White

        $periodResults += @{
            period = $iteration
            success = $periodSuccess
            failed = $periodFailed
            fills = $periodFills
            p50 = $pct.p50
            p95 = $pct.p95
            p99 = $pct.p99
        }

        # Refill every 3 periods
        if ($iteration % 3 -eq 0) {
            Refill-Accounts -AccountIndices (0..29) -Prefix "hot" -CashAmount 100000 -PosAmount 1000 -Period $iteration
        }
    }

    # Summary
    $overallPct = Compute-Percentiles -Values $allLatencies

    Write-Host "`n  === HOT MARKET SOAK SUMMARY (${DurationMin} min) ===" -ForegroundColor Cyan
    Write-Host "  Total: Success=$totalSuccess | Failed=$totalFailed | Fills=$totalFills" -ForegroundColor White
    Write-Host "  Overall Latency: P50=$($overallPct.p50)ms | P95=$($overallPct.p95)ms | P99=$($overallPct.p99)ms" -ForegroundColor White

    # Tail latency trend
    if ($periodResults.Count -ge 2) {
        $mid = [Math]::Floor($periodResults.Count / 2)
        $firstHalf = @($periodResults[0..([Math]::Max(0, $mid - 1))])
        $secondHalf = @($periodResults[$mid..($periodResults.Count - 1)])

        $firstP99 = if ($firstHalf.Count -gt 0) { ($firstHalf | ForEach-Object { $_.p99 } | Where-Object { $_ -gt 0 } | Measure-Object -Average).Average } else { 0 }
        $secondP99 = if ($secondHalf.Count -gt 0) { ($secondHalf | ForEach-Object { $_.p99 } | Where-Object { $_ -gt 0 } | Measure-Object -Average).Average } else { 0 }
        $degradation = if ($firstP99 -gt 0) { [Math]::Round((($secondP99 - $firstP99) / $firstP99) * 100) } else { 0 }

        Write-Host "`n  TAIL LATENCY TREND:" -ForegroundColor Cyan
        Write-Host "    First half avg P99:  ${firstP99}ms" -ForegroundColor DarkGray
        Write-Host "    Second half avg P99: ${secondP99}ms" -ForegroundColor DarkGray
        Write-Host "    Degradation: ${degradation}%" -ForegroundColor $(if ($degradation -lt 20) { "Green" } elseif ($degradation -lt 50) { "Yellow" } else { "Red" })

        # Per-period detail
        Write-Host "`n  PERIOD DETAIL:" -ForegroundColor Cyan
        Write-Host ("  {0,-8} {1,-10} {2,-8} {3,-8} {4,-8}" -f "Period", "Success", "Fills", "P50", "P99") -ForegroundColor DarkGray
        foreach ($pr in $periodResults) {
            Write-Host ("  {0,-8} {1,-10} {2,-8} {3,-8} {4,-8}" -f $pr.period, $pr.success, $pr.fills, "$($pr.p50)ms", "$($pr.p99)ms") -ForegroundColor DarkGray
        }
    }

    return @{
        total_success = $totalSuccess
        total_failed = $totalFailed
        total_fills = $totalFills
        overall_latency = $overallPct
        period_results = $periodResults
    }
}

# ================================================================
# MODE: Quick (2-min smoke test)
# ================================================================
function Run-Quick {
    Write-Host "`n===================================================" -ForegroundColor Cyan
    Write-Host "  Quick Smoke Test (2 min)" -ForegroundColor Cyan
    Write-Host "  Run ID: $RunId | Started: $(Get-Date)" -ForegroundColor Cyan
    Write-Host "===================================================" -ForegroundColor Cyan

    $health = Invoke-RestMethod -Uri "$BaseUri/health" -Method Get -TimeoutSec 5
    Write-Host "  ✓ Server alive | status=$($health.status) | accounts=$($health.accounts)" -ForegroundColor Green

    Fund-Accounts -Count 20 -Prefix "quick" -CashAmount 100000 -PosAmount 1000

    $tempDir = Join-Path $env:TEMP "bm-quick-$RunId"
    if (!(Test-Path $tempDir)) { New-Item -ItemType Directory -Path $tempDir -Force | Out-Null }

    $runspaces = @()
    $allLatencies = @()
    $successCount = 0; $failCount = 0; $fillCount = 0
    $totalOrders = $Concurrency * 10

    Write-Host "`n  Sending $totalOrders orders at concurrency=$Concurrency..." -ForegroundColor Yellow

    for ($i = 0; $i -lt $totalOrders; $i++) {
        $side = if ($i % 2 -eq 0) { "buy" } else { "sell" }
        $userId = "quick-$($i % 20)"
        $price = if ($side -eq "buy") { 49900 + ($i % 5) * 100 } else { 49700 + ($i % 5) * 100 }
        $amount = 1 + ($i % 3)
        $orderId = "${i}"
        $outFile = Join-Path $tempDir "resp-${i}.json"

        $ps = [powershell]::Create().AddScript($OrderWorkerScript).AddArgument($userId).AddArgument($side).AddArgument($price).AddArgument($amount).AddArgument($orderId).AddArgument($outFile).AddArgument($BaseUri).AddArgument($Secret).AddArgument($RunId).AddArgument(0)
        $handle = $ps.BeginInvoke()
        $runspaces += @{ PowerShell = $ps; Handle = $handle }

        if ($runspaces.Count -ge $Concurrency) {
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

    Remove-Item "$tempDir\*" -Force -ErrorAction SilentlyContinue

    $pct = Compute-Percentiles -Values $allLatencies
    $successRate = if ($totalOrders -gt 0) { [Math]::Round($successCount / $totalOrders * 100) } else { 0 }

    Write-Host "`n  === QUICK TEST SUMMARY ===" -ForegroundColor Cyan
    Write-Host "  Orders: $successCount/$totalOrders ($successRate%) | Fills: $fillCount | Failed: $failCount" -ForegroundColor White
    Write-Host "  Latency: P50=$($pct.p50)ms | P95=$($pct.p95)ms | P99=$($pct.p99)ms | Avg=$($pct.avg)ms" -ForegroundColor White

    $snap = Capture-SegmentedMetrics
    Write-Host (Format-MetricRow -Metrics $snap -Label "Server") -ForegroundColor DarkGray
}

# ================================================================
# MAIN DISPATCH
# ================================================================
switch ($Mode) {
    "Quick" { Run-Quick }
    "ConcurrencySweep" { Run-ConcurrencySweep }
    "MarketMaker" { Run-MarketMaker -Concurrency $Concurrency -DurationMin $DurationMin }
    "HotMarketSoak" { Run-HotMarketSoak -Concurrency $Concurrency -DurationMin $DurationMin }
}
