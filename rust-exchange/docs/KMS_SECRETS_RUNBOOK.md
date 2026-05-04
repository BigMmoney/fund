# KMS Secrets Runbook

> How sensitive material (HMAC shared secret, ETH hot-wallet private
> key, sanctions API keys) is provisioned, loaded, rotated, and
> revoked. Authoritative for gates **P0-SEC-1** + **P2-SEC-1**.
>
> The application code is already abstracted behind
> `wallet::SecretLoader` (see `crates/wallet/src/secrets.rs`). This
> runbook covers the operational side: WHERE the ciphertexts live,
> WHO can touch them, and HOW to rotate without downtime.

---

## 1. Inventory

| Logical name | Used by | Rotation cadence | Pages whom on rotation? |
|---|---|---|---|
| `INTERNAL_AUTH_SHARED_SECRET` | `with_principal()` HMAC verification | 90 days | Security |
| `WALLET_ETH_HOT_PRIVATE_KEY` | ETH chain adapter signing path | quarterly + on incident | Wallet on-call + Security |
| `CHAINALYSIS_API_KEY` | sanctions screening | per provider TTL (annual) | Compliance |
| (planned) `OIDC_SESSION_SIGNING_KEY` | session JWT mint/verify | 30 days | Security |
| (planned) `BACKUP_ENCRYPTION_KEY` | client-side encryption of S3 backups | yearly | Platform |

Every value MUST be loadable through `SecretLoader::load(name)`. If
a future feature needs sensitive material that isn't loadable this
way, that's a P0 blocker on its own.

---

## 2. Backend choice

For v1 production: **AWS KMS** with envelope encryption.

| Why not... | Reason |
|---|---|
| Plain env var | No rotation primitive; secret hits process arg list and `/proc` |
| Sealed `.env` (mozilla/sops, age) | Acceptable for staging; loses rotation property at scale |
| HashiCorp Vault | Fine alternative; choose if existing infra |
| AWS Secrets Manager | Easier UI but doubles cost vs raw KMS for this volume |

If a different backend is chosen, the rest of this runbook describes
the required SHAPE of operations — substitute the provider-specific
commands.

---

## 3. Provisioning (initial)

### 3.1 Create the KMS key

One regional CMK per environment. Production:

```
aws kms create-key \
  --description "rust-exchange production secrets — DO NOT DELETE" \
  --key-usage ENCRYPT_DECRYPT \
  --key-spec SYMMETRIC_DEFAULT \
  --tags TagKey=service,TagValue=exchange TagKey=env,TagValue=prod \
  --policy file://kms-policy.json
```

`kms-policy.json` allows:
- Root account (break-glass admin only)
- The `exchange` IAM role used by the api process: `kms:Decrypt`
- The `secret-rotator` role (humans + automation): `kms:Encrypt`,
  `kms:GenerateDataKey`

DENY everything else, including `kms:DeleteKey`. Deletion requires
a manual policy edit + 30-day waiting period.

Alias the key for human readability:

```
aws kms create-alias \
  --alias-name alias/exchange-prod-secrets \
  --target-key-id <key-id-from-create>
```

### 3.2 Encrypt each secret

For every entry in §1's inventory:

```
secret_value="$(generate-or-fetch)"
ciphertext=$(echo -n "$secret_value" \
  | aws kms encrypt \
      --key-id alias/exchange-prod-secrets \
      --plaintext fileb:///dev/stdin \
      --query CiphertextBlob \
      --output text)
echo "$ciphertext" > "/etc/exchange/secrets/${LOGICAL_NAME}.kms"
unset secret_value
```

The plaintext stays only in shell history (which the operator clears
before logging out — `shred ~/.bash_history` or use a no-history
shell). Production: use AWS Secrets Manager's GenerateDataKey path
to avoid the plaintext-on-host step entirely.

### 3.3 Bake the ciphertexts into the deploy

The api process reads ciphertexts from `/etc/exchange/secrets/*.kms`
at boot. Two delivery mechanisms:

| Setup | Mechanism |
|---|---|
| Kubernetes | Mount a `Secret` resource that contains the ciphertext blobs (NOT plaintext); the Pod's IAM role permits `kms:Decrypt` |
| systemd / VM | sealed file at `/etc/exchange/secrets/`; owned by `exchange:exchange` mode `0600` |

The `KmsSecretLoader` (currently a scaffold) reads each blob, calls
`kms:Decrypt`, and returns the plaintext as a `Secret` to the caller.
Plaintext NEVER touches disk after that — `Secret`'s Drop impl zeroes
the buffer.

---

## 4. Loading at runtime

`api` startup:

```rust
let loader = wallet::loader_from_env();
// WALLET_SECRET_BACKEND=kms  in production
// WALLET_SECRET_KMS_KEY_ID=alias/exchange-prod-secrets

let hmac = loader.load("INTERNAL_AUTH_SHARED_SECRET")
    .expect("HMAC secret must be loadable");
// hmac is a Secret; expose() once into the auth state, then drop the
// Secret. The auth state holds the bytes only as long as the process
// is running.
```

