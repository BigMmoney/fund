param(
    [string]$DataDir = "$PSScriptRoot\..\data",
    [string]$OutputDir = "$PSScriptRoot\..\artifacts\wal-backups",
    [int]$RetainCount = 14
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not (Test-Path $DataDir)) {
    throw "data directory not found: $DataDir"
}

$resolvedData = (Resolve-Path $DataDir).Path
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
$resolvedOut = (Resolve-Path $OutputDir).Path

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
$archiveName = "wal-$timestamp.tar.gz"
$archivePath = Join-Path $resolvedOut $archiveName
$manifestPath = Join-Path $resolvedOut "wal-$timestamp.manifest.json"

$walFiles = Get-ChildItem -Path $resolvedData -File -Filter "*.jsonl" -ErrorAction SilentlyContinue
if (@($walFiles).Count -eq 0) {
    throw "no WAL (*.jsonl) files found in $resolvedData"
}

$manifest = [ordered]@{
    archive       = $archivePath
    created_utc   = (Get-Date).ToUniversalTime().ToString("o")
    source_dir    = $resolvedData
    file_count    = @($walFiles).Count
    total_bytes   = ($walFiles | Measure-Object -Property Length -Sum).Sum
    files         = @($walFiles | ForEach-Object {
        [ordered]@{
            name  = $_.Name
            bytes = $_.Length
            mtime = $_.LastWriteTimeUtc.ToString("o")
        }
    })
}

$tarExe = Join-Path $env:SystemRoot "System32\tar.exe"
if (-not (Test-Path $tarExe)) {
    $tarExe = "tar"
}

Push-Location $resolvedData
try {
    $relativeNames = $walFiles | ForEach-Object { $_.Name }
    & $tarExe -czf $archivePath -- $relativeNames
    if ($LASTEXITCODE -ne 0) {
        throw "tar failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

$archiveBytes = (Get-Item $archivePath).Length
$manifest["archive_bytes"] = $archiveBytes
$manifest | ConvertTo-Json -Depth 6 | Set-Content -Path $manifestPath -Encoding UTF8

if ($RetainCount -gt 0) {
    $existing = Get-ChildItem -Path $resolvedOut -File -Filter "wal-*.tar.gz" |
        Sort-Object LastWriteTime -Descending
    if (@($existing).Count -gt $RetainCount) {
        $toDelete = $existing | Select-Object -Skip $RetainCount
        foreach ($f in $toDelete) {
            $stem = [System.IO.Path]::GetFileNameWithoutExtension($f.Name)
            $stem = $stem -replace '\.tar$', ''
            $companionManifest = Join-Path $resolvedOut "$stem.manifest.json"
            Remove-Item -LiteralPath $f.FullName -Force
            if (Test-Path $companionManifest) {
                Remove-Item -LiteralPath $companionManifest -Force
            }
        }
    }
}

Write-Host "WAL backup complete:" -ForegroundColor Green
Write-Host "  archive:  $archivePath" -ForegroundColor DarkGray
Write-Host "  manifest: $manifestPath" -ForegroundColor DarkGray
Write-Host "  files:    $($manifest.file_count)" -ForegroundColor DarkGray
Write-Host "  size:     $archiveBytes bytes" -ForegroundColor DarkGray
