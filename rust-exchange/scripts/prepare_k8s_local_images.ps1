param(
    [string]$AppImage = "rust-exchange-local:dev",
    [string]$BenchImage = "exchange-http-bench-local:dev",
    [string]$NodeName = "desktop-control-plane"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Split-Path -Parent $scriptRoot
$workspaceRoot = Split-Path -Parent $repoRoot

function Invoke-Checked {
    param(
        [scriptblock]$Action,
        [string]$FailureMessage
    )

    & $Action
    if ($LASTEXITCODE -ne 0) {
        throw $FailureMessage
    }
}

function Import-ImageToDockerDesktopNode {
    param(
        [string]$Image,
        [string]$Node
    )

    $safeName = (($Image -replace "[^A-Za-z0-9]+", "-").Trim("-")).ToLowerInvariant()
    $tarPath = Join-Path $env:TEMP ("{0}.tar" -f $safeName)
    $debugOutput = ""
    $debugPod = $null

    try {
        Invoke-Checked -FailureMessage "docker save failed for $Image" -Action {
            docker save $Image -o $tarPath
        }

        $debugOutput = kubectl debug ("node/{0}" -f $Node) --image=kindest/node:v1.34.3 --profile=general -- sh -c "sleep 600"
        if ($LASTEXITCODE -ne 0) {
            throw "kubectl debug failed for node $Node"
        }

        $match = [regex]::Match(($debugOutput | Out-String), "Creating debugging pod (?<name>\S+)")
        if (-not $match.Success) {
            throw "could not determine debug pod name from kubectl debug output"
        }
        $debugPod = $match.Groups["name"].Value

        Invoke-Checked -FailureMessage "debug pod $debugPod did not become ready" -Action {
            kubectl wait --for=condition=Ready ("pod/{0}" -f $debugPod) --timeout=120s
        }

        Push-Location (Split-Path $tarPath -Parent)
        try {
            Invoke-Checked -FailureMessage "kubectl cp failed for $Image" -Action {
                kubectl cp (Split-Path $tarPath -Leaf) ("{0}:/host/tmp/{1}.tar" -f $debugPod, $safeName)
            }
        } finally {
            Pop-Location
        }

        Invoke-Checked -FailureMessage "containerd import failed for $Image" -Action {
            kubectl exec $debugPod -- chroot /host ctr -n k8s.io images import ("/tmp/{0}.tar" -f $safeName)
        }
    } finally {
        if ($debugPod) {
            kubectl delete pod $debugPod --ignore-not-found --wait=true | Out-Null
        }
        Remove-Item $tarPath -Force -ErrorAction SilentlyContinue
    }
}

Push-Location $repoRoot
try {
    Invoke-Checked -FailureMessage "exchange app image build failed" -Action {
        docker build --pull=false -t $AppImage .
    }

    $benchBinDir = Join-Path $workspaceRoot "benchmark\bin"
    $benchBinPath = Join-Path $benchBinDir "exchange_http_bench_linux_amd64"
    New-Item -ItemType Directory -Force -Path $benchBinDir | Out-Null
    $previousGoos = $env:GOOS
    $previousGoarch = $env:GOARCH
    try {
        $env:GOOS = "linux"
        $env:GOARCH = "amd64"
        Invoke-Checked -FailureMessage "exchange benchmark linux binary build failed" -Action {
            go build -o $benchBinPath (Join-Path $workspaceRoot "benchmark\cmd\exchange_http_bench\main.go")
        }
    } finally {
        $env:GOOS = $previousGoos
        $env:GOARCH = $previousGoarch
    }

    Invoke-Checked -FailureMessage "exchange benchmark image build failed" -Action {
        docker build --pull=false -f (Join-Path $workspaceRoot "benchmark\Dockerfile.exchange-http-bench") -t $BenchImage $workspaceRoot
    }
} finally {
    Pop-Location
}

Import-ImageToDockerDesktopNode -Image $AppImage -Node $NodeName
Import-ImageToDockerDesktopNode -Image $BenchImage -Node $NodeName

Write-Output ("Imported images into docker-desktop node {0}: {1}, {2}" -f $NodeName, $AppImage, $BenchImage)
