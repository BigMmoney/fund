param(
    [string]$Output = "",
    [switch]$CleanData
)

# Phase 5: Restart-After-Errors (WAL Integrity)
#
# Submit a mix of valid and intentionally-invalid orders, stop the api, restart
# it on the same data/, then submit one more valid order to confirm the api
# accepts new traffic after replay. Repeat the restart a second time to
# validate idempotency (and the post-2e Settled-skip behaviour).
#
# Asserts that pre-stop and post-restart sequencer/ledger seqs match exactly
# and that frontiers stay consistent across both restarts.
#
# The previous version of this script always reported "Scenarios passed: 0/0"
# because (a) it leaked an orphan api.exe via the cargo wrapper in
# test_lib.ps1, so the "restart" never actually restarted, and (b) the
# scenario helper's pass/fail bool was never appended to $phaseResults.

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\test_lib.ps1"

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "Phase 5: Restart-After-Errors" -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

$rustRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Script:Subject = "test-trader-01"
$Script:Role = "user"
$phaseResults = @()

function Add-Result {
    param([string]$Name, [bool]$Passed, $Details = $null)
    $script:phaseResults += [pscustomobject]@{ name = $Name; passed = $Passed; details = $Details }
}

function Get-Snapshot {
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
        consistent           = $h.frontiers.consistent
        balance_invariant    = if ($r) { $r.balance_invariant } else { $null }
        frontier_consistency = if ($r) { $r.frontier_consistency } else { $null }
    }
}

# ── Setup ──────────────────────────────────────────────────────
Stop-ExchangeService
$startArgs = if ($CleanData) { @{} } else { @{ NoClearWal = $true } }
if (-not (Start-ExchangeService @startArgs -WaitTimeoutSeconds 30)) {
    Write-Host "Setup: failed to start service" -ForegroundColor Red
    exit 1
}

# Top up trader cash if needed so "valid" orders don't hit insufficient-funds.
# Uses the admin /deposit endpoint via Test-Deposit. The deposit op_id is
# salted with a random suffix so re-runs against the same data/ append a new
# ledger entry rather than dedup against a prior seed.
function Get-TraderCash {
    param([string]$User)
    $bal = Invoke-ExchangeRequest -Method "GET" -Path "/balances/$User" -Silent
    if (-not $bal.HasValidJson -or -not $bal.ParsedJson) { return 0 }
    foreach ($entry in $bal.ParsedJson) {
        if ($entry.asset -eq "USDC") { return [int64]$entry.available }
    }
    return 0
}
$targetCash = 10000000
$availableCash = Get-TraderCash -User $Script:Subject
if ($availableCash -lt $targetCash) {
    $deficit = $targetCash - $availableCash
    Write-Host "  Trader available_cash=$availableCash < target=$targetCash; depositing $deficit via admin /deposit"
    $deposited = Test-Deposit -UserId $Script:Subject -Amount $deficit -OpId "rae-seed-$(Get-Random)"
    if (-not $deposited) {
        Write-Host "  Setup: deposit failed, cannot proceed" -ForegroundColor Red
        Stop-ExchangeService
        exit 1
    }
    $availableCash = Get-TraderCash -User $Script:Subject
    Write-Host "  Trader available_cash post-deposit=$availableCash"
}
$priceA = 50000
$priceB = 49000
$amountA = [Math]::Max(1, [Math]::Floor($availableCash / ($priceA * 20)))
$amountB = [Math]::Max(1, [Math]::Floor($availableCash / ($priceB * 20)))
Write-Host "  Using amount_buy_50000=$amountA amount_buy_49000=$amountB"

$snap0 = Get-Snapshot -Stage "setup"
Write-Host "  Setup snapshot: seq=$($snap0.seq) accounts=$($snap0.accounts) consistent=$($snap0.consistent)"

