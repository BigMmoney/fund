param([int]$Count = 30, [string]$Market = "btc-usdt", [int]$BasePrice = 50000, [string]$Port = "3050", [string]$Prefix = "t1")
$secret = "dev-secret-change-me-to-32-chars-min!"
$results = @{}
$errors = @()
for ($i = 1; $i -le $Count; $i++) {
    $price = $BasePrice + $i
    $order = @{ client_order_id = [Guid]::NewGuid().ToString("N"); market_id = $Market; side = "buy"; order_type = "limit"; price = $price; amount = 1; outcome = 1; time_in_force = "gtc" } | ConvertTo-Json -Compress
    $bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($order)
    $ts = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds().ToString()
    $rid = "$Prefix-$i"
    $payload = "POST`n/submit-order`n`nadmin`nadmin`n`n$ts`n$rid"
    $hmac = [System.Security.Cryptography.HMACSHA256]::new([System.Text.Encoding]::UTF8.GetBytes($secret))
    $sig = [BitConverter]::ToString($hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($payload))).Replace("-","").ToLower()
    $hmac.Dispose()
    $sha = [System.Security.Cryptography.SHA256]::Create()
    $bh = [BitConverter]::ToString($sha.ComputeHash($bodyBytes)).Replace("-","").ToLowerInvariant()
    $sha.Dispose()
    try {
        $resp = Invoke-WebRequest -Uri "http://localhost:$Port/submit-order" -Method POST -Headers @{"Content-Type"="application/json"; "x-internal-auth-subject"="admin"; "x-internal-auth-role"="admin"; "x-internal-auth-session-id"=""; "x-internal-auth-timestamp"=$ts; "x-internal-auth-signature"=$sig; "x-internal-auth-body-sha256"=$bh; "x-request-id"=$rid} -Body $order -UseBasicParsing
        $results["200"] = ($results["200"], 0 | Where-Object { $_ -ne $null } | Measure-Object -Maximum).Maximum + 1
    } catch {
        $sc = $_.Exception.Response.StatusCode.value__
        $scStr = $sc.ToString()
        try {
            $reader = [System.IO.StreamReader]::new($_.Exception.Response.GetResponseStream())
            $errBody = $reader.ReadToEnd()
            $reader.Close()
        } catch { $errBody = "N/A" }
        $results[$scStr] = ($results[$scStr], 0 | Where-Object { $_ -ne $null } | Measure-Object -Maximum).Maximum + 1
        if ($sc -eq 500) { $errors += "Order $i : HTTP $sc - $errBody" }
    }
}
Write-Host "Results: $($Count) orders"
foreach ($k in $results.Keys | Sort-Object) { Write-Host "  HTTP $k = $($results[$k])" }
if ($errors.Count -gt 0) { Write-Host "500 errors:"; $errors | ForEach-Object { Write-Host "  $_" } }
