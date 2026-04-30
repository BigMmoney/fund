param(
    [int]$MixedSamples = 40000,
    [string]$MixedLevels = '8,16',
    [int]$MixedPartitions = 4,
    [int]$MixedMarkets = 8,
    [int]$HotMarkets = 2,
    [int]$HotSharePct = 70,
    [int]$RuntimeWorkers = 8,
    [int]$SoakSeconds = 60,
    [int]$SoakConcurrency = 8,
    [int]$SoakSamplesPerRound = 12000,
    [int]$ApiSoakSeconds = 10,
    [int]$ApiSoakConcurrency = 2,
    [int]$ReplayPortBase = 3041
)

$ErrorActionPreference = 'Stop'

function Invoke-CargoCapture {
    param(
        [string]$Workdir,
        [hashtable]$Env = @{},
        [string[]]$CommandArgs
    )

    Push-Location $Workdir
    try {
        foreach ($key in $Env.Keys) {
            Set-Item -Path "Env:$key" -Value $Env[$key]
        }
        $output = & cargo @CommandArgs 2>&1
        if ($LASTEXITCODE -ne 0) {
            throw "cargo $($CommandArgs -join ' ') failed: $($output | Out-String)"
        }
        return @($output | ForEach-Object { "$_" })
    }
    finally {
        foreach ($key in $Env.Keys) {
            Remove-Item "Env:$key" -ErrorAction SilentlyContinue
        }
        Pop-Location
    }
}

function Invoke-PythonCapture {
    param(
        [string]$Workdir,
        [string[]]$CommandArgs
    )

    Push-Location $Workdir
    try {
        $output = & python.exe $CommandArgs 2>&1
        if ($LASTEXITCODE -ne 0) {
            throw "python $($CommandArgs -join ' ') failed: $($output | Out-String)"
        }
        return @($output | ForEach-Object { "$_" })
    }
    finally {
        Pop-Location
    }
}

function Parse-KeyValueLine {
    param([string]$Line)

    $map = [ordered]@{}
    foreach ($match in [regex]::Matches($Line, '([A-Za-z0-9_]+)=("[^"]*"|\S+)')) {
        $key = $match.Groups[1].Value
        $value = $match.Groups[2].Value.Trim('"')
        $map[$key] = $value
    }
    return $map
}

function Parse-MixedBenchmark {
    param([string[]]$Lines)

    $parsed = [ordered]@{
        topology = @()
        mixed = @()
        submit_path = @()
        queue = @()
    }
    foreach ($line in $Lines) {
        if ($line -notmatch '^mode=') {
            continue
        }
        $kv = Parse-KeyValueLine -Line $line
        switch ($kv['mode']) {
            'topology' { $parsed.topology += [pscustomobject]$kv }
            'mixed' { $parsed.mixed += [pscustomobject]$kv }
            'submit_path' { $parsed.submit_path += [pscustomobject]$kv }
            'queue' { $parsed.queue += [pscustomobject]$kv }
        }
    }
    return $parsed
}

function Resolve-ProcessCounterInstance {
    param([int]$ProcessId)

    $samples = (Get-Counter '\Process(*)\ID Process').CounterSamples
    $match = $samples | Where-Object { [int]$_.CookedValue -eq $ProcessId } | Select-Object -First 1
    if ($null -eq $match) {
        return $null
    }
    if ($match.Path -match '\\Process\(([^)]+)\)\\ID Process$') {
        return $Matches[1]
    }
    return $null
}

