# End-to-end event-orchestration test.
#
# Exercises REST paths + the canonical backend event streams:
#   /ws/order-trace     — every order's lifecycle (api → seq → match → ledger)
#   /ws/trades/<market> — public trade tape
#
# What it does:
#   1. Health + version snapshot.
#   2. Open both WS streams (order-trace via /v2/ws-token, trades public).
#      Each stream auto-reconnects on close (logs every reconnect attempt).
#   3. Capture audit-log size before, run scenarios, capture after, diff.
#   4. Scenarios:
#       A. single order: submit → cancel
#       B. concurrent: 5 orders fired in parallel via ThreadJob
#       C. balance change before/after deposit
#       D. role switch: same /balances call as user (own) and admin (target)
#   5. After each scenario, drain the in-memory event ring and print
#      timestamped entries: stream / kind / market / order_id / stage /
#      principal-relevant fields.
#   6. Final: WS error/reconnect counters; ws_connections_active.
#
# Pre-req: server up at $BaseUri, secret in data/internal_auth.secret matches
# $Secret. Order-trace endpoint requires the new ws-token mint flow (already
# tested by test_p1_landing.ps1).

[CmdletBinding()]
param(
    [string]$BaseUri = "http://127.0.0.1:3030",
    [string]$Secret  = "dev-secret-change-me-to-32-chars-min!",
    [string]$Market  = "btc-usdt",
    [string]$User    = "trader-001",
    [string]$Admin   = "ops-1",
    [int]$ConcurrentOrders = 5,
    [int]$DelayMs   = 250
)

$ErrorActionPreference = "Stop"

