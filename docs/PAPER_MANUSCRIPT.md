# Design and Evaluation of a Ledger-First Frequent Batch Auction Market System

## Abstract

Modern electronic markets depend on low-latency matching and reliable post-trade state transitions, but continuous-time matching architectures often create fairness and complexity tradeoffs. This repository implements a modular market infrastructure prototype that combines frequent batch auction (FBA) matching with a ledger-first settlement model, explicit risk controls, and event-driven state propagation. The system separates matching, ledger mutation, risk management, and indexing into independent services connected through a shared event model. Its design objective is not only throughput, but deterministic behavior under replay, partial fills, and policy changes. We evaluate the prototype using a synthetic workload across multiple batch-window settings and report throughput and latency for 100 ms, 500 ms, and 1000 ms batch intervals. Under a 200 buy/sell-pair scenario in a single market/outcome bucket, the prototype sustains 4958.8 orders/s at a 100 ms batch window and 832.8 orders/s at 500 ms, while preserving deterministic clearing-price selection and exact matched-volume conservation. In addition to performance results, the project encodes system correctness as executable invariants covering matching determinism, ledger conservation, replay idempotency, and monotonic risk-state restrictions. The result is a research-oriented market systems artifact that emphasizes reproducibility, correctness, and explicit engineering tradeoffs over product completeness.

**Keywords:** market infrastructure, frequent batch auction, double-entry ledger, deterministic matching, replay safety, risk controls

## 1. Introduction

Electronic markets are shaped by the interaction between matching policy, settlement semantics, and system latency. In a continuous limit order book, orders are processed serially as they arrive. That design can support high responsiveness, but it also amplifies speed advantages and introduces well-studied fairness concerns at fine time scales. Frequent batch auctions provide a different design point: orders are aggregated over a short discrete interval and cleared together at a single price, shifting competition away from sub-millisecond ordering and toward price formation.

This repository explores that design point in the form of a ledger-first market infrastructure prototype. The system is organized around three principles. First, matching should be deterministic for equivalent batches. Second, settlement should be encoded as explicit double-entry state mutation with replay protection. Third, risk controls should be expressed as executable gates rather than informal operational policy. The implementation therefore combines an FBA matching engine, a double-entry ledger service, a risk service with market-state and kill-switch logic, and an event-driven integration layer.

The project is not a production exchange. It is an executable systems artifact intended to answer a narrower question: can a compact, modular market prototype demonstrate meaningful correctness guarantees and interpretable performance behavior while remaining small enough for inspection and experimentation? The answer is yes, but only if correctness properties are treated as first-class constraints rather than as implied behavior.

This paper makes four concrete contributions:

1. It presents a modular market infrastructure prototype centered on ledger-first settlement and frequent batch auction matching.
2. It implements deterministic matching behavior through explicit clearing-price tie-break rules and exact pro-rata allocation using largest-remainder assignment.
3. It encodes correctness as executable invariants covering matching, ledger, and risk semantics.
4. It evaluates throughput and latency under multiple batch windows and discusses the resulting fairness-versus-latency tradeoff.

## 2. Background and Motivation

### 2.1 Continuous Matching and Latency Competition

Continuous matching remains the dominant design in electronic markets, but its semantics make temporal priority a central determinant of outcome. Budish, Cramton, and Shim argue that this creates an arms race for speed and enables latency-sensitive arbitrage that is rooted in market design rather than individual bad actors [1]. Their proposed alternative is frequent batch auctions: discrete-time uniform-price auctions run at high frequency, such as every 100 ms or 1 s, so that competition is shaped more by price and less by microscopic time precedence.

For a research prototype, FBA has two appealing properties. It makes timing policy explicit, and it creates a natural framework for deterministic batch processing. Those properties align well with systems concerns around replay, testability, and formal reasoning about state transitions.

### 2.2 Ledger-First Settlement

Market systems often fail not because a match cannot be found, but because post-trade state changes become inconsistent under retries, partial fills, or operator intervention. A ledger-first design treats every financial mutation as an explicit debit/credit delta, guarded by validation and idempotency checks before it is committed. This is particularly useful in prototypes, where the matching layer may change frequently but settlement invariants should remain stable.

