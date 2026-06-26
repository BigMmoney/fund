# Test 2: Dual-Market Sequential Ordering
# Validates: 200/429/500 distribution across two markets, no single-market bias

$ErrorActionPreference = "Stop"
$serverDir = "d:\pre_trading\rust-exchange"
$apiExe = "$serverDir\target\x86_64-pc-windows-gnu\release\api.exe"
$port = 3040

Write-Host "`n=== Test 2: Dual-Market Sequential Ordering ===" -ForegroundColor Cyan

# Step 1: Clean start
Write-Host "`n[Step 1] Starting server on port $port..." -ForegroundColor Yellow
Get-Process api -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep 2
Remove-Item "$serverDir\data\*.jsonl" -Force -ErrorAction SilentlyContinue

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $apiExe
$psi.WorkingDirectory = $serverDir
$psi.UseShellExecute = $false
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.EnvironmentVariables["API_BIND_PORT"] = $port.ToString()
$psi.EnvironmentVariables["RUST_LOG"] = "info"
$proc = [System.Diagnostics.Process]::Start($psi)

Start-Sleep 8
if ($proc.HasExited) {
    Write-Host "  Server failed to start!" -ForegroundColor Red
    $proc.StandardError.ReadToEnd() | Write-Host
    exit 1
}
Write-Host "  Server started." -ForegroundColor Green

# Step 2: Check available markets
Write-Host "`n[Step 2] Checking available markets..." -ForegroundColor Yellow
try {
    $markets = Invoke-RestMethod -Uri "http://localhost:$port/markets" -UseBasicParsing -TimeoutSec 5
    Write-Host "  Available markets:" -ForegroundColor Green
    $markets | ForEach-Object { Write-Host "    - $_" -ForegroundColor Gray }
} catch {
    Write-Host "  Could not fetch markets, using defaults: btc-usdt, eth-usdt" -ForegroundColor Yellow
}

# Step 3: Submit orders alternating between two markets
Write-Host "`n[Step 3] Submitting 20 orders: 10 btc-usdt + 10 eth-usdt (alternating)..." -ForegroundColor Yellow
$secret = "dev-secret-change-me-to-32-chars-min!"
$results = @{
    "btc-usdt" = @{ success = 0; rateLimited = 0; errors = 0; errorDetails = @() }
    "eth-usdt" = @{ success = 0; rateLimited = 0; errors = 0; errorDetails = @() }
}
$markets_to_use = @("btc-usdt", "eth-usdt")

for ($i = 1; $i -le 20; $i++) {
    $marketIdx = ($i - 1) % 2
    $market = $markets_to_use[$marketIdx]
    $price = if ($market -eq "btc-usdt") { 50000 + $i } else { 3000 + $i }
    
    $order = @{
        client_order_id = [Guid]::NewGuid().ToString("N")
        market_id = $market
        side = "buy"
        order_type = "limit"
        price = $price
        amount = 1
        outcome = 1
        time_in_force = "gtc"
    } | ConvertTo-Json -Compress
    
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($order)
    $ts = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $rid = "dual-test-$i"
    $payload = "POST`n/submit-order`n`nadmin`nadmin`n`n$ts`n$rid"
    $hmac = [System.Security.Cryptography.HMACSHA256]::new([System.Text.Encoding]::UTF8.GetBytes($secret))
    $sig = [BitConverter]::ToString($hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($payload))).Replace("-","").ToLower()
    $hmac.Dispose()
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $bh = [BitConverter]::ToString($sha.ComputeHash($bodyBytes)).Replace("-","").ToLowerInvariant()
    $sha.Dispose()
    
    try {
        $resp = Invoke-WebRequest -Uri "http://localhost:$port/submit-order" -Method POST -Headers @{
            "Content-Type"="application/json"
            "x-internal-auth-subject"="admin"
            "x-internal-auth-role"="admin"
            "x-internal-auth-session-id"=""
            "x-internal-auth-timestamp"=$ts
            "x-internal-auth-signature"=$sig
            "x-internal-auth-body-sha256"=$bh
            "x-request-id"=$rid
        } -Body $order -UseBasicParsing
        $results[$market].success++
    } catch {
        $sc = $_.Exception.Response.StatusCode.value__
        if ($sc -eq 429) {
            $results[$market].rateLimited++
        } else {
            $results[$market].errors++
            try {
                $reader = [System.IO.StreamReader]::new($_.Exception.Response.GetResponseStream())
                $errBody = $reader.ReadToEnd()
                $reader.Close()
                $results[$market].errorDetails += @{ status = $sc; index = $i; body = $errBody }
            } catch {
                $results[$market].errorDetails += @{ status = $sc; index = $i; body = "N/A" }
            }
        }
    }
}

# Results
Write-Host "`n=== Test 2 Results ===" -ForegroundColor Cyan
$totalSuccess = 0
$totalErrors = 0

foreach ($market in $markets_to_use) {
    $r = $results[$market]
    $totalSuccess += $r.success
    $totalErrors += $r.errors
    Write-Host "`n  Market: $market" -ForegroundColor Yellow
    Write-Host "    200 (success): $($r.success)" -ForegroundColor $(if ($r.success -gt 0) { "Green" } else { "Gray" })
    Write-Host "    429 (rate-limited): $($r.rateLimited)" -ForegroundColor Yellow
    Write-Host "    Other errors: $($r.errors)" -ForegroundColor $(if ($r.errors -eq 0) { "Green" } else { "Red" })
    
    if ($r.errorDetails.Count -gt 0) {
        Write-Host "    Error details:" -ForegroundColor Red
        $r.errorDetails | ForEach-Object {
            Write-Host "      Order #$($_.index): HTTP $($_.status) - $($_.body)" -ForegroundColor Red
        }
    }
}

Write-Host "`n  Overall: $totalSuccess success, $totalErrors errors" -ForegroundColor $(if ($totalErrors -eq 0) { "Green" } else { "Red" })

# Cleanup
$proc.Kill()
$proc.WaitForExit(5000)

Write-Host "`n=== Test 2 Complete ===" -ForegroundColor Cyan
if ($totalErrors -eq 0) {
    Write-Host "PASS: Both markets handled correctly, no 500 errors." -ForegroundColor Green
} else {
    Write-Host "FAIL: $totalErrors unexpected errors across markets." -ForegroundColor Red
}
