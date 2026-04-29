# Core Chain Next Phase

## Scope

This phase closes the user-facing trading loop and formalizes the internal control path:

1. Limit / Market / Cancel / Query Order
2. available / locked / fee account ledger model
3. trades + order book snapshot REST interfaces
4. API Key HMAC authentication
5. baseline risk controls
6. market state machine Admin API

## What Exists Now

- Limit / Market / Cancel submit paths already run through sequencer -> matching -> settlement.
- Order book snapshot and public trades REST endpoints already exist.
- Ledger already tracks:
  - available cash
  - locked cash
  - available spot position
  - locked spot position
  - derivative position
  - isolated margin
  - fee collector
- Market state transition rules already exist in `types::MarketState::can_transition_to`.
- Matching already enforces core risk guards such as price band, fat-finger, rate limit, and per-user aggregate risk checks.

## New Interfaces Added In This Phase

- `GET /orders/{user_id}/{order_id}`
  - resolves a single order from open-book snapshots first
  - falls back to trade-log reconstruction for historical filled orders
- `GET /ledger/{user_id}`
  - explicit available / locked cash projection
  - position view
  - isolated margin view
  - raw ledger accounts
  - fee account exposure for admins
- `GET /markets/{market_id}/trades`
  - canonical market-scoped trade feed
- `GET /admin/market-state/{market_id}`
  - current state
  - allowed transitions
  - pending governance actions
- `GET/POST /admin/risk/users/{user_id}/limits`
  - per-user notional and open-order controls

## API Key HMAC Model

Public API key authentication is now supported alongside internal auth.

Registry:

- file path default: `data/api_keys.json`
- env override: `API_KEY_REGISTRY_FILE`

Expected JSON shape:

```json
[
  {
    "api_key": "trader-key-1",
    "subject": "user-123",
    "secret": "replace-with-long-random-secret",
    "role": "user",
    "session_id": "desk-a",
    "enabled": true
  }
]
```

Headers:

- `x-api-key`
- `x-api-timestamp`
- `x-api-signature`
- `x-api-body-sha256`
- `x-request-id`

Signature payload:

```text
METHOD
PATH
QUERY
API_KEY
SUBJECT
ROLE
TIMESTAMP
REQUEST_ID
BODY_SHA256
```

Properties:

- write requests reuse the replay guard already used by internal auth
- API keys can be disabled without code changes
- subject and role are resolved server-side from the registry, not client input

### Benchmark / Smoke Examples

Go benchmark trade requests can now use per-subject API keys while keeping internal admin auth for seed and top-up flows:

```bash
go run ./cmd/exchange_http_bench \
  -base-url http://127.0.0.1:3030 \
  -secret dev-secret-change-me-to-32-chars-min! \
  -buyers user-a,user-b \
  -sellers user-c,user-d \
  -api-key-template bench-key-%s \
  -api-secret-template bench-secret-%s
```

PowerShell smoke helpers now support:

```powershell
$Script:AuthMode = "api_key"
$Script:ApiKey = "trader-key-1"
$Script:ApiSecret = "replace-with-long-random-secret"
```

## Core Chain

Canonical flow:

1. Order ingress
   - authenticate principal
   - verify request id and body hash
   - apply read/write rate limits
2. Order log / sequencing
   - allocate command metadata
   - assign sequence
   - persist command intent via sequencer lifecycle
3. Matching
   - validate instrument and market state
   - reserve risk / balances
   - enqueue into partition
   - execute matching
4. Trade log
   - append fills to trade journal
   - record settlement intent and lifecycle
5. Ledger
   - move available/locked balances
   - collect fees into fee collector
   - update positions and isolated margin as required
6. Read models
   - open orders from runtime snapshots
   - historical trades from trade journal
   - balances and positions from ledger projections

## Next Follow-Through

- Completed: added API key examples to benchmark and smoke helpers.
- Completed: single-order lookup now reconstructs cancelled, replaced, and rejected no-fill orders from sequencer command history.
- Completed: added admin treasury read endpoints for fee collector, insurance fund, and per-market treasury views.
- Next: if replacement orders must be recoverable after generated `new_client_order_id` values, persist a richer order-state projection keyed by final order id.
