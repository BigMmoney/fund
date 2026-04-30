# Test InsufficientFunds returns HTTP 400 instead of 500

$ErrorActionPreference = "Stop"

$Secret = "dev-secret-change-me-to-32-chars-min!"
$Subject = "user-test-123"
$Role = "user"
$SessionId = ""

# Build the signing payload
# Submit an order that should trigger InsufficientFunds (buying with zero balance)
$BodyJson = '{"market_id":"btc-usdt","side":"buy","order_type":"limit","price":50000,"amount":10,"outcome":0,"time_in_force":"gtc"}'
$bodyBytes = [System.Text.Encoding]::UTF8.GetBytes($BodyJson)

# Write body to temp file (exact bytes, no BOM)
$RequestId = [guid]::NewGuid().ToString()
$tempFile = [System.IO.Path]::Combine([System.IO.Path]::GetTempPath(), "test_order_$RequestId.json")
[System.IO.File]::WriteAllBytes($tempFile, $bodyBytes)

# Compute SHA256 of the request body
$sha256 = [System.Security.Cryptography.SHA256]::Create()
$bodyHash = [BitConverter]::ToString($sha256.ComputeHash($bodyBytes)).Replace("-", "").ToLowerInvariant()

# Refresh timestamp and compute signature right before request (within 30s skew window)
$Timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()

$Method = "POST"
$Path = "/submit-order"
$Query = ""

$Payload = "$Method`n$Path`n$Query`n$Subject`n$Role`n$SessionId`n$Timestamp`n$RequestId"

# Compute HMAC-SHA256
$hmac = New-Object System.Security.Cryptography.HMACSHA256
$hmac.Key = [System.Text.Encoding]::UTF8.GetBytes($Secret)
$signatureBytes = $hmac.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($Payload))
$Signature = [BitConverter]::ToString($signatureBytes).Replace("-", "").ToLowerInvariant()

Write-Host "Timestamp: $Timestamp"
Write-Host "RequestId: $RequestId"
Write-Host "Signature: $Signature"
Write-Host "Body SHA256: $bodyHash"
Write-Host ""
Write-Host "Body file: $tempFile"
Write-Host ""

# Build curl headers
$curlHeaders = @(
    "-H", "Content-Type: application/json",
    "-H", "x-request-id: $RequestId",
    "-H", "x-internal-auth-subject: $Subject",
    "-H", "x-internal-auth-role: $Role",
    "-H", "x-internal-auth-session-id: $SessionId",
    "-H", "x-internal-auth-timestamp: $Timestamp",
    "-H", "x-internal-auth-signature: $Signature",
    "-H", "x-internal-auth-body-sha256: $bodyHash"
)

try {
    Write-Host "Submitting order (expecting InsufficientFunds -> HTTP 400)..."
    
    # Use curl with --data-binary to send exact file bytes
    $curlArgs = @("-s", "-w", "\n%{http_code}") + $curlHeaders + @("--data-binary", "@$tempFile", "http://127.0.0.1:3031/submit-order")
    $curlOutput = & curl.exe $curlArgs 2>&1
    
    # Parse response: last line is status code, rest is body
    $lines = $curlOutput -split "`n"
    $statusCode = [int]$lines[-1].Trim()
    $responseBody = ($lines[0..($lines.Length-2)] -join "`n").Trim()
    
    # Clean up temp file
    Remove-Item $tempFile -ErrorAction SilentlyContinue
    
    Write-Host "Response Status: $statusCode" -ForegroundColor $(if ($statusCode -eq 400) { "Green" } else { "Red" })
    Write-Host "Response Body: $responseBody"
    
    if ($statusCode -eq 400 -and $responseBody -match "InsufficientFunds") {
        Write-Host "`nSUCCESS: InsufficientFunds correctly returns HTTP 400!" -ForegroundColor Green
    } elseif ($statusCode -eq 500) {
        Write-Host "`nFAILURE: Still returning HTTP 500!" -ForegroundColor Red
    } else {
        Write-Host "`nUNEXPECTED: Got status $statusCode" -ForegroundColor Yellow
    }
} catch {
    Remove-Item $tempFile -ErrorAction SilentlyContinue
    Write-Host "ERROR: $_" -ForegroundColor Red
    throw
}
