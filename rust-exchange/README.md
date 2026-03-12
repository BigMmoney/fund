# Rust Exchange

`rust-exchange` is the canonical trading core for this repository.

If older notes, legacy prototypes, or compatibility-layer behavior conflict with the
current Rust implementation, the Rust code and the latest architecture / validation
documents in this directory should be treated as the source of truth.

## Project Position

The repository is no longer in a dual-core state.

- `rust-exchange` owns trading truth.
- The Go API is a compatibility and migration layer.
- Frontends and scripts must go through the Rust auth, risk, matching, and ledger path
  to create real state.

In practice, this means:

- order truth is in Rust
- trade truth is in Rust
- balance and position truth are in Rust
- recovery truth is in Rust
- risk execution truth is in Rust

## One-Line Architecture

Canonical runtime path:

`HTTP Gateway -> Auth/Principal -> Sequencer WAL -> Risk Reserve/Check -> Partitioned Matching -> Ledger Commit -> Trade Journal -> Snapshot + WAL Replay -> Read Models / Risk Automation`

## Full Architecture Map

```text
                      Client / Scripts / Frontend
                                 |
                                 v
                     +---------------------------+
                     | crates/api                |
                     |---------------------------|
                     | HTTP routes               |
                     | auth / admin guard        |
                     | rate limit                |
                     | body limit / rejections   |
                     | startup recovery          |
                     | risk automation schedulers|
                     +-------------+-------------+
                                   |
                                   v
                     +---------------------------+
                     | crates/sequencer          |
                     |---------------------------|
                     | command_seq               |
                     | lifecycle tracking        |
                     | command WAL               |
                     +-------------+-------------+
                                   |
                                   v
                     +---------------------------+
                     | crates/risk               |
                     |---------------------------|
                     | reserve / release         |
                     | reduce_only checks        |
                     | margin snapshot           |
                     | liquidation evaluation    |
                     | funding preview           |
                     | liquidation execution     |
                     | funding batch settlement  |
                     +-------------+-------------+
                                   |
                                   v
                     +---------------------------+
                     | crates/matching           |
                     |---------------------------|
                     | partitioned engine        |
                     | order book runtime        |
                     | price-time priority       |
                     | self-trade prevention     |
                     | replace(cancel+new)       |
                     | snapshot export / restore |
                     | replay-aware recovery     |
                     +-------------+-------------+
                                   |
                                   v
                     +---------------------------+
                     | crates/ledger             |
                     |---------------------------|
                     | balances / holds          |
                     | spot positions            |
                     | derivative positions      |
                     | op_id dedupe              |
                     | WAL-backed commit         |
                     | internal cash transfer    |
                     +-------------+-------------+
                                   |
                +------------------+------------------+
                v                                     v
   +---------------------------+        +---------------------------+
   | trade journal / snapshots |        | projections / read models |
   |---------------------------|        |---------------------------|
   | trade journal WAL         |        | positions                 |
   | partition snapshots       |        | margin                    |
   | restore + replay boundary |        | pnl                       |
   +---------------------------+        +---------------------------+

                                   v
                     +---------------------------+
                     | risk automation runtime   |
                     |---------------------------|
                     | auto liquidation scan     |
                     | auto liquidation execute  |
                     | funding rate store        |
                     | funding batch settle      |
                     | automation audit WAL      |
                     +---------------------------+
```

## Workspace Layout

### `crates/types`

Shared domain types:

- commands and command metadata
- order fields and enums
- `InstrumentKind`, `InstrumentSpec`, `MarginMode`
- principal, roles, lifecycle types

### `crates/eventbus`

Lightweight in-process event fanout.

### `crates/persistence`

Append-only WAL abstractions used by:

- sequencer
- ledger
- matching snapshots
- trade journal
- instrument registry
- funding rate control plane
- risk automation audit stream

### `crates/sequencer`

Owns:

- monotonic `command_seq`
- lifecycle markers
- replay source for post-snapshot recovery

### `crates/ledger`

Owns financial state:

- available cash
- held cash
- spot balances
- spot holds
- derivative position balances
- idempotent `op_id` dedupe
- internal cash transfer primitive

