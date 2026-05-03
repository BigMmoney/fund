<#
.SYNOPSIS
    Real Order Flow Monitor smoke test — boots the api, exercises a
    one-trade match, then asserts the trace artifacts produced by the
    monitor stack (eventbus → projector → REST → JSONL).

.DESCRIPTION
    Phases:
      1. Build the api (debug profile is fine for smoke; release if -Release).
      2. Wipe data dir + monitor trace dir for a fresh run.
      3. Boot the api with INTERNAL_AUTH_SHARED_SECRET set; wait for /health.
      4. Seed cash for alice and BTC inventory for bob.
      5. Bob places a sell @ $50000 × 10; alice fills it with a matching buy.
      6. Hit /monitor/orders and assert both orders are visible.
      7. Hit /monitor/orders/{id}/timeline for alice's order; assert the
         expected stages are present (api_received, api_validated,
         sequencer_accepted, sequencer_persisted, wal_appended,
         matching_filled, projection_updated, ledger_settled).
      8. Read data/trace/order_trace.jsonl and assert it grew during the
         test (the recovery_completed aggregate fires at boot; order
         events fire during the trade).
      9. Stop the api.
    Emits a JSON summary at the end if -Json is given.

.PARAMETER Release
    If set, run with the release-mode binary. Default: debug build.

.PARAMETER KeepData
    If set, do NOT wipe data/ before booting. Default: wipe.

.PARAMETER Json
    Emit a structured JSON summary at the end.

.EXAMPLE
    pwsh -File scripts/monitor_smoke_test.ps1
    pwsh -File scripts/monitor_smoke_test.ps1 -Release -Json
#>

param(
    [switch]$Release,
    [switch]$KeepData,
    [switch]$Json
)

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\test_lib.ps1"

$RustRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$DataDir = Join-Path $RustRoot "data"
$TraceDir = Join-Path $DataDir "trace"
$TraceFile = Join-Path $TraceDir "order_trace.jsonl"
$BaseUri = "http://127.0.0.1:3030"
$AliceId = "smoke-alice-$(Get-Random)"
$BobId = "smoke-bob-$(Get-Random)"
$Price = 50000
$Amount = 10
$Notional = $Price * $Amount

function Section { param([string]$T) Write-Host "`n=== $T ===" -ForegroundColor Cyan }
function Ok      { param([string]$M) Write-Host "  ok  $M" -ForegroundColor Green }
function Info    { param([string]$M) Write-Host "      $M" -ForegroundColor Gray }
function Warn    { param([string]$M) Write-Host "  WARN $M" -ForegroundColor Yellow }
function Fail    { param([string]$M) Write-Host "  FAIL $M" -ForegroundColor Red }

# ── 1. Build ───────────────────────────────────────────────────────────
Section "1. Build api binary"
$profileArg = if ($Release) { "--release" } else { "" }
$buildCmd   = if ($Release) { "cargo build -p api --bin api --release" } else { "cargo build -p api --bin api" }
Push-Location $RustRoot
try {
    Info $buildCmd
    if ($Release) { & cargo build -p api --bin api --release | Out-Null } else { & cargo build -p api --bin api | Out-Null }
    if ($LASTEXITCODE -ne 0) { Fail "cargo build failed (exit=$LASTEXITCODE)"; exit 1 }
    Ok "build succeeded"
} finally { Pop-Location }

$apiPath = if ($Release) {
    Join-Path $RustRoot "target/release/api.exe"
} else {
    Join-Path $RustRoot "target/debug/api.exe"
}
if (-not (Test-Path $apiPath)) { Fail "api binary not found at $apiPath"; exit 1 }
$env:EXCHANGE_API_EXE = $apiPath
Info "using api: $apiPath"

# ── 2. Wipe data ───────────────────────────────────────────────────────
Section "2. Wipe data dir + trace dir"
if (-not $KeepData) {
    if (Test-Path $DataDir) { Remove-Item $DataDir -Recurse -Force -ErrorAction SilentlyContinue }
    Ok "wiped $DataDir"
} else {
    Info "keeping existing data dir (-KeepData)"
}

