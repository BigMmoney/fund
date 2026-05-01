# Security Review Summary — 2026-04-07

> **Public-safe summary.** This document deliberately contains NO exploit detail, attack chain reproduction steps, or unmitigated vulnerability internals. The full audit lives in private storage as `DEEP_SECURITY_AUDIT_2026-04-07.md` and must NOT be committed to this repository in its current form. Any sensitive content in this summary must be redacted to abstraction-level descriptions before commit.
>
> **Review date:** 2026-04-07
> **Repo commit at review:** TBD by reviewer
> **Reviewer:** _to be filled in_
> **Sign-off:** _to be filled in_
> **Last update of this summary:** TBD

## 1. Methodology

The audit covered the rust-exchange backend's external attack surface (HTTP/WS), internal authentication path, financial integrity (ledger, risk, sequencer), persistent state (WAL, snapshots, custody store), and privileged endpoints (admin / governance). Findings were classified by category (see §3) and severity (P0–P3). Each finding was assigned one of three end states:

- **Fixed** — code change merged; commit hash recorded in this summary.
- **Accepted with mitigation** — risk understood, compensating control documented, no code change required.
- **Open** — work in flight, planned, or pending decision.

Exploit-level detail (proof-of-concept payloads, reproduction steps, internal request shapes) is out of scope for this public summary and lives only in the private audit.

## 2. Scope of this review

| In scope | Out of scope |
|---|---|
| `rust-exchange/crates/api` request handlers | external dependencies (third-party crates) — covered by `cargo audit` separately |
| `rust-exchange/crates/{ledger,risk,sequencer,persistence,matching}` invariants | frontend (`frontend-modern/`) — separate review |
| HTTP/WS auth path (HMAC, role mapping, rate limits) | k8s deployment hardening — separate review |
| WAL recovery + crash semantics | docker image hardening — separate review |
| Admin / governance endpoints | observability stack itself |
| Custody whitelist + withdrawal flow | external integrations (price feeds, etc.) |

## 3. Finding categories

The review enumerated findings under the following categories. Each row in the §4 status table is tagged with one of these:

| Tag | Description |
|---|---|
| `auth` | HMAC-SHA256 internal auth, role mapping, session validation |
| `rate-limit` | Per-IP, per-user, per-admin rate limiting and back-off |
| `input-val` | Order validation, request schema, parameter bounds |
| `fin-integrity` | Ledger balance invariants, risk reservation, position limits, leverage |
| `data-integrity` | WAL durability, CRC verification, snapshot/WAL skew, replay determinism |
| `custody` | Withdrawal whitelist, address policy, velocity tracker, dual-auth |
| `privilege` | Admin endpoints, governance dual-auth, kill switch, market state |
| `transport` | TLS, ingress, network policy |
| `secrets` | Secret loading, file mode, env-var deprecation, k8s Secret schema |
| `audit-log` | Event-bus logging, admin action audit, risk automation audit |
| `ws` | WebSocket auth, connection limits, fanout backpressure |
| `dos` | Resource exhaustion, queue saturation, large-payload handling |

## 4. Status table

> **Reviewer instructions:** for each finding in the private audit, add one row below. **Do NOT include exploit detail in any column.** Use abstraction-level descriptions only ("rate-limit bypass via X header" → write "rate-limit policy gap"; "ledger debit underflow on Y endpoint" → write "ledger invariant edge case in withdrawal path").

| Finding ID | Category | Severity | Status | Fixed-in commit | Public-safe one-line description |
|---|---|---|---|---|---|
| _DEEP-2026-04-07-001_ | _to fill_ | _to fill_ | _to fill_ | _to fill_ | _to fill_ |
| _DEEP-2026-04-07-002_ | | | | | |
| _DEEP-2026-04-07-003_ | | | | | |
| _DEEP-2026-04-07-004_ | | | | | |
| _DEEP-2026-04-07-005_ | | | | | |
| _DEEP-2026-04-07-006_ | | | | | |
| _DEEP-2026-04-07-007_ | | | | | |
| _DEEP-2026-04-07-008_ | | | | | |
| _DEEP-2026-04-07-009_ | | | | | |
| _DEEP-2026-04-07-010_ | | | | | |
| _DEEP-2026-04-07-011_ | | | | | |
| _DEEP-2026-04-07-012_ | | | | | |
| _DEEP-2026-04-07-013_ | | | | | |
| _DEEP-2026-04-07-014_ | | | | | |
| _DEEP-2026-04-07-015_ | | | | | |
| _DEEP-2026-04-07-016_ | | | | | |
| _DEEP-2026-04-07-017_ | | | | | |
| _DEEP-2026-04-07-018_ | | | | | |
| _DEEP-2026-04-07-019_ | | | | | |
| _DEEP-2026-04-07-020_ | | | | | |
| _DEEP-2026-04-07-021_ | | | | | |
| _DEEP-2026-04-07-022_ | | | | | |

