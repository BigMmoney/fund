# Quick Performance Test - P50/P55/P95/P99/P99.9 Latency + Full Pipeline + Complex Market
# Direct binary launch, no cargo run overhead

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\test_lib.ps1"

function Get-Percentile {
    param([double[]]$Values, [double]$Percentile)
    if ($Values.Count -eq 0) { return 0 }
    $sorted = $Values | Sort-Object
    $idx = [math]::Floor(($sorted.Count - 1) * $Percentile)
    if ($idx -ge $sorted.Count) { $idx = $sorted.Count - 1 }
    return $sorted[$idx]
}

function PrintLatencyStats {
    param([string]$Label, [double[]]$Latencies, [int]$Success, [int]$Failed, [int]$Fills)
    if ($Latencies.Count -eq 0) { Write-Host "  [WARN] No data for $Label" -ForegroundColor Yellow; return }
    $p50  = Get-Percentile -Values $Latencies -Percentile 0.50
    $p55  = Get-Percentile -Values $Latencies -Percentile 0.55
    $p95  = Get-Percentile -Values $Latencies -Percentile 0.95
    $p99  = Get-Percentile -Values $Latencies -Percentile 0.99
    $p999 = Get-Percentile -Values $Latencies -Percentile 0.999
    $min  = ($Latencies | Measure-Object -Minimum).Minimum
    $avg  = ($Latencies | Measure-Object -Average).Average
    $max  = ($Latencies | Measure-Object -Maximum).Maximum
    
    Write-Host ""
    Write-Host "  === $Label ===" -ForegroundColor Green
    Write-Host "  Total: $($Latencies.Count) | OK: $Success | Fail: $Failed | Fills: $Fills" -ForegroundColor White
    Write-Host "  P50=$([math]::Round($p50,2))ms  P55=$([math]::Round($p55,2))ms  P95=$([math]::Round($p95,2))ms  P99=$([math]::Round($p99,2))ms  P99.9=$([math]::Round($p999,2))ms" -ForegroundColor Cyan
    Write-Host "  Min=$([math]::Round($min,2))ms  Avg=$([math]::Round($avg,2))ms  Max=$([math]::Round($max,2))ms" -ForegroundColor White
    Write-Host ""
}
function PlaceOrderSync {
    param([string]$MarketId="btc-usdt",[int]$Outcome=0,[string]$Side="buy",[int64]$Price=49900,[int64]$Amount=10,[string]$OrderId,[string]$Subject=$Script:Subject,[string]$Role="user")
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $orderJson = "{`"market_id`":`"$MarketId`",`"outcome`":$Outcome,`"side`":`"$Side`",`"price`":$Price,`"amount`":$Amount,`"order_id`":`"$OrderId`",`"client_order_id`":`"$OrderId`"}"
        $resp = Invoke-ExchangeRequestAs -Method POST -Path "/order" -BodyJson $orderJson -Subject $Subject -Role $Role -Silent
        $sw.Stop()
        return @{ LatencyMs=$sw.Elapsed.TotalMilliseconds; Success=($resp.StatusCode -ge 200 -and $resp.StatusCode -lt 300) }
    } catch {
        $sw.Stop()
        return @{ LatencyMs=$sw.Elapsed.TotalMilliseconds; Success=$false }
    }
}

# ============================================================
# Phase 0: Launch service
# ============================================================
Write-Host "========================================" -ForegroundColor Magenta
Write-Host "Performance & Complex Market Test" -ForegroundColor Magenta
Write-Host "========================================" -ForegroundColor Magenta
Write-Host ""

Write-Host "[Phase 0] Starting service..." -ForegroundColor Cyan
Get-Process -Name "api","rust-exchange" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500

$walDir = Join-Path $PSScriptRoot "..\data"
if (Test-Path $walDir) {
    Get-ChildItem $walDir -Filter "*.wal*" | Remove-Item -Force -ErrorAction SilentlyContinue
    Get-ChildItem $walDir -Filter "*.jsonl" | Remove-Item -Force -ErrorAction SilentlyContinue
}

$binPath = Join-Path $PSScriptRoot "..\target\x86_64-pc-windows-gnu\release\api.exe"
if (-not (Test-Path $binPath)) {
    Write-Host "Binary NOT found at $binPath" -ForegroundColor Red
    exit 1
}

