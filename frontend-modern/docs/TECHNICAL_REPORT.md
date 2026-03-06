# Technical Report: Market Infrastructure / Systems Frontend
Date: 2026-03-06  
Project: `frontend-modern`  
Scope: architecture, reliability posture, invariants, benchmark method, CI guardrails, remediation roadmap

## 1. Executive Summary

This project is a systems-facing frontend for market infrastructure workflows, not a conventional content UI. The core challenge is not static rendering; it is controlled representation of unstable, high-frequency, high-ambiguity inputs such as policy updates, sanctions list changes, and market-impacting events.

The frontend must therefore satisfy engineering constraints that are usually associated with distributed systems:

- bounded numeric behavior under malformed inputs
- deterministic state transitions under noisy update streams
- stable ordering and identity in continuously changing lists
- visible performance budget for critical primitives
- machine-enforced quality gates before merge

In this iteration, four quality pillars were introduced:

1. Invariant tests for core data-plane primitives (`safemath`, `realtimeBuffer`).
2. Benchmark harness with both machine-readable and human-readable artifacts.
3. CI workflow that enforces invariants and benchmark regression thresholds.
4. Documentation reframing from “UI app” to “market infrastructure / systems project”.

This report documents architecture, controls, measured outcomes, and remaining risks.

## 2. System Context and Problem Model

### 2.1 Functional Context

The frontend is expected to:

- aggregate and surface heterogeneous external/internal signal feeds
- transform those feeds into structured decision support objects
- present low-latency operational interfaces for analysis and execution

The user persona is operational: trader, risk analyst, policy monitor, and platform operator. They require low ambiguity and high reproducibility.

### 2.2 Failure Modes

Without systems-grade controls, common failure classes include:

- **Numeric contamination**: NaN/Infinity spreading into display and scoring logic.
- **Update thrash**: excessive updates causing unstable UI and inconsistent derived state.
- **Key churn**: unstable list keys causing component remount storms and visual artifacts.
- **Silent regressions**: performance degradation not caught during review.
- **Merge without guarantees**: no deterministic pre-merge checks for runtime invariants.

These failure modes are not hypothetical; they appear frequently in high-churn real-time interfaces.

## 3. Architecture View

### 3.1 Layering

The project is interpreted in three planes:

- **Data Plane**
  - Modules: `src/lib/safemath.ts`, `src/lib/realtimeBuffer.ts`, service transforms
  - Responsibility: enforce bounded transformations and stable buffering semantics

- **Control Plane**
  - Decision/risk logic (scoring, thresholds, policy transitions)
  - Responsibility: ensure a deterministic and explainable path from raw signal to decision state

- **Presentation Plane**
  - React pages/components
  - Responsibility: render stable model output without reintroducing nondeterminism

### 3.2 Why Invariants First

Given current compile debt in large page modules, broad compile-time guarantees are not yet reliable end-to-end. The highest leverage move is to lock down the foundational primitives that everything else depends on:

- safe math helpers
- real-time buffering, dedupe, and stable list mechanics
- change classification logic

This reduces system risk immediately while broader type refactors proceed.

## 4. Invariant Testing Strategy

### 4.1 Philosophy

Traditional unit tests verify examples. Invariant tests verify properties that must hold across many values and update sequences.

Adopted invariants include:

- finite output guarantees
- boundedness (e.g., clamp behavior)
- deterministic dedupe and buffer limits
- update classification priority ordering
- stable behavior under repeated transitions

### 4.2 Implemented Test Suites

Test files:

- `tests/invariants/safemath.invariant.test.ts`
- `tests/invariants/realtimeBuffer.invariant.test.ts`

Coverage focus:

1. `safeDivide` finite guarantees under random and edge denominators.
2. `safePercentChange` null semantics for near-zero denominator.
3. `clamp` range and idempotence.
4. `safeWeightedAverage` scaling invariance and insufficient sample handling.
5. `sanitizeOutput` NaN/Infinity nullification.
6. `RealtimeBuffer` dedupe correctness.
7. `RealtimeBuffer` max buffer enforcement.
8. timer lifecycle semantics (`start/stop` and flush behavior).
9. `SignalHysteresis` threshold-based update behavior.
10. `StableList` ordering and cap semantics.
11. `classifyChange` precedence (`state > grade > value > none`).

### 4.3 Why This Matters in Market Infrastructure

In trading or policy-response contexts, minor type or state drifts can propagate into:

- incorrect confidence displays
- false urgency or stale status
- operational mistrust of dashboards

Invariant tests create a compact and maintainable safety boundary around those failure paths.

## 5. Benchmark Methodology and Result Artifacts

### 5.1 Method

Benchmark runner: `scripts/benchmark.ts`

Method details:

- warmup execution before timing
- multiple rounds per case
- mean latency and p95 round latency
- ops/s normalization
- deterministic output files for audit trail

### 5.2 Benchmarked Primitives

Current benchmark cases:

- `safeDivide hot path`
- `safeWeightedAverage small vector`
- `classifyChange state + grade + value`
- `StableList.update (batch=128)`
- `SignalHysteresis.addSample`
- `formatPercentChange`

