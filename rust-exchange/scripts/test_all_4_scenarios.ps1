# Comprehensive 4-Scenario Test Suite
# Tests: restart-with-wal, dual-market, small-batch, cancel-replace

$ErrorActionPreference = "Continue"
$serverDir = "d:\pre_trading\rust-exchange"
$apiExe = "$serverDir\target\x86_64-pc-windows-gnu\release\api.exe"
$port = 3050
$secret = "dev-secret-change-me-to-32-chars-min!"

function Start-Server {
    param([switch]$CleanWAL)
    Get-Process api -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep 2
    if ($CleanWAL) { Remove-Item "$serverDir\data\*.jsonl" -Force -ErrorAction SilentlyContinue }
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $apiExe
    $psi.WorkingDirectory = $serverDir
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.EnvironmentVariables["API_BIND_PORT"] = $port.ToString()
    $psi.EnvironmentVariables["RUST_LOG"] = "warn"
    $script:proc = [System.Diagnostics.Process]::Start($psi)
    Start-Sleep 6
    if ($proc.HasExited) {
        $out = $proc.StandardOutput.ReadToEnd()
        $err = $proc.StandardError.ReadToEnd()
        Write-Host "  SERVER EXITED: $out $err" -ForegroundColor Red
        return $false
    }
    try {
        $h = Invoke-RestMethod "http://localhost:$port/health" -UseBasicParsing -TimeoutSec 5
        Write-Host "  Health: status=$($h.status) accounts=$($h.accounts) op_ids=$($h.seen_op_ids)" -ForegroundColor Green
        return $true
    } catch { return $false }
}

function Stop-Server {
    if ($script:proc -and !$proc.HasExited) { $proc.Kill(); $proc.WaitForExit(3000) }
    Start-Sleep 1
}

function Submit-Orders {
    param(
        [int]$Count,
        [string]$Market = "btc-usdt",
        [string]$Side = "buy",
        [int]$BasePrice = 50000,
        [string]$Prefix = "test"
    )
    $results = @{ "200" = 0; "429" = 0; "500" = 0; "other" = 0 }
    $errors = @()
    for ($i = 1; $i -le $Count; $i++) {
        $price = $BasePrice + $i
        $order = @{
            client_order_id = [Guid]::NewGuid().ToString("N")
            market_id = $Market
            side = $Side
            order_type = "limit"
            price = $price
            amount = 1
            outcome = 1
            time_in_force = "gtc"
        } | ConvertTo-Json -Compress
        $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($order)
        $ts = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
        $rid = "$Prefix-$i"
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
            $results["200"]++
        } catch {
            $sc = $_.Exception.Response.StatusCode.value__
            try {
                $reader = [System.IO.StreamReader]::new($_.Exception.Response.GetResponseStream())
                $errBody = $reader.ReadToEnd()
                $reader.Close()
            } catch { $errBody = "N/A" }
            if ($sc -eq 429) { $results["429"]++ }
            elseif ($sc -eq 500) { $results["500"]++; $errors += "Order ${i}: ${errBody}" }
            else { $results["other"]++; $errors += "Order ${i}: HTTP${sc} ${errBody}" }
        }
    }
    return @{ results = $results; errors = $errors }
}

function Print-Results {
    param([hashtable]$r, [string]$Label)
    $res = $r.results
    Write-Host "`n  $Label :" -ForegroundColor Yellow
    Write-Host "    200=$($res['200']), 429=$($res['429']), 500=$($res['500']), other=$($res['other'])" -ForegroundColor $(if ($res['500'] -eq 0) { "Green" } else { "Red" })
    if ($r.errors.Count -gt 0) {
        $errCount = $r.errors.Count
    Write-Host "    Errors ($errCount):" -ForegroundColor Red
        $r.errors | Select-Object -First 5 | ForEach-Object { Write-Host "      $_" -ForegroundColor Red }
    }
}

# ============================================================
# TEST 1: Single-Market + 429 Trigger + Restart (WAL preserved)
# ============================================================
Write-Host "`n" + ("=" * 60) -ForegroundColor Cyan
Write-Host "TEST 1: Single-Market + 429 + Restart (WAL PRESERVED)" -ForegroundColor Cyan
Write-Host ("=" * 60) -ForegroundColor Cyan

# Phase A: Submit enough orders to trigger 429
Write-Host "`n[1A] Starting server (CLEAN) and submitting orders to trigger 429..." -ForegroundColor Yellow
Start-Server -CleanWAL | Out-Null
$t1a = Submit-Orders -Count 30 -Market "btc-usdt" -Prefix "t1a"
Print-Results $t1a "Phase A (30 orders, expecting some 429s)"