(Add additional rows as needed. The original audit listed 22 findings per its own status table; renumber if the actual count differs.)

## 5. Aggregate status

> Fill in once §4 is complete.

| Severity | Total | Fixed | Accepted | Open |
|---|---:|---:|---:|---:|
| P0 (critical) | _n_ | _n_ | _n_ | _n_ |
| P1 (high) | _n_ | _n_ | _n_ | _n_ |
| P2 (medium) | _n_ | _n_ | _n_ | _n_ |
| P3 (low / informational) | _n_ | _n_ | _n_ | _n_ |
| **Total** | _n_ | _n_ | _n_ | _n_ |

**Closure target:** all P0 + P1 findings fixed or accepted-with-mitigation before RC 0.1 ships. P2/P3 findings tracked as follow-ups.

## 6. Accepted findings — written rationale

> One subsection per finding marked "Accepted with mitigation" in §4. Each subsection states:
>
> 1. The finding (abstract description, not exploit detail).
> 2. Why a code fix was deferred or rejected.
> 3. The compensating control in place (operational guard, monitoring, rate limit, etc.).
> 4. Conditions under which the decision should be revisited.
> 5. Reviewer who approved acceptance.

### 6.1 _DEEP-2026-04-07-XYZ_ — _abstract title_

_to fill_

(Add additional subsections per accepted finding.)

## 7. Cross-reference: fixes already in branch

Code fixes that map directly to security findings live in regular commits on this branch. Each commit message includes the finding ID. Reviewer should verify that each "Fixed" row in §4 references a commit whose diff is bounded by the finding's scope.

| Finding ID | Commit | Files touched |
|---|---|---|
| _to fill_ | _to fill_ | _to fill_ |

### 7.1 Candidate fixes — public-history inventory

> The reviewer fills §4 and the table above by mapping each finding in the private audit to the correct commit. As an aid, the table below inventories commits already on `p0-recovery-20260430` whose surface area touches a security category. **This is a candidate list, not an authoritative finding-to-fix mapping.** The reviewer must independently verify that each row actually closes a specific audit finding (or none) before promoting it into §4.

