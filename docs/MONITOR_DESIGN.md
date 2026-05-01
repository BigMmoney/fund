# Real-Time Order Flow Monitor — Design

> **Scope of this document.** Architecture and API contract for an order-lifecycle monitor that observes every stage from API submission through recovery replay. **Behavior-preserving instrumentation only.** No business logic changes anywhere.
>
> **Companion type model:** `crates/types/src/order_trace.rs` (lands in the same commit as this design doc).
>
> **Status:** v1 design + types only. **No emission, no routes, no projector, no WS** are wired in this commit. Those land as separate follow-up commits per §7.

## 1. Goals and non-goals

### Goals
- Single coherent timeline per order, observable in real time and post-hoc.
- Cover **every lifecycle stage** the user enumerated: API, sequencer, matching, projection, settlement, WAL, recovery.
- **Zero behavior change.** Adding instrumentation only.
- Front-end-ready output (stable schema, WS broadcast, REST queries).
- Lightweight v1: in-memory state + append-only JSONL trace log.

### Non-goals (v1)
- No persistent database for trace events.
- No cross-process aggregation (single-node only).
- No retention policy / index / search; just a ring buffer + JSONL with size-cap rotation.
- No tracing-grade sampling — one event per stage per order.
- Not a replacement for `/metrics` or the existing `tracing::info!` log.

## 2. Architecture

### 2.1 High-level flow

```
                                       ┌──────────────────────────┐
   api submit → api_received           │ TraceBus (broadcast       │
              ↓ validation             │   channel, in-process)    │
              api_validated/api_rejected├─→ TraceProjector ─→ in-mem│
              ↓ sequencer              │     order index           │
              sequencer_accepted       │                           │
              sequencer_persisted      │   Append-only JSONL ──────┼─→ data/trace/order_trace.jsonl
              ↓ matching               │                           │
              matching_resting/partial ├─→ WS hub /ws/order-trace ─┼─→ frontend monitor page
              matching_filled/cancelled│                           │
              ↓ projections + ledger   │   REST handlers           │
              projection_updated       │   /monitor/orders         │
              ledger_settled           │   /monitor/orders/{id}    │
              ↓ wal append             │   /monitor/orders/{id}/timeline
              wal_appended             │                           │
                                       └──────────────────────────┘
   bootstrap replay → recovery_completed (always; aggregate counts)
                    → recovery_replayed / recovery_skipped_terminal
                                  (debug-only, off by default; see §3.6)
```

### 2.2 Crate placement

| Crate | Role |
|---|---|
| `crates/types` | `OrderTraceEvent` struct + `OrderTraceStage` enum. **This commit's scope.** |
| `crates/eventbus` | Add a `TraceEvent(OrderTraceEvent)` variant to `Event`. Reuses existing broadcast plumbing. **Future commit.** |
| `crates/api/src/monitor.rs` (new) | Trace projector, JSONL writer, REST handlers. **Future commit.** |
| `crates/api/src/{trading,accounts,custody,withdrawals}.rs`, `sequencer/src/lib.rs` (incl. `wal_appended` post-Ok — see §3.7), `matching/src/partitioned.rs`, `ledger/src/lib.rs`, `api/src/bootstrap.rs` | Per-stage emit sites. **Future commits, one per stage class.** **`crates/persistence` is intentionally not in this list** — see §3.7. |
| `crates/api/src/websocket.rs` | New `/ws/order-trace` endpoint. **Future commit.** |

### 2.3 Why an EventBus variant rather than a separate channel
1. Reuses existing infrastructure (already used for `LedgerCommitted`, `FillCreated`, etc.).
2. The WebSocket bridge already maps `eventbus::Event` to WS frames — only one new arm to add.
3. Avoids two parallel "publish" sites in the matching engine.
4. Single backpressure / shutdown story.

### 2.4 Backpressure and overhead
- Trace events are fire-and-forget on a `tokio::sync::broadcast` channel. If consumers fall behind, `Lagged(n)` is acceptable for v1 — exposed as a counter on `/metrics` (`trace_events_dropped_total`).
- One trace event per stage = ≤14 events per order. At 1 K orders/sec sustained, that's ~14 K events/sec — well within the existing eventbus capacity.
- Each emit site is `let _ = bus.publish(Event::Trace(...))` — same pattern as existing `LedgerCommitted` emits. No `await` on a slow consumer.
- The JSONL writer runs on a dedicated tokio task with a bounded `mpsc::channel(8192)`; if the queue fills, the writer logs `WARN` once per minute and drops the oldest. **This is a monitor, not a system of record.**

