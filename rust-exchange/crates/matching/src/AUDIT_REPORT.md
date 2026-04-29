# Matching Engine Core Audit Report

**Date:** 2026-03-24  
**Scope:** `rust-exchange/crates/matching/src/`  
**Focus Areas:** Consistency, Order Book Integrity, STP, Circuit Breakers, Concurrency.

## 1. Consistency Guarantees (WAL & Recovery)

*   **Two-Phase Settlement:** The engine uses a "Prepared/Applied" pattern in the WAL to ensure atomicity. Trades are recorded as `Prepared` before ledger updates and marked `Applied` after successful settlement. This prevents double-spending or lost funds during crashes. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L4200-L4300))
*   **Snapshot Recovery:** State is periodically snapshotted based on command counts (`snapshot_interval_commands`). Snapshots include all resting orders, trigger orders, and market state. Recovery replays from the last snapshot using the `ReplayCursor`. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L1800-L1850))
*   **Deduplication:** A `seen_trade_ids` set is maintained to prevent re-processing trades during WAL replay. This set is compacted during snapshots to manage memory. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L1820))
*   **Invariant:** `replay_cursor.command_seq` is strictly monotonic. Skipped commands during replay are handled gracefully by returning early with a "skipped" result. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L1900))

## 2. Order Book Integrity

*   **Price-Time Priority:** Implemented via `BTreeMap<i64, VecDeque<String>>`. `BTreeMap` ensures price priority (sorted keys), and `VecDeque` ensures FIFO time priority within each price level. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L2650))
*   **Iceberg Replenishment:** Iceberg orders lose their time priority upon replenishment. They are removed from the front of the queue and re-added to the back, ensuring fair queuing for the newly revealed quantity. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L4500))
*   **Fill Accounting:** Fills are generated atomically within the `match_incoming` loop. Each fill includes a `fill_index` to track the sequence of executions for a single aggressive order. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L4100))
*   **Min-Fill Enforcement:** Orders with `min_fill_qty` will skip fills that don't meet the threshold, preventing partial executions that violate user constraints. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L4400))

## 3. Self-Trade Prevention (STP)

*   **Validation Phase:** STP checks occur in `validate_order_acceptance` before any matching logic. It identifies `self_trade_resting_ids` by comparing the incoming order's `user_id` and `stp_group_id` against resting orders. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L3500))
*   **Modes Supported:** `CancelTaker`, `CancelMaker`, and `CancelBoth` are implemented. The logic correctly handles both same-user and cross-user (group-based) prevention. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L3600))
*   **Group Scope:** If `stp_group_id` is present, STP applies across different users sharing the same group ID, which is critical for institutional accounts managing multiple sub-users. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L3550))

## 4. Circuit Breaker & Kill Switch

*   **Rolling Volatility:** The circuit breaker calculates volatility based on the price range (High - Low) over the last `vol_lookback_trades`. It transitions through states: `Normal` → `Stress` → `CancelOnly` → `Halted` based on BPS thresholds. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L5050))
*   **Per-Market Kill Switch:** An admin action can instantly halt a specific market via `MarketKillSwitch`, transitioning it to `Halted` regardless of volatility metrics. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L9550))
*   **Auto-Recovery:** Markets in `CancelOnly` can automatically recover to `Normal` after a configurable number of consecutive successful new orders, allowing for gradual normalization without manual intervention. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L5150))

## 5. Concurrency & Race Conditions

*   **Actor Model:** The `PartitionedMatchingEngine` uses an actor model where each partition runs in its own Tokio task (`run_partition`). Communication is exclusively via `mpsc` channels. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L1750))
*   **Single-Threaded Access:** Within each partition, `MarketRuntime` instances are accessed only by the partition's event loop. There are no shared mutable references (`Arc<Mutex>` or `RwLock`) around the order books themselves, eliminating internal race conditions. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L1700))
*   **No Unsafe Code:** The implementation relies entirely on safe Rust primitives. Shared global state (like the instrument registry or kill switch) uses `Arc<AtomicBool>` or thread-safe registries, avoiding complex locking patterns.
*   **Batch Processing:** The event loop drains messages in batches (size 32) to improve throughput while maintaining sequential consistency within the partition. ([partitioned.rs](d:\pre_trading\rust-exchange\crates\matching\src\partitioned.rs#L1700))

## Summary

The matching engine core demonstrates high engineering quality. The use of an actor-based partitioned architecture provides excellent scalability while simplifying concurrency control. The two-phase WAL settlement and rigorous STP validation provide strong consistency and safety guarantees. No critical race conditions or logical flaws were identified in the reviewed code.
