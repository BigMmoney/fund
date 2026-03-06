# Technical Report
## Market Infrastructure / Systems Project Artifact

Date: 2026-03-06  
Repository: `BigMmoney/fund`  
Scope: `frontend-modern` + core execution services (`matching`, `ledger`, `risk`)

---

## 1. Problem

Modern market-facing systems fail less often from UI rendering bugs and more often from state inconsistency under bursty event streams:

- inconsistent matching outcomes under equivalent order sets
- replayed operations causing duplicate financial effects
- risk controls not enforcing monotonic restriction semantics
- data-plane numerical contamination (`NaN`/`Infinity`) propagating to decisions

This project is framed as a market infrastructure artifact, not a generic frontend app.  
The engineering objective is to enforce deterministic behavior and safety properties across:

1. event ingestion and transformation
2. matching and allocation
3. ledger mutation and replay protection
4. risk-state controls
5. operator-facing rendering

Success criteria:

- explicit invariants with executable tests
- benchmarked throughput for critical hot paths
- CI quality gates for regression containment
- documentation suitable for citation in technical review/interview context

---

## 2. System Design

### 2.1 Layered Architecture

The system is organized as three interacting planes.

1. Data Plane
- numerical safety, buffering, change classification (`frontend-modern/src/lib/*`)
- matching and allocation (`matching/main.go`)
- ledger mutation pipeline (`ledger/main.go`)

2. Control Plane
- market-state transitions
- kill-switch controls
- dynamic risk-parameter tightening/relaxing (`risk/main.go`)

3. Presentation Plane
- operator terminal, policy/news dashboards, and review surfaces (`frontend-modern/src/pages/*`)

### 2.2 Key Design Choice: Determinism as First-Class Constraint

Two examples implemented in this revision:

1. Matching clearing-price tie behavior is deterministic.
- Equal-volume ties now resolve by lowest price, removing map-iteration nondeterminism.

2. Pro-rata allocation is conservation-safe.
- Largest-remainder allocation is used so allocated volume exactly matches batch matched volume (instead of floor-only under-allocation drift).

### 2.3 Idempotency and Safety Boundaries

- Ledger mutation is guarded by `op_id` replay rejection.
- Invalid entries are rejected early (empty op_id, empty entries, non-positive amount, self-transfer).
- System-prefixed accounts (`SYS:*`) are treated as externally funded for balance-check semantics.

---

## 3. Invariants

This section lists invariants treated as hard constraints with executable tests.

### 3.1 Matching Correctness Invariants

Files:
- `matching/main.go`
- `matching/main_test.go`

Invariants:

1. Clearing Price Determinism
- For equivalent order books, clearing price selection is deterministic.
- Tie-break for equal matched volume: choose lower price.

2. Volume Conservation Across Sides
- At batch clearing, total buy fill amount equals total sell fill amount.
- No fill exceeds its originating order amount.

3. Market/Outcome Isolation
- Intents from different `(market, outcome)` groups must not cross-match.

### 3.2 Ledger Invariants

Files:
- `ledger/main.go`
- `ledger/main_test.go`

Invariants:

1. Replay/Idempotency
- A previously committed `op_id` is rejected on replay.
- Replayed operation must not mutate balances or total ledger sum.

2. Conservation
- Sum of all account balances remains unchanged by a valid delta (internal transfer conservation).

3. Entry Validity
- Delta must have non-empty `op_id` and at least one entry.
- Amounts must be strictly positive.
- Debit and credit accounts cannot be identical.

4. Version Progress
- Affected accounts increment version exactly once per committed delta.

### 3.3 Risk-State Transition Invariants

Files:
- `risk/main.go`
- `risk/main_test.go`

Invariants:

1. State-Driven Trade Permission
- `OPEN` allows trading (subject to kill switch).
- `CLOSE_ONLY` and `CLOSED` deny new trading.

2. Kill-Switch Monotonic Restriction
- `L1` blocks trading.
- `L2` blocks withdrawals.
- `L3` blocks chain-sign operations.
- `L4` enforces read-only mode.

3. Parameter Bounds Under Tightening
- Tightening respects fee and batch-window caps.
- Relax restores baseline parameter profile.

### 3.4 Frontend Data-Plane Invariants

Files:
- `frontend-modern/tests/invariants/safemath.invariant.test.ts`
- `frontend-modern/tests/invariants/realtimeBuffer.invariant.test.ts`

Invariants:

- finite output guarantees for safe math
- bounded clamp behavior
- dedupe correctness in real-time buffer
- max-buffer enforcement
- change classification precedence consistency

