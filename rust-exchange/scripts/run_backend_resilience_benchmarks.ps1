param(
    [string]$BaseUrl = "",
    [int]$Port = 3131,
    [string]$Secret = "dev-secret-change-me-to-32-chars-min!",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [string]$CargoTarget = "",
    [switch]$BuildBinary,
    [switch]$Smoke,
    [switch]$Full,
    [switch]$ScaleLadder,
    [switch]$Ci,
    [ValidateSet("auto", "powershell", "go")]
    [string]$HttpBenchClient = "auto",
    [switch]$CompareGoClientModes,
    [bool]$GoDisableKeepAlives = $false,
    [string]$BaselineReportPath = "",
    [switch]$FailOnRegression,
    [int]$LatencySamples = 0,
    [int]$Concurrency = 0,
    [int]$WarmDepthOrders = 0,
    [int]$FaultCycles = 0,
    [int]$SoakSeconds = 0,
    [int]$SoakBurstSize = 0
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

. "$PSScriptRoot/backend_resilience_lib.ps1"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$workspaceRoot = (Resolve-Path (Join-Path $repoRoot "..")).Path
$runId = [DateTimeOffset]::UtcNow.ToString("yyyyMMdd-HHmmss")
$artifactsRoot = Join-Path $repoRoot "artifacts/backend-resilience"
$layout = New-BackendRunLayout -Root $artifactsRoot -RunId $runId
$resolvedBaselineReportPath = if (-not [string]::IsNullOrWhiteSpace($BaselineReportPath)) {
    $BaselineReportPath
} else {
    Find-LatestBackendBaselineReport -ArtifactsRoot $artifactsRoot -CurrentRunId $runId
}
$resolvedBaseUrl = if ([string]::IsNullOrWhiteSpace($BaseUrl)) { "http://127.0.0.1:$Port" } else { $BaseUrl.TrimEnd("/") }
$resolvedPort = if ([string]::IsNullOrWhiteSpace($BaseUrl)) { $Port } else { ([Uri]$resolvedBaseUrl).Port }
$existingBinary = if ([string]::IsNullOrWhiteSpace($CargoTarget)) {
    Find-ExistingApiBinary -RepoRoot $repoRoot -Profile $Profile
} else {
    $null
}
$effectiveCargoTarget = if (-not [string]::IsNullOrWhiteSpace($CargoTarget)) {
    $CargoTarget
} elseif ($existingBinary) {
    $existingBinary.cargo_target
} elseif ($env:OS -eq "Windows_NT") {
    "x86_64-pc-windows-msvc"
} else {
    ""
}
$binaryPath = if ($existingBinary) { $existingBinary.path } else { "" }
$effectiveMode = if ($Ci) {
    "ci"
} elseif ($Full) {
    "full"
} else {
    "smoke"
}
$effectiveHttpBenchClient = Resolve-HttpBenchClient -RequestedClient $HttpBenchClient -WorkspaceRoot $workspaceRoot

if ($effectiveMode -eq "ci") {
    if ($LatencySamples -le 0) { $LatencySamples = 1200 }
    if ($Concurrency -le 0) { $Concurrency = 12 }
    if ($WarmDepthOrders -le 0) { $WarmDepthOrders = 160 }
    if (-not $PSBoundParameters.ContainsKey("FaultCycles")) { $FaultCycles = 2 }
    if (-not $PSBoundParameters.ContainsKey("SoakSeconds")) { $SoakSeconds = 90 }
    if (-not $PSBoundParameters.ContainsKey("SoakBurstSize")) { $SoakBurstSize = 48 }
} elseif ($effectiveMode -eq "full") {
    if ($LatencySamples -le 0) { $LatencySamples = 3000 }
    if ($Concurrency -le 0) { $Concurrency = 24 }
    if ($WarmDepthOrders -le 0) { $WarmDepthOrders = 320 }
    if (-not $PSBoundParameters.ContainsKey("FaultCycles")) { $FaultCycles = 4 }
    if (-not $PSBoundParameters.ContainsKey("SoakSeconds")) { $SoakSeconds = 300 }
    if (-not $PSBoundParameters.ContainsKey("SoakBurstSize")) { $SoakBurstSize = 64 }
} else {
    if ($LatencySamples -le 0) { $LatencySamples = 80 }
    if ($Concurrency -le 0) { $Concurrency = 6 }
    if ($WarmDepthOrders -le 0) { $WarmDepthOrders = 16 }
    if (-not $PSBoundParameters.ContainsKey("FaultCycles")) { $FaultCycles = 1 }
    if (-not $PSBoundParameters.ContainsKey("SoakSeconds")) { $SoakSeconds = 30 }
    if (-not $PSBoundParameters.ContainsKey("SoakBurstSize")) { $SoakBurstSize = 12 }
}

Write-Host ""
Write-Host "====================================================" -ForegroundColor Cyan
Write-Host " Backend Resilience Benchmarks" -ForegroundColor Cyan
Write-Host "====================================================" -ForegroundColor Cyan
Write-Host "Run ID:       $runId" -ForegroundColor DarkGray
Write-Host "Mode:         $effectiveMode" -ForegroundColor DarkGray
Write-Host "Base URL:     $resolvedBaseUrl" -ForegroundColor DarkGray
Write-Host "Profile:      $Profile" -ForegroundColor DarkGray
Write-Host "Cargo target: $(if ([string]::IsNullOrWhiteSpace($effectiveCargoTarget)) { '<default>' } else { $effectiveCargoTarget })" -ForegroundColor DarkGray
Write-Host "HTTP client:  $effectiveHttpBenchClient" -ForegroundColor DarkGray
Write-Host "Binary plan:  $(if ([string]::IsNullOrWhiteSpace($binaryPath)) { 'build or resolved target lookup' } else { $binaryPath })" -ForegroundColor DarkGray
Write-Host "Baseline:     $(if ([string]::IsNullOrWhiteSpace($resolvedBaselineReportPath)) { '<none>' } else { $resolvedBaselineReportPath })" -ForegroundColor DarkGray
Write-Host "Latency:      $LatencySamples requests @ concurrency $Concurrency" -ForegroundColor DarkGray
Write-Host "Fault cycles: $FaultCycles" -ForegroundColor DarkGray
Write-Host "Soak:         $SoakSeconds sec" -ForegroundColor DarkGray

[void](Write-BackendExchangeConfig -Layout $layout -Port $resolvedPort)

$client = New-HttpClient -TimeoutSeconds 30
$process = $null
$startedAt = [DateTimeOffset]::UtcNow.ToString("o")

function Invoke-TimedPhase {
    param(
        [string]$Name,
        [scriptblock]$Action
    )

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    Write-Host ("[phase] {0}..." -f $Name) -ForegroundColor Cyan
    try {
        return & $Action
    } finally {
        $sw.Stop()
        Write-Host ("[phase] {0} done in {1:N2}s" -f $Name, $sw.Elapsed.TotalSeconds) -ForegroundColor DarkGray
    }
}

function New-MakerAndBuyerSets {
    param(
        [string[]]$Markets,
        [int]$UserCount = 8,
        [string]$Prefix = "bench"
    )

    $buyers = @()
    $sellers = @()
    for ($i = 0; $i -lt $UserCount; $i++) {
        $buyers += "$Prefix-buyer-$i"
        $sellers += "$Prefix-seller-$i"
    }

    return [pscustomobject]@{
        buyers  = $buyers
        sellers = $sellers
        markets = $Markets
    }
}

function Convert-BurstToSummary {
    param(
        [object[]]$Results,
        [object]$MetricsJson
    )

    $resultsArray = @($Results)
    $overall = New-HttpResultPathSummary -Results $resultsArray
    $successResults = @($resultsArray | Where-Object { $_.status_code -ge 200 -and $_.status_code -lt 300 })
    $errorResults = @($resultsArray | Where-Object { $_.status_code -lt 200 -or $_.status_code -ge 300 })

    $summary = ConvertTo-PlainOrdered -InputObject $overall
    $summary["client_impl"] = "powershell"
    $summary["client_mode"] = "powershell"
    $summary["client_mode_description"] = "PowerShell/.NET harness"
    $summary["success_rate"] = Get-SuccessRate -SuccessCount $summary["success_count"] -TotalRequests $summary["total_requests"]
    $summary["http_4xx_count"] = Get-Http4xxCount -StatusBreakdown $summary["status_breakdown"]
    $summary["http_429_count"] = Get-StatusCount -StatusBreakdown $summary["status_breakdown"] -StatusCode "429"
    $summary["success_path"] = New-HttpResultPathSummary -Results $successResults
    $summary["error_path"] = New-HttpResultPathSummary -Results $errorResults
    $summary["server_metrics"] = Get-ServerMetricsSummary -MetricsJson $MetricsJson
    return $summary
}

function Set-PrimaryHttpMetricFields {
    param(
        [object]$Summary
    )

    if (-not $Summary) {
        return $Summary
    }

    $hasDirectPath = $null -ne $Summary.Keys -and ($Summary.Keys -contains "direct_success_path")
    $hasRescuedPath = $null -ne $Summary.Keys -and ($Summary.Keys -contains "rescued_success_path")
    $hasFlowControlPath = $null -ne $Summary.Keys -and ($Summary.Keys -contains "flow_controlled_success_path")
    $directPath = if ($hasDirectPath) { $Summary["direct_success_path"] } else { $null }
    $rescuedPath = if ($hasRescuedPath) { $Summary["rescued_success_path"] } else { $null }
    $flowControlPath = if ($hasFlowControlPath) { $Summary["flow_controlled_success_path"] } else { $null }
    $directCount = if ($directPath) { [int]$directPath["total_requests"] } else { 0 }
    $rescuedCount = if ($rescuedPath) { [int]$rescuedPath["total_requests"] } else { 0 }
    $flowControlledCount = if ($flowControlPath) { [int]$flowControlPath["total_requests"] } else { 0 }
    $rateLimitedCount = if ($Summary.Keys -contains "http_429_count") { [int]$Summary["http_429_count"] } else { 0 }
    $capacityCleanTotal = [Math]::Max(0, ([int]$Summary["total_requests"] - $rateLimitedCount))
    $apiRateLimitedCount = 0
    if ($Summary.Keys -contains "error_categories" -and $Summary["error_categories"]) {
        foreach ($category in @($Summary["error_categories"])) {
            if ($category["category"] -eq "api rate limit") {
                $apiRateLimitedCount += [int]$category["count"]
            }
        }
    }
    $systemCoreTotal = [Math]::Max(0, ([int]$Summary["total_requests"] - $apiRateLimitedCount))
    $directLatency = if ($directPath) { $directPath["client_latency_us"] } else { [ordered]@{} }
    $rescuedLatency = if ($rescuedPath) { $rescuedPath["client_latency_us"] } else { [ordered]@{} }
    $flowControlledLatency = if ($flowControlPath) { $flowControlPath["client_latency_us"] } else { [ordered]@{} }

    $Summary["primary_metric_basis"] = "keepalive_on + direct success"
    $Summary["primary_metric"] = [ordered]@{
        client_mode       = $Summary["client_mode"]
        success_type      = "direct success"
        count             = $directCount
        clean_total       = $capacityCleanTotal
        excluded_429      = $rateLimitedCount
        success_rate_pct  = Get-SuccessRate -SuccessCount $directCount -TotalRequests $capacityCleanTotal
        client_latency_us = $directLatency
    }
    $Summary["system_core_metric_basis"] = "keepalive_on + direct success + api rate-limit guardrails excluded"
    $Summary["system_core_metric"] = [ordered]@{
        client_mode         = $Summary["client_mode"]
        success_type        = "direct success"
        count               = $directCount
        clean_total         = $systemCoreTotal
        excluded_api_limits = $apiRateLimitedCount
        success_rate_pct    = Get-SuccessRate -SuccessCount $directCount -TotalRequests $systemCoreTotal
        client_latency_us   = $directLatency
    }
    $Summary["single_ip_metric_basis"] = "keepalive_on + direct success + single-IP entrypoint constraints included"
    $Summary["single_ip_metric"] = [ordered]@{
        client_mode         = $Summary["client_mode"]
        success_type        = "direct success"
        count               = $directCount
        clean_total         = $capacityCleanTotal
        excluded_429        = $rateLimitedCount
        success_rate_pct    = Get-SuccessRate -SuccessCount $directCount -TotalRequests $capacityCleanTotal
        client_latency_us   = $directLatency
    }
    $Summary["rescued_success_metric"] = [ordered]@{
        success_type      = "rescued success"
        count             = $rescuedCount
        client_latency_us = $rescuedLatency
    }
    $Summary["flow_controlled_success_metric"] = [ordered]@{
        success_type      = "flow-controlled success"
        count             = $flowControlledCount
        client_latency_us = $flowControlledLatency
    }

    return $Summary
}

function Get-ScaleProfiles {
    param(
        [int]$LatencySamples,
        [int]$Concurrency,
        [switch]$Enabled
    )

    if (-not $Enabled) {
        return @([ordered]@{
            name = "primary"
            requests = $LatencySamples
            concurrency = $Concurrency
        })
    }

    $smallRequests = [Math]::Max(40, [Math]::Floor($LatencySamples * 0.25))
    $mediumRequests = [Math]::Max(80, [Math]::Floor($LatencySamples * 0.5))
    $smallConcurrency = [Math]::Max(2, [Math]::Floor($Concurrency * 0.5))
    $mediumConcurrency = [Math]::Max(4, [Math]::Floor($Concurrency * 0.75))
    $includeXlarge = $LatencySamples -ge 1000 -or $Concurrency -ge 16
    $profiles = New-Object System.Collections.Generic.List[object]

    $profiles.Add([ordered]@{
        name = "small"
        requests = [int]$smallRequests
        concurrency = [int]$smallConcurrency
    })
    $profiles.Add([ordered]@{
        name = "medium"
        requests = [int]$mediumRequests
        concurrency = [int]$mediumConcurrency
    })
    $profiles.Add([ordered]@{
        name = "large"
        requests = [int]$LatencySamples
        concurrency = [int]$Concurrency
    })

    if ($includeXlarge) {
        $profiles.Add([ordered]@{
            name = "xlarge"
            requests = [int][Math]::Max($LatencySamples + 200, $LatencySamples * 2)
            concurrency = [int][Math]::Max($Concurrency + 8, $Concurrency * 2)
        })
    }

    return $profiles.ToArray()
}

function Invoke-WebSocketIntegrityScenario {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$BaseUrl,
        [string]$Secret,
        [string]$Market,
        [string]$MakerUser,
        [string]$TakerUser
    )

    $wsBase = $BaseUrl -replace "^http", "ws"
    $tradeWs = $null
    $bookWs = $null
    $tickerWs = $null
    $userWs = $null

    try {
        TopUp-ExchangeUsers -Client $Client -BaseUrl $BaseUrl -Secret $Secret -Market $Market -BuyerUsers @($TakerUser) -SellerUsers @($MakerUser) -Prefix ("ws-{0}" -f $script:RunId) -AdminSubject ("admin-ws-topup-{0}" -f $script:RunId) -PositionAmount 50 -CashAmount 50000000
        $tradeWs = Connect-ExchangeWebSocket -Url "$wsBase/ws/trades/$Market"
        $bookWs = Connect-ExchangeWebSocket -Url "$wsBase/ws/orderbook/$Market"
        $tickerWs = Connect-ExchangeWebSocket -Url "$wsBase/ws/ticker/$Market"

        $userBody = ""
        $userBytes = [System.Text.Encoding]::UTF8.GetBytes($userBody)
        $userHeaders = New-AuthHeaderMap -Method "GET" -Path "/ws/user" -Subject $TakerUser -Role "user" -Secret $Secret -BodyBytes $userBytes -RequestId ([guid]::NewGuid().ToString("N"))
        $userWs = Connect-ExchangeWebSocket -Url "$wsBase/ws/user" -Headers $userHeaders

        Start-Sleep -Milliseconds 500
        $seedMaker = New-OrderBody -MarketId $Market -Side "sell" -Price 51000 -Amount 3 -ClientOrderId "ws-maker-$([guid]::NewGuid().ToString('N').Substring(0,8))"
        $seedTaker = New-OrderBody -MarketId $Market -Side "buy" -Price 51000 -Amount 3 -ClientOrderId "ws-taker-$([guid]::NewGuid().ToString('N').Substring(0,8))"
        $makerResp = Invoke-ApiJsonRequest -Client $Client -BaseUrl $BaseUrl -Method "POST" -Path "/submit-order" -Secret $Secret -Subject $MakerUser -Role "user" -Body $seedMaker
        Start-Sleep -Milliseconds 150
        $takerResp = Invoke-ApiJsonRequest -Client $Client -BaseUrl $BaseUrl -Method "POST" -Path "/submit-order" -Secret $Secret -Subject $TakerUser -Role "user" -Body $seedTaker
        Start-Sleep -Milliseconds 400
        $balanceProbe = [ordered]@{
            user_id = $TakerUser
            amount  = 2500
            op_id   = "ws-balance-probe-$([guid]::NewGuid().ToString('N').Substring(0,12))"
        }
        $balanceProbeResp = Invoke-ApiJsonRequest -Client $Client -BaseUrl $BaseUrl -Method "POST" -Path "/deposit" -Secret $Secret -Subject ("admin-ws-probe-{0}" -f $script:RunId) -Role "admin" -Body $balanceProbe
        Start-Sleep -Milliseconds 650

        $tradeMessages = Receive-ExchangeWebSocketMessages -Socket $tradeWs -TimeoutSeconds 2 -MaxMessages 8
        $bookMessages = Receive-ExchangeWebSocketMessages -Socket $bookWs -TimeoutSeconds 2 -MaxMessages 8
        $tickerMessages = Receive-ExchangeWebSocketMessages -Socket $tickerWs -TimeoutSeconds 2 -MaxMessages 8
        $userMessages = Receive-ExchangeWebSocketMessages -Socket $userWs -TimeoutSeconds 2 -MaxMessages 12

        $metrics = Get-ApiMetricsJson -Client $Client -BaseUrl $BaseUrl -Secret $Secret
        $prom = Get-ApiPrometheusText -Client $Client -BaseUrl $BaseUrl -Secret $Secret

        $userFillMessages = @($userMessages | Where-Object { $_.parsed -and (Get-ExchangeWsMessageType -ParsedMessage $_.parsed) -eq "fill" })
        $userBalanceMessages = @($userMessages | Where-Object { $_.parsed -and (Get-ExchangeWsMessageType -ParsedMessage $_.parsed) -eq "balance_update" })
        $userMessageTypes = @($userMessages | ForEach-Object { Get-ExchangeWsMessageType -ParsedMessage $_.parsed } | Where-Object { $_ })
        $takerFills = 0
        if ($takerResp.parsed) {
            if ($takerResp.parsed -is [System.Array]) {
                foreach ($entry in $takerResp.parsed) {
                    if ($entry -and $entry.PSObject.Properties.Name -contains "fills") {
                        try {
                            $takerFills += [int]$entry.fills
                        } catch {
                        }
                    }
                }
            } elseif ($takerResp.parsed.PSObject.Properties.Name -contains "fills") {
                try {
                    $takerFills = [int]$takerResp.parsed.fills
                } catch {
                    $takerFills = 0
                }
            }
        }

        return [ordered]@{
            taker_status_code      = $takerResp.status_code
            maker_status_code      = $makerResp.status_code
            taker_fills            = $takerFills
            trade_messages         = @($tradeMessages).Count
            orderbook_messages     = @($bookMessages).Count
            ticker_messages        = @($tickerMessages).Count
            user_messages          = @($userMessages).Count
            user_fill_messages     = @($userFillMessages).Count
            user_balance_messages  = @($userBalanceMessages).Count
            user_message_types     = $userMessageTypes
            balance_probe_status   = $balanceProbeResp.status_code
            bridge_alive           = if ($metrics.parsed) { [bool]$metrics.parsed.bridge_alive } else { $false }
            ws_connections_total   = Get-MetricValueFromPrometheus -PrometheusText $prom.body -MetricName "exchange_ws_connections_total"
            ws_messages_sent_total = Get-MetricValueFromPrometheus -PrometheusText $prom.body -MetricName "exchange_ws_messages_sent_total"
        }
    } finally {
        Close-ExchangeWebSocket -Socket $tradeWs
        Close-ExchangeWebSocket -Socket $bookWs
        Close-ExchangeWebSocket -Socket $tickerWs
        Close-ExchangeWebSocket -Socket $userWs
    }
}

