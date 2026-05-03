# Backoffice RBAC and Employee Permission System — Design

> **Status:** v1 design only. No code. Companion to `docs/MONITOR_DESIGN.md`.
>
> **Scope:** Authentication, authorization, approval workflow, and audit
> trail for *employee* (backoffice) operators. Customer-facing user auth
> (HMAC, API keys) is unchanged and out of scope.
>
> **Branch:** `p0-recovery-20260430`
> **Date:** 2026-05-03

## 1. Goals and threat model

### 1.1 Goals
- Enforce **least privilege** across the operator surface so day-to-day
  staff cannot perform actions outside their job function.
- Make every sensitive action **attributable** to a specific employee,
  with a typed reason and an immutable audit row.
- Provide **defense in depth** against a single compromised operator
  account: high-impact actions require maker-checker (two-person rule).
- Provide a **break-glass path** so an oncall engineer can recover from
  an incident without a 30-minute waiting room — but every break-glass
  use is auto-paged, time-boxed, and reviewed.
- Keep the design **observable**: every privilege check, every
  approval, every break-glass session ends up in the same audit log
  the security team monitors.

### 1.2 Non-goals
- Customer SSO / customer 2FA / customer KYC. Out of scope.
- HSM-backed key custody for hot/cold wallets. Future track.
- Cross-region replication of the audit log. Single-region for v1.
- Fine-grained per-resource ACLs (e.g. "only this user's orders").
  Roles are global-scoped + market-scoped at most.

### 1.3 Threat model

| Adversary | What they have | What we defend |
|---|---|---|
| Compromised L1 support | Stolen Slack OAuth + cookie | They can read but cannot move money, cancel orders en masse, or change anyone's permissions |
| Compromised trading_ops | Stolen MFA-aware credential | They can halt a market and trigger maker-checker requests but cannot unilaterally adjust balances or approve their own withdrawal review |
| Compromised security_admin | Full role-management privilege | Maker-checker still required for role grants above their own level; audit log writes are append-only and shipped to a separate sink |
| Insider with super_admin_break_glass | Time-boxed full access | All actions auto-page security and oncall; break-glass session is auto-revoked at TTL; post-session forensic review is mandatory |
| External attacker via API | No employee credential | Employee endpoints require both internal HMAC AND a session cookie tied to MFA at login; API-key auth path cannot reach `/admin/employees/*` |

### 1.4 What is OUT of the threat model
- A backend operator with full database write access.
- A compromised CI runner that can deploy unsigned binaries.
- Supply-chain compromise of dependencies.
These are real risks but addressed by separate tracks (CI signing,
dependency scanning, infra access controls).

## 2. Role vs level vs scope model

The authorization decision is a triple `(role, level, scope)` evaluated
against the requested `(action, resource)`:

- **Role** — what job function the operator performs (e.g. `support_l1`).
- **Level** — privilege height within the role family. Three levels:
  - `read` — observe only.
  - `act` — perform documented day-to-day actions.
  - `escalate` — initiate maker-checker requests for higher-impact actions.
- **Scope** — which slice of the system the role applies to:
  - `*` — global.
  - `market:btc-usdt` — single market.
  - `desk:asia-night` — operational shift / desk.
  - `customer:user-12345` — single customer (rarely used; mostly for
    support tickets).

A single employee may hold multiple `(role, level, scope)` triples.
The effective permission for an action is the OR of all triples.

Each action has a static minimum requirement table (Section 4). The
authorization service answers `is_allowed(employee, action, resource)`
by walking the employee's grants and matching against the requirement.
**No code path may grant access by inspecting only the role name** —
the level + scope must be checked together.

### 2.1 Role lifecycle states
- `pending_invite` — invited, not yet logged in. Cannot do anything.
- `active` — normal.
- `suspended` — login allowed only to clear the alert; no actions
  succeed.
- `revoked` — login refused. Audit history retained.

### 2.2 Grant lifecycle states
- `provisional` — granted by a single approver at-or-above the grant's
  required level; awaiting second approver if maker-checker required.
- `active` — fully approved, valid until `expires_at`.
- `expired` — past TTL; any action attempt produces an audit row and a
  403.