---

## 4. Evaluation

### 4.1 Method

Benchmark runner:
- `frontend-modern/scripts/benchmark.ts`

Artifacts:
- `frontend-modern/docs/benchmarks/benchmark-latest.json`
- `frontend-modern/docs/benchmarks/benchmark-latest.svg`

The benchmark measures ops/sec and timing stats over controlled iteration/batch windows for core data-plane primitives.

### 4.2 Benchmark Results (Latest Artifact)

Representative results:

- `safeDivide hot path`: **~123.6M ops/sec** (200k micro-batch)
- `safeWeightedAverage`: **~4.03M ops/sec**
- `classifyChange`: **~16.9M ops/sec**
- `StableList.update (batch=128)`: **~17.8k ops/sec**
- `SignalHysteresis.addSample`: **~1.41M ops/sec**
- `formatPercentChange`: **~8.27M ops/sec**

Interpretation:

- Numeric safety primitives are not a throughput bottleneck.
- State-heavy list update path remains stable in burst-like batch settings.
- Stream-smoothing path supports high update frequency with safety margins.

### 4.3 Test Outcomes

Current invariant suites (frontend + core services) pass locally:

- frontend invariants (safemath/realtimeBuffer)
- matching invariants
- ledger invariants
- risk-state invariants

### 4.4 CI Integration

CI workflows:

1. `frontend-modern/.github/workflows/quality.yml`
- runs frontend invariants and benchmark regression check

2. `.github/workflows/system-invariants.yml`
- runs Go invariants (`go test ./matching ./ledger ./risk -v`)

This split allows targeted quality gates for both JS/TS and Go system layers.

---

## 5. Tradeoffs

### 5.1 Determinism vs Price-Selection Optimality Complexity

Choosing lowest price on equal-volume tie is simple and reproducible, but it is only one of many fair auction tie policies.  
Alternative policies (midpoint minimization, surplus fairness, time-priority overlays) may optimize different objectives with higher complexity.

### 5.2 Strict Entry Validation vs Integration Flexibility

Ledger now rejects malformed deltas aggressively.  
This increases safety but may require adaptation in upstream producers that previously relied on permissive behavior.

### 5.3 Throughput Benchmarks vs End-to-End Latency

Micro-benchmarks isolate data-plane hot paths and are excellent regression sentinels.  
They do not replace full path SLO measurements including network, persistence, and scheduler effects.

### 5.4 System Account Semantics

Allowing `SYS:*` accounts to bypass insufficient-fund checks simplifies external-flow modeling (e.g., deposit bridge).  
This should be combined with explicit reconciliation and audit controls to prevent silent drift.

---

## 6. Limitations

1. Full strict type health is not yet achieved across all large frontend pages/services.
2. Benchmark currently reports per-process local execution; no cross-host normalized baseline registry yet.
3. Matching engine still lacks explicit fairness/priority policy documentation beyond deterministic tie-break.
4. Ledger currently uses in-memory state + WAL slice; no durable storage/recovery replay pipeline yet.
5. Risk transitions are policy-consistent but not yet constrained by a formally declared transition graph.

---

## 7. Practical Citation Snippets

### 7.1 Resume-Friendly (Single Line)

- Achieved **123.6M ops/sec** on core safety primitive under controlled 200k-iteration workload with CI-tracked benchmark artifacts.
- Implemented invariant-driven matching/ledger/risk test suites covering determinism, conservation, idempotency, and monotonic safety gates.
- Built dual-language quality gates (TS + Go) enforcing benchmark regression checks and core system invariants on each push/PR.

### 7.2 Interview/Review-Friendly

- “We replaced floor-only pro-rata allocation with largest-remainder allocation to guarantee matched-volume conservation and deterministic behavior.”
- “Replay idempotency is tested at ledger level by asserting duplicate `op_id` rejection with zero state mutation.”

---

## 8. Reproducibility

Frontend quality:

```bash
cd frontend-modern
npm install
npm run test:invariants
npm run benchmark:check
```

Core system invariants:

```bash
cd ..
go test ./matching ./ledger ./risk -v
```

---

## 9. Next Steps

1. Add explicit transition graph validation for `MarketState` changes.
2. Add deterministic replay harness for fill->ledger->risk event chains.
3. Add p99 and memory-pressure benchmarks for burst windows.
4. Add property-based fuzz tests around matching and ledger delta generation.
5. Add persistent WAL + restore-time invariant rechecks.
