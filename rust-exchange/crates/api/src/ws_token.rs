// P2-SEC-2 / RC 0.1 follow-up: short-TTL bearer tokens for browser
// WebSocket auth. Browser `WebSocket` cannot set custom headers on
// the upgrade request, so the existing internal-auth HMAC scheme
// (which signs `x-internal-auth-*` headers) is unreachable from the
// frontend. This module mints a token over an authenticated REST
// call and verifies it as a `?token=` query param on the WS path.
//
// Wire format:
//
//   v1.<hex(subject)>.<role>.<issued_at_ms>.<hex(ws_path)>.<sig_hex>
//
// Signature payload (UTF-8, lines joined by `\n`):
//
//   ws-token-v1
//   <hex(subject)>
//   <role>
//   <issued_at_ms>
//   <hex(ws_path)>
//
// Why hex-encode subject and path on the wire: keeps the token URL-
// safe regardless of what characters the subject or path contain
// (subjects come from external IdPs and may include `+` `/` `=`),
// and avoids needing a base64 dependency.
//
// Lifetime: 60 s default, overridable via `WS_TOKEN_TTL_SECS` (clamped
// to [10, 300]). Short TTL is the security boundary — a leaked token
// expires fast. The frontend mints a fresh token before each WS
// connect, so the typical token never lives more than a few seconds.

use std::time::SystemTime;

use hmac::Mac;
use serde::Deserialize;

use super::{AuthenticatedPrincipal, HmacSha256, PrincipalRole};

const TOKEN_VERSION: &str = "v1";
const SIG_DOMAIN: &str = "ws-token-v1";
const DEFAULT_TTL_SECS: u64 = 60;
const MIN_TTL_SECS: u64 = 10;
const MAX_TTL_SECS: u64 = 300;

/// Read the configured TTL, clamped to a safe range. A long TTL is
/// the failure mode here — a leaked token with a 1-day TTL is a
/// silent privilege escalation.
pub(crate) fn ws_token_ttl_secs() -> u64 {
    std::env::var("WS_TOKEN_TTL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TTL_SECS)
        .clamp(MIN_TTL_SECS, MAX_TTL_SECS)
}

#[derive(Debug)]
pub(crate) enum WsTokenError {
    BadShape,
    UnknownVersion,
    BadEncoding,
    BadUtf8,
    BadIssuedAt,
    BadRole,
    BadSignature,
    Expired,
    NotYetValid,
    PathMismatch,
    SecretUnavailable,
    EmptySubject,
}

impl std::fmt::Display for WsTokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::BadShape => "ws token has wrong field count",
            Self::UnknownVersion => "ws token has unknown version",
            Self::BadEncoding => "ws token field not valid hex",
            Self::BadUtf8 => "ws token field is not valid UTF-8",
            Self::BadIssuedAt => "ws token issued_at not parseable",
            Self::BadRole => "ws token has invalid role",
            Self::BadSignature => "ws token signature mismatch",
            Self::Expired => "ws token expired",
            Self::NotYetValid => "ws token from the future",
            Self::PathMismatch => "ws token bound to a different ws path",
            Self::SecretUnavailable => "ws token: server secret not configured",
            Self::EmptySubject => "ws token: subject must not be empty",
        };
        f.write_str(s)
    }
}

impl std::error::Error for WsTokenError {}

/// Mint a token bound to (subject, role, ws_path) and the current time.
/// `secret` is the same shared secret used by `verify_internal_principal`.
pub(crate) fn mint_token(
    secret: &str,
    principal: &AuthenticatedPrincipal,
    ws_path: &str,
) -> Result<String, WsTokenError> {
    if principal.subject.trim().is_empty() {
        return Err(WsTokenError::EmptySubject);
    }
    let issued_at_ms = current_unix_ms();
    let role = role_str(principal.role);
    let subject_hex = hex::encode(principal.subject.as_bytes());
    let path_hex = hex::encode(ws_path.as_bytes());
    let payload = sig_payload(&subject_hex, role, issued_at_ms, &path_hex);
    let sig_hex = hex_hmac(secret, &payload)?;
    Ok(format!(
        "{TOKEN_VERSION}.{subject_hex}.{role}.{issued_at_ms}.{path_hex}.{sig_hex}"
    ))
}

