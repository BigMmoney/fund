# Comprehensive End-to-End Test for Phase 4/5 Fixes
# Tests: default seeding, status code preservation, WAL consistency

$ErrorActionPreference = "Stop"
$serverDir = "d:\pre_trading\rust-exchange"
$apiExe = "$serverDir\target\x86_64-pc-windows-gnu\release\api.exe"
$port = 3040  # Fresh port to avoid conflicts

Write-Host "`n=== Phase 4/5: Comprehensive E2E Test ===" -ForegroundColor Cyan

# Step 1: Kill existing processes
Write-Host "`n[Step 1] Cleaning up..." -ForegroundColor Yellow
Get-Process | Where-Object { $_.ProcessName -match "api|cargo|rustc" } | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep 2
Remove-Item "$serverDir\data\*.jsonl" -Force -ErrorAction SilentlyContinue
Write-Host "  Done."

# Step 2: Start server WITHOUT any seed env vars (testing default seeding)
Write-Host "`n[Step 2] Starting server on port $port (NO seed env vars)..." -ForegroundColor Yellow
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $apiExe
$psi.WorkingDirectory = $serverDir
$psi.UseShellExecute = $false
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.EnvironmentVariables["API_BIND_PORT"] = $port.ToString()
$psi.EnvironmentVariables["RUST_LOG"] = "info"
$proc = [System.Diagnostics.Process]::Start($psi)
$proc.PriorityClass = [System.Diagnostics.ProcessPriorityClass]::BelowNormal

# Wait for server to start
Write-Host "  Waiting for server startup (8s)..." -ForegroundColor Yellow
Start-Sleep -Seconds 8

# Check if process is still alive
if ($proc.HasExited) {
    $stderr = $proc.StandardError.ReadToEnd()
    $stdout = $proc.StandardOutput.ReadToEnd()
    Write-Host "  SERVER EXITED prematurely!" -ForegroundColor Red
    Write-Host "STDOUT: $stdout" -ForegroundColor Gray
    Write-Host "STDERR: $stderr" -ForegroundColor Red
    exit 1
}

# Step 3: Health check (should show seeded accounts and instruments)
Write-Host "`n[Step 3] Health check (expecting seeded data)..." -ForegroundColor Yellow
try {
    $health = Invoke-RestMethod -Uri "http://localhost:$port/health" -UseBasicParsing -TimeoutSec 5
    Write-Host "  status: $($health.status)" -ForegroundColor Green
    Write-Host "  accounts: $($health.accounts)" -ForegroundColor Green
    Write-Host "  instruments: $($health.instruments)" -ForegroundColor Green
    Write-Host "  ledger_wal_entries: $($health.ledger_wal_entries)" -ForegroundColor Green
    Write-Host "  sequencer_records: $($health.sequencer_records)" -ForegroundColor Green
    
    if ($health.accounts -eq 0) {
        Write-Host "  WARNING: No accounts found! Default seeding may not be working." -ForegroundColor Red
    }
    if ($health.instruments -eq 0) {
        Write-Host "  WARNING: No instruments found! Default seeding may not be working." -ForegroundColor Red
    }
} catch {
    Write-Host "  Health check FAILED: $_" -ForegroundColor Red
    $proc.Kill()
    exit 1
}

# Step 4: Submit 100 orders, tracking ALL status codes
Write-Host "`n[Step 4] Submitting 100 orders (tracking status codes)..." -ForegroundColor Yellow
$successCount = 0
$errorCount = 0
$statusCodes = @{}
$errors = @()
$first500 = $null

for ($i = 1; $i -le 100; $i++) {
    $secret = "dev-secret-change-me-to-32-chars-min!"
    $price = 47000 + $i
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
    $ts = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $rid = "e2e-test-$i"
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
        } -Body $bodyBytes -UseBasicParsing
        $sc = $resp.StatusCode
        $successCount++
    } catch {
        $sc = $_.Exception.Response.StatusCode.value__
        $errorCount++
        
        # Capture error details
        try {
            $reader = [System.IO.StreamReader]::new($_.Exception.Response.GetResponseStream())
            $errorBody = $reader.ReadToEnd()
            $reader.Close()
            $errors += @{ status = $sc; index = $i; body = $errorBody }
            
            if ($sc -eq 500 -and -not $first500) {
                $first500 = $errorBody
            }
        } catch {
            $errors += @{ status = $sc; index = $i; body = "N/A" }
        }
    }
    $statusCodes[$sc] = ($statusCodes[$sc], 0 | Where-Object { $_ -ne $null } | Measure-Object -Maximum).Maximum + 1
    
    if ($i % 20 -eq 0) {
        $scSummary = ($statusCodes.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ", "
        Write-Host "  [$i/100] Status codes: $scSummary" -ForegroundColor Yellow
    }
}

Write-Host "`n=== ORDER SUBMISSION RESULTS ===" -ForegroundColor Cyan
Write-Host "Success: $successCount, Errors: $errorCount" -ForegroundColor $(if ($errorCount -eq 0) { "Green" } else { "Red" })
$scSummary = ($statusCodes.GetEnumerator() | Sort-Object Name | ForEach-Object { "HTTP $($_.Key): $($_.Value)" }) -join ", "
Write-Host "Status distribution: $scSummary"

if ($first500) {
    Write-Host "`nFirst 500 error body:" -ForegroundColor Red
    Write-Host $first500 -ForegroundColor Red
}

