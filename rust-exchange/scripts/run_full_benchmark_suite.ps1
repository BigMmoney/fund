param(
    [string]$OutputDir = "",
    [ValidateSet('Small','Medium','Large')]
    [string]$Scale = 'Medium',
    [switch]$SkipMatching,
    [switch]$SkipWalAppend,
    [switch]$SkipReplay,
    [switch]$SkipRto,
    [string]$CompareAgainstBaseline = "",
    [switch]$FailOnRegression
)

# Full benchmark suite orchestrator.
#
# Runs the committed bench scenarios in order, captures per-scenario logs +
# raw output paths + criterion artifacts, and writes a single aggregated
# `bench_summary.json` under the output directory.
#
# Scenarios in this version:
#   matching_micro      — cargo bench -p matching --bench matching_benchmark
#   wal_append          — cargo bench -p persistence --bench wal_append
#   replay_scaling      — cargo bench -p sequencer --bench replay_scaling
#   rto_rpo             — measure_rto_rpo.ps1
#
# Optional comparator (-CompareAgainstBaseline) defers to bench_compare.ps1
# (when present) for regression decisions.

$ErrorActionPreference = "Stop"
. "$PSScriptRoot\test_lib.ps1"

$rustRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

if ([string]::IsNullOrWhiteSpace($OutputDir)) {
    $ts = Get-Date -Format "yyyyMMdd_HHmmss"
    $OutputDir = Join-Path $rustRoot ".." "artifacts" "bench_$ts"
}
if (-not [System.IO.Path]::IsPathRooted($OutputDir)) {
    $OutputDir = Join-Path $rustRoot $OutputDir
}
New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
$OutputDir = (Resolve-Path $OutputDir).Path

# ── Scale-dependent parameters ───────────────────────────────
$rtoIterations  = 3
$rtoCommandCount = 200
switch ($Scale) {
    'Small'  { $rtoIterations = 3;  $rtoCommandCount = 100  }
    'Medium' { $rtoIterations = 5;  $rtoCommandCount = 1000 }
    'Large'  { $rtoIterations = 10; $rtoCommandCount = 5000 }
}

# ── Hardware/environment fingerprint ──────────────────────────
function Get-EnvironmentFingerprint {
    $ci = Get-ComputerInfo -ErrorAction SilentlyContinue
    $cpuName = if ($ci -and $ci.CsProcessors) { ($ci.CsProcessors | Select-Object -First 1).Name } else { $env:PROCESSOR_IDENTIFIER }
    $coresLog = if ($ci -and $ci.CsNumberOfLogicalProcessors) { $ci.CsNumberOfLogicalProcessors } else { $env:NUMBER_OF_PROCESSORS }
    $coresPhys = if ($ci -and $ci.CsNumberOfProcessors) { $ci.CsNumberOfProcessors } else { $null }
    $ramGb = if ($ci -and $ci.CsTotalPhysicalMemory) { [Math]::Round($ci.CsTotalPhysicalMemory / 1GB, 1) } else { $null }
    $osName = if ($ci -and $ci.OsName) { $ci.OsName } else { (Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue).Caption }
    $osVer = if ($ci -and $ci.OsVersion) { $ci.OsVersion } else { [Environment]::OSVersion.Version.ToString() }
    $rustVer = (& rustc --version) 2>&1
    $cargoVer = (& cargo --version) 2>&1
    $gitSha = (& git rev-parse --short HEAD) 2>&1
    $gitBranch = (& git rev-parse --abbrev-ref HEAD) 2>&1
    return [ordered]@{
        host                  = $env:COMPUTERNAME
        os_name               = $osName
        os_version            = $osVer
        cpu_model             = ($cpuName -as [string]).Trim()
        cores_logical         = [int]$coresLog
        cores_physical        = $coresPhys
        ram_total_gb          = $ramGb
        rust_version          = ($rustVer -as [string]).Trim()
        cargo_version         = ($cargoVer -as [string]).Trim()
        git_commit            = ($gitSha -as [string]).Trim()
        git_branch            = ($gitBranch -as [string]).Trim()
        captured_at_utc       = (Get-Date).ToUniversalTime().ToString("o")
    }
}

# ── Scenario runner helpers ───────────────────────────────────
$scenarios = New-Object System.Collections.Generic.List[object]

function Add-Scenario {
    param(
        [string]$Name,
        [int]$ExitCode,
        [double]$ElapsedSeconds,
        [string]$LogPath,
        [string]$ReportPath = $null,
        [string]$Note = ""
    )
    $scenarios.Add([pscustomobject][ordered]@{
        name             = $Name
        exit_code        = $ExitCode
        elapsed_seconds  = [Math]::Round($ElapsedSeconds, 3)
        log_path         = $LogPath
        report_path      = $ReportPath
        note             = $Note
        passed           = ($ExitCode -eq 0)
    }) | Out-Null
    $color = if ($ExitCode -eq 0) { 'Green' } else { 'Red' }
    Write-Host ("  [{0}] {1}  ({2}s)" -f ($(if ($ExitCode -eq 0) {'PASS'} else {'FAIL'})), $Name, [Math]::Round($ElapsedSeconds, 1)) -ForegroundColor $color
}

function Invoke-CargoBench {
    param([string]$Name, [string]$Crate, [string]$BenchName, [string]$LogPath, [switch]$Quick)
    $started = Get-Date
    $args = @("bench","-p",$Crate,"--bench",$BenchName,"--")
    if ($Quick) { $args += @("--quick") }
    Push-Location $rustRoot
    # Relax ErrorAction locally: cargo writes progress to stderr and PS 5.1
    # would otherwise wrap each line as a terminating NativeCommandError.
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & cargo @args *>&1 | Tee-Object -FilePath $LogPath | Out-Null
        $code = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prev
        Pop-Location
    }
    $elapsed = ((Get-Date) - $started).TotalSeconds
    # Locate criterion estimates.json files for this bench (if any)
    $critRoot = Join-Path $rustRoot "target\criterion"
    Add-Scenario -Name $Name -ExitCode $code -ElapsedSeconds $elapsed -LogPath $LogPath -ReportPath $critRoot
}