## 3. Data model

### 3.1 `OrderTraceStage`

15 variants, `serde(rename_all = "snake_case")`. **Wire form is identical to the variant rendered in snake_case** (no dot separator). Tables, JSON examples, query-param values, and WS frame `type` fields all use the snake_case form below; producers and consumers MUST match exactly.

| Variant | Wire form | Emit site (planned, see §3.7) |
|---|---|---|
| `ApiReceived` | `api_received` | api crate, immediately after auth + path match |
| `ApiValidated` | `api_validated` | after pre-flight validation passes |
| `ApiRejected` | `api_rejected` | unified error mapper (catches all reject paths) |
| `SequencerAccepted` | `sequencer_accepted` | after `next_seq` increment |
| `SequencerPersisted` | `sequencer_persisted` | after `wal_store.append(...)` Ok |
| `MatchingResting` | `matching_resting` | order placed in book without (full) match |
| `MatchingPartiallyFilled` | `matching_partially_filled` | per partial-fill chunk |
| `MatchingFilled` | `matching_filled` | `remaining == 0` post-match |
| `MatchingCancelled` | `matching_cancelled` | cancel / replace / mass-cancel / STP |
| `ProjectionUpdated` | `projection_updated` | `OrderStateProjectionStore::apply` |
| `LedgerSettled` | `ledger_settled` | `commit_delta_if_absent` Ok for fill / settlement deltas |
| `WalAppended` | `wal_appended` | **emitted by the higher-level call site after the WAL append returns Ok** — see §3.7. Not emitted from inside `crates/persistence`. |
| `RecoveryReplayed` | `recovery_replayed` | per non-skipped record (debug-only, off by default — §3.6) |
| `RecoverySkippedTerminal` | `recovery_skipped_terminal` | per skipped record (debug-only, off by default — §3.6) |
| `RecoveryCompleted` | `recovery_completed` | once per process startup with aggregate counts (always emitted) |

### 3.2 `OrderTraceEvent`

See `crates/types/src/order_trace.rs` for the canonical Rust definition. Wire format (JSON) for a fully-resolved post-sequencer event. On pre-sequencer stages (`api_received`, `api_validated`, `api_rejected`-from-validation), `order_id` is absent — see §3.3.1.

```json
{
  "schema_version": 1,
  "event_id": "0192b6e7-d34c-7456-9c8e-...",
  "recorded_at": "2026-05-02T07:00:00.123Z",
  "order_id": "ord-abc123",
  "client_order_id": "demo-alice-buy-1",
  "user_id": "alice",
  "session_id": "sess-7f3...",
  "request_id": "req_a1b2...",
  "command_seq": 12345,
  "market_id": "btc-usdt",
  "outcome": 0,
  "stage": "matching_resting",
  "lifecycle": "routed",
  "side": "buy",
  "price": 50000,
  "amount": 10,
  "remaining_amount": 10,
  "filled_amount": 0,
  "fee": null,
  "detail": { "is_market_maker": false },
  "reject_code": null,
  "reject_message": null,
  "elapsed_us_since_request": 1240,
  "trace_id": "..."
}
```

### 3.3 Field discipline

- Mandatory: `schema_version`, `event_id`, `recorded_at`, `stage`. **Plus at least one of `order_id`, `client_order_id`, or `request_id`** — see §3.3.1.
- `order_id` is **optional on the wire and in `OrderTraceEvent`** (`Option<String>`). At `api_received` and `api_validated`, the canonical `order_id` is not yet assigned; the request is still pre-sequencer. The emit site fills `request_id` and (when present) `client_order_id` instead, and the projector binds those to the eventual `order_id` once `sequencer_accepted` carries both.
- All other fields are populated on a best-effort basis at the emit site. `None` is fine when not yet known (e.g., `command_seq` is `None` at `api_received`, populated by `sequencer_accepted`).
- `event_id` uses `uuid::Uuid::new_v4()` (workspace `uuid` features = `["v4", "serde"]`). v7 would be preferable for time-sortability; not adopted in v1 to avoid a Cargo feature change. Time ordering is provided by `recorded_at` instead.
- `recorded_at` uses `chrono::Utc::now()` — ISO-8601 serialisation; consumers (front-end, WS) get readable timestamps directly.
- `detail` is a small free-form JSON object for stage-specific extras (e.g., `matching_partially_filled` puts `fill_index`, `aggressor_side` in there). Bound to ≤256 bytes serialised; longer payloads truncate with a marker `{"_truncated": true}`.
- `reject_code` is the canonical `types::ApiErrorCode` enum (already exists).
- `lifecycle` is the canonical `types::CommandLifecycle` enum (already exists).

