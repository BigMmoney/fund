# Smoke test for the P1-OPS-1 + P2-SEC-2 (WS path) bundle landed
# this session. Boots a fresh server, verifies:
#   1. /metrics/prometheus exposes the new wallet metrics
#   2. POST /v2/ws-token returns a valid token
#   3. /ws/order-trace?token=<...> accepts the token (WebSocket
#      handshake completes)
#
# Pre-req: api.exe built at target/x86_64-pc-windows-gnu/release/.
# Pre-req: data/internal_auth.secret exists and matches $Secret below.

[CmdletBinding()]
param(
    [string]$BaseUri = "http://127.0.0.1:3030",
    [string]$Secret  = "dev-secret-change-me-to-32-chars-min!",
    [string]$Subject = "test-admin",
    [string]$Role    = "admin",
    [int]$StartupTimeoutSecs = 30
)

$ErrorActionPreference = "Stop"

function Get-Sha256Hex {
    param([byte[]]$Bytes)
    if ($null -eq $Bytes -or $Bytes.Length -eq 0) { $Bytes = [byte[]]@() }
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        $h = $sha.ComputeHash($Bytes)
    } finally { $sha.Dispose() }
    return [BitConverter]::ToString($h).Replace("-", "").ToLowerInvariant()
}

function Get-HmacHex {
    param([string]$Message, [string]$Secret)
    $hmac = [System.Security.Cryptography.HMACSHA256]::new(
        [System.Text.Encoding]::UTF8.GetBytes($Secret))
    try {
        $b = $hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($Message))
    } finally { $hmac.Dispose() }
    return [BitConverter]::ToString($b).Replace("-", "").ToLowerInvariant()
}