$proc = Start-Process -FilePath $binPath -WorkingDirectory (Join-Path $PSScriptRoot "..") -PassThru -WindowStyle Hidden
$maxWait = 30; $waited = 0
while ($waited -lt $maxWait) {
    try {
        $r = Invoke-WebRequest -Uri "$Script:ExchangeBaseUrl/health" -TimeoutSec 2 -UseBasicParsing
        if ($r.StatusCode -eq 200) { Write-Host "Service ready after ${waited}s" -ForegroundColor Green; break }
    } catch {}
    Start-Sleep -Milliseconds 500; $waited += 0.5
}
if ($waited -ge $maxWait) { Write-Host "Service startup timeout" -ForegroundColor Red; exit 1 }
Start-Sleep -Milliseconds 1000

# ============================================================
# Phase 1: Endpoint latency (sequential, 50 reqs each)
# ============================================================
Write-Host "[Phase 1] Endpoint Latency (sequential, n=50 each)..." -ForegroundColor Cyan
$endpointTests = @(
    @{ Name="HealthCheck";      Method="GET";    Path="/health" },
    @{ Name="MarketsList";      Method="GET";    Path="/markets" },
    @{ Name="OrderBookQuery";   Method="GET";    Path="/orderbook?market_id=btc-usdt&outcome=0" },
    @{ Name="BalanceQuery";     Method="GET";    Path="/balance" },
    @{ Name="PositionsQuery";   Method="GET";    Path="/positions" }
)

foreach ($ep in $endpointTests) {
    $lats = @(); $ok = 0
    for ($i = 0; $i -lt 50; $i++) {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        try {
            $resp = Invoke-WebRequest -Uri "$Script:ExchangeBaseUrl$($ep.Path)" -Method $ep.Method -UseBasicParsing -TimeoutSec 10
            $sw.Stop()
            $lats += $sw.Elapsed.TotalMilliseconds
            if ($resp.StatusCode -ge 200 -and $resp.StatusCode -lt 300) { $ok++ }
        } catch {
            $sw.Stop()
            $lats += $sw.Elapsed.TotalMilliseconds
        }
        Start-Sleep -Milliseconds 5
    }
    PrintLatencyStats -Label $ep.Name -Latencies $lats -Success $ok -Failed ($lats.Count - $ok) -Fills 0
}

# ============================================================
# Phase 2: Sequential Order Latency
# ============================================================
Write-Host "[Phase 2] Sequential Order Latency (n=100)..." -ForegroundColor Cyan
Test-Deposit -UserId $Script:Subject -Amount 500000 -OpId "perf-seed-cash" | Out-Null
Test-PositionDeposit -UserId $Script:Subject -MarketId "btc-usdt" -Outcome 0 -Amount 50000 -OpId "perf-seed-pos" | Out-Null
Test-PositionDeposit -UserId $Script:Subject -MarketId "btc-usdt" -Outcome 0 -Amount 50000 -OpId "perf-seed-pos-2" | Out-Null

$orderLats = @(); $orderOk = 0; $orderFails = 0; $fills = 0
for ($i = 0; $i -lt 100; $i++) {
    $side = if ($i % 2 -eq 0) { "buy" } else { "sell" }
    $price = if ($side -eq "buy") { 49900 + ($i % 20) } else { 50100 - ($i % 20) }
    $oid = "seq-$i-$(Get-Random)"
    $r = PlaceOrderSync -Side $side -Price $price -Amount 10 -OrderId $oid
        $r = PlaceOrderSync -Side $side -Price $price -Amount 10 -OrderId $oid
    $orderLats += $r.LatencyMs
    if ($r.Success) { $orderOk++ } else { $orderFails++ }
    Start-Sleep -Milliseconds 5
}
PrintLatencyStats -Label "SequentialOrders" -Latencies $orderLats -Success $orderOk -Failed $orderFails -Fills $fills

# ============================================================
# Phase 3: Concurrent Order Latency (via runspace pool)
# ============================================================
Write-Host "[Phase 3] Concurrent Order Latency..." -ForegroundColor Cyan

$concurrencyLevels = @(1, 2, 5, 10)
$ordersPerLevel = 100

