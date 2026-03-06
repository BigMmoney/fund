# Benchmark-Track arXiv Source

This directory is independent from `docs/arxiv/` and belongs to the benchmark/simulator paper line.

## Files

- `main.tex`: benchmark-track manuscript
- `references.bib`: bibliography
- `build.ps1`: local build helper

## Build

```powershell
.\build.ps1
```

The script runs:

1. `pdflatex`
2. `bibtex`
3. `pdflatex`
4. `pdflatex`
