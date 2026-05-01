<#
.SYNOPSIS
    Soak Test — curl-based with PowerShell runspaces (accurate latency)
.DESCRIPTION
    Runs continuous 50/50 buy/sell orders for a specified duration.
    Uses curl.exe for HTTP to eliminate PowerShell Start-Job overhead.
    Uses PowerShell runspaces for true concurrent execution.
.EXAMPLE
    .\scripts\soak_test_v2.ps1 -DurationMin 30 -Concurrency 5
#>
param(
    [int]$DurationMin = 30,
    [int]$Concurrency = 5
)

$ErrorActionPreference = "Stop"
$BaseUri = "http://localhost:3030"
$Secret = "dev-secret-change-me-to-32-chars-min!"
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

function Invoke-AdminDeposit {
    param([string]$UserId, [int]$Amount, [string]$OpId)
    $body = @{ user_id = $UserId; amount = $Amount; op_id = $OpId } | ConvertTo-Json -Compress
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($body)
    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $payload = "POST`n/deposit`n`nadmin`nadmin`n`n${timestamp}`n${OpId}"
    $signature = Compute-HmacSignature -Message $payload -Secret $Secret
    $bodyHash = Compute-BodyHash -BodyBytes $bodyBytes
    $headers = @{
        "x-internal-auth-subject"     = "admin"
        "x-internal-auth-role"        = "admin"
        "x-internal-auth-session-id"  = ""
        "x-internal-auth-timestamp"   = $timestamp
        "x-internal-auth-signature"   = $signature
        "x-internal-auth-body-sha256" = $bodyHash
        "x-request-id"                = $OpId
        "Content-Type"                = "application/json"
    }
    return Invoke-RestMethod -Uri "$BaseUri/deposit" -Method Post -Headers $headers -Body $bodyBytes -TimeoutSec 10
}

function Invoke-AdminPositionDeposit {
    param([string]$UserId, [string]$MarketId, [int]$Outcome, [int]$Amount, [string]$OpId)
    $body = @{ user_id = $UserId; market_id = $MarketId; outcome = $Outcome; amount = $Amount; op_id = $OpId } | ConvertTo-Json -Compress
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($body)
    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $payload = "POST`n/position-deposit`n`nadmin`nadmin`n`n${timestamp}`n${OpId}"
    $signature = Compute-HmacSignature -Message $payload -Secret $Secret
    $bodyHash = Compute-BodyHash -BodyBytes $bodyBytes
    $headers = @{
        "x-internal-auth-subject"     = "admin"
        "x-internal-auth-role"        = "admin"
        "x-internal-auth-session-id"  = ""
        "x-internal-auth-timestamp"   = $timestamp
        "x-internal-auth-signature"   = $signature
        "x-internal-auth-body-sha256" = $bodyHash
        "x-request-id"                = $OpId
        "Content-Type"                = "application/json"
    }
    return Invoke-RestMethod -Uri "$BaseUri/position-deposit" -Method Post -Headers $headers -Body $bodyBytes -TimeoutSec 10
}

function Fund-Accounts {
    param([int]$Count, [string]$Prefix = "soak")
    Write-Host "[FUNDING] Provisioning $Count accounts..." -ForegroundColor Yellow
    for ($i = 0; $i -lt $Count; $i++) {
        $userId = "${Prefix}-$i"
        $cashOpId = "soak-cash-${Prefix}-$i-$RunId"
        try {
            Invoke-AdminDeposit -UserId $userId -Amount 100000 -OpId $cashOpId | Out-Null
            $posOpId = "soak-pos-${Prefix}-$i-$RunId"
            Invoke-AdminPositionDeposit -UserId $userId -MarketId "btc-usdt" -Outcome 0 -Amount 1000 -OpId $posOpId | Out-Null
        } catch {
            # Account may already exist, continue
        }
    }
    Write-Host "  ✓ Funded $Count accounts (cash + positions)" -ForegroundColor Green
}

# Curl-based order submit — writes JSON result to temp file
function Send-OrderCurl {
    param([string]$UserId, [string]$Side, [int]$Price, [int]$Amount, [string]$OrderId, [string]$OutFile)
    $requestId = "soak-${RunId}-${OrderId}"
    $body = @{
        market_id = "btc-usdt"
        side = $Side
        price = $Price
        amount = $Amount
        outcome = 0
        client_order_id = "soak-$OrderId"
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
        "-X", "POST",
        "$BaseUri/intent",
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
        "--connect-timeout", "5",
        "--max-time", "10"
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
            $fillsVal = 0
            if ($resp.fills -ne $null) { $fillsVal = $resp.fills }
            $stateVal = "unknown"
            if ($resp.order_state -ne $null) { $stateVal = $resp.order_state }
            return @{ ok = $true; ms = $ms; fills = $fillsVal; state = $stateVal }
        } catch {
            return @{ ok = $false; ms = $ms; error = "parse_error" }
        }
    } else {
        return @{ ok = $false; ms = $ms; error = "no_response" }
    }
}