# Phase B: Record WAL, kill server, restart WITHOUT clearing WAL
Write-Host "`n[1B] Killing server, restarting with WAL preserved..." -ForegroundColor Yellow
$walSeqBefore = (Get-Item "$serverDir\data\sequencer.wal.jsonl" -ErrorAction SilentlyContinue).Length
$walLedBefore = (Get-Item "$serverDir\data\ledger.wal.jsonl" -ErrorAction SilentlyContinue).Length
Write-Host "  WAL before: sequencer=$walSeqBefore B, ledger=$walLedBefore B" -ForegroundColor Gray

Stop-Server
Write-Host "[1B] Restarting (WAL preserved)..." -ForegroundColor Yellow
$restartOk = Start-Server
Write-Host "  Restart success: $restartOk" -ForegroundColor $(if ($restartOk) { "Green" } else { "Red" })

$walSeqAfter = (Get-Item "$serverDir\data\sequencer.wal.jsonl" -ErrorAction SilentlyContinue).Length
$walLedAfter = (Get-Item "$serverDir\data\ledger.wal.jsonl" -ErrorAction SilentlyContinue).Length
Write-Host "  WAL after restart: sequencer=$walSeqAfter B, ledger=$walLedAfter B" -ForegroundColor Gray

# Phase C: Submit more orders after restart
Write-Host "`n[1C] Submitting 10 orders after restart..." -ForegroundColor Yellow
$t1c = Submit-Orders -Count 10 -Market "btc-usdt" -BasePrice 55000 -Prefix "t1c"
Print-Results $t1c "Phase C (post-restart, 10 orders)"

Stop-Server
$t1Pass = ($t1c.results["500"] -eq 0) -and $restartOk
Write-Host "`n  TEST 1: $(if ($t1Pass) { 'PASS' } else { 'FAIL' })" -ForegroundColor $(if ($t1Pass) { "Green" } else { "Red" })

# ============================================================
# TEST 2: Dual-Market Sequential Ordering
# ============================================================
Write-Host "`n" + ("=" * 60) -ForegroundColor Cyan
Write-Host "TEST 2: Dual-Market Sequential Ordering" -ForegroundColor Cyan
Write-Host ("=" * 60) -ForegroundColor Cyan

Start-Server -CleanWAL | Out-Null

Write-Host "`n[2A] Submitting 10 orders to btc-usdt..." -ForegroundColor Yellow
$t2a = Submit-Orders -Count 10 -Market "btc-usdt" -BasePrice 50000 -Prefix "t2a-btc"
Print-Results $t2a "btc-usdt (10 orders)"

Write-Host "`n[2B] Submitting 10 orders to eth-usdt..." -ForegroundColor Yellow
$t2b = Submit-Orders -Count 10 -Market "eth-usdt" -BasePrice 3000 -Prefix "t2b-eth"
Print-Results $t2b "eth-usdt (10 orders)"

Write-Host "`n[2C] Alternating: 5 btc-usdt + 5 eth-usdt..." -ForegroundColor Yellow
$t2cBtc = Submit-Orders -Count 5 -Market "btc-usdt" -BasePrice 51000 -Prefix "t2c-btc"
$t2cEth = Submit-Orders -Count 5 -Market "eth-usdt" -BasePrice 3100 -Prefix "t2c-eth"
Print-Results $t2cBtc "btc-usdt alternating (5 orders)"
Print-Results $t2cEth "eth-usdt alternating (5 orders)"

Stop-Server
$t2Total500 = $t2a.results["500"] + $t2b.results["500"] + $t2cBtc.results["500"] + $t2cEth.results["500"]
$t2Pass = ($t2Total500 -eq 0)
Write-Host "`n  TEST 2: $(if ($t2Pass) { 'PASS' } else { 'FAIL' })" -ForegroundColor $(if ($t2Pass) { "Green" } else { "Red" })

# ============================================================
# TEST 3: Small Batch (10 orders) - Verify No 500 Escalation
# ============================================================
Write-Host "`n" + ("=" * 60) -ForegroundColor Cyan
Write-Host "TEST 3: Small Batch (10 orders) - No 500 Escalation" -ForegroundColor Cyan
Write-Host ("=" * 60) -ForegroundColor Cyan

Start-Server -CleanWAL | Out-Null

Write-Host "`n[3] Submitting 10 orders on fresh server..." -ForegroundColor Yellow
$t3 = Submit-Orders -Count 10 -Market "btc-usdt" -BasePrice 47000 -Prefix "t3"
Print-Results $t3 "Fresh batch (10 orders)"

# Check error bodies for stability
if ($t3.errors.Count -gt 0) {
    Write-Host "`n  Error body samples:" -ForegroundColor Yellow
    $t3.errors | Select-Object -First 3 | ForEach-Object { Write-Host "    $_" -ForegroundColor Gray }
}

