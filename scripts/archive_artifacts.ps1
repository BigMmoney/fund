param(
    [Parameter(Mandatory = $true)]
    [string]$Label,
    [Parameter(Mandatory = $true)]
    [string[]]$RelativePaths,
    [string]$RepoArchiveRoot = "",
    [string]$DeliverableArchiveRoot = "",
    [switch]$SkipLatest
)

$ErrorActionPreference = "Stop"
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$timestamp = Get-Date -Format "yyyyMMdd_HHmmss"

if ([string]::IsNullOrWhiteSpace($RepoArchiveRoot) -and [string]::IsNullOrWhiteSpace($DeliverableArchiveRoot)) {
    throw "At least one of RepoArchiveRoot or DeliverableArchiveRoot must be provided."
}

$seen = New-Object System.Collections.Generic.HashSet[string] ([System.StringComparer]::OrdinalIgnoreCase)
$entries = New-Object System.Collections.Generic.List[object]

foreach ($relativePath in $RelativePaths) {
    $absolutePattern = Join-Path $repoRoot $relativePath
    $matches = @()
    if (Test-Path $absolutePattern) {
        $matches = @(Get-Item -Path $absolutePattern -Force)
    } else {
        $matches = @(Get-ChildItem -Path $absolutePattern -Force -ErrorAction SilentlyContinue)
    }
    if ($matches.Count -eq 0) {
        throw "No artifacts matched pattern: $relativePath"
    }
    foreach ($match in $matches) {
        if ($seen.Add($match.FullName)) {
            $entries.Add($match) | Out-Null
        }
    }
}

$targets = @()
if (-not [string]::IsNullOrWhiteSpace($RepoArchiveRoot)) {
    $targets += [pscustomobject]@{
        Name       = "repo"
        VersionDir = Join-Path (Join-Path $repoRoot $RepoArchiveRoot) ($Label + "_" + $timestamp)
        LatestDir  = if ($SkipLatest) { $null } else { Join-Path (Join-Path $repoRoot $RepoArchiveRoot) ($Label + "_latest") }
    }
}
if (-not [string]::IsNullOrWhiteSpace($DeliverableArchiveRoot)) {
    $targets += [pscustomobject]@{
        Name       = "deliverable"
        VersionDir = Join-Path (Join-Path $repoRoot $DeliverableArchiveRoot) ($Label + "_" + $timestamp)
        LatestDir  = if ($SkipLatest) { $null } else { Join-Path (Join-Path $repoRoot $DeliverableArchiveRoot) ($Label + "_latest") }
    }
}

function Get-RelativePath {
    param(
        [string]$BasePath,
        [string]$TargetPath
    )

    $baseUri = New-Object System.Uri(($BasePath.TrimEnd('\', '/') + [System.IO.Path]::DirectorySeparatorChar))
    $targetUri = New-Object System.Uri($TargetPath)
    return [System.Uri]::UnescapeDataString($baseUri.MakeRelativeUri($targetUri).ToString()).Replace('/', [System.IO.Path]::DirectorySeparatorChar)
}

function Copy-Entry {
    param(
        [System.IO.FileSystemInfo]$Entry,
        [string]$DestinationRoot
    )

    $relative = Get-RelativePath -BasePath $repoRoot -TargetPath $Entry.FullName
    $destination = Join-Path $DestinationRoot $relative
    $parent = Split-Path -Parent $destination
    if ($parent) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }

    if ($Entry.PSIsContainer) {
        Copy-Item -Path $Entry.FullName -Destination $parent -Recurse -Force
    } else {
        Copy-Item -Path $Entry.FullName -Destination $destination -Force
    }

    return $relative
}

$gitBranch = (& git branch --show-current 2>$null)
$gitCommit = (& git rev-parse HEAD 2>$null)
$copiedRelativePaths = New-Object System.Collections.Generic.List[string]

foreach ($entry in $entries) {
    $copiedRelativePaths.Add((Get-RelativePath -BasePath $repoRoot -TargetPath $entry.FullName)) | Out-Null
}

foreach ($target in $targets) {
    New-Item -ItemType Directory -Force -Path $target.VersionDir | Out-Null
    foreach ($entry in $entries) {
        Copy-Entry -Entry $entry -DestinationRoot $target.VersionDir | Out-Null
    }

    $manifest = [ordered]@{
        label          = $Label
        created_at     = (Get-Date).ToString("o")
        git_branch     = $gitBranch
        git_commit     = $gitCommit
        source_patterns = $RelativePaths
        copied_paths   = $copiedRelativePaths
    }
    ($manifest | ConvertTo-Json -Depth 4) + "`n" | Set-Content -Path (Join-Path $target.VersionDir "manifest.json") -Encoding UTF8

    if ($target.LatestDir) {
        if (Test-Path $target.LatestDir) {
            Remove-Item -Path $target.LatestDir -Recurse -Force
        }
        New-Item -ItemType Directory -Force -Path $target.LatestDir | Out-Null
        foreach ($entry in $entries) {
            Copy-Entry -Entry $entry -DestinationRoot $target.LatestDir | Out-Null
        }
        ($manifest | ConvertTo-Json -Depth 4) + "`n" | Set-Content -Path (Join-Path $target.LatestDir "manifest.json") -Encoding UTF8
    }

    Write-Host "$($target.Name) archive: $($target.VersionDir)"
    if ($target.LatestDir) {
        Write-Host "$($target.Name) latest: $($target.LatestDir)"
    }
}
