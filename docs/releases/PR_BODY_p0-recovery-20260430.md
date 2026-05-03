## Summary

Backend reliability + recovery work landed on `p0-recovery-20260430`, packaged as **RC 0.1**, plus a complete **Order Flow Monitor** v1 and an end-to-end **Trade Journey Demo**.

Highlights:
- Backend reliability fixes: api crate compile blocker (Group 2c — boxed warp routes cut type-check from infinity-with-SIGTERM to ~30 s), WAL replay determinism (`should_skip_replay_record` now correctly skips `Settled`/`Completed` lifecycles, so the previously-corrupt `data/` snapshot boots cleanly).
- RC 0.1 release package: tag plan + release notes (`docs/releases/RC_0.1_RELEASE_NOTES.md`), staging runbook (`docs/STAGING_RUNBOOK.md`), benchmark report with stability + soak + harness-latency decomposition, security review summary skeleton.
- Trade Journey Demo: `scripts/demo_trade_journey.ps1` — end-to-end one-trade match cycle with narrated transcript suitable for stakeholder walkthroughs.
- Order Flow Monitor: full producer/consumer/REST/WS/JSONL/frontend stack with per-stage emit coverage and a smoke test that verifies the stack end-to-end against a live api boot.
- Backoffice RBAC + Employee Permission design v1 (`docs/BACKOFFICE_RBAC_DESIGN.md`): paper design only — no code. Covers role × level × scope model, 10 employee roles, 28-action permission matrix, single-actor / maker-checker / break-glass approval classes, audit row schema, REST API contract, MVP scope (6 of 10 roles), and 10 numbered security invariants. Follow-up implementation lands in a separate PR.

## Order Flow Monitor features

**REST endpoints** (under `/monitor`, all gated by `with_principal()`; non-admin sees only own orders):
- `GET /monitor/orders` — list summaries, filterable by user_id / market_id / stage / terminal / since_ms / limit (sorted by `last_updated_at` desc, capped at 500).
- `GET /monitor/orders/{order_id}` — single order summary (no timeline). 404 (not 403) for non-owners to avoid leaking existence.
- `GET /monitor/orders/{order_id}/timeline` — windowed timeline, `since_event_id` for incremental polling, capped at 1000 events.

**WebSocket**:
- `/ws/order-trace` — live stream of `Event::OrderTrace` events. Frame protocol: `{"type":"ready"}` on upgrade, `{"type":"trace","event":{...}}` per event, `{"type":"lagged","skipped":n}` on broadcast lag. Same auth model as REST (admin sees all, non-admin sees own subject only).

**JSONL trace trail**:
- `data/trace/order_trace.jsonl` (override via `MONITOR_TRACE_DIR`). Append-only, fsync every 64 events, size-based rotation at 100 MB to dated archives. Per-record recovery events (`recovery_replayed`, `recovery_skipped_terminal`) are filtered at the writer regardless of any flag — design §3.6 says they live on the broadcast channel only and never hit the durable trail. Aggregate `recovery_completed` is always written.