| Commit | Subject (abbrev.) | Touches | Candidate categories |
|---|---|---|---|
| `8b43964` | feat(api): harden order validation and box warp routes | `crates/api/src/{trading,accounts,admin,custody,governance,liquidation,security,websocket,…}.rs` (≈25 files) | `input-val`, `auth`, `privilege`, `rate-limit`, `custody`, `audit-log` — broad. Reviewer must split into per-finding scope. |
| `1d6ac04` | fix(api): skip terminal commands during WAL replay recovery | `crates/api/src/bootstrap.rs` (+35 / −14) | `data-integrity` — bootstrap-replay determinism / availability under crash recovery. |
| `c7d8ca3` | build(rust): refresh Cargo.lock for API and matching dev deps | `Cargo.lock` | none direct — but reviewer should confirm `cargo audit` advisories on transitive deps in this lockfile. |
| `0bd5f1f` | chore(ci): restore recovery drill script and add backend-resilience workflows | `.github/workflows/{rust-ci,backend-resilience,recovery-drills,ci}.yml`, `scripts/run_backend_recovery_checks.ps1`, `scripts/run_backend_resilience_benchmarks.ps1`, `rust-exchange/scripts/run_recovery_drill.py` | `audit-log` (CI surfaces audit hooks), `data-integrity` (recovery-drill workflow). |
| `acf5c45` | fix(scripts): real WAL replay test, restart-after-errors counting, no orphan api.exe | `rust-exchange/scripts/{test_lib,test_wal_recovery,test_restart_after_errors}.ps1` | `data-integrity` — closes harness gaps that previously masked recovery issues. |
| `5a421cf` | feat(scripts): one-shot P0 wrapper with aggregated JSON report | `rust-exchange/scripts/run_p0_full.ps1` | `audit-log` (machine-readable run reports support security-relevant evidence trails). |
| `5427842` | feat(bench): WAL append, replay scaling, RTO/RPO harnesses | `crates/persistence/benches/wal_append.rs`, `crates/sequencer/benches/replay_scaling.rs`, `rust-exchange/scripts/measure_rto_rpo.ps1`, `Cargo.toml` ×2 | `data-integrity` — RTO/RPO measurement reinforces the recovery contract. |
| `f387496` | fix(scripts): correct soak harness secret and metric aggregation | `rust-exchange/scripts/soak_test_v2.ps1` (+2 / −2) | `secrets` — fixed an undersized dev secret that would have failed the api's 32-char minimum. Not a production-secret bug. |
| `eaf4542` | feat(scripts): add trade-journey demo script | `rust-exchange/scripts/demo_trade_journey.ps1` | none direct — but exercises authenticated end-to-end flow. |
| `b7c7f35` | chore: ignore local artifacts, data backups, and IDE solution files | `.gitignore` | `secrets`, `audit-log` — ensures `data/internal_auth.secret` and `data.bak.*/` cannot be accidentally committed via `git add -A`. |
| `7503cc8` | docs: refresh architecture, security, and deployment notes | `rust-exchange/SECURITY.md`, `rust-exchange/README*.md`, `docs/REAL_ARCHITECTURE_AND_DATA_FLOW_ZH.md`, others | docs only — not a code fix but documents the auth/secret/role schema reviewer is verifying. |
| `bdd28bd` | chore(p0): add WAL backup restore scripts and runbook | `rust-exchange/scripts/wal_backup.ps1`, `run_wal_restore_drill.ps1`, `docs/P0_DEPLOYMENT_READINESS.md` | `data-integrity` — durability / DR. |
| `31ca355` | deploy(k8s): base, observability, benchmarks, docker-desktop overlays | `rust-exchange/deploy/k8s/**`, `Dockerfile`, `docker-compose.yml` | `transport`, `secrets`, `privilege` — container hardening (`read_only`, `cap_drop: ALL`, `no-new-privileges`); secret schema migrated to file-mounted; explicit `CHANGE_ME` placeholders only. |
| `037e9a1` | feat(matching): partitioned engine refinements + tests + bench | `crates/matching/src/{partitioned,high_performance,lib}.rs` + tests/benches | `fin-integrity` — matching engine determinism, queue saturation behaviour. |
| `24d98d8` | feat(core): extend instruments, ledger, persistence, sequencer, types | `crates/{instruments,ledger,persistence,sequencer,types}/**` | `fin-integrity`, `data-integrity` — ledger account invariants, sequencer dedup, persistence CRC. |

### 7.2 Code paths to inspect during review (no commit hash; structural)

These are areas of the api crate / supporting crates that the reviewer should walk through against the audit's findings. They are not commits but standing surfaces of the codebase as of `rc-0.1`:

