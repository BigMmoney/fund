<#
.SYNOPSIS
    One-shot helper: open the RC 0.1 + Order Flow Monitor + RBAC PR and
    poll CI until the run completes.

.DESCRIPTION
    Prerequisite: `gh auth login` (one-time interactive login) OR set
    GH_TOKEN to a personal access token with `repo` + `workflow` scope.

    Phases:
      1. Confirm gh is authenticated.
      2. Confirm branch is in sync with origin.
      3. `gh pr create` (or detect existing PR for the branch and reuse).
      4. Poll the PR's checks every 15 s until all complete.
      5. Print final pass/fail summary and the PR URL.

    Exit code is the CI verdict: 0 if all checks pass, non-zero if any
    fail. The script does NOT auto-merge — that requires a human.

.PARAMETER GhPath
    Override the path to gh.exe. Default: C:\Program Files\GitHub CLI\gh.exe.

.PARAMETER PollInterval
    Seconds between CI poll cycles. Default 15.

.PARAMETER MaxWaitMinutes
    Hard ceiling on total wait time. Default 30.
#>

param(
    [string]$GhPath = "C:\Program Files\GitHub CLI\gh.exe",
    [int]$PollInterval = 15,
    [int]$MaxWaitMinutes = 30
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $GhPath)) {
    Write-Host "gh not found at $GhPath. Install GitHub CLI from https://cli.github.com or pass -GhPath." -ForegroundColor Red
    exit 2
}
function Gh { & $GhPath @args }

Write-Host "=== 1. Auth check ===" -ForegroundColor Cyan
$authStatus = & $GhPath auth status 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Host "gh is not authenticated. Run one of:" -ForegroundColor Red
    Write-Host "  & '$GhPath' auth login" -ForegroundColor Yellow
    Write-Host "  `$env:GH_TOKEN = '<personal access token with repo+workflow scope>'" -ForegroundColor Yellow
    exit 2
}
Write-Host ($authStatus -join "`n")
Write-Host "ok auth ready" -ForegroundColor Green

Write-Host ""
Write-Host "=== 2. Branch sync check ===" -ForegroundColor Cyan
$branch = (git rev-parse --abbrev-ref HEAD).Trim()
if ($branch -ne "p0-recovery-20260430") {
    Write-Host "expected branch p0-recovery-20260430, got $branch" -ForegroundColor Red
    exit 2
}
& git fetch origin p0-recovery-20260430 main | Out-Null
$ahead = (git rev-list --count origin/p0-recovery-20260430..HEAD).Trim()
$behind = (git rev-list --count HEAD..origin/p0-recovery-20260430).Trim()
if ($ahead -ne "0") {
    Write-Host "HEAD is ahead of origin by $ahead commits — push first." -ForegroundColor Red
    exit 2
}
if ($behind -ne "0") {
    Write-Host "HEAD is behind origin by $behind commits — pull first." -ForegroundColor Red
    exit 2
}
Write-Host "ok branch synced with origin/p0-recovery-20260430" -ForegroundColor Green

Write-Host ""
Write-Host "=== 3. PR create (or reuse existing) ===" -ForegroundColor Cyan
$existingPr = & $GhPath pr list --head p0-recovery-20260430 --base main --state open --json number,url 2>$null
$prInfo = $null
if ($existingPr -and $existingPr.Trim() -ne "[]") {
    $prInfo = $existingPr | ConvertFrom-Json | Select-Object -First 1
    Write-Host "Reusing existing PR #$($prInfo.number) at $($prInfo.url)" -ForegroundColor Yellow
} else {
    $bodyFile = "D:/pre_trading/docs/releases/PR_BODY_p0-recovery-20260430.md"
    if (-not (Test-Path $bodyFile)) {
        Write-Host "PR body file missing: $bodyFile" -ForegroundColor Red
        exit 2
    }
    Write-Host "Creating PR..." -ForegroundColor Gray
    $createOut = & $GhPath pr create `
        --title "Backend Reliability RC 0.1 + Order Flow Monitor + RBAC design" `
        --body-file $bodyFile `
        --base main `
        --head p0-recovery-20260430 `
        --repo BigMmoney/fund 2>&1
    if ($LASTEXITCODE -ne 0) {
        Write-Host "gh pr create failed:" -ForegroundColor Red
        Write-Host ($createOut -join "`n")
        exit 2
    }
    $url = ($createOut | Where-Object { $_ -match "github.com" } | Select-Object -First 1).Trim()
    $number = if ($url -match "/pull/(\d+)") { $Matches[1] } else { $null }
    $prInfo = [pscustomobject]@{ number = $number; url = $url }
    Write-Host "ok PR created: $url" -ForegroundColor Green
}

Write-Host ""
Write-Host "=== 4. Watch CI ===" -ForegroundColor Cyan
Write-Host "Polling every ${PollInterval}s, max ${MaxWaitMinutes} min total" -ForegroundColor Gray
$deadline = (Get-Date).AddMinutes($MaxWaitMinutes)
$lastSummary = ""
while ((Get-Date) -lt $deadline) {
    $statusJson = & $GhPath pr checks $prInfo.number --repo BigMmoney/fund --json name,state,conclusion,bucket 2>$null
    if ($LASTEXITCODE -eq 0 -and $statusJson) {
        $checks = $statusJson | ConvertFrom-Json
        $total = $checks.Count
        $pending = ($checks | Where-Object { $_.bucket -eq "pending" }).Count
        $passing = ($checks | Where-Object { $_.bucket -eq "pass" }).Count
        $failing = ($checks | Where-Object { $_.bucket -eq "fail" }).Count
        $skipping = ($checks | Where-Object { $_.bucket -eq "skipping" }).Count
        $summary = "$total checks: pass=$passing pending=$pending fail=$failing skip=$skipping"
        if ($summary -ne $lastSummary) {
            Write-Host "$(Get-Date -Format 'HH:mm:ss') $summary"
            $lastSummary = $summary
        }
        if ($pending -eq 0) {
            Write-Host ""
            Write-Host "=== 5. CI complete ===" -ForegroundColor Cyan
            $checks | Sort-Object name | Format-Table name, state, conclusion -AutoSize
            if ($failing -eq 0) {
                Write-Host "All $total checks PASS" -ForegroundColor Green
                Write-Host "PR URL: $($prInfo.url)" -ForegroundColor Green
                Write-Host "Next: human review + merge via GitHub UI or 'gh pr merge $($prInfo.number) --squash'" -ForegroundColor Yellow
                exit 0
            } else {
                Write-Host "$failing check(s) FAILED" -ForegroundColor Red
                Write-Host "PR URL: $($prInfo.url)" -ForegroundColor Red
                Write-Host "Next: inspect failures via 'gh pr checks $($prInfo.number) --repo BigMmoney/fund' and 'gh run view <run-id> --log-failed --repo BigMmoney/fund'" -ForegroundColor Yellow
                exit 1
            }
        }
    } else {
        Write-Host "$(Get-Date -Format 'HH:mm:ss') no checks yet (CI may still be queueing)" -ForegroundColor Gray
    }
    Start-Sleep -Seconds $PollInterval
}

Write-Host ""
Write-Host "Timed out after $MaxWaitMinutes min waiting for CI to complete." -ForegroundColor Yellow
Write-Host "PR URL: $($prInfo.url)" -ForegroundColor Yellow
Write-Host "Re-run this script to keep watching." -ForegroundColor Yellow
exit 3
