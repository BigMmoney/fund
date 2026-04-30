param(
    [int]$Iterations = 5,
    [int]$CommandCount = 5000,
    [int]$UserPoolSize = 8,
    [int]$BurstSize = 25,
    [int]$BurstSleepMs = 50,
    [string]$Output = "",
    [int]$RtoBudgetSeconds = 30
)

# RTO / RPO measurement harness.
#
# For each iteration:
#   1. Start api on a fresh data/ (clean WAL).
#   2. Submit -CommandCount orders via the authenticated /submit-order path.
#   3. Capture pre-kill seq from /health.
#   4. Kill api hard (Stop-Process -Force, no graceful drain) — simulates crash.
#   5. Wait for port 3030 to release.
#   6. Start api on the SAME data/ — measures RTO from Start-Process to first
#      successful /health.status==ok response.
#   7. Capture post-recovery seq from /health.
#   8. RPO assertion: post-recovery seq must equal pre-kill seq (no committed
#      command lost). Any loss is a hard failure.
#
# Output: JSON report with per-iteration timings + aggregate p50/p95/p99 RTO,
#         worst RPO loss count, and PASS/FAIL verdict.

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\test_lib.ps1"

$rustRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Script:Role = "user"

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "RTO/RPO measurement: Iterations=$Iterations CommandCount=$CommandCount UserPoolSize=$UserPoolSize BurstSize=$BurstSize" -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

function Get-UserPool {
    param([int]$Size)
    return @(1..$Size | ForEach-Object { "rto-trader-{0:D2}" -f $_ })
}

function Seed-UserPool {
    param([string[]]$Users, [int]$Iter)
    foreach ($u in $Users) {
        $ok = Test-Deposit -UserId $u -Amount 100000000 -OpId "rto-seed-${Iter}-${u}-$(Get-Random)"
        if (-not $ok) { return $false }
    }
    return $true
}

# Sequential submitter that rotates across a pool of funded users so per-user
# rate limits don't trigger. After every $BurstSize requests we sleep briefly
# to let the IP-rate-limit window roll. RTO/RPO accuracy doesn't depend on
# submit throughput — what matters is the WAL state at kill time.
function Submit-Orders {
    param([int]$Total, [string[]]$Users, [int]$BurstSize, [int]$BurstSleepMs)
    $started = Get-Date
    $accepted = 0
    $rejected = 0
    for ($i = 0; $i -lt $Total; $i++) {
        $u = $Users[$i % $Users.Count]
        $Script:Subject = $u
        $price = 50000 + ($i % 100)
        $body = New-OrderJson -Side "buy" -Price $price -Amount 1 -ClientOrderId "rto-i${i}-$(Get-Random)"
        try {
            $resp = Invoke-ExchangeRequest -Method "POST" -Path "/submit-order" -BodyJson $body -Silent
            if ($resp.StatusCode -eq 200) { $accepted++ } else { $rejected++ }
        } catch { $rejected++ }
        if ($BurstSize -gt 0 -and (($i + 1) % $BurstSize) -eq 0 -and $BurstSleepMs -gt 0) {
            Start-Sleep -Milliseconds $BurstSleepMs
        }
    }
    $elapsed = ((Get-Date) - $started).TotalSeconds
    return @{ accepted = $accepted; rejected = $rejected; elapsed_s = $elapsed }
}

function Wait-PortReleased {
    param([int]$Port = 3030, [int]$TimeoutSeconds = 10)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        $listening = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue
        if (-not $listening) { return $true }
        Start-Sleep -Milliseconds 200
    }
    return $false
}

$results = New-Object System.Collections.Generic.List[object]