- `revoked` — explicitly removed. Cannot be reused; a new grant must
  be issued.

## 3. Employee roles

Ten roles, organized into four families:

| Family | Roles |
|---|---|
| Support | `support_l1`, `support_l2` |
| Read-only review | `auditor_readonly`, `compliance_ops` |
| Operations | `trading_ops`, `risk_ops`, `finance_ops`, `sre_ops` |
| Privilege management | `security_admin`, `super_admin_break_glass` |

### 3.1 `auditor_readonly`
- **Purpose:** External / internal audit. Read-only across the
  platform with no ability to write anything.
- **MFA:** Required at login.
- **Scope:** Always `*`.
- **Default TTL:** 90 days, renewable by `security_admin`.

### 3.2 `support_l1`
- **Purpose:** First-line customer support. Look up customer state to
  answer questions. Cannot move money or change order state.
- **Scope:** `*` for read; `customer:<id>` for any action (must select
  a target customer per session).
- **MFA:** Required.

### 3.3 `support_l2`
- **Purpose:** Escalated support. Can initiate maker-checker requests
  for sensitive customer-facing actions (e.g. unfreeze an account).
  Cannot approve their own requests.
- **Scope:** `*` for read; `customer:<id>` for write.

### 3.4 `trading_ops`
- **Purpose:** Day-to-day market operations. Halt / resume markets,
  initiate mass-cancel against the live book under runbook conditions,
  query the order flow monitor.
- **Scope:** `market:<id>` or `*`.
- **Maker-checker required for:** mass-cancel of >100 orders, market
  state transitions to `Halted`.

### 3.5 `risk_ops`
- **Purpose:** Risk parameter management — position limits, leverage
  caps, fee tiers, kill-switch.
- **Scope:** `*`.
- **Maker-checker required for:** kill-switch toggle, lowering
  any risk limit (raising it is a maker-checker too — design §5),
  fee tier table edits.

### 3.6 `finance_ops`
- **Purpose:** Withdrawals review/approval, treasury reconciliation,
  manual ledger adjustments.
- **Scope:** `*`.
- **Maker-checker required for:** all withdrawal approvals, all
  manual ledger adjustments. Single-actor permitted only for
  withdrawal *rejections* (rejecting is reversible by re-submission).

### 3.7 `compliance_ops`
- **Purpose:** Regulatory / KYC / sanctions review. Read across the
  platform; can freeze (but not unfreeze) a customer account
  unilaterally on a documented sanctions hit. Unfreeze always
  maker-checker.
- **Scope:** `*`.

### 3.8 `sre_ops`
- **Purpose:** Infra ops: drain a node, trigger checkpoint, reset
  circuit breakers, pull a snapshot. No business-state writes.
- **Scope:** `*`.

### 3.9 `security_admin`
- **Purpose:** Manage employee accounts, roles, and grants. Can
  create grants up to (but not including) `super_admin_break_glass`.
  All grant changes are maker-checker against another `security_admin`.
- **Scope:** `*`.
- **Self-grant prohibited** — security_admin cannot grant or modify
  their own permissions.

### 3.10 `super_admin_break_glass`
- **Purpose:** Emergency-only override for incident recovery.
- **TTL:** 4 hours, non-renewable in the same session.
- **Activation:** Requires two-person approval from `security_admin`
  + a written incident reference (PagerDuty incident id, runbook id).
- **Side effects of activation:**
  - Auto-page security on-call and engineering on-call.
  - Auto-create a Sentinel incident with status `active`.
  - All actions during the session log at `WARN`+ with the
    `break_glass_session_id`.
  - Session is auto-revoked at TTL or when the linked PagerDuty
    incident closes.
- **Post-session:** Mandatory forensic review by a different
  `security_admin` within 24h; review notes attached to the audit
  trail.

## 4. Permission matrix

Each cell lists the minimum `(level, maker-checker?)` requirement.
Empty cell = denied.

