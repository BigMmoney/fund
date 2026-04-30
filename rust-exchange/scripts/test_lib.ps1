# Shared test library for exchange testing
# Usage: . "$PSScriptRoot\test_lib.ps1"

$Script:ExchangeBaseUrl = "http://127.0.0.1:3030"
$Script:Secret = "dev-secret-change-me-to-32-chars-min!"
$Script:AuthMode = "internal"
$Script:ApiKey = ""
$Script:ApiSecret = ""
$Script:Subject = "user-test-123"
$Script:Role = "user"
$Script:SessionId = ""

# Admin credentials for admin operations
$Script:AdminSubject = "admin-test-123"
$Script:AdminRole = "admin"

# Second admin for dual-approval governance
$Script:AdminSubject2 = "admin-test-456"
$Script:AdminRole2 = "admin"

# Third admin for dual-approval governance (needed when required_approvals = 2)
$Script:AdminSubject3 = "admin-test-789"
$Script:AdminRole3 = "admin"

# ============================================================
# HMAC Authentication
# ============================================================

function Compute-HmacSignature {
    param(
        [string]$Method,
        [string]$Path,
        [string]$Query,
        [string]$Timestamp,
        [string]$RequestId,
        [string]$Subject = $Script:Subject,
        [string]$Role = $Script:Role
    )
    $Payload = "$Method`n$Path`n$Query`n$Subject`n$Role`n$Script:SessionId`n$Timestamp`n$RequestId"
    $hmac = New-Object System.Security.Cryptography.HMACSHA256
    $hmac.Key = [System.Text.Encoding]::UTF8.GetBytes($Script:Secret)
    $signatureBytes = $hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($Payload))
    return [BitConverter]::ToString($signatureBytes).Replace("-", "").ToLowerInvariant()
}

function Compute-ApiKeySignature {
    param(
        [string]$Method,
        [string]$Path,
        [string]$Query,
        [string]$Timestamp,
        [string]$RequestId,
        [string]$Subject = $Script:Subject,
        [string]$Role = $Script:Role,
        [string]$BodyHash,
        [string]$ApiKey = $Script:ApiKey,
        [string]$ApiSecret = $Script:ApiSecret
    )
    $Payload = "$Method`n$Path`n$Query`n$ApiKey`n$Subject`n$Role`n$Timestamp`n$RequestId`n$BodyHash"
    $hmac = New-Object System.Security.Cryptography.HMACSHA256
    $hmac.Key = [System.Text.Encoding]::UTF8.GetBytes($ApiSecret)
    $signatureBytes = $hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($Payload))
    return [BitConverter]::ToString($signatureBytes).Replace("-", "").ToLowerInvariant()
}

function Invoke-AdminRequest {
    param(
        [string]$Method = "POST",
        [string]$Path,
        [string]$Query = "",
        [string]$BodyJson = "",
        [string]$Subject = $Script:AdminSubject,
        [string]$Role = $Script:AdminRole,
        [switch]$Silent
    )
    
    $RequestId = [guid]::NewGuid().ToString()
    $Timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    
    $bodyBytes = if ($BodyJson) { [System.Text.Encoding]::UTF8.GetBytes($BodyJson) } else { @() }
    $bodyHash = Compute-BodySha256 -BodyBytes $bodyBytes
    $signature = Compute-HmacSignature -Method $Method -Path $Path -Query $Query -Timestamp $Timestamp -RequestId $RequestId -Subject $Subject -Role $Role
    
    $tempFile = $null
    if ($BodyJson) {
        $tempFile = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), "exchange_admin_req_$RequestId.json")
        [System.IO.File]::WriteAllBytes($tempFile, $bodyBytes)
    }
    
    $curlHeaders = @(
        "-H", "Content-Type: application/json",
        "-H", "x-request-id: $RequestId",
        "-H", "x-internal-auth-subject: $Subject",
        "-H", "x-internal-auth-role: $Role",
        "-H", "x-internal-auth-session-id: $Script:SessionId",
        "-H", "x-internal-auth-timestamp: $Timestamp",
        "-H", "x-internal-auth-signature: $signature",
        "-H", "x-internal-auth-body-sha256: $bodyHash"
    )
    
    $url = "$Script:ExchangeBaseUrl$Path"
    if ($Query) { $url += "?$Query" }
    
    $curlArgs = @("-s", "-w", "\n%{http_code}") + $curlHeaders
    if ($tempFile) {
        $curlArgs += @("--data-binary", "@$tempFile")
    }
    $curlArgs += @($url)
    
    try {
        $curlOutput = & curl.exe $curlArgs 2>&1
        $lines = $curlOutput -split "`n"
        $statusCode = [int]$lines[-1].Trim()
        $responseBody = ($lines[0..($lines.Length-2)] -join "`n").Trim()
        $parsedJson = $null
        try {
            $parsedJson = $responseBody | ConvertFrom-Json -ErrorAction SilentlyContinue
        } catch {}
        if (-not $Silent) {
            Write-Host "  [ADMIN][$statusCode] $Method $Path" -ForegroundColor $(if ($statusCode -ge 200 -and $statusCode -lt 300) { "Green" } elseif ($statusCode -lt 500) { "Yellow" } else { "Red" })
        }
        return @{
            StatusCode   = $statusCode
            Body         = $responseBody
            ParsedJson   = $parsedJson
            RequestId    = $RequestId
            HasValidJson = $null -ne $parsedJson
        }
    } finally {
        if ($tempFile -and (Test-Path $tempFile)) {
            Remove-Item $tempFile -ErrorAction SilentlyContinue
        }
    }
}