foreach ($conc in $concurrencyLevels) {
    $ordersPerWorker = [math]::Ceiling($ordersPerLevel / $conc)
    $scriptContent = @"
. "$PSScriptRoot\test_lib.ps1"
`$results = @()
for (`$i = 0; `$i -lt $ordersPerWorker; `$i++) {
    `$oid = "conc-$conc-`$i-$(Get-Random)"
    `$sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        `$orderObj = [PSCustomObject]@{ market_id="btc-usdt"; outcome=0; side="buy"; price=49900; amount=5; order_id=`$oid; client_order_id=`$oid }
        `$orderJson = `$orderObj | ConvertTo-Json -Compress
        `$resp = Invoke-ExchangeRequestAs -Method POST -Path "/order" -BodyJson `$orderJson -Subject "${Script:Subject}" -Role "user" -Silent
        `$sw.Stop()
        `$results += [PSCustomObject]@{ LatencyMs=`$sw.Elapsed.TotalMilliseconds; Success=(`$resp.StatusCode -ge 200 -and `$resp.StatusCode -lt 300) }
    } catch {
        `$sw.Stop()
        `$results += [PSCustomObject]@{ LatencyMs=`$sw.Elapsed.TotalMilliseconds; Success=`$false }
    }
    Start-Sleep -Milliseconds 10
}
`$results | ConvertTo-Json -Compress
"@
    
    $ps = [powershell]::Create()
    $ps.AddScript($scriptContent) | Out-Null
    $asyncResult = $ps.BeginInvoke()
    
    $jobs = @(@{ Handle = $ps; AsyncResult = $asyncResult })
    for ($w = 1; $w -lt $conc; $w++) {
        $ps2 = [powershell]::Create()
        $ps2.AddScript($scriptContent) | Out-Null
        $ar2 = $ps2.BeginInvoke()
        $jobs += @{ Handle = $ps2; AsyncResult = $ar2 }
    }
    
    foreach ($j in $jobs) { $j.AsyncResult.AsyncWaitHandle.WaitOne() | Out-Null }
    
    $allLats = @(); $allOk = 0; $allFail = 0
    foreach ($j in $jobs) {
        $raw = $j.Handle.EndInvoke($j.AsyncResult)
        $j.Handle.Dispose()
        $raw -split "`n" | ForEach-Object {
            try {
                $obj = $_ | ConvertFrom-Json
                $allLats += $obj.LatencyMs
                if ($obj.Success) { $allOk++ } else { $allFail++ }
            } catch {}
        }
    }
    
    PrintLatencyStats -Label "Concurrent-$conc" -Latencies $allLats -Success $allOk -Failed $allFail -Fills 0
}

# ============================================================
# Phase 4: Full Pipeline (Order -> Match -> Settlement)
# ============================================================
Write-Host "[Phase 4] Full Pipeline Test (matching orders)..." -ForegroundColor Cyan

# Fresh restart
$proc.Kill(); $proc.WaitForExit(3000) | Out-Null
Start-Sleep -Milliseconds 1000
$proc = Start-Process -FilePath $binPath -WorkingDirectory (Join-Path $PSScriptRoot "..") -PassThru -WindowStyle Hidden
$waited = 0
while ($waited -lt 30) {
    try { $r = Invoke-WebRequest -Uri "$Script:ExchangeBaseUrl/health" -TimeoutSec 2 -UseBasicParsing; if ($r.StatusCode -eq 200) { break } } catch {}
    Start-Sleep -Milliseconds 500; $waited += 0.5
}
Start-Sleep -Milliseconds 1000

# Setup two traders
Test-Deposit -UserId "trader-a" -Amount 200000 -OpId "pipe-a-cash" | Out-Null
Test-PositionDeposit -UserId "trader-a" -MarketId "eth-usdt" -Outcome 0 -Amount 10000 -OpId "pipe-a-pos" | Out-Null
Test-Deposit -UserId "trader-a" -MarketId "eth-usdt" -Outcome 0 -Amount 10000 -OpId "pipe-a-pos-2" | Out-Null
Test-Deposit -UserId "trader-b" -Amount 200000 -OpId "pipe-b-cash" | Out-Null
Test-PositionDeposit -UserId "trader-b" -MarketId "eth-usdt" -Outcome 0 -Amount 10000 -OpId "pipe-b-pos" | Out-Null
Test-Deposit -UserId "trader-b" -MarketId "eth-usdt" -Outcome 0 -Amount 10000 -OpId "pipe-b-pos-2" | Out-Null

$pipelineLats = @(); $matchCount = 0
for ($i = 0; $i -lt 30; $i++) {
    $oidSell = "pipe-sell-$i"
    $oidBuy = "pipe-buy-$i"
    
    # Trader A places sell
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $orderJsonA = "{`"market_id`":`"eth-usdt`",`"outcome`":0,`"side`":`"sell`",`"price`":1800,`"amount`":5,`"order_id`":`"$oidSell`",`"client_order_id`":`"$oidSell`"}"
    $respA = Invoke-ExchangeRequestAs -Method POST -Path "/order" -BodyJson $orderJsonA -Subject "trader-a" -Role "user" -Silent
    $sw.Stop()
    $pipelineLats += $sw.Elapsed.TotalMilliseconds
    $okA = ($respA.StatusCode -ge 200 -and $respA.StatusCode -lt 300)
    
    Start-Sleep -Milliseconds 30
    
    # Trader B places buy (should match)
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $orderJsonB = "{`"market_id`":`"eth-usdt`",`"outcome`":0,`"side`":`"buy`",`"price`":1800,`"amount`":5,`"order_id`":`"$oidBuy`",`"client_order_id`":`"$oidBuy`"}"
    $respB = Invoke-ExchangeRequestAs -Method POST -Path "/order" -BodyJson $orderJsonB -Subject "trader-b" -Role "user" -Silent
    $sw.Stop()
    $okB = ($respB.StatusCode -ge 200 -and $respB.StatusCode -lt 300)
    $pipelineLats += $sw.Elapsed.TotalMilliseconds
    if ($okA -and $okB) { $matchCount++ }
    
    Start-Sleep -Milliseconds 50
}
PrintLatencyStats -Label "FullPipeline" -Latencies $pipelineLats -Success ($pipelineLats.Count) -Failed 0 -Fills $matchCount