function Invoke-FaultReplayScenario {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$RepoRoot,
        [object]$Layout,
        [System.Diagnostics.Process]$Process,
        [string]$BaseUrl,
        [string]$Secret,
        [string[]]$Markets,
        [string[]]$BuyerUsers,
        [string[]]$SellerUsers,
        [int]$Cycles,
        [ValidateSet("debug", "release")]
        [string]$Profile,
        [string]$CargoTarget,
        [bool]$BuildBinary
    )

    $baselineWal = Get-WalSnapshot -Layout $Layout
    $restartSummaries = @()
    $currentProcess = $Process

    for ($cycle = 1; $cycle -le $Cycles; $cycle++) {
        TopUp-ExchangeUsers -Client $Client -BaseUrl $BaseUrl -Secret $Secret -Market $Markets[0] -BuyerUsers $BuyerUsers -SellerUsers $SellerUsers -Prefix ("fault-topup-{0}-{1}" -f $script:RunId, $cycle) -AdminSubject ("admin-fault-{0}-{1}" -f $script:RunId, $cycle)
        $burst = Invoke-CrossingPairBurst -Client $Client -BaseUrl $BaseUrl -Secret $Secret -Market $Markets[0] -BuyerUsers $BuyerUsers -SellerUsers $SellerUsers -PairCount 24 -PairConcurrency 3 -Prefix ("fault-{0}-{1}" -f $script:RunId, $cycle)
        $preStopMetrics = Get-ApiMetricsJson -Client $Client -BaseUrl $BaseUrl -Secret $Secret
        Stop-ExchangeApi -Process $currentProcess
        Start-Sleep -Milliseconds 800
        $currentProcess = Start-ExchangeApi -RepoRoot $RepoRoot -Layout $Layout -Profile $Profile -CargoTarget $CargoTarget -BuildBinary:$BuildBinary
        [void](Wait-ExchangeHealthy -Client $Client -BaseUrl $BaseUrl)

        $probe = New-OrderBody -MarketId $Markets[0] -Side "buy" -Price 52000 -Amount 1 -ClientOrderId ("restart-probe-{0}" -f $cycle)
        $probeResp = Invoke-ApiJsonRequest -Client $Client -BaseUrl $BaseUrl -Method "POST" -Path "/submit-order" -Secret $Secret -Subject $BuyerUsers[$cycle % $BuyerUsers.Count] -Role "user" -Body $probe
        $postMetrics = Get-ApiMetricsJson -Client $Client -BaseUrl $BaseUrl -Secret $Secret

        $restartSummaries += [ordered]@{
            cycle                     = $cycle
            probe_status_code         = $probeResp.status_code
            pre_stop_orders_received  = if ($preStopMetrics.parsed) { [int64]$preStopMetrics.parsed.orders_received } else { 0 }
            post_restart_orders_received = if ($postMetrics.parsed) { [int64]$postMetrics.parsed.orders_received } else { 0 }
            burst_success_count       = @($burst | Where-Object { $_.status_code -ge 200 -and $_.status_code -lt 300 }).Count
        }
    }

    $finalWal = Get-WalSnapshot -Layout $Layout
    $prom = Get-ApiPrometheusText -Client $Client -BaseUrl $BaseUrl -Secret $Secret
    $walGrowth = 0
    foreach ($file in $finalWal.Keys) {
        $walGrowth += ([int64]$finalWal[$file].bytes - [int64]$baselineWal[$file].bytes)
    }

    return [ordered]@{
        restart_cycles             = $Cycles
        post_restart_probe_passed  = (@($restartSummaries | Where-Object { $_.probe_status_code -eq 200 }).Count -eq $Cycles)
        wal_growth_bytes           = $walGrowth
        baseline_wal               = $baselineWal
        final_wal                  = $finalWal
        restart_summaries          = $restartSummaries
        prometheus_wal_errors_total = Get-MetricValueFromPrometheus -PrometheusText $prom.body -MetricName "exchange_wal_errors_total"
        process                    = $currentProcess
    }
}

