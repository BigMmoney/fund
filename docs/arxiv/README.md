# arXiv Package

This folder contains the LaTeX source for the paper and a compiled PDF artifact.

Files:

- `main.tex`: primary LaTeX manuscript
- `references.bib`: BibTeX database
- `figures/`: paper figures
- `main.pdf`: compiled PDF artifact
- `build.ps1`: local build helper for Windows

Build sequence:

```powershell
./build.ps1
```

The build script tries `pdflatex` and `bibtex` from `PATH` first, then falls back to a common MiKTeX user install path.
