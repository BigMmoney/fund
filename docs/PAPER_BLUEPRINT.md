# Paper Blueprint (arXiv + Journal Ready)

## Candidate Titles

1. **Design and Evaluation of a Ledger-First Frequent Batch Auction Market System** (recommended)
2. **A Modular Market Infrastructure Prototype with Frequent Batch Auction Matching and Double-Entry Settlement**
3. **Building a Low-Latency Market Infrastructure with Batch Auction Matching and Deterministic Ledger Settlement**

## Abstract Template

Modern electronic markets rely on low-latency matching engines and reliable settlement infrastructure. However, continuous matching mechanisms often introduce fairness and latency trade-offs.

In this paper we present the design and implementation of a modular market infrastructure prototype that combines a frequent batch auction matching engine with a ledger-first double-entry settlement model.

Our system integrates a matching engine, risk control module, event streaming pipeline, and deterministic ledger layer to ensure fund conservation and replay safety.

We evaluate the system using a synthetic trading workload and demonstrate its ability to process high-throughput order flows while maintaining deterministic settlement semantics.

Our results show that the proposed architecture provides a robust foundation for experimental financial market infrastructure and distributed trading platforms.

## Full Structure

1. Introduction
- Continuous matching pain points: latency arbitrage, fairness issues.
- FBA + ledger-first motivation.
- Contributions (3-4 concrete bullets).

2. Background
- Continuous limit order book.
- Frequent batch auction.
- Double-entry accounting and replay safety.
- Prior work: Budish (2015), exchange infra papers, settlement models.

3. System Architecture
- API Gateway, Matching, Ledger, Risk, Indexer, Event Streaming.
- Include architecture figure.

4. Matching Engine Design
- Batch window semantics.
- Order aggregation and clearing.
- Allocation rules and deterministic tie-break policy.

5. Ledger Model
- Double-entry model.
- Idempotent op handling.
- WAL + recovery model.
- Invariants: conservation, non-negativity, replay safety.

6. Risk Control
- Market state machine.
- Kill switch levels.
- Position/parameter controls.

7. Experimental Evaluation
- Throughput (orders/s, fills/s).
- Latency (p50/p95/p99).
- Batch-window sensitivity (100ms/500ms/1000ms).
- Recovery/replay consistency checks.

8. Discussion
- Fairness vs latency tradeoff.
- Operational complexity and practical constraints.

9. Related Work
- Exchange architecture.
- Distributed event systems.
- DeFi/ledger settlement systems.

10. Conclusion
- Summary of architecture + guarantees + empirical findings.

## Experiment Code Mapping in This Repo

- Order generator: `benchmark/order_generator.go`
- Latency metrics: `benchmark/latency_measurement.go`
- Throughput metrics: `benchmark/throughput_measurement.go`
- Batch-window profile generation: `matching/profile_test.go`
- Generated metrics: `docs/benchmarks/matching_system_profile.json`

## 4-Week Execution Plan

Week 1:
- lock paper structure + intro/background/system sections.

Week 2:
- run throughput/latency/batch-window experiments.

Week 3:
- produce plots, discussion, limitations.

Week 4:
- finalize manuscript and publish preprint (arXiv).

## Candidate Venues

- ICAIF (ACM AI in Finance)
- DEBS
- ICDCS workshops
- selected finance-systems journals (extended version)