| Action | auditor_ro | support_l1 | support_l2 | trading_ops | risk_ops | finance_ops | compliance_ops | sre_ops | security_admin | super_admin_break_glass |
|---|---|---|---|---|---|---|---|---|---|---|
| **orders.read** | read | read | read | read | read | read | read | read | read | act |
| **orders.timeline** | read | read | read | read | read | read | read | read | read | act |
| **orders.cancel (single)** | | | act | act | | | | | | act |
| **orders.mass_cancel (≤100)** | | | | act | | | | | | act |
| **orders.mass_cancel (>100)** | | | | act+MC | | | | | | act |
| **monitor.access** | read | read | read | read | read | read | read | read | read | act |
| **users.read** | read | read | read | | | read | read | | read | act |
| **users.freeze** | | | escalate+MC | | | | act | | | act |
| **users.unfreeze** | | | escalate+MC | | | | escalate+MC | | | act+MC |
| **users.restrict** | | | act | | | | act | | | act |
| **balances.read** | read | read | read | | | read | read | | | act |
| **balances.adjust** | | | | | | act+MC | | | | act+MC |
| **withdrawals.review** | read | read | read | | | act | read | | | act |
| **withdrawals.approve** | | | | | | act+MC | | | | act+MC |
| **withdrawals.reject** | | | | | | act | act | | | act |
| **risk.limits.read** | read | | read | read | read | read | read | read | read | act |
| **risk.limits.update (raise)** | | | | | act+MC | | | | | act+MC |
| **risk.limits.update (lower)** | | | | | act+MC | | | | | act+MC |
| **risk.kill_switch.toggle** | | | | | act+MC | | | | | act |
| **market.halt** | | | | act+MC | | | | | | act |
| **market.resume** | | | | act+MC | | | | | | act+MC |
| **audit.log.read** | read | | read | read | read | read | read | read | read | act |
| **audit.log.export** | act | | | | | | act+MC | | act+MC | act+MC |
| **employees.list** | read | | | | | | | | read | act |
| **employees.create** | | | | | | | | | act+MC | act+MC |
| **employees.grant_role** | | | | | | | | | act+MC | act+MC |
| **employees.revoke_role** | | | | | | | | | act | act |
| **employees.suspend** | | | | | | | | | act | act |
| **employees.delete** | | | | | | | | | act+MC | act+MC |

Legend:
- `read` — read-only access permitted.
- `act` — single-actor write action permitted.
- `act+MC` — write action permitted but requires maker-checker
  approval before it commits.
- `escalate+MC` — role can *initiate* the maker-checker request but
  cannot approve it; another role with the appropriate `act+MC`
  permission must approve.
- empty — denied (returns 403 + audit row).

### 4.1 Notes on specific cells
- `orders.cancel (single)` is permitted to `support_l2` so a customer
  with a stuck order can be helped synchronously; the action is
  audit-logged with the customer scope.
- `users.unfreeze` is **never** single-actor (even for break-glass)
  because freezing on a sanctions hit is reversible by an attacker
  with one compromised account otherwise.
- `risk.limits.update` is maker-checker for *both* directions (raise
  and lower). Raising under attacker control enables fraud; lowering
  enables denial-of-service against legitimate customers. Symmetry
  removes the "attacker raises limit, withdraws, then lowers it back"
  pattern.
- `audit.log.export` is maker-checker for compliance/security_admin
  because a full export is a data-exfil signal worth two-person
  oversight.

## 5. Approval model

Three classes of action:

### 5.1 Single-actor actions
- Visible immediately on submit; commit is synchronous.
- Audit row written before the action's first side effect.
- Reason field required (free text, 16-512 chars).
- Examples: `orders.read`, `users.read`, `withdrawals.reject`,
  `employees.suspend`, `market.resume` (under `super_admin_break_glass`).

### 5.2 Maker-checker actions
- Submitter creates an `ApprovalRequest` row in `pending` state with
  the typed action, target resource, reason, and a server-computed
  `expires_at` (default 24h, configurable per action class).
- Eligible approvers see the request in `GET /admin/approval-requests`.
- A second employee with the matching `act+MC` permission posts to
  `/admin/approval-requests/{id}/approve` with their own reason. **The
  approver must be a different person than the submitter** — server-
  side check on `submitter_employee_id != approver_employee_id`.
- On approval, the action commits and an audit row is written tying
  the request id, both employee ids, and both reasons.
- Approver can also `reject` the request, which closes it without
  committing.
- Pending requests auto-expire at `expires_at`; expired requests
  require a fresh submission.