# ============================================================
# Phase 5: Complex Market Simulation
# ============================================================
Write-Host "[Phase 5] Complex Market Simulation..." -ForegroundColor Cyan

# Three markets, one trader
$markets = @("btc-usdt", "eth-usdt", "sol-usdt")
Test-Deposit -UserId "complex-trader" -Amount 1500000 -OpId "cx-seed-all" | Out-Null
foreach ($m in $markets) {
    Test-PositionDeposit -UserId "complex-trader" -MarketId $m -Outcome 0 -Amount 10000 -OpId "cx-pos-$m" | Out-Null
}

$cxlats = @()

# Scenario 1: High volatility
Write-Host "  Scenario 1: High Volatility" -ForegroundColor Yellow
for ($i = 0; $i -lt 50; $i++) {
    $price = 50000 + (Get-Random -Minimum -5000 -Maximum 5000)
    $side = if ($i % 2 -eq 0) { "buy" } else { "sell" }
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $orderJson = "{`"market_id`":`"btc-usdt`",`"outcome`":0,`"side`":`"$side`",`"price`":$price,`"amount`":5,`"order_id`":`"vol-$i`",`"client_order_id`":`"vol-$i`"}"
    $resp = Invoke-ExchangeRequestAs -Method POST -Path "/order" -BodyJson $orderJson -Subject "complex-trader" -Role "user" -Silent
    $sw.Stop()
    $cxlats += $sw.Elapsed.TotalMilliseconds
    if ($resp.StatusCode -ge 200 -and $resp.StatusCode -lt 300) { $cxok++ } else { $cxfail++ }
    Start-Sleep -Milliseconds 10
}