In the present system, the ledger is deliberately simple: account state is in memory, every delta has an `op_id`, and a write-ahead log slice records committed deltas. This is not durable enough for production, but it is sufficient to make replay semantics concrete and testable.

### 2.3 Event-Driven Integration

The services in this repository communicate through an event bus abstraction rather than direct point-to-point coupling. This follows a common pattern in streaming systems where a log or event layer decouples producers from downstream consumers and allows replay or recovery logic to be expressed explicitly [2]. In this prototype, the event bus is local and in-process rather than distributed, but the architectural role is the same: matching emits fills, the ledger emits commit or rejection events, and the risk service publishes market-state transitions and kill-switch activations.

## 3. System Architecture

### 3.1 Service Decomposition

The repository is organized around several core services:

- `api/`: gateway layer for request ingestion and orchestration.
- `matching/`: frequent batch auction engine.
- `ledger/`: double-entry settlement service.
- `risk/`: market-state and kill-switch controls.
- `indexer/`: event reconciliation and downstream processing hooks.
- `services/`: shared event bus, type definitions, and utilities.

At a high level, the processing path is:

1. client or strategy agent submits an intent through the API layer;
2. the matching engine batches intents by `(market_id, outcome)`;
3. matching computes a clearing price and emits fills;
4. downstream services consume fill events and turn them into ledger deltas;
5. the ledger validates and commits state changes;
6. the risk and indexer layers observe the resulting state and events.

### 3.2 Architectural Data Flow

The architecture can be summarized as:

```text
Client / Strategy
        |
        v
    API Gateway
        |
        +--------------------+
        |                    |
        v                    v
Matching Engine         Risk Service
        |                    |
        v                    |
      Event Bus <------------+
        |
        v
   Ledger Service
        |
        v
  Indexer / Consumers
```

The key design choice is that matching and ledger are distinct responsibilities. Matching decides economic outcome for a batch. Ledger decides whether the resulting state transition is valid and commit-worthy. This separation keeps allocation logic from being entangled with balance validation or replay checks.

### 3.3 Determinism as a Design Constraint

The implementation uses determinism as an explicit systems goal. Two examples are central:

1. Clearing-price tie-breaks are deterministic. When several prices produce equal matched volume, the engine selects the lower price. This removes nondeterminism caused by iteration order over price points.
2. Pro-rata allocation is exact rather than floor-based. Base allocations are computed proportionally, then remaining units are assigned by largest remainder with a stable tie-break on amount and order ID. This guarantees that matched volume is conserved exactly.

These choices are not the only possible policies, but they produce stable and testable semantics.

## 4. Matching Engine Design

### 4.1 Batch Semantics

The matching engine processes intents on a periodic ticker. A batch window is configured at engine construction time, with 500 ms used as the default demonstration value. Every cycle:

1. expired, cancelled, and already filled intents are filtered out;
2. remaining intents are grouped by market and outcome;
3. buy and sell books are aggregated for each group;
4. a clearing price is computed from demand and supply curves;
5. fills are allocated proportionally among eligible participants.

An intent is defined by side, price, amount, market, outcome, and expiry. The engine treats the batch as the unit of market design, rather than the single message arrival.

### 4.2 Clearing Price Computation

For each market/outcome group, the engine constructs demand and supply curves across observed price points. At each candidate price, it computes:

- cumulative demand from buy orders willing to trade at or above that price;
- cumulative supply from sell orders willing to trade at or below that price;
- matched volume as the minimum of the two.

The selected clearing price is the one that maximizes matched volume. If there is a tie, the engine chooses the lowest price. This tie-break rule is intentionally simple and deterministic. More elaborate policies could be adopted, but the current rule makes equivalence testing straightforward and removes map-order nondeterminism.

### 4.3 Allocation Policy

After computing the clearing price, the engine filters eligible buys and sells. Total eligible demand and supply determine matched volume:

