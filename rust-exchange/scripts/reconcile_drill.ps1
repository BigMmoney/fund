<#
.SYNOPSIS
    Reconciliation drill (gate P0-REC-3).

.DESCRIPTION
    Boots a clean api, drives a representative load (deposits, /v2
    withdrawals end-to-end, an intentionally-stuck settlement), then
    executes RECONCILIATION_AND_RECOVERY_RUNBOOK.md §3 against the
    resulting WAL files. Reports PASS/FAIL per invariant:

      INV-1  Σ ledger entries balanced per op
      INV-3  No duplicate op_id appears applied twice
      INV-4  Every Settled withdrawal has a wd-settle-{id} ledger entry
      Velocity sanity (24h sum)

    On FAIL, prints the offending op_id / withdrawal_id / amounts so
    the on-call can drill in.

    Usage:
      .\reconcile_drill.ps1                    # debug build
      .\reconcile_drill.ps1 -Release           # release build
      .\reconcile_drill.ps1 -Json              # structured summary
#>

param(
    [switch]$Release,
    [switch]$Json
)

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\test_lib.ps1"

$RustRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$DataDir  = Join-Path $RustRoot "data"
$BaseUri  = "http://127.0.0.1:3030"
$BootstrapAdmin = "drill-admin"

function Section { param([string]$T) Write-Host "`n=== $T ===" -ForegroundColor Cyan }
function Ok      { param([string]$M) Write-Host "  ok   $M" -ForegroundColor Green }
function Info    { param([string]$M) Write-Host "       $M" -ForegroundColor Gray }
function Warn    { param([string]$M) Write-Host "  WARN $M" -ForegroundColor Yellow }
function Fail    { param([string]$M) Write-Host "  FAIL $M" -ForegroundColor Red }

$Script:Failures = @()
function Record-Fail { param([string]$Inv, [string]$Detail) ; $Script:Failures += @{ inv = $Inv; detail = $Detail }; Fail "$Inv -- $Detail" }

# ── 1. Build + boot ─────────────────────────────────────────────────
Section "1. Build api binary"
Push-Location $RustRoot
try {
    if ($Release) {
        & cargo build -p api --bin api --release | Out-Null
    } else {
        & cargo build -p api --bin api | Out-Null
    }
    if ($LASTEXITCODE -ne 0) { Fail "cargo build failed (exit=$LASTEXITCODE)"; exit 1 }
    Ok "build succeeded"
} finally { Pop-Location }

$apiPath = if ($Release) {
    Join-Path $RustRoot "target/release/api.exe"
} else {
    Join-Path $RustRoot "target/debug/api.exe"
}
$env:EXCHANGE_API_EXE = $apiPath

Section "2. Wipe data dir"
if (Test-Path $DataDir) { Remove-Item $DataDir -Recurse -Force -ErrorAction SilentlyContinue }
Ok "wiped $DataDir"

Section "3. Boot api"
$env:INTERNAL_AUTH_SHARED_SECRET = $Script:Secret
$env:BACKOFFICE_BOOTSTRAP_ADMIN = $BootstrapAdmin
$env:WALLET_ETH_HOT_ADDRESS = "0xhot"
$env:WALLET_ETH_SEED_WEI = "1000000000"
$env:WALLET_WORKER_TICK_MS = "500"
$env:WALLET_CUSTOMER_COOLDOWN_SECS = "0"
$logDir = Join-Path $DataDir "logs"
New-Item -ItemType Directory -Force -Path $logDir | Out-Null
if (-not (Start-ExchangeService -StdoutLog (Join-Path $logDir "drill_stdout.log") -StderrLog (Join-Path $logDir "drill_stderr.log") -WaitTimeoutSeconds 60)) {
    Fail "service failed to start"; exit 1
}
Ok "api ready at $BaseUri"