/// Verify a token. Returns the principal it was minted for.
///
/// `expected_ws_path` MUST be the actual path the WebSocket request
/// was made on — this is what binds the token to a specific endpoint
/// so a token minted for `/ws/order-trace` can't be replayed against
/// `/ws/admin-firehose`.
///
/// `now_ms` is parameterised so tests can pin time.
pub(crate) fn verify_token(
    secret: &str,
    token: &str,
    expected_ws_path: &str,
    now_ms: u64,
    ttl_secs: u64,
) -> Result<AuthenticatedPrincipal, WsTokenError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 6 {
        return Err(WsTokenError::BadShape);
    }
    if parts[0] != TOKEN_VERSION {
        return Err(WsTokenError::UnknownVersion);
    }
    let subject_hex = parts[1];
    let role_raw = parts[2];
    let issued_at_str = parts[3];
    let path_hex = parts[4];
    let sig_hex_provided = parts[5];

    let subject_bytes = hex::decode(subject_hex).map_err(|_| WsTokenError::BadEncoding)?;
    let path_bytes = hex::decode(path_hex).map_err(|_| WsTokenError::BadEncoding)?;
    let subject = String::from_utf8(subject_bytes).map_err(|_| WsTokenError::BadUtf8)?;
    let bound_path = String::from_utf8(path_bytes).map_err(|_| WsTokenError::BadUtf8)?;
    let issued_at_ms: u64 = issued_at_str.parse().map_err(|_| WsTokenError::BadIssuedAt)?;
    let role = parse_role_strict(role_raw).ok_or(WsTokenError::BadRole)?;

    // Path binding (constant time not strictly necessary — the path
    // is not a secret — but cheap).
    if !constant_time_eq(bound_path.as_bytes(), expected_ws_path.as_bytes()) {
        return Err(WsTokenError::PathMismatch);
    }

    // Recompute signature over the wire fields and compare.
    let payload = sig_payload(subject_hex, role_raw, issued_at_ms, path_hex);
    let expected_sig_hex = hex_hmac(secret, &payload)?;
    if !constant_time_eq(expected_sig_hex.as_bytes(), sig_hex_provided.as_bytes()) {
        return Err(WsTokenError::BadSignature);
    }

    // Time check. Allow up to 5 s clock skew in the past direction
    // (issued_at_ms slightly ahead of our clock); reject anything
    // further into the future.
    let skew_floor_ms = now_ms.saturating_sub(ttl_secs * 1_000);
    let skew_ceiling_ms = now_ms + 5_000;
    if issued_at_ms < skew_floor_ms {
        return Err(WsTokenError::Expired);
    }
    if issued_at_ms > skew_ceiling_ms {
        return Err(WsTokenError::NotYetValid);
    }

    Ok(AuthenticatedPrincipal {
        subject,
        role,
        session_id: None,
    })
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Query-string filter shape for the `?token=...` parameter on
/// `/ws/order-trace`.
#[derive(Debug, Deserialize)]
pub(crate) struct TokenQuery {
    pub token: String,
}

/// Wrapper so `WsTokenError` can ride warp's `Rejection` machinery
/// without having to add a `From` to the error itself.
#[derive(Debug)]
pub(crate) struct WsTokenRejection(pub WsTokenError);

impl warp::reject::Reject for WsTokenRejection {}

/// One-shot helper: parse the query, resolve the secret, verify, and
/// return the bound `AuthenticatedPrincipal`. Invariant: only call from
/// a WS upgrade handler — the path-binding check assumes the caller
/// passes the actual path the request came in on.
pub(crate) fn resolve_principal_from_token(
    query: &TokenQuery,
    expected_ws_path: &str,
) -> Result<AuthenticatedPrincipal, WsTokenError> {
    let secret =
        crate::security::internal_auth_secret_opt().ok_or(WsTokenError::SecretUnavailable)?;
    let now = current_unix_ms();
    let ttl = ws_token_ttl_secs();
    verify_token(secret, &query.token, expected_ws_path, now, ttl)
}

fn role_str(r: PrincipalRole) -> &'static str {
    match r {
        PrincipalRole::User => "user",
        PrincipalRole::Admin => "admin",
    }
}

fn parse_role_strict(s: &str) -> Option<PrincipalRole> {
    match s {
        "user" => Some(PrincipalRole::User),
        "admin" => Some(PrincipalRole::Admin),
        _ => None,
    }
}

fn sig_payload(subject_hex: &str, role: &str, issued_at_ms: u64, path_hex: &str) -> String {
    format!("{SIG_DOMAIN}\n{subject_hex}\n{role}\n{issued_at_ms}\n{path_hex}")
}