function Compute-BodySha256 {
    param([Parameter(Mandatory=$false)][byte[]]$BodyBytes)
    $sha256 = [System.Security.Cryptography.SHA256]::Create()
    if ($null -eq $BodyBytes -or $BodyBytes.Length -eq 0) {
        $hashBytes = $sha256.ComputeHash([byte[]]@(0))
        # SHA256 of empty input
        $emptyBytes = [System.Text.Encoding]::UTF8.GetBytes("")
        $hashBytes = $sha256.ComputeHash($emptyBytes)
    } else {
        $hashBytes = $sha256.ComputeHash($BodyBytes)
    }
    return [BitConverter]::ToString($hashBytes).Replace("-", "").ToLowerInvariant()
}

# ============================================================
# HTTP Request Helpers
# ============================================================

function Invoke-ExchangeRequest {
    param(
        [string]$Method = "POST",
        [string]$Path,
        [string]$Query = "",
        [string]$BodyJson = "",
        [switch]$Silent
    )
    return Invoke-ExchangeRequestAs -Method $Method -Path $Path -Query $Query -BodyJson $BodyJson -Subject $Script:Subject -Role $Script:Role -Silent:$Silent.IsPresent
}

function Invoke-ExchangeRequestAs {
    param(
        [string]$Method = "POST",
        [string]$Path,
        [string]$Query = "",
        [string]$BodyJson = "",
        [string]$Subject = $Script:Subject,
        [string]$Role = $Script:Role,
        [switch]$Silent
    )
    
    $RequestId = [guid]::NewGuid().ToString()
    $Timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    
    $bodyBytes = if ($BodyJson) { [System.Text.Encoding]::UTF8.GetBytes($BodyJson) } else { @() }
    $bodyHash = Compute-BodySha256 -BodyBytes $bodyBytes
    $useApiKeyAuth = $Script:AuthMode -eq "api_key" -and -not [string]::IsNullOrWhiteSpace($Script:ApiKey) -and -not [string]::IsNullOrWhiteSpace($Script:ApiSecret)
    if ($useApiKeyAuth) {
        $signature = Compute-ApiKeySignature -Method $Method -Path $Path -Query $Query -Timestamp $Timestamp -RequestId $RequestId -Subject $Subject -Role $Role -BodyHash $bodyHash
    } else {
        $signature = Compute-HmacSignature -Method $Method -Path $Path -Query $Query -Timestamp $Timestamp -RequestId $RequestId -Subject $Subject -Role $Role
    }
    
    # Write body to temp file for curl --data-binary
    $tempFile = $null
    if ($BodyJson) {
        $tempFile = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), "exchange_req_$RequestId.json")
        [System.IO.File]::WriteAllBytes($tempFile, $bodyBytes)
    }
    
    $curlHeaders = @("-H", "Content-Type: application/json", "-H", "x-request-id: $RequestId")
    if ($useApiKeyAuth) {
        $curlHeaders += @(
            "-H", "x-api-key: $Script:ApiKey",
            "-H", "x-api-timestamp: $Timestamp",
            "-H", "x-api-signature: $signature",
            "-H", "x-api-body-sha256: $bodyHash"
        )
    } else {
        $curlHeaders += @(
            "-H", "x-internal-auth-subject: $Subject",
            "-H", "x-internal-auth-role: $Role",
            "-H", "x-internal-auth-session-id: $Script:SessionId",
            "-H", "x-internal-auth-timestamp: $Timestamp",
            "-H", "x-internal-auth-signature: $signature",
            "-H", "x-internal-auth-body-sha256: $bodyHash"
        )
    }
    
    $url = "$Script:ExchangeBaseUrl$Path"
    if ($Query) { $url += "?$Query" }
    
    $curlArgs = @("-s", "-w", "\n%{http_code}") + $curlHeaders
    if ($tempFile) {
        $curlArgs += @("--data-binary", "@$tempFile")
    }
    $curlArgs += @($url)
    
    try {
        $curlOutput = & curl.exe $curlArgs 2>&1
        
        # Parse: last line is status code
        $lines = $curlOutput -split "`n"
        $statusCode = [int]$lines[-1].Trim()
        $responseBody = ($lines[0..($lines.Length-2)] -join "`n").Trim()
        
        # Try parse JSON
        $parsedJson = $null
        try {
            $parsedJson = $responseBody | ConvertFrom-Json -ErrorAction SilentlyContinue
        } catch {}
        
        if (-not $Silent) {
            Write-Host "  [$statusCode] $Method $Path" -ForegroundColor $(if ($statusCode -ge 200 -and $statusCode -lt 300) { "Green" } elseif ($statusCode -lt 500) { "Yellow" } else { "Red" })
        }
        
        return @{
            StatusCode   = $statusCode
            Body         = $responseBody
            ParsedJson   = $parsedJson
            RequestId    = $RequestId
            HasValidJson = $null -ne $parsedJson
        }
    } finally {
        if ($tempFile -and (Test-Path $tempFile)) {
            Remove-Item $tempFile -ErrorAction SilentlyContinue
        }
    }
}

