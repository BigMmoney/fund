$ErrorActionPreference = "Continue"
Set-Location "d:\pre_trading\rust-exchange"
$logFile = "d:\pre_trading\rust-exchange\soak_30min.log"

"$(Get-Date) - Starting 30-min soak test" | Out-File $logFile -Append

# Run soak test with autoflush
$output = & .\scripts\soak_test_v2.ps1 -DurationMin 30 -Concurrency 5 2>&1
$output | ForEach-Object { "$(Get-Date) - $_" | Out-File $logFile -Append }

"$(Get-Date) - Soak test completed" | Out-File $logFile -Append