\[
V = \min \left(\sum_i b_i,\ \sum_j s_j \right)
\]

Each side is then allocated proportionally. A naive floor-only pro-rata method can under-allocate total volume, especially for small orders. To avoid that drift, the engine performs:

1. a base proportional allocation using integer division;
2. a largest-remainder pass to distribute leftover units;
3. a stable tie-break using remainder, then order amount, then order ID.

This produces exact total allocation equal to `V` while keeping each fill bounded by the originating order size.

### 4.4 Matching Invariants

Three matching invariants are encoded in unit tests:

1. Clearing-price determinism.
2. Buy-side fill volume equals sell-side fill volume.
3. Orders from different market/outcome buckets cannot cross-match.

These are implemented in `matching/main_test.go`. The tests do not only validate outputs; they encode the intended semantics of the engine.

## 5. Ledger Model

### 5.1 Double-Entry State Mutation

The ledger service implements atomic deltas consisting of one or more entries. Each entry carries a debit account, a credit account, an amount, an operation ID, and a timestamp. A delta is committed only if:

1. `op_id` is non-empty and unseen;
2. entries are syntactically valid;
3. accounts have sufficient balance for the aggregate debit effect;
4. the resulting mutation can be applied atomically.

The ledger stores accounts in memory, tracks seen operation IDs for replay protection, and appends committed deltas to an in-memory write-ahead log slice.

### 5.2 Validation Pipeline

The validation path in `CommitDelta` is:

1. reject empty `op_id`;
2. reject duplicate `op_id`;
3. reject malformed or empty entry sets;
4. reject non-positive amounts and self-transfers;
5. verify sufficient balance for non-system accounts;
6. apply entries atomically and increment account versions.

The implementation treats accounts prefixed by `SYS:` as externally funded sources or sinks. This allows deposit-style flows to be modeled without requiring the system vault account to carry a prefunded balance inside the same in-memory ledger.

### 5.3 Replay Safety and WAL Semantics

Replay safety is achieved through `seenOpIDs`. If the same operation is retried, the ledger returns a duplicate-op error and must not mutate state. The write-ahead log is currently a slice stored in memory, which means it is useful for reasoning, testing, and future recovery work, but not yet for crash persistence. This paper therefore describes the WAL as a prototype recovery hook, not as a durable recovery implementation.

### 5.4 Ledger Invariants

The ledger tests encode four primary guarantees:

1. internal transfers conserve total balance;
2. duplicate `op_id` replay does not mutate balances;
3. invalid entries are rejected;
4. account versions advance exactly once per committed delta.

These tests live in `ledger/main_test.go`.

## 6. Risk Control

### 6.1 Market State Machine

The risk service manages market states such as `OPEN`, `CLOSE_ONLY`, `CLOSED`, and `FINALIZED`. Trading permission is a function of both market state and kill-switch level. In the current implementation:

- `OPEN` allows new trading when no kill switch blocks it;
- `CLOSE_ONLY` denies new trading;
- `CLOSED` denies trading entirely;
- `FINALIZED` is intended to trigger settlement workflows.

The system defaults unknown markets to `OPEN` for state lookup. This is pragmatic for a prototype, but it is also a clear place for future tightening in a production design.

### 6.2 Kill Switch Levels

The risk layer exposes escalating kill-switch levels:

- `L1`: block new trading;
- `L2`: block withdrawals;
- `L3`: block chain-signing operations;
- `L4`: force read-only mode.

This monotonic structure allows the system to encode operational response as policy rather than as ad hoc manual intervention.

### 6.3 Dynamic Risk Parameters

For each market, the service can initialize and adjust risk parameters such as:

- maximum position size;
- base fee and current fee;
- batch window in milliseconds;
- maximum slippage.

`TightenRiskParams` increases fees, reduces max position size, and extends the batch window, all within hard caps. `RelaxRiskParams` restores baseline values. This creates a simple but explicit bridge between market conditions and system behavior.

### 6.4 Risk Invariants

The risk tests verify:

