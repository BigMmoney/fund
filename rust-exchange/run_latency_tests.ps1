# Run latency tests and capture results
$ErrorActionPreference = "Continue"

Write-Host "========================================" -ForegroundColor Cyan
Write-Host "Trading Engine Latency Testing Suite" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan
Write-Host ""

Set-Location "d:\pre_trading\rust-exchange"

$outputFile = "d:\pre_trading\rust-exchange\latency_test_results.txt"

Write-Host "Running simple latency tests..." -ForegroundColor Yellow
Write-Host "This may take 30-60 seconds..." -ForegroundColor Gray
Write-Host ""

# Run tests and capture output
cargo test --package matching --test simple_latency_tests -- --nocapture 2>&1 | Tee-Object -FilePath $outputFile

if ($LASTEXITCODE -eq 0) {
    Write-Host ""
    Write-Host "========================================" -ForegroundColor Green
    Write-Host "Tests completed successfully!" -ForegroundColor Green
    Write-Host "========================================" -ForegroundColor Green
    Write-Host ""
    Write-Host "Results saved to: $outputFile" -ForegroundColor Cyan
    Write-Host ""
    
    # Show summary from file
    if (Test-Path $outputFile) {
        $content = Get-Content $outputFile -Raw
        
        # Extract key metrics
        if ($content -match "Avg: ([\d.]+)") {
            Write-Host "Key Metrics Found:" -ForegroundColor Yellow
            $matches = [regex]::Matches($content, "(Cold Start|Warm Book|Market Order|Load Spike|Volatility).*?Avg: ([\d.]+)")
            foreach ($match in $matches) {
                Write-Host "  $($match.Groups[1].Value): $($match.Groups[2].Value) µs" -ForegroundColor White
            }
        }
    }
} else {
    Write-Host ""
    Write-Host "========================================" -ForegroundColor Red
    Write-Host "Tests failed or encountered errors" -ForegroundColor Red
    Write-Host "========================================" -ForegroundColor Red
    Write-Host ""
    Write-Host "Check the output above for details" -ForegroundColor Yellow
    exit 1
}
