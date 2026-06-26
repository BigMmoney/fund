# Reproduce single_market 500 and capture WAL state
param(
    [string]$BaseUrl = "http://localhost:3030",
    [string]$Secret = "dev-secret-change-me-to-32-chars-min!",
    [int]$MaxOrders = 100
)

$ErrorActionPreference = "Continue"

function Sign-Request {
    param([string]$Method, [string]$Path, [string]$Timestamp, [string]$RequestId, [string]$Subject = "admin", [string]$Role = "admin", [string]$SessionId = "")
    $payload = "${Method}`n${Path}`n`n${Subject}`n${Role}`n${SessionId}`n${Timestamp}`n${RequestId}"
    $keyBytes = [System.Text.Encoding]::UTF8.GetBytes($Secret)
    $payloadBytes = [System.Text.Encoding]::UTF8.GetBytes($payload)
    $hmac = [System.Security.Cryptography.HMACSHA256]::new($keyBytes)
    $hashBytes = $hmac.ComputeHash($payloadBytes)
    $signature = [BitConverter]::ToString($hashBytes).Replace("-", "").ToLower()
    $hmac.Dispose()
    return $signature
}

function Compute-BodyHash {
    param([byte[]]$BodyBytes)
    $hash = [System.Security.Cryptography.SHA256]::Create()
    $hashBytes = $hash.ComputeHash($BodyBytes)
    $hash.Dispose()
    return [BitConverter]::ToString($hashBytes).Replace("-", "").ToLowerInvariant()
}

$prices = @(49000, 48900, 48800, 48700, 48600, 48500, 48400, 48300, 48200, 48100)
$statusCodeCounts = @{}
$errors = @()

Write-Host "[START] Sending up to $MaxOrders orders to reproduce 500..." -ForegroundColor Cyan

for ($i = 1; $i -le $MaxOrders; $i++) {
    $price = $prices[($i - 1) % $prices.Count]
    $orderId = "repro-$i-$(Get-Date -Format 'HHmmssfff')"
    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    
    $order = @{
        client_order_id = [Guid]::NewGuid().ToString("N")
        market_id = "btc-usdt"
        side = "buy"
        order_type = "limit"
        price = $price
        amount = 1
        outcome = 1
        time_in_force = "gtc"
    }
    
    $bodyJson = $order | ConvertTo-Json -Compress
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($bodyJson)
    $bodyHash = Compute-BodyHash -BodyBytes $bodyBytes
    
    $signature = Sign-Request -Method "POST" -Path "/submit-order" -Timestamp $timestamp -RequestId $orderId -Subject "admin" -Role "admin"
    
    $headers = @{
        "Content-Type" = "application/json"
        "x-internal-auth-subject" = "admin"
        "x-internal-auth-role" = "admin"
        "x-internal-auth-session-id" = ""
        "x-internal-auth-timestamp" = $timestamp
        "x-internal-auth-signature" = $signature
        "x-internal-auth-body-sha256" = $bodyHash
        "x-request-id" = $orderId
    }
    
    try {
        $response = Invoke-WebRequest -Uri "$BaseUrl/submit-order" -Method POST -Headers $headers -Body $bodyBytes -UseBasicParsing -TimeoutSec 30 -ErrorAction Stop
        $sc = $response.StatusCode
    } catch {
        if ($_.Exception.Response) {
            $sc = $_.Exception.Response.StatusCode.value__
            try {
                $reader = [System.IO.StreamReader]::new($_.Exception.Response.GetResponseStream())
                $respBody = $reader.ReadToEnd()
                $reader.Close()
                
                if ($sc -eq 500) {
                    Write-Host "`n[!!!] FIRST 500 at order #$i!" -ForegroundColor Red
                    Write-Host "[!!!] Response body: $respBody" -ForegroundColor Red
                    
                    # Capture error details
                    $errObj = $respBody | ConvertFrom-Json -ErrorAction SilentlyContinue
                    $errors += @{
                        OrderNum = $i
                        StatusCode = $sc
                        ResponseBody = $respBody
                        TraceId = if ($errObj.trace_id) { $errObj.trace_id } else { "N/A" }
                        Timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss.fff"
                    }
                    
                    # Immediately capture WAL tails
                    Write-Host "`n=== WAL STATE AT FIRST 500 ===" -ForegroundColor Yellow
                    
                    $walFiles = @(
                        "sequencer.wal.jsonl",
                        "ledger.wal.jsonl",
                        "trade_journal.wal.jsonl",
                        "trade_settlement.wal.jsonl",
                        "replay_guard.jsonl"
                    )
                    
                    foreach ($wf in $walFiles) {
                        $path = "d:\pre_trading\rust-exchange\data\$wf"
                        if (Test-Path $path) {
                            $size = (Get-Item $path).Length
                            $lastMod = (Get-Item $path).LastWriteTime
                            $lines = Get-Content $path -ErrorAction SilentlyContinue
                            $lineCount = $lines.Count
                            Write-Host "`n--- $wf (size=$size, lines=$lineCount, lastModified=$lastMod) ---" -ForegroundColor Yellow
                            if ($lineCount -gt 0) {
                                $tailCount = [Math]::Min(5, $lineCount)
                                Write-Host "Last $tailCount entries:" -ForegroundColor DarkYellow
                                $lines | Select-Object -Last $tailCount | ForEach-Object { Write-Host "  $_" }
                            }
                        } else {
                            Write-Host "`n--- $wf`: NOT FOUND ---" -ForegroundColor Yellow
                        }
                    }
                    
                    Write-Host "`n=== END WAL STATE ===" -ForegroundColor Yellow
                    
                    if ($errors.Count -ge 3) {
                        Write-Host "[STOP] Collected 3 errors, stopping." -ForegroundColor Cyan
                        break
                    }
                }
            } catch {
                Write-Host "[WARN] HTTP $sc (no body readable)" -ForegroundColor Yellow
            }
        } else {
            $sc = 0
            Write-Host "[ERROR] Request failed: $_" -ForegroundColor Red
        }
    }
    
    $current = if ($statusCodeCounts.ContainsKey($sc)) { $statusCodeCounts[$sc] } else { 0 }
    $statusCodeCounts[$sc] = $current + 1
    
    if ($i % 10 -eq 0) {
        $countsStr = ($statusCodeCounts.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ", "
        Write-Host "[$i/$MaxOrders] Status codes: $countsStr" -ForegroundColor Gray
    }
    
    Start-Sleep -Milliseconds 200
}

Write-Host "`n=== FINAL RESULTS ===" -ForegroundColor Cyan
$statusCounts = ($statusCodeCounts.GetEnumerator() | Sort-Object Name | ForEach-Object { "HTTP $($_.Key): $($_.Value)" }) -join " | "
Write-Host "Status distribution: $statusCounts"
Write-Host "Total 500 errors: $($errors.Count)"

if ($errors.Count -gt 0) {
    Write-Host "`n=== ERROR DETAILS ===" -ForegroundColor Red
    foreach ($err in $errors) {
        Write-Host "Order #$( $err.OrderNum) at $($err.Timestamp)"
        Write-Host "  Trace ID: $($err.TraceId)"
        Write-Host "  Body: $($err.ResponseBody)"
    }
}