### 5.3 Break-glass actions
- Activation is itself a maker-checker action (see §3.10) requiring
  two `security_admin` approvers.
- During the active session, the operator's effective grants include
  `super_admin_break_glass`, so they can perform any action the table
  in §4 permits for that role.
- Session metadata (`break_glass_session_id`, incident reference,
  start time, expires_at, approvers) is stamped on every audit row
  produced during the session.
- Session auto-revokes at TTL OR when the linked incident closes,
  whichever is first.
- Post-session forensic review (by a *different* security_admin) is
  required within 24h; non-completion creates a ticket to the security
  team's backlog.

## 6. Admin action audit schema

One append-only row per admin action attempt — successes and failures.
Rows are written on a dedicated WAL (`data/admin_audit.jsonl`),
mirrored to the existing `AdminAuditStore`, and shipped to a
write-only sink outside the api process for tamper resistance.

### 6.1 Row shape

```json
{
  "schema_version": 1,
  "event_id": "audit-0192-...",
  "recorded_at": "2026-05-03T07:00:00.123Z",

  "employee_id": "alice@operator.example",
  "session_id": "sess-abc123",
  "mfa_method": "totp",
  "remote_ip": "10.0.1.42",
  "user_agent": "BackofficeWeb/2.0",

  "action": "withdrawals.approve",
  "resource": {
    "kind": "withdrawal",
    "id": "wd-7891"
  },
  "scope": "*",
  "reason": "customer KYC re-verified per ticket SUP-12345",

  "requested_at": "2026-05-03T07:00:00.100Z",
  "decision": "committed",
  "decision_reason": null,

  "approval_request_id": "appr-99",
  "approval": {
    "submitter_employee_id": "alice@operator.example",
    "submitter_reason": "...",
    "approver_employee_id": "bob@operator.example",
    "approver_reason": "verified per SUP-12345",
    "approved_at": "2026-05-03T07:00:00.110Z"
  },

  "break_glass_session_id": null,
  "incident_reference": null,

  "outcome": "success",
  "outcome_detail": null
}
```

### 6.2 Field semantics
- `decision` — one of `committed` | `denied_authz` | `denied_mfa` |
  `denied_self_approval` | `denied_expired_grant` | `pending_approval` |
  `expired_unapproved` | `rejected_by_approver`.
- `outcome` — `success` | `failure`. For `failure`, `outcome_detail`
  carries an opaque error string suitable for incident triage but not
  for end-user display.
- `approval` — present only for maker-checker actions that committed
  or were rejected.
- `break_glass_session_id` and `incident_reference` — present only
  for actions performed under an active break-glass session.

### 6.3 Indexes / queries (logical, storage-agnostic)
- by `employee_id` + `recorded_at` — "what did Alice do today?"
- by `action` + `recorded_at` — "show all withdrawal approvals this
  week."
- by `resource.kind` + `resource.id` — "history of changes to
  withdrawal wd-7891."
- by `break_glass_session_id` — "all actions performed under
  emergency session X."

### 6.4 Retention
- Online retention: 1 year.
- Cold retention: 7 years (regulatory floor for financial records).
- Deletion path: none. Records can be marked `redacted` (PII fields
  blanked, all other fields preserved) by a maker-checker action of
  `audit.redact`.

## 7. API contract

All endpoints under `/admin/employees/*`, `/admin/approval-requests/*`,
and `/admin/audit/*` require:
- Internal HMAC headers (existing scheme), AND
- A session cookie established via MFA at login, AND
- The operator's session is not in `suspended` or `revoked` state.

API-key auth (the customer-facing scheme) is **rejected** at the
filter layer for these paths.

### 7.1 `GET /admin/me/permissions`
- Returns the calling operator's role grants and an effective
  `(action -> verdict)` map for client UIs to gate buttons.
- No body.
- Response:
```json
{
  "employee_id": "alice@operator.example",
  "session_id": "sess-abc123",
  "mfa_method": "totp",
  "grants": [
    {
      "role": "trading_ops",
      "level": "act",
      "scope": "*",
      "expires_at": "2026-08-01T00:00:00Z"
    }
  ],
  "active_break_glass_session": null,
  "effective": {
    "orders.read": "allow",
    "orders.cancel.single": "allow",
    "orders.mass_cancel.gt100": "requires_approval",
    "users.freeze": "deny",
    "...": "..."
  }
}
```

