param(
    [string]$ConfigPath = "configs/calibration/binance_spot_smoke.json",
    [switch]$SkipArchive
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
$archiveManifestPath = ($outputDir -replace '^[.][\\/]', '') + "/manifest.json"

python scripts/download_binance_spot_calibration.py --config $ConfigPath
python scripts/compute_stylized_facts.py --input-dir $outputDir --profile-name $profileName

if (-not $SkipArchive) {
    & (Join-Path $PSScriptRoot "archive_artifacts.ps1") `
        -Label ("calibration_" + $profileName) `
        -RelativePaths @(
            ("docs/benchmarks/binance_spot_" + $profileName + "_facts.json"),
            ("docs/benchmarks/binance_spot_" + $profileName + "_facts.md"),
            $archiveManifestPath
        ) `
        -RepoArchiveRoot "docs/benchmarks/archives" `
        -DeliverableArchiveRoot "deliverables/calibration_archives"
}

Write-Host "Calibration pipeline completed for profile=$profileName"