function Invoke-SoakScenario {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$RepoRoot,
        [object]$Layout,
        [System.Diagnostics.Process]$Process,
        [string]$BaseUrl,
        [string]$Secret,
        [string[]]$Markets,
        [string[]]$BuyerUsers,
        [string[]]$SellerUsers,
        [int]$DurationSeconds,
        [int]$BurstSize,
        [ValidateSet("debug", "release")]
        [string]$Profile,
        [string]$CargoTarget,
        [bool]$BuildBinary
    )

    if ($DurationSeconds -le 0 -or $BurstSize -le 0) {
        return [ordered]@{
            samples                   = 0
            restart_count             = 0
            working_set_initial_bytes = 0
            working_set_peak_bytes    = 0
            working_set_final_bytes   = 0
            private_initial_bytes     = 0
            private_peak_bytes        = 0
            private_final_bytes       = 0
            detail                    = @()
            process                   = $Process
        }
    }

    $currentProcess = $Process
    $samples = @()
    $restartCount = 0
    $iterations = [Math]::Max(1, [Math]::Floor($DurationSeconds / 15))
    if ($iterations -lt 1) { $iterations = 1 }

    for ($iteration = 1; $iteration -le $iterations; $iteration++) {
        TopUp-ExchangeUsers -Client $Client -BaseUrl $BaseUrl -Secret $Secret -Market $Markets[0] -BuyerUsers $BuyerUsers -SellerUsers $SellerUsers -Prefix ("soak-topup-{0}-{1}" -f $script:RunId, $iteration) -AdminSubject ("admin-soak-{0}-{1}" -f $script:RunId, $iteration) -PositionAmount 2000 -CashAmount 50000000
        $pairCount = [Math]::Max(1, [Math]::Floor($BurstSize / 2))
        $burst = Invoke-CrossingPairBurst -Client $Client -BaseUrl $BaseUrl -Secret $Secret -Market $Markets[0] -BuyerUsers $BuyerUsers -SellerUsers $SellerUsers -PairCount $pairCount -PairConcurrency 2 -Prefix ("soak-{0}-{1}" -f $script:RunId, $iteration)
        $metrics = Get-ApiMetricsJson -Client $Client -BaseUrl $BaseUrl -Secret $Secret
        $memory = Get-ProcessMemorySample -Process $currentProcess
        $samples += [ordered]@{
            iteration            = $iteration
            timestamp            = [DateTimeOffset]::UtcNow.ToString("o")
            orders_ok            = @($burst | Where-Object { $_.status_code -ge 200 -and $_.status_code -lt 300 }).Count
            orders_fail          = @($burst | Where-Object { $_.status_code -lt 200 -or $_.status_code -ge 300 }).Count
            working_set_bytes    = if ($memory.available) { $memory.working_set_bytes } else { 0 }
            private_bytes        = if ($memory.available) { $memory.private_bytes } else { 0 }
            orders_received      = if ($metrics.parsed) { [int64]$metrics.parsed.orders_received } else { 0 }
            bridge_alive         = if ($metrics.parsed) { [bool]$metrics.parsed.bridge_alive } else { $false }
            wal_errors           = if ($metrics.parsed) { [int64]$metrics.parsed.wal_errors } else { 0 }
        }

        if ($iteration -lt $iterations -and $iteration % 4 -eq 0) {
            Stop-ExchangeApi -Process $currentProcess
            Start-Sleep -Milliseconds 500
            $currentProcess = Start-ExchangeApi -RepoRoot $RepoRoot -Layout $Layout -Profile $Profile -CargoTarget $CargoTarget -BuildBinary:$BuildBinary
            [void](Wait-ExchangeHealthy -Client $Client -BaseUrl $BaseUrl)
            $restartCount++
        }

        if ($iteration -lt $iterations) {
            Start-Sleep -Seconds 5
        }
    }

    $workingSetValues = @()
    $privateValues = @()
    foreach ($sample in $samples) {
        $workingSetValues += [double]$sample.working_set_bytes
        $privateValues += [double]$sample.private_bytes
    }

    return [ordered]@{
        samples                  = $samples.Count
        restart_count            = $restartCount
        working_set_initial_bytes = if ($workingSetValues.Count -gt 0) { [int64]$workingSetValues[0] } else { 0 }
        working_set_peak_bytes   = if ($workingSetValues.Count -gt 0) { [int64](($workingSetValues | Measure-Object -Maximum).Maximum) } else { 0 }
        working_set_final_bytes  = if ($workingSetValues.Count -gt 0) { [int64]$workingSetValues[-1] } else { 0 }
        private_initial_bytes    = if ($privateValues.Count -gt 0) { [int64]$privateValues[0] } else { 0 }
        private_peak_bytes       = if ($privateValues.Count -gt 0) { [int64](($privateValues | Measure-Object -Maximum).Maximum) } else { 0 }
        private_final_bytes      = if ($privateValues.Count -gt 0) { [int64]$privateValues[-1] } else { 0 }
        detail                   = $samples
        process                  = $currentProcess
    }
}

