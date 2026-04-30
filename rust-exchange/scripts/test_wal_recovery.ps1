param(
    [string]$Output = ""
)

# Phase 4: WAL Replay Recovery Test (real, no wipe)
#
# Validates that on restart, the api binary deterministically reconstructs
# in-memory matching engine state from the on-disk sequencer/ledger WAL.
# The previous version of this script wiped the WAL before restart — that
# does NOT exercise replay. This version preserves the WAL and asserts
# pre-stop and post-restart state are identical.
#
# Phases:
#   A. Generate WAL state (clean start, submit a small mixed batch).
#   B. Stop, then restart against the same data/. Assert seq/frontiers
#      identical to Phase A pre-stop.
#   C. Stop, restart again. Validates idempotency and that the post-2e
#      Settled-skip behaviour holds across multiple bootstrap cycles.

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\test_lib.ps1"

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "Phase 4: WAL Replay Recovery Test" -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

$rustRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$dataDir  = Join-Path $rustRoot "data"
$seqWal   = Join-Path $dataDir "sequencer.wal.jsonl"
$ledgerWal = Join-Path $dataDir "ledger.wal.jsonl"

function Get-WalLineCount {
    param([string]$Path)
    if (-not (Test-Path $Path)) { return 0 }
    return (Get-Content $Path -ErrorAction SilentlyContinue | Measure-Object).Count
}

function New-Snapshot {
    param([string]$Stage)
    $h = Wait-ExchangeReady -TimeoutSeconds 5
    if (-not $h) { throw "Service not ready when capturing snapshot at stage '$Stage'" }
    $r = Get-ExchangeReadiness
    return [pscustomobject]@{
        stage                = $Stage
        seq                  = $h.frontiers.sequencer_command_seq
        ledger_seq           = $h.frontiers.ledger_command_seq
        order_proj_seq       = $h.frontiers.order_projection_command_seq
        accounts             = $h.accounts
        seen_op_ids          = $h.seen_op_ids
        consistent           = $h.frontiers.consistent
        balance_invariant    = if ($r) { $r.balance_invariant } else { $null }
        frontier_consistency = if ($r) { $r.frontier_consistency } else { $null }
        wal_seq_lines        = (Get-WalLineCount $seqWal)
        wal_ledger_lines     = (Get-WalLineCount $ledgerWal)
    }
}

# ── Phase A: clean start, generate WAL ─────────────────────────
Write-Host "`n[Phase A] Clean start, generating WAL state..." -ForegroundColor Yellow
Stop-ExchangeService
if (-not (Start-ExchangeService -WaitTimeoutSeconds 30)) {
    Write-Host "Phase A: failed to start service" -ForegroundColor Red
    exit 1
}
$snapA0 = New-Snapshot -Stage "phase_a_initial"
Assert-FrontiersConsistent -Health (Wait-ExchangeReady -TimeoutSeconds 5) -Stage "phase_a_initial"

# Submit a deterministic mixed batch. Sized to use a small fraction of the
# seeded test-trader-01 cash so neither order hits insufficient-funds during
# replay even after settlement debits.
$Script:Subject = "test-trader-01"
$Script:Role = "user"
$ordersSubmitted = 0
$buy1 = New-OrderJson -Side "buy"  -Price 50000 -Amount 10 -ClientOrderId "wal-recov-buy-1"
$bad  = '{"market_id":"NONEXISTENT-MKT","side":"buy","order_type":"limit","price":50000,"amount":10,"outcome":0,"time_in_force":"gtc","client_order_id":"wal-recov-bad-mkt"}'
$miss = '{"market_id":"btc-usdt","side":"buy","order_type":"limit","client_order_id":"wal-recov-missing"}'
$buy2 = New-OrderJson -Side "buy"  -Price 49000 -Amount 5  -ClientOrderId "wal-recov-buy-2"

$r1 = Invoke-ExchangeRequest -Method "POST" -Path "/submit-order" -BodyJson $buy1 -Silent
$r2 = Invoke-ExchangeRequest -Method "POST" -Path "/submit-order" -BodyJson $bad  -Silent
$r3 = Invoke-ExchangeRequest -Method "POST" -Path "/submit-order" -BodyJson $miss -Silent
$r4 = Invoke-ExchangeRequest -Method "POST" -Path "/submit-order" -BodyJson $buy2 -Silent
Write-Host "  Phase A submits: r1=$($r1.StatusCode) r2=$($r2.StatusCode) r3=$($r3.StatusCode) r4=$($r4.StatusCode)"
if ($r1.StatusCode -eq 200) { $ordersSubmitted++ }
if ($r4.StatusCode -eq 200) { $ordersSubmitted++ }

