#Requires -Version 5.1
# Benchmark v4: High-precision latency benchmark using .NET HttpClient
# Eliminates curl.exe process creation overhead (~5-10ms per request)

param(
    [ValidateSet("Quick", "ConcurrencySweep")]
    [string]$Mode = "Quick",
    [int]$Concurrency = 10
)

Add-Type -AssemblyName "System.Net.Http"
Add-Type -AssemblyName "System.Security"

$BASE_URL = "http://localhost:3030"
$SECRET = "dev-secret-change-me"
$INSTRUMENT = "btc-usdt"
$OUTCOME = 0
$SIDE_OPTIONS = @("Buy", "Sell")
$AUTH_SUBJECT_PREFIX = "bench-user"
$AUTH_ROLE = "user"

function Compute-HmacSignature([string]$message, [string]$secret) {
    $hmac = [System.Security.Cryptography.HMACSHA256]::new(
        [System.Text.Encoding]::UTF8.GetBytes($secret))
    $hash = $hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($message))
    [BitConverter]::ToString($hash).Replace("-", "").ToLowerInvariant()
}

function Compute-BodyHash([byte[]]$bodyBytes) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $hash = $sha.ComputeHash($bodyBytes)
    $sha.Dispose()
    [BitConverter]::ToString($hash).Replace("-", "").ToLowerInvariant()
}

function Make-AuthHeaders([string]$method, [string]$path, [string]$subject, [string]$role, [string]$requestId, [byte[]]$bodyBytes) {
    $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $bodyHash = Compute-BodyHash -bodyBytes $bodyBytes
    $payload = "${method}`n${path}`n`n${subject}`n${role}`n`n${timestamp}`n${requestId}"
    $signature = Compute-HmacSignature -message $payload -secret $SECRET
    @{
        "x-internal-auth-subject"     = $subject
        "x-internal-auth-role"        = $role
        "x-internal-auth-session-id"  = ""
        "x-internal-auth-timestamp"   = $timestamp
        "x-internal-auth-signature"   = $signature
        "x-internal-auth-body-sha256" = $bodyHash
        "x-request-id"                = $requestId
        "Content-Type"                = "application/json"
    }
}

function New-BenchmarkClient {
    $handler = [System.Net.Http.HttpClientHandler]::new()
    $handler.UseCookies = $false
    $client = [System.Net.Http.HttpClient]::new($handler)
    $client.Timeout = [TimeSpan]::FromSeconds(5)
    $client
}

function Invoke-HttpPost {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$Url,
        [string]$Path,
        [string]$Subject,
        [string]$RequestId,
        [string]$Body
    )
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($Body)
    $headers = Make-AuthHeaders -method "POST" -path $Path -subject $Subject -role $AUTH_ROLE -requestId $RequestId -bodyBytes $bodyBytes
    
    $content = [System.Net.Http.StringContent]::new($body, [System.Text.Encoding]::UTF8, "application/json")
    foreach ($kv in $headers.GetEnumerator()) {
        if ($kv.Key -eq "Content-Type") {
            $content.Headers.ContentType = [System.Net.Http.Headers.MediaTypeHeaderValue]::Parse($kv.Value)
        } else {
            $content.Headers.TryAddWithoutValidation($kv.Key, $kv.Value) | Out-Null
        }
    }
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $resp = $client.PostAsync($Url, $content).GetAwaiter().GetResult()
        $sw.Stop()
        $respBody = $resp.Content.ReadAsStringAsync().GetAwaiter().GetResult()
        [PSCustomObject]@{
            StatusCode = [int]$resp.StatusCode
            DurationMs = [math]::Round($sw.Elapsed.TotalMilliseconds, 2)
            Body       = $respBody
        }
    } catch {
        $sw.Stop()
        [PSCustomObject]@{
            StatusCode = 0
            DurationMs = [math]::Round($sw.Elapsed.TotalMilliseconds, 2)
            Body       = $_.Exception.Message
        }
    } finally {
        $content.Dispose()
    }
}