### 3.3.1 Correlation before `order_id` exists

For pre-sequencer stages (`api_received`, `api_validated`, `api_rejected`-from-validation), no canonical `order_id` is assigned yet. To keep these events correlatable with the rest of the timeline, the projector uses a **trace key** — the first non-empty value among:

1. `request_id` (server-assigned, always present on api ingress)
2. `client_order_id` (caller-supplied, may be absent)

Algorithm:

- Pre-sequencer events land in a small secondary index `by_trace_key: DashMap<String, Vec<OrderTraceEvent>>`, keyed by `request_id` (or `client_order_id` when no `request_id` exists, e.g. WS submit paths).
- When `sequencer_accepted` emits with both `order_id` and `request_id` populated, the projector flushes the buffered events into the order's `OrderTraceState.timeline`, preserving `recorded_at` order, then removes the trace-key bucket.
- If a request never reaches the sequencer (validation reject), the bucket carries a single `api_rejected` event and is evicted on the same TTL as terminal orders (§4.1). REST/WS lookups by `request_id` still resolve it during that window.
- Buckets have a hard cap (default 4096 entries, oldest evicted on overflow); pre-sequencer instrumentation must never block the request path on monitor state.

This is purely a projector-side concern — the wire schema does not need a `trace_key` field. Producers populate `request_id` (and `client_order_id` when known) on every event from `api_received` onward; the projector handles binding.

### 3.4 What's NOT in the event (v1)

- Full order book context (bids / asks / depth) — too heavy.
- Counterparty user id on every fill — leaks privacy across users. Goes in `detail` only when both sides are the same user (self-trade prevention diagnostic).
- Full WAL bytes / hash — out of scope; reference by `command_seq` + WAL file path instead.

### 3.5 Schema versioning

`schema_version` is part of every event from day one. v1 = 1. Any breaking schema change (renamed field, removed field, narrower type) bumps it. Consumers MUST check `schema_version` before parsing; mismatched version -> log a warning and skip.

### 3.6 Recovery emission policy

A 1 M-command WAL replayed on bootstrap would emit 1 M `recovery_replayed` events if every record produced one. That floods the JSONL writer's bounded `mpsc(8192)` channel almost immediately and starves the broadcast bus during startup, with no operational benefit — recovery is a one-shot bulk operation, not user-observable per-record activity.

**v1 default policy:**

- `recovery_completed` is **always emitted** (exactly one per process startup), with aggregate counts in `detail`:
  ```json
  {
    "stage": "recovery_completed",
    "detail": {
      "records_replayed": 873421,
      "records_skipped_terminal": 126579,
      "duration_ms": 3712,
      "wal_first_seq": 0,
      "wal_last_seq": 999999
    }
  }
  ```
- `recovery_replayed` and `recovery_skipped_terminal` are **debug-only and disabled by default**. They are emitted only when the api crate is started with the env var `MONITOR_TRACE_RECOVERY_DETAIL=1` (or its config equivalent). When disabled, the bootstrap replay loop must not allocate `OrderTraceEvent` per record at all — the check happens before the build.
- The JSONL writer **never** records per-record recovery events even when they are enabled; the broadcast subscriber for the writer filters `recovery_replayed` and `recovery_skipped_terminal` regardless of the flag. Per-record events go to the broadcast channel only (where lagged consumers drop them safely).

This keeps the JSONL trail meaningful for analysts (one row per recovery, not a million) and lets a developer flip the flag for one-off deep diagnosis without a rebuild.

### 3.7 WAL emission boundary

`crates/persistence` is the storage primitive — it has no knowledge of `order_id`, `request_id`, `command_seq`, or `OrderTraceEvent`. Importing the trace types there would push monitoring concerns into the lowest layer of the stack and force every persistence consumer (not just orders) to depend on `OrderTraceEvent`.

**v1 emission boundary:**