### `crates/instruments`

Owns product metadata:

- `InstrumentRegistry` abstraction
- persistent instrument registry
- fallback market-to-instrument inference

### `crates/risk`

Owns pre-trade and post-trade risk logic:

- reserve / release
- reduce-only checks
- settlement decisions
- margin snapshots
- liquidation evaluation
- funding preview
- manual and automated liquidation execution
- manual and batch funding settlement

### `crates/matching`

Owns the matching state machine:

- partitioned order processing
- price-time priority
- self-trade prevention
- replace semantics
- snapshot export and restore
- partition-aware replay boundary logic

### `crates/projections`

Owns read-model transformations:

- positions
- margin
- pnl

Current implementation is pull-based, not yet a separate projection service.

### `crates/api`

Owns runtime composition:

- route wiring
- principal extraction
- user / admin authorization boundaries
- CORS / body limits / structured errors
- startup recovery bootstrap
- funding-rate control plane
- risk automation schedulers

Current route/module split:

- `crates/api/src/trading.rs` ? user trade write path (`intent`, submit, cancel, replace, user/session mass cancel)
- `crates/api/src/control.rs` ? admin trade control path (deposit, market mass cancel, kill-switch, market-state, reference price)
- `crates/api/src/accounts.rs` ? balances, positions, margin, pnl, orders, deposits
- `crates/api/src/markets.rs` ? markets, market detail, order book, trades, history, stats, matching status
- `crates/api/src/admin.rs` ? admin instruments, funding-rate control plane, risk events, funding settlement
- `crates/api/src/pricing.rs` ? index source store, arbitration, fair-price routes
- `crates/api/src/governance.rs` ? pending governance actions, dual approval workflow
- `crates/api/src/liquidation.rs` ? liquidation queue override, liquidation worker, auction and insurance routes
- `crates/api/src/security.rs` ? internal auth verification, principal filters, role/subject guards
- `crates/api/src/helpers.rs` ? request-id normalization, audit helpers, lifecycle marker helpers
- `crates/api/src/stores.rs` ? persistent store builders and registry seed wiring
- `crates/api/src/bootstrap.rs` ? runtime bootstrap, WAL recovery, partition-aware replay, automation task startup
- `crates/api/src/main.rs` ? top-level route composition, CORS/static wiring, HTTP server entrypoint

## Current Product Scope

Implemented as real core semantics today:

### Spot

- buy-side cash reservation
- sell-side spot inventory reservation
- spot settlement through ledger

### Margin

- leverage-aware order fields
- derivative-style position path
- margin snapshot and liquidation evaluation

### Perpetual

- derivative position accounting
- funding preview and settlement
- manual and automated liquidation path
- margin snapshot support

Not yet formally implemented as finished products:

- delivery futures
- options
- OTC negotiation venue
- wealth / structured products

## Canonical Write Path

Official write path:

`gateway -> auth/validate -> sequencer WAL -> risk reserve -> partitioned matching -> ledger settle -> trade journal -> snapshot/replay`

Expanded flow:

1. `api` accepts the request
2. principal is derived from authenticated request context
3. user and admin actions are separated
4. `sequencer` appends command WAL and assigns `command_seq`
5. `risk` validates and reserves
6. `matching` executes the partition-local state machine
7. `ledger` commits financial state changes
8. trade journal records fill facts
9. partition snapshots persist runtime state
10. restart uses snapshot restore plus partition-aware sequencer replay

## Recovery Model

Current recovery is not snapshot-only.

Current recovery is:

`snapshot restore + sequencer WAL replay(after per-partition snapshot boundary)`

Important property:

- replay is decided per partition
- each partition tracks its own `last_applied_command_seq`
- one fast partition snapshot cannot cause another partition to skip valid commands

## Matching Engine Rules

### Price-Time Priority

Current matching uses standard price-time priority.

### Self-Trade Prevention

Default policy is:

- `reject taker`

The engine rejects the incoming taker-side action instead of mutating the resting side
as an implicit side effect.

### Replace Semantics