function Compute-Percentiles([double[]]$Values) {
    if ($Values.Count -eq 0) { return @{ p50 = 0; p95 = 0; p99 = 0; avg = 0; min = 0; max = 0 } }
    $sorted = @($Values | Sort-Object)
    $p50 = $sorted[[math]::Floor($sorted.Count * 0.50)]
    $p95 = $sorted[[math]::Min([math]::Floor($sorted.Count * 0.95), $sorted.Count - 1)]
    $p99 = $sorted[[math]::Min([math]::Floor($sorted.Count * 0.99), $sorted.Count - 1)]
    $avg = [math]::Round(($Values | Measure-Object -Average).Average, 2)
    @{
        p50 = [math]::Round($p50, 2)
        p95 = [math]::Round($p95, 2)
        p99 = [math]::Round($p99, 2)
        avg = $avg
        min = [math]::Round($sorted[0], 2)
        max = [math]::Round($sorted[-1], 2)
    }
}

function Fund-Accounts {
    param([int]$Count = 8, [decimal]$Amount = 100000)
    Write-Host "`n=== Funding $Count Accounts ===" -ForegroundColor Cyan
    $client = New-BenchmarkClient
    $success = 0
    for ($i = 0; $i -lt $Count; $i++) {
        $opId = [Guid]::NewGuid().ToString()
        $body = @{ user_id = "bench-user-$i"; amount = [int64]$Amount; op_id = $opId } | ConvertTo-Json -Compress
        $requestId = [Guid]::NewGuid().ToString()
        $result = Invoke-HttpPost -Client $client -Url "$BASE_URL/deposit" -Path "/deposit" -Subject "admin" -RequestId $requestId -Body $body
        if ($result.StatusCode -eq 200) { $success++ }
        else { Write-Host "  Fund bench-user-$i failed: $($result.StatusCode) $($result.Body.Substring(0, [Math]::Min(60, $result.Body.Length)))" -ForegroundColor Yellow }
    }
    $client.Dispose()
    Write-Host "  Funded: $success/$Count" -ForegroundColor $(if ($success -eq $Count) { "Green" } else { "Yellow" })
}

function Capture-ServerMetrics {
    try {
        $raw = Invoke-RestMethod -Uri "$BASE_URL/metrics" -TimeoutSec 3
        $metrics = @{}
        if ($raw -is [array]) {
            foreach ($m in $raw) {
                $metrics[$m.name] = $m
            }
        }
        return $metrics
    } catch {
        return @{}
    }
}

function Format-MetricRow {
    param([hashtable]$Metrics, [string]$Name, [string]$Label)
    $m = $Metrics[$Name]
    if ($m -and $m.values) {
        $v = $m.values[0]
        $p50 = if ($v.quantiles) { [math]::Round($v.quantiles["0.5"] / 1000, 2) } else { "-" }
        $p95 = if ($v.quantiles) { [math]::Round($v.quantiles["0.95"] / 1000, 2) } else { "-" }
        $p99 = if ($v.quantiles) { [math]::Round($v.quantiles["0.99"] / 1000, 2) } else { "-" }
        Write-Host ("  {0,-22} p50={1,6}ms  p95={2,6}ms  p99={3,6}ms" -f $Label, $p50, $p95, $p99)
    }
}

function Run-Quick {
    param([int]$N = 10)
    Write-Host "`n=== Quick Test ($N orders) ===" -ForegroundColor Cyan
    Fund-Accounts -Count 8 -Amount 100000
    
    $client = New-BenchmarkClient
    $latencies = [System.Collections.Generic.List[double]]::new()
    $success = 0
    $failed = 0
    
    for ($i = 0; $i -lt $N; $i++) {
        $side = if ($i % 2 -eq 0) { "buy" } else { "sell" }
        $price = if ($side -eq "buy") { 95000 + ($i * 100) } else { 105000 - ($i * 100) }
        $requestId = [Guid]::NewGuid().ToString()
        $body = @{
            market_id       = $INSTRUMENT
            side            = $side
            price           = [int64]$price
            amount          = [int64]1
            outcome         = 0
            client_order_id = "v4-$i"
            request_id      = $requestId
        } | ConvertTo-Json -Compress
        
        $subject = "$AUTH_SUBJECT_PREFIX-$($i % 8)"
        $result = Invoke-HttpPost -Client $client -Url "$BASE_URL/intent" -Path "/intent" -Subject $subject -RequestId $requestId -Body $body
        
        $latencies.Add($result.DurationMs)
        if ($result.StatusCode -in @(200, 201, 202)) { $success++ } else { $failed++; Write-Host "  [$i] $($result.StatusCode): $($result.Body.Substring(0, [Math]::Min(80, $result.Body.Length)))" -ForegroundColor Yellow }
    }
    
    $client.Dispose()
    $pct = Compute-Percentiles -Values @($latencies)
    
    Write-Host "`n--- Results ---" -ForegroundColor Cyan
    Write-Host ("  Orders: {0}/{1} ({2:P0}) | Failed: {3}" -f $success, $N, ($success/$N), $failed)
    Write-Host ("  Latency: P50={0}ms | P95={1}ms | P99={2}ms | Avg={3}ms | Min={4}ms | Max={5}ms" -f $pct.p50, $pct.p95, $pct.p99, $pct.avg, $pct.min, $pct.max)
    
    Write-Host "`n--- Server Metrics ---" -ForegroundColor Cyan
    $metrics = Capture-ServerMetrics
    Format-MetricRow -Metrics $metrics -Name "http_request_us" -Label "HTTP Total"
    Format-MetricRow -Metrics $metrics -Name "match_e2e_us" -Label "Match E2E"
    Format-MetricRow -Metrics $metrics -Name "match_execution_us" -Label "Match Execution"
    Format-MetricRow -Metrics $metrics -Name "queue_wait_us" -Label "Queue Wait"
    Format-MetricRow -Metrics $metrics -Name "wal_append_us" -Label "WAL Append"
}

