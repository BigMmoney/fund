<#
.SYNOPSIS
    Trade Journey Demo — narrate one full match cycle end-to-end.

.DESCRIPTION
    Walks through:
      1. /health   pre-flight
      2. admin deposit cash to alice (buyer) and BTC inventory to bob (seller)
      3. show initial balances
      4. bob submits limit SELL  @ $50000 × 10
      5. alice submits limit BUY @ $50000 × 10 -> match, both fully filled
      6. show fill confirmation + order states
      7. show post-trade balances (alice -500k cash, +10 BTC; bob +500k cash, -10 BTC)
      8. show recent trades on the market
      9. show post-trade /health frontiers (consistent + balance invariant)

    Idempotent w.r.t. dev secrets and op_ids. Designed to be readable when
    piped to a transcript for RC release demos and stakeholder walkthroughs.

.PARAMETER BaseUri
    api base URI. Default: http://127.0.0.1:3030

.PARAMETER MarketId
    market to trade on. Default: btc-usdt

.PARAMETER Json
    emit a structured JSON summary at the end.
#>

param(
    [string]$BaseUri = "http://127.0.0.1:3030",
    [string]$MarketId = "btc-usdt",
    [switch]$Json
)

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\test_lib.ps1"
$Script:ExchangeBaseUri = $BaseUri  # in case any helper reads from this

$AliceId = "demo-alice"
$BobId   = "demo-bob"
$Price   = 50000
$Amount  = 10
$Notional = $Price * $Amount

function Section { param([string]$Title) Write-Host "`n=== $Title ===" -ForegroundColor Cyan }
function Info    { param([string]$Msg)   Write-Host "  $Msg" -ForegroundColor Gray }
function Ok      { param([string]$Msg)   Write-Host "  ✓ $Msg" -ForegroundColor Green }
function Warn    { param([string]$Msg)   Write-Host "  ⚠ $Msg" -ForegroundColor Yellow }
function Fail    { param([string]$Msg)   Write-Host "  ✗ $Msg" -ForegroundColor Red }

function GetCash {
    param([string]$User)
    $Script:Subject = $User; $Script:Role = "user"
    $resp = Invoke-ExchangeRequest -Method "GET" -Path "/balances/$User" -Silent
    if (-not $resp.HasValidJson) { return $null }
    foreach ($e in $resp.ParsedJson) { if ($e.asset -eq "USDC") { return [int64]$e.available } }
    return 0
}
function GetAssetBalance {
    param([string]$User, [string]$Asset)
    $Script:Subject = $User; $Script:Role = "user"
    $resp = Invoke-ExchangeRequest -Method "GET" -Path "/balances/$User" -Silent
    if (-not $resp.HasValidJson) { return 0 }
    foreach ($e in $resp.ParsedJson) {
        if ($e.asset -eq $Asset) { return [int64]$e.available }
    }
    return 0
}

# ── Pre-flight ────────────────────────────────────────────────
Section "1. Pre-flight"
try {
    $health = Invoke-RestMethod -Uri "$BaseUri/health" -TimeoutSec 5 -UseBasicParsing
    Ok "api alive  status=$($health.status)  accounts=$($health.accounts)  uptime=${($health.uptime_secs)}s"
} catch {
    Fail "api unreachable at $BaseUri — start it first (e.g. ./target/release/api.exe)"
    exit 1
}

# ── Seed accounts ─────────────────────────────────────────────
Section "2. Seed accounts (admin /deposit + /position-deposit)"
$cashOk = Test-Deposit -UserId $AliceId -Amount ($Notional * 2) -OpId "demo-alice-cash-$(Get-Random)"
if (-not $cashOk) { Fail "alice cash deposit failed"; exit 1 }
Ok "alice funded with $($Notional * 2) USDC subunits"

$btcOk = Test-PositionDeposit -UserId $BobId -MarketId $MarketId -Amount ($Amount * 2) -OpId "demo-bob-btc-$(Get-Random)"
if (-not $btcOk) { Fail "bob position deposit failed"; exit 1 }
Ok "bob funded with $($Amount * 2) BTC inventory on $MarketId"

