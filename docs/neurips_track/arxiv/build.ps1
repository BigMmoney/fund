$ErrorActionPreference = "Stop"

$miktexBin = "C:\Users\secbo\AppData\Local\Programs\MiKTeX\miktex\bin\x64"
if ($env:PATH -notlike "*$miktexBin*") {
    $env:PATH = "$miktexBin;$env:PATH"
}

$tools = @(
    "pdflatex",
    "bibtex",
    "pdflatex",
    "pdflatex"
)

foreach ($tool in $tools) {
    if ($tool -eq "pdflatex") {
        & $tool "-interaction=nonstopmode" "main.tex" | Out-Host
    } else {
        & $tool "main" | Out-Host
    }
    if ($LASTEXITCODE -ne 0) {
        throw "$tool failed with exit code $LASTEXITCODE"
    }
}
