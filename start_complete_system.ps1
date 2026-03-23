#!/usr/bin/env pwsh

# Pre-Trading System Startup Script
# ASCII-only output (Windows PowerShell safe).

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

try {
  $utf8 = [System.Text.UTF8Encoding]::new($false)
  [Console]::OutputEncoding = $utf8
  $OutputEncoding = $utf8
} catch {
  # ignore
}

function Write-Step([string]$label) {
  Write-Host ""
  Write-Host $label -ForegroundColor Yellow
}

function Fail([string]$message) {
  Write-Host ""
  Write-Host "[ERR] $message" -ForegroundColor Red
  exit 1
}

Write-Host "================================" -ForegroundColor Cyan
Write-Host "Pre-Trading Exchange System" -ForegroundColor Green
Write-Host "================================" -ForegroundColor Cyan

Write-Step "[1/4] Checking prerequisites"
$hasNode = $null -ne (Get-Command node -ErrorAction SilentlyContinue)
$hasRust = $null -ne (Get-Command cargo -ErrorAction SilentlyContinue)

if (-not $hasNode) { Fail "Node.js not found (install Node.js 18+)." }
if (-not $hasRust) { Fail "Rust not found (install stable Rust + cargo)." }

Write-Host "[OK] Node.js: $(node --version)" -ForegroundColor Green
Write-Host "[OK] Cargo:   $(cargo --version)" -ForegroundColor Green

Write-Step "[2/4] Building Rust Exchange API (release)"
Push-Location (Join-Path $PSScriptRoot 'rust-exchange')
try {
  cargo build --release | Out-Null
  if ($LASTEXITCODE -ne 0) { Fail "Rust build failed." }
  Write-Host "[OK] Rust build OK" -ForegroundColor Green
} finally {
  Pop-Location
}

Write-Step "[3/4] Installing frontend deps (frontend-modern)"
Push-Location (Join-Path $PSScriptRoot 'frontend-modern')
try {
  npm install --silent | Out-Null
  if ($LASTEXITCODE -ne 0) { Fail "Frontend dependency install failed." }
  Write-Host "[OK] Frontend deps OK" -ForegroundColor Green
} finally {
  Pop-Location
}

Write-Step "[4/4] Starting services (new windows)"

$internalSecret = $env:INTERNAL_AUTH_SHARED_SECRET
if (-not $internalSecret -or $internalSecret.Trim().Length -lt 16) {
  # Keep it deterministic for local dev; production should set a strong secret explicitly.
  $internalSecret = "dev-local-internal-auth-secret-change-me"
}

# Escape for PowerShell single-quoted string literal.
$internalSecretEscaped = $internalSecret -replace "'", "''"

$shellExe = $null
$pwshCmd = Get-Command pwsh -ErrorAction SilentlyContinue
if ($pwshCmd -and $pwshCmd.Source) {
  $shellExe = $pwshCmd.Source
} else {
  $powershellCmd = Get-Command powershell -ErrorAction SilentlyContinue
  if ($powershellCmd -and $powershellCmd.Source) {
    $shellExe = $powershellCmd.Source
  } else {
    $shellExe = $null
  }
}
if (-not $shellExe) { Fail "PowerShell executable not found (pwsh / powershell)." }

Write-Host "Starting Rust API: http://localhost:3030" -ForegroundColor Cyan
Start-Process -FilePath $shellExe -ArgumentList @(
  "-NoExit",
  "-Command",
  "cd `"$($PSScriptRoot)\rust-exchange`"; `$env:INTERNAL_AUTH_SHARED_SECRET='$internalSecretEscaped'; cargo run --release --bin api"
) -WindowStyle Normal

Start-Sleep -Seconds 2

Write-Host "Starting frontend: http://localhost:3000" -ForegroundColor Cyan
Start-Process -FilePath $shellExe -ArgumentList @(
  "-NoExit",
  "-Command",
  "cd `"$($PSScriptRoot)\frontend-modern`"; npm run dev"
) -WindowStyle Normal

Start-Sleep -Seconds 2

Write-Host ""
Write-Host "================================" -ForegroundColor Green
Write-Host "[OK] System started" -ForegroundColor Green
Write-Host "================================" -ForegroundColor Green
Write-Host ""
Write-Host "Links:" -ForegroundColor Yellow
Write-Host "- Trading Terminal: http://localhost:3000/#/trading" -ForegroundColor Cyan
Write-Host "- System:          http://localhost:3000/#/" -ForegroundColor Cyan
Write-Host "- API Health:      http://localhost:3030/health" -ForegroundColor Cyan
Write-Host ""
Write-Host "Tip: press Ctrl+C in each window to stop services" -ForegroundColor Yellow

try {
  Start-Process "http://localhost:3000/#/trading" | Out-Null
} catch {
  # ignore
}