try {
    $script:RunId = $runId
    $resolvedBinaryPath = if (-not [string]::IsNullOrWhiteSpace($binaryPath)) {
        $binaryPath
    } else {
        Get-ApiBinaryPath -RepoRoot $repoRoot -Profile $Profile -CargoTarget $effectiveCargoTarget
    }
    $shouldBuildBinary = $BuildBinary.IsPresent -or -not (Test-Path $resolvedBinaryPath)
    $process = Invoke-TimedPhase -Name "start_api" -Action {
        $p = Start-ExchangeApi -RepoRoot $repoRoot -Layout $layout -Profile $Profile -CargoTarget $effectiveCargoTarget -BinaryPath $resolvedBinaryPath -BuildBinary:$shouldBuildBinary
        [void](Wait-ExchangeHealthy -Client $client -BaseUrl $resolvedBaseUrl)
        $p
    }
    Write-Host "API server ready" -ForegroundColor Green

    $markets = Invoke-TimedPhase -Name "discover_markets" -Action {
        Get-PreferredMarketSet -Client $client -BaseUrl $resolvedBaseUrl -Secret $Secret
    }
    $tradingMarkets = @("btc-usdt")
    $userCount = if ($effectiveHttpBenchClient -eq "go") {
        [Math]::Max(24, [Math]::Min(192, $Concurrency * 4))
    } else {
        [Math]::Max(6, [Math]::Min(10, $Concurrency))
    }
    $seedUserSets = New-MakerAndBuyerSets -Markets $tradingMarkets -UserCount ([Math]::Max(4, [Math]::Min(6, $userCount))) -Prefix "seed"
    $httpUserSets = New-MakerAndBuyerSets -Markets $tradingMarkets -UserCount $userCount -Prefix "http"
    $wsUserSets = New-MakerAndBuyerSets -Markets $tradingMarkets -UserCount 2 -Prefix "ws"
    $faultUserSets = New-MakerAndBuyerSets -Markets $tradingMarkets -UserCount ([Math]::Max(4, [Math]::Min(6, $userCount))) -Prefix "fault"
    $soakUserSets = New-MakerAndBuyerSets -Markets $tradingMarkets -UserCount ([Math]::Max(4, [Math]::Min(6, $userCount))) -Prefix "soak"
    Write-Host "Markets: $([string]::Join(', ', $markets))" -ForegroundColor DarkGray
    Write-Host "Trading markets: $([string]::Join(', ', $tradingMarkets))" -ForegroundColor DarkGray

    Invoke-TimedPhase -Name "seed_users_and_books" -Action {
        Seed-ExchangeUsers -Client $client -BaseUrl $resolvedBaseUrl -Secret $Secret -Markets $tradingMarkets -BuyerUsers $seedUserSets.buyers -SellerUsers $seedUserSets.sellers -AdminSubject ("admin-seed-{0}" -f $runId)
        foreach ($market in $tradingMarkets) {
            Seed-MarketDepth -Client $client -BaseUrl $resolvedBaseUrl -Secret $Secret -MarketId $market -SellerUsers $seedUserSets.sellers -BasePrice 50000 -DepthOrders $WarmDepthOrders
        }
    }
    Write-Host "Seeded users and market depth" -ForegroundColor Green

    $scaleProfiles = Get-ScaleProfiles -LatencySamples $LatencySamples -Concurrency $Concurrency -Enabled:$ScaleLadder
    $scaleSummaries = New-Object System.Collections.Generic.List[object]
    $clientModeSummaries = New-Object System.Collections.Generic.List[object]
    $primaryBurstResults = $null
    $metricsAfterBurst = $null
    Invoke-TimedPhase -Name "http_latency" -Action {
        $modeConfigs = @()
        if ($effectiveHttpBenchClient -eq "go" -and $CompareGoClientModes) {
            $modeConfigs += [ordered]@{
                name = "keepalive_off"
                description = "DisableKeepAlives=true; conservative, stable, slightly pessimistic"
                disable_keep_alives = $true
            }
            $modeConfigs += [ordered]@{
                name = "keepalive_on"
                description = "DisableKeepAlives=false; closer to a production HTTP client"
                disable_keep_alives = $false
            }
        } else {
            $modeConfigs += [ordered]@{
                name = if ($effectiveHttpBenchClient -eq "go") {
                    if ($GoDisableKeepAlives) { "keepalive_off" } else { "keepalive_on" }
                } else { "powershell" }
                description = if ($effectiveHttpBenchClient -eq "go") {
                    if ($GoDisableKeepAlives) {
                        "DisableKeepAlives=true; conservative, stable, slightly pessimistic"
                    } else {
                        "DisableKeepAlives=false; closer to a production HTTP client"
                    }
                } else { "PowerShell/.NET harness" }
                disable_keep_alives = $GoDisableKeepAlives
            }
        }

        foreach ($modeConfig in $modeConfigs) {
            $modeScaleSummaries = New-Object System.Collections.Generic.List[object]
            foreach ($scale in $scaleProfiles) {
                $pairCount = [Math]::Max(1, [Math]::Floor($scale.requests / 2))
                $pairConcurrency = [Math]::Max(1, [Math]::Floor($scale.concurrency / 2))
                $scaleUserCount = if ($effectiveHttpBenchClient -eq "go") {
                    [Math]::Max(24, [Math]::Min(192, $scale.concurrency * 4))
                } else {
                    $userCount
                }
                $scaleUserSets = New-MakerAndBuyerSets -Markets $tradingMarkets -UserCount $scaleUserCount -Prefix ("http-{0}-{1}" -f $modeConfig.name, $scale.name)
                $requestsPerUser = [Math]::Ceiling($scale.requests / [Math]::Max(1, $scaleUserSets.buyers.Count))
                $initialCashPerUser = [int64][Math]::Max(50000000, $requestsPerUser * 50000 * 32)
                $initialPositionPerUser = [int64][Math]::Max(1024, $requestsPerUser * 32)
                $cashThresholdBps = [int64][Math]::Max(160000, $requestsPerUser * 12000)
                $cashTargetBps = [int64][Math]::Max(800000, $cashThresholdBps * 4)
                $posThresholdUnits = [int64][Math]::Max(32, $requestsPerUser * 4)
                $posTargetUnits = [int64][Math]::Max(1024, $requestsPerUser * 32)
                $rateLimitPerSecond = if ($effectiveHttpBenchClient -eq "go") {
                    [int][Math]::Max(18, [Math]::Min(24, [Math]::Floor($scale.concurrency * 0.5)))
                } else {
                    48
                }
                TopUp-ExchangeUsers -Client $client -BaseUrl $resolvedBaseUrl -Secret $Secret -Market $tradingMarkets[0] -BuyerUsers $scaleUserSets.buyers -SellerUsers $scaleUserSets.sellers -Prefix ("http-topup-{0}-{1}-{2}" -f $runId, $modeConfig.name, $scale.name) -AdminSubject ("admin-http-{0}-{1}-{2}" -f $runId, $modeConfig.name, $scale.name) -CashAmount $initialCashPerUser -PositionAmount $initialPositionPerUser
                if ($effectiveHttpBenchClient -eq "go") {
                    $summary = Invoke-GoHttpBenchmark -WorkspaceRoot $workspaceRoot -BaseUrl $resolvedBaseUrl -Secret $Secret -Market $tradingMarkets[0] -BuyerUsers $scaleUserSets.buyers -SellerUsers $scaleUserSets.sellers -PairCount $pairCount -PairConcurrency $pairConcurrency -BasePrice 50000 -Amount 1 -RateLimitPerSecond $rateLimitPerSecond -Prefix ("http-{0}-{1}-{2}" -f $runId, $modeConfig.name, $scale.name) -DisableKeepAlives $modeConfig.disable_keep_alives -InitialCash $initialCashPerUser -InitialPosition $initialPositionPerUser -CashThresholdBps $cashThresholdBps -CashTargetBps $cashTargetBps -PosThresholdUnits $posThresholdUnits -PosTargetUnits $posTargetUnits -RateLimitRetryMax 2 -RateLimitBackoffMs 150 -RequestStaggerMs 5
                    $scaleMetrics = Get-ApiMetricsJson -Client $client -BaseUrl $resolvedBaseUrl -Secret $Secret
                    $summary["server_metrics"] = Get-ServerMetricsSummary -MetricsJson $scaleMetrics
                    $summary["client_impl"] = "go"
                    $summary["client_mode"] = $modeConfig.name
                    $summary["client_mode_description"] = $modeConfig.description
                    $summary = Set-PrimaryHttpMetricFields -Summary $summary
                    $primaryBurstResults = $null
                } else {
                    $burstResults = Invoke-CrossingPairBurst -Client $client -BaseUrl $resolvedBaseUrl -Secret $Secret -Market $tradingMarkets[0] -BuyerUsers $httpUserSets.buyers -SellerUsers $httpUserSets.sellers -PairCount $pairCount -PairConcurrency $pairConcurrency -Prefix ("http-{0}-{1}" -f $runId, $scale.name)
                    $scaleMetrics = Get-ApiMetricsJson -Client $client -BaseUrl $resolvedBaseUrl -Secret $Secret
                    $summary = Convert-BurstToSummary -Results $burstResults -MetricsJson $scaleMetrics
                    $summary["client_mode"] = $modeConfig.name
                    $summary["client_mode_description"] = $modeConfig.description
                    $summary = Set-PrimaryHttpMetricFields -Summary $summary
                    $primaryBurstResults = $burstResults
                }
                $summary["scale_name"] = $scale.name
                $summary["configured_requests"] = $scale.requests
                $summary["configured_concurrency"] = $scale.concurrency
                $scaleSummaries.Add($summary)
                $modeScaleSummaries.Add($summary)
                $metricsAfterBurst = $scaleMetrics
                Write-Host ("  mode {0} / scale {1}: success {2}/{3}, fills={4}, overall P99={5}us, success P99={6}us, error P99={7}us" -f `
                    $modeConfig.name, $scale.name, $summary.success_count, $summary.total_requests, $summary.fills_reported, $summary.client_latency_us.p99, $summary.success_path.client_latency_us.p99, $summary.error_path.client_latency_us.p99) -ForegroundColor DarkGray
            }

            $modeScaleArray = @($modeScaleSummaries.ToArray() | ForEach-Object { ConvertTo-PlainOrdered -InputObject $_ })
            $modeSummary = ConvertTo-PlainOrdered -InputObject $modeScaleArray[$modeScaleArray.Length - 1]
            $modeSummary["scale_runs"] = $modeScaleArray
            $modeSummary["client_mode"] = $modeConfig.name
            $modeSummary["client_mode_description"] = $modeConfig.description
            $modeSummary = Set-PrimaryHttpMetricFields -Summary $modeSummary
            $clientModeSummaries.Add($modeSummary)
        }
    }
    $scaleSummaryArray = @($scaleSummaries.ToArray() | ForEach-Object { ConvertTo-PlainOrdered -InputObject $_ })
    $clientModeSummaryArray = @($clientModeSummaries.ToArray() | ForEach-Object { ConvertTo-PlainOrdered -InputObject $_ })
    $preferredModeSummary = $clientModeSummaryArray | Where-Object { $_["client_mode"] -eq "keepalive_on" } | Select-Object -First 1
    if (-not $preferredModeSummary) {
        $preferredModeSummary = $clientModeSummaryArray[$clientModeSummaryArray.Length - 1]
    }
    $httpSummary = ConvertTo-PlainOrdered -InputObject $preferredModeSummary
    $httpSummary["scale_runs"] = $scaleSummaryArray
    $httpSummary["client_mode_runs"] = $clientModeSummaryArray
    $httpSummary = Set-PrimaryHttpMetricFields -Summary $httpSummary
    $httpSummary["scale_ladder_summary"] = @(
        foreach ($scale in $scaleSummaryArray) {
            $apiRateLimitCount = 0
            $riskRejectCount = 0
            foreach ($category in @($scale["error_categories"])) {
                if ($category["category"] -eq "api rate limit") {
                    $apiRateLimitCount += [int]$category["count"]
                }
                if ($category["category"] -eq "risk reject") {
                    $riskRejectCount += [int]$category["count"]
                }
            }
            [ordered]@{
                scale_name                 = $scale["scale_name"]
                client_mode                = $scale["client_mode"]
                configured_requests        = $scale["configured_requests"]
                configured_concurrency     = $scale["configured_concurrency"]
                system_core_success_count  = $scale["system_core_metric"]["count"]
                system_core_total          = $scale["system_core_metric"]["clean_total"]
                system_core_success_rate   = $scale["system_core_metric"]["success_rate_pct"]
                single_ip_success_count    = $scale["single_ip_metric"]["count"]
                single_ip_total            = $scale["single_ip_metric"]["clean_total"]
                single_ip_success_rate     = $scale["single_ip_metric"]["success_rate_pct"]
                direct_success_p50_us      = $scale["primary_metric"]["client_latency_us"]["p50"]
                direct_success_p95_us      = $scale["primary_metric"]["client_latency_us"]["p95"]
                direct_success_p99_us      = $scale["primary_metric"]["client_latency_us"]["p99"]
                direct_success_p999_us     = $scale["primary_metric"]["client_latency_us"]["p999"]
                rescued_success_count      = $scale["rescued_success_metric"]["count"]
                flow_controlled_count      = $scale["flow_controlled_success_metric"]["count"]
                api_rate_limit_count       = $apiRateLimitCount
                risk_reject_count          = $riskRejectCount
                excluded_api_limits        = $scale["system_core_metric"]["excluded_api_limits"]
                excluded_429               = $scale["single_ip_metric"]["excluded_429"]
                fills_reported             = $scale["fills_reported"]
            }
        }
    )
    Write-Host ("HTTP client latency P99/P999: {0}us / {1}us" -f $httpSummary.client_latency_us.p99, $httpSummary.client_latency_us.p999) -ForegroundColor Yellow

    $wsSummary = Invoke-TimedPhase -Name "websocket_integrity" -Action {
        Invoke-WebSocketIntegrityScenario -Client $client -BaseUrl $resolvedBaseUrl -Secret $Secret -Market $tradingMarkets[0] -MakerUser $wsUserSets.sellers[0] -TakerUser $wsUserSets.buyers[0]
    }
    Write-Host ("WS messages: trade={0}, orderbook={1}, ticker={2}, user_fill={3}, user_balance={4}" -f `
        $wsSummary.trade_messages, $wsSummary.orderbook_messages, $wsSummary.ticker_messages, $wsSummary.user_fill_messages, $wsSummary.user_balance_messages) -ForegroundColor Yellow

    $faultSummary = Invoke-TimedPhase -Name "fault_replay" -Action {
        Invoke-FaultReplayScenario -Client $client -RepoRoot $repoRoot -Layout $layout -Process $process -BaseUrl $resolvedBaseUrl -Secret $Secret -Markets $tradingMarkets -BuyerUsers $faultUserSets.buyers -SellerUsers $faultUserSets.sellers -Cycles $FaultCycles -Profile $Profile -CargoTarget $effectiveCargoTarget -BuildBinary:$false
    }
    $process = $faultSummary.process
    $faultSummary.Remove("process")
    Write-Host ("Fault replay cycles passed: {0}/{1}" -f `
        (@($faultSummary.restart_summaries | Where-Object { $_.probe_status_code -eq 200 }).Count), $faultSummary.restart_cycles) -ForegroundColor Yellow

    $soakSummary = Invoke-TimedPhase -Name "soak" -Action {
        Invoke-SoakScenario -Client $client -RepoRoot $repoRoot -Layout $layout -Process $process -BaseUrl $resolvedBaseUrl -Secret $Secret -Markets $tradingMarkets -BuyerUsers $soakUserSets.buyers -SellerUsers $soakUserSets.sellers -DurationSeconds $SoakSeconds -BurstSize $SoakBurstSize -Profile $Profile -CargoTarget $effectiveCargoTarget -BuildBinary:$false
    }
    $process = $soakSummary.process
    $soakSummary.Remove("process")
    Write-Host ("Soak working set initial/peak/final: {0} / {1} / {2}" -f `
        $soakSummary.working_set_initial_bytes, $soakSummary.working_set_peak_bytes, $soakSummary.working_set_final_bytes) -ForegroundColor Yellow

    foreach ($modeSummary in $httpSummary.client_mode_runs) {
        $modeSummary["user_balance_messages"] = $wsSummary.user_balance_messages
    }
    $httpSummary["user_balance_messages"] = $wsSummary.user_balance_messages

    $report = [ordered]@{
        run_id                = $runId
        mode                  = $effectiveMode
        base_url              = $resolvedBaseUrl
        markets               = $markets
        started_at            = $startedAt
        completed_at          = [DateTimeOffset]::UtcNow.ToString("o")
        artifacts             = [ordered]@{
            config_path = $layout.config_path
            stdout_log  = $layout.stdout_log
            stderr_log  = $layout.stderr_log
            json_report = $layout.json_report
            md_report   = $layout.md_report
            csv_report  = $layout.csv_report
            binary_path = $resolvedBinaryPath
        }
        http_latency          = $httpSummary
        websocket_integrity   = $wsSummary
        fault_replay          = $faultSummary
        soak                  = $soakSummary
    }
    $report["regression_summary"] = Compare-BackendRegression -CurrentReport $report -BaselineReportPath $resolvedBaselineReportPath

    Write-BackendReportFiles -Layout $layout -Report $report
    Write-Host ""
    Write-Host "Reports written:" -ForegroundColor Green
    Write-Host "  $($layout.json_report)" -ForegroundColor DarkGray
    Write-Host "  $($layout.md_report)" -ForegroundColor DarkGray
    Write-Host "  $($layout.csv_report)" -ForegroundColor DarkGray
    if ($report["regression_summary"]["status"] -eq "fail") {
        Write-Warning ("Regression checks failed: {0} failed, {1} warnings" -f $report["regression_summary"]["failed_checks"], $report["regression_summary"]["warning_checks"])
        if ($FailOnRegression) {
            throw "Regression checks failed"
        }
    } elseif ($report["regression_summary"]["status"] -eq "warn") {
        Write-Warning ("Regression checks warned: {0} warnings" -f $report["regression_summary"]["warning_checks"])
    } elseif ($report["regression_summary"]["status"] -eq "pass") {
        Write-Host "Regression checks: pass" -ForegroundColor Green
    }
} finally {
    Stop-ExchangeApi -Process $process
    if ($client) {
        $client.Dispose()
    }
}
