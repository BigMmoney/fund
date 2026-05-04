# Identity Provider Integration

> How operators authenticate to the backoffice (`/admin/*`), how the
> `BACKOFFICE_BOOTSTRAP_ADMIN` first-boot grant works, and how the
> production OIDC/SAML cutover lands.
> Gates **P0-SEC-5** (bootstrap admin documented and rotated) +
> **P3-SEC-1** (frontend bearer-token migration).

---

## 1. Today (v1)

The api accepts `with_principal()` HMAC-signed requests with three
required headers:

```
x-internal-auth-subject:    <employee_id>
x-internal-auth-role:       admin
x-internal-auth-signature:  <HMAC-SHA256 over canonical payload>
```

The HMAC shared secret (`INTERNAL_AUTH_SHARED_SECRET`) is loaded via
`SecretLoader` (gate P0-SEC-1; default `EnvSecretLoader`, target
`KmsSecretLoader`). The frontend signs in-browser; admin tooling
signs with the same secret.

This works for the closed-team trial but is unfit for production:
- No identity (the secret IS the identity)
- No MFA enforcement
- No session revocation without rotating the secret
- No audit trail tied to a real human

---

## 2. Bootstrap admin contract

`BACKOFFICE_BOOTSTRAP_ADMIN` env solves the chicken-and-egg problem:
the FIRST admin must exist before any RBAC handler will accept a
grant write.

### 2.1 Behaviour

On every boot, before serving traffic:

1. Read `BACKOFFICE_BOOTSTRAP_ADMIN` env. Empty → no bootstrap.
2. If the named subject already exists in `data/admin/employees.jsonl`,
   skip.
3. Otherwise create an `Employee { employee_id, status: Active,
   created_at: now }` and grant `super_admin_break_glass` at scope
   `global` with level `Act`.
4. Write a `bootstrap_admin_seed` audit row to
   `data/admin/rbac_audit.jsonl`.

### 2.2 Required value format

For production: must match the IdP's stable subject claim. Pre-IdP
cutover: any opaque identifier works as long as it's unique to a
real person (e.g. `alice.smith@company.com`). NEVER reuse:
- `admin` (too generic, masks identity in the audit log)
- `dev-secret-*` patterns from the test harness
- A shared mailbox

### 2.3 Rotation procedure

The bootstrap admin is meant to be self-deprecating: the first thing
that admin does is grant `super_admin_break_glass` to two real
operators (with maker-checker), then DEMOTE themselves by setting
`status: Suspended` on their own employee row.

```
1. operator-a, operator-b sign in (HMAC for now; OIDC after §3 lands)
2. bootstrap-admin POST /admin/employees     {"employee_id":"operator-a"}
3. bootstrap-admin POST /admin/role-grants   {"employee_id":"operator-a", role:"super_admin_break_glass", scope:"global", level:"Act"}
4. (repeat for operator-b)
5. bootstrap-admin POST /admin/approval-requests
     action=DemoteSelf   resource=Employee:bootstrap-admin
6. operator-a (NOT operator-b OR bootstrap-admin) POST /admin/approval-requests/{id}/approve
7. After commit: bootstrap-admin's row flips to Suspended; further
   bootstrap-admin requests get 403 from AdminAuthzService.
```

After step 7, if `BACKOFFICE_BOOTSTRAP_ADMIN` env is left unchanged,
the next reboot's "already exists" check in §2.1 step 2 keeps the
demotion: bootstrap is NOT re-promoted. The env can be unset after
the first successful demotion.

### 2.4 Acceptance for P0-SEC-5

| Check | Verification |
|---|---|
| `BACKOFFICE_BOOTSTRAP_ADMIN` set in production deploy manifest | `kubectl get cronjob exchange -o yaml \| grep BACKOFFICE_BOOTSTRAP_ADMIN` |
| Value matches a real person's IdP subject | manual review |
| Demotion procedure executed in staging | drill report linked from PRODUCTION_LAUNCH_CHECKLIST.md |
| Audit row `bootstrap_admin_seed` present after first boot | `grep bootstrap_admin_seed data/admin/rbac_audit.jsonl` |

---

## 3. Production OIDC cutover (target)

### 3.1 Topology