### 5.3 Artifacts

Outputs generated on each run:

- `docs/benchmarks/benchmark-latest.json`
- `docs/benchmarks/benchmark-latest.svg`

The JSON is intended for diffing/automation; the SVG is intended for human review in PRs and release notes.

### 5.4 Regression Guard

`npm run benchmark:check` enables CI thresholds. Thresholds are intentionally conservative for cross-machine variance and are designed to catch major regressions rather than micro-variance.

This is a pragmatic first step. In later phases, thresholds should move from absolute values to baseline-relative windows using dedicated benchmark runners.

## 6. CI Quality Gate

Workflow: `.github/workflows/quality.yml`

Pipeline:

1. checkout
2. node setup
3. dependency install (`npm ci`)
4. invariant tests
5. benchmark with threshold checks
6. artifact upload

This creates minimum merge hygiene in a codebase that currently has broader compile debt.

### 6.1 Why Separate from Deploy

The existing deploy workflow builds full frontend output. Current compile debt can block build completion. A separate quality gate allows:

- immediate enforcement of core system invariants
- objective performance observability
- independent progression of architectural refactoring

This split is common during remediation programs in production systems.

## 7. Current Engineering Risks and Gaps

The project still contains material debt that can affect production reliability:

### 7.1 Type Model Divergence (High)

Large modules (notably in `src/pages` and `src/services`) exhibit type contract drift, naming mismatches, and obsolete fields. This prevents a full strict compile from passing.

Impact:

- reduced confidence in refactoring
- increased runtime mismatch risk between source adapters and UI models

### 7.2 Lint Configuration Gap (Medium)

`package.json` defines lint scripts but no project ESLint config is present. This blocks static style/smell checks.

Impact:

- dead code and anti-pattern detection is weakened
- code review burden increases

### 7.3 Encoding Artifacts in Source Strings (Medium)

Some comments/string literals show mojibake-like corruption, indicating encoding inconsistency in parts of the repository history.

Impact:

- reduced maintainability and readability
- potential user-facing text quality problems

### 7.4 Oversized Page Modules (High)

Very large page files increase blast radius and weaken isolation. Example: `NewsIntelligence.tsx` exceeds 10k lines.

Impact:

- difficult root-cause analysis
- high merge conflict probability
- lower testability and review quality

## 8. Remediation Plan

### 8.1 Phase A (Immediate, 1-2 weeks)

1. Keep invariant + benchmark CI mandatory for all merges.
2. Add ESLint flat config and run against changed files first.
3. Normalize encoding in source and docs (UTF-8 policy).
4. Introduce `src/vite-env.d.ts` and minimal type guards for env usage.

### 8.2 Phase B (Stabilization, 2-4 weeks)

1. Partition oversized pages into feature modules by bounded context:
   - ingestion view model adapters
   - scoring/decision blocks
   - presentation components
2. Enforce source model contracts with dedicated schema/type mapping layer.
3. Add “strict compile target” job for stabilized modules.

### 8.3 Phase C (Systems Maturity, 1-2 months)

1. Deterministic replay framework for event-stream regression tests.
2. Latency budget dashboard (build artifacts + historical trend).
3. Memory/GC benchmark extensions under synthetic burst traffic.
4. Introduce release quality score combining:
   - invariant pass rate
   - benchmark budget compliance
   - type health metrics

## 9. Reproducibility and Operating Instructions

### 9.1 Local Commands

```bash
npm run test:invariants
npm run benchmark
npm run benchmark:check
npm run ci:quality
```

### 9.2 Artifact Review

- Benchmark chart: open `docs/benchmarks/benchmark-latest.svg`
- Raw metrics: open `docs/benchmarks/benchmark-latest.json`

### 9.3 Suggested Pull Request Checklist

1. invariant tests pass
2. benchmark check passes
3. no change introduces unbounded numeric path
4. no change breaks dedupe/list stability assumptions
5. benchmark artifact attached/reviewed when touching data-plane code

## 10. Conclusion

The project is now framed and partially governed as a market systems surface rather than a generic UI:

- core invariants are explicit and executable
- performance is measured and reviewable
- CI enforces a deterministic minimum quality bar
- documentation aligns to infrastructure-level expectations

The remaining risk is concentrated in large unrefactored modules and type drift in service/page integration. The current controls reduce immediate operational risk while creating a structured path to full strict compile and long-term systems maturity.

---

## Appendix A: Deliverables in This Iteration

1. `tests/invariants/safemath.invariant.test.ts`
2. `tests/invariants/realtimeBuffer.invariant.test.ts`
3. `scripts/benchmark.ts`
4. `docs/benchmarks/benchmark-latest.json` (generated)
5. `docs/benchmarks/benchmark-latest.svg` (generated)
6. `.github/workflows/quality.yml`
7. Rewritten `README.md` (systems narrative)

## Appendix B: Next Recommended Metrics

- p99 latency for key transformations
- allocation count and peak RSS under burst update load
- event drop ratio under dedupe and cap policies
- model-mapping error rate by source type
- UI frame stability under stress replay
