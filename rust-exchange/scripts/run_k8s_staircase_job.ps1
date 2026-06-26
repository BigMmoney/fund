param(
    [string]$DeployOverlay = ".\deploy\k8s\overlays\docker-desktop",
    [string]$BenchmarkOverlay = ".\deploy\k8s\benchmarks\overlays\docker-desktop",
    [string]$Namespace = "exchange",
    [string]$JobName = "exchange-staircase-benchmark"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $scriptRoot

Push-Location $repoRoot
try {
    kubectl apply -k $DeployOverlay
    if ($LASTEXITCODE -ne 0) {
        throw "failed to apply deploy overlay"
    }

    kubectl rollout status deploy/exchange -n $Namespace --timeout=300s
    if ($LASTEXITCODE -ne 0) {
        throw "exchange deployment did not become ready"
    }

    kubectl delete job $JobName -n $Namespace --ignore-not-found
    kubectl apply -k $BenchmarkOverlay
    if ($LASTEXITCODE -ne 0) {
        throw "failed to apply benchmark overlay"
    }

    kubectl wait --for=condition=complete ("job/{0}" -f $JobName) -n $Namespace --timeout=7200s
    if ($LASTEXITCODE -ne 0) {
        Write-Host "benchmark job did not complete successfully, collecting diagnostics..."
        kubectl get job $JobName -n $Namespace -o wide
        kubectl get pods -n $Namespace -l app=exchange-staircase-benchmark -o wide
        kubectl logs ("job/{0}" -f $JobName) -n $Namespace
        throw "benchmark job did not complete in time"
    }

    kubectl logs ("job/{0}" -f $JobName) -n $Namespace
} finally {
    Pop-Location
}