$snapA1 = New-Snapshot -Stage "phase_a_after_orders"
Assert-FrontiersConsistent -Health (Wait-ExchangeReady -TimeoutSeconds 5) -Stage "phase_a_after_orders"
Write-Host "  Phase A end: seq=$($snapA1.seq) ledger_seq=$($snapA1.ledger_seq) wal_seq_lines=$($snapA1.wal_seq_lines)"
Stop-ExchangeService

# ── Phase B: restart on same data/, assert exact replay ────────
Write-Host "`n[Phase B] Restart on same data/, asserting exact replay..." -ForegroundColor Yellow
if (-not (Start-ExchangeService -NoClearWal -WaitTimeoutSeconds 30)) {
    Write-Host "Phase B: replay BOOTSTRAP FAILED — service did not become ready (api.exe likely panicked)" -ForegroundColor Red
    exit 1
}
$snapB = New-Snapshot -Stage "phase_b_after_replay"

try {
    Assert-Eq $snapA1.seq            $snapB.seq            "phase_b sequencer_command_seq matches phase_a"
    Assert-Eq $snapA1.ledger_seq     $snapB.ledger_seq     "phase_b ledger_command_seq matches phase_a"
    Assert-Eq $snapA1.order_proj_seq $snapB.order_proj_seq "phase_b order_projection_command_seq matches phase_a"
    Assert-Eq $snapA1.accounts       $snapB.accounts       "phase_b accounts count matches phase_a"
    Assert-Eq $true                  $snapB.consistent     "phase_b frontiers.consistent"
    Assert-Eq $true                  $snapB.balance_invariant "phase_b balance_invariant"
    Assert-Eq $true                  $snapB.frontier_consistency "phase_b frontier_consistency"
} catch {
    Write-Host "Phase B FAILED: $_" -ForegroundColor Red
    Stop-ExchangeService
    exit 1
}
Write-Host "  Phase B replay OK: seq matches ($($snapB.seq))" -ForegroundColor Green
Stop-ExchangeService

# ── Phase C: second restart for idempotency ────────────────────
Write-Host "`n[Phase C] Second restart on same data/..." -ForegroundColor Yellow
if (-not (Start-ExchangeService -NoClearWal -WaitTimeoutSeconds 30)) {
    Write-Host "Phase C: SECOND replay BOOTSTRAP FAILED" -ForegroundColor Red
    exit 1
}
$snapC = New-Snapshot -Stage "phase_c_after_2nd_replay"

try {
    Assert-Eq $snapA1.seq        $snapC.seq        "phase_c sequencer_command_seq matches phase_a"
    Assert-Eq $snapA1.ledger_seq $snapC.ledger_seq "phase_c ledger_command_seq matches phase_a"
    Assert-Eq $true              $snapC.consistent "phase_c frontiers.consistent"
} catch {
    Write-Host "Phase C FAILED: $_" -ForegroundColor Red
    Stop-ExchangeService
    exit 1
}
Write-Host "  Phase C replay OK: seq matches ($($snapC.seq))" -ForegroundColor Green
Stop-ExchangeService

# ── Result ─────────────────────────────────────────────────────
$report = [ordered]@{
    generated_at_epoch       = [int][double]::Parse((Get-Date -UFormat %s))
    phase_a_initial          = $snapA0
    phase_a_after_orders     = $snapA1
    phase_b_after_replay     = $snapB
    phase_c_after_2nd_replay = $snapC
    orders_submitted         = $ordersSubmitted
    passed                   = $true
}
$rendered = $report | ConvertTo-Json -Depth 6

if ($Output) {
    $outPath = if ([System.IO.Path]::IsPathRooted($Output)) { $Output } else { Join-Path $rustRoot $Output }
    $outDir = Split-Path -Parent $outPath
    if ($outDir) { New-Item -ItemType Directory -Path $outDir -Force | Out-Null }
    Set-Content -Path $outPath -Value $rendered -Encoding UTF8
    Write-Host "Report written to $outPath" -ForegroundColor Green
}

Write-Host "`n=========================================" -ForegroundColor Green
Write-Host "Phase 4 PASSED: WAL replay recovery clean across 2 restarts" -ForegroundColor Green
Write-Host "=========================================" -ForegroundColor Green
exit 0