# ============================================================
# Order Builders
# ============================================================

function New-OrderJson {
    param(
        [string]$MarketId = "btc-usdt",
        [string]$Side = "buy",
        [string]$OrderType = "limit",
        [decimal]$Price = 50000,
        [int64]$Amount = 1000,
        [int]$Outcome = 0,
        [string]$TimeInForce = "gtc",
        [string]$ClientOrderId = ""
    )
    if (-not $ClientOrderId) {
        $ClientOrderId = "ord_$([guid]::NewGuid().ToString().Substring(0,8))"
    }
    $obj = [ordered]@{
        market_id       = $MarketId
        side            = $Side
        order_type      = $OrderType
        price           = $Price
        amount          = $Amount
        outcome         = $Outcome
        time_in_force   = $TimeInForce
        client_order_id = $ClientOrderId
    }
    return $obj | ConvertTo-Json -Compress
}

function New-CancelJson {
    param(
        [string]$MarketId = "btc-usdt",
        [string]$OrderId
    )
    return @{
        market_id = $MarketId
        order_id  = $OrderId
    } | ConvertTo-Json -Compress
}

function New-BatchOrdersJson {
    param([int]$Count = 5)
    $orders = @()
    for ($i = 0; $i -lt $Count; $i++) {
        $orders += @(New-OrderJson -Side "sell" -Price (60000 + $i) -Amount 1000)
    }
    return "[$($orders -join ',')]"
}

function Test-Deposit {
    param(
        [string]$UserId = $Script:Subject,
        [int64]$Amount = 10000000,
        [string]$OpId = "seed-$(Get-Random)"
    )
    $depositJson = @"
{"user_id":"$UserId","amount":$Amount,"op_id":"$OpId"}
"@
    $resp = Invoke-AdminRequest -Path "/deposit" -BodyJson $depositJson -Silent
    return $resp.StatusCode -eq 200
}