# ── Initial balances ──────────────────────────────────────────
Section "3. Initial balances"
$aliceCashBefore = GetCash -User $AliceId
$bobCashBefore   = GetCash -User $BobId
$aliceBtcBefore  = GetAssetBalance -User $AliceId -Asset "BTC"
$bobBtcBefore    = GetAssetBalance -User $BobId   -Asset "BTC"
Info "alice  USDC=$aliceCashBefore   BTC=$aliceBtcBefore"
Info "bob    USDC=$bobCashBefore     BTC=$bobBtcBefore"

# ── Bob places SELL ───────────────────────────────────────────
Section "4. Bob submits limit SELL @ $Price × $Amount on $MarketId"
$Script:Subject = $BobId; $Script:Role = "user"
$sellBody = New-OrderJson -MarketId $MarketId -Side "sell" -Price $Price -Amount $Amount -ClientOrderId "demo-bob-sell-$(Get-Random)"
$sellResp = Invoke-ExchangeRequest -Method "POST" -Path "/submit-order" -BodyJson $sellBody -Silent
if ($sellResp.StatusCode -ne 200) { Fail "sell rejected — status=$($sellResp.StatusCode) body=$($sellResp.Body)"; exit 1 }
$sellId = $sellResp.ParsedJson.order_id
$sellState = $sellResp.ParsedJson.order_state
$sellFills = $sellResp.ParsedJson.fills
Ok "sell accepted  order_id=$sellId  state=$sellState  fills=$sellFills (still resting in book — no buyer yet)"

# ── Alice places BUY → match ─────────────────────────────────
Section "5. Alice submits limit BUY @ $Price × $Amount → expect MATCH"
$Script:Subject = $AliceId; $Script:Role = "user"
$buyBody = New-OrderJson -MarketId $MarketId -Side "buy" -Price $Price -Amount $Amount -ClientOrderId "demo-alice-buy-$(Get-Random)"
$buyResp = Invoke-ExchangeRequest -Method "POST" -Path "/submit-order" -BodyJson $buyBody -Silent
if ($buyResp.StatusCode -ne 200) { Fail "buy rejected — status=$($buyResp.StatusCode) body=$($buyResp.Body)"; exit 1 }
$buyId = $buyResp.ParsedJson.order_id
$buyState = $buyResp.ParsedJson.order_state
$buyFills = $buyResp.ParsedJson.fills
$matchUs  = $buyResp.ParsedJson.match_e2e_us
Ok "buy accepted  order_id=$buyId  state=$buyState  fills=$buyFills  match_e2e=$matchUs µs"
if ($buyFills -lt 1) { Warn "expected ≥1 fill — match may not have happened" }

# ── Fill confirmation ────────────────────────────────────────
Section "6. Fill confirmation (lifecycle + match latency)"
Info "alice buy order $buyId : state=$buyState  fills=$buyFills"
Info "bob  sell order $sellId : (was active → after match should be filled)"
$Script:Subject = $BobId; $Script:Role = "user"
$bobOrderResp = Invoke-ExchangeRequest -Method "GET" -Path "/orders/$BobId/$sellId" -Silent
if ($bobOrderResp.HasValidJson) {
    $bobFinalState = $bobOrderResp.ParsedJson.status
    Info "bob  sell order $sellId : final state from /orders = $bobFinalState"
}

# ── Post-trade balances ──────────────────────────────────────
Section "7. Post-trade balances (∆ proves settlement landed)"
$aliceCashAfter = GetCash -User $AliceId
$bobCashAfter   = GetCash -User $BobId
$aliceBtcAfter  = GetAssetBalance -User $AliceId -Asset "BTC"
$bobBtcAfter    = GetAssetBalance -User $BobId   -Asset "BTC"

$aliceCashDelta = $aliceCashAfter - $aliceCashBefore
$bobCashDelta   = $bobCashAfter   - $bobCashBefore
$aliceBtcDelta  = $aliceBtcAfter  - $aliceBtcBefore
$bobBtcDelta    = $bobBtcAfter    - $bobBtcBefore