Replace is explicitly modeled as:

- `cancel + new`
- priority is lost
- outward behavior must remain atomic

Meaning:

- invalid replacement does not destroy the valid resting order
- the engine no longer performs destructive replace-before-validation behavior

### Reduce-Only

Reduce-only is not a naive balance-presence check.

It validates against:

- net position
- already reserved sell quantity

### Failure Consistency

Current engine fixes guarantee:

- settlement failure does not leave half-applied book state
- trade journal failure does not leave partial trade state
- severe commit-path failure halts the market instead of silently corrupting state

## Security Defaults

### Bind Defaults

Rust API binds to loopback by default:

- `127.0.0.1`

### Identity Model

The system does not trust caller-supplied `user_id` in request bodies as the source of
identity.

Current rule:

- user actions derive subject from authenticated context
- admin actions require admin role

### Admin-Only Control Plane

Admin-only controls include:

- kill switch
- market-state changes
- manual reference-price updates
- instrument registry updates
- liquidation execution
- funding settlement execution
- funding-rate control plane
- risk automation event inspection

### Request Guardrails

Current API runtime includes:

- IP rate limit
- user-level rate limit
- stricter admin rate limit
- request body size limit
- structured rejection handling

## Risk Automation

The system now contains basic automated execution, not only risk evaluation.

### Automated Liquidation

When `RISK_AUTOMATION_ENABLED=true`, runtime can:

1. export market snapshots on a schedule
2. use `last_trade_price` or `reference_price` as mark price
3. scan all known ledger users
4. compute liquidation candidates
5. transfer underwater positions to a designated liquidator account
6. collect available-cash penalty to the liquidator
7. append queued / success / error events to the automation audit WAL

### Automated Funding

Runtime can:

1. read funding rates from a persistent funding-rate store
2. scan long and short derivative holders
3. pair counterparties in batch
4. execute cash transfers for funding settlement
5. append success / skipped / error events to automation audit WAL

### Current Limits of Automation

This is still a v1 automation model:

- liquidation transfers directly to a designated liquidator account
- it is not yet an auction-style liquidation engine
- funding rates are operator-managed control-plane values
- rates are not yet derived automatically from premium / index data

## Read Models and Query Surface

Current read-model endpoints include:

- `GET /positions/:user_id`
- `GET /margin/:user_id`
- `GET /pnl/:user_id`
- `GET /markets`
- `GET /markets/:market_id`
- `GET /markets/:market_id/book`
- `GET /trades`
- `GET /orders/:user_id`
- `GET /stats`

Admin read / control endpoints include:

- `GET /admin/instruments`
- `POST /admin/instruments`
- `GET /admin/risk/funding-rates`
- `POST /admin/risk/funding-rates`
- `GET /admin/risk/events`
- `POST /admin/risk/liquidations/execute`
- `POST /admin/risk/funding/settle`
- `GET /matching-status`

## Major Correctness Fixes Already Landed

The current core already includes fixes for these previously high-risk issues:

- non-atomic replace that could remove a valid resting order
- partition inflight accounting race causing invalid queue-depth values
- snapshot / replay boundary bug that could skip slower-partition commands
- settlement and trade-journal failure paths that could leave partial local state
- in-memory-only instrument registry with no persistent control-plane truth
- risk layer limited to evaluation only, without basic execution flows
- missing formal automation audit stream

## Known Remaining Gaps

These are known next-stage items, not hidden bugs.

### Liquidation System Is Still Incomplete

Not yet implemented:

- insurance fund
- bankruptcy price
- liquidation waterfall
- liquidation auction / liquidation order book

### Funding System Is Still Incomplete

Not yet implemented:

- premium / index based automatic rate generation
- stricter funding-clock semantics
- better global netting and batch optimization

### Read Side Is Still Lightweight

Not yet implemented:

- standalone projection service
- richer position / margin / risk monitoring views
- separate market-data / audit read store

### More Product Lines Remain Future Work

Not yet formally complete:

- delivery futures
- options
- OTC
- wealth / structured assets

## Recommended Reading Order