function Test-PositionDeposit {
    param(
        [string]$UserId = $Script:Subject,
        [string]$MarketId = "btc-usdt",
        [int]$Outcome = 0,
        [int64]$Amount = 10000,
        [string]$OpId = "seed-pos-$(Get-Random)"
    )
    $depositJson = @"
{"user_id":"$UserId","market_id":"$MarketId","outcome":$Outcome,"amount":$Amount,"op_id":"$OpId"}
"@
    $resp = Invoke-AdminRequest -Path "/position-deposit" -BodyJson $depositJson -Silent
    return $resp.StatusCode -eq 200
}

# ============================================================
# Service Management
# ============================================================

function Start-ExchangeService {
    param(
        [switch]$NoClearWal,
        [int]$WaitTimeoutSeconds = 30,
        [string]$StdoutLog = $null,
        [string]$StderrLog = $null
    )

    $rustRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
    $apiPath = if ($env:EXCHANGE_API_EXE) {
        $env:EXCHANGE_API_EXE
    } elseif (Test-Path (Join-Path $rustRoot "target/release/api.exe")) {
        Join-Path $rustRoot "target/release/api.exe"
    } else {
        Join-Path $rustRoot "target/x86_64-pc-windows-gnu/release/api.exe"
    }
    if (-not (Test-Path $apiPath)) {
        Write-Host "API binary not found at $apiPath — run 'cargo build -p api --release' first" -ForegroundColor Red
        return $false
    }

    if (-not $NoClearWal) {
        $walDir = Join-Path $rustRoot "data"
        if (Test-Path $walDir) {
            Get-ChildItem $walDir -Filter "*.wal*" | Remove-Item -Force -ErrorAction SilentlyContinue
            Get-ChildItem $walDir -Filter "*.jsonl" | Remove-Item -Force -ErrorAction SilentlyContinue
        }
    }

    Write-Host "Starting exchange service ($apiPath)..." -ForegroundColor Cyan
    $startProcessArgs = @{
        FilePath         = $apiPath
        WorkingDirectory = $rustRoot
        PassThru         = $true
        WindowStyle      = 'Hidden'
    }
    if ($StdoutLog) { $startProcessArgs.RedirectStandardOutput = $StdoutLog }
    if ($StderrLog) { $startProcessArgs.RedirectStandardError = $StderrLog }
    $process = Start-Process @startProcessArgs
    $Script:ApiProcess = $process
    $Script:ExchangeProcess = $process   # back-compat alias for legacy callers

    $health = Wait-ExchangeReady -TimeoutSeconds $WaitTimeoutSeconds
    if ($null -eq $health) {
        Write-Host "Service startup timeout after ${WaitTimeoutSeconds}s (PID=$($process.Id))" -ForegroundColor Red
        return $false
    }
    Write-Host "Service ready (PID=$($process.Id), seq=$($health.frontiers.sequencer_command_seq))" -ForegroundColor Green
    return $true
}

function Stop-ExchangeService {
    param([int]$WaitSeconds = 5)

    if ($Script:ApiProcess -and -not $Script:ApiProcess.HasExited) {
        try { $Script:ApiProcess.Kill() } catch {}
        $Script:ApiProcess.WaitForExit($WaitSeconds * 1000) | Out-Null
    }
    # Belt-and-suspenders: also kill any orphan api.exe (catches cargo-wrapper orphans
    # from older harness versions and any process started outside this session).
    Get-Process -Name "api" -ErrorAction SilentlyContinue | ForEach-Object {
        try { Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue } catch {}
    }
    Get-Process -Name "rust-exchange" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    # Wait for port 3030 to be released by the OS.
    $deadline = (Get-Date).AddSeconds($WaitSeconds)
    while ((Get-Date) -lt $deadline) {
        $listening = Get-NetTCPConnection -LocalPort 3030 -State Listen -ErrorAction SilentlyContinue
        if (-not $listening) { break }
        Start-Sleep -Milliseconds 250
    }
    $Script:ApiProcess = $null
    $Script:ExchangeProcess = $null
    Write-Host "Service stopped" -ForegroundColor Yellow
}

function Restart-ExchangeService {
    param([switch]$ClearWal)
    Stop-ExchangeService
    Start-Sleep -Milliseconds 500
    if ($ClearWal) {
        return Start-ExchangeService
    } else {
        return Start-ExchangeService -NoClearWal
    }
}