1. state transitions correctly gate trading permission;
2. kill-switch levels are monotonic in restriction;
3. parameter tightening respects bounds and relaxation restores defaults.

These invariants are implemented in `risk/main_test.go`.

## 7. Experimental Evaluation

### 7.1 Experimental Setup

The main system profile is generated by `matching/profile_test.go`. Each scenario creates:

- one market and one outcome bucket;
- 200 buy/sell pairs;
- 400 orders total;
- price-compatible orders that can fully match in the batch.

The engine is run with three batch windows: 100 ms, 500 ms, and 1000 ms. For each scenario, the benchmark records:

- total orders and fills;
- total elapsed duration;
- derived orders per second and fills per second;
- p50 and p99 fill latency from intent creation to fill emission.

This is a single-process synthetic benchmark. It does not include network, disk durability, or distributed coordination overhead.

### 7.2 Results

Table 1 reports the generated profile currently committed in `docs/benchmarks/matching_system_profile.md`.

| Batch Window | Orders | Fills | Orders/s | Fills/s | p50 Latency | p99 Latency |
|---|---:|---:|---:|---:|---:|---:|
| 100 ms | 400 | 400 | 4958.8 | 4958.8 | 80.66 ms | 80.66 ms |
| 500 ms | 400 | 400 | 832.8 | 832.8 | 480.29 ms | 480.29 ms |
| 1000 ms | 400 | 400 | 407.4 | 407.4 | 981.85 ms | 981.85 ms |

Two trends are immediate.

First, wider batch windows predictably reduce throughput measured in orders per second, because the system clears less often under the same fixed-size synthetic workload. Second, latency scales with the batch window. The p50 latency moves from 80.66 ms at 100 ms windows to 981.85 ms at 1000 ms windows. This is consistent with the underlying FBA design: larger windows provide more batching opportunity at the cost of increased waiting time.

### 7.3 Interpretation

The benchmark does not show that one window is globally superior. Instead, it quantifies the expected policy tradeoff:

- a 100 ms window reduces waiting time and yields higher observed throughput under this test;
- a 500 ms or 1000 ms window increases temporal aggregation, which may improve fairness or reduce churn in some deployments but raises latency.

For a market-systems artifact, this is an acceptable and useful result. The experiment turns an architectural policy knob into a measurable engineering tradeoff.

### 7.4 Correctness Evaluation via Invariants

Performance results alone would not justify claims about settlement safety or replay behavior. The repository therefore treats invariant suites as part of the experimental evidence:

- matching: deterministic clearing and exact volume conservation;
- ledger: conservation, duplicate-op replay rejection, and entry validity;
- risk: monotonic operational restriction under kill-switch escalation.

These tests run in CI through the repository workflows and are part of the artifact, not a separate unpublished evaluation harness.

## 8. Discussion

### 8.1 Fairness Versus Latency

Frequent batch auctions are attractive because they weaken the economic value of tiny speed differences. However, they do so by introducing bounded waiting time. In this prototype, the batch window becomes the principal control knob for that tradeoff. Short windows preserve responsiveness; longer windows increase aggregation and reduce sensitivity to arrival micro-ordering. The benchmark confirms the expected cost of longer windows in latency terms.

### 8.2 Determinism Versus Policy Richness

The prototype uses a lowest-price tie-break and largest-remainder pro-rata allocation because these are easy to reason about and test. Production venues might choose different rules to optimize for fairness, price improvement, or participant incentives. The current design favors reproducibility over policy richness.

### 8.3 Modularity Versus End-to-End Completeness

The service decomposition is a strength for research and experimentation, but the repository remains incomplete from an operational perspective. The ledger is not durably persisted, the event bus is local rather than distributed, and the indexer/recovery path is not yet measured under crash scenarios. These are deliberate simplifications, not oversights. They allow the prototype to isolate core semantics before operational complexity is added.

### 8.4 Ledger-First Design Value

The strongest argument for the current architecture is not that it is the fastest possible exchange design, but that it is easier to reason about under failure and replay. By forcing all financial mutation through explicit ledger deltas, the prototype makes correctness conditions visible and testable. This is especially valuable in experimental market infrastructure where matching rules may evolve rapidly.

