param(
    [Parameter(Mandatory = $true)]
    [string]$BackupArchive,
    [string]$RestoreDir = "",
    [switch]$CleanRestoreDir
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not (Test-Path $BackupArchive)) {
    throw "backup archive not found: $BackupArchive"
}

$resolvedArchive = (Resolve-Path $BackupArchive).Path
$targetDir = if ([string]::IsNullOrWhiteSpace($RestoreDir)) {
    Join-Path (Split-Path $resolvedArchive -Parent) "restore-drill"
} else {
    $RestoreDir
}

if ($CleanRestoreDir -and (Test-Path $targetDir)) {
    Remove-Item -LiteralPath $targetDir -Recurse -Force
}

New-Item -ItemType Directory -Path $targetDir -Force | Out-Null

$tarExe = Join-Path $env:SystemRoot "System32\tar.exe"
if (-not (Test-Path $tarExe)) {
    $tarExe = "tar"
}
& $tarExe -xzf $resolvedArchive -C $targetDir
if ($LASTEXITCODE -ne 0) {
    throw "tar extraction failed with exit code $LASTEXITCODE"
}

$files = Get-ChildItem -Path $targetDir -Recurse -File | Select-Object FullName, Length
$report = [ordered]@{
    backup_archive = $resolvedArchive
    restore_dir    = (Resolve-Path $targetDir).Path
    restored_files = @($files | ForEach-Object {
        [ordered]@{
            path  = $_.FullName
            bytes = $_.Length
        }
    })
    restored_count = @($files).Count
}

$reportPath = Join-Path $targetDir "restore_drill_report.json"
$report | ConvertTo-Json -Depth 6 | Set-Content -Path $reportPath -Encoding UTF8
Write-Host "Restore drill complete:" -ForegroundColor Green
Write-Host "  archive: $resolvedArchive" -ForegroundColor DarkGray
Write-Host "  restore: $targetDir" -ForegroundColor DarkGray
Write-Host "  report:  $reportPath" -ForegroundColor DarkGray
