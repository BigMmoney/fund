param(
    [switch]$SkipSoakSeed,
    [switch]$IncludeStress,
    [int]$SoakDurationSeconds = 5,
    [int]$SoakConcurrency = 2,
    [int]$ReplayPort = 3041
)

$ErrorActionPreference = 'Stop'

function Wait-Healthy {
    param(
        [string]$BaseUrl,
        [int]$TimeoutSeconds = 60
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    do {
        try {
            $resp = Invoke-RestMethod -Uri "$BaseUrl/health" -Method Get -TimeoutSec 3
            if ($resp.status -eq 'ok' -or $resp.status -eq 'healthy') {
                return $resp
            }
        } catch {
        }
        Start-Sleep -Milliseconds 300
    } while ((Get-Date) -lt $deadline)

    throw "server did not become healthy within ${TimeoutSeconds}s: $BaseUrl"
}

function Start-ReplayApi {
    param(
        [string]$RepoRoot,
        [string]$DataRoot,
        [int]$Port,
        [string]$Secret
    )

    $envMap = @{
        'INTERNAL_AUTH_SHARED_SECRET'      = $Secret
        'API_BIND_HOST'                    = '127.0.0.1'
        'API_BIND_PORT'                    = "$Port"
        'RUST_LOG'                         = 'warn'
        'LEDGER_WAL_PATH'                  = (Join-Path $DataRoot 'ledger.wal.jsonl')
        'SEQUENCER_WAL_PATH'               = (Join-Path $DataRoot 'sequencer.wal.jsonl')
        'MATCHING_SNAPSHOT_WAL_PATH'       = (Join-Path $DataRoot 'matching.snapshot.jsonl')
        'TRADE_JOURNAL_WAL_PATH'           = (Join-Path $DataRoot 'trade_journal.wal.jsonl')
        'TRADE_SETTLEMENT_WAL_PATH'        = (Join-Path $DataRoot 'trade_settlement.wal.jsonl')
        'INSTRUMENTS_REGISTRY_WAL_PATH'    = (Join-Path $DataRoot 'instruments.registry.jsonl')
        'FUNDING_RATES_WAL_PATH'           = (Join-Path $DataRoot 'funding_rates.jsonl')
        'RISK_AUTOMATION_AUDIT_WAL_PATH'   = (Join-Path $DataRoot 'risk_automation.audit.jsonl')
        'LIQUIDATION_QUEUE_WAL_PATH'       = (Join-Path $DataRoot 'liquidation.queue.jsonl')
        'LIQUIDATION_AUCTION_WAL_PATH'     = (Join-Path $DataRoot 'liquidation.auction.jsonl')
        'ADL_GOVERNANCE_WAL_PATH'          = (Join-Path $DataRoot 'adl.governance.jsonl')
        'LIQUIDATION_POLICY_WAL_PATH'      = (Join-Path $DataRoot 'liquidation.policy.jsonl')
        'INDEX_PRICE_WAL_PATH'             = (Join-Path $DataRoot 'index.price.jsonl')
        'INDEX_SOURCE_POLICY_WAL_PATH'     = (Join-Path $DataRoot 'index.source.policy.jsonl')
        'POSITION_COST_STATE_WAL_PATH'     = (Join-Path $DataRoot 'position.cost.state.jsonl')
        'POSITION_COST_EVENT_WAL_PATH'     = (Join-Path $DataRoot 'position.cost.events.jsonl')
        'GOVERNANCE_ACTION_WAL_PATH'       = (Join-Path $DataRoot 'governance.actions.jsonl')
        'WITHDRAWALS_WAL_PATH'             = (Join-Path $DataRoot 'withdrawals.wal.jsonl')
        'FEE_TIERS_WAL_PATH'               = (Join-Path $DataRoot 'fee_tiers.jsonl')
        'TRANSFERS_WAL_PATH'               = (Join-Path $DataRoot 'transfers.jsonl')
        'STOP_ORDERS_WAL_PATH'             = (Join-Path $DataRoot 'stop_orders.jsonl')
        'ADDRESS_WHITELIST_WAL_PATH'       = (Join-Path $DataRoot 'address_whitelist.jsonl')
        'CARGO_TARGET_DIR'                 = (Join-Path $RepoRoot 'target')
    }

    $binary = Join-Path $RepoRoot 'target\release\api.exe'
    if (-not (Test-Path $binary)) {
        throw "API binary not found: $binary. Build with cargo build --release -p api first."
    }

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $binary
    $psi.WorkingDirectory = $RepoRoot
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true

    foreach ($entry in $envMap.GetEnumerator()) {
        $psi.Environment[$entry.Key] = $entry.Value
    }

    $proc = New-Object System.Diagnostics.Process
    $proc.StartInfo = $psi
    [void]$proc.Start()
    return $proc
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$rustRoot = Join-Path $repoRoot 'rust-exchange'

Write-Host '[recovery-check] repository:' $repoRoot -ForegroundColor Cyan

$results = [ordered]@{}

Push-Location $rustRoot
try {
    Write-Host '[recovery-check] 1/5 persistence best-effort recovery test' -ForegroundColor Cyan
    cargo test -q -p persistence best_effort_recovery_skips_corrupt_entries
    $results['persistence_best_effort'] = 'pass'

    Write-Host '[recovery-check] 2/5 ledger recovery test' -ForegroundColor Cyan
    cargo test -q -p ledger recover_from_wal_rebuilds_balances_and_seen_ops
    $results['ledger_recovery'] = 'pass'

    Write-Host '[recovery-check] 3/5 sequencer recovery test' -ForegroundColor Cyan
    cargo test -q -p sequencer recover_from_wal_restores_latest_metadata_and_next_seq
    $results['sequencer_recovery'] = 'pass'

    Write-Host '[recovery-check] 4/5 matching crash recovery drill' -ForegroundColor Cyan
    $crashOutput = cargo run --release --example crash_recovery_drill -p matching | Out-String
    $results['crash_recovery_drill'] = $crashOutput.Trim()

    if ($IncludeStress) {
        Write-Host '[recovery-check] stress 1/4 backpressure / queue saturation' -ForegroundColor Cyan
        cargo test -q -p api stress_s1_queue_saturation
        $results['stress_queue_saturation'] = 'pass'

        Write-Host '[recovery-check] stress 2/4 wal storm' -ForegroundColor Cyan
        cargo test -q -p api stress_s3_wal_storm
        $results['stress_wal_storm'] = 'pass'

        Write-Host '[recovery-check] stress 3/4 backpressure ramp' -ForegroundColor Cyan
        cargo test -q -p api stress_s7_backpressure_ramp
        $results['stress_backpressure_ramp'] = 'pass'

        Write-Host '[recovery-check] stress 4/4 snapshot recovery' -ForegroundColor Cyan
        cargo test -q -p api stress_s8_snapshot_recovery
        $results['stress_snapshot_recovery'] = 'pass'
    }

    if (-not $SkipSoakSeed) {
        Write-Host '[recovery-check] 5/5 generating real WAL data with api_e2e_soak.py' -ForegroundColor Cyan
        try {
            $soakOutput = python .\scripts\api_e2e_soak.py --duration-seconds $SoakDurationSeconds --concurrency $SoakConcurrency --port 3031 --server-profile release --keep-artifacts
            $soakJson = $soakOutput | ConvertFrom-Json
            $tempRoot = $soakJson.artifacts.temp_root
            $dataRoot = Join-Path $tempRoot 'data'
            $results['soak_seed_temp_root'] = $tempRoot
            $results['seed_summary'] = $soakJson.operations.overall

            Write-Host '[recovery-check] measuring replay startup time from preserved WALs' -ForegroundColor Cyan
            $apiProc = $null
            try {
                $sw = [System.Diagnostics.Stopwatch]::StartNew()
                $apiProc = Start-ReplayApi -RepoRoot $rustRoot -DataRoot $dataRoot -Port $ReplayPort -Secret 'soak-secret'
                $health = Wait-Healthy -BaseUrl "http://127.0.0.1:$ReplayPort" -TimeoutSeconds 90
                $sw.Stop()
                $partitionHealth = Invoke-RestMethod -Uri "http://127.0.0.1:$ReplayPort/health/partitions" -Method Get -TimeoutSec 5
                $metrics = Invoke-RestMethod -Uri "http://127.0.0.1:$ReplayPort/metrics" -Method Get -TimeoutSec 5

                $results['replay_startup'] = [ordered]@{
                    elapsed_ms       = $sw.ElapsedMilliseconds
                    health           = $health
                    partition_health = $partitionHealth
                    metrics          = $metrics
                }
            }
            finally {
                if ($null -ne $apiProc -and -not $apiProc.HasExited) {
                    $apiProc.Kill()
                    $apiProc.WaitForExit()
                }
            }
        }
        catch {
            $results['replay_startup_error'] = $_.Exception.Message
        }
    }
}
finally {
    Pop-Location
}

$json = $results | ConvertTo-Json -Depth 8
Write-Host '[recovery-check] summary:' -ForegroundColor Green
Write-Output $json