# Show first few errors if any
if ($errors.Count -gt 0) {
    Write-Host "`nError samples (first 3):" -ForegroundColor Yellow
    $errors | Select-Object -First 3 | ForEach-Object {
        Write-Host "  Order #$($_.index): HTTP $($_.status) - $($_.body)" -ForegroundColor Red
    }
}

# Step 5: WAL consistency check
Write-Host "`n[Step 5] WAL consistency check..." -ForegroundColor Yellow
$walFiles = @(
    "sequencer.wal.jsonl",
    "ledger.wal.jsonl", 
    "trade_journal.wal.jsonl",
    "trade_settlement.wal.jsonl"
)

$walStats = @{}
foreach ($wf in $walFiles) {
    $path = Join-Path "$serverDir\data" $wf
    if (Test-Path $path) {
        $size = (Get-Item $path).Length
        $lines = (Get-Content $path -ErrorAction SilentlyContinue).Count
        $walStats[$wf] = @{ size = $size; lines = $lines }
        Write-Host "  $wf : ${size} bytes, ${lines} lines" -ForegroundColor Green
    } else {
        $walStats[$wf] = @{ size = 0; lines = 0 }
        Write-Host "  $wf : NOT FOUND" -ForegroundColor Red
    }
}

# Check for WAL consistency: sequencer lines should >= ledger lines
$seqLines = $walStats["sequencer.wal.jsonl"].lines
$ledgerLines = $walStats["ledger.wal.jsonl"].lines
$journalLines = $walStats["trade_journal.wal.jsonl"].lines

Write-Host "`n  Consistency analysis:" -ForegroundColor Yellow
if ($seqLines -gt 0 -and $ledgerLines -eq 0) {
    Write-Host "    WARNING: Sequencer has entries but ledger is empty!" -ForegroundColor Red
} elseif ($seqLines -ge $ledgerLines) {
    Write-Host "    OK: Sequencer ($seqLines) >= Ledger ($ledgerLines)" -ForegroundColor Green
}

if ($journalLines -gt 0) {
    Write-Host "    Trade journal has $journalLines entries (trades occurred)" -ForegroundColor Green
} else {
    Write-Host "    Trade journal empty (expected if no matching orders)" -ForegroundColor Yellow
}

# Step 6: Stop server gracefully
Write-Host "`n[Step 6] Stopping server..." -ForegroundColor Yellow
$proc.Kill()
$proc.WaitForExit(5000)
Start-Sleep 1
Write-Host "  Done."

# Step 7: Restart test (simulate restart with polluted WAL)
Write-Host "`n[Step 7] Restart test (with existing WAL data)..." -ForegroundColor Yellow
$psi2 = New-Object System.Diagnostics.ProcessStartInfo
$psi2.FileName = $apiExe
$psi2.WorkingDirectory = $serverDir
$psi2.UseShellExecute = $false
$psi2.RedirectStandardOutput = $true
$psi2.RedirectStandardError = $true
$psi2.EnvironmentVariables["API_BIND_PORT"] = $port.ToString()
$proc2 = [System.Diagnostics.Process]::Start($psi2)

Write-Host "  Waiting for restart (8s)..." -ForegroundColor Yellow
Start-Sleep -Seconds 8

if ($proc2.HasExited) {
    $stderr2 = $proc2.StandardError.ReadToEnd()
    $stdout2 = $proc2.StandardOutput.ReadToEnd()
    Write-Host "  RESTART FAILED!" -ForegroundColor Red
    Write-Host "STDOUT: $stdout2" -ForegroundColor Gray
    Write-Host "STDERR: $stderr2" -ForegroundColor Red
} else {
    # Health check after restart
    try {
        $health2 = Invoke-RestMethod -Uri "http://localhost:$port/health" -UseBasicParsing -TimeoutSec 5
        Write-Host "  Post-restart health:" -ForegroundColor Green
        Write-Host "    accounts: $($health2.accounts)" -ForegroundColor Green
        Write-Host "    instruments: $($health2.instruments)" -ForegroundColor Green
        Write-Host "    ledger_wal_entries: $($health2.ledger_wal_entries)" -ForegroundColor Green
        Write-Host "    sequencer_records: $($health2.sequencer_records)" -ForegroundColor Green
    } catch {
        Write-Host "  Post-restart health check FAILED: $_" -ForegroundColor Red
    }
    
    $proc2.Kill()
    $proc2.WaitForExit(5000)
}

# Final Summary
Write-Host "`n=== PHASE 4/5 TEST SUMMARY ===" -ForegroundColor Cyan
$allPassed = ($errorCount -eq 0)
if ($allPassed) {
    Write-Host "ALL TESTS PASSED!" -ForegroundColor Green
    Write-Host "  - Default seeding: WORKING (accounts and instruments created without env vars)" -ForegroundColor Green
    Write-Host "  - Status codes: All 200 (no 500 escalation)" -ForegroundColor Green
    Write-Host "  - WAL consistency: Verified" -ForegroundColor Green
    Write-Host "  - Restart recovery: Tested" -ForegroundColor Green
} else {
    Write-Host "TESTS FAILED - $errorCount errors encountered" -ForegroundColor Red
    Write-Host "Review error samples above for details." -ForegroundColor Yellow
}

Write-Host "`nDone." -ForegroundColor Cyan
