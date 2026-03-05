# API Test Script for Prediction Platform

$baseUrl = "http://localhost:8080"

Write-Host "Testing Prediction Platform API..." -ForegroundColor Green
Write-Host "Base URL: $baseUrl`n" -ForegroundColor Cyan

# Test 1: Health Check
Write-Host "1. Testing Health Check..." -ForegroundColor Yellow
try {
    $response = Invoke-RestMethod -Uri "$baseUrl/health" -Method Get
    Write-Host "   ✓ Health check passed: $($response.status)" -ForegroundColor Green
} catch {
    Write-Host "   ✗ Health check failed: $_" -ForegroundColor Red
}

Start-Sleep -Seconds 1

# Test 2: Get Markets
Write-Host "`n2. Testing Get Markets..." -ForegroundColor Yellow
try {
    $markets = Invoke-RestMethod -Uri "$baseUrl/v1/markets" -Method Get
    Write-Host "   ✓ Retrieved $($markets.Count) markets" -ForegroundColor Green
    foreach ($market in $markets) {
        Write-Host "     - $($market.name) (ID: $($market.id), State: $($market.state))" -ForegroundColor White
    }
} catch {
    Write-Host "   ✗ Get markets failed: $_" -ForegroundColor Red
}

Start-Sleep -Seconds 1

# Test 3: Get Balances
Write-Host "`n3. Testing Get Balances..." -ForegroundColor Yellow
try {
    $balances = Invoke-RestMethod -Uri "$baseUrl/v1/balances?user_id=user1" -Method Get
    Write-Host "   ✓ Retrieved balances for user1" -ForegroundColor Green
    foreach ($balance in $balances) {
        Write-Host "     - $($balance.asset): Available=$($balance.available), Hold=$($balance.hold)" -ForegroundColor White
    }
} catch {
    Write-Host "   ✗ Get balances failed: $_" -ForegroundColor Red
}

Start-Sleep -Seconds 1

# Test 4: Create Intent (Buy)
Write-Host "`n4. Testing Create Intent (Buy)..." -ForegroundColor Yellow
$intentBody = @{
    user_id = "user1"
    market_id = "market1"
    side = "buy"
    price = 55
    amount = 1000
    outcome = 0
    expires_in = 60
} | ConvertTo-Json

try {
    $intent = Invoke-RestMethod -Uri "$baseUrl/v1/intents" -Method Post -Body $intentBody -ContentType "application/json"
    Write-Host "   ✓ Intent created: $($intent.intent_id)" -ForegroundColor Green
    Write-Host "     - Status: $($intent.status)" -ForegroundColor White
    Write-Host "     - Created: $($intent.created_at)" -ForegroundColor White
    $buyIntentId = $intent.intent_id
} catch {
    Write-Host "   ✗ Create intent failed: $_" -ForegroundColor Red
}

Start-Sleep -Seconds 1

# Test 5: Create Intent (Sell)
Write-Host "`n5. Testing Create Intent (Sell)..." -ForegroundColor Yellow
$intentBody2 = @{
    user_id = "user2"
    market_id = "market1"
    side = "sell"
    price = 54
    amount = 800
    outcome = 0
    expires_in = 60
} | ConvertTo-Json

try {
    $intent2 = Invoke-RestMethod -Uri "$baseUrl/v1/intents" -Method Post -Body $intentBody2 -ContentType "application/json"
    Write-Host "   ✓ Intent created: $($intent2.intent_id)" -ForegroundColor Green
    Write-Host "     - Status: $($intent2.status)" -ForegroundColor White
    $sellIntentId = $intent2.intent_id
} catch {
    Write-Host "   ✗ Create intent failed: $_" -ForegroundColor Red
}

Start-Sleep -Seconds 2

# Test 6: Get Order Book
Write-Host "`n6. Testing Get Order Book..." -ForegroundColor Yellow
try {
    $orderBook = Invoke-RestMethod -Uri "$baseUrl/v1/markets/market1/book" -Method Get
    Write-Host "   ✓ Order book retrieved for market1" -ForegroundColor Green
    Write-Host "     - Bids: $($orderBook.bids.Count)" -ForegroundColor White
    Write-Host "     - Asks: $($orderBook.asks.Count)" -ForegroundColor White
} catch {
    Write-Host "   ✗ Get order book failed: $_" -ForegroundColor Red
}

Start-Sleep -Seconds 1

# Test 7: Cancel Order
if ($buyIntentId) {
    Write-Host "`n7. Testing Cancel Order..." -ForegroundColor Yellow
    try {
        $cancel = Invoke-RestMethod -Uri "$baseUrl/v1/orders/$buyIntentId/cancel" -Method Post
        Write-Host "   ✓ Order cancelled: $($cancel.status)" -ForegroundColor Green
    } catch {
        Write-Host "   ✗ Cancel order failed: $_" -ForegroundColor Red
    }
}

Start-Sleep -Seconds 1

# Test 8: Request Withdrawal
Write-Host "`n8. Testing Withdrawal Request..." -ForegroundColor Yellow
$withdrawalBody = @{
    user_id = "user1"
    amount = 500
    address = "0x1234567890abcdef1234567890abcdef12345678"
} | ConvertTo-Json

try {
    $withdrawal = Invoke-RestMethod -Uri "$baseUrl/v1/withdrawals" -Method Post -Body $withdrawalBody -ContentType "application/json"
    Write-Host "   ✓ Withdrawal requested: $($withdrawal.withdrawal_id)" -ForegroundColor Green
    Write-Host "     - Status: $($withdrawal.status)" -ForegroundColor White
} catch {
    Write-Host "   ✗ Withdrawal request failed: $_" -ForegroundColor Red
}

Write-Host "`n" + "="*60 -ForegroundColor Cyan
Write-Host "API Testing Complete!" -ForegroundColor Green
Write-Host "="*60 -ForegroundColor Cyan

Write-Host "`nNext Steps:" -ForegroundColor Yellow
Write-Host "  1. Check service logs in ./logs/" -ForegroundColor White
Write-Host "  2. Connect to WebSocket: ws://localhost:8080/ws" -ForegroundColor White
Write-Host "  3. Monitor matching engine for fills" -ForegroundColor White
Write-Host "  4. Review ledger state for balance updates`n" -ForegroundColor White