function Wait-ExchangeReady {
    param([int]$TimeoutSeconds = 30)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        try {
            $h = Invoke-RestMethod -Uri "$Script:ExchangeBaseUrl/health" -TimeoutSec 2 -UseBasicParsing -ErrorAction Stop
            if ($h.status -eq "ok") { return $h }
        } catch {}
        Start-Sleep -Milliseconds 500
    }
    return $null
}

function Get-ExchangeReadiness {
    try {
        return Invoke-RestMethod -Uri "$Script:ExchangeBaseUrl/ready" -TimeoutSec 5 -UseBasicParsing -ErrorAction Stop
    } catch { return $null }
}

function Assert-FrontiersConsistent {
    param($Health, [string]$Stage = "")
    if (-not $Health -or -not $Health.frontiers -or -not $Health.frontiers.consistent) {
        $detail = if ($Health -and $Health.frontiers) { ($Health.frontiers | ConvertTo-Json -Compress) } else { "<none>" }
        throw "Frontiers inconsistent at stage '$Stage' (got: $detail)"
    }
}

function Assert-Eq {
    param($Expected, $Actual, [string]$Label)
    if ($Expected -ne $Actual) { throw "Assertion failed [$Label]: expected $Expected, got $Actual" }
}

function Assert-Gt {
    param($Lo, $Hi, [string]$Label)
    if (-not ($Hi -gt $Lo)) { throw "Assertion failed [$Label]: expected $Hi > $Lo" }
}

# ============================================================
# Health Check
# ============================================================

function Test-ServiceHealth {
    param([int]$OrderCount = 5)
    
    Write-Host "  Health check: sending $OrderCount normal orders..." -ForegroundColor Gray
    
    $successCount = 0
    for ($i = 0; $i -lt $OrderCount; $i++) {
        $orderJson = New-OrderJson -Side "sell" -Price (70000 + $i) -Amount 1000
        $resp = Invoke-ExchangeRequest -Path "/submit-order" -BodyJson $orderJson -Silent
        if ($resp.StatusCode -eq 200) {
            $successCount++
        } else {
            Write-Host "  FAIL: Order $i got $($resp.StatusCode)" -ForegroundColor Red
        }
    }
    
    return $successCount -eq $OrderCount
}

# ============================================================
# Result Logging
# ============================================================

$Script:TestResults = @()

function Log-Result {
    param(
        [string]$Phase,
        [string]$Scenario,
        [int]$StatusCode,
        [string]$ExpectedStatus,
        [bool]$HasValidJson,
        [string]$Message = "",
        [string]$TraceId = ""
    )
    $pass = $StatusCode.ToString() -eq $ExpectedStatus
    $Script:TestResults += @{
        Phase          = $Phase
        Scenario       = $Scenario
        StatusCode     = $StatusCode
        ExpectedStatus = $ExpectedStatus
        Pass           = $pass
        HasValidJson   = $HasValidJson
        Message        = $Message
        TraceId        = $TraceId
    }
    
    $color = if ($pass) { "Green" } else { "Red" }
    $icon = if ($pass) { "PASS" } else { "FAIL" }
    Write-Host "  [$icon] $Scenario -> HTTP $StatusCode (expected $ExpectedStatus)" -ForegroundColor $color
    if ($Message) {
        Write-Host "       $Message" -ForegroundColor DarkGray
    }
}

function Show-TestSummary {
    Write-Host "`n========================================" -ForegroundColor Cyan
    Write-Host "TEST SUMMARY" -ForegroundColor Cyan
    Write-Host "========================================" -ForegroundColor Cyan
    
    $total = $Script:TestResults.Count
    $passed = ($Script:TestResults | Where-Object { $_.Pass }).Count
    $failed = $total - $passed
    
    Write-Host "Total: $total | Passed: $passed | Failed: $failed" -ForegroundColor $(if ($failed -eq 0) { "Green" } else { "Red" })
    
    if ($failed -gt 0) {
        Write-Host "`nFailed scenarios:" -ForegroundColor Red
        $Script:TestResults | Where-Object { -not $_.Pass } | ForEach-Object {
            Write-Host "  - $($_.Phase)/$($_.Scenario): got $($_.StatusCode), expected $($_.ExpectedStatus)" -ForegroundColor Red
        }
    }
    
    Write-Host "========================================`n" -ForegroundColor Cyan
    return $failed -eq 0
}
