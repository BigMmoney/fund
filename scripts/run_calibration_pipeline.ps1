param(
    [string]$ConfigPath = "configs/calibration/binance_spot_smoke.json"
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot

$configPath = Join-Path $RepoRoot $ConfigPath
if (-not (Test-Path $configPath)) {
    throw "Config not found: $configPath"
}

$config = Get-Content $configPath | ConvertFrom-Json
$profileName = $config.profile_name
$outputDir = $config.output_dir

python scripts/download_binance_spot_calibration.py --config $ConfigPath
python scripts/compute_stylized_facts.py --input-dir $outputDir --profile-name $profileName

Write-Host "Calibration pipeline completed for profile=$profileName"