# Runspace worker script
$WorkerScript = {
    param($UserId, $Side, $Price, $Amount, $OrderId, $OutFile, $BaseUri, $Secret, $RunId)
    
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
    
    $requestId = "soak-${RunId}-${OrderId}"
    $body = @{
        market_id = "btc-usdt"
        side = $Side
        price = $Price
        amount = $Amount
        outcome = 0
        client_order_id = "soak-$OrderId"
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
        "-X", "POST",
        "$BaseUri/intent",
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
        "--connect-timeout", "5",
        "--max-time", "10"
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
            $fillsVal = 0
            if ($resp.fills -ne $null) { $fillsVal = $resp.fills }
            $stateVal = "unknown"
            if ($resp.order_state -ne $null) { $stateVal = $resp.order_state }
            return @{ ok = $true; ms = $ms; fills = $fillsVal; state = $stateVal }
        } catch {
            return @{ ok = $false; ms = $ms; error = "parse_error" }
        }
    } else {
        return @{ ok = $false; ms = $ms; error = "no_response" }
    }
}

function Run-ConcurrentBatch {
    param([int]$OrderCount, [int]$Concurrency, [int]$OrderStartIndex, [int]$GlobalCounter, [string]$Prefix = "soak")
    
    $allLatencies = @()
    $successCount = 0
    $failCount = 0
    $fillCount = 0
    $accountCount = [Math]::Min($OrderCount, 50)
    
    $runspaces = @()
    $tempDir = Join-Path $env:TEMP "soak-test-$RunId"
    if (!(Test-Path $tempDir)) { New-Item -ItemType Directory -Path $tempDir -Force | Out-Null }
    
    for ($i = 0; $i -lt $OrderCount; $i++) {
        $globalIdx = $OrderStartIndex + $i
        
        # 50/50 buy/sell split
        $side = if ($globalIdx % 2 -eq 0) { "buy" } else { "sell" }
        
        $userId = "${Prefix}-$($globalIdx % $accountCount)"
        
        # Price levels: overlapping buys and sells for realistic fills
        $price = if ($side -eq "buy") {
            @(49900, 50000, 50100, 50200, 50300)[$globalIdx % 5]
        } else {
            @(49700, 49800, 49900, 50000, 50100)[$globalIdx % 5]
        }
        
        $amount = 1 + ($globalIdx % 3)
        $orderId = "${globalIdx}"
        $outFile = Join-Path $tempDir "resp-${globalIdx}.json"
        
        $ps = [powershell]::Create().AddScript($WorkerScript).AddArgument($userId).AddArgument($side).AddArgument($price).AddArgument($amount).AddArgument($orderId).AddArgument($outFile).AddArgument($BaseUri).AddArgument($Secret).AddArgument($RunId)
        
        $handle = $ps.BeginInvoke()
        $runspaces += @{ PowerShell = $ps; Handle = $handle }
        
        # Throttle: wait if we hit concurrency limit
        if ($runspaces.Count -ge $Concurrency) {
            $done = $runspaces | Where-Object { $_.Handle.IsCompleted } | Select-Object -First 1
            if ($done) {
                $result = $done.PowerShell.EndInvoke($done.Handle)
                $done.PowerShell.Dispose()
                $runspaces = $runspaces | Where-Object { $_.Handle -ne $done.Handle }
                
                $allLatencies += $result.ms
                if ($result.ok) {
                    $successCount++
                    $fillCount += $result.fills
                } else {
                    $failCount++
                }
            } else {
                Start-Sleep -Milliseconds 50
            }
        }
    }
    
    # Wait for remaining runspaces
    while ($runspaces.Count -gt 0) {
        $done = $runspaces | Where-Object { $_.Handle.IsCompleted } | Select-Object -First 1
        if ($done) {
            $result = $done.PowerShell.EndInvoke($done.Handle)
            $done.PowerShell.Dispose()
            $runspaces = $runspaces | Where-Object { $_.Handle -ne $done.Handle }
            
            $allLatencies += $result.ms
            if ($result.ok) {
                $successCount++
                $fillCount += $result.fills
            } else {
                $failCount++
            }
        } else {
            Start-Sleep -Milliseconds 50
        }
    }
    
    # Cleanup temp files
    Remove-Item "$tempDir\*" -Force -ErrorAction SilentlyContinue
    
    $sorted = $allLatencies | Sort-Object
    $p50 = if ($sorted.Count -gt 0) { $sorted[[Math]::Floor($sorted.Count * 0.50)] } else { 0 }
    $p95 = if ($sorted.Count -gt 0) { $sorted[[Math]::Floor($sorted.Count * 0.95)] } else { 0 }
    $p99 = if ($sorted.Count -gt 0) { $sorted[[Math]::Floor($sorted.Count * 0.99)] } else { 0 }
    
    return @{
        latencies = $allLatencies
        success = $successCount
        failed = $failCount
        fills = $fillCount
        p50 = $p50
        p95 = $p95
        p99 = $p99
    }
}

# ============================================================
# MAIN
# ============================================================
$totalSeconds = $DurationMin * 60
$endTime = (Get-Date).AddSeconds($totalSeconds)

