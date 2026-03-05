# Full Platform Test Script
# Tests all API endpoints

Write-Host "`n========================================"
Write-Host "  Prediction Platform - Full Test"
Write-Host "========================================"

$API_BASE = "http://localhost:8080"
$passed = 0
$failed = 0

function Test-Endpoint {
    param($Name, $Url)
    
    Write-Host "Testing: $Name... " -NoNewline
    
    try {
        $response = Invoke-WebRequest -Uri $Url -Method GET -UseBasicParsing -TimeoutSec 10
        
        if ($response.StatusCode -eq 200) {
            Write-Host "[PASS]" -ForegroundColor Green
            $script:passed++
            return ($response.Content | ConvertFrom-Json)
        } else {
            Write-Host "[FAIL]" -ForegroundColor Red
            $script:failed++
            return $null
        }
    } catch {
        Write-Host "[FAIL]" -ForegroundColor Red
        $script:failed++
        return $null
    }
}

Write-Host "`nAPI Endpoints:"
Write-Host "----------------------------------------"

$markets = Test-Endpoint "GET /v1/markets" "$API_BASE/v1/markets"
if ($markets) { Write-Host "   Found $($markets.Count) markets" }

if ($markets -and $markets.Count -gt 0) {
    $marketId = $markets[0].ID
    Test-Endpoint "GET /v1/markets/$marketId" "$API_BASE/v1/markets/$marketId"
    
    $orderBook = Test-Endpoint "GET /v1/markets/$marketId/book" "$API_BASE/v1/markets/$marketId/book"
    if ($orderBook) { Write-Host "   Bids: $($orderBook.bids.Count), Asks: $($orderBook.asks.Count)" }
}

Test-Endpoint "GET /v1/balances" "$API_BASE/v1/balances?user_id=user1"

Write-Host "`nAdmin Endpoints:"
Write-Host "----------------------------------------"

$stats = Test-Endpoint "GET /admin/stats" "$API_BASE/admin/stats"
if ($stats) { Write-Host "   Volume: $($stats.volume_24h), Users: $($stats.total_users)" }

Test-Endpoint "GET /admin/markets" "$API_BASE/admin/markets"
Test-Endpoint "GET /admin/users" "$API_BASE/admin/users"
Test-Endpoint "GET /admin/trades" "$API_BASE/admin/trades"

Write-Host "`n========================================"
Write-Host "  Test Results"
Write-Host "========================================"
Write-Host "  Passed: $passed" -ForegroundColor Green
Write-Host "  Failed: $failed" -ForegroundColor $(if ($failed -gt 0) { "Red" } else { "Green" })

if ($failed -eq 0) {
    Write-Host "`nAll tests passed! Platform is ready."
    Write-Host "Homepage:  http://localhost:8080/"
    Write-Host "Trading:   http://localhost:8080/app.html"
    Write-Host "Admin:     http://localhost:8080/admin.html"
}