- **`wal_appended` is emitted by the higher-level call site** (the sequencer command-persistence path, the matching post-snapshot WAL writer, or whatever caller invoked `JsonlFileWal::append`) **only after the append returns `Ok` for an order-bearing record type.** The call site has the `order_id` / `command_seq` context already; persistence does not.
- `crates/persistence` is **not modified** by step 7 of the implementation ladder (§7). Step 7 instead lives in the call sites that wrap the WAL append for order-bearing records (primarily `crates/sequencer`).
- Non-order WAL records (e.g., snapshot markers, ledger checkpoint commits) do not produce `wal_appended` trace events. They have no `order_id` to attach to and would only add noise.
- This keeps the trace plumbing strictly above the storage layer and eliminates a circular-dependency risk between `persistence` and `types`/`eventbus`.

## 4. Trace projector (planned, future commit)

A thin in-memory aggregator in `crates/api/src/monitor.rs`:

```rust
pub struct OrderTraceState {
    pub order_id: String,
    pub client_order_id: Option<String>,
    pub user_id: Option<String>,
    pub market_id: Option<String>,
    pub current_stage: OrderTraceStage,
    pub command_seq: Option<u64>,
    pub remaining_amount: Option<i64>,
    pub fill_count: u32,
    pub first_seen_at: DateTime<Utc>,
    pub last_updated_at: DateTime<Utc>,
    pub timeline: Vec<OrderTraceEvent>,
    pub terminal: bool,                       // filled / cancelled / rejected
}

pub struct OrderTraceProjector {
    by_order:         DashMap<String, OrderTraceState>,
    by_command_seq:   DashMap<u64, String>,
    by_trace_key:     DashMap<String, Vec<OrderTraceEvent>>,   // pre-sequencer buffer (§3.3.1)
    recent_order_ids: parking_lot::Mutex<VecDeque<String>>,    // tier-1/2 eviction queue
    config:           OrderTraceConfig,
    jsonl_tx:         tokio::sync::mpsc::Sender<OrderTraceEvent>,
}
```

### 4.1 Capacity discipline

The projector keeps order state in memory only. Two thresholds govern eviction:

- **soft cap** = 10,000 — the steady-state target. When `by_order.len()` exceeds the soft cap, the projector evicts on the *next* write.
- **hard cap** = 20,000 — an absolute ceiling. If reached (e.g., during a long-running incident with thousands of resting orders), eviction proceeds even at the cost of dropping non-terminal state.

Eviction tiers, applied in order until `by_order.len() <= soft_cap` (or until the eviction loop reaches the hard-cap floor):

1. **Terminal first.** Evict orders with `terminal == true` (`matching_filled`, `matching_cancelled`, `api_rejected`, plus rejected-at-validation buckets from §3.3.1), oldest `last_updated_at` first.
2. **Stale pre-sequencer trace-key buckets.** Evict `by_trace_key` entries that never bound to an `order_id` and have not been updated for the bucket TTL (default 60 s).
3. **Non-terminal, only if hard cap reached.** Only when steps 1+2 cannot bring the size below the soft cap *and* `by_order.len() >= hard_cap`, evict open orders, oldest `last_updated_at` first. This is a last-resort tier — open orders should remain observable in the monitor while they live in the book.

Counters on `/metrics`:

- `monitor_orders_evicted_terminal_total`
- `monitor_orders_evicted_open_total` — alarmable; non-zero means the soft cap is too low for the workload.
- `monitor_trace_key_buckets_evicted_total`

Restart semantics: in-memory state is per-process. JSONL is the durable trail; on restart the projector starts empty and refills from new traffic + recovery aggregate events.

### 4.2 JSONL writer

- Bounded `mpsc::channel(8192)` from `apply` to a dedicated task.
- **Filters out** `recovery_replayed` and `recovery_skipped_terminal` regardless of the env-var flag in §3.6 — the JSONL trail records aggregate `recovery_completed` only.
- Task writes to `data/trace/order_trace.jsonl`, one record per line, fsync every 64 events (mirrors `JsonlFileWal` group-commit pattern).
- Rotation: cap at 100 MB; rename to `order_trace.<utc>.jsonl` and start fresh. Retention: 14 archives by default.
- Recovery on next start: do **not** reload state from JSONL (defeats "latest snapshot" semantics). The JSONL is for offline analysis only.

## 5. API contract

### 5.1 `GET /monitor/orders`

List orders currently tracked in the projector ring.