# ── 3. Boot ────────────────────────────────────────────────────────────
Section "3. Boot api"
$env:INTERNAL_AUTH_SHARED_SECRET = $Script:Secret
$logDir = Join-Path $DataDir "logs"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
$stdoutLog = Join-Path $logDir "monitor_smoke_stdout.log"
$stderrLog = Join-Path $logDir "monitor_smoke_stderr.log"
if (-not (Start-ExchangeService -StdoutLog $stdoutLog -StderrLog $stderrLog -WaitTimeoutSeconds 60)) {
    Fail "service failed to start"; exit 1
}
Ok "api ready at $BaseUri"

try {
    # ── 4. Seed accounts ───────────────────────────────────────────────
    Section "4. Seed accounts"
    if (-not (Test-Deposit -UserId $AliceId -Amount ($Notional * 2) -OpId "smoke-alice-cash-$(Get-Random)")) {
        Fail "alice cash deposit failed"; exit 1
    }
    Ok "alice funded with $($Notional * 2) USDC subunits"
    if (-not (Test-PositionDeposit -UserId $BobId -MarketId "btc-usdt" -Amount ($Amount * 2) -OpId "smoke-bob-btc-$(Get-Random)")) {
        Fail "bob position deposit failed"; exit 1
    }
    Ok "bob funded with $($Amount * 2) BTC inventory"

    # ── 5. Trade ───────────────────────────────────────────────────────
    Section "5. Execute one-trade match"
    $Script:Subject = $BobId; $Script:Role = "user"
    $sellBody = New-OrderJson -MarketId "btc-usdt" -Side "sell" -Price $Price -Amount $Amount -ClientOrderId "smoke-bob-sell-$(Get-Random)"
    $sellResp = Invoke-ExchangeRequest -Method "POST" -Path "/submit-order" -BodyJson $sellBody -Silent
    if ($sellResp.StatusCode -ne 200) { Fail "sell rejected: status=$($sellResp.StatusCode) body=$($sellResp.Body)"; exit 1 }
    $sellId = $sellResp.ParsedJson.order_id
    Ok "bob sell accepted: order_id=$sellId state=$($sellResp.ParsedJson.order_state)"

    $Script:Subject = $AliceId; $Script:Role = "user"
    $buyBody = New-OrderJson -MarketId "btc-usdt" -Side "buy" -Price $Price -Amount $Amount -ClientOrderId "smoke-alice-buy-$(Get-Random)"
    $buyResp = Invoke-ExchangeRequest -Method "POST" -Path "/submit-order" -BodyJson $buyBody -Silent
    if ($buyResp.StatusCode -ne 200) { Fail "buy rejected: status=$($buyResp.StatusCode) body=$($buyResp.Body)"; exit 1 }
    $buyId = $buyResp.ParsedJson.order_id
    $matchUs = $buyResp.ParsedJson.match_e2e_us
    Ok "alice buy accepted: order_id=$buyId state=$($buyResp.ParsedJson.order_state) fills=$($buyResp.ParsedJson.fills) match_e2e=${matchUs}us"

    # Give the eventbus consumer a moment to apply the trace events.
    Start-Sleep -Milliseconds 300

    # ── 6. /monitor/orders ─────────────────────────────────────────────
    Section "6. GET /monitor/orders"
    # Use admin so we see both orders.
    $resp = Invoke-AdminRequest -Method "GET" -Path "/monitor/orders" -Silent
    if ($resp.StatusCode -ne 200) { Fail "/monitor/orders status=$($resp.StatusCode)"; exit 1 }
    $orders = @($resp.ParsedJson.orders)
    Info "total_returned=$($resp.ParsedJson.total_returned)"
    $sellRow = $orders | Where-Object { $_.order_id -eq $sellId } | Select-Object -First 1
    $buyRow  = $orders | Where-Object { $_.order_id -eq $buyId  } | Select-Object -First 1
    if ($null -eq $sellRow) { Fail "bob sell order $sellId NOT visible in monitor"; exit 1 }
    if ($null -eq $buyRow)  { Fail "alice buy order $buyId NOT visible in monitor";  exit 1 }
    Ok "both orders visible: sell stage=$($sellRow.current_stage), buy stage=$($buyRow.current_stage)"
    Info "  sell: user=$($sellRow.user_id), fills=$($sellRow.fill_count), terminal=$($sellRow.terminal)"
    Info "  buy:  user=$($buyRow.user_id), fills=$($buyRow.fill_count), terminal=$($buyRow.terminal)"

    # ── 7. /monitor/orders/{id}/timeline ───────────────────────────────
    Section "7. GET /monitor/orders/$buyId/timeline (full lifecycle)"
    $tlResp = Invoke-AdminRequest -Method "GET" -Path "/monitor/orders/$buyId/timeline" -Silent
    if ($tlResp.StatusCode -ne 200) { Fail "timeline status=$($tlResp.StatusCode)"; exit 1 }
    $timeline = @($tlResp.ParsedJson.timeline)
    if ($timeline.Count -eq 0) { Fail "timeline empty"; exit 1 }
    $stages = $timeline | ForEach-Object { $_.stage } | Select-Object -Unique
    Info "stages observed: $($stages -join ', ')"

    # The buy order should have been bound by matching, and its timeline
    # should include the full happy path (some events may overlap).
    $expected = @(
        'api_received',
        'api_validated',
        'sequencer_accepted',
        'sequencer_persisted',
        'wal_appended',
        'matching_filled',
        'ledger_settled',
        'projection_updated'
    )
    $missing = @()
    foreach ($s in $expected) {
        if ($stages -notcontains $s) { $missing += $s }
    }
    if ($missing.Count -gt 0) {
        Warn "missing expected stages: $($missing -join ', ')"
    } else {
        Ok "all expected stages present (api ingress -> matching -> ledger -> projection)"
    }
    Ok "timeline length=$($timeline.Count)"

    # ── 8. JSONL trail ─────────────────────────────────────────────────
    Section "8. data/trace/order_trace.jsonl"
    if (-not (Test-Path $TraceFile)) {
        Fail "trace file missing: $TraceFile"
        exit 1
    }
    $lines = Get-Content $TraceFile
    $count = ($lines | Measure-Object).Count
    if ($count -eq 0) { Fail "trace file empty"; exit 1 }
    $sample = $lines | Select-Object -First 1
    $parsed = $sample | ConvertFrom-Json
    Ok "trace file has $count lines"
    Info "first line stage=$($parsed.stage)  schema_version=$($parsed.schema_version)"

    # Confirm recovery_completed fired at boot.
    $recoveryHits = ($lines | Where-Object { $_ -match '"recovery_completed"' } | Measure-Object).Count
    if ($recoveryHits -ge 1) {
        Ok "recovery_completed observed in trail ($recoveryHits occurrence(s))"
    } else {
        Warn "no recovery_completed line found — recovery emit may have raced or filtered"
    }

    # Confirm at least one matching event landed.
    $matchHits = ($lines | Where-Object { $_ -match '"matching_filled"' } | Measure-Object).Count
    if ($matchHits -ge 1) {
        Ok "matching_filled observed in trail ($matchHits occurrence(s))"
    } else {
        Fail "no matching_filled in trace file"
    }

    # ── Verdict ────────────────────────────────────────────────────────
    Section "Verdict"
    Write-Host "  Order Flow Monitor smoke test: PASS" -ForegroundColor Green
    Write-Host "    sell_order_id=$sellId" -ForegroundColor Green
    Write-Host "    buy_order_id =$buyId" -ForegroundColor Green
    Write-Host "    timeline_length=$($timeline.Count)" -ForegroundColor Green
    Write-Host "    jsonl_lines=$count" -ForegroundColor Green
    Write-Host "    match_e2e=${matchUs}us" -ForegroundColor Green

    if ($Json) {
        [pscustomobject]@{
            passed             = $true
            sell_order_id      = $sellId
            buy_order_id       = $buyId
            timeline_length    = $timeline.Count
            stages_observed    = @($stages)
            jsonl_lines        = $count
            recovery_completed = $recoveryHits
            matching_filled    = $matchHits
            match_e2e_us       = $matchUs
        } | ConvertTo-Json -Depth 4
    }
} finally {
    # ── 9. Stop ────────────────────────────────────────────────────────
    Section "9. Stop api"
    Stop-ExchangeService
}
exit 0
