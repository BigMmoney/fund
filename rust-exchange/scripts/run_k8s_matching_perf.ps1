param(
    [string]$BaseUrl = "http://127.0.0.1:30030",
    [string]$Secret = "deployment-acceptance-secret-32-bytes!!",
    [string]$Market = "btc-usdt",
    [int]$BuyerCount = 24,
    [int]$SellerCount = 24,
    [int]$PairCount = 2000,
    [int]$PairConcurrency = 20,
    [int]$RateLimitPerSecond = 400,
    [int]$RequestStaggerMs = 0,
    [long]$BasePrice = 50000,
    [long]$Amount = 1
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $scriptRoot
$workspaceRoot = Split-Path -Parent $repoRoot

. (Join-Path $scriptRoot "backend_resilience_lib.ps1")

$runId = Get-Date -Format "yyyyMMdd-HHmmss"
$artifactDir = Join-Path $repoRoot ("artifacts\k8s-matching-perf\{0}" -f $runId)
New-Item -ItemType Directory -Force -Path $artifactDir | Out-Null

$buyers = 1..$BuyerCount | ForEach-Object { "k8s-buyer-{0:D2}" -f $_ }
$sellers = 1..$SellerCount | ForEach-Object { "k8s-seller-{0:D2}" -f $_ }

$client = New-HttpClient -TimeoutSeconds 60
try {
    $health = Invoke-RestMethod -Uri ($BaseUrl.TrimEnd('/') + "/health") -TimeoutSec 15
    $ready = Invoke-RestMethod -Uri ($BaseUrl.TrimEnd('/') + "/ready") -TimeoutSec 15

    Seed-ExchangeUsers `
        -Client $client `
        -BaseUrl $BaseUrl `
        -Secret $Secret `
        -Markets @($Market) `
        -BuyerUsers $buyers `
        -SellerUsers $sellers `
        -AdminSubject "bench-admin"

    $preMetrics = Get-ApiMetricsJson -Client $client -BaseUrl $BaseUrl -Secret $Secret -Subject "bench-admin" -Role "admin"

    $buyerCsv = [string]::Join(",", $buyers)
    $sellerCsv = [string]::Join(",", $sellers)
    $benchJsonPath = Join-Path $artifactDir "exchange_http_bench.json"

    $goArgs = @(
        "run",
        ".\benchmark\cmd\exchange_http_bench\main.go",
        "--base-url", $BaseUrl,
        "--secret", $Secret,
        "--market", $Market,
        "--buyers", $buyerCsv,
        "--sellers", $sellerCsv,
        "--pair-count", $PairCount,
        "--pair-concurrency", $PairConcurrency,
        "--rate-limit-per-second", $RateLimitPerSecond,
        "--request-stagger-ms", $RequestStaggerMs,
        "--base-price", $BasePrice,
        "--amount", $Amount,
        "--disable-keep-alives=false",
        "--admin-subject", "bench-admin",
        "--prefix", ("k8s-{0}" -f $runId)
    )

    $benchJson = & go @goArgs
    if ($LASTEXITCODE -ne 0) {
        throw "exchange_http_bench failed with exit code $LASTEXITCODE"
    }
    Set-Content -Path $benchJsonPath -Value $benchJson -Encoding UTF8

    $bench = $benchJson | ConvertFrom-Json
    $postMetrics = Get-ApiMetricsJson -Client $client -BaseUrl $BaseUrl -Secret $Secret -Subject "bench-admin" -Role "admin"
    $prometheus = Get-ApiPrometheusText -Client $client -BaseUrl $BaseUrl -Secret $Secret
    $prometheusPath = Join-Path $artifactDir "metrics.prometheus.txt"
    Set-Content -Path $prometheusPath -Value $prometheus.body -Encoding UTF8

    $summary = [ordered]@{
        run_id = $runId
        collected_at = (Get-Date).ToString("s")
        base_url = $BaseUrl
        market = $Market
        buyers = $buyers
        sellers = $sellers
        pair_count = $PairCount
        pair_concurrency = $PairConcurrency
        rate_limit_per_second = $RateLimitPerSecond
        health = $health
        ready = $ready
        benchmark = ConvertTo-PlainOrdered -InputObject $bench
        pre_metrics = ConvertTo-PlainOrdered -InputObject $preMetrics.parsed
        post_metrics = ConvertTo-PlainOrdered -InputObject $postMetrics.parsed
        server_metrics_summary = Get-ServerMetricsSummary -MetricsJson $postMetrics
    }

    $summaryPath = Join-Path $artifactDir "summary.json"
    $summary | ConvertTo-Json -Depth 20 | Set-Content -Path $summaryPath -Encoding UTF8

    $serverSummary = $summary.server_metrics_summary
    $mdLines = @(
        "# Kubernetes Matching Performance",
        "",
        "- Run ID: $runId",
        "- Base URL: $BaseUrl",
        "- Access path: NodePort (no kubectl port-forward)",
        "- Market: $Market",
        "- Pair count / concurrency / rate / stagger: $PairCount / $PairConcurrency / $RateLimitPerSecond / $RequestStaggerMs",
        "- Success count / total: $($bench.success_count) / $($bench.total_requests) ($($bench.success_rate)%)",
        "- Client P50 / P95 / P99 / P999 (us): $($bench.client_latency_us.p50) / $($bench.client_latency_us.p95) / $($bench.client_latency_us.p99) / $($bench.client_latency_us.p999)",
        "- Client stage P99 (us): prepare=$($bench.prepare_order_us.p99), encode=$($bench.encode_request_us.p99), build_sign=$($bench.build_and_sign_request_us.p99), roundtrip=$($bench.http_roundtrip_us.p99), read=$($bench.response_read_us.p99), parse=$($bench.response_parse_us.p99), retry_wait=$($bench.retry_backoff_us.p99), recovery=$($bench.recovery_action_us.p99)",
        "- Direct-success client stage P99 (us): prepare=$($bench.direct_success_path.prepare_order_us.p99), encode=$($bench.direct_success_path.encode_request_us.p99), build_sign=$($bench.direct_success_path.build_and_sign_request_us.p99), roundtrip=$($bench.direct_success_path.http_roundtrip_us.p99), read=$($bench.direct_success_path.response_read_us.p99), parse=$($bench.direct_success_path.response_parse_us.p99)",
        "- Matching core P99 (us): $($bench.matching_core_us.p99)",
        "- Queue wait P99 (us): $($bench.queue_wait_us.p99)",
        "- Server match_e2e P99 (us): $($serverSummary.match_e2e_p99_us)",
        "- Server HTTP request P99 (us): $($serverSummary.http_request_p99_us)",
        "- Fills reported: $($bench.fills_reported)",
        "- HTTP 4xx / 429: $($bench.http_4xx_count) / $($bench.http_429_count)",
        "- Artifact JSON: $summaryPath",
        "- Prometheus snapshot: $prometheusPath"
    )
    $mdPath = Join-Path $artifactDir "summary.md"
    Set-Content -Path $mdPath -Value ($mdLines -join "`r`n") -Encoding UTF8

    Write-Output $summaryPath
} finally {
    $client.Dispose()
}