# ── Auth helpers ─────────────────────────────────────────────────
function Sha256Hex([byte[]]$Bytes) {
    if ($null -eq $Bytes) { $Bytes = [byte[]]@() }
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try { $h = $sha.ComputeHash($Bytes) } finally { $sha.Dispose() }
    [BitConverter]::ToString($h).Replace("-","").ToLowerInvariant()
}
function HmacHex([string]$Msg, [string]$Sec) {
    $hmac = [System.Security.Cryptography.HMACSHA256]::new([System.Text.Encoding]::UTF8.GetBytes($Sec))
    try { $b = $hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($Msg)) } finally { $hmac.Dispose() }
    [BitConverter]::ToString($b).Replace("-","").ToLowerInvariant()
}
function Send-Signed {
    param(
        [string]$Method, [string]$Path, [string]$Query="", [string]$Body=$null,
        [string]$Subject, [string]$Role, [string]$Session=""
    )
    $bodyBytes = if ($Body) { [System.Text.Encoding]::UTF8.GetBytes($Body) } else { [byte[]]@() }
    $ts  = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $rid = [Guid]::NewGuid().ToString("N")
    $payload = "{0}`n{1}`n{2}`n{3}`n{4}`n{5}`n{6}`n{7}" -f `
        $Method.ToUpperInvariant(), $Path, $Query, $Subject, $Role, $Session, $ts, $rid
    $sig = HmacHex $payload $Secret
    $url = "$BaseUri$Path" + $(if ($Query) { "?$Query" } else { "" })
    $req = [System.Net.HttpWebRequest]::Create($url)
    $req.Method = $Method
    if ($Body) { $req.ContentType = "application/json" }
    $req.Headers.Add("x-request-id", $rid)
    $req.Headers.Add("x-internal-auth-subject", $Subject)
    $req.Headers.Add("x-internal-auth-role", $Role)
    $req.Headers.Add("x-internal-auth-session-id", $Session)
    $req.Headers.Add("x-internal-auth-timestamp", $ts)
    $req.Headers.Add("x-internal-auth-signature", $sig)
    $req.Headers.Add("x-internal-auth-body-sha256", (Sha256Hex $bodyBytes))
    if ($Body) { $stream = $req.GetRequestStream(); $stream.Write($bodyBytes, 0, $bodyBytes.Length); $stream.Close() }
    try {
        $resp = $req.GetResponse()
        $reader = [System.IO.StreamReader]::new($resp.GetResponseStream())
        $b = $reader.ReadToEnd()
        $reader.Close(); $resp.Close()
        return [pscustomobject]@{ status=200; body=$b }
    } catch [System.Net.WebException] {
        $r = $_.Exception.Response
        if ($r) {
            $reader = [System.IO.StreamReader]::new($r.GetResponseStream())
            $b = $reader.ReadToEnd(); $reader.Close()
            return [pscustomobject]@{ status=[int]$r.StatusCode; body=$b }
        }
        throw
    }
}

# ── Event ring (thread-safe append; main thread drains) ─────────
$script:Events = [System.Collections.Concurrent.ConcurrentQueue[object]]::new()
function Record-Event([string]$Stream, $Frame) {
    $rec = [pscustomobject]@{
        ts     = [DateTimeOffset]::UtcNow.ToString("HH:mm:ss.fff")
        stream = $Stream
        frame  = $Frame
    }
    $script:Events.Enqueue($rec)
}
function Drain-Events([string]$Label) {
    Write-Host ""; Write-Host "── events ($Label) ─────────────────────────"
    $count = 0; $ev = $null
    while ($script:Events.TryDequeue([ref]$ev)) {
        $count++
        # Compact summary: pull the most useful fields per stream type.
        $summary = ""
        try {
            $r = $ev.frame
            if ($r.type -eq "trace") {
                $e = $r.event
                $summary = "trace stage=$($e.stage) order_id=$($e.order_id) cmd_seq=$($e.command_seq) market=$($e.market_id) user=$($e.user_id) filled=$($e.filled_amount) reject=$($e.reject_code)"
            } elseif ($r.type -eq "ready") {
                $summary = "ready"
            } elseif ($r.type -eq "lagged") {
                $summary = "lagged skipped=$($r.skipped)"
            } elseif ($r.event_type) {
                $summary = "$($r.event_type) market=$($r.market_id) data=$([Math]::Min(120, ($r.data | ConvertTo-Json -Compress -Depth 3 | Measure-Object -Character).Characters))"
                if ($null -ne $r.data) { $summary = "$($r.event_type) market=$($r.market_id) data=$($r.data | ConvertTo-Json -Compress -Depth 3)" }
            } else {
                $summary = ($r | ConvertTo-Json -Compress -Depth 3)
                if ($summary.Length -gt 200) { $summary = $summary.Substring(0, 197) + "..." }
            }
        } catch { $summary = "<unparseable>" }
        Write-Host ("  [{0}] {1,-12} {2}" -f $ev.ts, $ev.stream, $summary)
    }
    if ($count -eq 0) { Write-Host "  (no new events)" }
    return $count
}

# ── WS stream (auto-reconnect; appends to $script:Events) ───────
$script:WsStats = @{}  # name -> @{ opens; reconnects; errors; closes }
function Start-WsStream {
    param([string]$Name, [string]$WsUri)
    if (-not $script:WsStats.ContainsKey($Name)) {
        $script:WsStats[$Name] = @{ opens=0; reconnects=0; errors=0; closes=0 }
    }
    # Background thread: own its own ClientWebSocket, reconnect on close.
    $rs = [runspacefactory]::CreateRunspace()
    $rs.Open()
    $rs.SessionStateProxy.SetVariable("Events", $script:Events)
    $rs.SessionStateProxy.SetVariable("WsStats", $script:WsStats)
    $rs.SessionStateProxy.SetVariable("Name", $Name)
    $rs.SessionStateProxy.SetVariable("WsUri", $WsUri)
    $ps = [powershell]::Create()
    $ps.Runspace = $rs
    [void]$ps.AddScript({
        $stop = $false
        $attempt = 0
        while (-not $stop) {
            $client = [System.Net.WebSockets.ClientWebSocket]::new()
            $cts = [System.Threading.CancellationTokenSource]::new()
            $cts.CancelAfter(8000)
            try {
                $client.ConnectAsync([Uri]$WsUri, $cts.Token).Wait()
                $WsStats[$Name].opens++
                $rec = [pscustomobject]@{
                    ts = [DateTimeOffset]::UtcNow.ToString("HH:mm:ss.fff")
                    stream = $Name
                    frame = [pscustomobject]@{ type="_ws_open"; uri=$WsUri }
                }
                $Events.Enqueue($rec)
                $attempt = 0
                $buf = New-Object 'byte[]' 65536
                while ($client.State -eq [System.Net.WebSockets.WebSocketState]::Open) {
                    $cts2 = [System.Threading.CancellationTokenSource]::new()
                    $cts2.CancelAfter(60000)
                    try {
                        $r = $client.ReceiveAsync([System.ArraySegment[byte]]::new($buf), $cts2.Token).Result
                        if ($r.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close) { break }
                        $msg = [System.Text.Encoding]::UTF8.GetString($buf, 0, $r.Count)
                        try { $obj = $msg | ConvertFrom-Json } catch { $obj = [pscustomobject]@{ raw=$msg } }
                        $rec = [pscustomobject]@{
                            ts = [DateTimeOffset]::UtcNow.ToString("HH:mm:ss.fff")
                            stream = $Name
                            frame = $obj
                        }
                        $Events.Enqueue($rec)
                    } catch { break }
                }
            } catch {
                $WsStats[$Name].errors++
                $rec = [pscustomobject]@{
                    ts = [DateTimeOffset]::UtcNow.ToString("HH:mm:ss.fff")
                    stream = $Name
                    frame = [pscustomobject]@{ type="_ws_error"; error=$_.Exception.Message.Substring(0,[Math]::Min(140, $_.Exception.Message.Length)) }
                }
                $Events.Enqueue($rec)
            } finally {
                try { $client.Dispose() } catch {}
                $WsStats[$Name].closes++
            }
            # If reconnect requested, exponential backoff.
            $attempt++
            $WsStats[$Name].reconnects++
            $delay = [Math]::Min(30000, 1000 * [Math]::Pow(2, [Math]::Min($attempt, 5)))
            Start-Sleep -Milliseconds ([int]$delay)
        }
    })
    $async = $ps.BeginInvoke()
    return [pscustomobject]@{ ps=$ps; async=$async; runspace=$rs }
}
function Stop-WsStream($handle) {
    try { $handle.ps.Stop() | Out-Null } catch {}
    try { $handle.runspace.Close() } catch {}
}

# ── Mint a ws-token + open both streams ──────────────────────────
Write-Host "=== Phase 1: health + version ==============================="
$health  = (Invoke-WebRequest -UseBasicParsing -Uri "$BaseUri/health").StatusCode
$ready   = (Invoke-WebRequest -UseBasicParsing -Uri "$BaseUri/ready").StatusCode
$version = ((Invoke-WebRequest -UseBasicParsing -Uri "$BaseUri/version").Content | ConvertFrom-Json).version
Write-Host "  health=$health ready=$ready version=$version"

Write-Host ""; Write-Host "=== Phase 2: mint /v2/ws-token + open streams ================"
$mintResp = Send-Signed -Method POST -Path "/v2/ws-token" -Body '{"ws_path":"/ws/order-trace"}' -Subject "test-admin" -Role "admin"
if ($mintResp.status -ne 200) { throw "ws-token mint failed: $($mintResp.body)" }
$mint = $mintResp.body | ConvertFrom-Json
Write-Host "  token len=$($mint.token.Length) ttl=$($mint.ttl_secs)s"

$wsBase = $BaseUri -replace '^http', 'ws'
$traceUri  = "$wsBase/ws/order-trace?token=$([Uri]::EscapeDataString($mint.token))"
$tradesUri = "$wsBase/ws/trades/$([Uri]::EscapeDataString($Market))"
$traceHandle  = Start-WsStream -Name "order-trace" -WsUri $traceUri
$tradesHandle = Start-WsStream -Name "trades"      -WsUri $tradesUri
Start-Sleep -Milliseconds 800   # allow open + ready frames

Write-Host ""; Write-Host "=== Phase 3: capture audit baseline =========================="
$auditBefore = Send-Signed -Method GET -Path "/admin/audit/actions" -Query "limit=200" -Subject "test-admin" -Role "admin"
$auditB = ($auditBefore.body | ConvertFrom-Json).items
Write-Host "  audit rows before: $($auditB.Count)"
$beforeStamp = if ($auditB.Count -gt 0) { $auditB[0].recorded_at } else { "" }

$balanceBefore = (Send-Signed -Method GET -Path "/balances/$User" -Subject $User -Role "user").body
Write-Host "  $User balances before: $balanceBefore"

Drain-Events "after open"

Write-Host ""; Write-Host "=== Phase 4: SCENARIO A — single submit + cancel ============="
$rand = Get-Random -Maximum 99999
# Seed cash so the bid can reserve.
$null = Send-Signed -Method POST -Path "/deposit" -Body ('{"user_id":"' + $User + '","amount":1000000,"op_id":"orch-seed-' + $rand + '"}') -Subject $Admin -Role "admin"
Start-Sleep -Milliseconds $DelayMs

$bidBody = @{ market_id=$Market; side="buy"; order_type="limit"; time_in_force="gtc"; price=49000; amount=1; outcome=0; request_id=("orch-bid-$rand") } | ConvertTo-Json -Compress
$bidResp = Send-Signed -Method POST -Path "/submit-order" -Body $bidBody -Subject $User -Role "user"
$bid = $bidResp.body | ConvertFrom-Json
Write-Host "  submit -> status=$($bidResp.status) order_id=$($bid.order_id) lifecycle=$($bid.lifecycle)"
Start-Sleep -Milliseconds $DelayMs

$cancelBody = @{ market_id=$Market; order_id=$bid.order_id; outcome=0; request_id=("orch-cncl-$rand") } | ConvertTo-Json -Compress
$cncResp = Send-Signed -Method POST -Path "/cancel-order" -Body $cancelBody -Subject $User -Role "user"
Write-Host "  cancel -> status=$($cncResp.status) body=$($cncResp.body)"
Start-Sleep -Milliseconds $DelayMs

Drain-Events "Scenario A"

Write-Host ""; Write-Host "=== Phase 5: SCENARIO B — $ConcurrentOrders concurrent submits ====="
$jobs = 1..$ConcurrentOrders | ForEach-Object {
    $i = $_
    # PS 5.1 doesn't ship Start-ThreadJob; Start-Job spawns a new PS host
    # per job (slower start-up, but the orchestration cares about the
    # request-arrival pattern at the server, not job-spawn perf).
    Start-Job -ScriptBlock {
        param($BaseUri, $Secret, $Market, $User, $Tag, $i, $price, $rand)
        # Re-define helpers inside the job (separate runspace).
        function Sha256Hex([byte[]]$Bytes) { if ($null -eq $Bytes) { $Bytes = [byte[]]@() }; $sha=[System.Security.Cryptography.SHA256]::Create(); try { $h=$sha.ComputeHash($Bytes) } finally { $sha.Dispose() }; [BitConverter]::ToString($h).Replace("-","").ToLowerInvariant() }
        function HmacHex([string]$M, [string]$S) { $hmac=[System.Security.Cryptography.HMACSHA256]::new([System.Text.Encoding]::UTF8.GetBytes($S)); try { $b=$hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($M)) } finally { $hmac.Dispose() }; [BitConverter]::ToString($b).Replace("-","").ToLowerInvariant() }
        $bodyJson = '{"market_id":"' + $Market + '","side":"buy","order_type":"limit","time_in_force":"gtc","price":' + $price + ',"amount":1,"outcome":0,"request_id":"orch-conc-' + $rand + '-' + $i + '"}'
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($bodyJson)
        $ts = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
        $rid = [Guid]::NewGuid().ToString("N")
        $payload = "POST`n/submit-order`n`n$User`nuser`n`n$ts`n$rid"
        $sig = HmacHex $payload $Secret
        $req = [System.Net.HttpWebRequest]::Create("$BaseUri/submit-order")
        $req.Method = "POST"; $req.ContentType = "application/json"
        $req.Headers.Add("x-request-id", $rid)
        $req.Headers.Add("x-internal-auth-subject", $User)
        $req.Headers.Add("x-internal-auth-role", "user")
        $req.Headers.Add("x-internal-auth-session-id", "")
        $req.Headers.Add("x-internal-auth-timestamp", $ts)
        $req.Headers.Add("x-internal-auth-signature", $sig)
        $req.Headers.Add("x-internal-auth-body-sha256", (Sha256Hex $bytes))
        $stream = $req.GetRequestStream(); $stream.Write($bytes, 0, $bytes.Length); $stream.Close()
        try {
            $r = $req.GetResponse(); $rd = [System.IO.StreamReader]::new($r.GetResponseStream())
            return @{ i=$i; status=200; body=$rd.ReadToEnd() }
        } catch [System.Net.WebException] {
            $rsp = $_.Exception.Response; $rd = [System.IO.StreamReader]::new($rsp.GetResponseStream())
            return @{ i=$i; status=[int]$rsp.StatusCode; body=$rd.ReadToEnd() }
        }
    } -ArgumentList $BaseUri, $Secret, $Market, $User, "conc", $i, (48000 + $i), $rand
}
$results = $jobs | Wait-Job | Receive-Job
$results | ForEach-Object {
    $b = if ($_.body) { ($_.body | ConvertFrom-Json) } else { $null }
    Write-Host "  conc[$($_.i)] status=$($_.status) order_id=$($b.order_id) lifecycle=$($b.lifecycle)"
}
$jobs | Remove-Job
Start-Sleep -Milliseconds ($DelayMs * 2)