function Run-ProcessWithSampling {
    param(
        [string]$FilePath,
        [string]$Workdir,
        [hashtable]$Env = @{},
        [int]$SampleIntervalMs = 1000
    )

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $FilePath
    $psi.WorkingDirectory = $Workdir
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    foreach ($entry in $Env.GetEnumerator()) {
        $psi.Environment[$entry.Key] = $entry.Value
    }

    $proc = New-Object System.Diagnostics.Process
    $proc.StartInfo = $psi
    [void]$proc.Start()

    $samples = New-Object System.Collections.Generic.List[object]
    $counterInstance = $null
    while (-not $proc.HasExited) {
        if (-not $counterInstance) {
            $counterInstance = Resolve-ProcessCounterInstance -ProcessId $proc.Id
        }
        try {
            $ps = Get-Process -Id $proc.Id -ErrorAction Stop
            $sample = [ordered]@{
                ts_utc = (Get-Date).ToUniversalTime().ToString('o')
                pid = $proc.Id
                cpu_seconds = $ps.CPU
                working_set_bytes = $ps.WorkingSet64
                private_bytes = $ps.PrivateMemorySize64
                handles = $ps.HandleCount
            }
            if ($counterInstance) {
                $counters = Get-Counter @(
                    "\Process($counterInstance)\% Processor Time",
                    "\Process($counterInstance)\IO Read Bytes/sec",
                    "\Process($counterInstance)\IO Write Bytes/sec",
                    "\Process($counterInstance)\Working Set - Private"
                )
                foreach ($counter in $counters.CounterSamples) {
                    switch -Regex ($counter.Path) {
                        '% Processor Time$' { $sample['cpu_pct'] = [math]::Round($counter.CookedValue, 2) }
                        'IO Read Bytes/sec$' { $sample['io_read_bytes_per_sec'] = [math]::Round($counter.CookedValue, 2) }
                        'IO Write Bytes/sec$' { $sample['io_write_bytes_per_sec'] = [math]::Round($counter.CookedValue, 2) }
                        'Working Set - Private$' { $sample['private_working_set_bytes'] = [int64]$counter.CookedValue }
                    }
                }
            }
            $samples.Add([pscustomobject]$sample)
        }
        catch {
        }
        Start-Sleep -Milliseconds $SampleIntervalMs
    }

    $outputText = [string]$proc.StandardOutput.ReadToEnd()
    $errorText = [string]$proc.StandardError.ReadToEnd()
    $proc.WaitForExit()
    if ($proc.ExitCode -ne 0) {
        throw "process failed: $FilePath`nSTDOUT:`n$outputText`nSTDERR:`n$errorText"
    }
    $outputLines = @()
    if ($outputText) {
        $outputLines = @([regex]::Split("$outputText", "\r?\n") | Where-Object { $_ -ne '' })
    }
    $errorLines = @()
    if ($errorText) {
        $errorLines = @([regex]::Split("$errorText", "\r?\n") | Where-Object { $_ -ne '' })
    }
    return @{
        stdout = $outputLines
        stderr = $errorLines
        samples = @($samples.ToArray())
    }
}

function Summarize-ProcessSamples {
    param([object[]]$Samples)

    if (-not $Samples -or $Samples.Count -eq 0) {
        return [pscustomobject]@{
            sample_count = 0
        }
    }
    [pscustomobject]@{
        sample_count = $Samples.Count
        cpu_pct_peak = ($Samples | Measure-Object -Property cpu_pct -Maximum).Maximum
        cpu_pct_avg = [math]::Round((($Samples | Measure-Object -Property cpu_pct -Average).Average), 2)
        working_set_peak_bytes = ($Samples | Measure-Object -Property working_set_bytes -Maximum).Maximum
        private_bytes_peak = ($Samples | Measure-Object -Property private_bytes -Maximum).Maximum
        io_read_peak_bytes_per_sec = ($Samples | Measure-Object -Property io_read_bytes_per_sec -Maximum).Maximum
        io_write_peak_bytes_per_sec = ($Samples | Measure-Object -Property io_write_bytes_per_sec -Maximum).Maximum
    }
}

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
        }
        catch {
        }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)

    throw "server did not become healthy within ${TimeoutSeconds}s: $BaseUrl"
}