Query params:
- `user_id=<s>` — filter
- `market_id=<s>` — filter
- `stage=<snake_case_stage>` — filter
- `terminal=<bool>` — filter
- `limit=<n>` (default 100, max 500)
- `since_ms=<unix_ms>` — orders updated after this timestamp

Response:
```json
{
  "orders": [
    {
      "order_id": "...",
      "client_order_id": "...",
      "user_id": "...",
      "market_id": "btc-usdt",
      "current_stage": "matching_partially_filled",
      "command_seq": 12345,
      "remaining_amount": 7,
      "fill_count": 1,
      "first_seen_at": "2026-05-02T07:00:00.123Z",
      "last_updated_at": "2026-05-02T07:00:00.456Z",
      "terminal": false
    }
  ],
  "total_returned": 100,
  "ring_capacity": 10000,
  "ring_used": 9876
}
```

Auth: requires authenticated user (any role). Non-admin sees only their own orders; admin can omit `user_id` to see all.

### 5.2 `GET /monitor/orders/{order_id}`

Single-order summary (no timeline).

Response: `OrderTraceState` minus `timeline`.

`404` if order unknown OR evicted from ring.

### 5.3 `GET /monitor/orders/{order_id}/timeline`

Full timeline.

Query params:
- `since_event_id=<ulid>` — only events after this (for incremental polling)
- `limit=<n>` (default 200, max 1000)

Response:
```json
{
  "order_id": "...",
  "current_stage": "...",
  "terminal": false,
  "timeline": [ /* OrderTraceEvent, ... */ ],
  "next_since_event_id": "..." // null when caught up
}
```

Auth: user must own the order, or admin.

### 5.4 `GET /ws/order-trace`

Live event stream over WebSocket.

Query params (parsed from URL):
- `user_id=<s>` — only events for this user
- `market_id=<s>` — only events for this market
- `since_event_id=<ulid>` — best-effort backfill since this event id (from ring)

Frame format (one JSON object per WS message):
```json
{ "type": "trace", "event": { /* OrderTraceEvent */ } }
{ "type": "snapshot", "order": { /* OrderTraceState minus timeline */ } }
{ "type": "lagged", "skipped": 47 }
{ "type": "ready" }
```

Backpressure: if a client falls more than 256 events behind, the server sends one `lagged` frame and resumes from current. Clients that care about completeness refetch via REST.

Auth: same as `/ws/user` (`with_principal`). Non-admin users only see their own orders + admin-broadcast events (ledger/recovery summaries).

## 6. Frontend page (suggestion, not a deliverable)

A future React/Vite page at `frontend-modern/src/pages/MonitorPage.tsx`:

```
┌─────────────────────────────────────────────────────────────────────────┐
│  Order Flow Monitor                                                     │
│  filters: [user_id ▼] [market ▼] [stage ▼] [open|terminal|all]          │
├─────────────────────────────────────────────────────────────────────────┤
│  Orders (live, top of list = latest update)                             │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │ order_id   user      market   stage              cmd_seq remaining│   │
│  │ ord-abc..  alice     btc-usdt matching_resting   12345    10      │   │
│  │ ord-def..  bob       btc-usdt matching_filled    12344    0       │   │
│  └──────────────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────────────┤
│  Selected order: ord-abc...                                             │
│  Timeline (oldest → newest)                                             │
│  ┌──────────────────────────────────────────────────────────────────┐   │
│  │ 07:00:00.123  api_received                             elapsed=0  │  │
│  │ 07:00:00.124  api_validated                            elapsed=1  │  │
│  │ 07:00:00.125  sequencer_accepted   cmd_seq=12345       elapsed=2  │  │
│  │ 07:00:00.125  sequencer_persisted                      elapsed=2  │  │
│  │ 07:00:00.126  wal_appended                             elapsed=3  │  │
│  │ 07:00:00.126  matching_resting                         elapsed=3  │  │
│  │ 07:00:00.127  projection_updated                       elapsed=4  │  │
│  └──────────────────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────────────────┤
│  Status indicators                                                      │
│  ws: 🟢 connected   ring: 9876 / 10000   lagged: 0   recovery: ready    │
└─────────────────────────────────────────────────────────────────────────┘
```