# Scenario 2: Multi-market activity
Write-Host "  Scenario 2: Multi-Market" -ForegroundColor Yellow
foreach ($m in $markets) {
    $basePrice = if ($m -eq "btc-usdt") { 50000 } elseif ($m -eq "eth-usdt") { 1800 } else { 100 }
    for ($i = 0; $i -lt 20; $i++) {
        $side = if ($i % 2 -eq 0) { "buy" } else { "sell" }
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $orderJson = "{`"market_id`":`"$m`",`"outcome`":0,`"side`":`"$side`",`"price`":$basePrice,`"amount`":10,`"order_id`":`"cx-$m-$i`",`"client_order_id`":`"cx-$m-$i`"}"
        $resp = Invoke-ExchangeRequestAs -Method POST -Path "/order" -BodyJson $orderJson -Subject "complex-trader" -Role "user" -Silent
        $sw.Stop()
        $cxlats += $sw.Elapsed.TotalMilliseconds
        if ($resp.StatusCode -ge 200 -and $resp.StatusCode -lt 300) { $cxok++ } else { $cxfail++ }
        Start-Sleep -Milliseconds 10
    }
}

# Scenario 3: Order book depth (30 price levels)
Write-Host "  Scenario 3: Order Book Depth" -ForegroundColor Yellow
for ($i = 0; $i -lt 30; $i++) {
    $price = 50000 + ($i * 100)
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $orderJson = "{`"market_id`":`"btc-usdt`",`"outcome`":0,`"side`":`"sell`",`"price`":$price,`"amount`":1,`"order_id`":`"depth-$i`",`"client_order_id`":`"depth-$i`"}"
    $resp = Invoke-ExchangeRequestAs -Method POST -Path "/order" -BodyJson $orderJson -Subject "complex-trader" -Role "user" -Silent
    $sw.Stop()
    $cxlats += $sw.Elapsed.TotalMilliseconds
    if ($resp.StatusCode -ge 200 -and $resp.StatusCode -lt 300) { $cxok++ } else { $cxfail++ }
    Start-Sleep -Milliseconds 5
}

PrintLatencyStats -Label "ComplexMarket" -Latencies $cxlats -Success $cxok -Failed $cxfail -Fills 0

# ============================================================
# Phase 6: Soak Test (short, 3 min)
# ============================================================
Write-Host "[Phase 6] Soak Test (3 min)..." -ForegroundColor Cyan
Test-Deposit -UserId "soak-user" -Amount 1000000 -OpId "soak-seed" | Out-Null
Test-PositionDeposit -UserId "soak-user" -MarketId "btc-usdt" -Outcome 0 -Amount 50000 -OpId "soak-pos" | Out-Null
Test-PositionDeposit -UserId "soak-user" -MarketId "btc-usdt" -Outcome 0 -Amount 50000 -OpId "soak-pos-2" | Out-Null

$soakLats = @(); $soakOk = 0; $soakFail = 0; $soakStart = Get-Date; $soakDur = [TimeSpan]::FromMinutes(3); $si = 0
while ((Get-Date) - $soakStart -lt $soakDur) {
    $side = if ($si % 2 -eq 0) { "buy" } else { "sell" }
    $price = if ($side -eq "buy") { 49900 } else { 50100 }
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $orderJson = "{`"market_id`":`"btc-usdt`",`"outcome`":0,`"side`":`"$side`",`"price`":$price,`"amount`":1,`"order_id`":`"soak-$si`",`"client_order_id`":`"soak-$si`"}"
    $resp = Invoke-ExchangeRequestAs -Method POST -Path "/order" -BodyJson $orderJson -Subject "soak-user" -Role "user" -Silent
    $sw.Stop()
    $soakLats += $sw.Elapsed.TotalMilliseconds
    if ($resp.StatusCode -ge 200 -and $resp.StatusCode -lt 300) { $soakOk++ } else { $soakFail++ }
    $si++
    Start-Sleep -Milliseconds 50
    
    if ($si % 100 -eq 0) {
        $elapsed = (Get-Date) - $soakStart
        Write-Host "  Soak: $([math]::Round($elapsed.TotalMinutes,1))/3.0 min ($si orders)" -ForegroundColor DarkGray
    }
}
PrintLatencyStats -Label "SoakTest" -Latencies $soakLats -Success $soakOk -Failed $soakFail -Fills 0

# ============================================================
# Cleanup
# ============================================================
Write-Host "[Cleanup]" -ForegroundColor Yellow
$proc.Kill(); $proc.WaitForExit(3000) | Out-Null
Get-Process -Name "api" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue

Write-Host "========================================" -ForegroundColor Magenta
Write-Host "ALL TESTS COMPLETE" -ForegroundColor Magenta
Write-Host "========================================" -ForegroundColor Magenta