```
   browser                       api
      │                           │
      │   GET /admin/login?...    │
      ├─────────────────────────► │
      │                           │ 302 → IdP /authorize
      │ ◄─────────────────────────┤
      ▼                           │
   IdP /authorize ──► /token      │
      │                           │
      │   redirect with code       │
      ├─────────────────────────► │ POST /admin/oidc/callback
      │                           │
      │                           │ exchange code → id_token
      │                           │ verify signature against IdP JWKS
      │                           │ extract sub claim
      │                           │ mint short-lived session JWT
      │                           │ (HS256 signed with KMS-loaded secret)
      │                           │
      │                           │ 302 + Set-Cookie: session=<JWT>
      │ ◄─────────────────────────┤
      ▼
   browser uses HttpOnly cookie on every /admin/* request
                                  │
                                  │ session middleware:
                                  │   verify cookie JWT
                                  │   load principal from sub claim
                                  │   continue to existing AdminAuthzService
```

### 3.2 Required IdP features

| Capability | Why |
|---|---|
| OIDC (OpenID Connect 1.0) | Standard auth code flow with PKCE |
| MFA enforcement on the admin app | Single password is not sufficient for break-glass |
| JWKS endpoint | Verify id_token without per-request IdP roundtrip |
| Group / role mapping | Optional: pre-seed `RoleGrant` from IdP groups |
| SCIM 2.0 (preferred) | Employee deprovisioning propagates to AdminEmployeeStore automatically |

Compatible providers: Okta, Auth0, Microsoft Entra ID (Azure AD),
Google Workspace, Keycloak.

### 3.3 New session endpoint contract

| Method | Path | Body / Query | Response |
|---|---|---|---|
| GET | `/admin/login` | `?return=/admin/dashboard` | 302 → IdP `/authorize?...&state={csrf}` |
| GET | `/admin/oidc/callback` | `?code=...&state=...` | 302 → return URL + `Set-Cookie: session=<JWT>` |
| POST | `/admin/logout` | — | clears the session cookie + 200 |
| GET | `/admin/me/session` | — | `{ employee_id, expires_at, mfa_verified_at }` |

The session JWT carries:
```json
{
  "iss": "exchange-api",
  "sub": "<employee_id>",
  "iat": ...,
  "exp": ...,                 // 1 h max
  "mfa_at": ...,
  "csrf": "<32-byte hex>"
}
```

`csrf` is also placed in a `Set-Cookie: __Host-csrf=...; SameSite=Strict`;
all mutating requests must echo it in `x-csrf` header. Stops a
session-fixation / cross-site write attack from a compromised
non-admin tab.

### 3.4 What gets DELETED at cutover

- `INTERNAL_AUTH_SHARED_SECRET` for the **frontend** path. Customer
  SDK clients can keep API-key auth (different code path).
- `with_principal()` HMAC verification for `/admin/*` (kept for
  back-end-to-back-end internal automation behind a feature flag).
- The frontend's HMAC-in-browser code path.

### 3.5 What stays unchanged

- `AuthenticatedPrincipal` struct — JWT verifier produces the same
  shape, just from a different source.
- `AdminAuthzService` — unchanged.
- `data/admin/employees.jsonl` and `role_grants.jsonl` — unchanged
  (with optional SCIM sync layered on top).
- `denied_self_approval` / `committed_approval` audit rows —
  unchanged. Maker-checker doesn't care how the principal was
  authenticated, only that the two principals differ.

### 3.6 Migration plan (gate P3-SEC-1)

1. **Land the verifier** — new `oidc_session.rs` module that mints
   + verifies the session JWT. Behind `--features oidc`. No change
   to existing handlers.
2. **Stand up a test tenant** at the chosen IdP. `redirect_uri`
   pointed at staging `/admin/oidc/callback`.
3. **Enable for staging admin only** — frontend toggles between
   HMAC and OIDC based on env. Prove the session lifecycle drives
   the audit log correctly.
4. **Migrate operator accounts** — one-by-one create the IdP user
   AND backfill `Employee` rows whose `employee_id` matches the
   IdP `sub`.
5. **Production cutover** — flip the frontend env; HMAC stays as a
   fallback for the first 30 days.
6. **Remove the HMAC fallback** — close-out PR after the first 30
   days. `INTERNAL_AUTH_SHARED_SECRET` shrinks to back-end-only use.

---

## 4. Acceptance summary

| Gate | Today | Target | Owner |
|---|---|---|---|
| P0-SEC-5 (bootstrap admin) | Documented in §2 | Set + audited; demotion executed in staging | security |
| P3-SEC-1 (OIDC cutover) | Designed in §3 | Implemented + frontend migrated | security + platform |

Until P3-SEC-1 ships, every operator action is identifiable only by
their `employee_id` value. Reuse it the way an email address would
be reused — uniquely, per human, never as a role.

---

*Last updated 2026-05-04. Owners: P0-SEC-5 UNASSIGNED (operational),
P3-SEC-1 UNASSIGNED.*
