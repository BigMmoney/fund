# Prediction Platform Startup Script for Windows

Write-Host "Starting Prediction Platform Services..." -ForegroundColor Green

# Check if Go is installed
if (!(Get-Command go -ErrorAction SilentlyContinue)) {
    Write-Host "Error: Go is not installed or not in PATH" -ForegroundColor Red
    exit 1
}

Write-Host "`nInitializing Go modules..." -ForegroundColor Yellow
go mod tidy

# Create data directories
Write-Host "`nCreating data directories..." -ForegroundColor Yellow
New-Item -ItemType Directory -Force -Path ".\data\ledger_wal" | Out-Null
New-Item -ItemType Directory -Force -Path ".\logs" | Out-Null

# Start services in background
Write-Host "`nStarting services..." -ForegroundColor Yellow

$services = @(
    @{Name="Ledger"; Path=".\ledger"; Port=8081},
    @{Name="Matching"; Path=".\matching"; Port=8082},
    @{Name="Indexer"; Path=".\indexer"; Port=8083},
    @{Name="Risk"; Path=".\risk"; Port=8084},
    @{Name="API"; Path=".\api"; Port=8080}
)

$jobs = @()

foreach ($service in $services) {
    Write-Host "  Starting $($service.Name) service on port $($service.Port)..." -ForegroundColor Cyan
    
    $job = Start-Job -ScriptBlock {
        param($path, $name)
        Set-Location $path
        go run main.go 2>&1 | Out-File -FilePath "..\logs\$name.log" -Append
    } -ArgumentList $service.Path, $service.Name
    
    $jobs += @{Job=$job; Name=$service.Name}
    Start-Sleep -Milliseconds 500
}

Write-Host "`n✓ All services started!" -ForegroundColor Green
Write-Host "`nService Status:" -ForegroundColor Yellow
Write-Host "  API Gateway:     http://localhost:8080" -ForegroundColor White
Write-Host "  Ledger Service:  Running on :8081" -ForegroundColor White
Write-Host "  Matching Engine: Running on :8082" -ForegroundColor White
Write-Host "  Indexer Service: Running on :8083" -ForegroundColor White
Write-Host "  Risk Service:    Running on :8084" -ForegroundColor White

Write-Host "`nWebSocket endpoint: ws://localhost:8080/ws" -ForegroundColor Cyan
Write-Host "Health check:       http://localhost:8080/health" -ForegroundColor Cyan

Write-Host "`nLogs are being written to ./logs/" -ForegroundColor Yellow
Write-Host "`nPress Ctrl+C to stop all services...`n" -ForegroundColor Red

# Monitor services
try {
    while ($true) {
        Start-Sleep -Seconds 5
        
        $allRunning = $true
        foreach ($item in $jobs) {
            if ($item.Job.State -ne "Running") {
                Write-Host "Warning: $($item.Name) service stopped!" -ForegroundColor Red
                $allRunning = $false
            }
        }
        
        if (-not $allRunning) {
            Write-Host "Some services have stopped. Check logs for details." -ForegroundColor Red
            break
        }
    }
}
finally {
    Write-Host "`nStopping all services..." -ForegroundColor Yellow
    foreach ($item in $jobs) {
        Stop-Job -Job $item.Job
        Remove-Job -Job $item.Job
    }
    Write-Host "All services stopped." -ForegroundColor Green
}
