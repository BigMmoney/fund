# NeurIPS-Track arXiv Source

This directory is independent from `docs/arxiv/` and belongs to the benchmark/simulator paper line.

## Files

- `main.tex`: NeurIPS-track manuscript
- `references.bib`: bibliography
- `build.ps1`: local build helper
- `main.pdf`: compiled NeurIPS-track PDF

## Build

```powershell
.\build.ps1
```

The script runs:

1. `pdflatex`
2. `bibtex`
3. `pdflatex`
4. `pdflatex`