| Surface | Path | Categories |
|---|---|---|
| HMAC-SHA256 internal auth | `rust-exchange/crates/api/src/security.rs` | `auth`, `secrets` |
| Per-IP / per-user / per-admin rate limiting | `rust-exchange/crates/api/src/security.rs` (`FixedWindowRateLimiter`) | `rate-limit`, `dos` |
| Role mapping + filter-level checks | `rust-exchange/crates/api/src/security.rs` (`require_admin`, `require_operator`, `require_user`) | `auth`, `privilege` |
| Order input validation | `rust-exchange/crates/api/src/trading.rs`, `crates/types/src/lib.rs` | `input-val` |
| Risk engine reservation, leverage, position limits | `rust-exchange/crates/risk/src/lib.rs`, `crates/api/src/{accounts,trading}.rs` | `fin-integrity` |
| Ledger balance invariant | `rust-exchange/crates/ledger/src/lib.rs` (`verify_global_invariant`) | `fin-integrity` |
| WAL CRC + recovery | `rust-exchange/crates/persistence/src/lib.rs`, `crates/api/src/bootstrap.rs` | `data-integrity` |
| Custody whitelist, velocity, breaker, dual-auth | `rust-exchange/crates/api/src/custody.rs`, `withdrawals.rs` | `custody`, `privilege` |
| Governance dual-auth + pending actions | `rust-exchange/crates/api/src/governance.rs` | `privilege` |
| Admin / kill-switch / market-state endpoints | `rust-exchange/crates/api/src/{admin,control,markets}.rs` | `privilege`, `audit-log` |
| WebSocket connection limit + auth | `rust-exchange/crates/api/src/websocket.rs` (`WsHub::with_max_connections`, `with_principal`) | `auth`, `ws`, `dos` |
| Admin action audit log | `rust-exchange/crates/api/src/admin_audit.rs` | `audit-log` |
| Risk automation audit log | `rust-exchange/crates/api/src/admin.rs` (`RiskAutomationAuditStore`) | `audit-log` |
| Beta controls (per-market gating) | `rust-exchange/crates/api/src/beta_controls.rs` | `privilege` |
| Deploy hardening (k8s / Docker) | `rust-exchange/deploy/k8s/**`, `Dockerfile`, `docker-compose.yml` | `transport`, `secrets`, `privilege` |

### 7.3 Open questions for the reviewer

These are explicit unknowns the reviewer should resolve during the session. They are NOT findings — they are scope-clarifications for the §4 fill-in:

1. Is `8b43964` ("harden order validation and box warp routes") closing one finding, several, or none? The commit's diff spans ~25 files and touches multiple categories simultaneously. May warrant splitting into per-finding sub-commits in a future RC if this is a problem for §4 traceability.
2. Does any audit finding cover the dev-secret literal `dev-secret-change-me-to-32-chars-min!` shipped in `rust-exchange/scripts/test_lib.ps1` and the docker-desktop overlay's Secret? If so, the resolution is documenting that this is dev-only and never a production secret (`secrets` category, "Accepted with mitigation").
3. Does the audit cover `cargo audit` advisories on transitive dependencies as of the locked Cargo.lock? If so, the resolution is the `audit` job in `.github/workflows/rust-ci.yml` plus its blocking behaviour.
4. Are there findings on the orphan `stress.rs` module (declared `mod stress;` but never used)? If so, decision is keep-and-wire vs delete.
5. Does the audit address the still-untracked PowerShell scripts under `rust-exchange/scripts/` (e.g., `cancel_storm_test.ps1` has the same short-secret pattern that `f387496` fixed in `soak_test_v2.ps1`)? If the audit pre-dated those, they may need re-review.

## 8. Re-review schedule

| Trigger | Action |
|---|---|
| Any P0 added in subsequent audits | re-open this document, add row, escalate immediately |
| New external attack-surface endpoint added | scope additional review before merge |
| 90-day review cadence | open follow-up review with new dated summary file |
| Production incident with security relevance | append RCA pointer below; consider full re-audit |

## 9. Reviewer sign-off

> Required before this document is considered closed. Sign-off attests that:
>
> 1. Every finding in the private audit has a corresponding row in §4.
> 2. Every "Fixed" row has a verified commit.
> 3. Every "Accepted" row has a §6 rationale and a named approver.
> 4. No exploit detail leaked into this document.

| Role | Name | Date | Signature / commit |
|---|---|---|---|
| Lead reviewer | _to fill_ | _to fill_ | _to fill_ |
| Security peer | _to fill_ | _to fill_ | _to fill_ |
| Engineering owner | _to fill_ | _to fill_ | _to fill_ |

## 10. Notes for future reviewers

- The original `DEEP_SECURITY_AUDIT_2026-04-07.md` is **untracked** and must remain so. It is stored in private security records.
- Before any CI workflow ingests this summary file (e.g., as an artifact attestation), confirm with the on-call security peer that no row has accidentally been filled with exploit-level content.
- This file is committed to the public repo, so all PR reviews of changes to §4 / §6 / §7 must include a security peer.
- The structure of this document (sections, table columns, sign-off rows) is the contract; row content can be amended via normal PR review.