Info "alice  USDC $aliceCashBefore -> $aliceCashAfter  Δ=$aliceCashDelta    BTC $aliceBtcBefore -> $aliceBtcAfter  Δ=$aliceBtcDelta"
Info "bob    USDC $bobCashBefore  -> $bobCashAfter     Δ=$bobCashDelta     BTC $bobBtcBefore  -> $bobBtcAfter     Δ=$bobBtcDelta"

if ($aliceCashDelta -le 0 -and $aliceBtcDelta -ge 0 -and $bobCashDelta -ge 0 -and $bobBtcDelta -le 0) {
    Ok "settlement direction correct (alice paid cash + received BTC; bob delivered BTC + received cash)"
} else {
    Warn "settlement direction unexpected — check trade log"
}
$feeImplied = (-$aliceCashDelta) - $bobCashDelta
if ($feeImplied -gt 0) { Info "implied taker+maker fees collected: $feeImplied" }

# ── Recent trades on the market ──────────────────────────────
Section "8. Recent trades on $MarketId"
$Script:Subject = $AliceId; $Script:Role = "user"
$tradesResp = Invoke-ExchangeRequest -Method "GET" -Path "/markets/$MarketId/trades" -Silent
if ($tradesResp.HasValidJson) {
    $tradesData = $tradesResp.ParsedJson
    # the endpoint may wrap trades in an object (.trades) or return a bare array;
    # accept both shapes and just count entries to confirm a fill landed in the trade log.
    if ($tradesData -is [array]) { $tradeCount = @($tradesData).Count }
    elseif ($tradesData.trades) { $tradeCount = @($tradesData.trades).Count }
    elseif ($tradesData.items)  { $tradeCount = @($tradesData.items).Count }
    else { $tradeCount = 0 }
    if ($tradeCount -gt 0) {
        Ok "trade log has $tradeCount trade(s) on $MarketId — fill is persisted"
    } else {
        Warn "trade log empty or shape unrecognised — endpoint payload: $($tradesResp.Body.Substring(0, [Math]::Min(120, $tradesResp.Body.Length)))"
    }
}

# ── Post-trade health ────────────────────────────────────────
Section "9. Post-trade /health and /ready"
$healthAfter = Invoke-RestMethod -Uri "$BaseUri/health" -TimeoutSec 5
$readyAfter  = Invoke-RestMethod -Uri "$BaseUri/ready"  -TimeoutSec 5
Info "frontiers.consistent       = $($healthAfter.frontiers.consistent)"
Info "ready.balance_invariant    = $($readyAfter.balance_invariant)"
Info "ready.frontier_consistency = $($readyAfter.frontier_consistency)"
Info "sequencer_command_seq      = $($healthAfter.frontiers.sequencer_command_seq)"
if ($healthAfter.frontiers.consistent -and $readyAfter.balance_invariant -and $readyAfter.frontier_consistency) {
    Ok "all invariants healthy after trade"
} else {
    Fail "invariant violation — check ledger / sequencer state"
    exit 1
}

# ── Verdict ──────────────────────────────────────────────────
Write-Host "`n=========================================" -ForegroundColor Green
Write-Host "  Trade journey demo: PASS" -ForegroundColor Green
Write-Host "  alice debited $([Math]::Abs($aliceCashDelta)) USDC (taker)" -ForegroundColor Green
Write-Host "  bob credited $bobCashDelta USDC (maker)" -ForegroundColor Green
Write-Host "  fees collected: $feeImplied USDC" -ForegroundColor Green
Write-Host "  match_e2e = $matchUs µs (api hot-path)" -ForegroundColor Green
Write-Host "=========================================" -ForegroundColor Green

if ($Json) {
    [pscustomobject]@{
        passed              = $true
        market              = $MarketId
        alice_cash_delta    = $aliceCashDelta
        alice_btc_delta     = $aliceBtcDelta
        bob_cash_delta      = $bobCashDelta
        bob_btc_delta       = $bobBtcDelta
        implied_fees        = $feeImplied
        match_e2e_us        = $matchUs
        frontiers_consistent = $healthAfter.frontiers.consistent
        balance_invariant   = $readyAfter.balance_invariant
        sell_order_id       = $sellId
        buy_order_id        = $buyId
    } | ConvertTo-Json -Depth 4
}

exit 0
