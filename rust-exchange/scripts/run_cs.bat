# Wrapper: Run ConcurrencySweep and save results to file
param([string]$OutFile = ".\benchmark_cs_results.md")

$output = & "$PSScriptRoot\benchmark_v3.ps1" -Mode ConcurrencySweep 2>&1
$output | Out-File -FilePath $OutFile -Encoding utf8
Write-Host "Results saved to $OutFile"
Write-Host "--- Last 40 lines ---"
$output | Select-Object -Last 40
