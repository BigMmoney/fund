# Phase 4/5: WAL Consistency & Replay Recovery Test
# Tests what happens when server restarts with polluted WAL data

$ErrorActionPreference = "Stop"
$serverDir = "d:\pre_trading\rust-exchange"
$dataDir = "$serverDir\data"
$apiExe = if ($env:EXCHANGE_API_EXE) {
    $env:EXCHANGE_API_EXE
} elseif (Test-Path "$serverDir\target\release\api.exe") {
    "$serverDir\target\release\api.exe"
} else {
    "$serverDir\target\x86_64-pc-windows-gnu\release\api.exe"
}

Write-Host "`n=== Phase 4/5: WAL Consistency & Replay Recovery Test ===" -ForegroundColor Cyan

# Step 1: Stop any running server
Write-Host "`n[Step 1] Stopping existing servers..." -ForegroundColor Yellow
Get-Process | Where-Object { $_.ProcessName -eq "api" } | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

# Step 2: Backup current WAL files
Write-Host "`n[Step 2] Backing up current WAL files..." -ForegroundColor Yellow
$backupDir = "$dataDir\wal_backup_$(Get-Date -Format 'yyyyMMdd_HHmmss')"
New-Item -ItemType Directory -Path $backupDir -Force | Out-Null
Get-ChildItem "$dataDir\*.jsonl" -ErrorAction SilentlyContinue | Copy-Item -Destination $backupDir -Force
Write-Host "  Backup saved to: $backupDir"

# Step 3: Analyze current WAL state
Write-Host "`n[Step 3] Analyzing current WAL state..." -ForegroundColor Yellow

$walFiles = @(
    "sequencer.wal.jsonl",
    "ledger.wal.jsonl", 
    "trade_journal.wal.jsonl",
    "trade_settlement.wal.jsonl",
    "matching_snapshots.wal.jsonl"
)

foreach ($wf in $walFiles) {
    $path = Join-Path $dataDir $wf
    if (Test-Path $path) {
        $size = (Get-Item $path).Length
        $lines = (Get-Content $path -ErrorAction SilentlyContinue).Count
        Write-Host "  $wf : ${size} bytes, ${lines} lines" -ForegroundColor Green
    } else {
        Write-Host "  $wf : NOT FOUND" -ForegroundColor Red
    }
}

# Step 4: Show sequencer WAL lifecycle distribution
Write-Host "`n[Step 4] Sequencer WAL lifecycle distribution:" -ForegroundColor Yellow
$seqPath = Join-Path $dataDir "sequencer.wal.jsonl"
if (Test-Path $seqPath) {
    $lifecycleCounts = @{}
    Get-Content $seqPath | ForEach-Object {
        $parts = $_ -split "`t", 2
        if ($parts[1]) {
            try {
                $obj = $parts[1] | ConvertFrom-Json
                $lc = $obj.command.NewOrder.metadata.lifecycle
                $lifecycleCounts[$lc] = ($lifecycleCounts[$lc], 0 | Where-Object { $_ -ne $null } | Measure-Object -Maximum).Maximum + 1
            } catch {}
        }
    }
    foreach ($lc in $lifecycleCounts.Keys) {
        Write-Host "  $lc : $($lifecycleCounts[$lc])" -ForegroundColor $(if ($lc -eq "rejected") { "Red" } else { "Green" })
    }
}

# Step 5: Clear ALL WAL data for clean restart test
Write-Host "`n[Step 5] Clearing ALL WAL data for clean restart test..." -ForegroundColor Yellow
Get-ChildItem "$dataDir\*.jsonl" -ErrorAction SilentlyContinue | Remove-Item -Force
Write-Host "  All WAL files cleared."

# Step 6: Start server with clean state
Write-Host "`n[Step 6] Starting server with CLEAN state..." -ForegroundColor Yellow
$proc = Start-Process -FilePath $apiExe -WorkingDirectory $serverDir -PassThru -WindowStyle Hidden
Write-Host "  Server PID: $($proc.Id)"