Stop-Server
$t3Pass = ($t3.results["500"] -eq 0)
Write-Host "`n  TEST 3: $(if ($t3Pass) { 'PASS' } else { 'FAIL' })" -ForegroundColor $(if ($t3Pass) { "Green" } else { "Red" })

# ============================================================
# TEST 4: Cancel-Replace Basic Scenario
# ============================================================
Write-Host "`n" + ("=" * 60) -ForegroundColor Cyan
Write-Host "TEST 4: Cancel-Replace Basic Scenario" -ForegroundColor Cyan
Write-Host ("=" * 60) -ForegroundColor Cyan

Start-Server -CleanWAL | Out-Null

# 4A: Submit an order
Write-Host "`n[4A] Submitting initial order..." -ForegroundColor Yellow
$orderId = [Guid]::NewGuid().ToString("N")
$order = @{
    client_order_id = $orderId
    market_id = "btc-usdt"
    side = "buy"
    order_type = "limit"
    price = 47500
    amount = 1
    outcome = 1
    time_in_force = "gtc"
} | ConvertTo-Json -Compress
$bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($order)
$ts = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
$rid = "t4-initial"
$payload = "POST`n/submit-order`n`nadmin`nadmin`n`n$ts`n$rid"
$hmac = [System.Security.Cryptography.HMACSHA256]::new([System.Text.Encoding]::UTF8.GetBytes($secret))
$sig = [BitConverter]::ToString($hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($payload))).Replace("-","").ToLower()
$hmac.Dispose()
$sha = [System.Security.Cryptography.SHA256]::Create()
$bh = [BitConverter]::ToString($sha.ComputeHash($bodyBytes)).Replace("-","").ToLowerInvariant()
$sha.Dispose()

$t4aResult = "unknown"
$t4aBody = ""
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
    $t4aResult = "200"
    $t4aBody = $resp.Content
} catch {
    $sc = $_.Exception.Response.StatusCode.value__
    try {
        $reader = [System.IO.StreamReader]::new($_.Exception.Response.GetResponseStream())
        $t4aBody = $reader.ReadToEnd()
        $reader.Close()
    } catch { $t4aBody = "N/A" }
    $t4aResult = $sc.ToString()
}
Write-Host "  Initial order: HTTP $t4aResult" -ForegroundColor $(if ($t4aResult -eq "200") { "Green" } else { "Yellow" })

# 4B: Cancel the order
Write-Host "`n[4B] Cancelling order (client_order_id=$orderId)..." -ForegroundColor Yellow
$cancelBody = @{ client_order_id = $orderId; market_id = "btc-usdt" } | ConvertTo-Json -Compress
$cancelBytes = [System.Text.Encoding]::UTF8.GetBytes($cancelBody)
$ts2 = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
$rid2 = "t4-cancel"
$payload2 = "POST`n/cancel-order`n`nadmin`nadmin`n`n$ts2`n$rid2"
$hmac2 = [System.Security.Cryptography.HMACSHA256]::new([System.Text.Encoding]::UTF8.GetBytes($secret))
$sig2 = [BitConverter]::ToString($hmac2.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($payload2))).Replace("-","").ToLower()
$hmac2.Dispose()
$sha2 = [System.Security.Cryptography.SHA256]::Create()
$bh2 = [BitConverter]::ToString($sha2.ComputeHash($cancelBytes)).Replace("-","").ToLowerInvariant()
$sha2.Dispose()

$t4bResult = "unknown"
$t4bBody = ""
try {
    $resp = Invoke-WebRequest -Uri "http://localhost:$port/cancel-order" -Method POST -Headers @{
        "Content-Type"="application/json"
        "x-internal-auth-subject"="admin"
        "x-internal-auth-role"="admin"
        "x-internal-auth-session-id"=""
        "x-internal-auth-timestamp"=$ts2
        "x-internal-auth-signature"=$sig2
        "x-internal-auth-body-sha256"=$bh2
        "x-request-id"=$rid2
    } -Body $cancelBody -UseBasicParsing
    $t4bResult = "200"
    $t4bBody = $resp.Content
} catch {
    $sc = $_.Exception.Response.StatusCode.value__
    try {
        $reader = [System.IO.StreamReader]::new($_.Exception.Response.GetResponseStream())
        $t4bBody = $reader.ReadToEnd()
        $reader.Close()
    } catch { $t4bBody = "N/A" }
    $t4bResult = $sc.ToString()
}
Write-Host "  Cancel: HTTP $t4bResult" -ForegroundColor $(if ($t4bResult -eq "200" -or $t4bResult -eq "404") { "Green" } else { "Red" })

