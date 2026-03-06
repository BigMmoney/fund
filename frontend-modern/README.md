# frontend-modern

Market infrastructure and systems frontend for high-frequency policy, news, and risk-aware execution workflows.

## Project Narrative

This repository is not positioned as a generic UI project. It is the operator surface for a market infrastructure stack:

- Ingests heterogeneous signals (policy, sanctions, macro headlines, execution metadata)
- Applies bounded transformations and scoring logic
- Surfaces deterministic state transitions for traders, risk analysts, and operations teams
- Preserves auditability with explicit invariants and reproducible benchmark outputs

The architecture follows a systems mindset:

- Data plane: data ingestion, normalization, and state transitions
- Control plane: policy/risk gating, confidence caps, and kill-switch style controls
- Presentation plane: deterministic, typed, and bounded UI rendering

## Current Status

- Frontend stack: React 18 + TypeScript + Vite
- Added: invariant tests (core math + real-time buffering abstractions)
- Added: benchmark runner with SVG chart artifact
- Added: CI quality gate for invariants + benchmark regression checks
- Added: technical report in `docs/TECHNICAL_REPORT.md`

## Quick Start

```bash
npm install
npm run dev
```

App starts at:

- `http://localhost:3000`

## Quality & Reliability Commands

```bash
# Invariant test suite
npm run test:invariants

# Generate benchmark JSON + SVG chart
npm run benchmark

# CI-grade check with threshold assertions
npm run benchmark:check
npm run ci:quality
```

Generated artifacts:

- `docs/benchmarks/benchmark-latest.json`
- `docs/benchmarks/benchmark-latest.svg`

## Core Design Principles

1. Bounded State, Not Unbounded UI Mutation
2. Deterministic Transformations Over Ad-Hoc Side Effects
3. Explicit Invariants Over Implicit Assumptions
4. Throughput + Latency Visibility via Benchmarks
5. Operational Explainability and Auditability

## System Components

- `src/lib/safemath.ts`
  - Numerical safety primitives
  - NaN/Infinity containment
  - Bounded output normalization

- `src/lib/realtimeBuffer.ts`
  - Real-time buffering and flush scheduling
  - Dedupe/aggregation mechanics
  - Stable list and change classification primitives

- `src/services/*`
  - Policy and signal ingestion/transformation logic
  - Data source abstraction and scoring support

- `src/pages/*`
  - Operator-facing dashboards and trading views

## CI Pipeline

Workflow file: `.github/workflows/quality.yml`

Pipeline stages:

1. install dependencies (`npm ci`)
2. run invariant tests (`npm run test:invariants`)
3. run benchmark guard (`npm run benchmark:check`)
4. upload benchmark artifacts (JSON + SVG)

This creates a lightweight but enforceable system-quality baseline before merges.

## Benchmark Policy

Benchmarking is treated as a first-class engineering control:

- repeatable local command (`npm run benchmark`)
- CI enforcement mode (`npm run benchmark:check`)
- machine-readable results (`.json`)
- reviewable visual artifact (`.svg`)

The benchmark currently targets core data-plane primitives:

- `safeDivide`
- `safeWeightedAverage`
- `classifyChange`
- `StableList.update`
- `SignalHysteresis.addSample`
- `formatPercentChange`

## Invariant Test Scope

Invariant tests are focused on properties, not only examples:

- output boundedness
- finite-number guarantees
- dedupe correctness
- max-buffer enforcement
- update classification priority
- deterministic state behavior under repeated updates

Test location:

- `tests/invariants/safemath.invariant.test.ts`
- `tests/invariants/realtimeBuffer.invariant.test.ts`

## Repository Layout

```text
frontend-modern/
├─ .github/workflows/
│  ├─ deploy.yml
│  └─ quality.yml
├─ docs/
│  ├─ TECHNICAL_REPORT.md
│  └─ benchmarks/
├─ scripts/
│  └─ benchmark.ts
├─ src/
│  ├─ components/
│  ├─ contexts/
│  ├─ hooks/
│  ├─ lib/
│  ├─ pages/
│  └─ services/
├─ tests/invariants/
├─ vite.config.ts
└─ vitest.config.ts
```

## Known Engineering Debt

- Type-check currently fails on several large page/service modules and type model drifts.
- Existing lint script requires an ESLint config file.
- Some source strings/comments show encoding artifacts and should be normalized.

These are tracked in the technical report with staged remediation.

## Roadmap (Systems-Centric)

1. Restore full compile health (strict TS for all pages/services)
2. Add schema contracts between ingestion and UI models
3. Introduce source-level trace IDs for end-to-end audit trails
4. Add deterministic replay harness for historical incident simulation
5. Expand benchmark matrix with memory and GC pressure metrics

## License

Private/internal use unless explicitly relicensed.
