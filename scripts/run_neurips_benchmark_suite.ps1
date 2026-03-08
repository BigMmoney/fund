param(
    [switch]$ReviewerOnly,
    [switch]$IncludeMarketProvenance,
    [switch]$SkipFigures,
    [switch]$SkipArchive
)

$ErrorActionPreference = "Stop"
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

$coreCommands = @(
    @{ Env = "RUN_SIM_BENCH"; Test = "TestGenerateSimulatorBenchmarkArtifacts" },
    @{ Env = "RUN_SIM_BENCH_MULTI"; Test = "TestGenerateSimulatorMultiSeedArtifacts" },
    @{ Env = "RUN_SIM_ABLATION"; Test = "TestGenerateSimulatorAblationArtifacts" },
    @{ Env = "RUN_SIM_AGENT_ABLATION"; Test = "TestGenerateSimulatorAgentAblationArtifacts" },
    @{ Env = "RUN_SIM_GRID"; Test = "TestGenerateSimulatorParameterGridArtifacts" },
    @{ Env = "RUN_SIM_CUBE"; Test = "TestGenerateSimulatorParameterCubeArtifacts" },
    @{ Env = "RUN_SIM_HYPER"; Test = "TestGenerateSimulatorParameterHypercubeArtifacts" },
    @{ Env = "RUN_SIM_HELDOUT"; Test = "TestGenerateSimulatorHeldOutPolicyArtifacts" },
    @{ Env = "RUN_SIM_FITTEDQ_CURVE"; Test = "TestGenerateSimulatorFittedQLearningCurveArtifacts" },
    @{ Env = "RUN_SIM_ONLINE_DQN"; Test = "TestGenerateSimulatorOnlineDQNTrainingArtifacts" },
    @{ Env = "RUN_SIM_REWARD_SENSITIVITY"; Test = "TestGenerateSimulatorOnlineDQNRewardSensitivityArtifacts" },
    @{ Env = "RUN_SIM_DOUBLE_DQN"; Test = "TestGenerateSimulatorDoubleDQNTrainingArtifacts" },
    @{ Env = "RUN_SIM_STRATEGIC_AGENTS"; Test = "TestGenerateSimulatorStrategicAgentArtifacts" },
    @{ Env = "RUN_SIM_CALIBRATION_COMPARE"; Test = "TestGenerateSimulatorCalibrationArtifacts" },
    @{ Env = "RUN_SIM_CALIBRATED_BENCH"; Test = "TestGenerateSimulatorCalibratedBenchmarkArtifacts" },
    @{ Env = "RUN_SIM_CALIBRATED_PROTOCOL"; Test = "TestGenerateSimulatorCalibratedLearningProtocolArtifacts" },
    @{ Env = "RUN_SIM_COUNTERFACTUAL"; Test = "TestGenerateSimulatorCounterfactualControlArtifacts" }
)

$reviewerCommands = @(
    @{ Env = "RUN_SIM_RUNTIME"; Test = "TestGenerateSimulatorRuntimeProfileArtifacts" },
    @{ Env = "RUN_SIM_STATS"; Test = "TestGenerateSimulatorStatisticalReviewArtifacts" },
    @{ Env = "RUN_SIM_NECESSITY"; Test = "TestGenerateSimulatorNecessityArtifacts" },
    @{ Env = "RUN_SIM_WELFARE_ROBUSTNESS"; Test = "TestGenerateSimulatorWelfareRobustnessArtifacts" },
    @{ Env = "RUN_SIM_LEADERBOARD"; Test = "TestGenerateSimulatorLeaderboardArtifacts" }
)

$commands = @()
if (-not $ReviewerOnly) {
    $commands += $coreCommands
}
$commands += $reviewerCommands

foreach ($command in $commands) {
    Write-Host "==> $($command.Test)"
    Set-Item -Path ("Env:" + $command.Env) -Value "1"
    try {
        & go test -timeout 30m ./simulator -run $command.Test -v
    }
    finally {
        Remove-Item -Path ("Env:" + $command.Env) -ErrorAction SilentlyContinue
    }
}

if ($IncludeMarketProvenance) {
    Write-Host "==> TestGenerateMarketDataProvenanceArtifacts"
    Set-Item -Path "Env:RUN_MARKET_PROVENANCE" -Value "1"
    try {
        & go test -timeout 30m ./simulator -run TestGenerateMarketDataProvenanceArtifacts -v
    }
    finally {
        Remove-Item -Path "Env:RUN_MARKET_PROVENANCE" -ErrorAction SilentlyContinue
    }
}

if (-not $SkipFigures) {
    Write-Host "==> generate_neurips_figures.py"
    & python scripts/generate_neurips_figures.py
}

if (-not $SkipArchive) {
    $archiveLabel = if ($ReviewerOnly) { "neurips_benchmark_suite_reviewer" } else { "neurips_benchmark_suite_full" }
    Write-Host "==> archive_artifacts.ps1 ($archiveLabel)"
    & (Join-Path $PSScriptRoot "archive_artifacts.ps1") `
        -Label $archiveLabel `
        -RelativePaths @(
            "docs/benchmarks/*.json",
            "docs/benchmarks/*.md",
            "docs/benchmarks/*.csv",
            "docs/neurips_track/figures/*.svg"
        ) `
        -RepoArchiveRoot "docs/benchmarks/archives" `
        -DeliverableArchiveRoot "deliverables/benchmark_archives"
}
