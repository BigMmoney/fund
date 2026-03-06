$ErrorActionPreference = "Stop"

function Resolve-Tool {
    param(
        [string]$Name,
        [string]$Fallback
    )

    $cmd = Get-Command $Name -ErrorAction SilentlyContinue
    if ($cmd) {
        return $cmd.Source
    }

    if (Test-Path $Fallback) {
        return $Fallback
    }

    throw "Unable to locate $Name"
}

$miktexBin = Join-Path $env:LOCALAPPDATA "Programs\MiKTeX\miktex\bin\x64"
$pdflatex = Resolve-Tool "pdflatex" (Join-Path $miktexBin "pdflatex.exe")
$bibtex = Resolve-Tool "bibtex" (Join-Path $miktexBin "bibtex.exe")

& $pdflatex -interaction=nonstopmode -halt-on-error main.tex
& $bibtex main
& $pdflatex -interaction=nonstopmode -halt-on-error main.tex
& $pdflatex -interaction=nonstopmode -halt-on-error main.tex
