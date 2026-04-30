#!/usr/bin/env pwsh
# Security Audit Script for Rust Exchange
# Performs dependency auditing, cargo-audit scanning, and unsafe code review
# Usage: .\scripts\security_audit.ps1

$ErrorActionPreference = "Stop"
Push-Location "$PSScriptRoot\..\rust-exchange"

Write-Host "=== Rust Exchange Security Audit ===" -ForegroundColor Cyan
Write-Host "Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')" -ForegroundColor Cyan
Write-Host ""

# ── 1. Cargo Audit (RUSTSEC advisory database) ──────────────────
Write-Host "[1/5] Scanning for known vulnerabilities (cargo-audit)..." -ForegroundColor Yellow
if (Get-Command cargo-audit -ErrorAction SilentlyContinue) {
    cargo audit 2>&1 | Write-Host
} else {
    Write-Host "  cargo-audit not installed. Install with: cargo install cargo-audit" -ForegroundColor Red
    Write-Host "  Skipping known vulnerability scan." -ForegroundColor Yellow
}
Write-Host ""

# ── 2. Dependency Inventory ─────────────────────────────────────
Write-Host "[2/5] Dependency inventory..." -ForegroundColor Yellow
$cargoToml = Get-Content "Cargo.toml" -Raw
$deps = @()
foreach ($line in $cargoToml -split "`n") {
    if ($line -match '^\s*(\w[\w-]*)\s*=\s*\{?\s*version\s*=\s*"([^"]+)"') {
        $deps += [PSCustomObject]@{ Name = $Matches[1]; Version = $Matches[2] }
    }
}
$deps | Format-Table -AutoSize | Out-String | Write-Host
Write-Host "  Total direct dependencies: $($deps.Count)" -ForegroundColor Green
Write-Host ""

# ── 3. Unsafe Code Scan ─────────────────────────────────────────
Write-Host "[3/5] Scanning for unsafe code blocks..." -ForegroundColor Yellow
$unsafeFiles = Get-ChildItem -Recurse -Filter "*.rs" crates/ | Select-String -Pattern "^\s*unsafe\s" | Select-Object -Unique FileName
if ($unsafeFiles) {
    Write-Host "  Found unsafe blocks in:" -ForegroundColor Red
    $unsafeFiles | ForEach-Object { Write-Host "    - $($_.FileName)" }
} else {
    Write-Host "  No unsafe code blocks found." -ForegroundColor Green
}
Write-Host ""

# ── 4. Secret Exposure Check ────────────────────────────────────
Write-Host "[4/5] Checking for hardcoded secrets..." -ForegroundColor Yellow
$secretPatterns = @(
    'password\s*=\s*"[^"]+"',
    'api_key\s*=\s*"[^"]+"',
    'secret\s*=\s*"[^"]+"'
)
$found = $false
foreach ($pattern in $secretPatterns) {
    $matches = Get-ChildItem -Recurse -Filter "*.rs" crates/ | Select-String -Pattern $pattern
    if ($matches) {
        $found = $true
        $matches | ForEach-Object {
            Write-Host "  POTENTIAL SECRET: $($_.Filename):$($_.LineNumber)" -ForegroundColor Red
        }
    }
}
if (-not $found) {
    Write-Host "  No hardcoded secrets detected." -ForegroundColor Green
}
Write-Host ""

# ── 5. Unwrap/Panic Count ───────────────────────────────────────
Write-Host "[5/5] Counting unwrap/panic calls in production code..." -ForegroundColor Yellow
$unwrapCount = (Get-ChildItem -Recurse -Filter "*.rs" crates/src/ -ErrorAction SilentlyContinue | Select-String "\.unwrap\(\)|\.expect\(" | Measure-Object).Count
$panicCount = (Get-ChildItem -Recurse -Filter "*.rs" crates/src/ -ErrorAction SilentlyContinue | Select-String "panic!\(" | Measure-Object).Count
Write-Host "  .unwrap()/.expect() calls: $unwrapCount" -ForegroundColor $(if ($unwrapCount -gt 20) { "Red" } else { "Green" })
Write-Host "  panic!() calls: $panicCount" -ForegroundColor $(if ($panicCount -gt 5) { "Red" } else { "Green" })
Write-Host ""

Write-Host "=== Audit Complete ===" -ForegroundColor Cyan

Pop-Location