For a fast but accurate understanding, read in this order:

1. `README.md`
2. `ARCHITECTURE_ZH_CN.md`
3. `ARCHITECTURE_HANDOFF_2026-03-11.md`
4. `REMAINING_ARCHITECTURE_AND_CODE_DESIGN_ZH_2026-03-11.md`
5. validation reports:
   - `VALIDATION_REPORT_2026-03-11_REGISTRY_RISK_PROJECTIONS.md`
   - `VALIDATION_REPORT_2026-03-12_LIQUIDATION_FUNDING_EXECUTION.md`
   - `VALIDATION_REPORT_2026-03-12_RISK_AUTOMATION.md`

## Bottom Line

The most accurate summary of the current system is:

- Rust is the only formal exchange core
- the matching path is a credible v1 correctness foundation
- recovery is snapshot plus partition-aware replay, not snapshot-only
- instrument registry, margin snapshot, and position / pnl / margin projections are now
  part of the formal runtime path
- liquidation and funding now have both execution and automation primitives
- the system is still not a fully finished production venue

Best short description:

> This is a Rust exchange core v1 that has already closed its core correctness loop,
> entered the risk-automation stage, and now needs deeper production subsystems rather
> than another rewrite of the main trading truth.


## Local Docs

Current Chinese handoff / audit documents in this directory:

- `ARCHITECTURE_ZH_CN_2026-03-12.md` ? full Chinese architecture overview
- `CONCURRENCY_ROUTE_AUDIT_ZH_2026-03-12.md` ? route split and lock/concurrency audit
- `API_REMAINING_COUPLING_AUDIT_ZH_2026-03-12.md` ? remaining API coupling review and next split plan
- `API_DTO_SPLIT_AUDIT_ZH_2026-03-12.md` ? DTO split and API boundary audit
- `API_BOOTSTRAP_STORES_AUDIT_ZH_2026-03-12.md` ? stores/bootstrap split audit and next-step plan
- `API_BOOTSTRAP_CONTEXT_AUDIT_ZH_2026-03-12.md` ? bootstrap/app context split audit and remaining closure plan
- `RUNTIME_ERROR_AUDIT_ZH_2026-03-12.md` ? confirmed fixes, remaining issues, and static audit findings
- `VALIDATION_REPORT_2026-03-12_ARBITRATION_LADDER_DUAL_APPROVAL.md` ? pricing arbitration / liquidation ladder / dual approval validation
- `FINAL_WORKSPACE_VALIDATION_REPORT_2026-03-12.md` ? final workspace-wide validation closure report
- `TRADING_RISK_STATE_MACHINE_ZH_2026-03-12.md` ? formal Chinese trading/risk state-machine report
- `ARCHITECTURE_MERMAID_ZH_2026-03-12.md` ? Chinese architecture diagrams with Mermaid
- `TRADING_RULE_AUDIT_ZH_2026-03-12.md` ? line-by-line trading-rule audit report
- `LARGE_CHAIN_TEST_REPORT_2026-03-12.md` ? crash recovery / latency / 1k-10k scale test report

Recommended read order:

1. `README.md`
2. `ARCHITECTURE_ZH_CN_2026-03-12.md`
3. `CONCURRENCY_ROUTE_AUDIT_ZH_2026-03-12.md`
4. `API_REMAINING_COUPLING_AUDIT_ZH_2026-03-12.md`
5. `API_BOOTSTRAP_STORES_AUDIT_ZH_2026-03-12.md`
6. `API_BOOTSTRAP_CONTEXT_AUDIT_ZH_2026-03-12.md`
7. `RUNTIME_ERROR_AUDIT_ZH_2026-03-12.md`
8. `FINAL_WORKSPACE_VALIDATION_REPORT_2026-03-12.md`
9. `TRADING_RISK_STATE_MACHINE_ZH_2026-03-12.md`
10. `ARCHITECTURE_MERMAID_ZH_2026-03-12.md`
11. `TRADING_RULE_AUDIT_ZH_2026-03-12.md`
12. `LARGE_CHAIN_TEST_REPORT_2026-03-12.md`