Write-Host "`n===================================================" -ForegroundColor Cyan
Write-Host "  Soak Test v2 (curl-based) — ${DurationMin} min" -ForegroundColor Cyan
Write-Host "  Run ID: $RunId | Started: $(Get-Date)" -ForegroundColor Cyan
Write-Host "===================================================" -ForegroundColor Cyan

# Pre-flight
Write-Host "`n[PRE-FLIGHT] Checking server health..." -ForegroundColor Yellow
try {
    $health = Invoke-RestMethod -Uri "$BaseUri/health" -Method Get -TimeoutSec 5
    Write-Host "  ✓ Server alive | status=$($health.status) | accounts=$($health.accounts)" -ForegroundColor Green
} catch {
    Write-Host "  ✗ Server unreachable: $_" -ForegroundColor Red
    exit 1
}

# Baseline metrics
$baseline = Invoke-RestMethod -Uri "$BaseUri/metrics" -Method Get -TimeoutSec 5
Write-Host "[PRE-FLIGHT] Baseline metrics captured" -ForegroundColor DarkGray
$lat = $baseline.latency
Write-Host "  Match E2E  p50=$($lat.match_e2e_us.p50)μs  p99=$($lat.match_e2e_us.p99)μs" -ForegroundColor DarkGray
Write-Host "  Queue Wait p50=$($lat.queue_wait_us.p50)μs  p99=$($lat.queue_wait_us.p99)μs" -ForegroundColor DarkGray

# Fund accounts
Write-Host "`n[FUNDING] Provisioning 50 accounts..." -ForegroundColor Yellow
Fund-Accounts -Count 50 -Prefix "soak"

# Soak loop
$periodSeconds = 30
$ordersPerPeriod = $Concurrency * 4
$periodResults = @()
$totalSuccess = 0
$totalFailed = 0
$totalFills = 0
$allLatencies = @()
$globalOrderIndex = 0

Write-Host "`n[SOAK TEST] Running for ${DurationMin} minutes ($totalSeconds seconds)" -ForegroundColor Yellow
Write-Host "  Target: $Concurrency concurrent, continuous 50/50 buy/sell" -ForegroundColor DarkGray

$iteration = 0
while ((Get-Date) -lt $endTime) {
    $iteration++
    $remaining = [Math]::Round(($endTime - (Get-Date)).TotalMinutes, 1)
    Write-Host "`n  [Period $iteration] ${remaining}min remaining..." -ForegroundColor DarkGray
    
    $periodResult = Run-ConcurrentBatch -OrderCount $ordersPerPeriod -Concurrency $Concurrency -OrderStartIndex $globalOrderIndex -GlobalCounter $globalOrderIndex
    $globalOrderIndex += $ordersPerPeriod
    
    $periodResults += [pscustomobject]@{
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
    
    # Server metrics
    try {
        $snap = Invoke-RestMethod -Uri "$BaseUri/metrics" -Method Get -TimeoutSec 5
        $lat = $snap.latency
        Write-Host "    Queue Wait p99=$($lat.queue_wait_us.p99)μs | Match Exec p99=$($lat.match_execution_us.p99)μs | WAL p99=$($lat.wal_append_us.p99)μs" -ForegroundColor DarkGray
    } catch {}
    
    # Period summary
    Write-Host "    Orders: Success=$($periodResult.success) | Failed=$($periodResult.failed) | Fills=$($periodResult.fills)" -ForegroundColor White
    Write-Host "    Latency: P50=$($periodResult.p50)ms | P95=$($periodResult.p95)ms | P99=$($periodResult.p99)ms" -ForegroundColor White
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
if ($periodResults.Count -ge 2) {
    $mid = [Math]::Floor($periodResults.Count / 2)
    $firstHalf = @($periodResults[0..([Math]::Max(0, $mid - 1))])
    $secondHalf = @($periodResults[$mid..($periodResults.Count - 1)])
    
    $firstP99 = if ($firstHalf.Count -gt 0) { ($firstHalf | Measure-Object -Property p99 -Average).Average } else { 0 }
    $secondP99 = if ($secondHalf.Count -gt 0) { ($secondHalf | Measure-Object -Property p99 -Average).Average } else { 0 }
    $degradation = if ($firstP99 -gt 0) { [Math]::Round((($secondP99 - $firstP99) / $firstP99) * 100) } else { 0 }
    
    Write-Host "`n  TAIL LATENCY TREND:" -ForegroundColor Cyan
    Write-Host "    First half avg P99:  ${firstP99}ms" -ForegroundColor DarkGray
    Write-Host "    Second half avg P99: ${secondP99}ms" -ForegroundColor DarkGray
    Write-Host "    Degradation: ${degradation}%" -ForegroundColor $(if ($degradation -lt 20) { "Green" } elseif ($degradation -lt 50) { "Yellow" } else { "Red" })
}

# Cleanup temp directory
$tempDir = Join-Path $env:TEMP "soak-test-$RunId"
if (Test-Path $tempDir) { Remove-Item $tempDir -Recurse -Force -ErrorAction SilentlyContinue }

Write-Host "`n✓ Soak test complete.`n" -ForegroundColor Green