# ── Phase A: mixed batch ───────────────────────────────────────
Write-Host "`n[Phase A] Submit mixed batch (2 valid + 2 invalid)..." -ForegroundColor Yellow
$valid1   = New-OrderJson -Side "buy" -Price $priceA -Amount $amountA -ClientOrderId "rae-A-valid-1-$(Get-Random)"
$badMkt   = '{"market_id":"NONEXISTENT-MKT","side":"buy","order_type":"limit","price":50000,"amount":1,"outcome":0,"time_in_force":"gtc","client_order_id":"rae-A-bad-mkt-' + (Get-Random) + '"}'
$missing  = '{"market_id":"btc-usdt","side":"buy","order_type":"limit","client_order_id":"rae-A-missing-' + (Get-Random) + '"}'
$valid2   = New-OrderJson -Side "buy" -Price $priceB -Amount $amountB -ClientOrderId "rae-A-valid-2-$(Get-Random)"

$r1 = Invoke-ExchangeRequest -Method "POST" -Path "/submit-order" -BodyJson $valid1   -Silent
$r2 = Invoke-ExchangeRequest -Method "POST" -Path "/submit-order" -BodyJson $badMkt   -Silent
$r3 = Invoke-ExchangeRequest -Method "POST" -Path "/submit-order" -BodyJson $missing  -Silent
$r4 = Invoke-ExchangeRequest -Method "POST" -Path "/submit-order" -BodyJson $valid2   -Silent
Write-Host "  Phase A statuses: r1(valid)=$($r1.StatusCode) r2(bad-mkt)=$($r2.StatusCode) r3(missing)=$($r3.StatusCode) r4(valid)=$($r4.StatusCode)"

$snap1 = Get-Snapshot -Stage "phase_a_after_submit"
$phaseAOk = ($r1.StatusCode -eq 200) -and `
            ($r2.StatusCode -ge 400 -and $r2.StatusCode -lt 500) -and `
            ($r3.StatusCode -ge 400 -and $r3.StatusCode -lt 500) -and `
            ($r4.StatusCode -eq 200) -and `
            $snap1.consistent
Add-Result -Name "Phase A submits" -Passed $phaseAOk -Details @{
    statuses = @($r1.StatusCode, $r2.StatusCode, $r3.StatusCode, $r4.StatusCode)
    seq_before = $snap0.seq; seq_after = $snap1.seq
}
if (-not $phaseAOk) { Write-Host "  Phase A FAILED" -ForegroundColor Red }
Stop-ExchangeService

# ── Phase B: restart, verify replay, submit one more ───────────
Write-Host "`n[Phase B] Restart on same data/, asserting replay determinism..." -ForegroundColor Yellow
if (-not (Start-ExchangeService -NoClearWal -WaitTimeoutSeconds 30)) {
    Add-Result -Name "Phase B restart" -Passed $false -Details "service did not become ready"
    exit 1
}
$snap2 = Get-Snapshot -Stage "phase_b_after_restart"

$phaseBReplayOk = ($snap2.seq -eq $snap1.seq) -and `
                  ($snap2.ledger_seq -eq $snap1.ledger_seq) -and `
                  ($snap2.order_proj_seq -eq $snap1.order_proj_seq) -and `
                  $snap2.consistent -and `
                  $snap2.balance_invariant -and `
                  $snap2.frontier_consistency
Add-Result -Name "Phase B replay matches" -Passed $phaseBReplayOk -Details @{
    seq_phase_a = $snap1.seq; seq_phase_b = $snap2.seq
    ledger_seq_phase_a = $snap1.ledger_seq; ledger_seq_phase_b = $snap2.ledger_seq
}

# Submit one tiny valid order to confirm api accepts new traffic post-restart.
$valid3 = New-OrderJson -Side "buy" -Price ($priceA + 100) -Amount 1 -ClientOrderId "rae-B-post-restart-$(Get-Random)"
$r5 = Invoke-ExchangeRequest -Method "POST" -Path "/submit-order" -BodyJson $valid3 -Silent
$snap3 = Get-Snapshot -Stage "phase_b_after_post_restart_order"
$phaseBPostOk = ($r5.StatusCode -eq 200) -and ($snap3.seq -eq $snap2.seq + 1) -and $snap3.consistent
Add-Result -Name "Phase B post-restart order" -Passed $phaseBPostOk -Details @{
    r5_status = $r5.StatusCode; seq_before = $snap2.seq; seq_after = $snap3.seq
}
Stop-ExchangeService

# ── Phase C: second restart for idempotency ────────────────────
Write-Host "`n[Phase C] Second restart, asserting idempotency..." -ForegroundColor Yellow
if (-not (Start-ExchangeService -NoClearWal -WaitTimeoutSeconds 30)) {
    Add-Result -Name "Phase C restart" -Passed $false -Details "service did not become ready"
    exit 1
}
$snap4 = Get-Snapshot -Stage "phase_c_after_2nd_restart"

