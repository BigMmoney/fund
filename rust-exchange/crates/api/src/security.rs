use super::*;
use sha2::Digest;
use std::sync::atomic::{AtomicU64, Ordering};
use warp::hyper::body::Bytes;

const AUTHENTICATED_WRITE_REPLAY_GUARD_MAX_KEYS: usize = 100_000;
const AUTHENTICATED_WRITE_REPLAY_GUARD_CLEANUP_INTERVAL: u64 = 256;
const REPLAY_GUARD_WAL_PATH: &str = "data/replay_guard.jsonl";

static AUTHENTICATED_WRITE_REPLAY_GUARD: OnceLock<AuthenticatedWriteReplayGuard> = OnceLock::new();

struct AuthenticatedWriteReplayGuard {
    ttl: Duration,
    max_keys: usize,
    cleanup_interval: u64,
    cleanup_counter: AtomicU64,
    seen: DashMap<String, Instant>,
    /// Persistent request_id set that survives restarts (H-7 fix)
    persisted_request_ids: parking_lot::Mutex<std::collections::HashSet<String>>,
}

impl AuthenticatedWriteReplayGuard {
    fn new(ttl: Duration, max_keys: usize, cleanup_interval: u64) -> Self {
        let persisted = Self::load_persisted_request_ids();
        Self {
            ttl,
            max_keys,
            cleanup_interval,
            cleanup_counter: AtomicU64::new(0),
            seen: DashMap::new(),
            persisted_request_ids: parking_lot::Mutex::new(persisted),
        }
    }

    fn load_persisted_request_ids() -> std::collections::HashSet<String> {
        let path = std::path::Path::new(REPLAY_GUARD_WAL_PATH);
        let mut set = std::collections::HashSet::new();
        if !path.exists() {
            return set;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            return set;
        };
        let cutoff = Utc::now().timestamp() - 60; // keep last 60 seconds
        for line in content.lines() {
            if let Some((ts_str, request_id)) = line.split_once('\t') {
                if let Ok(ts) = ts_str.parse::<i64>() {
                    if ts >= cutoff {
                        set.insert(request_id.to_string());
                    }
                }
            }
        }
        tracing::info!(
            recovered = set.len(),
            "replay guard: loaded persisted request_ids"
        );
        set
    }

    fn persist_request_id(&self, request_id: &str) {
        let ts = Utc::now().timestamp();
        let mut persisted = self.persisted_request_ids.lock();
        persisted.insert(request_id.to_string());
        // Best-effort append to WAL file
        let path = std::path::Path::new(REPLAY_GUARD_WAL_PATH);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{ts}\t{request_id}")
            });
    }

    fn is_request_id_persisted(&self, request_id: &str) -> bool {
        self.persisted_request_ids.lock().contains(request_id)
    }

    fn register(&self, key: String, request_id: &str) -> Result<(), Rejection> {
        let now = Instant::now();
        self.maybe_cleanup(now);
        if self.seen.contains_key(&key) || self.is_request_id_persisted(request_id) {
            return Err(reject_api(
                StatusCode::CONFLICT,
                "duplicate authenticated write request",
            ));
        }
        if self.seen.len() >= self.max_keys {
            self.cleanup_expired(now);
            if self.seen.len() >= self.max_keys && !self.seen.contains_key(&key) {
                return Err(reject_api(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "authenticated write replay guard saturated",
                ));
            }
        }
        self.seen.insert(key, now);
        self.persist_request_id(request_id);
        Ok(())
    }

    fn maybe_cleanup(&self, now: Instant) {
        let attempt = self.cleanup_counter.fetch_add(1, Ordering::Relaxed) + 1;
        if attempt % self.cleanup_interval == 0 || self.seen.len() > self.max_keys {
            self.cleanup_expired(now);
        }
    }

    fn cleanup_expired(&self, now: Instant) {
        self.seen
            .retain(|_, seen_at| now.duration_since(*seen_at) <= self.ttl);
    }

    #[cfg(test)]
    fn clear(&self) {
        self.seen.clear();
        self.persisted_request_ids.lock().clear();
        self.cleanup_counter.store(0, Ordering::Relaxed);
    }
}

