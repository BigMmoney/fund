param(
    [string]$BaseUrl = "",
    [int]$Port = 3131,
    [string]$Secret = "dev-secret-change-me-to-32-chars-min!",
    [ValidateSet("debug", "release")]
    [string]$Profile = "release",
    [string]$CargoTarget = "",
    [switch]$BuildBinary
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

. "$PSScriptRoot/backend_resilience_lib.ps1"

function Invoke-SubmitOrderWave {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$BaseUrl,
        [string]$Secret,
        [object[]]$Specs
    )

    $entries = New-Object System.Collections.Generic.List[object]
    foreach ($spec in $Specs) {
        $jsonBody = $spec.body | ConvertTo-Json -Compress -Depth 8
        $entry = New-RequestWaveEntry -Client $Client -BaseUrl $BaseUrl -Path "/submit-order" -Method "POST" -Subject $spec.subject -Role "user" -Secret $Secret -JsonBody $jsonBody -Metadata ([ordered]@{
            scenario = $spec.scenario
            user     = $spec.subject
        })
        $entries.Add($entry)
    }

    $results = New-Object System.Collections.Generic.List[object]
    foreach ($entry in $entries) {
        $results.Add((Complete-RequestWaveEntry -Entry $entry))
    }
    return $results.ToArray()
}

function Get-RateLimitMetricSlice {
    param(
        [object]$MetricsResponse
    )

    $parsed = if ($MetricsResponse) { $MetricsResponse.parsed } else { $null }
    return [ordered]@{
        submit_order_ip_rate_limited     = if ($parsed -and $parsed.PSObject.Properties.Name -contains "submit_order_ip_rate_limited") { [int64]$parsed.submit_order_ip_rate_limited } else { 0 }
        submit_order_user_rate_limited   = if ($parsed -and $parsed.PSObject.Properties.Name -contains "submit_order_user_rate_limited") { [int64]$parsed.submit_order_user_rate_limited } else { 0 }
        submit_order_engine_rate_limited = if ($parsed -and $parsed.PSObject.Properties.Name -contains "submit_order_engine_rate_limited") { [int64]$parsed.submit_order_engine_rate_limited } else { 0 }
        http_requests_total              = if ($parsed -and $parsed.PSObject.Properties.Name -contains "http_requests_total") { [int64]$parsed.http_requests_total } else { 0 }
        http_errors_total                = if ($parsed -and $parsed.PSObject.Properties.Name -contains "http_errors_total") { [int64]$parsed.http_errors_total } else { 0 }
    }
}

function Get-RateLimitMetricDelta {
    param(
        [object]$Before,
        [object]$After
    )

    return [ordered]@{
        submit_order_ip_rate_limited     = [int64]$After.submit_order_ip_rate_limited - [int64]$Before.submit_order_ip_rate_limited
        submit_order_user_rate_limited   = [int64]$After.submit_order_user_rate_limited - [int64]$Before.submit_order_user_rate_limited
        submit_order_engine_rate_limited = [int64]$After.submit_order_engine_rate_limited - [int64]$Before.submit_order_engine_rate_limited
        http_requests_total              = [int64]$After.http_requests_total - [int64]$Before.http_requests_total
        http_errors_total                = [int64]$After.http_errors_total - [int64]$Before.http_errors_total
    }
}