function Invoke-PSScript {
    param([string]$Name, [string]$Script, [string[]]$Args, [string]$LogPath, [string]$ReportPath = $null)
    $started = Get-Date
    $argList = @("-ExecutionPolicy","Bypass","-File",$Script) + $Args
    $prev = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & powershell @argList *>&1 | Tee-Object -FilePath $LogPath | Out-Null
        $code = $LASTEXITCODE
    } finally { $ErrorActionPreference = $prev }
    $elapsed = ((Get-Date) - $started).TotalSeconds
    Add-Scenario -Name $Name -ExitCode $code -ElapsedSeconds $elapsed -LogPath $LogPath -ReportPath $ReportPath
}

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host "Full benchmark suite — output: $OutputDir"  -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

$envFingerprint = Get-EnvironmentFingerprint

# ── Run scenarios in order (smallest first) ──────────────────
if (-not $SkipMatching) {
    Write-Host "`n[matching_micro] cargo bench -p matching --bench matching_benchmark" -ForegroundColor Yellow
    Invoke-CargoBench -Name "matching_micro" -Crate "matching" -BenchName "matching_benchmark" -LogPath (Join-Path $OutputDir "matching_micro.log") -Quick
}

if (-not $SkipWalAppend) {
    Write-Host "`n[wal_append] cargo bench -p persistence --bench wal_append" -ForegroundColor Yellow
    Invoke-CargoBench -Name "wal_append" -Crate "persistence" -BenchName "wal_append" -LogPath (Join-Path $OutputDir "wal_append.log") -Quick
}

if (-not $SkipReplay) {
    Write-Host "`n[replay_scaling] cargo bench -p sequencer --bench replay_scaling" -ForegroundColor Yellow
    Invoke-CargoBench -Name "replay_scaling" -Crate "sequencer" -BenchName "replay_scaling" -LogPath (Join-Path $OutputDir "replay_scaling.log") -Quick
}

if (-not $SkipRto) {
    Write-Host "`n[rto_rpo] measure_rto_rpo.ps1 -Iterations $rtoIterations -CommandCount $rtoCommandCount" -ForegroundColor Yellow
    $rtoReport = Join-Path $OutputDir "rto_rpo.json"
    Invoke-PSScript `
        -Name "rto_rpo" `
        -Script "$PSScriptRoot\measure_rto_rpo.ps1" `
        -Args @("-Iterations", "$rtoIterations", "-CommandCount", "$rtoCommandCount", "-Output", $rtoReport) `
        -LogPath (Join-Path $OutputDir "rto_rpo.log") `
        -ReportPath $rtoReport
}

# ── Aggregate summary ────────────────────────────────────────
$allPassed = ($scenarios | Where-Object { -not $_.passed } | Measure-Object).Count -eq 0
$summary = [ordered]@{
    schema_version       = 1
    generated_at_epoch   = [int][double]::Parse((Get-Date -UFormat %s))
    output_dir           = $OutputDir
    scale                = $Scale
    environment          = $envFingerprint
    scenarios            = $scenarios
    passed               = $allPassed
}
$summaryPath = Join-Path $OutputDir "bench_summary.json"
$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $summaryPath -Encoding UTF8
Write-Host "`nAggregated summary: $summaryPath" -ForegroundColor Green

# ── Optional baseline comparison ─────────────────────────────
$comparatorExit = 0
if ($CompareAgainstBaseline) {
    $comparatorScript = Join-Path $PSScriptRoot "bench_compare.ps1"
    if (Test-Path $comparatorScript) {
        Write-Host "`n[bench_compare] comparing against baseline $CompareAgainstBaseline" -ForegroundColor Yellow
        & powershell -ExecutionPolicy Bypass -File $comparatorScript `
            -Current $summaryPath `
            -Baseline $CompareAgainstBaseline `
            -Output (Join-Path $OutputDir "bench_compare.json")
        $comparatorExit = $LASTEXITCODE
    } else {
        Write-Host "`n[bench_compare] script not found ($comparatorScript) — skipping" -ForegroundColor Yellow
        $comparatorExit = 0
    }
}

# ── Verdict ──────────────────────────────────────────────────
Write-Host "`n=========================================" -ForegroundColor Cyan
Write-Host "BENCHMARK SUITE SUMMARY"                     -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan
foreach ($s in $scenarios) {
    $color = if ($s.passed) { 'Green' } else { 'Red' }
    Write-Host ("  [{0}] {1}  ({2}s)  exit={3}" -f ($(if ($s.passed) {'PASS'} else {'FAIL'})), $s.name, $s.elapsed_seconds, $s.exit_code) -ForegroundColor $color
}
$passCount = ($scenarios | Where-Object { $_.passed } | Measure-Object).Count
$totalCount = $scenarios.Count
Write-Host ("Passed: {0}/{1}" -f $passCount, $totalCount) -ForegroundColor $(if ($allPassed) { 'Green' } else { 'Red' })
Write-Host "=========================================" -ForegroundColor Cyan

# Exit non-zero on any scenario failure OR on regression (when comparator was invoked with -FailOnRegression)
if (-not $allPassed) { exit 1 }
if ($FailOnRegression -and $comparatorExit -ne 0) { exit $comparatorExit }
exit 0