Drain-Events "Scenario B (concurrent)"

Write-Host ""; Write-Host "=== Phase 6: SCENARIO C — balance change before/after deposit ==="
$balMid = (Send-Signed -Method GET -Path "/balances/$User" -Subject $User -Role "user").body
Write-Host "  balance before deposit2: $balMid"
$null = Send-Signed -Method POST -Path "/deposit" -Body ('{"user_id":"' + $User + '","amount":777,"op_id":"orch-bal-' + $rand + '"}') -Subject $Admin -Role "admin"
Start-Sleep -Milliseconds $DelayMs
$balAfter = (Send-Signed -Method GET -Path "/balances/$User" -Subject $User -Role "user").body
Write-Host "  balance after  deposit2: $balAfter"

Drain-Events "Scenario C (balance)"

Write-Host ""; Write-Host "=== Phase 7: SCENARIO D — role-switch /balances ============="
# Same path /balances/<id>; first as the user themself, then as admin reading
# the same id. Both should succeed; admin sees the same data + adds an admin
# cross-account audit row in the server log (per security.rs).
$asUser  = Send-Signed -Method GET -Path "/balances/$User" -Subject $User  -Role "user"
$asAdmin = Send-Signed -Method GET -Path "/balances/$User" -Subject $Admin -Role "admin"
Write-Host "  as user  -> $($asUser.status): $($asUser.body)"
Write-Host "  as admin -> $($asAdmin.status): $($asAdmin.body)"
Start-Sleep -Milliseconds $DelayMs