fn authenticated_write_replay_guard() -> &'static AuthenticatedWriteReplayGuard {
    AUTHENTICATED_WRITE_REPLAY_GUARD.get_or_init(|| {
        AuthenticatedWriteReplayGuard::new(
            Duration::from_secs((INTERNAL_AUTH_MAX_SKEW_SECONDS + 5) as u64),
            AUTHENTICATED_WRITE_REPLAY_GUARD_MAX_KEYS,
            AUTHENTICATED_WRITE_REPLAY_GUARD_CLEANUP_INTERVAL,
        )
    })
}

fn should_enforce_authenticated_write_replay_guard(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn authenticated_write_replay_key(
    method: &Method,
    path: &str,
    query: &str,
    subject: &str,
    role: &str,
    session_id: &str,
    request_id: &str,
) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}",
        method.as_str(),
        path,
        query,
        subject,
        role,
        session_id,
        request_id
    )
}

pub(crate) fn parse_role(value: &str) -> Option<PrincipalRole> {
    match value.trim().to_ascii_lowercase().as_str() {
        "user" => Some(PrincipalRole::User),
        "admin" => Some(PrincipalRole::Admin),
        _ => None,
    }
}

pub(crate) fn initialize_internal_auth_secret() -> anyhow::Result<()> {
    let secret = env::var("INTERNAL_AUTH_SHARED_SECRET")
        .map_err(|_| anyhow::anyhow!("INTERNAL_AUTH_SHARED_SECRET must be configured"))?;
    let secret = secret.trim().to_string();
    if secret.is_empty() {
        anyhow::bail!("INTERNAL_AUTH_SHARED_SECRET must not be empty");
    }
    let _ = INTERNAL_AUTH_SHARED_SECRET.set(secret);
    Ok(())
}

fn internal_auth_secret() -> Result<&'static str, Rejection> {
    INTERNAL_AUTH_SHARED_SECRET
        .get()
        .map(|value| value.as_str())
        .ok_or_else(|| {
            reject_api(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal auth is not configured",
            )
        })
}

#[allow(clippy::too_many_arguments)]
fn internal_auth_payload(
    method: &Method,
    path: &str,
    query: &str,
    subject: &str,
    role: &str,
    session_id: &str,
    timestamp: i64,
    request_id: &str,
) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        method.as_str(),
        path,
        query,
        subject,
        role,
        session_id,
        timestamp,
        request_id
    )
}