**MonitorPage** (`frontend-modern/src/pages/MonitorPage.tsx`):
- New `/monitor` route in the React workspace shell.
- Filter form (User / Market / Stage / Terminal / Limit) + auto-refresh selector (manual / 2s / 5s / 15s).
- Click any row to load `/monitor/orders/{id}/timeline`; per-stage table with stage / cmd_seq / side / price / amount / remaining / filled / fee / reject.
- Polling-only for now (browser WS can't set custom auth headers — frontend WS upgrade is a deferred follow-up).

**Per-stage emit coverage** (every stage in design §3.1 has a producer):
| Crate | Stages emitted |
|---|---|
| `crates/api` ingress | `api_received`, `api_validated`, `api_rejected` (intent / submit-order / cancel-order / replace-order) |
| `crates/sequencer` | `sequencer_accepted`, `sequencer_persisted`, `wal_appended` |
| `crates/matching` | `matching_resting`, `matching_partially_filled`, `matching_filled`, `matching_cancelled` (incoming taker AND resting maker sides; cancel/replace/mass-cancel paths) |
| `crates/api` projection | `projection_updated` |
| `crates/matching` (per fill) | `ledger_settled` (one per side, at fill construction site after ledger commits succeed) |
| `crates/api` bootstrap | `recovery_completed` (always; aggregate counts) — `recovery_replayed` / `recovery_skipped_terminal` per-record gated on `MONITOR_TRACE_RECOVERY_DETAIL=1` |

## Safety guarantees

- **Observer-only.** Every emit site runs alongside the existing business operation, never inside a critical path that affects state. The `TraceEmitter` trait contract requires implementations to be fire-and-forget (no blocking, no panicking, no error propagation).
- **Fire-and-forget publish.** Producers call `event_bus.publish(Event::OrderTrace(ev))` (a `tokio::sync::broadcast` send) which silently drops messages with no subscribers and never blocks the producer thread. The consumer task forwards into a bounded `mpsc(8192)` via `try_send` — channel-full drops, never blocks.
- **Monitor failure cannot affect matching, ledger, sequencer, WAL, or recovery.** If the consumer task panics, the JSONL writer fails to open, or the projector errors: the publisher side is unaffected. If `event_bus.publish` somehow returned an error, it would be ignored. Order acceptance, validation, sequencing, matching, settlement direction, fee math, WAL append, and recovery loop semantics are all byte-identical with the monitor disabled.
- **JSONL is not the source of truth.** Recovery does NOT replay from `order_trace.jsonl`; the file is for offline analysis only. Loss of the file is not a correctness issue.
- **No business behaviour depends on monitor.** Tests that construct `Sequencer`, `OrderStateProjectionStore`, etc. with `trace_emitter = None` (the default) continue to pass with no special handling.

## Validation

- **Smoke test passes** (`scripts/monitor_smoke_test.ps1`, fresh data dir):
  - Boot → seed → one-trade match → `/monitor/orders` shows both orders → `/monitor/orders/{id}/timeline` shows all 8 expected stages on the buy → JSONL trail has 18 lines including `recovery_completed` and `matching_filled`.
  - End-to-end match latency ~700 µs.
- **`cargo test -p api --bin api`**: 262/262 pass (10 projector + 7 jsonl + 12 http + 5 integration + 228 pre-existing api tests).
- **`cargo test -p matching --lib`**: 105/105 pass (no regressions in matching engine semantics from the new emit calls).
- **`cargo test -p sequencer`**: 16/16 pass (11 pre + 5 emitter tests).
- **`cargo test -p eventbus`**: 7/7 pass.
- **`cargo test -p types`**: 51/51 pass.
- **`recovery_completed` visible on cold boot** (fixed in `96cf916`): the consumer subscribes synchronously and runs *before* `bootstrap_runtime`, so the publish from inside `replay_commands_after_snapshot` lands in the broadcast buffer.
- **Maker / resting-side timeline now accurate** (fixed in `9452eac`): the maker gets its own `matching_filled` / `matching_partially_filled` per fill chunk, so `current_stage`, `fill_count`, and `terminal` reflect actual order state instead of stalling at `matching_resting` from initial placement.
- Frontend `npm run build` and `npm run lint` both clean.

## Known follow-ups

- **Frontend WS auth upgrade deferred.** Browser WebSocket cannot set custom HTTP headers on the upgrade request, so the React MonitorPage cannot consume `/ws/order-trace` directly. Requires a URL-parameter HMAC auth scheme on the backend before the frontend can switch from polling to live streaming. The polling MonitorPage is fully functional; backend `/ws/order-trace` is reachable today via HMAC-supporting CLI clients.
- **Security audit finding-by-finding sign-off pending.** `docs/SECURITY_REVIEW_2026-04-07_SUMMARY.md` is a public-safe skeleton; the original audit (`DEEP_SECURITY_AUDIT_2026-04-07.md`) remains untracked / private. All 22 findings are unsigned in the public summary as of this PR.
- **Throughput baseline + CI perf gate pending.** Run-to-run variance was 19-48% on the dev capture host (see `docs/benchmarks/.../BENCHMARK_REPORT.md`); RTO/RPO baseline is captured (RTO p99 = 0.787 s, RPO = 0) but throughput needs dedicated bench hardware before `bench_compare.ps1` can be wired into `rust-ci.yml` as a regression gate.
- **Wallet / hot-wallet / backoffice RBAC are future design tracks** — out of scope for RC 0.1 + Order Flow Monitor.
