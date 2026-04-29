# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| main    | :white_check_mark: |

## Reporting a Vulnerability

Report security vulnerabilities privately to the repository maintainers via GitHub's
[Security Advisories](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing/privately-reporting-a-security-vulnerability) feature.
Do not open public issues for suspected security problems.

---

## Security Architecture

### Authentication

- **Internal service-to-service auth**: HMAC-SHA256 signed requests with replay guard.
  - Secret loaded from `data/internal_auth.secret` (preferred) or `INTERNAL_AUTH_SHARED_SECRET` env var.
  - Minimum secret length enforced: 32 characters.
  - Timestamp skew tolerance: ±5 seconds.
  - Replay prevention via monotonic request ID tracking (DashMap-backed, cleanup every 500 ops).
- **Role-based access**: Server-side role mapping from `data/role_mapping.json` overrides client-provided roles.
- **Brute-force mitigation**: Per-IP auth failure tracking with configurable ban duration (default: 10 failures → 5-minute ban).

### Authorization

- Principal roles: `Admin`, `Operator`, `User`.
- Role checks enforced at the filter level (`require_admin()`, `require_operator()`, `require_user()`).
- Server-side role resolution prevents privilege escalation via client header manipulation.

### Rate Limiting

- Per-key sliding window rate limiter (`FixedWindowRateLimiter`) with configurable window size and limits.
- Per-IP and per-user rate limiting applied on authenticated endpoints.
- Per-IP WebSocket connection limits (default: 20 connections per IP, 1024 global).
- IP ban check filter (`with_ip_ban_check()`) rejects requests from IPs with excessive auth failures.

### Data Integrity

- Double-entry ledger with append-only WAL (write-ahead log).
- Replay guard on all authenticated write operations.
- Deterministic settlement: all ledger mutations validated against balance invariants.

---

## Unsafe Code Audit

**Status: ZERO `unsafe` blocks in production code.**

As of the latest audit, the `rust-exchange` crate contains no `unsafe` blocks in its production
binary or library code. All `unwrap()`/`expect()` calls are confined to:

| Location | Context | Risk |
|----------|---------|------|
| `crates/*/src/**/*_test.rs` | Unit tests | None — test harness only |
| `crates/matching/examples/*.rs` | Benchmark/example binaries | Low — not deployed |
| `crates/instruments/src/lib.rs:30` | Startup instrument registration | Medium — panics on invalid config (fail-fast, not exploitable remotely) |

### Panic Surface Analysis

No `panic!` macros are reachable from network input in the API binary. The `instruments` crate
panics during startup on invalid specs, which is a deliberate fail-fast behavior — the server
will not boot with malformed configuration rather than silently accepting it.

---

## Dependency Security

### Auditing Process

Run `cargo audit` regularly to check for known vulnerabilities in dependencies:

```bash
cargo install cargo-audit
cargo audit
```

Or use the automated script:

```powershell
.\scripts\security_audit.ps1
```

### Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `warp` | 0.3.7 | HTTP/WebSocket framework |
| `hmac` | 0.12.x | HMAC-SHA256 signing |
| `sha2` | 0.10.x | SHA-256 hashing |
| `dashmap` | 6.1.x | Concurrent hash maps |
| `parking_lot` | 0.12.x | Synchronization primitives |
| `serde` / `serde_json` | 1.0.x | Serialization |
| `tokio` | 1.x | Async runtime |
| `chrono` | 0.4.x | Date/time handling |

### Supply Chain Recommendations

1. Pin all dependency versions in `Cargo.toml` (avoid `*` or unbounded ranges).
2. Run `cargo audit` in CI on every PR.
3. Review `cargo tree` output periodically for unexpected transitive dependencies.
4. Monitor RustSec Advisory Database (https://rustsec.org/).

---

## Configuration Security

### Secrets Management

- **Never commit secrets** — `.secret` files and credential files are in `.gitignore`.
- Production deployments should use a secrets manager (HashiCorp Vault, AWS Secrets Manager, etc.).
- The `data/internal_auth.secret` file should have permissions `0600` (owner read/write only).

### Required Files

| File | Purpose | Sensitivity |
|------|---------|-------------|
| `data/internal_auth.secret` | HMAC shared secret | 🔴 Critical |
| `data/role_mapping.json` | Subject → role mapping | 🟡 Internal |
| `data/admin_principals.json` | Admin principal definitions | 🟡 Internal |

---

## Threat Model

### Mitigated Threats

| Threat | Mitigation |
|--------|-----------|
| Replay attacks | Monotonic request ID + timestamp skew check |
| Credential stuffing | Per-IP auth failure tracking with timed bans |
| WebSocket connection exhaustion | Per-IP connection limits (default 20/IP) |
| Request flooding | Sliding window rate limiting per IP and per user |
| Privilege escalation | Server-side role mapping overrides client claims |
| Secret exposure in logs | Secrets loaded from file, not env vars visible in `/proc` |
| TOCTOU race on order submission | Idempotency cache with atomic deduplication |
| Double-spend / ledger corruption | Double-entry accounting with replay guard |

### Known Limitations

| Limitation | Impact | Planned Fix |
|------------|--------|-------------|
| Rate limiter is in-memory | Survives restart, but not distributed across instances | Redis-backed rate limiter |
| WebSocket IP extraction relies on `warp::addr::remote()` | May show proxy IP behind load balancer | X-Forwarded-For parsing |
| No TLS termination in API binary | Requires reverse proxy for encryption | Document nginx/Caddy config |