function Start-ReplayApi {
    param(
        [string]$BinaryPath,
        [string]$RepoRoot,
        [string]$DataRoot,
        [int]$Port,
        [string]$Secret,
        [string]$RecoveryMode
    )

    $envMap = @{
        'INTERNAL_AUTH_SHARED_SECRET'    = $Secret
        'API_BIND_HOST'                  = '127.0.0.1'
        'API_BIND_PORT'                  = "$Port"
        'RUST_LOG'                       = 'warn'
        'WAL_RECOVERY_MODE'              = $RecoveryMode
        'LEDGER_WAL_PATH'                = (Join-Path $DataRoot 'ledger.wal.jsonl')
        'SEQUENCER_WAL_PATH'             = (Join-Path $DataRoot 'sequencer.wal.jsonl')
        'MATCHING_SNAPSHOT_WAL_PATH'     = (Join-Path $DataRoot 'matching.snapshot.jsonl')
        'TRADE_JOURNAL_WAL_PATH'         = (Join-Path $DataRoot 'trade_journal.wal.jsonl')
        'TRADE_SETTLEMENT_WAL_PATH'      = (Join-Path $DataRoot 'trade_settlement.wal.jsonl')
        'INSTRUMENTS_REGISTRY_WAL_PATH'  = (Join-Path $DataRoot 'instruments.registry.jsonl')
        'FUNDING_RATES_WAL_PATH'         = (Join-Path $DataRoot 'funding_rates.jsonl')
        'RISK_AUTOMATION_AUDIT_WAL_PATH' = (Join-Path $DataRoot 'risk_automation.audit.jsonl')
        'LIQUIDATION_QUEUE_WAL_PATH'     = (Join-Path $DataRoot 'liquidation.queue.jsonl')
        'LIQUIDATION_AUCTION_WAL_PATH'   = (Join-Path $DataRoot 'liquidation.auction.jsonl')
        'ADL_GOVERNANCE_WAL_PATH'        = (Join-Path $DataRoot 'adl.governance.jsonl')
        'LIQUIDATION_POLICY_WAL_PATH'    = (Join-Path $DataRoot 'liquidation.policy.jsonl')
        'INDEX_PRICE_WAL_PATH'           = (Join-Path $DataRoot 'index.price.jsonl')
        'INDEX_SOURCE_POLICY_WAL_PATH'   = (Join-Path $DataRoot 'index.source.policy.jsonl')
        'POSITION_COST_STATE_WAL_PATH'   = (Join-Path $DataRoot 'position.cost.state.jsonl')
        'POSITION_COST_EVENT_WAL_PATH'   = (Join-Path $DataRoot 'position.cost.events.jsonl')
        'GOVERNANCE_ACTION_WAL_PATH'     = (Join-Path $DataRoot 'governance.actions.jsonl')
        'WITHDRAWALS_WAL_PATH'           = (Join-Path $DataRoot 'withdrawals.wal.jsonl')
        'FEE_TIERS_WAL_PATH'             = (Join-Path $DataRoot 'fee_tiers.jsonl')
        'TRANSFERS_WAL_PATH'             = (Join-Path $DataRoot 'transfers.jsonl')
        'STOP_ORDERS_WAL_PATH'           = (Join-Path $DataRoot 'stop_orders.jsonl')
        'ADDRESS_WHITELIST_WAL_PATH'     = (Join-Path $DataRoot 'address_whitelist.jsonl')
        'CARGO_TARGET_DIR'               = (Join-Path $RepoRoot 'target')
    }

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $BinaryPath
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

function Measure-ReplayStartup {
    param(
        [string]$BinaryPath,
        [string]$RepoRoot,
        [string]$DataRoot,
        [int]$Port,
        [string]$RecoveryMode
    )

    $proc = $null
    try {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $proc = Start-ReplayApi -BinaryPath $BinaryPath -RepoRoot $RepoRoot -DataRoot $DataRoot -Port $Port -Secret 'soak-secret' -RecoveryMode $RecoveryMode
        $health = Wait-Healthy -BaseUrl "http://127.0.0.1:$Port" -TimeoutSeconds 90
        $sw.Stop()
        $partitions = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/health/partitions" -Method Get -TimeoutSec 5
        $metrics = Invoke-RestMethod -Uri "http://127.0.0.1:$Port/metrics" -Method Get -TimeoutSec 5
        return [pscustomobject]@{
            status = 'ok'
            recovery_mode = $RecoveryMode
            elapsed_ms = $sw.ElapsedMilliseconds
            health = $health
            partition_health = $partitions
            metrics = $metrics
        }
    }
    catch {
        return [pscustomobject]@{
            status = 'error'
            recovery_mode = $RecoveryMode
            error = $_.Exception.Message
        }
    }
    finally {
        if ($null -ne $proc -and -not $proc.HasExited) {
            $proc.Kill()
            $proc.WaitForExit()
        }
    }
}

function New-CorruptedDataRoot {
    param(
        [string]$SourceRoot,
        [string]$WalFileName
    )

    $targetRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("api-corrupt-" + [guid]::NewGuid().ToString('N'))
    Copy-Item -LiteralPath $SourceRoot -Destination $targetRoot -Recurse
    $targetWal = Join-Path $targetRoot $WalFileName
    Add-Content -LiteralPath $targetWal -Value '00000000	{"corrupt_tail":'
    return $targetRoot
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$rustRoot = Join-Path $repoRoot 'rust-exchange'
$mixedExe = Join-Path $rustRoot 'target\release\examples\mixed_workload_bench.exe'
$soakExe = Join-Path $rustRoot 'target\release\examples\soak_bench.exe'
$apiDebugExe = Join-Path $rustRoot 'target\debug\api.exe'

$summary = [ordered]@{}

Write-Host '[resilience-bench] building release examples' -ForegroundColor Cyan
Push-Location $rustRoot
try {
    cargo build --release -p matching --example mixed_workload_bench --example soak_bench | Out-Null
}
finally {
    Pop-Location
}

Write-Host '[resilience-bench] baseline spread topology' -ForegroundColor Cyan
$spreadEnv = @{
    'MIXED_BENCH_SAMPLES' = "$MixedSamples"
    'MIXED_BENCH_LEVELS' = $MixedLevels
    'MIXED_BENCH_PARTITIONS' = "$MixedPartitions"
    'MIXED_BENCH_MARKETS' = "$MixedMarkets"
    'MIXED_BENCH_MARKET_PLACEMENT' = 'spread'
    'MIXED_BENCH_HOT_MARKETS' = "$HotMarkets"
    'MIXED_BENCH_HOT_SHARE_PCT' = "$HotSharePct"
    'MIXED_BENCH_RUNTIME_WORKERS' = "$RuntimeWorkers"
}
$summary['baseline_spread'] = Parse-MixedBenchmark (Invoke-CargoCapture -Workdir $rustRoot -Env $spreadEnv -CommandArgs @('run', '--release', '--example', 'mixed_workload_bench', '-p', 'matching'))

Write-Host '[resilience-bench] hotspot packed topology' -ForegroundColor Cyan
$packedEnv = $spreadEnv.Clone()
$packedEnv['MIXED_BENCH_MARKET_PLACEMENT'] = 'packed'
$summary['hotspot_packed'] = Parse-MixedBenchmark (Invoke-CargoCapture -Workdir $rustRoot -Env $packedEnv -CommandArgs @('run', '--release', '--example', 'mixed_workload_bench', '-p', 'matching'))

Write-Host '[resilience-bench] slow write degradation' -ForegroundColor Cyan
$slowEnv = @{
    'MIXED_BENCH_SAMPLES' = '5000'
    'MIXED_BENCH_LEVELS' = '4'
    'MIXED_BENCH_PARTITIONS' = '2'
    'MIXED_BENCH_MARKETS' = '4'
    'MIXED_BENCH_MARKET_PLACEMENT' = 'spread'
    'MIXED_BENCH_RUNTIME_WORKERS' = '4'
    'MIXED_BENCH_PERSISTENCE_MODE' = 'file'
    'MIXED_BENCH_WAL_DELAY_US' = '300'
    'MIXED_BENCH_WAL_JITTER_US' = '0'
}
$summary['slow_write_file_wal'] = Parse-MixedBenchmark (Invoke-CargoCapture -Workdir $rustRoot -Env $slowEnv -CommandArgs @('run', '--release', '--example', 'mixed_workload_bench', '-p', 'matching'))

Write-Host '[resilience-bench] fsync jitter degradation' -ForegroundColor Cyan
$jitterEnv = $slowEnv.Clone()
$jitterEnv['MIXED_BENCH_WAL_DELAY_US'] = '100'
$jitterEnv['MIXED_BENCH_WAL_JITTER_US'] = '900'
$summary['fsync_jitter_file_wal'] = Parse-MixedBenchmark (Invoke-CargoCapture -Workdir $rustRoot -Env $jitterEnv -CommandArgs @('run', '--release', '--example', 'mixed_workload_bench', '-p', 'matching'))

Write-Host '[resilience-bench] release soak with resource sampling' -ForegroundColor Cyan
$soakRun = Run-ProcessWithSampling -FilePath $soakExe -Workdir $rustRoot -Env @{
    'SOAK_BENCH_SECONDS' = "$SoakSeconds"
    'SOAK_BENCH_CONCURRENCY' = "$SoakConcurrency"
    'SOAK_BENCH_SAMPLES_PER_ROUND' = "$SoakSamplesPerRound"
}
$summary['soak'] = [ordered]@{
    lines = $soakRun.stdout
    resource_summary = Summarize-ProcessSamples $soakRun.samples
    resource_samples = $soakRun.samples
}

Write-Host '[resilience-bench] stress suite' -ForegroundColor Cyan
$summary['stress_suite'] = Invoke-CargoCapture -Workdir $rustRoot -CommandArgs @('test', '-p', 'api', 'stress_full_suite_runs', '--', '--nocapture')

Write-Host '[resilience-bench] debug api soak seed for replay benchmark' -ForegroundColor Cyan
$env:API_E2E_SOAK_SKIP_BUILD = '1'
try {
    $apiSoakOutput = Invoke-PythonCapture -Workdir $repoRoot -CommandArgs @(
        'rust-exchange\scripts\api_e2e_soak.py',
        '--duration-seconds', "$ApiSoakSeconds",
        '--concurrency', "$ApiSoakConcurrency",
        '--port', '3031',
        '--server-profile', 'debug',
        '--keep-artifacts'
    )
}
finally {
    Remove-Item Env:API_E2E_SOAK_SKIP_BUILD -ErrorAction SilentlyContinue
}
$apiSoakJson = ($apiSoakOutput -join "`n") | ConvertFrom-Json
$dataRoot = Join-Path $apiSoakJson.artifacts.temp_root 'data'
$summary['api_seed'] = $apiSoakJson

Write-Host '[resilience-bench] clean startup replay benchmark' -ForegroundColor Cyan
$summary['replay_clean_strict'] = Measure-ReplayStartup -BinaryPath $apiDebugExe -RepoRoot $rustRoot -DataRoot $dataRoot -Port $ReplayPortBase -RecoveryMode 'strict'

Write-Host '[resilience-bench] corrupt trade journal tail and compare strict vs best_effort' -ForegroundColor Cyan
$corruptRoot = New-CorruptedDataRoot -SourceRoot $dataRoot -WalFileName 'trade_journal.wal.jsonl'
$summary['replay_corrupt_trade_journal_strict'] = Measure-ReplayStartup -BinaryPath $apiDebugExe -RepoRoot $rustRoot -DataRoot $corruptRoot -Port ($ReplayPortBase + 1) -RecoveryMode 'strict'
$summary['replay_corrupt_trade_journal_best_effort'] = Measure-ReplayStartup -BinaryPath $apiDebugExe -RepoRoot $rustRoot -DataRoot $corruptRoot -Port ($ReplayPortBase + 2) -RecoveryMode 'best_effort'

$json = $summary | ConvertTo-Json -Depth 8
Write-Host '[resilience-bench] summary:' -ForegroundColor Green
Write-Output $json