function Run-ConcurrencySweep {
    Write-Host "`n=== Concurrency Sweep (HttpClient) ===" -ForegroundColor Cyan
    Fund-Accounts -Count 8 -Amount 100000
    
    $levels = @(1, 2, 4, 8, 16, 32)
    $ordersPerLevel = 50
    $refillThreshold = 0.3
    
    $results = @()
    
    foreach ($C in $levels) {
        Write-Host "`n--- C=$C ($ordersPerLevel orders) ---" -ForegroundColor Yellow
        
        $allLatencies = [System.Collections.Generic.List[double]]::new()
        $totalSuccess = 0
        $totalFailed = 0
        $totalSent = 0
        
        # Create shared clients (one per thread to avoid contention)
        $clients = @()
        for ($c = 0; $c -lt $C; $c++) {
            $clients += New-BenchmarkClient
        }
        
        $batchSw = [System.Diagnostics.Stopwatch]::StartNew()
        
        for ($batch = 0; $batch -lt $ordersPerLevel; $batch += $C) {
            $remaining = [Math]::Min($C, $ordersPerLevel - $batch)
            
            # Fire requests in parallel using jobs (faster than runspaces for this workload)
            $jobs = @()
            for ($j = 0; $j -lt $remaining; $j++) {
                $idx = $batch + $j
                $clientId = $j % $C
                $side = $SIDE_OPTIONS[$idx % 2]
                $price = if ($side -eq "Buy") { 95000 + ($idx * 100) } else { 105000 - ($idx * 100) }
                
                $job = Start-Job -ScriptBlock {
                    Add-Type -AssemblyName "System.Net.Http"
                    Add-Type -AssemblyName "System.Security"
                    
                    param($baseUrl, $secret, $instrument, $outcome, $accountId, $side, $price, $quantity)
                    
                    function Compute-HmacSig([string]$b, [string]$s) {
                        $hmac = [System.Security.Cryptography.HMACSHA256]::new([System.Text.Encoding]::UTF8.GetBytes($s))
                        $hash = $hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($b))
                        [BitConverter]::ToString($hash).Replace("-", "").ToLowerInvariant()
                    }
                    function Compute-BodyH([string]$b) {
                        $sha = [System.Security.Cryptography.SHA256]::Create()
                        [Convert]::ToBase64String($sha.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($b)))
                    }
                    
                    $body = @{
                        account_id  = $accountId
                        instrument  = $instrument
                        outcome     = $outcome
                        side        = $side
                        price       = [decimal]$price
                        quantity    = [decimal]$quantity
                        order_type  = "Limit"
                        tif         = "GTC"
                    } | ConvertTo-Json -Compress
                    
                    $sig = Compute-HmacSig -b $body -s $secret
                    $bh = Compute-BodyH -b $body
                    
                    $handler = [System.Net.Http.HttpClientHandler]::new()
                    $handler.UseCookies = $false
                    $client = [System.Net.Http.HttpClient]::new($handler)
                    $client.Timeout = [TimeSpan]::FromSeconds(5)
                    
                    $content = [System.Net.Http.StringContent]::new($body, [System.Text.Encoding]::UTF8, "application/json")
                    $content.Headers.TryAddWithoutValidation("X-Auth-Role", "user") | Out-Null
                    $content.Headers.TryAddWithoutValidation("X-Auth-Signature", $sig) | Out-Null
                    $content.Headers.TryAddWithoutValidation("X-Auth-Body-Hash", $bh) | Out-Null
                    
                    $sw = [System.Diagnostics.Stopwatch]::StartNew()
                    try {
                        $resp = $client.PostAsync("$baseUrl/intent", $content).GetAwaiter().GetResult()
                        $sw.Stop()
                        $respBody = $resp.Content.ReadAsStringAsync().GetAwaiter().GetResult()
                        @{ StatusCode = [int]$resp.StatusCode; DurationMs = [math]::Round($sw.Elapsed.TotalMilliseconds, 2) }
                    } catch {
                        $sw.Stop()
                        @{ StatusCode = 0; DurationMs = [math]::Round($sw.Elapsed.TotalMilliseconds, 2) }
                    } finally {
                        $content.Dispose(); $client.Dispose()
                    }
                } -ArgumentList "$BASE_URL", $SECRET, $INSTRUMENT, $OUTCOME, "acct_$(($batch + $j) % 8)", $side, $price, "0.01"
                
                $jobs += $job
            }
            
            # Collect results
            foreach ($job in $jobs) {
                $jobResult = Receive-Job -Job $job -Wait -AutoRemoveJob
                $allLatencies.Add($jobResult.DurationMs)
                if ($jobResult.StatusCode -in @(200, 201, 202)) { $totalSuccess++ } else { $totalFailed++ }
                $totalSent++
            }
        }
        
        $batchSw.Stop()
        
        # Cleanup clients
        foreach ($c in $clients) { $c.Dispose() }
        
        $pct = Compute-Percentiles -Values @($allLatencies)
        $elapsedSec = [math]::Round($batchSw.Elapsed.TotalSeconds, 2)
        $throughput = [math]::Round($totalSent / $elapsedSec, 0)
        
        $row = [PSCustomObject]@{
            C       = $C
            Sent    = $totalSent
            Success = $totalSuccess
            Failed  = $totalFailed
            P50     = $pct.p50
            P95     = $pct.p95
            P99     = $pct.p99
            Avg     = $pct.avg
            Min     = $pct.min
            Max     = $pct.max
            Sec     = $elapsedSec
            OpsSec  = $throughput
        }
        $results += $row
        
        Write-Host ("  C={0,-3} Sent={1,-4} OK={2,-4} Fail={3,-4} P50={4,5}ms P95={5,5}ms P99={6,6}ms Avg={7,5}ms | {8,4} ops/s ({9}s)" -f
            $row.C, $row.Sent, $row.Success, $row.Failed, $row.P50, $row.P95, $row.P99, $row.Avg, $row.OpsSec, $row.Sec)
    }
    
    # Summary table
    Write-Host "`n=== Summary ===" -ForegroundColor Cyan
    Write-Host ("{0,-4} {1,-6} {2,-6} {3,-6} {4,-7} {5,-7} {6,-7} {7,-6} {8,-6}" -f "C", "Sent", "OK", "Fail", "P50", "P95", "P99", "Avg", "ops/s")
    Write-Host ("{0,-4} {1,-6} {2,-6} {3,-6} {4,-7} {5,-7} {6,-7} {7,-6} {8,-6}" -f "---", "------", "------", "------", "-------", "-------", "-------", "------", "------")
    foreach ($r in $results) {
        Write-Host ("{0,-4} {1,-6} {2,-6} {3,-6} {4,-7} {5,-7} {6,-7} {7,-6} {8,-6}" -f
            $r.C, $r.Sent, $r.Success, $r.Failed, "$($r.P50)ms", "$($r.P95)ms", "$($r.P99)ms", "$($r.Avg)ms", $r.OpsSec)
    }
    
    Write-Host "`n--- Server Metrics ---" -ForegroundColor Cyan
    $metrics = Capture-ServerMetrics
    Format-MetricRow -Metrics $metrics -Name "http_request_us" -Label "HTTP Total"
    Format-MetricRow -Metrics $metrics -Name "match_e2e_us" -Label "Match E2E"
    Format-MetricRow -Metrics $metrics -Name "match_execution_us" -Label "Match Execution"
    Format-MetricRow -Metrics $metrics -Name "queue_wait_us" -Label "Queue Wait"
    Format-MetricRow -Metrics $metrics -Name "wal_append_us" -Label "WAL Append"
}

switch ($Mode) {
    "Quick"            { Run-Quick -N $Concurrency }
    "ConcurrencySweep" { Run-ConcurrencySweep }
}
