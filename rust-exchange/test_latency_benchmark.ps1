# Test the comprehensive latency benchmark
Write-Host "=== Testing Comprehensive Latency Benchmark ===" -ForegroundColor Cyan
Write-Host ""

Set-Location "d:\pre_trading\rust-exchange"

Write-Host "Step 1: Checking benchmark compilation..." -ForegroundColor Yellow
cargo check --package matching --bench comprehensive_latency 2>&1 | Select-Object -Last 20

if ($LASTEXITCODE -ne 0) {
    Write-Host "Compilation failed!" -ForegroundColor Red
    exit 1
}

Write-Host ""
Write-Host "Step 2: Running benchmark tests..." -ForegroundColor Yellow
cargo test --package matching --bench comprehensive_latency 2>&1 | Select-Object -Last 50

Write-Host ""
Write-Host "Step 3: Running full benchmarks (this may take a while)..." -ForegroundColor Yellow
cargo bench --package matching --bench comprehensive_latency 2>&1 | Select-Object -Last 100

Write-Host ""
Write-Host "=== Benchmark Complete ===" -ForegroundColor Green