# Wait for server to start
Write-Host "`n[Step 7] Waiting for server startup (10s)..." -ForegroundColor Yellow
Start-Sleep -Seconds 10

# Check health endpoint
try {
    $health = Invoke-RestMethod -Uri "http://localhost:3030/internal/health" -UseBasicParsing -TimeoutSec 5
    Write-Host "`n  Health check:" -ForegroundColor Green
    Write-Host "    status: $($health.status)" -ForegroundColor Green
    Write-Host "    ledger_wal_entries: $($health.ledger_wal_entries)" -ForegroundColor Green
    Write-Host "    sequencer_records: $($health.sequencer_records)" -ForegroundColor Green
} catch {
    Write-Host "`n  Health check FAILED: $_" -ForegroundColor Red
}

# Step 8: Run benchmark against clean server
Write-Host "`n[Step 8] Running 50-order benchmark against CLEAN server..." -ForegroundColor Yellow
$successCount = 0
$errorCount = 0
$statusCodes = @{}
$firstError = $null

for ($i = 1; $i -le 50; $i++) {
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
    $rid = "clean-test-$i"
    $payload = "POST`n/submit-order`n`nadmin`nadmin`n`n$ts`n$rid"
    $hmac = [System.Security.Cryptography.HMACSHA256]::new([System.Text.Encoding]::UTF8.GetBytes($secret))
    $sig = [BitConverter]::ToString($hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($payload))).Replace("-","").ToLower()
    $hmac.Dispose()
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $bh = [BitConverter]::ToString($sha.ComputeHash($bodyBytes)).Replace("-","").ToLowerInvariant()
    $sha.Dispose()
    
    try {
        $resp = Invoke-WebRequest -Uri "http://localhost:3030/submit-order" -Method POST -Headers @{
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
        if (-not $firstError) {
            $reader = [System.IO.StreamReader]::new($_.Exception.Response.GetResponseStream())
            $firstError = $reader.ReadToEnd()
            $reader.Close()
        }
    }
    $statusCodes[$sc] = ($statusCodes[$sc], 0 | Where-Object { $_ -ne $null } | Measure-Object -Maximum).Maximum + 1
    
    if ($i % 10 -eq 0) {
        $scSummary = ($statusCodes.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ", "
        Write-Host "  [$i/50] Status codes: $scSummary" -ForegroundColor Yellow
    }
}

Write-Host "`n=== CLEAN SERVER RESULTS ===" -ForegroundColor Cyan
Write-Host "Success: $successCount, Errors: $errorCount" -ForegroundColor $(if ($errorCount -eq 0) { "Green" } else { "Red" })
$scSummary = ($statusCodes.GetEnumerator() | ForEach-Object { "HTTP $($_.Key): $($_.Value)" }) -join ", "
Write-Host "Status distribution: $scSummary"
if ($firstError) {
    Write-Host "First error: $firstError" -ForegroundColor Red
}

# Step 9: Check WAL state after clean run
Write-Host "`n[Step 9] WAL state after clean run:" -ForegroundColor Yellow
foreach ($wf in $walFiles) {
    $path = Join-Path $dataDir $wf
    if (Test-Path $path) {
        $size = (Get-Item $path).Length
        $lines = (Get-Content $path -ErrorAction SilentlyContinue).Count
        Write-Host "  $wf : ${size} bytes, ${lines} lines" -ForegroundColor Green
    } else {
        Write-Host "  $wf : NOT FOUND" -ForegroundColor Red
    }
}

# Step 10: Stop server
Write-Host "`n[Step 10] Stopping server..." -ForegroundColor Yellow
Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 1

Write-Host "`n=== Phase 4 Complete ===" -ForegroundColor Cyan
Write-Host "Next: Phase 5 - Fix WAL consistency / recovery pipeline" -ForegroundColor Magenta
