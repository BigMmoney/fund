$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$rustRoot = Join-Path $repoRoot 'rust-exchange'

Write-Host '[crash-drill] running crash recovery drills...' -ForegroundColor Cyan
Push-Location $rustRoot
try {
    cargo run --release --example crash_recovery_drill -p matching
}
finally {
    Pop-Location
}
