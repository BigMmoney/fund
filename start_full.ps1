# Complete Platform Startup Script

Write-Host "🚀 Starting Prediction Platform..." -ForegroundColor Green
Write-Host ""

# Set Go proxy
$env:GOPROXY = "https://goproxy.cn,direct"

# Create directories
Write-Host "📁 Creating directories..." -ForegroundColor Yellow
New-Item -ItemType Directory -Force -Path "d:\pre_trading\logs","d:\pre_trading\data\ledger_wal" | Out-Null

# Check if API is already running
$apiRunning = Get-Process | Where-Object {$_.ProcessName -like "*go*" -and $_.Path -like "*api*"}
if ($apiRunning) {
    Write-Host "⚠️  API service already running. Stopping..." -ForegroundColor Yellow
    Stop-Process -Name "go" -Force -ErrorAction SilentlyContinue
    Start-Sleep -Seconds 2
}

# Start API service
Write-Host ""
Write-Host "🔧 Starting API Service..." -ForegroundColor Cyan
Start-Process powershell -ArgumentList "-NoExit", "-Command", "cd d:\pre_trading\api; Write-Host 'API Gateway Starting...' -ForegroundColor Green; `$env:GOPROXY='https://goproxy.cn,direct'; go run main.go"

Start-Sleep -Seconds 5

# Check if API is responding
Write-Host "🔍 Checking API health..." -ForegroundColor Yellow
$maxAttempts = 10
$attempt = 0
$apiReady = $false

while ($attempt -lt $maxAttempts -and -not $apiReady) {
    try {
        $response = Invoke-RestMethod -Uri "http://localhost:8080/health" -TimeoutSec 2 -ErrorAction Stop
        if ($response.status -eq "healthy") {
            $apiReady = $true
            Write-Host "✅ API is ready!" -ForegroundColor Green
        }
    } catch {
        $attempt++
        Write-Host "   Attempt $attempt/$maxAttempts..." -ForegroundColor Gray
        Start-Sleep -Seconds 2
    }
}

if (-not $apiReady) {
    Write-Host "❌ API failed to start. Check logs." -ForegroundColor Red
    exit 1
}

# Install frontend dependencies if needed
Write-Host ""
Write-Host "📦 Checking frontend dependencies..." -ForegroundColor Cyan
if (-not (Test-Path "d:\pre_trading\frontend\node_modules")) {
    Write-Host "Installing Node.js dependencies..." -ForegroundColor Yellow
    Set-Location "d:\pre_trading\frontend"
    npm install
}

# Start frontend
Write-Host ""
Write-Host "🎨 Starting Frontend..." -ForegroundColor Cyan
Start-Process powershell -ArgumentList "-NoExit", "-Command", "cd d:\pre_trading\frontend; Write-Host 'Frontend Starting...' -ForegroundColor Green; npm start"

Write-Host ""
Write-Host "=" * 60 -ForegroundColor Green
Write-Host "✨ Platform Started Successfully!" -ForegroundColor Green
Write-Host "=" * 60 -ForegroundColor Green
Write-Host ""
Write-Host "🌐 Services:" -ForegroundColor Yellow
Write-Host "   Backend API:  http://localhost:8080" -ForegroundColor White
Write-Host "   Frontend:     http://localhost:3000" -ForegroundColor White
Write-Host "   WebSocket:    ws://localhost:8080/ws" -ForegroundColor White
Write-Host ""
Write-Host "📊 API Endpoints:" -ForegroundColor Yellow
Write-Host "   GET  /v1/markets              - List markets" -ForegroundColor White
Write-Host "   GET  /v1/markets/:id/book     - Get order book" -ForegroundColor White
Write-Host "   GET  /v1/balances             - Get user balance" -ForegroundColor White
Write-Host "   POST /v1/intents              - Place order" -ForegroundColor White
Write-Host "   POST /v1/orders/:id/cancel    - Cancel order" -ForegroundColor White
Write-Host ""
Write-Host "💡 Tips:" -ForegroundColor Yellow
Write-Host "   - Frontend will open automatically at http://localhost:3000" -ForegroundColor White
Write-Host "   - Use the web interface to trade" -ForegroundColor White
Write-Host "   - Check ./logs/ for service logs" -ForegroundColor White
Write-Host ""

# Wait a bit for frontend to start
Start-Sleep -Seconds 8

# Try to open browser
Write-Host "🌐 Opening browser..." -ForegroundColor Cyan
try {
    Start-Process "http://localhost:3000"
} catch {
    Write-Host "Please manually open: http://localhost:3000" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "Press any key to exit..." -ForegroundColor Gray
$null = $Host.UI.RawUI.ReadKey("NoEcho,IncludeKeyDown")
