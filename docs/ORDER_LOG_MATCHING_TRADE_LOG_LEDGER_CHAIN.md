# Order Log -> Matching -> Trade Log -> Ledger Chain

## Goal

Define the exchange core write path so deployment, benchmarking, and future refactors preserve the same invariants:

- every accepted order is sequenced exactly once
- matching decisions are deterministic inside a partition
- fills are journaled before they are treated as durable trade facts
- ledger state is the financial source of truth
- crash recovery can replay the chain without inventing or losing money

## Core Stages

### 1. Order Log

The order log is the sequenced command stream. In the current codebase this is centered on the sequencer WAL and `command_seq`.

Responsibilities:

- accept authenticated write intents from the API layer
- assign a durable monotonic sequence number
- persist enough metadata for replay and deduplication
- preserve idempotency with `op_id` / `request_id`

Required record fields:

- `command_seq`
- `request_id`
- `op_id`
- `user_id`
- `principal_role`
- `command_type`
- `market_id`
- `partition_key`
- `payload`
- `accepted_at`

Key invariant:

- nothing enters matching as a durable write without first existing in the order log

### 2. Matching

Matching consumes ordered commands partition by partition. The partition boundary is the concurrency boundary.

Responsibilities:

- rebuild per-market order book state from snapshot + sequencer replay
- enforce price-time priority
- apply cancel / replace / self-trade prevention rules
- emit deterministic fill events and order state transitions

Required outputs:

- accepted order state transition
- rejected order state transition
- zero or more fills
- updated resting book state
- snapshot checkpoints

Key invariants:

- same ordered input yields the same fill stream
- no fill is emitted without a corresponding order transition
- partition replay never skips valid commands after snapshot boundaries

### 3. Trade Log

The trade log is the durable business fact stream for fills. It is separate from ephemeral book mutation.

Responsibilities:

- persist every fill with stable trade identifiers
- store maker / taker identities, prices, quantities, fees, and timestamps
- support audit, reconciliation, and downstream read models
- act as the replayable source for trade history and settlement verification

Required record fields:

- `trade_id`
- `command_seq`
- `market_id`
- `maker_order_id`
- `taker_order_id`
- `maker_user_id`
- `taker_user_id`
- `price`
- `amount`
- `fee_model`
- `created_at`

Key invariants:

- every ledger settlement caused by matching must be traceable back to one or more trade-log entries
- trade IDs are unique and replay-stable

### 4. Ledger

The ledger is the financial source of truth for balances, holds, positions, and realized transfers.

Responsibilities:

- reserve balances and positions before match execution
- settle fills into cash, inventory, margin, fee, and position deltas
- maintain double-entry style invariants where applicable
- reject impossible states instead of silently repairing them

Required write categories:

- balance hold / release
- spot inventory hold / release
- derivative position updates
- fee debits / credits
- funding / liquidation side effects
- transfer and governance side channels

Key invariants:

- no money or inventory is created by replay
- every settled fill has a trade-log origin
- reserve and release operations converge to the same final state after replay

## Recommended Runtime Flow

```text
HTTP/API
-> auth / principal
-> risk pre-check + reserve intent
-> order log append (sequencer WAL)
-> partitioned matching
-> trade log append
-> ledger commit
-> projections / websocket / metrics
-> snapshot checkpoint
```

## Failure Handling

### Before Order Log Append

- request can fail safely
- no replay responsibility exists yet

### After Order Log Append, Before Matching Commit

- replay must resume from `command_seq`
- duplicate API retries must collapse through idempotency keys

### After Matching, Before Trade Log / Ledger Durability

- market should halt if atomicity cannot be guaranteed
- do not expose partial fills as final facts

### After Trade Log, Before Ledger Commit

- recovery must either re-drive settlement deterministically or block startup with operator-visible failure

### After Ledger Commit

- projections and websocket delivery are downstream and can be replayed or rebuilt

## Recovery Model

Recovery should continue to rely on:

```text
matching snapshot + partition-aware sequencer replay + ledger/trade-log rehydration
```

Operational rules:

- replay starts from the last partition-safe checkpoint
- the trade log and ledger must reconcile against the same `command_seq` frontier
- startup should fail closed if command, trade, and ledger frontiers disagree materially

## Scaling Direction

### Near-Term

- keep a single financial truth in the ledger
- scale matching by partition and market affinity
- keep trade-log writes append-only

### Medium-Term

- split the order log into partition shards with a global monotonic envelope
- create dedicated reconciliation jobs:
  - order log vs matching snapshot
  - trade log vs ledger postings
  - ledger vs user-facing projections

## What To Measure

For every stage, expose P50/P95/P99:

- order-log append latency
- queue wait before partition execution
- matching-core latency
- trade-log append latency
- ledger commit latency
- end-to-end request latency

Also track:

- replay duration
- WAL corruption / checksum failures
- settlement rollback / halt count
- reconciliation mismatch count

## Design Next Steps

1. Introduce an explicit trade-log frontier metric tied to `command_seq`.
2. Add a daily reconciliation job that validates `Order Log -> Trade Log -> Ledger`.
3. Define a fail-closed policy for `trade-log durable but ledger failed` and `ledger durable but trade-log failed`.
4. Separate benchmark scenarios for order-log pressure, matching pressure, and settlement pressure so performance regressions can be localized quickly.
