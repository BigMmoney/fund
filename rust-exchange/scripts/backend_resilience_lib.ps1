Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function New-HttpClient {
    param(
        [int]$TimeoutSeconds = 30
    )

    Add-Type -AssemblyName "System.Net.Http" | Out-Null
    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.UseCookies = $false
    $client = [System.Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromSeconds($TimeoutSeconds)
    return $client
}

function Get-Sha256Hex {
    param(
        [byte[]]$Bytes
    )

    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hashBytes = $sha.ComputeHash($Bytes)
    } finally {
        $sha.Dispose()
    }
    return [BitConverter]::ToString($hashBytes).Replace("-", "").ToLowerInvariant()
}

function Get-HmacHex {
    param(
        [string]$Message,
        [string]$Secret
    )

    $hmac = [System.Security.Cryptography.HMACSHA256]::new(
        [System.Text.Encoding]::UTF8.GetBytes($Secret)
    )
    try {
        $hashBytes = $hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($Message))
    } finally {
        $hmac.Dispose()
    }
    return [BitConverter]::ToString($hashBytes).Replace("-", "").ToLowerInvariant()
}

function New-AuthHeaderMap {
    param(
        [string]$Method,
        [string]$Path,
        [string]$Subject,
        [string]$Role,
        [string]$Secret,
        [byte[]]$BodyBytes,
        [string]$RequestId,
        [string]$SessionId = ""
    )

    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $payload = "{0}`n{1}`n`n{2}`n{3}`n{4}`n{5}`n{6}" -f `
        $Method.ToUpperInvariant(), $Path, $Subject, $Role, $SessionId, $timestamp, $RequestId
    $signature = Get-HmacHex -Message $payload -Secret $Secret
    $bodyHash = Get-Sha256Hex -Bytes $BodyBytes

    return [ordered]@{
        "x-request-id"                = $RequestId
        "x-internal-auth-subject"     = $Subject
        "x-internal-auth-role"        = $Role
        "x-internal-auth-session-id"  = $SessionId
        "x-internal-auth-timestamp"   = $timestamp
        "x-internal-auth-signature"   = $signature
        "x-internal-auth-body-sha256" = $bodyHash
    }
}

function New-JsonContent {
    param(
        [string]$Json
    )

    $bytes = [System.Text.Encoding]::UTF8.GetBytes($Json)
    $content = [System.Net.Http.ByteArrayContent]::new($bytes)
    $content.Headers.ContentType = [System.Net.Http.Headers.MediaTypeHeaderValue]::new("application/json")
    return [pscustomobject]@{
        Bytes   = $bytes
        Content = $content
    }
}

function Invoke-ApiJsonRequest {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$BaseUrl,
        [string]$Method,
        [string]$Path,
        [string]$Secret,
        [string]$Subject,
        [string]$Role,
        [object]$Body = $null,
        [int]$TimeoutSeconds = 30
    )

    $requestId = [guid]::NewGuid().ToString("N")
    $json = if ($null -eq $Body) { "" } elseif ($Body -is [string]) { [string]$Body } else { $Body | ConvertTo-Json -Compress -Depth 8 }
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($json)
    $headers = New-AuthHeaderMap -Method $Method -Path $Path -Subject $Subject -Role $Role -Secret $Secret -BodyBytes $bodyBytes -RequestId $requestId
    $uri = [Uri]::new(("{0}{1}" -f $BaseUrl.TrimEnd('/'), $Path))
    $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::new($Method.ToUpperInvariant()), $uri)
    $request.Headers.TryAddWithoutValidation("Accept", "application/json") | Out-Null

    foreach ($key in $headers.Keys) {
        $request.Headers.TryAddWithoutValidation($key, [string]$headers[$key]) | Out-Null
    }

    $jsonContent = $null
    if ($Method.ToUpperInvariant() -ne "GET") {
        $jsonContent = New-JsonContent -Json $json
        $request.Content = $jsonContent.Content
    }

    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $response = $Client.SendAsync($request).GetAwaiter().GetResult()
        $bodyText = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
        $stopwatch.Stop()
        $parsed = $null
        if (-not [string]::IsNullOrWhiteSpace($bodyText)) {
            try {
                $parsed = $bodyText | ConvertFrom-Json -ErrorAction Stop
            } catch {
                $parsed = $null
            }
        }

        return [pscustomobject]@{
            status_code = [int]$response.StatusCode
            body        = $bodyText
            parsed      = $parsed
            latency_us  = [math]::Round($stopwatch.Elapsed.TotalMilliseconds * 1000.0, 2)
            request_id  = $requestId
            method      = $Method.ToUpperInvariant()
            path        = $Path
            subject     = $Subject
            role        = $Role
        }
    } catch {
        $stopwatch.Stop()
        return [pscustomobject]@{
            status_code = 0
            body        = $_.Exception.Message
            parsed      = $null
            latency_us  = [math]::Round($stopwatch.Elapsed.TotalMilliseconds * 1000.0, 2)
            request_id  = $requestId
            method      = $Method.ToUpperInvariant()
            path        = $Path
            subject     = $Subject
            role        = $Role
            error       = $_.Exception.Message
        }
    } finally {
        if ($jsonContent) {
            $jsonContent.Content.Dispose()
        }
        $request.Dispose()
    }
}

function New-RequestWaveEntry {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$BaseUrl,
        [string]$Path,
        [string]$Method,
        [string]$Subject,
        [string]$Role,
        [string]$Secret,
        [string]$JsonBody,
        [hashtable]$Metadata
    )

    $requestId = [guid]::NewGuid().ToString("N")
    $jsonContent = New-JsonContent -Json $JsonBody
    $headers = New-AuthHeaderMap -Method $Method -Path $Path -Subject $Subject -Role $Role -Secret $Secret -BodyBytes $jsonContent.Bytes -RequestId $requestId
    $uri = [Uri]::new(("{0}{1}" -f $BaseUrl.TrimEnd('/'), $Path))
    $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::new($Method.ToUpperInvariant()), $uri)
    $request.Content = $jsonContent.Content
    $request.Headers.TryAddWithoutValidation("Accept", "application/json") | Out-Null
    foreach ($key in $headers.Keys) {
        $request.Headers.TryAddWithoutValidation($key, [string]$headers[$key]) | Out-Null
    }

    $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $task = $Client.SendAsync($request)

    return [pscustomobject]@{
        task       = $task
        stopwatch  = $stopwatch
        request    = $request
        content    = $jsonContent.Content
        request_id = $requestId
        subject    = $Subject
        role       = $Role
        path       = $Path
        metadata   = $Metadata
    }
}

function Complete-RequestWaveEntry {
    param(
        [object]$Entry
    )

    try {
        $response = $Entry.task.GetAwaiter().GetResult()
        $bodyText = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
        $Entry.stopwatch.Stop()
        $parsed = $null
        if (-not [string]::IsNullOrWhiteSpace($bodyText)) {
            try {
                $parsed = $bodyText | ConvertFrom-Json -ErrorAction Stop
            } catch {
                $parsed = $null
            }
        }

        return [pscustomobject]@{
            status_code = [int]$response.StatusCode
            body        = $bodyText
            parsed      = $parsed
            latency_us  = [math]::Round($Entry.stopwatch.Elapsed.TotalMilliseconds * 1000.0, 2)
            request_id  = $Entry.request_id
            subject     = $Entry.subject
            role        = $Entry.role
            path        = $Entry.path
            metadata    = $Entry.metadata
        }
    } catch {
        $Entry.stopwatch.Stop()
        return [pscustomobject]@{
            status_code = 0
            body        = $_.Exception.Message
            parsed      = $null
            latency_us  = [math]::Round($Entry.stopwatch.Elapsed.TotalMilliseconds * 1000.0, 2)
            request_id  = $Entry.request_id
            subject     = $Entry.subject
            role        = $Entry.role
            path        = $Entry.path
            metadata    = $Entry.metadata
            error       = $_.Exception.Message
        }
    } finally {
        $Entry.content.Dispose()
        $Entry.request.Dispose()
    }
}

function Get-NumericPercentile {
    param(
        [double[]]$Values,
        [double]$Percentile
    )

    if (-not $Values -or $Values.Count -eq 0) {
        return 0
    }

    $sorted = $Values | Sort-Object
    $index = [Math]::Ceiling(($Percentile / 100.0) * $sorted.Count) - 1
    if ($index -lt 0) {
        $index = 0
    }
    if ($index -ge $sorted.Count) {
        $index = $sorted.Count - 1
    }
    return [math]::Round([double]$sorted[$index], 2)
}

function Get-NumericSummary {
    param(
        [double[]]$Values
    )

    if (-not $Values -or $Values.Count -eq 0) {
        return [ordered]@{
            count = 0
            min   = 0
            p50   = 0
            p95   = 0
            p99   = 0
            p999  = 0
            max   = 0
            avg   = 0
        }
    }

    return [ordered]@{
        count = $Values.Count
        min   = [math]::Round(($Values | Measure-Object -Minimum).Minimum, 2)
        p50   = Get-NumericPercentile -Values $Values -Percentile 50
        p95   = Get-NumericPercentile -Values $Values -Percentile 95
        p99   = Get-NumericPercentile -Values $Values -Percentile 99
        p999  = Get-NumericPercentile -Values $Values -Percentile 99.9
        max   = [math]::Round(($Values | Measure-Object -Maximum).Maximum, 2)
        avg   = [math]::Round(($Values | Measure-Object -Average).Average, 2)
    }
}

function Get-StageValue {
    param(
        [object]$Parsed,
        [string]$Name
    )

    if ($null -eq $Parsed) {
        return 0
    }

    if ($Parsed.PSObject.Properties.Name -contains "granular_timing") {
        $timing = $Parsed.granular_timing
        if ($timing -and $timing.PSObject.Properties.Name -contains $Name) {
            return [double]$timing.$Name
        }
    }

    if ($Parsed.PSObject.Properties.Name -contains $Name) {
        return [double]$Parsed.$Name
    }

    if ($Parsed.PSObject.Properties.Name -contains "timings") {
        $timing = $Parsed.timings
        if ($timing -and $timing.PSObject.Properties.Name -contains $Name) {
            return [double]$timing.$Name
        }
    }

    return 0
}

function ConvertTo-PlainOrdered {
    param(
        [object]$InputObject
    )

    if ($null -eq $InputObject) {
        return $null
    }

    if ($InputObject -is [string] -or
        $InputObject -is [char] -or
        $InputObject -is [bool] -or
        $InputObject -is [byte] -or
        $InputObject -is [int16] -or
        $InputObject -is [int32] -or
        $InputObject -is [int64] -or
        $InputObject -is [uint16] -or
        $InputObject -is [uint32] -or
        $InputObject -is [uint64] -or
        $InputObject -is [single] -or
        $InputObject -is [double] -or
        $InputObject -is [decimal] -or
        $InputObject -is [datetime] -or
        $InputObject -is [guid]) {
        return $InputObject
    }

    if ($InputObject -is [System.Collections.IDictionary]) {
        $copy = [ordered]@{}
        foreach ($key in $InputObject.Keys) {
            $copy[[string]$key] = ConvertTo-PlainOrdered -InputObject $InputObject[$key]
        }
        return $copy
    }

    if ($InputObject -is [System.Collections.IEnumerable] -and $InputObject -isnot [string]) {
        $items = @()
        foreach ($item in $InputObject) {
            $items += ,(ConvertTo-PlainOrdered -InputObject $item)
        }
        return $items
    }

    if ($InputObject.PSObject) {
        $properties = @($InputObject.PSObject.Properties)
        if ($properties.Count -le 0) {
            return $InputObject
        }
        $copy = [ordered]@{}
        foreach ($prop in $properties) {
            $copy[$prop.Name] = ConvertTo-PlainOrdered -InputObject $prop.Value
        }
        return $copy
    }

    return $InputObject
}

function New-HttpResultPathSummary {
    param(
        [object[]]$Results
    )

    $clientLatencies = @()
    $queueLatencies = @()
    $riskLatencies = @()
    $matchCoreLatencies = @()
    $settlementLatencies = @()
    $postMatchLatencies = @()
    $byMarket = [ordered]@{}
    $successCount = 0
    $errorCount = 0
    $fillCount = 0
    $statusBreakdown = [ordered]@{}

    foreach ($item in @($Results)) {
        if ($null -eq $item) {
            continue
        }

        $clientLatencies += [double]$item.latency_us
        $queueLatencies += [double](Get-StageValue -Parsed $item.parsed -Name "queue_wait_us")
        $riskLatencies += [double](Get-StageValue -Parsed $item.parsed -Name "risk_us")
        $matchCoreLatencies += [double](Get-StageValue -Parsed $item.parsed -Name "matching_core_us")
        $settlementLatencies += [double](Get-StageValue -Parsed $item.parsed -Name "settlement_persist_us")
        $postMatchLatencies += [double](Get-StageValue -Parsed $item.parsed -Name "post_match_us")

        $market = ""
        if ($item.PSObject.Properties.Name -contains "metadata" -and $item.metadata) {
            if ($item.metadata -is [System.Collections.IDictionary]) {
                if ($item.metadata.Contains("market")) {
                    $market = [string]$item.metadata["market"]
                }
            } elseif ($item.metadata.PSObject.Properties.Name -contains "market") {
                $market = [string]$item.metadata.market
            }
        }
        if (-not [string]::IsNullOrWhiteSpace($market)) {
            if (-not $byMarket.Contains($market)) {
                $byMarket[$market] = New-Object System.Collections.Generic.List[double]
            }
            $byMarket[$market].Add([double]$item.latency_us)
        }

        if ($item.status_code -ge 200 -and $item.status_code -lt 300) {
            $successCount++
        } else {
            $errorCount++
        }
        $statusKey = [string]$item.status_code
        if (-not $statusBreakdown.Contains($statusKey)) {
            $statusBreakdown[$statusKey] = 0
        }
        $statusBreakdown[$statusKey]++

        if ($item.parsed -and $item.parsed.PSObject.Properties.Name -contains "fills") {
            try {
                $fillCount += [int]$item.parsed.fills
            } catch {
            }
        }
    }

    $marketSummary = [ordered]@{}
    foreach ($key in $byMarket.Keys) {
        $marketSummary[$key] = Get-NumericSummary -Values $byMarket[$key].ToArray()
    }

    return [ordered]@{
        total_requests         = @($Results).Count
        success_count          = $successCount
        error_count            = $errorCount
        fills_reported         = $fillCount
        status_breakdown       = $statusBreakdown
        client_latency_us      = Get-NumericSummary -Values $clientLatencies
        queue_wait_us          = Get-NumericSummary -Values $queueLatencies
        risk_us                = Get-NumericSummary -Values $riskLatencies
        matching_core_us       = Get-NumericSummary -Values $matchCoreLatencies
        settlement_persist_us  = Get-NumericSummary -Values $settlementLatencies
        post_match_us          = Get-NumericSummary -Values $postMatchLatencies
        per_market             = $marketSummary
    }
}

function Get-StatusCount {
    param(
        [object]$StatusBreakdown,
        [string]$StatusCode
    )

    if ($null -eq $StatusBreakdown) {
        return 0
    }
    if ($StatusBreakdown -is [System.Collections.IDictionary]) {
        if ($StatusBreakdown.Contains($StatusCode)) {
            return [int]$StatusBreakdown[$StatusCode]
        }
        return 0
    }
    if ($StatusBreakdown.PSObject.Properties.Name -contains $StatusCode) {
        return [int]$StatusBreakdown.$StatusCode
    }
    return 0
}

function Get-Http4xxCount {
    param(
        [object]$StatusBreakdown
    )

    $count = 0
    if ($null -eq $StatusBreakdown) {
        return 0
    }

    if ($StatusBreakdown -is [System.Collections.IDictionary]) {
        foreach ($key in $StatusBreakdown.Keys) {
            $code = 0
            if ([int]::TryParse([string]$key, [ref]$code) -and $code -ge 400 -and $code -lt 500) {
                $count += [int]$StatusBreakdown[$key]
            }
        }
        return $count
    }

    foreach ($prop in @($StatusBreakdown.PSObject.Properties)) {
        $code = 0
        if ([int]::TryParse([string]$prop.Name, [ref]$code) -and $code -ge 400 -and $code -lt 500) {
            $count += [int]$prop.Value
        }
    }
    return $count
}

function Get-SuccessRate {
    param(
        [int]$SuccessCount,
        [int]$TotalRequests
    )

    if ($TotalRequests -le 0) {
        return 0
    }
    return [math]::Round((100.0 * $SuccessCount) / $TotalRequests, 2)
}

function Get-ServerMetricsSummary {
    param(
        [object]$MetricsJson
    )

    $metricsParsed = if ($MetricsJson) { $MetricsJson.parsed } else { $null }
    return [ordered]@{
        orders_received                 = if ($metricsParsed) { [int64]$metricsParsed.orders_received } else { 0 }
        orders_filled                   = if ($metricsParsed) { [int64]$metricsParsed.orders_filled } else { 0 }
        ws_messages_sent                = if ($metricsParsed) { [int64]$metricsParsed.ws_messages_sent } else { 0 }
        submit_order_ip_rate_limited    = if ($metricsParsed -and $metricsParsed.PSObject.Properties.Name -contains "submit_order_ip_rate_limited") { [int64]$metricsParsed.submit_order_ip_rate_limited } else { 0 }
        submit_order_user_rate_limited  = if ($metricsParsed -and $metricsParsed.PSObject.Properties.Name -contains "submit_order_user_rate_limited") { [int64]$metricsParsed.submit_order_user_rate_limited } else { 0 }
        submit_order_engine_rate_limited = if ($metricsParsed -and $metricsParsed.PSObject.Properties.Name -contains "submit_order_engine_rate_limited") { [int64]$metricsParsed.submit_order_engine_rate_limited } else { 0 }
        match_e2e_p99_us                = if ($metricsParsed) { [double]$metricsParsed.latency.match_e2e_us.p99_us } else { 0 }
        http_request_p99_us             = if ($metricsParsed) { [double]$metricsParsed.latency.http_request_us.p99_us } else { 0 }
        queue_wait_p99_us               = if ($metricsParsed) { [double]$metricsParsed.latency.queue_wait_us.p99_us } else { 0 }
        risk_p99_us                     = if ($metricsParsed) { [double]$metricsParsed.latency.granular.risk_us.p99_us } else { 0 }
        matching_core_p99_us            = if ($metricsParsed) { [double]$metricsParsed.latency.granular.matching_core_us.p99_us } else { 0 }
        settlement_p99_us               = if ($metricsParsed) { [double]$metricsParsed.latency.granular.settlement_persist_us.p99_us } else { 0 }
        post_match_p99_us               = if ($metricsParsed) { [double]$metricsParsed.latency.granular.post_match_us.p99_us } else { 0 }
    }
}

function Resolve-HttpBenchClient {
    param(
        [ValidateSet("auto", "powershell", "go")]
        [string]$RequestedClient = "auto",
        [string]$WorkspaceRoot
    )

    if ($RequestedClient -eq "powershell") {
        return "powershell"
    }

    $goCommand = Get-Command go -ErrorAction SilentlyContinue
    $goClientPath = Join-Path $WorkspaceRoot "benchmark/cmd/exchange_http_bench/main.go"
    if ($RequestedClient -eq "go") {
        if (-not $goCommand) {
            throw "Go benchmark client requested, but 'go' is not available in PATH."
        }
        if (-not (Test-Path $goClientPath)) {
            throw "Go benchmark client source not found at $goClientPath"
        }
        return "go"
    }

    if ($goCommand -and (Test-Path $goClientPath)) {
        return "go"
    }
    return "powershell"
}

function Invoke-GoHttpBenchmark {
    param(
        [string]$WorkspaceRoot,
        [string]$BaseUrl,
        [string]$Secret,
        [string]$Market,
        [string[]]$BuyerUsers,
        [string[]]$SellerUsers,
        [int]$PairCount,
        [int]$PairConcurrency,
        [long]$BasePrice = 50000,
        [long]$Amount = 1,
        [int]$RateLimitPerSecond = 48,
        [string]$Prefix = "go-http",
        [int]$TimeoutSeconds = 180,
        [bool]$DisableKeepAlives = $true,
        [long]$InitialCash = 100000000,
        [long]$InitialPosition = 20000,
        [long]$CashThresholdBps = 30000,
        [long]$CashTargetBps = 240000,
        [long]$PosThresholdUnits = 4,
        [long]$PosTargetUnits = 32,
        [int]$RateLimitRetryMax = 2,
        [int]$RateLimitBackoffMs = 150,
        [int]$RequestStaggerMs = 5
    )

    $arguments = @(
        "run",
        "./benchmark/cmd/exchange_http_bench",
        "--base-url", $BaseUrl,
        "--secret", $Secret,
        "--market", $Market,
        "--buyers", ($BuyerUsers -join ","),
        "--sellers", ($SellerUsers -join ","),
        "--pair-count", [string]$PairCount,
        "--pair-concurrency", [string]$PairConcurrency,
        "--base-price", [string]$BasePrice,
        "--amount", [string]$Amount,
        "--rate-limit-per-second", [string]$RateLimitPerSecond,
        "--prefix", $Prefix,
        "--disable-keep-alives=$($DisableKeepAlives.ToString().ToLowerInvariant())",
        "--initial-cash", [string]$InitialCash,
        "--initial-position", [string]$InitialPosition,
        "--cash-threshold-bps", [string]$CashThresholdBps,
        "--cash-target-bps", [string]$CashTargetBps,
        "--position-threshold-units", [string]$PosThresholdUnits,
        "--position-target-units", [string]$PosTargetUnits,
        "--rate-limit-retry-max", [string]$RateLimitRetryMax,
        "--rate-limit-backoff-ms", [string]$RateLimitBackoffMs,
        "--request-stagger-ms", [string]$RequestStaggerMs
    )

    $stdoutPath = Join-Path ([System.IO.Path]::GetTempPath()) ("exchange-http-bench-{0}.stdout.log" -f ([guid]::NewGuid().ToString("N")))
    $stderrPath = Join-Path ([System.IO.Path]::GetTempPath()) ("exchange-http-bench-{0}.stderr.log" -f ([guid]::NewGuid().ToString("N")))
    try {
        $proc = Start-Process -FilePath "go" -ArgumentList $arguments -WorkingDirectory $WorkspaceRoot -RedirectStandardOutput $stdoutPath -RedirectStandardError $stderrPath -PassThru
        if (-not $proc.WaitForExit($TimeoutSeconds * 1000)) {
            try {
                $proc.Kill($true)
            } catch {
            }
            $stdout = if (Test-Path $stdoutPath) { Get-Content -Path $stdoutPath -Raw } else { "" }
            $stderr = if (Test-Path $stderrPath) { Get-Content -Path $stderrPath -Raw } else { "" }
            throw "go benchmark timed out after $TimeoutSeconds seconds`nstdout:`n$stdout`nstderr:`n$stderr"
        }
        $proc.WaitForExit()
        $stdout = if (Test-Path $stdoutPath) { Get-Content -Path $stdoutPath -Raw } else { "" }
        $stderr = if (Test-Path $stderrPath) { Get-Content -Path $stderrPath -Raw } else { "" }
    } finally {
        Remove-Item -Path $stdoutPath, $stderrPath -Force -ErrorAction SilentlyContinue
    }

    $exitCode = 0
    try {
        if ($null -ne $proc.ExitCode) {
            $exitCode = [int]$proc.ExitCode
        }
    } catch {
        $exitCode = 0
    }

    if ($exitCode -ne 0) {
        throw "go benchmark failed with exit code $exitCode`nstdout:`n$stdout`nstderr:`n$stderr"
    }
    if ([string]::IsNullOrWhiteSpace($stdout)) {
        throw "go benchmark returned empty stdout"
    }

    try {
        $parsed = $stdout | ConvertFrom-Json -ErrorAction Stop
    } catch {
        throw "failed to parse go benchmark JSON: $($_.Exception.Message)`nstdout:`n$stdout`nstderr:`n$stderr"
    }

    return ConvertTo-PlainOrdered -InputObject $parsed
}

function New-BackendRunLayout {
    param(
        [string]$Root,
        [string]$RunId
    )

    $baseDir = Join-Path $Root $RunId
    $dataDir = Join-Path $baseDir "data"
    $logDir = Join-Path $baseDir "logs"
    $reportDir = Join-Path $baseDir "reports"
    foreach ($path in @($baseDir, $dataDir, $logDir, $reportDir)) {
        New-Item -ItemType Directory -Path $path -Force | Out-Null
    }

    return [pscustomobject]@{
        base_dir    = $baseDir
        data_dir    = $dataDir
        log_dir     = $logDir
        report_dir  = $reportDir
        config_path = Join-Path $baseDir "exchange.generated.toml"
        stdout_log  = Join-Path $logDir "api.stdout.log"
        stderr_log  = Join-Path $logDir "api.stderr.log"
        json_report = Join-Path $reportDir "backend_resilience_report.json"
        md_report   = Join-Path $reportDir "backend_resilience_report.md"
        csv_report  = Join-Path $reportDir "backend_resilience_scale_ladder_summary.csv"
    }
}

function Write-BackendExchangeConfig {
    param(
        [object]$Layout,
        [int]$Port,
        [int]$OrderbookSnapshotIntervalMs = 100,
        [int]$WalRotationMaxEntries = 100000,
        [int]$WalGroupCommitSize = 64,
        [int]$WalSnapshotIntervalCommands = 128
    )

    $data = $Layout.data_dir.Replace("\", "/")
    $content = @"
[server]
bind_host = "127.0.0.1"
bind_port = $Port
log_level = "warn"
max_body_size_bytes = 16384
request_timeout_secs = 30

[wal]
data_dir = "$data"
rotation_max_entries = $WalRotationMaxEntries
group_commit_size = $WalGroupCommitSize
snapshot_interval_commands = $WalSnapshotIntervalCommands
ledger = "$data/ledger.wal.jsonl"
sequencer = "$data/sequencer.wal.jsonl"
matching_snapshot = "$data/matching.snapshot.jsonl"
trade_journal = "$data/trade_journal.wal.jsonl"
trade_settlement = "$data/trade_settlement.wal.jsonl"
instruments_registry = "$data/instruments.registry.jsonl"
funding_rates = "$data/funding_rates.jsonl"
risk_automation_audit = "$data/risk_automation.audit.jsonl"
liquidation_queue = "$data/liquidation.queue.jsonl"
liquidation_auction = "$data/liquidation.auction.jsonl"
adl_governance = "$data/adl.governance.jsonl"
liquidation_policy = "$data/liquidation.policy.jsonl"
index_price = "$data/index.price.jsonl"
index_source_policy = "$data/index.source.policy.jsonl"
position_cost_state = "$data/position.cost.state.jsonl"
position_cost_events = "$data/position.cost.events.jsonl"
governance_actions = "$data/governance.actions.jsonl"
withdrawals = "$data/withdrawals.wal.jsonl"
fee_tiers = "$data/fee_tiers.jsonl"
transfers = "$data/transfers.wal.jsonl"
stop_orders = "$data/stop_orders.wal.jsonl"
address_whitelist = "$data/address_whitelist.wal.jsonl"

[risk]
automation_enabled = false
liquidation_interval_secs = 30
funding_interval_secs = 60
liquidation_worker_interval_secs = 5
liquidation_auction_window_secs = 15
liquidator_user_id = "system-liquidator"
maintenance_margin_bps = 1000
liquidation_penalty_bps = 500
position_cost_resync_interval_ms = 60000

[websocket]
orderbook_snapshot_interval_ms = $OrderbookSnapshotIntervalMs
max_connections = 1024

[cors]
allowed_origins = ["http://127.0.0.1:5173", "http://localhost:5173"]
"@

    Set-Content -Path $Layout.config_path -Value $content -Encoding UTF8
    return $Layout.config_path
}

function Get-ApiBinaryPath {
    param(
        [string]$RepoRoot,
        [ValidateSet("debug", "release")]
        [string]$Profile = "release",
        [string]$CargoTarget = ""
    )

    $isWindows = $env:OS -eq "Windows_NT"
    $name = if ($isWindows) { "api.exe" } else { "api" }
    $relative = if ([string]::IsNullOrWhiteSpace($CargoTarget)) {
        "target/{0}/{1}" -f $Profile, $name
    } else {
        "target/{0}/{1}/{2}" -f $CargoTarget, $Profile, $name
    }
    return Join-Path $RepoRoot $relative
}

function Find-ExistingApiBinary {
    param(
        [string]$RepoRoot,
        [ValidateSet("debug", "release")]
        [string]$Profile = "release"
    )

    $isWindows = $env:OS -eq "Windows_NT"
    $binaryName = if ($isWindows) { "api.exe" } else { "api" }
    $candidates = @(
        [pscustomobject]@{ path = (Join-Path $RepoRoot ("target/x86_64-pc-windows-msvc/{0}/{1}" -f $Profile, $binaryName)); cargo_target = "x86_64-pc-windows-msvc"; rank = 1 },
        [pscustomobject]@{ path = (Join-Path $RepoRoot ("target/x86_64-pc-windows-gnu/{0}/{1}" -f $Profile, $binaryName)); cargo_target = "x86_64-pc-windows-gnu"; rank = 2 },
        [pscustomobject]@{ path = (Join-Path $RepoRoot ("target/{0}/{1}" -f $Profile, $binaryName)); cargo_target = ""; rank = 3 },
        [pscustomobject]@{ path = (Join-Path $RepoRoot ("target/x86_64-pc-windows-msvc/debug/{0}" -f $binaryName)); cargo_target = "x86_64-pc-windows-msvc"; rank = 4 },
        [pscustomobject]@{ path = (Join-Path $RepoRoot ("target/x86_64-pc-windows-gnu/debug/{0}" -f $binaryName)); cargo_target = "x86_64-pc-windows-gnu"; rank = 5 },
        [pscustomobject]@{ path = (Join-Path $RepoRoot ("target/debug/{0}" -f $binaryName)); cargo_target = ""; rank = 6 }
    )

    $existing = foreach ($candidate in $candidates) {
        if (Test-Path $candidate.path) {
            $item = Get-Item $candidate.path
            [pscustomobject]@{
                path         = $candidate.path
                cargo_target = $candidate.cargo_target
                rank         = $candidate.rank
                last_write   = $item.LastWriteTimeUtc
                size         = $item.Length
            }
        }
    }

    if (-not $existing) {
        return $null
    }

    return $existing |
        Sort-Object @{ Expression = "rank"; Ascending = $true }, @{ Expression = "last_write"; Ascending = $false } |
        Select-Object -First 1
}

function Invoke-CargoCommand {
    param(
        [string]$RepoRoot,
        [string[]]$Arguments
    )

    $psi = [System.Diagnostics.ProcessStartInfo]::new()
    $psi.FileName = "cargo"
    $psi.WorkingDirectory = $RepoRoot
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.Arguments = [string]::Join(" ", ($Arguments | ForEach-Object {
        if ($_ -match "\s") {
            '"{0}"' -f $_
        } else {
            $_
        }
    }))

    $proc = [System.Diagnostics.Process]::new()
    $proc.StartInfo = $psi
    $null = $proc.Start()
    $stdout = $proc.StandardOutput.ReadToEnd()
    $stderr = $proc.StandardError.ReadToEnd()
    $proc.WaitForExit()

    return [pscustomobject]@{
        exit_code = $proc.ExitCode
        stdout    = $stdout
        stderr    = $stderr
    }
}

function Start-ExchangeApi {
    param(
        [string]$RepoRoot,
        [object]$Layout,
        [ValidateSet("debug", "release")]
        [string]$Profile = "release",
        [string]$CargoTarget = "",
        [string]$BinaryPath = "",
        [switch]$BuildBinary
    )

    if ($BuildBinary) {
        $buildArgs = @("build", "-p", "api")
        if ($Profile -eq "release") {
            $buildArgs += "--release"
        }
        if (-not [string]::IsNullOrWhiteSpace($CargoTarget)) {
            $buildArgs += @("--target", $CargoTarget)
        }
        $buildResult = Invoke-CargoCommand -RepoRoot $RepoRoot -Arguments $buildArgs
        if ($buildResult.exit_code -ne 0) {
            throw "cargo build failed: $($buildResult.stderr)"
        }
    }

    $binary = if (-not [string]::IsNullOrWhiteSpace($BinaryPath)) {
        $BinaryPath
    } else {
        Get-ApiBinaryPath -RepoRoot $RepoRoot -Profile $Profile -CargoTarget $CargoTarget
    }
    if (-not (Test-Path $binary)) {
        throw "API binary not found at $binary"
    }

    $previousConfig = $env:EXCHANGE_CONFIG_PATH
    $previousLog = $env:RUST_LOG
    try {
        $env:EXCHANGE_CONFIG_PATH = $Layout.config_path
        $env:RUST_LOG = "warn"
        $process = Start-Process -FilePath $binary `
            -WorkingDirectory $RepoRoot `
            -RedirectStandardOutput $Layout.stdout_log `
            -RedirectStandardError $Layout.stderr_log `
            -PassThru
    } finally {
        $env:EXCHANGE_CONFIG_PATH = $previousConfig
        $env:RUST_LOG = $previousLog
    }

    return $process
}

function Stop-ExchangeApi {
    param(
        [System.Diagnostics.Process]$Process
    )

    if ($null -eq $Process) {
        return
    }

    try {
        if (-not $Process.HasExited) {
            Stop-Process -Id $Process.Id -Force -ErrorAction Stop
            $Process.WaitForExit()
        }
    } catch {
    }
}

function Wait-ExchangeHealthy {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$BaseUrl,
        [int]$TimeoutSeconds = 45
    )

    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        try {
            $health = Invoke-ApiJsonRequest -Client $Client -BaseUrl $BaseUrl -Method "GET" -Path "/ready" -Secret "unused" -Subject "health" -Role "user"
            if ($health.status_code -eq 200) {
                return $health
            }
        } catch {
        }
        Start-Sleep -Milliseconds 500
    }

    throw "service did not become healthy within ${TimeoutSeconds}s"
}

function Get-ApiMetricsJson {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$BaseUrl,
        [string]$Secret
    )

    return Invoke-ApiJsonRequest -Client $Client -BaseUrl $BaseUrl -Method "GET" -Path "/metrics" -Secret $Secret -Subject "observer" -Role "admin"
}

function Get-ApiPrometheusText {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$BaseUrl,
        [string]$Secret
    )

    return Invoke-ApiJsonRequest -Client $Client -BaseUrl $BaseUrl -Method "GET" -Path "/metrics/prometheus" -Secret $Secret -Subject "observer" -Role "admin"
}

function Get-WalSnapshot {
    param(
        [object]$Layout
    )

    $files = @(
        "ledger.wal.jsonl",
        "sequencer.wal.jsonl",
        "matching.snapshot.jsonl",
        "trade_journal.wal.jsonl",
        "trade_settlement.wal.jsonl"
    )
    $snapshot = [ordered]@{}
    foreach ($file in $files) {
        $path = Join-Path $Layout.data_dir $file
        if (Test-Path $path) {
            $item = Get-Item $path
            $lineCount = 0
            try {
                $lineCount = (Get-Content -Path $path).Count
            } catch {
                $lineCount = 0
            }
            $snapshot[$file] = [ordered]@{
                exists = $true
                bytes  = [int64]$item.Length
                lines  = [int]$lineCount
            }
        } else {
            $snapshot[$file] = [ordered]@{
                exists = $false
                bytes  = 0
                lines  = 0
            }
        }
    }
    return $snapshot
}

function Get-ProcessMemorySample {
    param(
        [System.Diagnostics.Process]$Process
    )

    if ($null -eq $Process -or $Process.HasExited) {
        return [ordered]@{
            available = $false
            pid       = if ($Process) { $Process.Id } else { 0 }
        }
    }

    $proc = Get-Process -Id $Process.Id -ErrorAction Stop
    return [ordered]@{
        available          = $true
        pid                = $proc.Id
        working_set_bytes  = [int64]$proc.WorkingSet64
        private_bytes      = [int64]$proc.PrivateMemorySize64
        virtual_bytes      = [int64]$proc.VirtualMemorySize64
        handles            = if ($proc.PSObject.Properties.Name -contains "HandleCount") { [int]$proc.HandleCount } else { 0 }
        threads            = if ($proc.PSObject.Properties.Name -contains "Threads") { [int]$proc.Threads.Count } else { 0 }
    }
}

function Get-MetricValueFromPrometheus {
    param(
        [string]$PrometheusText,
        [string]$MetricName
    )

    $pattern = "(?m)^" + [regex]::Escape($MetricName) + "\s+([0-9]+(?:\.[0-9]+)?)$"
    $match = [regex]::Match($PrometheusText, $pattern)
    if ($match.Success) {
        return [double]$match.Groups[1].Value
    }
    return 0
}

function Get-PreferredMarketSet {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$BaseUrl,
        [string]$Secret
    )

    $marketsResp = Invoke-ApiJsonRequest -Client $Client -BaseUrl $BaseUrl -Method "GET" -Path "/markets" -Secret $Secret -Subject "observer" -Role "admin"
    if ($marketsResp.status_code -ne 200 -or $null -eq $marketsResp.parsed) {
        throw "failed to discover markets from /markets"
    }

    $available = @()
    foreach ($entry in $marketsResp.parsed) {
        if ($entry.market_id) {
            $available += [string]$entry.market_id
        } elseif ($entry.id) {
            $available += [string]$entry.id
        }
    }

    $preferred = @("btc-usdt", "margin:btc-usdt", "perp:btc-usdt")
    $selected = @()
    foreach ($market in $preferred) {
        if ($available -contains $market) {
            $selected += $market
        }
    }

    if ($selected.Count -lt 3) {
        foreach ($market in $available) {
            if ($selected -notcontains $market) {
                $selected += $market
            }
            if ($selected.Count -ge 3) {
                break
            }
        }
    }

    if ($selected.Count -eq 0) {
        throw "no markets available for resilience suite"
    }

    return $selected
}

function New-OrderBody {
    param(
        [string]$MarketId,
        [string]$Side,
        [long]$Price,
        [long]$Amount,
        [string]$ClientOrderId,
        [string]$OrderType = "limit",
        [int]$Outcome = 0,
        [string]$TimeInForce = "gtc"
    )

    return [ordered]@{
        market_id       = $MarketId
        side            = $Side
        order_type      = $OrderType
        price           = $Price
        amount          = $Amount
        outcome         = $Outcome
        time_in_force   = $TimeInForce
        client_order_id = $ClientOrderId
    }
}

function Seed-ExchangeUsers {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$BaseUrl,
        [string]$Secret,
        [string[]]$Markets,
        [string[]]$BuyerUsers,
        [string[]]$SellerUsers,
        [string]$AdminSubject = "admin",
        [long]$CashAmount = 500000000,
        [long]$PositionAmount = 200000,
        [int]$PauseEvery = 4,
        [int]$PauseMs = 100
    )

    $opCount = 0
    foreach ($user in $BuyerUsers) {
        $depositBody = [ordered]@{
            user_id = $user
            amount  = $CashAmount
            op_id   = "seed-cash-$user"
        }
        [void](Invoke-ApiJsonRequest -Client $Client -BaseUrl $BaseUrl -Method "POST" -Path "/deposit" -Secret $Secret -Subject $AdminSubject -Role "admin" -Body $depositBody)
        $opCount++
        if ($PauseEvery -gt 0 -and ($opCount % $PauseEvery) -eq 0) {
            Start-Sleep -Milliseconds $PauseMs
        }
    }

    foreach ($user in $SellerUsers) {
        $depositBody = [ordered]@{
            user_id = $user
            amount  = $CashAmount
            op_id   = "seed-cash-$user"
        }
        [void](Invoke-ApiJsonRequest -Client $Client -BaseUrl $BaseUrl -Method "POST" -Path "/deposit" -Secret $Secret -Subject $AdminSubject -Role "admin" -Body $depositBody)
        $opCount++
        if ($PauseEvery -gt 0 -and ($opCount % $PauseEvery) -eq 0) {
            Start-Sleep -Milliseconds $PauseMs
        }

        foreach ($market in $Markets) {
            $positionBody = [ordered]@{
                user_id   = $user
                market_id = $market
                outcome   = 0
                amount    = $PositionAmount
                op_id     = "seed-pos-$user-$($market.Replace(':','_'))"
            }
            [void](Invoke-ApiJsonRequest -Client $Client -BaseUrl $BaseUrl -Method "POST" -Path "/position-deposit" -Secret $Secret -Subject $AdminSubject -Role "admin" -Body $positionBody)
            $opCount++
            if ($PauseEvery -gt 0 -and ($opCount % $PauseEvery) -eq 0) {
                Start-Sleep -Milliseconds $PauseMs
            }
        }
    }
}

function TopUp-ExchangeUsers {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$BaseUrl,
        [string]$Secret,
        [string]$Market,
        [string[]]$BuyerUsers,
        [string[]]$SellerUsers,
        [string]$Prefix = "topup",
        [string]$AdminSubject = "admin",
        [long]$CashAmount = 100000000,
        [long]$PositionAmount = 20000,
        [int]$PauseEvery = 3,
        [int]$PauseMs = 120
    )

    $opCount = 0
    foreach ($user in $BuyerUsers) {
        $depositBody = [ordered]@{
            user_id = $user
            amount  = $CashAmount
            op_id   = "{0}-cash-{1}-{2}" -f $Prefix, $user, ([guid]::NewGuid().ToString("N").Substring(0, 6))
        }
        [void](Invoke-ApiJsonRequest -Client $Client -BaseUrl $BaseUrl -Method "POST" -Path "/deposit" -Secret $Secret -Subject $AdminSubject -Role "admin" -Body $depositBody)
        $opCount++
        if ($PauseEvery -gt 0 -and ($opCount % $PauseEvery) -eq 0) {
            Start-Sleep -Milliseconds $PauseMs
        }
    }

    foreach ($user in $SellerUsers) {
        $depositBody = [ordered]@{
            user_id = $user
            amount  = $CashAmount
            op_id   = "{0}-cash-{1}-{2}" -f $Prefix, $user, ([guid]::NewGuid().ToString("N").Substring(0, 6))
        }
        [void](Invoke-ApiJsonRequest -Client $Client -BaseUrl $BaseUrl -Method "POST" -Path "/deposit" -Secret $Secret -Subject $AdminSubject -Role "admin" -Body $depositBody)
        $opCount++
        if ($PauseEvery -gt 0 -and ($opCount % $PauseEvery) -eq 0) {
            Start-Sleep -Milliseconds $PauseMs
        }

        $positionBody = [ordered]@{
            user_id   = $user
            market_id = $Market
            outcome   = 0
            amount    = $PositionAmount
            op_id     = "{0}-pos-{1}-{2}" -f $Prefix, $user, ([guid]::NewGuid().ToString("N").Substring(0, 6))
        }
        [void](Invoke-ApiJsonRequest -Client $Client -BaseUrl $BaseUrl -Method "POST" -Path "/position-deposit" -Secret $Secret -Subject $AdminSubject -Role "admin" -Body $positionBody)
        $opCount++
        if ($PauseEvery -gt 0 -and ($opCount % $PauseEvery) -eq 0) {
            Start-Sleep -Milliseconds $PauseMs
        }
    }
}

function Seed-MarketDepth {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$BaseUrl,
        [string]$Secret,
        [string]$MarketId,
        [string[]]$SellerUsers,
        [long]$BasePrice,
        [int]$DepthOrders,
        [long]$AmountPerOrder = 10
    )

    for ($i = 0; $i -lt $DepthOrders; $i++) {
        $seller = $SellerUsers[$i % $SellerUsers.Count]
        $order = New-OrderBody -MarketId $MarketId -Side "sell" -Price ($BasePrice + $i) -Amount $AmountPerOrder -ClientOrderId ("seed-sell-{0}-{1}" -f $MarketId.Replace(':','_'), $i)
        [void](Invoke-ApiJsonRequest -Client $Client -BaseUrl $BaseUrl -Method "POST" -Path "/submit-order" -Secret $Secret -Subject $seller -Role "user" -Body $order)
    }
}

function Invoke-OrderBurst {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$BaseUrl,
        [string]$Secret,
        [string[]]$Markets,
        [string[]]$BuyerUsers,
        [int]$TotalRequests,
        [int]$Concurrency,
        [long]$BasePrice = 50000,
        [long]$Amount = 1
    )

    $results = New-Object System.Collections.Generic.List[object]
    $preferredWeights = @()
    foreach ($market in $Markets) {
        switch ($market) {
            "btc-usdt" { $preferredWeights += @(, $market, $market, $market, $market, $market, $market, $market) }
            default { $preferredWeights += @(, $market, $market) }
        }
    }
    if ($preferredWeights.Count -eq 0) {
        $preferredWeights = $Markets
    }

    for ($offset = 0; $offset -lt $TotalRequests; $offset += $Concurrency) {
        $waveCount = [Math]::Min($Concurrency, $TotalRequests - $offset)
        $entries = @()
        for ($i = 0; $i -lt $waveCount; $i++) {
            $globalIndex = $offset + $i
            $market = $preferredWeights[$globalIndex % $preferredWeights.Count]
            $subject = $BuyerUsers[$globalIndex % $BuyerUsers.Count]
            $priceBump = if ($market -eq "btc-usdt") { 0 } else { ($globalIndex % 5) }
            $body = New-OrderBody -MarketId $market -Side "buy" -Price ($BasePrice + $priceBump + 50) -Amount $Amount -ClientOrderId ("burst-{0}" -f $globalIndex)
            $json = $body | ConvertTo-Json -Compress -Depth 8
            $entries += New-RequestWaveEntry -Client $Client -BaseUrl $BaseUrl -Path "/submit-order" -Method "POST" -Subject $subject -Role "user" -Secret $Secret -JsonBody $json -Metadata @{
                market = $market
                index  = $globalIndex
            }
        }

        foreach ($entry in $entries) {
            $results.Add((Complete-RequestWaveEntry -Entry $entry))
        }
    }

    return $results
}

function Invoke-CrossingPairBurst {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$BaseUrl,
        [string]$Secret,
        [string]$Market,
        [string[]]$BuyerUsers,
        [string[]]$SellerUsers,
        [int]$PairCount,
        [int]$PairConcurrency = 3,
        [long]$BasePrice = 50000,
        [long]$Amount = 1,
        [int]$RateLimitPerSecond = 48,
        [string]$Prefix = "pair"
    )

    $results = New-Object System.Collections.Generic.List[object]
    $pairConcurrency = [Math]::Max(1, $PairConcurrency)
    $rateLimitPerSecond = [Math]::Max(1, $RateLimitPerSecond)

    for ($offset = 0; $offset -lt $PairCount; $offset += $pairConcurrency) {
        $wavePairs = [Math]::Min($pairConcurrency, $PairCount - $offset)

        $sellEntries = @()
        for ($i = 0; $i -lt $wavePairs; $i++) {
            $pairIndex = $offset + $i
            $seller = $SellerUsers[$pairIndex % $SellerUsers.Count]
            $price = $BasePrice + ($pairIndex % 25)
            $body = New-OrderBody -MarketId $Market -Side "sell" -Price $price -Amount $Amount -ClientOrderId ("{0}-maker-{1}-{2}" -f $Prefix, $pairIndex, ([guid]::NewGuid().ToString("N").Substring(0, 6)))
            $json = $body | ConvertTo-Json -Compress -Depth 8
            $sellEntries += New-RequestWaveEntry -Client $Client -BaseUrl $BaseUrl -Path "/submit-order" -Method "POST" -Subject $seller -Role "user" -Secret $Secret -JsonBody $json -Metadata @{
                market = $Market
                side = "sell"
                pair_index = $pairIndex
                phase = "maker"
            }
        }
        foreach ($entry in $sellEntries) {
            $results.Add((Complete-RequestWaveEntry -Entry $entry))
        }

        $buyEntries = @()
        for ($i = 0; $i -lt $wavePairs; $i++) {
            $pairIndex = $offset + $i
            $buyer = $BuyerUsers[$pairIndex % $BuyerUsers.Count]
            $price = $BasePrice + ($pairIndex % 25)
            $body = New-OrderBody -MarketId $Market -Side "buy" -Price $price -Amount $Amount -ClientOrderId ("{0}-taker-{1}-{2}" -f $Prefix, $pairIndex, ([guid]::NewGuid().ToString("N").Substring(0, 6)))
            $json = $body | ConvertTo-Json -Compress -Depth 8
            $buyEntries += New-RequestWaveEntry -Client $Client -BaseUrl $BaseUrl -Path "/submit-order" -Method "POST" -Subject $buyer -Role "user" -Secret $Secret -JsonBody $json -Metadata @{
                market = $Market
                side = "buy"
                pair_index = $pairIndex
                phase = "taker"
            }
        }
        foreach ($entry in $buyEntries) {
            $results.Add((Complete-RequestWaveEntry -Entry $entry))
        }

        $requestsThisWave = 2 * $wavePairs
        $sleepMs = [math]::Ceiling((1000.0 * $requestsThisWave) / $rateLimitPerSecond)
        if ($offset + $wavePairs -lt $PairCount -and $sleepMs -gt 0) {
            Start-Sleep -Milliseconds $sleepMs
        }
    }

    return $results
}

function Connect-ExchangeWebSocket {
    param(
        [string]$Url,
        [hashtable]$Headers = @{}
    )

    $socket = [System.Net.WebSockets.ClientWebSocket]::new()
    foreach ($key in $Headers.Keys) {
        [void]$socket.Options.SetRequestHeader($key, [string]$Headers[$key])
    }

    $cts = [System.Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(15))
    try {
        [void]$socket.ConnectAsync([Uri]$Url, $cts.Token).GetAwaiter().GetResult()
        return $socket
    } finally {
        $cts.Dispose()
    }
}

function Receive-ExchangeWebSocketMessages {
    param(
        [System.Net.WebSockets.ClientWebSocket]$Socket,
        [int]$TimeoutSeconds = 5,
        [int]$MaxMessages = 16
    )

    $messages = @()
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ($messages.Count -lt $MaxMessages -and [DateTime]::UtcNow -lt $deadline) {
        $remaining = $deadline - [DateTime]::UtcNow
        if ($remaining.TotalMilliseconds -le 0) {
            break
        }

        $buffer = New-Object byte[] 4096
        $segment = [ArraySegment[byte]]::new($buffer)
        $stream = [System.IO.MemoryStream]::new()
        $cts = [System.Threading.CancellationTokenSource]::new($remaining)

        try {
            do {
                $result = $Socket.ReceiveAsync($segment, $cts.Token).GetAwaiter().GetResult()
                if ($result.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close) {
                    return $messages
                }
                $stream.Write($buffer, 0, $result.Count)
            } while (-not $result.EndOfMessage)
        } catch [System.OperationCanceledException] {
            return $messages
        } finally {
            $cts.Dispose()
        }

        $text = [System.Text.Encoding]::UTF8.GetString($stream.ToArray())
        $stream.Dispose()
        try {
            $parsed = $text | ConvertFrom-Json -ErrorAction Stop
        } catch {
            $parsed = $null
        }
        $messages += [pscustomobject]@{
            raw    = $text
            parsed = $parsed
        }
    }

    return $messages
}

function Get-ExchangeWsMessageType {
    param(
        [object]$ParsedMessage
    )

    if ($null -eq $ParsedMessage) {
        return $null
    }

    if ($ParsedMessage.PSObject.Properties.Name -contains "type") {
        return [string]$ParsedMessage.type
    }

    if ($ParsedMessage.PSObject.Properties.Name -contains "event_type") {
        return [string]$ParsedMessage.event_type
    }

    return $null
}

function Close-ExchangeWebSocket {
    param(
        [object]$Socket
    )

    if ($null -eq $Socket) {
        return
    }

    if ($Socket -is [System.Array]) {
        foreach ($item in $Socket) {
            Close-ExchangeWebSocket -Socket $item
        }
        return
    }

    if ($Socket -isnot [System.Net.WebSockets.ClientWebSocket]) {
        return
    }

    try {
        if ($Socket.State -eq [System.Net.WebSockets.WebSocketState]::Open) {
            $cts = [System.Threading.CancellationTokenSource]::new([TimeSpan]::FromSeconds(3))
            try {
                [void]$Socket.CloseAsync([System.Net.WebSockets.WebSocketCloseStatus]::NormalClosure, "done", $cts.Token).GetAwaiter().GetResult()
            } finally {
                $cts.Dispose()
            }
        }
    } catch {
    } finally {
        $Socket.Dispose()
    }
}

function Find-LatestBackendBaselineReport {
    param(
        [string]$ArtifactsRoot,
        [string]$CurrentRunId
    )

    if ([string]::IsNullOrWhiteSpace($ArtifactsRoot) -or -not (Test-Path $ArtifactsRoot)) {
        return $null
    }

    $escapedRunId = [regex]::Escape("\$CurrentRunId\")
    $candidates = Get-ChildItem -Path $ArtifactsRoot -Recurse -Filter "backend_resilience_report.json" -File |
        Where-Object { $_.FullName -notmatch $escapedRunId } |
        Sort-Object LastWriteTimeUtc -Descending
    if (-not $candidates -or $candidates.Count -le 0) {
        return $null
    }

    return $candidates[0].FullName
}

function Convert-ScaleLadderSummaryToCsvRows {
    param(
        [object]$Report
    )

    $http = $Report["http_latency"]
    if (-not $http -or -not $http["scale_ladder_summary"]) {
        return @()
    }

    $runId = $Report["run_id"]
    $completedAt = $Report["completed_at"]
    $mode = $Report["mode"]
    $baseUrl = $Report["base_url"]
    $userBalanceMessages = if ($http["user_balance_messages"]) { $http["user_balance_messages"] } else { 0 }

    $rows = foreach ($scale in @($http["scale_ladder_summary"])) {
        [pscustomobject]@{
            run_id                     = $runId
            completed_at               = $completedAt
            mode                       = $mode
            base_url                   = $baseUrl
            client_mode                = $scale["client_mode"]
            scale_name                 = $scale["scale_name"]
            configured_requests        = $scale["configured_requests"]
            configured_concurrency     = $scale["configured_concurrency"]
            system_core_success_count  = $scale["system_core_success_count"]
            system_core_total          = $scale["system_core_total"]
            system_core_success_rate   = $scale["system_core_success_rate"]
            single_ip_success_count    = $scale["single_ip_success_count"]
            single_ip_total            = $scale["single_ip_total"]
            single_ip_success_rate     = $scale["single_ip_success_rate"]
            direct_success_p50_us      = $scale["direct_success_p50_us"]
            direct_success_p95_us      = $scale["direct_success_p95_us"]
            direct_success_p99_us      = $scale["direct_success_p99_us"]
            direct_success_p999_us     = $scale["direct_success_p999_us"]
            rescued_success_count      = $scale["rescued_success_count"]
            flow_controlled_count      = $scale["flow_controlled_count"]
            api_rate_limit_count       = $scale["api_rate_limit_count"]
            risk_reject_count          = $scale["risk_reject_count"]
            excluded_api_limits        = $scale["excluded_api_limits"]
            excluded_429               = $scale["excluded_429"]
            fills_reported             = $scale["fills_reported"]
            user_balance_messages      = $userBalanceMessages
        }
    }

    return @($rows)
}

function New-RegressionCheck {
    param(
        [string]$Scope,
        [string]$ScaleName,
        [string]$Metric,
        [string]$Severity,
        [string]$Status,
        [double]$CurrentValue,
        [Nullable[double]]$BaselineValue,
        [Nullable[double]]$ThresholdValue,
        [string]$Rule
    )

    return [ordered]@{
        scope          = $Scope
        scale_name     = $ScaleName
        metric         = $Metric
        severity       = $Severity
        status         = $Status
        current_value  = $CurrentValue
        baseline_value = $BaselineValue
        threshold      = $ThresholdValue
        rule           = $Rule
    }
}

function Compare-BackendRegression {
    param(
        [object]$CurrentReport,
        [string]$BaselineReportPath
    )

    $summary = [ordered]@{
        status               = "skipped"
        baseline_report_path = $BaselineReportPath
        baseline_run_id      = $null
        total_checks         = 0
        failed_checks        = 0
        warning_checks       = 0
        checks               = @()
    }

    $mode = [string]$CurrentReport["mode"]
    $isSmokeMode = $mode -eq "smoke"
    $p99Severity = if ($isSmokeMode) { "warn" } else { "fail" }
    $successRateSeverity = if ($isSmokeMode) { "warn" } else { "fail" }
    $p99Multiplier = if ($isSmokeMode) { 1.40 } else { 1.25 }
    $p99AbsoluteSlackUs = if ($isSmokeMode) { 2500 } else { 1500 }
    $successRateSlackPct = if ($isSmokeMode) { 5.0 } else { 2.0 }

    $currentScaleSummary = @($CurrentReport["http_latency"]["scale_ladder_summary"])
    if (-not $currentScaleSummary -or $currentScaleSummary.Count -le 0) {
        return $summary
    }

    $checks = New-Object System.Collections.Generic.List[object]
    $xlargeCurrent = $currentScaleSummary | Where-Object { $_["client_mode"] -eq "keepalive_on" -and $_["scale_name"] -eq "xlarge" } | Select-Object -First 1
    if ($xlargeCurrent) {
        $checks.Add((New-RegressionCheck -Scope "absolute_guardrail" -ScaleName "xlarge" -Metric "api_rate_limit_count" -Severity "fail" -Status $(if ([double]$xlargeCurrent["api_rate_limit_count"] -gt 0) { "fail" } else { "pass" }) -CurrentValue ([double]$xlargeCurrent["api_rate_limit_count"]) -BaselineValue $null -ThresholdValue 0 -Rule "xlarge api_rate_limit_count must stay at 0"))
        $checks.Add((New-RegressionCheck -Scope "absolute_guardrail" -ScaleName "xlarge" -Metric "direct_success_p99_us" -Severity "warn" -Status $(if ([double]$xlargeCurrent["direct_success_p99_us"] -gt 10000) { "warn" } else { "pass" }) -CurrentValue ([double]$xlargeCurrent["direct_success_p99_us"]) -BaselineValue $null -ThresholdValue 10000 -Rule "xlarge direct_success_p99_us should stay <= 10000us"))
        $checks.Add((New-RegressionCheck -Scope "absolute_guardrail" -ScaleName "xlarge" -Metric "system_core_success_rate" -Severity "warn" -Status $(if ([double]$xlargeCurrent["system_core_success_rate"] -lt 85.0) { "warn" } else { "pass" }) -CurrentValue ([double]$xlargeCurrent["system_core_success_rate"]) -BaselineValue $null -ThresholdValue 85.0 -Rule "xlarge system_core_success_rate should stay >= 85%"))
    }

    if (-not [string]::IsNullOrWhiteSpace($BaselineReportPath) -and (Test-Path $BaselineReportPath)) {
        $baselineReport = ConvertTo-PlainOrdered -InputObject ((Get-Content $BaselineReportPath -Raw) | ConvertFrom-Json)
        $summary["baseline_run_id"] = $baselineReport["run_id"]
        $baselineScaleSummary = @($baselineReport["http_latency"]["scale_ladder_summary"])
        $baselineByKey = @{}
        foreach ($item in $baselineScaleSummary) {
            $baselineByKey["$($item["client_mode"])|$($item["scale_name"])|$($item["configured_requests"])|$($item["configured_concurrency"])"] = $item
        }

        foreach ($scale in $currentScaleSummary) {
            $key = "$($scale["client_mode"])|$($scale["scale_name"])|$($scale["configured_requests"])|$($scale["configured_concurrency"])"
            if (-not $baselineByKey.ContainsKey($key)) {
                continue
            }
            $baseline = $baselineByKey[$key]

            $baselineP99 = [double]$baseline["direct_success_p99_us"]
            $currentP99 = [double]$scale["direct_success_p99_us"]
            $p99Threshold = [Math]::Max(($baselineP99 * $p99Multiplier), ($baselineP99 + $p99AbsoluteSlackUs))
            $p99Status = if ($currentP99 -gt $p99Threshold) { if ($p99Severity -eq "fail") { "fail" } else { "warn" } } else { "pass" }
            $checks.Add((New-RegressionCheck -Scope "baseline_compare" -ScaleName $scale["scale_name"] -Metric "direct_success_p99_us" -Severity $p99Severity -Status $p99Status -CurrentValue $currentP99 -BaselineValue $baselineP99 -ThresholdValue $p99Threshold -Rule "current <= max(baseline*$p99Multiplier, baseline+$p99AbsoluteSlackUs us)"))

            $baselineSuccessRate = [double]$baseline["system_core_success_rate"]
            $currentSuccessRate = [double]$scale["system_core_success_rate"]
            $successThreshold = $baselineSuccessRate - $successRateSlackPct
            $successStatus = if ($currentSuccessRate -lt $successThreshold) { if ($successRateSeverity -eq "fail") { "fail" } else { "warn" } } else { "pass" }
            $checks.Add((New-RegressionCheck -Scope "baseline_compare" -ScaleName $scale["scale_name"] -Metric "system_core_success_rate" -Severity $successRateSeverity -Status $successStatus -CurrentValue $currentSuccessRate -BaselineValue $baselineSuccessRate -ThresholdValue $successThreshold -Rule "current >= baseline-$successRateSlackPct pct"))

            $baselineRescued = [double]$baseline["rescued_success_count"]
            $currentRescued = [double]$scale["rescued_success_count"]
            $rescuedThreshold = [Math]::Max(($baselineRescued + 25), [Math]::Ceiling($baselineRescued * 1.5))
            $checks.Add((New-RegressionCheck -Scope "baseline_compare" -ScaleName $scale["scale_name"] -Metric "rescued_success_count" -Severity "warn" -Status $(if ($currentRescued -gt $rescuedThreshold) { "warn" } else { "pass" }) -CurrentValue $currentRescued -BaselineValue $baselineRescued -ThresholdValue $rescuedThreshold -Rule "current <= max(baseline+25, baseline*1.5)"))

            $baselineRiskReject = [double]$baseline["risk_reject_count"]
            $currentRiskReject = [double]$scale["risk_reject_count"]
            $riskRejectThreshold = [Math]::Max(($baselineRiskReject + 10), [Math]::Ceiling($baselineRiskReject * 1.5))
            $checks.Add((New-RegressionCheck -Scope "baseline_compare" -ScaleName $scale["scale_name"] -Metric "risk_reject_count" -Severity "warn" -Status $(if ($currentRiskReject -gt $riskRejectThreshold) { "warn" } else { "pass" }) -CurrentValue $currentRiskReject -BaselineValue $baselineRiskReject -ThresholdValue $riskRejectThreshold -Rule "current <= max(baseline+10, baseline*1.5)"))
        }
    }

    $checkArray = @($checks.ToArray())
    $summary["checks"] = $checkArray
    $summary["total_checks"] = $checkArray.Count
    $summary["failed_checks"] = @($checkArray | Where-Object { $_["status"] -eq "fail" }).Count
    $summary["warning_checks"] = @($checkArray | Where-Object { $_["status"] -eq "warn" }).Count
    if ($summary["failed_checks"] -gt 0) {
        $summary["status"] = "fail"
    } elseif ($summary["warning_checks"] -gt 0) {
        $summary["status"] = "warn"
    } elseif ($checkArray.Count -gt 0) {
        $summary["status"] = "pass"
    }

    return $summary
}

function Write-BackendReportFiles {
    param(
        [object]$Layout,
        [object]$Report
    )

    $json = $Report | ConvertTo-Json -Depth 20
    Set-Content -Path $Layout.json_report -Value $json -Encoding UTF8
    $csvRows = @(Convert-ScaleLadderSummaryToCsvRows -Report $Report)
    if ($csvRows.Count -gt 0) {
        $csvRows | Export-Csv -Path $Layout.csv_report -NoTypeInformation -Encoding UTF8
    } else {
        Set-Content -Path $Layout.csv_report -Value "" -Encoding UTF8
    }

    $http = $Report["http_latency"]
    $faults = $Report["fault_replay"]
    $ws = $Report["websocket_integrity"]
    $soak = $Report["soak"]
    $regression = $Report["regression_summary"]
    $scaleSummary = @()
    if ($http -and $http["scale_runs"]) {
        $scaleSummary += ""
        $scaleSummary += "## HTTP Scale Ladder"
        $scaleSummary += ""
        foreach ($scale in $http["scale_runs"]) {
            $scaleSummary += "- $($scale["client_mode"]) / $($scale["scale_name"]): system_core=$($scale["system_core_metric"]["count"])/$($scale["system_core_metric"]["clean_total"]) ($($scale["system_core_metric"]["success_rate_pct"])%), single_ip=$($scale["single_ip_metric"]["count"])/$($scale["single_ip_metric"]["clean_total"]) ($($scale["single_ip_metric"]["success_rate_pct"])%), direct P99/P999(us)=$($scale["primary_metric"]["client_latency_us"]["p99"]) / $($scale["primary_metric"]["client_latency_us"]["p999"]), rescued=$($scale["rescued_success_metric"]["count"]), api_rate_limits_excluded=$($scale["system_core_metric"]["excluded_api_limits"]), fills=$($scale["fills_reported"])"
        }
    }
    $clientModeSummary = @()
    if ($http -and $http["client_mode_runs"]) {
        $clientModeSummary += ""
        $clientModeSummary += "## HTTP Client Modes"
        $clientModeSummary += ""
        foreach ($mode in $http["client_mode_runs"]) {
            $clientModeSummary += "- $($mode["client_mode"]): system_core=$($mode["system_core_metric"]["count"])/$($mode["system_core_metric"]["clean_total"]) ($($mode["system_core_metric"]["success_rate_pct"])%), single_ip=$($mode["single_ip_metric"]["count"])/$($mode["single_ip_metric"]["clean_total"]) ($($mode["single_ip_metric"]["success_rate_pct"])%), rescued=$($mode["rescued_success_metric"]["count"]), flow_controlled=$($mode["flow_controlled_success_metric"]["count"]), excluded_api_limits=$($mode["system_core_metric"]["excluded_api_limits"]), excluded_429=$($mode["single_ip_metric"]["excluded_429"]), 4xx=$($mode["http_4xx_count"]), 429=$($mode["http_429_count"]), fills=$($mode["fills_reported"]), pre_submit_p50=$($mode["pre_submit_available_balance"]["p50"]), topups=$($mode["topup_count"]), topup_amount=$($mode["topup_amount"]), user_balance=$($mode["user_balance_messages"])"
        }
    }
    $errorCategorySummary = @()
    if ($http -and $http["error_categories"]) {
        $errorCategorySummary += ""
        $errorCategorySummary += "## Error Categories"
        $errorCategorySummary += ""
        foreach ($item in $http["error_categories"]) {
            $errorCategorySummary += "- $($item["category"]): count=$($item["count"]), share=$($item["share_pct"])%, P50/P95/P99(us)=$($item["client_latency_us"]["p50"]) / $($item["client_latency_us"]["p95"]) / $($item["client_latency_us"]["p99"]), trigger=$($item["trigger_hint"])"
        }
    }
    $regressionSummary = @()
    if ($regression) {
        $regressionSummary += ""
        $regressionSummary += "## Regression Checks"
        $regressionSummary += ""
        $regressionSummary += "- Status: $($regression["status"])"
        $regressionSummary += "- Baseline run: $($regression["baseline_run_id"])"
        $regressionSummary += "- Baseline report: $($regression["baseline_report_path"])"
        $regressionSummary += "- Failed / warning / total: $($regression["failed_checks"]) / $($regression["warning_checks"]) / $($regression["total_checks"])"
        foreach ($check in @($regression["checks"] | Where-Object { $_["status"] -ne "pass" })) {
            $regressionSummary += "- [$($check["severity"])] $($check["scope"]) $($check["scale_name"]) $($check["metric"]): current=$($check["current_value"]), baseline=$($check["baseline_value"]), threshold=$($check["threshold"]), rule=$($check["rule"])"
        }
    }
    $summary = @(
        "# Backend Resilience Report"
        ""
        "- Run ID: $($Report["run_id"])"
        "- Base URL: $($Report["base_url"])"
        "- Markets: $([string]::Join(', ', $Report["markets"]))"
        "- Started At: $($Report["started_at"])"
        "- Completed At: $($Report["completed_at"])"
        "- CSV Summary: $($Layout.csv_report)"
        ""
        "## HTTP Latency"
        ""
        "- Client impl: $($http["client_impl"])"
        "- Primary metric basis: $($http["primary_metric_basis"])"
        "- Samples: $($http["client_latency_us"]["count"])"
        "- Client P50/P95/P99/P999 (us): $($http["client_latency_us"]["p50"]) / $($http["client_latency_us"]["p95"]) / $($http["client_latency_us"]["p99"]) / $($http["client_latency_us"]["p999"])"
        "- Success: $($http["success_count"]) / $($http["total_requests"]) ($($http["success_rate"])%)"
        "- 4xx / 429: $($http["http_4xx_count"]) / $($http["http_429_count"])"
        "- Fills: $($http["fills_reported"])"
        "- Pre-submit available balance P50/P95/P99: $($http["pre_submit_available_balance"]["p50"]) / $($http["pre_submit_available_balance"]["p95"]) / $($http["pre_submit_available_balance"]["p99"])"
        "- Top-up count / amount: $($http["topup_count"]) / $($http["topup_amount"])"
        "- System-core metric basis: $($http["system_core_metric_basis"])"
        "- System-core direct-success count/rate: $($http["system_core_metric"]["count"]) / $($http["system_core_metric"]["clean_total"]) ($($http["system_core_metric"]["success_rate_pct"])%), excluded_api_limits=$($http["system_core_metric"]["excluded_api_limits"])"
        "- Single-IP metric basis: $($http["single_ip_metric_basis"])"
        "- Single-IP direct-success count/rate: $($http["single_ip_metric"]["count"]) / $($http["single_ip_metric"]["clean_total"]) ($($http["single_ip_metric"]["success_rate_pct"])%), excluded_429=$($http["single_ip_metric"]["excluded_429"])"
        "- Direct-success P50/P95/P99/P999 (us): $($http["primary_metric"]["client_latency_us"]["p50"]) / $($http["primary_metric"]["client_latency_us"]["p95"]) / $($http["primary_metric"]["client_latency_us"]["p99"]) / $($http["primary_metric"]["client_latency_us"]["p999"])"
        "- Rescued-success count/P99/P999 (us): $($http["rescued_success_metric"]["count"]) / $($http["rescued_success_metric"]["client_latency_us"]["p99"]) / $($http["rescued_success_metric"]["client_latency_us"]["p999"])"
        "- Flow-controlled-success count/P99/P999 (us): $($http["flow_controlled_success_metric"]["count"]) / $($http["flow_controlled_success_metric"]["client_latency_us"]["p99"]) / $($http["flow_controlled_success_metric"]["client_latency_us"]["p999"])"
        "- Success-path P99/P999 (us): $($http["success_path"]["client_latency_us"]["p99"]) / $($http["success_path"]["client_latency_us"]["p999"])"
        "- Error-path P99/P999 (us): $($http["error_path"]["client_latency_us"]["p99"]) / $($http["error_path"]["client_latency_us"]["p999"])"
        "- Server match_e2e P99 (us): $($http["server_metrics"]["match_e2e_p99_us"])"
        "- Server stage P99s (us): queue=$($http["server_metrics"]["queue_wait_p99_us"]), risk=$($http["server_metrics"]["risk_p99_us"]), matching=$($http["server_metrics"]["matching_core_p99_us"]), settlement=$($http["server_metrics"]["settlement_p99_us"]), post=$($http["server_metrics"]["post_match_p99_us"])"
        "- Submit-order rate-limit sources: ip=$($http["server_metrics"]["submit_order_ip_rate_limited"]), user_write=$($http["server_metrics"]["submit_order_user_rate_limited"]), engine=$($http["server_metrics"]["submit_order_engine_rate_limited"])"
        ""
        $errorCategorySummary
        $scaleSummary
        $clientModeSummary
        $regressionSummary
        "## Fault Replay"
        ""
        "- Restart cycles: $($faults["restart_cycles"])"
        "- Post-restart probes passed: $($faults["post_restart_probe_passed"])"
        "- WAL bytes growth: $($faults["wal_growth_bytes"])"
        "- Prometheus wal_errors_total: $($faults["prometheus_wal_errors_total"])"
        ""
        "## WebSocket Integrity"
        ""
        "- Trade messages: $($ws["trade_messages"])"
        "- Orderbook messages: $($ws["orderbook_messages"])"
        "- Ticker messages: $($ws["ticker_messages"])"
        "- User fill messages: $($ws["user_fill_messages"])"
        "- User balance messages: $($ws["user_balance_messages"])"
        "- Bridge alive metric: $($ws["bridge_alive"])"
        ""
        "## Soak"
        ""
        "- Samples: $($soak["samples"])"
        "- Working set initial/max/final (bytes): $($soak["working_set_initial_bytes"]) / $($soak["working_set_peak_bytes"]) / $($soak["working_set_final_bytes"])"
        "- Private bytes initial/max/final (bytes): $($soak["private_initial_bytes"]) / $($soak["private_peak_bytes"]) / $($soak["private_final_bytes"])"
        "- Restart count: $($soak["restart_count"])"
    ) -join "`n"

    Set-Content -Path $Layout.md_report -Value $summary -Encoding UTF8
}
