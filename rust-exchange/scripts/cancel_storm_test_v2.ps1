<#
.SYNOPSIS
    Focused Cancel Storm Benchmark — curl-based (accurate latency)
.DESCRIPTION
    Places N orders to build the book, then mass-cancels them.
    Uses curl.exe for HTTP to eliminate PowerShell Start-Job overhead (~2s per batch).
.EXAMPLE
    .\scripts\cancel_storm_test_v2.ps1 -OrderCount 100 -Concurrency 5
#>
param(
    [int]$OrderCount = 100,
    [int]$Concurrency = 5
)

$ErrorActionPreference = "Stop"
$BaseUri = "http://localhost:3030"
$Secret = "dev-secret-change-me"
$RunId = (New-Guid).ToString().Substring(0, 8)

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

# Curl-based order submit — writes JSON result to temp file
function Send-OrderCurl {
    param([string]$UserId, [string]$Side, [int]$Price, [int]$Amount, [string]$OrderId, [string]$OutFile)
    $requestId = "cancel-$RunId-$OrderId"
    $body = @{
        market_id = "btc-usdt"
        side = $Side
        price = $Price
        amount = $Amount
        outcome = 0
        client_order_id = "cancel-storm-$OrderId"
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
    
    $timeTotal = if ($result) { [double]($result -split "`n")[-1] } else { 999 }
    $elapsedMs = [Math]::Round($timeTotal * 1000)
    
    try {
        $resp = Get-Content $OutFile -Raw | ConvertFrom-Json
        Remove-Item $OutFile -Force -ErrorAction SilentlyContinue
        return @{ ok = $true; ms = $elapsedMs; fills = ($resp.fills -or 0); orderId = "cancel-storm-$OrderId" }
    } catch {
        Remove-Item $OutFile -Force -ErrorAction SilentlyContinue
        return @{ ok = $false; ms = $elapsedMs; error = "parse_error"; orderId = "cancel-storm-$OrderId" }
    }
}

# Curl-based cancel — writes JSON result to temp file
function Send-CancelCurl {
    param([string]$UserId, [string]$OrderId, [string]$OutFile)
    $requestId = "cancel-req-$RunId-$OrderId"
    $body = @{
        market_id = "btc-usdt"
        order_id = $OrderId
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
        "-X", "POST",
        "$BaseUri/cancel-order",
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
    
    $timeTotal = if ($result) { [double]($result -split "`n")[-1] } else { 999 }
    $elapsedMs = [Math]::Round($timeTotal * 1000)
    
    try {
        Get-Content $OutFile -Raw | ConvertFrom-Json | Out-Null
        Remove-Item $OutFile -Force -ErrorAction SilentlyContinue
        return @{ ok = $true; ms = $elapsedMs }
    } catch {
        Remove-Item $OutFile -Force -ErrorAction SilentlyContinue
        return @{ ok = $false; ms = $elapsedMs; error = "parse_error" }
    }
}

# ============================================================
# PRE-FLIGHT
# ============================================================
Write-Host "`n===================================================" -ForegroundColor Cyan
Write-Host "  Cancel Storm Benchmark v2 (curl-based, accurate latency)" -ForegroundColor Cyan
Write-Host "  Orders=$OrderCount | Concurrency=$Concurrency | Run=$RunId" -ForegroundColor Cyan
Write-Host "===================================================`n" -ForegroundColor Cyan

$baseline = Invoke-RestMethod -Uri "$BaseUri/metrics" -TimeoutSec 5
Write-Host "[PRE-FLIGHT] Server: orders_received=$($baseline.orders_received) | filled=$($baseline.orders_filled)" -ForegroundColor DarkGray

# ============================================================
# PHASE 1: Fund accounts & place orders
# ============================================================
Write-Host "`n[PHASE 1] Funding $Concurrency accounts and placing $OrderCount orders..." -ForegroundColor Yellow

$accountCount = $Concurrency
for ($i = 0; $i -lt $accountCount; $i++) {
    $userId = "cs-$i"
    try { Invoke-AdminDeposit -UserId $userId -Amount 10000000 -OpId "cs-dep-$i-$RunId" | Out-Null } catch {}
    try { Invoke-AdminPositionDeposit -UserId $userId -MarketId "btc-usdt" -Outcome 0 -Amount 1000 -OpId "cs-pos-$i-$RunId" | Out-Null } catch {}
}
Write-Host "  ✓ Funded $accountCount accounts" -ForegroundColor Green

$orderLatencies = @()
$orderSuccess = 0
$orderFails = 0
$orderFills = 0
$placedOrderIds = @()

$priceLevels = @{
    buy  = @(49500, 49600, 49700, 49800, 49900)
    sell = @(50100, 50200, 50300, 50400, 50500)
}

$tempDir = Join-Path $env:TEMP "cancel-storm-$RunId"
New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

for ($i = 0; $i -lt $OrderCount; $i++) {
    $side = if ($i % 2 -eq 0) { "buy" } else { "sell" }
    $userId = "cs-$(($i % $accountCount))"
    $priceArr = $priceLevels[$side]
    $price = $priceArr[$i % $priceArr.Count]
    $amount = 1 + ($i % 3)
    $orderId = "$i"
    $outFile = Join-Path $tempDir "order-$i.json"

    $result = Send-OrderCurl -UserId $userId -Side $side -Price $price -Amount $amount -OrderId $orderId -OutFile $outFile
    $orderLatencies += $result.ms

    if ($result.ok) {
        $orderSuccess++
        $orderFills += $result.fills
        $placedOrderIds += $result.orderId
    } else {
        $orderFails++
    }

    if (($i + 1) % 20 -eq 0) {
        Write-Host "  Progress: $($i + 1)/$OrderCount placed" -ForegroundColor DarkGray
    }
}

Write-Host "`n  ── Phase 1 Results ──" -ForegroundColor Cyan
Write-Host "  Placed: Success=$orderSuccess | Failed=$orderFails | Fills=$orderFills" -ForegroundColor White
$sortedOrders = $orderLatencies | Sort-Object
$oP50 = $sortedOrders[[Math]::Floor($sortedOrders.Count * 0.50)]
$oP99 = $sortedOrders[[Math]::Floor($sortedOrders.Count * 0.99)]
Write-Host "  Order Latency: P50=${oP50}ms | P99=${oP99}ms" -ForegroundColor White
Write-Host "  Open orders to cancel: $($placedOrderIds.Count)" -ForegroundColor DarkGray

# ============================================================
# PHASE 2: Cancel Storm (curl-based, accurate timing)
# ============================================================
Write-Host "`n[PHASE 2] Cancel storm: cancelling $($placedOrderIds.Count) orders with concurrency=$Concurrency..." -ForegroundColor Yellow

$cancelLatencies = @()
$cancelSuccess = 0
$cancelFailed = 0
$cancelBatches = [Math]::Ceiling($placedOrderIds.Count / $Concurrency)

for ($batch = 0; $batch -lt $cancelBatches; $batch++) {
    $runspaces = @()
    $startIdx = $batch * $Concurrency
    $endIdx = [Math]::Min($startIdx + $Concurrency, $placedOrderIds.Count)

    for ($i = $startIdx; $i -lt $endIdx; $i++) {
        $orderId = $placedOrderIds[$i]
        $userId = "cs-$(($i % $accountCount))"
        $outFile = Join-Path $tempDir "cancel-$i.json"
        
        $ps = [powershell]::Create()
        $null = $ps.AddScript({
            param($BaseUri, $UserId, $OrderId, $Secret, $RunId, $OutFile)
            
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

            $requestId = "cancel-req-$RunId-$OrderId"
            $body = @{
                market_id = "btc-usdt"
                order_id = $OrderId
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
                "-X", "POST",
                "$BaseUri/cancel-order",
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
            
            $timeTotal = if ($result) { [double]($result -split "`n")[-1] } else { 999 }
            $elapsedMs = [Math]::Round($timeTotal * 1000)
            
            try {
                Get-Content $OutFile -Raw | ConvertFrom-Json | Out-Null
                Remove-Item $OutFile -Force -ErrorAction SilentlyContinue
                return @{ ok = $true; ms = $elapsedMs }
            } catch {
                Remove-Item $OutFile -Force -ErrorAction SilentlyContinue
                return @{ ok = $false; ms = $elapsedMs; error = "parse_error" }
            }
        }).AddArgument($BaseUri).AddArgument($userId).AddArgument($orderId).AddArgument($Secret).AddArgument($RunId).AddArgument($outFile)
        
        $runspaces += @{
            Handle = $ps.BeginInvoke()
            PowerShell = $ps
        }
    }

    foreach ($rs in $runspaces) {
        $result = $rs.PowerShell.EndInvoke($rs.Handle)
        $rs.PowerShell.Dispose()
        $cancelLatencies += $result.ms
        if ($result.ok) { $cancelSuccess++ } else { $cancelFailed++ }
    }

    Write-Host "  Cancel batch $($batch + 1)/$cancelBatches done" -ForegroundColor DarkGray
}

# Cleanup temp dir
Remove-Item $tempDir -Recurse -Force -ErrorAction SilentlyContinue

Write-Host "`n  ── Phase 2 Results ──" -ForegroundColor Cyan
Write-Host "  Cancelled: $cancelSuccess | Failed: $cancelFailed" -ForegroundColor White
if ($cancelLatencies.Count -gt 0) {
    $sortedCancel = $cancelLatencies | Sort-Object
    $cP50 = $sortedCancel[[Math]::Floor($sortedCancel.Count * 0.50)]
    $cP95 = $sortedCancel[[Math]::Floor($sortedCancel.Count * 0.95)]
    $cP99 = $sortedCancel[[Math]::Floor($sortedCancel.Count * 0.99)]
    $cAvg = [Math]::Round(($cancelLatencies | Measure-Object -Average).Average)
    $cMin = ($cancelLatencies | Measure-Object -Minimum).Minimum
    $cMax = ($cancelLatencies | Measure-Object -Maximum).Maximum
    Write-Host "  Cancel Latency: P50=${cP50}ms | P95=${cP95}ms | P99=${cP99}ms" -ForegroundColor White
    Write-Host "  Cancel Latency: Avg=${cAvg}ms | Min=${cMin}ms | Max=${cMax}ms" -ForegroundColor White
}

# ============================================================
# POST-FLIGHT
# ============================================================
$postMetrics = Invoke-RestMethod -Uri "$BaseUri/metrics" -TimeoutSec 5
Write-Host "`n  ── Server Metrics Delta ──" -ForegroundColor Cyan
Write-Host "  Orders Received: $($postMetrics.orders_received)" -ForegroundColor White
Write-Host "  Orders Filled:   $($postMetrics.orders_filled)" -ForegroundColor White
Write-Host "  Orders Cancelled: $($postMetrics.orders_cancelled)" -ForegroundColor White
Write-Host "  Orders Rejected: $($postMetrics.orders_rejected)" -ForegroundColor White

Write-Host "`n✅ Cancel Storm Benchmark Complete`n" -ForegroundColor Green