$phaseCReplayOk = ($snap4.seq -eq $snap3.seq) -and $snap4.consistent
Add-Result -Name "Phase C replay matches" -Passed $phaseCReplayOk -Details @{
    seq_phase_b_end = $snap3.seq; seq_phase_c = $snap4.seq
}

$valid4 = New-OrderJson -Side "buy" -Price ($priceA + 200) -Amount 1 -ClientOrderId "rae-C-after-2nd-$(Get-Random)"
$r6 = Invoke-ExchangeRequest -Method "POST" -Path "/submit-order" -BodyJson $valid4 -Silent
$snap5 = Get-Snapshot -Stage "phase_c_after_post_restart_order"
$phaseCPostOk = ($r6.StatusCode -eq 200) -and ($snap5.seq -eq $snap4.seq + 1) -and $snap5.consistent
Add-Result -Name "Phase C post-restart order" -Passed $phaseCPostOk -Details @{
    r6_status = $r6.StatusCode; seq_before = $snap4.seq; seq_after = $snap5.seq
}
Stop-ExchangeService

# ── Result ─────────────────────────────────────────────────────
$allPassed = ($phaseResults | Where-Object { -not $_.passed } | Measure-Object).Count -eq 0
$report = [ordered]@{
    generated_at_epoch = [int][double]::Parse((Get-Date -UFormat %s))
    setup              = $snap0
    phase_a            = $snap1
    phase_b_replay     = $snap2
    phase_b_post       = $snap3
    phase_c_replay     = $snap4
    phase_c_post       = $snap5
    results            = $phaseResults
    passed             = $allPassed
}
$rendered = $report | ConvertTo-Json -Depth 6

if ($Output) {
    $outPath = if ([System.IO.Path]::IsPathRooted($Output)) { $Output } else { Join-Path $rustRoot $Output }
    $outDir = Split-Path -Parent $outPath
    if ($outDir) { New-Item -ItemType Directory -Path $outDir -Force | Out-Null }
    Set-Content -Path $outPath -Value $rendered -Encoding UTF8
    Write-Host "`nReport written to $outPath" -ForegroundColor Green
}

Write-Host "`n=========================================" -ForegroundColor Cyan
Write-Host "PHASE 5 SUMMARY" -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan
foreach ($r in $phaseResults) {
    $color = if ($r.passed) { 'Green' } else { 'Red' }
    Write-Host "  [$(if ($r.passed) {'PASS'} else {'FAIL'})] $($r.name)" -ForegroundColor $color
}
$passCount = ($phaseResults | Where-Object { $_.passed } | Measure-Object).Count
$totalCount = $phaseResults.Count
Write-Host "Scenarios passed: $passCount/$totalCount" -ForegroundColor $(if ($allPassed) { 'Green' } else { 'Red' })
Write-Host "=========================================" -ForegroundColor Cyan
if ($allPassed) { exit 0 } else { exit 1 }