Failure modes at startup:

| Symptom | Cause | Action |
|---|---|---|
| `SecretError::Missing(name)` | Ciphertext file absent OR backend doesn't know the name | Re-check provisioning step §3.2 |
| `SecretError::Backend(...)` | KMS API returned an error | Most likely IAM (the api role lost `kms:Decrypt`); re-grant |
| Process panics at startup | Required secret is missing | Refuse to serve traffic — the right behaviour |

Production deploy MUST fail-closed: if any secret fails to load,
the api process exits with a non-zero status and the orchestrator
restarts (and re-fails) until the operator fixes the config.

---

## 5. Rotation

### 5.1 HMAC shared secret (90 days)

Two-secret window so existing signed requests don't break mid-rotation:

1. Generate new secret: `openssl rand -hex 32`
2. Encrypt under KMS, push as `INTERNAL_AUTH_SHARED_SECRET_NEXT`
3. Roll the api: every node now ACCEPTS both secrets but SIGNS only
   with the existing one (new responses bound for verification —
   admin tooling — use the existing).
4. Wait the max session window (1 hour for OIDC sessions; 30 minutes
   for batch job back-pressure).
5. Promote: rename `*_NEXT` → primary, drop the old.
6. Roll the api again: now signs and verifies with the new only.

### 5.2 ETH hot-wallet private key (quarterly + on incident)

Hot-wallet keys protect on-chain custody. Rotation procedure ties
into WITHDRAWAL_RISK_AND_CUSTODY.md §3.4 (drain → rotate → refill).

**Critical:** the OLD key MUST be revoked from the KMS the moment
the new hot wallet is funded. Otherwise a leaked old key still has
read access to drained-but-not-burned funds.

```
# After successful drain
aws kms schedule-key-deletion \
  --key-id <old-key-id> \
  --pending-window-in-days 30
```

### 5.3 Sanctions API key

Provider-specific; follow Chainalysis's docs. Two-key window not
needed — the customer-wallet handler already treats provider errors
as soft-block, so a brief auth blip surfaces as 503 to customers,
not as a silent allow.

### 5.4 OIDC session signing key (30 days)

Once P3-SEC-1 ships:

1. Maintain TWO active signing keys at all times: `current` and
   `previous`.
2. Mint new sessions with `current`.
3. Verify with EITHER `current` or `previous`.
4. On rotation: shift `current` → `previous`, generate a fresh
   `current`. Old sessions issued under the now-`previous` key
   continue to verify until they expire (max 1 hour).

This is a standard JWT key-id (`kid`) rotation; the verifier picks
the matching key from a JWKS-style map.

---

## 6. Revocation (incident path)

When a credential is suspected compromised:

| Step | Action |
|---|---|
| 1 | Page security on-call. Document the suspicion with a unique incident ID. |
| 2 | Trigger §9 kill switches for any affected surface (e.g. for ETH key compromise: pause hot-wallet worker). |
| 3 | Generate a new value, encrypt under KMS, push to the secret store. |
| 4 | Restart the api: the old value goes out of memory; the new one comes in. |
| 5 | Schedule deletion of the old KMS-encrypted blob (30-day window). |
| 6 | Reconcile: did the leaked credential get used? Audit `data/wallet/customer_audit.jsonl`, `data/admin/rbac_audit.jsonl`, the chain RPC tx history. |
| 7 | Post-mortem: how did the leak happen? Tighten `kms:Decrypt` IAM if relevant. |

Acceptance for P0-SEC-1 sign-off: the rotation procedure §5.1 has
been executed in staging (§5.1 step 1-6 with non-real values) and
documented in the launch report.

---

## 7. Why we zeroize on drop

Even with KMS doing the heavy lifting, the plaintext value lives in
process memory between `loader.load(name)` and the consumer's last
use. If a malicious operator gets a core dump (or the kernel writes
the page out to swap during high pressure), the plaintext is leaked.

`Secret`'s Drop impl writes zeros over the buffer before the
allocator reclaims it. This isn't perfect — the compiler may have
spilled copies onto the stack during formatting — but it's the
right floor. The Rust ecosystem's `zeroize` crate is the next step
when the v1 launch is done; for v1 we ship the manual write_volatile
loop.

---

## 8. Auditing

`/admin/me/session` (planned) and the existing `/admin/me/permissions`
return the loader's `provider_id()` so the operator can verify the
production node is actually using KMS:

```json
{
  "employee_id": "alice",
  "secret_backend": "aws-kms",
  "kms_key_alias": "alias/exchange-prod-secrets"
}
```

If `secret_backend` returns `env` in production, that's a sev-1
configuration drift — the deploy went out without the KMS toggle.

---

*Last updated 2026-05-04. Owner: security UNASSIGNED.*