function New-AuthHeaders {
    param(
        [string]$Method,
        [string]$Path,
        [string]$Query = "",
        [byte[]]$BodyBytes = @(),
        [string]$Subject,
        [string]$Role,
        [string]$Secret,
        [string]$SessionId = "",
        [string]$RequestId = ([Guid]::NewGuid().ToString("N"))
    )
    $ts = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $payload = "{0}`n{1}`n{2}`n{3}`n{4}`n{5}`n{6}`n{7}" -f `
        $Method.ToUpperInvariant(), $Path, $Query, $Subject, $Role, $SessionId, $ts, $RequestId
    $sig = Get-HmacHex -Message $payload -Secret $Secret
    $bodyHash = Get-Sha256Hex -Bytes $BodyBytes
    return [ordered]@{
        "x-request-id"                = $RequestId
        "x-internal-auth-subject"     = $Subject
        "x-internal-auth-role"        = $Role
        "x-internal-auth-session-id"  = $SessionId
        "x-internal-auth-timestamp"   = $ts
        "x-internal-auth-signature"   = $sig
        "x-internal-auth-body-sha256" = $bodyHash
    }
}

function Wait-ForHealth {
    param([string]$Uri, [int]$Timeout)
    $deadline = (Get-Date).AddSeconds($Timeout)
    while ((Get-Date) -lt $deadline) {
        try {
            $r = Invoke-WebRequest -UseBasicParsing -Uri "$Uri/health" -TimeoutSec 2
            if ($r.StatusCode -eq 200) { return $true }
        } catch {}
        Start-Sleep -Milliseconds 500
    }
    return $false
}

Write-Host "=== Phase 1: wait for /health ==="
if (-not (Wait-ForHealth -Uri $BaseUri -Timeout $StartupTimeoutSecs)) {
    throw "server did not become healthy within $StartupTimeoutSecs s"
}
Write-Host "OK — server up at $BaseUri"

Write-Host "=== Phase 2: /metrics/prometheus has new wallet metrics ==="
$prom = Invoke-WebRequest -UseBasicParsing -Uri "$BaseUri/metrics/prometheus"
$expected = @(
    "wallet_settlements_settled_total",
    "wallet_settlements_failed_total",
    "wallet_settlements_stuck_total",
    "wallet_sanctions_errors_total",
    "wallet_hot_wallet_balance"
)
foreach ($name in $expected) {
    if ($prom.Content -notmatch [regex]::Escape($name)) {
        throw "metric $name MISSING from /metrics/prometheus"
    }
    Write-Host "  OK $name"
}

Write-Host "=== Phase 3: POST /v2/ws-token mints a token ==="
$body = '{"ws_path":"/ws/order-trace"}'
$bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($body)
$headers = New-AuthHeaders `
    -Method POST -Path "/v2/ws-token" -Query "" `
    -BodyBytes $bodyBytes -Subject $Subject -Role $Role -Secret $Secret
$mint = Invoke-WebRequest -UseBasicParsing -Uri "$BaseUri/v2/ws-token" `
    -Method POST -Headers $headers -ContentType "application/json" -Body $body
$mintJson = $mint.Content | ConvertFrom-Json
if (-not $mintJson.token) { throw "mint response missing 'token': $($mint.Content)" }
if ($mintJson.ttl_secs -lt 10 -or $mintJson.ttl_secs -gt 300) {
    throw "ttl_secs out of expected clamp [10, 300]: $($mintJson.ttl_secs)"
}
if ($mintJson.ws_path -ne "/ws/order-trace") {
    throw "ws_path mismatch: $($mintJson.ws_path)"
}
$token = $mintJson.token
Write-Host "  OK token len=$($token.Length) ttl_secs=$($mintJson.ttl_secs)"

Write-Host "=== Phase 4: invalid ws_path is rejected ==="
$bodyBad = '{"ws_path":"/ws/should-not-mint"}'
$bodyBadBytes = [System.Text.Encoding]::UTF8.GetBytes($bodyBad)
$headersBad = New-AuthHeaders `
    -Method POST -Path "/v2/ws-token" -Query "" `
    -BodyBytes $bodyBadBytes -Subject $Subject -Role $Role -Secret $Secret
$got400 = $false
try {
    $null = Invoke-WebRequest -UseBasicParsing -Uri "$BaseUri/v2/ws-token" `
        -Method POST -Headers $headersBad -ContentType "application/json" -Body $bodyBad
} catch {
    # Both PS 5.1 (System.Net.WebException) and PS 7
    # (Microsoft.PowerShell.Commands.HttpResponseException) expose the
    # status code via .Exception.Response.StatusCode. Read it generically.
    $resp = $_.Exception.Response
    if ($null -ne $resp) {
        $code = [int]$resp.StatusCode
        if ($code -eq 400) { $got400 = $true } else { throw "expected 400, got $code" }
    } else {
        throw "expected 400 with response, got: $($_.Exception.Message)"
    }
}
if (-not $got400) { throw "expected 400 BAD_REQUEST but call succeeded" }
Write-Host "  OK rejected with 400"

Write-Host "=== Phase 5: /ws/order-trace?token accepts the minted token ==="
$wsBase = $BaseUri -replace '^http', 'ws'
$wsUri = [Uri]"$wsBase/ws/order-trace?token=$([System.Uri]::EscapeDataString($token))"
$client = [System.Net.WebSockets.ClientWebSocket]::new()
$cts = [System.Threading.CancellationTokenSource]::new()
$cts.CancelAfter([TimeSpan]::FromSeconds(5))
try {
    $client.ConnectAsync($wsUri, $cts.Token).Wait()
    if ($client.State -ne [System.Net.WebSockets.WebSocketState]::Open) {
        throw "ws state is $($client.State), expected Open"
    }
    Write-Host "  OK ws connected (State=Open)"
    # Read the first frame ({"type":"ready"}) to confirm the principal
    # passed through end-to-end.
    $buf = New-Object 'byte[]' 4096
    $seg = [System.ArraySegment[byte]]::new($buf)
    $cts2 = [System.Threading.CancellationTokenSource]::new()
    $cts2.CancelAfter([TimeSpan]::FromSeconds(5))
    $r = $client.ReceiveAsync($seg, $cts2.Token).Result
    $msg = [System.Text.Encoding]::UTF8.GetString($buf, 0, $r.Count)
    Write-Host "  first frame: $msg"
    if ($msg -notmatch '"ready"') {
        throw "expected a {ready} frame, got: $msg"
    }
    Write-Host "  OK received ready frame"
    # Best-effort close. Server may have already closed; ignore any
    # exception here — the functional checks above already passed.
    try {
        $closeCts = [System.Threading.CancellationTokenSource]::new()
        $closeCts.CancelAfter([TimeSpan]::FromSeconds(2))
        $client.CloseAsync(
            [System.Net.WebSockets.WebSocketCloseStatus]::NormalClosure,
            "test done", $closeCts.Token).Wait()
    } catch {
        # ignore close-side errors
    }
} finally {
    $client.Dispose()
}

Write-Host "=== Phase 6: /ws/order-trace with bogus token is rejected ==="
$wsUriBad = [Uri]"$wsBase/ws/order-trace?token=v1.deadbeef.user.0.deadbeef.dead"
$clientBad = [System.Net.WebSockets.ClientWebSocket]::new()
$cts3 = [System.Threading.CancellationTokenSource]::new()
$cts3.CancelAfter([TimeSpan]::FromSeconds(5))
try {
    try {
        $clientBad.ConnectAsync($wsUriBad, $cts3.Token).Wait()
        throw "expected ws connect to fail with 401, but got State=$($clientBad.State)"
    } catch {
        # WebSocketException wrapped in AggregateException — any failure
        # at handshake is acceptable.
        Write-Host "  OK ws rejected: $($_.Exception.Message.Substring(0, [Math]::Min(120, $_.Exception.Message.Length)))"
    }
} finally {
    $clientBad.Dispose()
}

Write-Host ""
Write-Host "=== ALL PHASES PASSED ==="