fn hex_hmac(secret: &str, payload: &str) -> Result<String, WsTokenError> {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).map_err(|_| WsTokenError::SecretUnavailable)?;
    mac.update(payload.as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "dev-secret-change-me-to-32-chars-min!";
    const PATH: &str = "/ws/order-trace";

    fn principal(subject: &str, role: PrincipalRole) -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            subject: subject.into(),
            role,
            session_id: None,
        }
    }

    #[test]
    fn round_trip_user() {
        let p = principal("alice", PrincipalRole::User);
        let token = mint_token(SECRET, &p, PATH).unwrap();
        let now = current_unix_ms();
        let verified = verify_token(SECRET, &token, PATH, now, 60).unwrap();
        assert_eq!(verified.subject, "alice");
        assert!(matches!(verified.role, PrincipalRole::User));
    }

    #[test]
    fn round_trip_admin() {
        let p = principal("ops-1", PrincipalRole::Admin);
        let token = mint_token(SECRET, &p, PATH).unwrap();
        let now = current_unix_ms();
        let verified = verify_token(SECRET, &token, PATH, now, 60).unwrap();
        assert_eq!(verified.subject, "ops-1");
        assert!(matches!(verified.role, PrincipalRole::Admin));
    }

    #[test]
    fn rejects_token_minted_for_other_path() {
        let p = principal("alice", PrincipalRole::User);
        let token = mint_token(SECRET, &p, "/ws/order-trace").unwrap();
        let now = current_unix_ms();
        let err = verify_token(SECRET, &token, "/ws/admin-firehose", now, 60).unwrap_err();
        assert!(matches!(err, WsTokenError::PathMismatch));
    }

    #[test]
    fn rejects_tampered_signature() {
        let p = principal("alice", PrincipalRole::User);
        let token = mint_token(SECRET, &p, PATH).unwrap();
        // Flip the last hex char of the signature.
        let mut chars: Vec<char> = token.chars().collect();
        let last = chars.len() - 1;
        chars[last] = if chars[last] == '0' { 'f' } else { '0' };
        let tampered: String = chars.into_iter().collect();
        let now = current_unix_ms();
        let err = verify_token(SECRET, &tampered, PATH, now, 60).unwrap_err();
        assert!(matches!(err, WsTokenError::BadSignature));
    }

    #[test]
    fn rejects_role_elevation() {
        let p = principal("alice", PrincipalRole::User);
        let token = mint_token(SECRET, &p, PATH).unwrap();
        // Replace `user` field with `admin`. Sig won't recompute.
        let elevated = token.replace(".user.", ".admin.");
        assert_ne!(elevated, token);
        let now = current_unix_ms();
        let err = verify_token(SECRET, &elevated, PATH, now, 60).unwrap_err();
        assert!(matches!(err, WsTokenError::BadSignature));
    }

    #[test]
    fn rejects_expired_token() {
        let p = principal("alice", PrincipalRole::User);
        let token = mint_token(SECRET, &p, PATH).unwrap();
        let issued = current_unix_ms();
        // Pretend we're verifying 10 minutes later with a 60 s TTL.
        let later = issued + 600_000;
        let err = verify_token(SECRET, &token, PATH, later, 60).unwrap_err();
        assert!(matches!(err, WsTokenError::Expired));
    }

    #[test]
    fn rejects_token_from_different_secret() {
        let p = principal("alice", PrincipalRole::User);
        let token = mint_token(SECRET, &p, PATH).unwrap();
        let now = current_unix_ms();
        let err = verify_token("different-secret-also-32-chars-yes!!", &token, PATH, now, 60)
            .unwrap_err();
        assert!(matches!(err, WsTokenError::BadSignature));
    }

    #[test]
    fn rejects_malformed_token_shape() {
        let now = current_unix_ms();
        let err = verify_token(SECRET, "not-a-token", PATH, now, 60).unwrap_err();
        assert!(matches!(err, WsTokenError::BadShape));
    }

    #[test]
    fn rejects_unknown_version() {
        let p = principal("alice", PrincipalRole::User);
        let token = mint_token(SECRET, &p, PATH).unwrap();
        let bad = token.replacen("v1.", "v9.", 1);
        let now = current_unix_ms();
        let err = verify_token(SECRET, &bad, PATH, now, 60).unwrap_err();
        assert!(matches!(err, WsTokenError::UnknownVersion));
    }

    #[test]
    fn empty_subject_is_rejected_at_mint() {
        let p = principal("", PrincipalRole::User);
        let err = mint_token(SECRET, &p, PATH).unwrap_err();
        assert!(matches!(err, WsTokenError::EmptySubject));
    }

    #[test]
    fn ttl_is_clamped() {
        // Should not panic for any input. Direct unit on the helper.
        std::env::remove_var("WS_TOKEN_TTL_SECS");
        let default = ws_token_ttl_secs();
        assert!((MIN_TTL_SECS..=MAX_TTL_SECS).contains(&default));
        std::env::set_var("WS_TOKEN_TTL_SECS", "1");
        assert_eq!(ws_token_ttl_secs(), MIN_TTL_SECS);
        std::env::set_var("WS_TOKEN_TTL_SECS", "100000");
        assert_eq!(ws_token_ttl_secs(), MAX_TTL_SECS);
        std::env::remove_var("WS_TOKEN_TTL_SECS");
    }
}
