param(
    [string]$Port = "3050",
    [string]$Market = "btc-usdt",
    [string]$User = "alice"
)

$ErrorActionPreference = 'Stop'

# Helper: submit order
function Submit-Order {
    param($OrderId, $ClientOrderId, $Side, $Price, $Amount, $RequestId)
    $body = @{
        market_id = $Market
        side = $Side
        price = $Price
        amount = $Amount
        order_type = "Limit"
        order_id = $OrderId
        client_order_id = $ClientOrderId
        request_id = $RequestId
        user_id = $User
    } | ConvertTo-Json -Compress
    
    try {
        $resp = Invoke-WebRequest -Uri "http://127.0.0.1:$Port/submit-order" -Method POST -ContentType "application/json" -Body $body -UseBasicParsing
        return @{ Status = $resp.StatusCode; Content = $resp.Content }
    } catch {
        $status = $_.Exception.Response.StatusCode.value__
        $reader = New-Object System.IO.StreamReader($_.Exception.Response.GetResponseStream())
        $body_content = $reader.ReadToEnd()
        $reader.Close()
        return @{ Status = $status; Content = $body_content }
    }
}

# Helper: cancel order
function Cancel-Order {
    param($OrderId, $ClientOrderId, $RequestId)
    $body = @{
        market_id = $Market
        order_id = $OrderId
        client_order_id = $ClientOrderId
        request_id = $RequestId
    } | ConvertTo-Json -Compress
    
    try {
        $resp = Invoke-WebRequest -Uri "http://127.0.0.1:$Port/cancel-order" -Method POST -ContentType "application/json" -Body $body -UseBasicParsing
        return @{ Status = $resp.StatusCode; Content = $resp.Content }
    } catch {
        $status = $_.Exception.Response.StatusCode.value__
        $reader = New-Object System.IO.StreamReader($_.Exception.Response.GetResponseStream())
        $body_content = $reader.ReadToEnd()
        $reader.Close()
        return @{ Status = $status; Content = $body_content }
    }
}

# Helper: replace order
function Replace-Order {
    param($OrderId, $ClientOrderId, $RequestId, $NewPrice, $NewAmount)
    $body = @{
        market_id = $Market
        order_id = $OrderId
        client_order_id = $ClientOrderId
        request_id = $RequestId
        new_price = $NewPrice
        new_amount = $NewAmount
    } | ConvertTo-Json -Compress
    
    try {
        $resp = Invoke-WebRequest -Uri "http://127.0.0.1:$Port/replace-order" -Method POST -ContentType "application/json" -Body $body -UseBasicParsing
        return @{ Status = $resp.StatusCode; Content = $resp.Content }
    } catch {
        $status = $_.Exception.Response.StatusCode.value__
        $reader = New-Object System.IO.StreamReader($_.Exception.Response.GetResponseStream())
        $body_content = $reader.ReadToEnd()
        $reader.Close()
        return @{ Status = $status; Content = $body_content }
    }
}

Write-Host "=== Test 4: Cancel-Replace Scenario ==="
Write-Host ""

# Step 1: Submit initial order
Write-Host "Step 1: Submit initial order (t4-order-1)..."
$r1 = Submit-Order -OrderId "t4-order-1" -ClientOrderId "t4-client-1" -Side "Buy" -Price 45000 -Amount 500 -RequestId "t4-req-1"
Write-Host "  Status: $($r1.Status)"
if ($r1.Status -ne 200) {
    Write-Host "  FAIL: Expected 200, got $($r1.Status)"
    Write-Host "  Body: $($r1.Content)"
    exit 1
}
Write-Host "  PASS: Order submitted successfully"

# Step 2: Cancel the order
Write-Host ""
Write-Host "Step 2: Cancel order (t4-order-1)..."
$r2 = Cancel-Order -OrderId "t4-order-1" -ClientOrderId "t4-client-1" -RequestId "t4-req-2"
Write-Host "  Status: $($r2.Status)"
if ($r2.Status -ne 200) {
    Write-Host "  FAIL: Expected 200, got $($r2.Status)"
    Write-Host "  Body: $($r2.Content)"
    exit 1
}
Write-Host "  PASS: Order cancelled successfully"

# Step 3: Submit replacement order
Write-Host ""
Write-Host "Step 3: Submit replacement order (t4-order-2)..."
$r3 = Replace-Order -OrderId "t4-order-1" -ClientOrderId "t4-client-1" -RequestId "t4-req-3" -NewPrice 45500 -NewAmount 0.3
Write-Host "  Status: $($r3.Status)"
if ($r3.Status -ne 200) {
    Write-Host "  FAIL: Expected 200, got $($r3.Status)"
    Write-Host "  Body: $($r3.Content)"
    exit 1
}
Write-Host "  PASS: Order replaced successfully"

# Step 4: Try to cancel already-cancelled order (should get business rejection, NOT 500)
Write-Host ""
Write-Host "Step 4: Cancel already-cancelled order (expect rejection, NOT 500)..."
$r4 = Cancel-Order -OrderId "t4-order-1" -ClientOrderId "t4-client-1" -RequestId "t4-req-4"
Write-Host "  Status: $($r4.Status)"
if ($r4.Status -eq 500) {
    Write-Host "  FAIL: Got 500 for duplicate cancel - this is the bug!"
    Write-Host "  Body: $($r4.Content)"
    exit 1
}
Write-Host "  PASS: Got $($r4.Status) (expected 400 or similar, not 500)"

# Step 5: Submit another fresh order to confirm system still works
Write-Host ""
Write-Host "Step 5: Submit fresh order after cancel-replace cycle..."
$r5 = Submit-Order -OrderId "t4-order-3" -ClientOrderId "t4-client-3" -Side "Sell" -Price 46000 -Amount 200 -RequestId "t4-req-5"
Write-Host "  Status: $($r5.Status)"
if ($r5.Status -ne 200) {
    Write-Host "  FAIL: Expected 200, got $($r5.Status)"
    Write-Host "  Body: $($r5.Content)"
    exit 1
}
Write-Host "  PASS: Fresh order submitted successfully"

Write-Host ""
Write-Host "=== Test 4 COMPLETE: All steps passed ==="
Write-Host "Summary:"
Write-Host "  Step 1 (Submit):     $($r1.Status)"
Write-Host "  Step 2 (Cancel):     $($r2.Status)"
Write-Host "  Step 3 (Replace):    $($r3.Status)"
Write-Host "  Step 4 (Dup Cancel): $($r4.Status) (expected != 500)"
Write-Host "  Step 5 (Fresh):      $($r5.Status)"
