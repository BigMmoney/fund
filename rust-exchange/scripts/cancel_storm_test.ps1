<#
.SYNOPSIS
    Focused Cancel Storm Benchmark
.DESCRIPTION
    Places N orders to build the book, then mass-cancals them.
    Avoids the double-account-provisioning bug in the main suite.
.EXAMPLE
    .\scripts\cancel_storm_test.ps1 -OrderCount 100 -Concurrency 5
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

function Send-Order {
    param([string]$UserId, [string]$Side, [int]$Price, [int]$Amount, [string]$OrderId)
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
    $headers = @{
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
        $resp = Invoke-RestMethod -Uri "$BaseUri/intent" -Method Post -Headers $headers -Body $bodyBytes -TimeoutSec 10
        $sw.Stop()
        return @{ ok = $true; ms = $sw.ElapsedMilliseconds; fills = ($resp.fills -or 0); orderId = "cancel-storm-$OrderId" }
    } catch {
        $sw.Stop()
        return @{ ok = $false; ms = $sw.ElapsedMilliseconds; error = $_.Exception.Message; orderId = "cancel-storm-$OrderId" }
    }
}

function Send-Cancel {
    param([string]$UserId, [string]$OrderId)
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
    $headers = @{
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
        $resp = Invoke-RestMethod -Uri "$BaseUri/cancel-order" -Method Post -Headers $headers -Body $bodyBytes -TimeoutSec 10
        $sw.Stop()
        return @{ ok = $true; ms = $sw.ElapsedMilliseconds }
    } catch {
        $sw.Stop()
        return @{ ok = $false; ms = $sw.ElapsedMilliseconds; error = $_.Exception.Message }
    }
}

# ============================================================
# PRE-FLIGHT
# ============================================================
Write-Host "`n===================================================" -ForegroundColor Cyan
Write-Host "  Cancel Storm Benchmark" -ForegroundColor Cyan
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

# Place orders sequentially to build the book cleanly
$orderLatencies = @()
$orderSuccess = 0
$orderFails = 0
$orderFills = 0
$placedOrderIds = @()

$priceLevels = @{
    buy  = @(49500, 49600, 49700, 49800, 49900)
    sell = @(50100, 50200, 50300, 50400, 50500)
}

for ($i = 0; $i -lt $OrderCount; $i++) {
    $side = if ($i % 2 -eq 0) { "buy" } else { "sell" }
    $userId = "cs-$(($i % $accountCount))"
    $priceArr = $priceLevels[$side]
    $price = $priceArr[$i % $priceArr.Count]
    $amount = 1 + ($i % 3)
    $orderId = "$i"

    $result = Send-Order -UserId $userId -Side $side -Price $price -Amount $amount -OrderId $orderId
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
# PHASE 2: Cancel Storm
# ============================================================
Write-Host "`n[PHASE 2] Cancel storm: cancelling $($placedOrderIds.Count) orders with concurrency=$Concurrency..." -ForegroundColor Yellow

$cancelLatencies = @()
$cancelSuccess = 0
$cancelFailed = 0
$cancelBatches = [Math]::Ceiling($placedOrderIds.Count / $Concurrency)

for ($batch = 0; $batch -lt $cancelBatches; $batch++) {
    $tasks = @()
    $startIdx = $batch * $Concurrency
    $endIdx = [Math]::Min($startIdx + $Concurrency, $placedOrderIds.Count)

    for ($i = $startIdx; $i -lt $endIdx; $i++) {
        $orderId = $placedOrderIds[$i]
        $userId = "cs-$(($i % $accountCount))"
        $tasks += Start-Job -ScriptBlock {
            param($BaseUri, $UserId, $OrderId, $Secret, $RunId)

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
            $headers = @{
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
                $resp = Invoke-RestMethod -Uri "$BaseUri/cancel-order" -Method Post -Headers $headers -Body $bodyBytes -TimeoutSec 10
                $sw.Stop()
                return @{ ok = $true; ms = $sw.ElapsedMilliseconds }
            } catch {
                $sw.Stop()
                return @{ ok = $false; ms = $sw.ElapsedMilliseconds; error = $_.Exception.Message }
            }
        } -ArgumentList @($BaseUri, $userId, $orderId, $Secret, $RunId)
    }

    $tasks | Wait-Job | ForEach-Object {
        $result = Receive-Job $_
        Remove-Job $_
        $cancelLatencies += $result.ms
        if ($result.ok) { $cancelSuccess++ } else { $cancelFailed++ }
    }

    Write-Host "  Cancel batch $($batch + 1)/$cancelBatches done" -ForegroundColor DarkGray
}

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
