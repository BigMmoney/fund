param(
    [switch]$SkipSoakSeed,
    [string]$CargoTarget = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

function Invoke-RecoveryCargoTest {
    param(
        [string[]]$Arguments
    )

    $effectiveArgs = @($Arguments)
    if (-not [string]::IsNullOrWhiteSpace($CargoTarget)) {
        $separatorIndex = [Array]::IndexOf($effectiveArgs, "--")
        if ($separatorIndex -ge 0) {
            $before = @()
            if ($separatorIndex -gt 0) {
                $before = $effectiveArgs[0..($separatorIndex - 1)]
            }
            $after = $effectiveArgs[$separatorIndex..($effectiveArgs.Count - 1)]
            $effectiveArgs = @($before + @("--target", $CargoTarget) + $after)
        } else {
            $effectiveArgs += @("--target", $CargoTarget)
        }
    }

    Write-Host ("cargo {0}" -f ($effectiveArgs -join " ")) -ForegroundColor Cyan
    & cargo @effectiveArgs
    if ($LASTEXITCODE -ne 0) {
        throw "cargo command failed: cargo $($effectiveArgs -join ' ')"
    }
}

Push-Location $repoRoot
try {
    Write-Host "Running backend recovery checks..." -ForegroundColor Green
    if ([string]::IsNullOrWhiteSpace($CargoTarget) -and $env:OS -eq "Windows_NT") {
        $CargoTarget = "x86_64-pc-windows-msvc"
    }

    Invoke-RecoveryCargoTest -Arguments @("test", "-p", "matching", "--test", "comprehensive_engine_flow")
    Invoke-RecoveryCargoTest -Arguments @("test", "-p", "matching", "--test", "pipeline_fault_matrix")
    Invoke-RecoveryCargoTest -Arguments @("test", "-p", "matching", "--test", "simple_latency_tests", "--", "--nocapture")

    if (-not $SkipSoakSeed) {
        Invoke-RecoveryCargoTest -Arguments @("test", "-p", "matching", "--lib", "--tests")
    }
} finally {
    Pop-Location
}