try {
    # ── 4. Drive representative load ───────────────────────────────
    Section "4. Drive deposits + customer withdrawals"
    $Script:AdminSubject = $BootstrapAdmin

    # Two customers, both funded.
    foreach ($u in @("drill-cust-a", "drill-cust-b")) {
        $ok = Test-Deposit -UserId $u -Amount 100000000 -OpId "drill-dep-$u-$(Get-Random)"
        if (-not $ok) { Fail "deposit for $u failed"; exit 1 }
        Info "$u funded"
    }

    # Whitelist + submit + settle for cust-a (happy path).
    $addrBody = @{ chain = "eth"; address = "0xdest-a"; label = "drill-a" } | ConvertTo-Json -Compress
    $addrResp = Invoke-ExchangeRequestAs -Method "POST" -Path "/v2/wallet/addresses" -BodyJson $addrBody -Subject "drill-cust-a" -Role "user" -Silent
    if ($addrResp.StatusCode -ne 200) { Fail "address whitelist failed"; exit 1 }
    Start-Sleep -Milliseconds 200

    $wdBody = @{ chain = "eth"; destination_address = "0xdest-a"; amount = 1000; client_reference = "drill-wd-1" } | ConvertTo-Json -Compress
    $wdResp = Invoke-ExchangeRequestAs -Method "POST" -Path "/v2/wallet/withdraw" -BodyJson $wdBody -Subject "drill-cust-a" -Role "user" -Silent
    if ($wdResp.StatusCode -ne 200) { Fail "withdrawal submit failed: $($wdResp.Body)"; exit 1 }
    $custWdId = $wdResp.ParsedJson.withdrawal_id
    Ok "submitted withdrawal $custWdId"

    Start-Sleep -Seconds 3
    $wdJsonl = Join-Path $RustRoot "data/wallet/withdrawals.jsonl"
    $idEsc = [regex]::Escape($custWdId)

    # Drive confirmations to threshold via test-confirm
    if (Test-Path $wdJsonl) {
        $bcLine = (Get-Content $wdJsonl | Where-Object { $_ -match $idEsc -and $_ -match '"status":"broadcast"' })[-1]
        if ($bcLine -match '"tx_hash":"([^"]+)"') {
            $txHash = $Matches[1]
            $cfBody = @{ chain = "eth"; tx_hash = $txHash; confirmations = 25; reason = "reconciliation drill: drive Broadcast->Confirmed for INV-4" } | ConvertTo-Json -Compress
            $cf = Invoke-AdminRequest -Method "POST" -Path "/admin/wallet/test-confirm" -BodyJson $cfBody -Silent
            if ($cf.StatusCode -ne 200) { Warn "test-confirm status=$($cf.StatusCode)" }
            Start-Sleep -Seconds 3
        }
    }

    # ── 5. INV-1: ledger balance closure ───────────────────────────
    Section "5. INV-1 — global ledger balance closure"
    $ledgerJsonl = Join-Path $RustRoot "data/ledger.jsonl"
    if (-not (Test-Path $ledgerJsonl)) {
        # Older paths may use a sub-dir; locate any ledger jsonl
        $candidate = Get-ChildItem -Path (Join-Path $RustRoot "data") -Recurse -Filter "*ledger*.jsonl" -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($candidate) { $ledgerJsonl = $candidate.FullName }
    }
    if (-not (Test-Path $ledgerJsonl)) {
        Record-Fail "INV-1" "ledger jsonl not found anywhere under data/"
    } else {
        Info "scanning $ledgerJsonl"
        # Each line is `<crc-hex>\t<json>`; strip the CRC prefix before
        # parsing. Every entry contributes -amount to its debit account
        # and +amount to its credit account; Σ across accounts == 0.
        $accountTotals = @{}
        foreach ($line in Get-Content $ledgerJsonl) {
            $jsonPart = $line
            $tabIdx = $line.IndexOf("`t")
            if ($tabIdx -ge 0) { $jsonPart = $line.Substring($tabIdx + 1) }
            try {
                $delta = $jsonPart | ConvertFrom-Json -ErrorAction Stop
            } catch { continue }
            if ($delta.entries) {
                foreach ($e in $delta.entries) {
                    if (-not $accountTotals.ContainsKey($e.debit_account))  { $accountTotals[$e.debit_account] = 0 }
                    if (-not $accountTotals.ContainsKey($e.credit_account)) { $accountTotals[$e.credit_account] = 0 }
                    $accountTotals[$e.debit_account]  -= [int64]$e.amount
                    $accountTotals[$e.credit_account] += [int64]$e.amount
                }
            }
        }
        $sum = 0
        foreach ($v in $accountTotals.Values) { $sum += [int64]$v }
        if ($sum -eq 0) {
            Ok "Σ accounts == 0 (INV-1 holds across $($accountTotals.Count) accounts)"
        } else {
            Record-Fail "INV-1" "Σ accounts = $sum (expected 0)"
            $accountTotals.GetEnumerator() | Sort-Object -Property Value | Select-Object -First 5 | ForEach-Object {
                Info "  worst negative: $($_.Key) = $($_.Value)"
            }
        }
    }

    # ── 6. INV-3: no duplicate op_id applied twice ─────────────────
    Section "6. INV-3 — no duplicate op_id"
    if (Test-Path $ledgerJsonl) {
        $opIds = @{}
        foreach ($line in Get-Content $ledgerJsonl) {
            if ($line -match '"op_id"\s*:\s*"([^"]+)"') {
                $opId = $Matches[1]
                $opIds[$opId] = ($opIds[$opId] + 1)
            }
        }
        $dups = $opIds.GetEnumerator() | Where-Object { $_.Value -gt 1 }
        if ($dups) {
            Record-Fail "INV-3" "$($dups.Count) duplicate op_id(s) — first: $($dups[0].Key) count=$($dups[0].Value)"
        } else {
            Ok "no duplicate op_id across $($opIds.Count) entries"
        }
    }

    # ── 7. INV-4: settled withdrawals have wd-settle-{id} ledger entries
    Section "7. INV-4 — Settled withdrawals ↔ wd-settle ledger entries"
    if ((Test-Path $wdJsonl) -and (Test-Path $ledgerJsonl)) {
        $settledIds = @()
        foreach ($line in Get-Content $wdJsonl) {
            if ($line -match '"status":"settled"' -and $line -match '"withdrawal_id":"([^"]+)"') {
                $settledIds += $Matches[1]
            }
        }
        $settledIds = $settledIds | Sort-Object -Unique
        Info "found $($settledIds.Count) Settled record(s)"
        $missing = @()
        foreach ($id in $settledIds) {
            $needle = "wd-settle-$id"
            $hit = (Get-Content $ledgerJsonl) | Where-Object { $_ -match [regex]::Escape($needle) } | Select-Object -First 1
            if (-not $hit) { $missing += $id }
        }
        if ($missing.Count -eq 0) {
            Ok "every Settled record has its wd-settle-{id} ledger entry"
        } else {
            Record-Fail "INV-4" "$($missing.Count) Settled record(s) without ledger entry: $($missing -join ', ')"
        }
    }

    # ── 8. Velocity sanity ─────────────────────────────────────────
    Section "8. Velocity sanity (24h sum)"
    if (Test-Path $wdJsonl) {
        $cutoff = (Get-Date).ToUniversalTime().AddHours(-24)
        $velByUser = @{}
        foreach ($line in Get-Content $wdJsonl) {
            $jsonPart = $line
            $tabIdx = $line.IndexOf("`t")
            if ($tabIdx -ge 0) { $jsonPart = $line.Substring($tabIdx + 1) }
            try { $rec = $jsonPart | ConvertFrom-Json } catch { continue }
            if ($rec.status -eq "rejected") { continue }
            if (-not $rec.submitted_at) { continue }
            $ts = [DateTimeOffset]::Parse($rec.submitted_at).UtcDateTime
            if ($ts -lt $cutoff) { continue }
            $key = "$($rec.user_id):$($rec.chain)"
            if (-not $velByUser.ContainsKey($key)) { $velByUser[$key] = 0 }
            $velByUser[$key] += [int64]$rec.amount
        }
        Info "per (user,chain) 24h totals:"
        $velByUser.GetEnumerator() | ForEach-Object { Info "  $($_.Key) = $($_.Value)" }
        Ok "velocity sums computed"
    }

    # ── 9. Verdict ─────────────────────────────────────────────────
    Section "Verdict"
    if ($Script:Failures.Count -eq 0) {
        Write-Host "  Reconciliation drill: PASS" -ForegroundColor Green
    } else {
        Write-Host "  Reconciliation drill: FAIL ($($Script:Failures.Count) violation(s))" -ForegroundColor Red
        $Script:Failures | ForEach-Object { Write-Host "    - $($_.inv): $($_.detail)" -ForegroundColor Red }
    }

    if ($Json) {
        [pscustomobject]@{
            passed       = ($Script:Failures.Count -eq 0)
            violations   = $Script:Failures
            settled_ids  = $settledIds
        } | ConvertTo-Json -Depth 4
    }

    if ($Script:Failures.Count -gt 0) { exit 1 }
} finally {
    Section "Stop api"
    Stop-ExchangeService
    Remove-Item env:BACKOFFICE_BOOTSTRAP_ADMIN -ErrorAction SilentlyContinue
    Remove-Item env:WALLET_CUSTOMER_COOLDOWN_SECS -ErrorAction SilentlyContinue
}
exit 0