### 7.2 `GET /admin/employees`
- List all employees and their grant summaries. Required permission:
  `employees.list` (auditor_readonly + security_admin + break-glass).
- Query params: `?role=trading_ops&status=active&limit=100&offset=0`.
- Response: array of employee records with grants embedded.

### 7.3 `POST /admin/employees/{id}/roles`
- Grant a `(role, level, scope)` triple to an employee. If the role
  requires maker-checker (most do — see §4), this creates an
  `ApprovalRequest` and returns `pending_approval`.
- Body:
```json
{
  "role": "risk_ops",
  "level": "act",
  "scope": "*",
  "expires_at": "2026-08-01T00:00:00Z",
  "reason": "Q3 oncall rotation per Linear OPS-441"
}
```
- Response (immediate-grant case):
```json
{ "status": "granted", "grant_id": "g-123", "audit_id": "audit-..." }
```
- Response (maker-checker case):
```json
{
  "status": "pending_approval",
  "approval_request_id": "appr-99",
  "approvers_required": 1,
  "approvers_eligible": ["security_admin"],
  "expires_at": "2026-05-04T07:00:00Z"
}
```

### 7.4 `POST /admin/approval-requests`
- Generic submission endpoint for any maker-checker action. The action
  payload is opaque to the approval layer; the action handler validates
  it on commit.
- Body:
```json
{
  "action": "withdrawals.approve",
  "resource": { "kind": "withdrawal", "id": "wd-7891" },
  "scope": "*",
  "reason": "customer KYC re-verified per ticket SUP-12345",
  "action_payload": { "withdrawal_id": "wd-7891" },
  "expires_in_seconds": 86400
}
```
- Response: 201 with the new `ApprovalRequest`.

### 7.5 `POST /admin/approval-requests/{id}/approve`
- Body:
```json
{ "reason": "verified per SUP-12345 + chat with finance" }
```
- Server checks:
  1. Caller has the `act+MC` permission for the target action.
  2. Caller is not the submitter (`denied_self_approval` if so).
  3. Request is still `pending` (not expired, not already approved).
- On success, the action commits synchronously and the audit row is
  written. Response: `{ "status": "committed", "audit_id": "audit-..." }`.

### 7.6 `POST /admin/approval-requests/{id}/reject`
- Body: `{ "reason": "..." }`. Closes the request without committing
  the action. Audit row written with `decision: "rejected_by_approver"`.

### 7.7 `GET /admin/audit/actions`
- Query params: `?employee_id=...&action=...&since_ms=...&limit=...`.
- Required permission: `audit.log.read` (most operator roles + auditor
  + compliance + security_admin).
- Response: array of audit rows per §6.1, sorted by `recorded_at` desc.
- Export endpoint `GET /admin/audit/actions/export?format=csv` is a
  separate maker-checker-gated path (see §4 `audit.log.export`).

## 8. MVP scope

Six roles for v1; the others land in v1.1+ once usage patterns
inform their action sets.

### 8.1 v1 roles
- `auditor_readonly`
- `support_l1`
- `trading_ops`
- `risk_ops`
- `finance_ops`
- `super_admin_break_glass`

### 8.2 v1 actions
The matrix in §4 is the long-term target. v1 implements:
- All `*.read` actions for the v1 roles.
- `monitor.access` for all v1 roles.
- `orders.cancel` and `orders.mass_cancel (≤100)` for `trading_ops`.
- `market.halt` and `market.resume` for `trading_ops` (both maker-checker).
- `risk.limits.update` and `risk.kill_switch.toggle` for `risk_ops`
  (both maker-checker).
- `withdrawals.review`, `withdrawals.approve` (maker-checker), and
  `withdrawals.reject` (single-actor) for `finance_ops`.
- `employees.list`, `employees.suspend`, `employees.revoke_role` for a
  v1 stand-in `security_admin` role (the role itself ships in v1.1
  but the actions are wired so super_admin_break_glass can use them).
- `audit.log.read` for all v1 roles.