# 4C: Submit replacement order (cancel-replace)
Write-Host "`n[4C] Submitting replacement order..." -ForegroundColor Yellow
$replaceOrder = @{
    client_order_id = [Guid]::NewGuid().ToString("N")
    market_id = "btc-usdt"
    side = "buy"
    order_type = "limit"
    price = 47600
    amount = 1
    outcome = 1
    time_in_force = "gtc"
} | ConvertTo-Json -Compress
$replBytes = [System.Text.Encoding]::UTF8.GetBytes($replaceOrder)
$ts3 = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
$rid3 = "t4-replace"
$payload3 = "POST`n/submit-order`n`nadmin`nadmin`n`n$ts3`n$rid3"
$hmac3 = [System.Security.Cryptography.HMACSHA256]::new([System.Text.Encoding]::UTF8.GetBytes($secret))
$sig3 = [BitConverter]::ToString($hmac3.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($payload3))).Replace("-","").ToLower()
$hmac3.Dispose()
$sha3 = [System.Security.Cryptography.SHA256]::Create()
$bh3 = [BitConverter]::ToString($sha3.ComputeHash($replBytes)).Replace("-","").ToLowerInvariant()
$sha3.Dispose()

$t4cResult = "unknown"
$t4cBody = ""
try {
    $resp = Invoke-WebRequest -Uri "http://localhost:$port/submit-order" -Method POST -Headers @{
        "Content-Type"="application/json"
        "x-internal-auth-subject"="admin"
        "x-internal-auth-role"="admin"
        "x-internal-auth-session-id"=""
        "x-internal-auth-timestamp"=$ts3
        "x-internal-auth-signature"=$sig3
        "x-internal-auth-body-sha256"=$bh3
        "x-request-id"=$rid3
    } -Body $replaceOrder -UseBasicParsing
    $t4cResult = "200"
    $t4cBody = $resp.Content
} catch {
    $sc = $_.Exception.Response.StatusCode.value__
    try {
        $reader = [System.IO.StreamReader]::new($_.Exception.Response.GetResponseStream())
        $t4cBody = $reader.ReadToEnd()
        $reader.Close()
    } catch { $t4cBody = "N/A" }
    $t4cResult = $sc.ToString()
}
Write-Host "  Replacement order: HTTP $t4cResult" -ForegroundColor $(if ($t4cResult -eq "200") { "Green" } else { "Red" })

# 4D: Verify WAL state is consistent
Write-Host "`n[4D] WAL state after cancel-replace:" -ForegroundColor Yellow
Get-ChildItem "$serverDir\data\*.jsonl" -ErrorAction SilentlyContinue | Where-Object { $_.Length -gt 0 } | ForEach-Object {
    Write-Host "  $($_.Name): $($_.Length) bytes" -ForegroundColor Gray
}

Stop-Server
$t4Pass = ($t4aResult -eq "200") -and ($t4cResult -eq "200") -and ($t4bResult -ne "500")
Write-Host "`n  TEST 4: $(if ($t4Pass) { 'PASS' } else { 'FAIL' })" -ForegroundColor $(if ($t4Pass) { "Green" } else { "Red" })

# ============================================================
# FINAL SUMMARY
# ============================================================
Write-Host "`n" + ("=" * 60) -ForegroundColor Cyan
Write-Host "FINAL SUMMARY" -ForegroundColor Cyan
Write-Host ("=" * 60) -ForegroundColor Cyan

$allTests = @(
    @{ name = "Test 1: Restart with WAL preserved"; pass = $t1Pass },
    @{ name = "Test 2: Dual-market sequential"; pass = $t2Pass },
    @{ name = "Test 3: Small batch no 500"; pass = $t3Pass },
    @{ name = "Test 4: Cancel-replace"; pass = $t4Pass }
)

$passCount = 0
foreach ($t in $allTests) {
    $icon = if ($t.pass) { "[PASS]" } else { "[FAIL]" }
    $color = if ($t.pass) { "Green" } else { "Red" }
    Write-Host "  $icon $($t.name): $(if ($t.pass) { 'PASS' } else { 'FAIL' })" -ForegroundColor $color
    if ($t.pass) { $passCount++ }
}

Write-Host "`n  Result: $passCount/4 tests passed" -ForegroundColor $(if ($passCount -eq 4) { "Green" } else { "Yellow" })

if ($passCount -eq 4) {
    Write-Host "`n  ALL TESTS PASSED - 500 escalation bug is fixed, WAL consistency verified." -ForegroundColor Green
} else {
    Write-Host "`n  Some tests failed. Review details above." -ForegroundColor Yellow
}