function Summarize-ProbeScenario {
    param(
        [string]$Name,
        [object[]]$Results,
        [object]$MetricDelta
    )

    $statusBreakdown = [ordered]@{}
    foreach ($result in $Results) {
        $key = [string]$result.status_code
        if (-not $statusBreakdown.Contains($key)) {
            $statusBreakdown[$key] = 0
        }
        $statusBreakdown[$key]++
    }

    $first429 = $Results | Where-Object { $_.status_code -eq 429 } | Select-Object -First 1
    return [ordered]@{
        scenario         = $Name
        total_requests   = @($Results).Count
        status_breakdown = $statusBreakdown
        first_429        = if ($first429) {
            [ordered]@{
                status_code = $first429.status_code
                body        = $first429.body
                parsed      = $first429.parsed
                latency_us  = $first429.latency_us
                subject     = $first429.subject
                request_id  = $first429.request_id
            }
        } else {
            $null
        }
        metric_delta     = $MetricDelta
    }
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$runId = [DateTimeOffset]::UtcNow.ToString("yyyyMMdd-HHmmss")
$artifactsRoot = Join-Path $repoRoot "artifacts/rate-limit-probe"
$layout = New-BackendRunLayout -Root $artifactsRoot -RunId $runId
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

[void](Write-BackendExchangeConfig -Layout $layout -Port $resolvedPort)

$client = New-HttpClient -TimeoutSeconds 30
$process = $null

try {
    $process = Start-ExchangeApi -RepoRoot $repoRoot -Layout $layout -Profile $Profile -CargoTarget $effectiveCargoTarget -BinaryPath $binaryPath -BuildBinary:$BuildBinary
    [void](Wait-ExchangeHealthy -Client $client -BaseUrl $resolvedBaseUrl -TimeoutSeconds 45)

    $market = "btc-usdt"
    $sameUser = "probe-user-limit"
    $sellers = @("probe-seller-0", "probe-seller-1", "probe-seller-2", "probe-seller-3")
    $ipUsers = 0..79 | ForEach-Object { "probe-ip-user-$_" }
    $allBuyers = @($sameUser) + $ipUsers

    Seed-ExchangeUsers -Client $client -BaseUrl $resolvedBaseUrl -Secret $Secret -Markets @($market) -BuyerUsers $allBuyers -SellerUsers $sellers -AdminSubject ("admin-probe-$runId") -PauseEvery 8 -PauseMs 50
    Seed-MarketDepth -Client $client -BaseUrl $resolvedBaseUrl -Secret $Secret -MarketId $market -SellerUsers $sellers -BasePrice 50000 -DepthOrders 160 -AmountPerOrder 10

    Start-Sleep -Milliseconds 1300

    $beforeSameUser = Get-RateLimitMetricSlice -MetricsResponse (Get-ApiMetricsJson -Client $client -BaseUrl $resolvedBaseUrl -Secret $Secret)
    $sameUserSpecs = 0..39 | ForEach-Object {
        [pscustomobject]@{
            scenario = "same_user_burst"
            subject  = $sameUser
            body     = (New-OrderBody -MarketId $market -Side "buy" -Price 50050 -Amount 1 -ClientOrderId ("probe-user-{0}" -f $_))
        }
    }
    $sameUserResults = Invoke-SubmitOrderWave -Client $client -BaseUrl $resolvedBaseUrl -Secret $Secret -Specs $sameUserSpecs
    $afterSameUser = Get-RateLimitMetricSlice -MetricsResponse (Get-ApiMetricsJson -Client $client -BaseUrl $resolvedBaseUrl -Secret $Secret)
    $sameUserSummary = Summarize-ProbeScenario -Name "same_user_burst" -Results $sameUserResults -MetricDelta (Get-RateLimitMetricDelta -Before $beforeSameUser -After $afterSameUser)

    Start-Sleep -Milliseconds 1300

    $beforeIp = Get-RateLimitMetricSlice -MetricsResponse (Get-ApiMetricsJson -Client $client -BaseUrl $resolvedBaseUrl -Secret $Secret)
    $ipSpecs = 0..79 | ForEach-Object {
        [pscustomobject]@{
            scenario = "multi_user_same_ip_burst"
            subject  = $ipUsers[$_]
            body     = (New-OrderBody -MarketId $market -Side "buy" -Price 50060 -Amount 1 -ClientOrderId ("probe-ip-{0}" -f $_))
        }
    }
    $ipResults = Invoke-SubmitOrderWave -Client $client -BaseUrl $resolvedBaseUrl -Secret $Secret -Specs $ipSpecs
    $afterIp = Get-RateLimitMetricSlice -MetricsResponse (Get-ApiMetricsJson -Client $client -BaseUrl $resolvedBaseUrl -Secret $Secret)
    $ipSummary = Summarize-ProbeScenario -Name "multi_user_same_ip_burst" -Results $ipResults -MetricDelta (Get-RateLimitMetricDelta -Before $beforeIp -After $afterIp)

    $report = [ordered]@{
        run_id       = $runId
        base_url     = $resolvedBaseUrl
        market       = $market
        completed_at = [DateTimeOffset]::UtcNow.ToString("o")
        artifacts    = [ordered]@{
            config_path = $layout.config_path
            stdout_log  = $layout.stdout_log
            stderr_log  = $layout.stderr_log
            json_report = $layout.json_report
            md_report   = $layout.md_report
        }
        scenarios    = @(
            $sameUserSummary,
            $ipSummary
        )
    }

    $json = $report | ConvertTo-Json -Depth 12
    Set-Content -Path $layout.json_report -Value $json -Encoding UTF8

    $md = @(
        "# Rate Limit Probe"
        ""
        "- Run ID: $runId"
        "- Base URL: $resolvedBaseUrl"
        "- Market: $market"
        ""
        "## same_user_burst"
        ""
        "- Statuses: $(($sameUserSummary.status_breakdown | ConvertTo-Json -Compress))"
        "- Metric delta: $(($sameUserSummary.metric_delta | ConvertTo-Json -Compress))"
        "- First 429 body: $($sameUserSummary.first_429.body)"
        ""
        "## multi_user_same_ip_burst"
        ""
        "- Statuses: $(($ipSummary.status_breakdown | ConvertTo-Json -Compress))"
        "- Metric delta: $(($ipSummary.metric_delta | ConvertTo-Json -Compress))"
        "- First 429 body: $($ipSummary.first_429.body)"
    ) -join "`n"
    Set-Content -Path $layout.md_report -Value $md -Encoding UTF8

    Write-Host "Probe reports written:" -ForegroundColor Green
    Write-Host "  $($layout.json_report)" -ForegroundColor DarkGray
    Write-Host "  $($layout.md_report)" -ForegroundColor DarkGray
} finally {
    Stop-ExchangeApi -Process $process
    if ($client) {
        $client.Dispose()
    }
}