# Negative path: non-admin reading someone else's balance MUST be denied.
$crossDenied = Send-Signed -Method GET -Path "/balances/other-user-not-me" -Subject $User -Role "user"
Write-Host "  as user reading OTHER user -> $($crossDenied.status) (expected 403)"

Drain-Events "Scenario D (role-switch)"

Write-Host ""; Write-Host "=== Phase 8: audit diff ======================================"
$auditAfter = Send-Signed -Method GET -Path "/admin/audit/actions" -Query "limit=200" -Subject "test-admin" -Role "admin"
$auditA = ($auditAfter.body | ConvertFrom-Json).items
Write-Host "  audit rows after: $($auditA.Count) (delta = $($auditA.Count - $auditB.Count))"
$newRows = $auditA | Where-Object { $_.recorded_at -gt $beforeStamp }
$newRows | Sort-Object recorded_at | Select-Object -First 30 | ForEach-Object {
    Write-Host ("    + {0} {1,-15} subject={2,-12} role={3}" -f `
        $_.recorded_at.Substring(11, 12), $_.action, $_.subject, $_.role)
}

Write-Host ""; Write-Host "=== Phase 9: WS stats + teardown =============================="
foreach ($k in $script:WsStats.Keys) {
    $s = $script:WsStats[$k]
    Write-Host ("  {0,-12} opens={1} reconnects={2} errors={3} closes={4}" -f `
        $k, $s.opens, $s.reconnects, $s.errors, $s.closes)
}
$prom = (Invoke-WebRequest -UseBasicParsing -Uri "$BaseUri/metrics/prometheus").Content
($prom -split "`n") | Where-Object { $_ -match "^exchange_(orders_received|orders_filled|orders_rejected|orders_cancelled|ws_connections_active|http_requests_total)_" } | ForEach-Object { Write-Host "  $_" }

Stop-WsStream $traceHandle
Stop-WsStream $tradesHandle

Write-Host ""; Write-Host "=== ORCHESTRATION COMPLETE ==="
