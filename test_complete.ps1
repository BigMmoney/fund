# Complete Platform Test Script

Write-Host "🧪 Testing Prediction Platform API..." -ForegroundColor Green
Write-Host ""

$baseUrl = "http://localhost:8080"
$testsPassed = 0
$testsFailed = 0

function Test-Endpoint {
    param($Name, $ScriptBlock)
    Write-Host "Testing: $Name" -ForegroundColor Yellow
    try {
        & $ScriptBlock
        Write-Host "  ✅ PASSED`n" -ForegroundColor Green
        $script:testsPassed++
    } catch {
        Write-Host "  ❌ FAILED: $_`n" -ForegroundColor Red
        $script:testsFailed++
    }
}

# Test 1: Health Check
Test-Endpoint "Health Check" {
    $response = Invoke-RestMethod -Uri "$baseUrl/health" -UseBasicParsing
    if ($response.status -ne "healthy") {
        throw "Health check failed"
    }
    Write-Host "  Status: $($response.status)" -ForegroundColor White
}

# Test 2: Get Markets
Test-Endpoint "Get Markets" {
    $markets = Invoke-RestMethod -Uri "$baseUrl/v1/markets" -UseBasicParsing
    Write-Host "  Found $($markets.Count) markets" -ForegroundColor White
    foreach ($market in $markets) {
        Write-Host "    - $($market.name)" -ForegroundColor Gray
    }
}

# Test 3: Get Balances
Test-Endpoint "Get Balances" {
    $balances = Invoke-RestMethod -Uri "$baseUrl/v1/balances?user_id=user1" -UseBasicParsing
    Write-Host "  User: user1" -ForegroundColor White
    foreach ($bal in $balances) {
        Write-Host "    - $($bal.asset): Available=$($bal.available/100), Hold=$($bal.hold/100)" -ForegroundColor Gray
    }
}

# Test 4: Get Order Book
Test-Endpoint "Get Order Book" {
    $book = Invoke-RestMethod -Uri "$baseUrl/v1/markets/market1/book" -UseBasicParsing
    Write-Host "  Bids: $($book.bids.Count), Asks: $($book.asks.Count)" -ForegroundColor White
}

# Test 5: Place Buy Order
Test-Endpoint "Place Buy Order" {
    $body = @{
        user_id = "user1"
        market_id = "market1"
        side = "buy"
        price = 55
        amount = 1000
        outcome = 0
        expires_in = 60
    } | ConvertTo-Json

    $intent = Invoke-RestMethod -Uri "$baseUrl/v1/intents" -Method Post -Body $body -ContentType "application/json" -UseBasicParsing
    Write-Host "  Intent ID: $($intent.intent_id)" -ForegroundColor White
    Write-Host "  Status: $($intent.status)" -ForegroundColor White
}

# Test 6: Place Sell Order
Test-Endpoint "Place Sell Order" {
    $body = @{
        user_id = "user2"
        market_id = "market1"
        side = "sell"
        price = 54
        amount = 800
        outcome = 0
        expires_in = 60
    } | ConvertTo-Json

    $intent = Invoke-RestMethod -Uri "$baseUrl/v1/intents" -Method Post -Body $body -ContentType "application/json" -UseBasicParsing
    Write-Host "  Intent ID: $($intent.intent_id)" -ForegroundColor White
}

# Test 7: Frontend Access
Test-Endpoint "Frontend Access" {
    $response = Invoke-WebRequest -Uri "$baseUrl/app.html" -UseBasicParsing
    if ($response.StatusCode -ne 200) {
        throw "Frontend not accessible"
    }
    Write-Host "  Frontend loaded successfully" -ForegroundColor White
}

# Summary
Write-Host ""
Write-Host "=" * 60 -ForegroundColor Cyan
Write-Host "Test Results:" -ForegroundColor Yellow
Write-Host "  ✅ Passed: $testsPassed" -ForegroundColor Green
Write-Host "  ❌ Failed: $testsFailed" -ForegroundColor Red
Write-Host "=" * 60 -ForegroundColor Cyan

if ($testsFailed -eq 0) {
    Write-Host ""
    Write-Host "🎉 All tests passed! Platform is working correctly." -ForegroundColor Green
    Write-Host ""
    Write-Host "🌐 Access the platform:" -ForegroundColor Yellow
    Write-Host "   Frontend: http://localhost:8080/app.html" -ForegroundColor White
    Write-Host "   API:      http://localhost:8080/v1/" -ForegroundColor White
    Write-Host "   WebSocket: ws://localhost:8080/ws" -ForegroundColor White
} else {
    Write-Host ""
    Write-Host "⚠️  Some tests failed. Check the output above." -ForegroundColor Yellow
}

Write-Host ""