Behavior notes for the future implementer:
- Initial load: `GET /monitor/orders?limit=100` for the table.
- Click a row: `GET /monitor/orders/{id}/timeline` for the full history.
- Open `/ws/order-trace?since_event_id=<last_seen>` once on mount; re-render on each message.
- On `lagged` frame: show a yellow banner, refetch the active order's timeline.
- On `recovery_completed` frame: hide the "loading" indicator on first connection.
- Compatible with the existing minimal JSON-panel-driven shell — re-uses `JsonPanel`/`Panel` components.

## 7. Implementation ladder (planned commits)

Each step is independently revertable.

| # | Commit | Files | Risk |
|---|---|---|---|
| **1** | **`feat(types): add OrderTraceEvent + OrderTraceStage` (THIS COMMIT — design doc + types only)** | `docs/MONITOR_DESIGN.md` (new), `crates/types/src/order_trace.rs` (new), `crates/types/src/lib.rs` (re-export) | very low — additive, no consumers yet |
| 2 | `feat(eventbus): add TraceEvent variant` | `crates/eventbus/src/lib.rs` | low |
| 3 | `feat(api): trace projector + JSONL writer + monitor handlers` | `crates/api/src/monitor.rs` (new), `crates/api/src/main.rs` (mount routes; box like other groups per Group 2c) | medium |
| 4 | `feat(api): emit api_received / api_validated / api_rejected` | `crates/api/src/{trading,accounts,custody,withdrawals,…}.rs` | medium — touches many handlers |
| 5 | `feat(sequencer): emit sequencer_accepted / sequencer_persisted` | `crates/sequencer/src/lib.rs` | low |
| 6 | `feat(matching): emit matching_* events` | `crates/matching/src/partitioned.rs` | medium — multiple emit sites; care to not double-emit |
| 7 | `feat(sequencer): emit wal_appended at the call site after Ok` | `crates/sequencer/src/lib.rs` (and any other order-bearing WAL append site) — **`crates/persistence` is not modified**, see §3.7 | low |
| 8 | `feat(api): emit projection_updated + ledger_settled` | `crates/api/src/order_state_projection.rs`, `crates/ledger/src/lib.rs` | low |
| 9 | `feat(api/bootstrap): emit recovery_completed (aggregate); recovery_replayed/skipped_terminal gated on `MONITOR_TRACE_RECOVERY_DETAIL=1`` | `crates/api/src/bootstrap.rs` | low |
| 10 | `feat(api): WS endpoint /ws/order-trace` | `crates/api/src/websocket.rs`, `monitor.rs` | medium |
| 11 | `test(api): integration tests for monitor handlers + WS frames` | new tests | medium |
| 12 | `feat(frontend-modern): MonitorPage` | `frontend-modern/src/pages/MonitorPage.tsx`, route registration | low |

## 8. Open questions for future commits

1. **Per-emit-site cost.** Even `let _ = bus.publish(...)` allocates an `OrderTraceEvent`. At 1 K orders/sec × 14 stages = 14 K allocs/sec. Acceptable for v1; a future optimization could use a slab / `Cow<'static, str>` for fixed strings.
2. **Counterparty visibility.** Fill events naturally carry the counterparty user_id. v1 strips it from the public WS frame; admin sees full. Confirm this is the right policy before step 6.
3. **Authorization scope on `/monitor`.** Non-admin users see ONLY their own orders, plus broadcast recovery markers. Admin sees all. v1 default; revisit at step 3.
4. **Eviction policy.** Resolved in §4.1 — terminal-first eviction with soft (10K) / hard (20K) caps. Open question for step 3: whether the soft cap should be configurable per environment (small dev box vs. staging).
5. **JSONL retention.** Single 100 MB file rotated to dated archives, 14 archives kept. Confirm at step 3.
6. **Recovery emission timing.** Resolved in §3.6 — aggregate `recovery_completed` only by default; per-record `recovery_replayed` / `recovery_skipped_terminal` are gated on `MONITOR_TRACE_RECOVERY_DETAIL=1` and never written to JSONL.

## 9. This commit's scope

- `docs/MONITOR_DESIGN.md` (this file).
- `crates/types/src/order_trace.rs` — `OrderTraceEvent`, `OrderTraceStage`.
- `crates/types/src/lib.rs` — `pub mod order_trace; pub use order_trace::*`.

That's it. **No emission, no eventbus variant, no routes, no WS, no projector.** All those are explicit follow-up commits per §7. This commit is reviewable in isolation: it adds compile-time structure that future commits will fill in, without changing any runtime behaviour.
