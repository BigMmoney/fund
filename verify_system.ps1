#!/usr/bin/env pwsh

# System Verification Script
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

Write-Host ""
Write-Host "================================" -ForegroundColor Cyan
Write-Host "Pre-Trading System Verification" -ForegroundColor Green
Write-Host "================================" -ForegroundColor Cyan
Write-Host ""

function Check-Endpoint([string]$name, [string]$url, [string]$hint) {
  Write-Host "Checking $name ..." -ForegroundColor Yellow
  try {
    $request = [System.Net.HttpWebRequest]::Create($url)
    $request.Method = 'GET'
    $request.Timeout = 3000
    $request.ReadWriteTimeout = 3000
    $resp = $request.GetResponse()
    Write-Host "[OK] $name reachable" -ForegroundColor Green
    try {
      $status = [int]$resp.StatusCode
      Write-Host "  Status: $status" -ForegroundColor Gray
    } catch {
      # ignore
    }
    try {
      if ($resp.ContentLength -gt 0) {
        Write-Host "  Bytes:  $($resp.ContentLength)" -ForegroundColor Gray
      }
    } catch {
      # ignore
    }
    try { $resp.Close() } catch { }
  } catch {
    Write-Host "[ERR] $name unreachable" -ForegroundColor Red
    Write-Host "  Error: $($_.Exception.Message)" -ForegroundColor Gray
    Write-Host "  Hint:  $hint" -ForegroundColor DarkYellow
  }
  Write-Host ""
}

Check-Endpoint "Rust API (/health)" "http://localhost:3030/health" "Run: cd rust-exchange; set INTERNAL_AUTH_SHARED_SECRET then: cargo run --release --bin api"
Check-Endpoint "Frontend (/)" "http://localhost:3000" "Run: cd frontend-modern; npm run dev"

Write-Host "Links:" -ForegroundColor Yellow
Write-Host "  System:          http://localhost:3000/#/" -ForegroundColor Cyan
Write-Host "  Trading Terminal: http://localhost:3000/#/trading" -ForegroundColor Cyan
Write-Host "  API:             http://localhost:3030" -ForegroundColor Cyan
Write-Host ""