## 9. Limitations and Future Work

The repository has several important limitations.

First, the evaluation is single-node and synthetic. It does not measure distributed event delivery, persistence, or failover. Second, the write-ahead log is in-memory, so crash recovery is not yet implemented end-to-end. Third, the benchmark workload is intentionally simple: one market, one outcome, and fully compatible prices. More varied flow, adversarial order distributions, and multi-market interference should be tested. Fourth, the risk state machine is encoded operationally in code and tests, but not yet specified as a formally checked transition graph. Fifth, there is no fairness study beyond the batch-window sensitivity results; such a study would require richer market simulation.

These limitations motivate the next phase of work:

1. durable WAL persistence and restore-time invariant checks;
2. end-to-end fill-to-ledger-to-risk replay harnesses;
3. multi-market and adversarial benchmark scenarios;
4. property-based testing around matching and ledger deltas;
5. formalization of market-state transitions and operator procedures.

## 10. Related Work

The most direct conceptual influence is the frequent batch auction literature of Budish, Cramton, and Shim [1], which argues that discrete-time clearing can reduce the pathological emphasis on microsecond speed present in continuous markets. More recent work on flow trading extends the broader design space for discrete-time market mechanisms [3].

From an infrastructure perspective, commercial exchange technology emphasizes low-latency matching, broad asset coverage, and operational resilience [4]. This repository does not attempt to match the scale or latency profile of industrial engines. Instead, it borrows the core systems question: how should matching, controls, and post-trade state transitions be composed?

The event-driven side of the design is informed by log-centric data systems such as Kafka, where ordered event streams act as a coordination and recovery substrate for downstream processing [2]. The prototype stops far short of a distributed log architecture, but it adopts the same separation-of-concerns intuition.

## 11. Conclusion

This paper presented a modular market infrastructure prototype built around frequent batch auction matching, ledger-first settlement, and explicit risk gating. The repository shows that even a relatively compact implementation can encode meaningful systems guarantees if determinism, conservation, and replay safety are treated as first-class requirements. Under a synthetic workload of 200 buy/sell pairs, the engine exhibits the expected throughput and latency tradeoffs across 100 ms, 500 ms, and 1000 ms batch windows. More importantly, the artifact turns core correctness claims into executable invariants and CI-enforced regression checks.

The result is not a finished exchange. It is a research-grade systems prototype that demonstrates how market design choices, settlement semantics, and operational policy can be made explicit, testable, and measurable within a single codebase. That makes it a useful foundation for future work on deterministic trading infrastructure, replay-safe settlement pipelines, and experimentally grounded market design.

## References

[1] Eric Budish, Peter Cramton, and John J. Shim. "The High-Frequency Trading Arms Race: Frequent Batch Auctions as a Market Design Response." *The Quarterly Journal of Economics*, 130(4):1547-1621, 2015. DOI: 10.1093/qje/qjv027.

[2] Jay Kreps, Neha Narkhede, and Jun Rao. "Kafka: a Distributed Messaging System for Log Processing." *Proceedings of the NetDB Workshop*, 2011.

[3] Eric Budish, Peter Cramton, Albert S. Kyle, Jeongmin Lee, and David Malec. "Flow Trading." NBER Working Paper 31098, 2023. DOI: 10.3386/w31098.

[4] Nasdaq. "Exchange Matching Engine." product documentation, accessed March 6, 2026.

## Appendix A. Reproducibility

The artifact can be reproduced directly from the repository.

Core invariants:

```powershell
go test ./matching ./ledger ./risk -v
```

Batch-window system profile:

```powershell
$env:RUN_SYSTEM_BENCH="1"
go test ./matching -run TestGenerateMatchingSystemProfile -v
```

Generated benchmark artifacts:

- `docs/benchmarks/matching_system_profile.json`
- `docs/benchmarks/matching_system_profile.md`

The current evaluation corresponds to a single-process local run and should be interpreted as an artifact-level systems profile rather than a production service-level objective.
