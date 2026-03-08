$ErrorActionPreference = "Stop"

$miktexBin = "C:\Users\secbo\AppData\Local\Programs\MiKTeX\miktex\bin\x64"
if ($env:PATH -notlike "*$miktexBin*") {
    $env:PATH = "$miktexBin;$env:PATH"
}

$tools = @(
    "pdflatex",
    "bibtex",
    "pdflatex",
    "pdflatex"
)

foreach ($tool in $tools) {
    if ($tool -eq "pdflatex") {
        & $tool "-interaction=nonstopmode" "main.tex" | Out-Host
    } else {
        & $tool "main" | Out-Host
    }
    if ($LASTEXITCODE -ne 0) {
        throw "$tool failed with exit code $LASTEXITCODE"
    }
}

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..\..")
$deliverablesDir = Join-Path $repoRoot "deliverables"
New-Item -ItemType Directory -Force -Path $deliverablesDir | Out-Null

$timestamp = Get-Date -Format "yyyyMMdd_HHmmss"
$latestPdf = Join-Path $deliverablesDir "neurips_main_latest.pdf"
$versionedPdf = Join-Path $deliverablesDir ("neurips_main_" + $timestamp + ".pdf")
$repoVersionsDir = Join-Path $PSScriptRoot "versions"
$repoVersionedPdf = Join-Path $repoVersionsDir ("main_v" + $timestamp + ".pdf")

Copy-Item (Join-Path $PSScriptRoot "main.pdf") $latestPdf -Force
Copy-Item (Join-Path $PSScriptRoot "main.pdf") $versionedPdf -Force
New-Item -ItemType Directory -Force -Path $repoVersionsDir | Out-Null
Copy-Item (Join-Path $PSScriptRoot "main.pdf") $repoVersionedPdf -Force

Write-Host "Latest PDF: $latestPdf"
Write-Host "Versioned PDF: $versionedPdf"
Write-Host "Repo Versioned PDF: $repoVersionedPdf"
