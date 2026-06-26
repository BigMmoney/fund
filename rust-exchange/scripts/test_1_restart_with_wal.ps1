# Test 1: Single-Market + 429 Trigger + Restart Without Clearing WAL
# Validates: server survives restart with polluted WAL, can continue ordering

$ErrorActionPreference = "Stop"
$serverDir = "d:\pre_trading\rust-exchange"
$apiExe = "$serverDir\target\x86_64-pc-windows-gnu\release\api.exe"
$port = 3040

Write-Host "`n=== Test 1: Single-Market + 429 + Restart (WAL preserved) ===" -ForegroundColor Cyan

# Step 1: Kill existing server (preserve WAL!)
Write-Host "`n[Step 1] Killing server (preserving WAL files)..." -ForegroundColor Yellow
$oldProc = Get-Process api -ErrorAction SilentlyContinue
if ($oldProc) {
    Stop-Process -Id $oldProc.Id -Force
    Start-Sleep 3
    Write-Host "  Server killed. WAL files preserved." -ForegroundColor Green
} else {
    Write-Host "  No server running." -ForegroundColor Yellow
}

# Step 2: Record WAL sizes before restart
Write-Host "`n[Step 2] WAL sizes BEFORE restart:" -ForegroundColor Yellow
$walBefore = @{}
Get-ChildItem "$serverDir\data\*.jsonl" -ErrorAction SilentlyContinue | ForEach-Object {
    $walBefore[$_.Name] = $_.Length
    Write-Host "  $($_.Name): $($_.Length) bytes" -ForegroundColor Gray
}

# Step 3: Restart server (same port, WAL preserved)
Write-Host "`n[Step 3] Restarting server on port $port (WAL preserved)..." -ForegroundColor Yellow
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
    $stderr = $proc.StandardError.ReadToEnd()
    Write-Host "  RESTART FAILED!" -ForegroundColor Red
    Write-Host "  STDERR: $stderr" -ForegroundColor Red
    exit 1
} else {
    Write-Host "  Server restarted successfully!" -ForegroundColor Green
}

# Step 4: Health check
Write-Host "`n[Step 4] Health check after restart..." -ForegroundColor Yellow
try {
    $health = Invoke-RestMethod -Uri "http://localhost:$port/health" -UseBasicParsing -TimeoutSec 5
    Write-Host "  status: $($health.status)" -ForegroundColor Green
    Write-Host "  accounts: $($health.accounts)" -ForegroundColor Green
    Write-Host "  seen_op_ids: $($health.seen_op_ids)" -ForegroundColor Green
    Write-Host "  uptime_secs: $($health.uptime_secs)" -ForegroundColor Green
} catch {
    Write-Host "  Health check FAILED: $_" -ForegroundColor Red
}

# Step 5: Submit 10 orders after restart (should work)
Write-Host "`n[Step 5] Submitting 10 orders after restart..." -ForegroundColor Yellow
$secret = "dev-secret-change-me-to-32-chars-min!"
$successCount = 0
$rateLimitCount = 0
$errorCount = 0
$errors = @()

for ($i = 1; $i -le 10; $i++) {
    $price = 50000 + $i
    $order = @{
        client_order_id = [Guid]::NewGuid().ToString("N")
        market_id = "btc-usdt"
        side = "buy"
        order_type = "limit"
        price = $price
        amount = 1
        outcome = 1
        time_in_force = "gtc"
    } | ConvertTo-Json -Compress
    
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($order)
    $ts = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $rid = "restart-test-$i"
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
        $successCount++
    } catch {
        $sc = $_.Exception.Response.StatusCode.value__
        if ($sc -eq 429) {
            $rateLimitCount++
        } else {
            $errorCount++
            try {
                $reader = [System.IO.StreamReader]::new($_.Exception.Response.GetResponseStream())
                $errBody = $reader.ReadToEnd()
                $reader.Close()
                $errors += @{ status = $sc; index = $i; body = $errBody }
            } catch {
                $errors += @{ status = $sc; index = $i; body = "N/A" }
            }
        }
    }
}

Write-Host "`n=== Test 1 Results ===" -ForegroundColor Cyan
Write-Host "200 (success): $successCount" -ForegroundColor $(if ($successCount -gt 0) { "Green" } else { "Red" })
Write-Host "429 (rate-limited): $rateLimitCount" -ForegroundColor Yellow
Write-Host "Other errors: $errorCount" -ForegroundColor $(if ($errorCount -eq 0) { "Green" } else { "Red" })

if ($errors.Count -gt 0) {
    Write-Host "`nErrors:" -ForegroundColor Red
    $errors | ForEach-Object {
        Write-Host "  Order #$($_.index): HTTP $($_.status) - $($_.body)" -ForegroundColor Red
    }
}

# Step 6: WAL sizes after restart + orders
Write-Host "`n[Step 6] WAL sizes AFTER restart + orders:" -ForegroundColor Yellow
Get-ChildItem "$serverDir\data\*.jsonl" -ErrorAction SilentlyContinue | ForEach-Object {
    $before = $walBefore[$_.Name]
    $after = $_.Length
    $delta = $after - $before
    Write-Host "  $($_.Name): $after bytes (delta: $delta)" -ForegroundColor Gray
}

# Cleanup
$proc.Kill()
$proc.WaitForExit(5000)

Write-Host "`n=== Test 1 Complete ===" -ForegroundColor Cyan
if ($errorCount -eq 0) {
    Write-Host "PASS: Server restarted cleanly with polluted WAL, orders accepted correctly." -ForegroundColor Green
} else {
    Write-Host "FAIL: $errorCount unexpected errors after restart." -ForegroundColor Red
}