for ($iter = 1; $iter -le $Iterations; $iter++) {
    Write-Host "`n--- Iteration $iter / $Iterations ---" -ForegroundColor Yellow
    Stop-ExchangeService

    # ── Fresh start ──
    if (-not (Start-ExchangeService -WaitTimeoutSeconds 30)) {
        $results.Add([pscustomobject]@{ iter = $iter; passed = $false; failure = "fresh start timeout" })
        continue
    }

    # ── Seed N users with cash so /submit-order can succeed without per-user rate-limit clamps ──
    $users = Get-UserPool -Size $UserPoolSize
    if (-not (Seed-UserPool -Users $users -Iter $iter)) {
        Write-Host "  user-pool seeding failed" -ForegroundColor Red
        Stop-ExchangeService
        $results.Add([pscustomobject]@{ iter = $iter; passed = $false; failure = "deposit failed" })
        continue
    }

    # ── Submit traffic (sequential, rotating across users + burst sleep) ──
    $submit = Submit-Orders -Total $CommandCount -Users $users -BurstSize $BurstSize -BurstSleepMs $BurstSleepMs
    Write-Host ("  submitted: accepted={0}/{1} rejected={2} in {3:N1}s" -f $submit.accepted, $CommandCount, $submit.rejected, $submit.elapsed_s)

    $hPre = Wait-ExchangeReady -TimeoutSeconds 5
    if (-not $hPre) {
        Stop-ExchangeService
        $results.Add([pscustomobject]@{ iter = $iter; passed = $false; failure = "pre-kill /health not ready" })
        continue
    }
    $seqPre        = $hPre.frontiers.sequencer_command_seq
    $ledgerSeqPre  = $hPre.frontiers.ledger_command_seq
    $accountsPre   = $hPre.accounts
    Write-Host ("  pre-kill: seq={0} ledger_seq={1} accounts={2}" -f $seqPre, $ledgerSeqPre, $accountsPre)

    # ── Hard kill (Stop-Process -Force = SIGTERM-equivalent on Windows; no graceful drain) ──
    $killStart = Get-Date
    $apiPid = $Script:ApiProcess.Id
    Stop-Process -Id $apiPid -Force -ErrorAction SilentlyContinue
    while (-not (Get-Process -Id $apiPid -ErrorAction SilentlyContinue).HasExited) {
        Start-Sleep -Milliseconds 50
        if (((Get-Date) - $killStart).TotalSeconds -gt 5) { break }
    }
    if (-not (Wait-PortReleased -TimeoutSeconds 10)) {
        Write-Host "  port 3030 did not release within 10s" -ForegroundColor Red
        $results.Add([pscustomobject]@{ iter = $iter; passed = $false; failure = "port hung" })
        continue
    }

    # ── RTO: time from Start-Process to /health.status == ok ──
    $rtoStart = Get-Date
    $startedOk = Start-ExchangeService -NoClearWal -WaitTimeoutSeconds $RtoBudgetSeconds
    $rtoSeconds = ((Get-Date) - $rtoStart).TotalSeconds

    if (-not $startedOk) {
        Stop-ExchangeService
        $results.Add([pscustomobject]@{
            iter = $iter; passed = $false; failure = "post-kill restart failed"
            rto_seconds = $rtoSeconds; seq_pre = $seqPre; seq_post = $null
        })
        continue
    }

    $hPost = Wait-ExchangeReady -TimeoutSeconds 5
    $seqPost       = $hPost.frontiers.sequencer_command_seq
    $ledgerSeqPost = $hPost.frontiers.ledger_command_seq
    $accountsPost  = $hPost.accounts
    Write-Host ("  post-replay: seq={0} ledger_seq={1} accounts={2} rto={3:N3}s" -f `
                 $seqPost, $ledgerSeqPost, $accountsPost, $rtoSeconds)

    # ── RPO: any committed command lost? Per the api architecture, /health
    # returns sequencer_command_seq from durable WAL state. Loss = pre-kill seq
    # > post-recovery seq. Equality means perfect RPO=0. ──
    $rpoLoss = [Math]::Max(0, $seqPre - $seqPost)
    $consistent = $hPost.frontiers.consistent
    $r = Get-ExchangeReadiness
    $balanceInv = if ($r) { $r.balance_invariant } else { $false }

    $passed = ($rpoLoss -eq 0) -and `
              ($rtoSeconds -le $RtoBudgetSeconds) -and `
              $consistent -and $balanceInv

    Stop-ExchangeService

    $results.Add([pscustomobject]@{
        iter             = $iter
        rto_seconds      = [Math]::Round($rtoSeconds, 3)
        seq_pre          = $seqPre
        seq_post         = $seqPost
        ledger_seq_pre   = $ledgerSeqPre
        ledger_seq_post  = $ledgerSeqPost
        rpo_loss_count   = $rpoLoss
        accepted_orders  = $submit.accepted
        rejected_orders  = $submit.rejected
        consistent       = $consistent
        balance_inv      = $balanceInv
        passed           = $passed
    })
}

# ── Aggregate ──
$rtoValues = @($results | Where-Object { $_.rto_seconds -ne $null } | ForEach-Object { $_.rto_seconds } | Sort-Object)
$worstRpo  = ($results | Measure-Object -Property rpo_loss_count -Maximum).Maximum
$allPassed = ($results | Where-Object { -not $_.passed } | Measure-Object).Count -eq 0

function PercentileOf { param([double[]]$Sorted, [double]$P)
    if ($Sorted.Count -eq 0) { return $null }
    $idx = [Math]::Min($Sorted.Count - 1, [Math]::Floor($P * $Sorted.Count))
    return $Sorted[$idx]
}

$report = [ordered]@{
    schema_version       = 1
    generated_at_epoch   = [int][double]::Parse((Get-Date -UFormat %s))
    iterations           = $Iterations
    command_count        = $CommandCount
    concurrency          = $Concurrency
    rto_budget_seconds   = $RtoBudgetSeconds
    rto_seconds_p50      = (PercentileOf -Sorted $rtoValues -P 0.50)
    rto_seconds_p95      = (PercentileOf -Sorted $rtoValues -P 0.95)
    rto_seconds_p99      = (PercentileOf -Sorted $rtoValues -P 0.99)
    rto_seconds_max      = if ($rtoValues.Count -gt 0) { $rtoValues[-1] } else { $null }
    rpo_worst_loss_count = $worstRpo
    iterations_passed    = ($results | Where-Object { $_.passed } | Measure-Object).Count
    iterations_failed    = ($results | Where-Object { -not $_.passed } | Measure-Object).Count
    per_iteration        = $results
    passed               = $allPassed
}
$rendered = $report | ConvertTo-Json -Depth 8

if ($Output) {
    $outPath = if ([System.IO.Path]::IsPathRooted($Output)) { $Output } else { Join-Path $rustRoot $Output }
    $outDir = Split-Path -Parent $outPath
    if ($outDir) { New-Item -ItemType Directory -Path $outDir -Force | Out-Null }
    Set-Content -Path $outPath -Value $rendered -Encoding UTF8
    Write-Host "`nReport written to $outPath" -ForegroundColor Green
}

Write-Host "`n=========================================" -ForegroundColor Cyan
Write-Host "RTO / RPO summary" -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan
Write-Host ("  iterations: {0} passed / {1} failed" -f $report.iterations_passed, $report.iterations_failed)
Write-Host ("  RTO seconds: p50={0:N3} p95={1:N3} p99={2:N3} max={3:N3}" -f $report.rto_seconds_p50, $report.rto_seconds_p95, $report.rto_seconds_p99, $report.rto_seconds_max)
Write-Host ("  RPO worst loss count: {0}" -f $report.rpo_worst_loss_count)
Write-Host ("  verdict: {0}" -f $(if ($allPassed) {'PASS'} else {'FAIL'})) -ForegroundColor $(if ($allPassed) {'Green'} else {'Red'})
Write-Host "=========================================" -ForegroundColor Cyan

if ($allPassed) { exit 0 } else { exit 1 }