#[allow(clippy::too_many_arguments)]
fn verify_internal_principal(
    method: Method,
    path: String,
    query: String,
    subject: Option<String>,
    role: Option<String>,
    session_id: Option<String>,
    timestamp: Option<String>,
    signature: Option<String>,
    request_id: Option<String>,
) -> Result<AuthenticatedPrincipal, Rejection> {
    let subject = subject
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| reject_api(StatusCode::UNAUTHORIZED, "missing internal auth subject"))?;
    let role_raw = role
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| reject_api(StatusCode::UNAUTHORIZED, "missing internal auth role"))?;
    let role = parse_role(&role_raw)
        .ok_or_else(|| reject_api(StatusCode::UNAUTHORIZED, "invalid internal auth role"))?;
    let timestamp_raw = timestamp
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| reject_api(StatusCode::UNAUTHORIZED, "missing internal auth timestamp"))?;
    let timestamp = timestamp_raw
        .parse::<i64>()
        .map_err(|_| reject_api(StatusCode::UNAUTHORIZED, "invalid internal auth timestamp"))?;
    let signature = signature
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| reject_api(StatusCode::UNAUTHORIZED, "missing internal auth signature"))?;
    let request_id = request_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| reject_api(StatusCode::UNAUTHORIZED, "missing x-request-id"))?;
    let now = Utc::now().timestamp();
    if (now - timestamp).abs() > INTERNAL_AUTH_MAX_SKEW_SECONDS {
        return Err(reject_api(
            StatusCode::UNAUTHORIZED,
            "internal auth timestamp outside allowed skew",
        ));
    }
    let session_id = session_id.unwrap_or_default();
    let payload = internal_auth_payload(
        &method,
        &path,
        &query,
        &subject,
        &role_raw.to_ascii_lowercase(),
        &session_id,
        timestamp,
        &request_id,
    );
    let signature_bytes = hex::decode(signature)
        .map_err(|_| reject_api(StatusCode::UNAUTHORIZED, "invalid internal auth signature"))?;
    let mut mac = HmacSha256::new_from_slice(internal_auth_secret()?.as_bytes()).map_err(|_| {
        reject_api(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal auth init failed",
        )
    })?;
    mac.update(payload.as_bytes());
    mac.verify_slice(&signature_bytes).map_err(|_| {
        reject_api(
            StatusCode::UNAUTHORIZED,
            "internal auth verification failed",
        )
    })?;
    if should_enforce_authenticated_write_replay_guard(&method) {
        authenticated_write_replay_guard().register(
            authenticated_write_replay_key(
                &method,
                &path,
                &query,
                &subject,
                &role_raw.to_ascii_lowercase(),
                &session_id,
                &request_id,
            ),
            &request_id,
        )?;
    }
    Ok(AuthenticatedPrincipal {
        subject,
        role,
        session_id: if session_id.trim().is_empty() {
            None
        } else {
            Some(session_id)
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn verify_optional_internal_principal(
    method: Method,
    path: String,
    query: String,
    subject: Option<String>,
    role: Option<String>,
    session_id: Option<String>,
    timestamp: Option<String>,
    signature: Option<String>,
    request_id: Option<String>,
) -> Result<Option<AuthenticatedPrincipal>, Rejection> {
    let auth_present = [
        subject.as_deref(),
        role.as_deref(),
        timestamp.as_deref(),
        signature.as_deref(),
    ]
    .into_iter()
    .any(|value| value.is_some_and(|inner| !inner.trim().is_empty()));
    if !auth_present {
        return Ok(None);
    }
    verify_internal_principal(
        method, path, query, subject, role, session_id, timestamp, signature, request_id,
    )
    .map(Some)
}

fn canonical_query_filter() -> impl Filter<Extract = (String,), Error = Infallible> + Clone {
    warp::query::raw().or(warp::any().map(String::new)).unify()
}

pub(crate) fn with_principal(
) -> impl Filter<Extract = (AuthenticatedPrincipal,), Error = Rejection> + Clone {
    warp::method()
        .and(warp::path::full())
        .and(canonical_query_filter())
        .and(warp::header::optional::<String>("x-internal-auth-subject"))
        .and(warp::header::optional::<String>("x-internal-auth-role"))
        .and(warp::header::optional::<String>(
            "x-internal-auth-session-id",
        ))
        .and(warp::header::optional::<String>(
            "x-internal-auth-timestamp",
        ))
        .and(warp::header::optional::<String>(
            "x-internal-auth-signature",
        ))
        .and(warp::header::optional::<String>("x-request-id"))
        .and_then(
            |method: Method,
             path: warp::path::FullPath,
             query: String,
             subject: Option<String>,
             role: Option<String>,
             session_id: Option<String>,
             timestamp: Option<String>,
             signature: Option<String>,
             request_id: Option<String>| async move {
                verify_internal_principal(
                    method,
                    path.as_str().to_string(),
                    query,
                    subject,
                    role,
                    session_id,
                    timestamp,
                    signature,
                    request_id,
                )
            },
        )
}

pub(crate) fn with_optional_principal(
) -> impl Filter<Extract = (Option<AuthenticatedPrincipal>,), Error = Rejection> + Clone {
    warp::method()
        .and(warp::path::full())
        .and(canonical_query_filter())
        .and(warp::header::optional::<String>("x-internal-auth-subject"))
        .and(warp::header::optional::<String>("x-internal-auth-role"))
        .and(warp::header::optional::<String>(
            "x-internal-auth-session-id",
        ))
        .and(warp::header::optional::<String>(
            "x-internal-auth-timestamp",
        ))
        .and(warp::header::optional::<String>(
            "x-internal-auth-signature",
        ))
        .and(warp::header::optional::<String>("x-request-id"))
        .and_then(
            |method: Method,
             path: warp::path::FullPath,
             query: String,
             subject: Option<String>,
             role: Option<String>,
             session_id: Option<String>,
             timestamp: Option<String>,
             signature: Option<String>,
             request_id: Option<String>| async move {
                verify_optional_internal_principal(
                    method,
                    path.as_str().to_string(),
                    query,
                    subject,
                    role,
                    session_id,
                    timestamp,
                    signature,
                    request_id,
                )
            },
        )
}

fn body_sha256_hex(body: &[u8]) -> String {
    hex::encode(sha2::Sha256::digest(body))
}

fn verify_json_body<T>(body_hash: Option<String>, body: Bytes) -> Result<T, Rejection>
where
    T: DeserializeOwned,
{
    let provided_hash = body_hash
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            reject_api(
                StatusCode::UNAUTHORIZED,
                "missing x-internal-auth-body-sha256",
            )
        })?;
    let actual_hash = body_sha256_hex(body.as_ref());
    if provided_hash.trim().to_ascii_lowercase() != actual_hash {
        return Err(reject_api(
            StatusCode::UNAUTHORIZED,
            "x-internal-auth-body-sha256 mismatch",
        ));
    }
    serde_json::from_slice(body.as_ref())
        .map_err(|_| reject_api(StatusCode::BAD_REQUEST, "invalid json body"))
}

pub(crate) fn verified_json_body<T>() -> impl Filter<Extract = (T,), Error = Rejection> + Clone
where
    T: DeserializeOwned + Send + 'static,
{
    warp::header::optional::<String>("x-internal-auth-body-sha256")
        .and(warp::body::bytes())
        .and_then(|body_hash: Option<String>, body: Bytes| async move {
            verify_json_body::<T>(body_hash, body)
        })
}

pub(crate) fn require_user(principal: &AuthenticatedPrincipal) -> Result<(), Rejection> {
    match principal.role {
        PrincipalRole::User | PrincipalRole::Admin => Ok(()),
    }
}

pub(crate) fn require_admin(principal: &AuthenticatedPrincipal) -> Result<(), Rejection> {
    if principal.role != PrincipalRole::Admin {
        return Err(reject_api(StatusCode::FORBIDDEN, "admin role required"));
    }
    Ok(())
}

pub(crate) fn ensure_subject_or_admin(
    principal: &AuthenticatedPrincipal,
    user_id: &str,
) -> Result<(), Rejection> {
    if principal.role == PrincipalRole::Admin {
        if principal.subject != user_id {
            tracing::info!(
                admin = %principal.subject,
                target_user = %user_id,
                "admin cross-account access"
            );
        }
        return Ok(());
    }
    ensure_subject_matches(principal, user_id)
}

pub(crate) fn ensure_subject_matches(
    principal: &AuthenticatedPrincipal,
    claimed_user_id: &str,
) -> Result<(), Rejection> {
    if claimed_user_id.trim().is_empty() {
        return Err(reject_api(StatusCode::BAD_REQUEST, "user_id is required"));
    }
    if principal.subject != claimed_user_id {
        return Err(reject_api(
            StatusCode::FORBIDDEN,
            "user_id does not match authenticated subject",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn auth_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn sign_payload(payload: &str, secret: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac init");
        mac.update(payload.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    #[test]
    fn internal_auth_payload_includes_path() {
        let payload = internal_auth_payload(
            &Method::POST,
            "/order/submit",
            "",
            "user-1",
            "user",
            "session-1",
            1_700_000_000,
            "req-1",
        );
        assert!(payload.contains("/order/submit"));
        assert!(!payload.contains("/order/cancel"));
    }

    #[test]
    fn verify_internal_principal_rejects_signature_for_wrong_path() {
        let _ = INTERNAL_AUTH_SHARED_SECRET.set("test-secret".to_string());
        let timestamp = Utc::now().timestamp();
        let payload = internal_auth_payload(
            &Method::POST,
            "/order/submit",
            "",
            "user-1",
            "user",
            "session-1",
            timestamp,
            "req-1",
        );
        let signature = sign_payload(&payload, "test-secret");
        let result = verify_internal_principal(
            Method::POST,
            "/order/cancel".to_string(),
            "".to_string(),
            Some("user-1".to_string()),
            Some("user".to_string()),
            Some("session-1".to_string()),
            Some(timestamp.to_string()),
            Some(signature),
            Some("req-1".to_string()),
        );
        assert!(result.is_err());
    }

    #[test]
    fn verify_internal_principal_rejects_signature_for_wrong_query() {
        let _ = INTERNAL_AUTH_SHARED_SECRET.set("test-secret".to_string());
        let timestamp = Utc::now().timestamp();
        let payload = internal_auth_payload(
            &Method::GET,
            "/orders",
            "market_id=btc-usdt",
            "user-1",
            "user",
            "session-1",
            timestamp,
            "req-2",
        );
        let signature = sign_payload(&payload, "test-secret");
        let result = verify_internal_principal(
            Method::GET,
            "/orders".to_string(),
            "market_id=eth-usdt".to_string(),
            Some("user-1".to_string()),
            Some("user".to_string()),
            Some("session-1".to_string()),
            Some(timestamp.to_string()),
            Some(signature),
            Some("req-2".to_string()),
        );
        assert!(result.is_err());
    }

    #[test]
    fn verify_json_body_rejects_mismatched_hash() {
        let result = verify_json_body::<serde_json::Value>(
            Some("deadbeef".to_string()),
            Bytes::from_static(br#"{"hello":"world"}"#),
        );
        assert!(result.is_err());
    }

    #[test]
    fn verify_json_body_accepts_matching_hash() {
        let body = Bytes::from_static(br#"{"hello":"world"}"#);
        let hash = body_sha256_hex(body.as_ref());
        let parsed =
            verify_json_body::<serde_json::Value>(Some(hash), body).expect("body verified");
        assert_eq!(parsed["hello"], "world");
    }

    #[test]
    fn verify_internal_principal_rejects_replayed_authenticated_write() {
        let _guard = auth_test_lock().lock().expect("auth test lock");
        let _ = INTERNAL_AUTH_SHARED_SECRET.set("test-secret".to_string());
        authenticated_write_replay_guard().clear();
        let timestamp = Utc::now().timestamp();
        let request_id = "req-replay-post";
        let payload = internal_auth_payload(
            &Method::POST,
            "/admin/risk/prices/index",
            "",
            "admin-1",
            "admin",
            "session-1",
            timestamp,
            request_id,
        );
        let signature = sign_payload(&payload, "test-secret");
        verify_internal_principal(
            Method::POST,
            "/admin/risk/prices/index".to_string(),
            "".to_string(),
            Some("admin-1".to_string()),
            Some("admin".to_string()),
            Some("session-1".to_string()),
            Some(timestamp.to_string()),
            Some(signature.clone()),
            Some(request_id.to_string()),
        )
        .expect("first write accepted");
        let replay = verify_internal_principal(
            Method::POST,
            "/admin/risk/prices/index".to_string(),
            "".to_string(),
            Some("admin-1".to_string()),
            Some("admin".to_string()),
            Some("session-1".to_string()),
            Some(timestamp.to_string()),
            Some(signature),
            Some(request_id.to_string()),
        );
        assert!(replay.is_err());
    }

    #[test]
    fn verify_internal_principal_allows_repeated_reads_with_same_request_id() {
        let _guard = auth_test_lock().lock().expect("auth test lock");
        let _ = INTERNAL_AUTH_SHARED_SECRET.set("test-secret".to_string());
        authenticated_write_replay_guard().clear();
        let timestamp = Utc::now().timestamp();
        let request_id = "req-replay-get";
        let payload = internal_auth_payload(
            &Method::GET,
            "/orders",
            "market_id=btc-usdt",
            "user-1",
            "user",
            "session-1",
            timestamp,
            request_id,
        );
        let signature = sign_payload(&payload, "test-secret");
        verify_internal_principal(
            Method::GET,
            "/orders".to_string(),
            "market_id=btc-usdt".to_string(),
            Some("user-1".to_string()),
            Some("user".to_string()),
            Some("session-1".to_string()),
            Some(timestamp.to_string()),
            Some(signature.clone()),
            Some(request_id.to_string()),
        )
        .expect("first read accepted");
        verify_internal_principal(
            Method::GET,
            "/orders".to_string(),
            "market_id=btc-usdt".to_string(),
            Some("user-1".to_string()),
            Some("user".to_string()),
            Some("session-1".to_string()),
            Some(timestamp.to_string()),
            Some(signature),
            Some(request_id.to_string()),
        )
        .expect("repeated read accepted");
    }
}