### 8.3 v1 deferred
- `support_l2`, `compliance_ops`, `sre_ops`, `security_admin` as full
  roles. Their action sets are hand-managed via break-glass in v1.
- Scope filtering finer than `*`. v1 grants are global-scoped only.
- `users.freeze` / `users.unfreeze` flow. v1 supports
  `withdrawals.reject` as the only customer-restricting lever, which
  unblocks Q3 launch without requiring the full freeze workflow.
- `audit.log.export`. Manual SQL export with security_admin sign-off
  in v1.

### 8.4 v1 deliverables
- `crates/api/src/admin_employees.rs` — employee + grant store and
  REST handlers.
- `crates/api/src/admin_approvals.rs` — approval request flow.
- Extension of `crates/api/src/admin_audit.rs` with the §6.1 row
  shape (the existing store is currently coarser).
- `frontend-modern/src/pages/AdminPage.tsx` — operator workspace
  (employee list, my permissions, pending approvals, audit log).
- Smoke test mirroring `scripts/monitor_smoke_test.ps1`.

## 9. Security rules

These rules are invariants enforced at the authorization layer; any
code path that reaches a protected handler MUST go through this
layer.

1. **Least privilege.** Default for every action is `deny`. Grants
   are explicit, time-boxed, and minimum-necessary. Reviews quarterly.
2. **MFA required.** Every employee session is authenticated against
   a second factor (TOTP at minimum; WebAuthn preferred) at login and
   re-prompted for every `act+MC` submission and every break-glass
   activation.
3. **No self-approval.** The approval handler rejects with
   `denied_self_approval` when `submitter_employee_id ==
   approver_employee_id`. This applies to break-glass activation too:
   a security_admin cannot self-grant break-glass.
4. **All sensitive actions require a typed reason.** The reason field
   is required (16-512 chars) for every maker-checker submission and
   approval, and for every break-glass action. Empty / whitespace-only
   reasons are rejected at the validator. The reason is preserved in
   the audit row indefinitely (subject to redaction per §6.4).
5. **Temporary access expires.** Every grant carries `expires_at`
   (max 1 year, default 90 days). The authorization service checks
   expiry on every request; expired grants produce
   `denied_expired_grant` and an audit row. There is no implicit
   renewal — renewal is a fresh `POST /admin/employees/{id}/roles`.
6. **All admin actions audited.** Every authorization decision —
   success or failure — produces exactly one audit row. The audit
   write happens before the action's first observable side effect
   (the row is committed first; the business action then runs). If
   the audit write fails, the action is denied with
   `denied_audit_write_failure` and a `CRITICAL` log line.
7. **Append-only audit storage.** No code path may delete or
   in-place-modify an audit row. Redaction (§6.4) creates a new row
   marking the original.
8. **Separation of duty for security_admin.** A security_admin cannot
   grant or modify their own permissions, cannot approve a maker-
   checker request they submitted, and cannot perform the post-
   break-glass forensic review for a session they approved.
9. **Break-glass auto-page.** Activation MUST trigger the security
   on-call page and create a Sentinel incident with status `active`.
   Failure to page is itself a `CRITICAL` event that pages a
   secondary on-call.
10. **Session affinity to MFA.** Session cookies are HttpOnly, Secure,
    SameSite=Strict, tied to the MFA evidence used at login, and
    bound to the source IP /24 (relaxed for known VPN egress
    ranges configured per-deployment).

## 10. Open questions for v1.1

- Should `support_l1` action scope default to `customer:<id>` selected
  per session, or be implicit-from-ticket-context? Customer-id-per-
  session is simpler but adds a click; ticket-context requires a
  trusted ticketing integration.
- Should break-glass activation require a written runbook id (we say
  "PagerDuty incident id OR runbook id" — does that need to be
  one-of, or both)? Survey on-call rotation in v1.1.
- Maker-checker quorum: is 1 approver always enough, or should
  high-impact actions (kill_switch, large balance adjustment, role
  grant for security_admin family) require 2 approvers? Default v1: 1
  approver; revisit after first quarterly security review.
- Audit log shipping: which write-only sink? Candidates: S3 with
  Object Lock, splunk, file-only with operator-managed shipping.
  Decision deferred to v1.1.
