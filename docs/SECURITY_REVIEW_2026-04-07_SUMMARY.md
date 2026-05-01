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
