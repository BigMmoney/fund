use super::*;
use sha2::Digest;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use warp::hyper::body::Bytes;

/// Maximum age of the replay guard TTL.
const AUTHENTICATED_WRITE_REPLAY_GUARD_MAX_KEYS: usize = 100_000;
const AUTHENTICATED_WRITE_REPLAY_GUARD_CLEANUP_INTERVAL: u64 = 256;
const REPLAY_GUARD_WAL_PATH: &str = "data/replay_guard.jsonl";

/// Path to the file containing the internal auth shared secret.
/// If this file exists, it takes precedence over the environment variable.
const SECRET_FILE_PATH: &str = "data/internal_auth.secret";
const SECRET_FILE_ENV: &str = "INTERNAL_AUTH_SHARED_SECRET_FILE";
const ROLE_MAPPING_FILE_PATH: &str = "data/role_mapping.json";
const ROLE_MAPPING_FILE_ENV: &str = "SERVER_ROLE_MAPPING_FILE";
const API_KEY_REGISTRY_FILE_PATH: &str = "data/api_keys.json";
const API_KEY_REGISTRY_FILE_ENV: &str = "API_KEY_REGISTRY_FILE";

/// Server-side role overrides: maps subject (user_id) to their actual role.
/// This prevents clients from elevating privileges by sending a fake role header.
/// Empty map = trust client-provided role (legacy mode, not recommended for production).
static SERVER_ROLE_MAP: OnceLock<std::collections::HashMap<String, PrincipalRole>> =
    OnceLock::new();
static API_KEY_REGISTRY: OnceLock<std::collections::HashMap<String, ApiKeyRecord>> =
    OnceLock::new();

/// Per-IP authentication failure tracker for brute-force mitigation.
static AUTH_FAILURE_TRACKER: OnceLock<AuthFailureTracker> = OnceLock::new();

/// Check if an IP is currently banned due to excessive auth failures.
#[allow(dead_code)]
pub(crate) fn is_ip_banned(ip: &str) -> bool {
    AUTH_FAILURE_TRACKER
        .get()
        .is_some_and(|tracker| tracker.is_banned(ip))
}

struct AuthFailureTracker {
    max_failures: usize,
    ban_duration_secs: u64,
    failures: DashMap<String, (usize, Instant)>, // IP -> (count, first_failure)
}

impl AuthFailureTracker {
    fn new(max_failures: usize, ban_duration_secs: u64) -> Self {
        Self {
            max_failures,
            ban_duration_secs,
            failures: DashMap::new(),
        }
    }

    fn record_failure(&self, ip: &str) -> bool {
        // Returns true if the IP should be banned
        let mut entry = self
            .failures
            .entry(ip.to_string())
            .or_insert((0, Instant::now()));
        let (count, first_failure) = entry.value_mut();
        if first_failure.elapsed().as_secs() > self.ban_duration_secs {
            // Reset window
            *count = 1;
            *first_failure = Instant::now();
            return false;
        }
        *count += 1;
        *count > self.max_failures
    }

    fn is_banned(&self, ip: &str) -> bool {
        if let Some(entry) = self.failures.get(ip) {
            let (count, first_failure) = entry.value();
            if *count > self.max_failures
                && first_failure.elapsed().as_secs() < self.ban_duration_secs
            {
                return true;
            }
        }
        false
    }
}

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
            Duration::from_secs((crate::internal_auth_max_skew_seconds() + 5) as u64),
            AUTHENTICATED_WRITE_REPLAY_GUARD_MAX_KEYS,
            AUTHENTICATED_WRITE_REPLAY_GUARD_CLEANUP_INTERVAL,
        )
    })
}

fn should_enforce_authenticated_write_replay_guard(method: &Method) -> bool {
    !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ApiKeyFileEntry {
    api_key: String,
    subject: String,
    secret: String,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default = "default_api_key_enabled")]
    enabled: bool,
}

#[derive(Debug, Clone)]
struct ApiKeyRecord {
    subject: String,
    secret: String,
    role: PrincipalRole,
    session_id: Option<String>,
    enabled: bool,
}

fn default_api_key_enabled() -> bool {
    true
}

fn configured_secret_file_path() -> String {
    env::var(SECRET_FILE_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| SECRET_FILE_PATH.to_string())
}

fn configured_role_mapping_path() -> String {
    env::var(ROLE_MAPPING_FILE_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| ROLE_MAPPING_FILE_PATH.to_string())
}

fn configured_api_key_registry_path() -> String {
    env::var(API_KEY_REGISTRY_FILE_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| API_KEY_REGISTRY_FILE_PATH.to_string())
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
    // Priority 1: File-based secret (recommended for production)
    // Priority 2: Environment variable (legacy / development)
    let secret_file_path = configured_secret_file_path();
    let explicit_secret_file = env::var(SECRET_FILE_ENV)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let secret = if Path::new(&secret_file_path).exists() {
        let content = std::fs::read_to_string(&secret_file_path).map_err(|e| {
            anyhow::anyhow!("Failed to read secret file {}: {}", secret_file_path, e)
        })?;
        let trimmed = content.trim().to_string();
        if trimmed.is_empty() {
            anyhow::bail!("Secret file {} is empty", secret_file_path);
        }
        tracing::info!(
            "Loaded internal auth secret from file: {}",
            secret_file_path
        );
        trimmed
    } else if explicit_secret_file {
        anyhow::bail!(
            "Configured {}={} does not exist",
            SECRET_FILE_ENV,
            secret_file_path
        );
    } else {
        let env_secret = env::var("INTERNAL_AUTH_SHARED_SECRET").map_err(|_| {
            anyhow::anyhow!(
                "INTERNAL_AUTH_SHARED_SECRET must be configured, or place secret in {}",
                secret_file_path
            )
        })?;
        let trimmed = env_secret.trim().to_string();
        if trimmed.is_empty() {
            anyhow::bail!("INTERNAL_AUTH_SHARED_SECRET must not be empty");
        }
        tracing::warn!(
            "Using env var for auth secret. For production, place secret in {}",
            secret_file_path
        );
        trimmed
    };

    // Enforce minimum secret strength
    if secret.len() < 32 {
        anyhow::bail!(
            "Internal auth secret must be at least 32 characters (got {}). Use a cryptographically random value.",
            secret.len()
        );
    }

    let _ = INTERNAL_AUTH_SHARED_SECRET.set(secret);
    Ok(())
}

pub(crate) fn initialize_api_key_registry() -> anyhow::Result<()> {
    let registry_path = configured_api_key_registry_path();
    let path = Path::new(&registry_path);
    if !path.exists() {
        let _ = API_KEY_REGISTRY.set(std::collections::HashMap::new());
        tracing::warn!(
            "No API key registry file at {}. Public API key auth will be disabled.",
            registry_path
        );
        return Ok(());
    }

    let content = std::fs::read_to_string(path).map_err(|error| {
        anyhow::anyhow!(
            "Failed to read API key registry {}: {}",
            registry_path,
            error
        )
    })?;
    let entries: Vec<ApiKeyFileEntry> = serde_json::from_str(&content)
        .map_err(|error| anyhow::anyhow!("Invalid API key registry JSON: {}", error))?;
    let mut registry = std::collections::HashMap::new();
    for entry in entries {
        let api_key = entry.api_key.trim();
        let subject = entry.subject.trim();
        let secret = entry.secret.trim();
        if api_key.is_empty() || subject.is_empty() || secret.is_empty() {
            anyhow::bail!("api_key, subject, and secret must all be non-empty");
        }
        let role = match entry.role.as_deref() {
            Some(role) => parse_role(role)
                .ok_or_else(|| anyhow::anyhow!("invalid role in API key registry"))?,
            None => PrincipalRole::User,
        };
        registry.insert(
            api_key.to_string(),
            ApiKeyRecord {
                subject: subject.to_string(),
                secret: secret.to_string(),
                role,
                session_id: entry
                    .session_id
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
                enabled: entry.enabled,
            },
        );
    }
    let count = registry.len();
    let _ = API_KEY_REGISTRY.set(registry);
    tracing::info!(count, path = %registry_path, "Loaded API key registry");
    Ok(())
}

/// Initialize the server-side role mapping from a JSON file.
/// File format: {"user-1": "admin", "user-2": "user"}
pub(crate) fn initialize_role_mapping() -> anyhow::Result<()> {
    let role_map_path = configured_role_mapping_path();
    if Path::new(&role_map_path).exists() {
        let content = std::fs::read_to_string(&role_map_path)
            .map_err(|e| anyhow::anyhow!("Failed to read role mapping {}: {}", role_map_path, e))?;
        let raw: std::collections::HashMap<String, String> = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Invalid role mapping JSON: {}", e))?;
        let mut map = std::collections::HashMap::new();
        for (subject, role_str) in raw {
            if let Some(role) = parse_role(&role_str) {
                map.insert(subject, role);
            } else {
                tracing::warn!(
                    "Unknown role '{}' for subject '{}', skipping",
                    role_str,
                    subject
                );
            }
        }
        let _ = SERVER_ROLE_MAP.set(map);
        tracing::info!(
            "Loaded {} role mappings from {}",
            SERVER_ROLE_MAP.get().map(|m| m.len()).unwrap_or(0),
            role_map_path
        );
    } else {
        let _ = SERVER_ROLE_MAP.set(std::collections::HashMap::new());
        tracing::warn!("No role mapping file at {}. Client-provided roles will be trusted (not recommended for production).", role_map_path);
    }
    Ok(())
}

/// Initialize the authentication failure tracker.
pub(crate) fn initialize_auth_failure_tracker() {
    let _ = AUTH_FAILURE_TRACKER.set(AuthFailureTracker::new(
        10,  // max_failures before ban
        300, // ban_duration_secs (5 minutes)
    ));
}

/// Resolve the effective role for a subject, using server-side mapping if available.
fn resolve_role(subject: &str, client_role: PrincipalRole) -> PrincipalRole {
    if let Some(role_map) = SERVER_ROLE_MAP.get() {
        if let Some(&server_role) = role_map.get(subject) {
            return server_role;
        }
    }
    client_role
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

fn api_key_registry() -> &'static std::collections::HashMap<String, ApiKeyRecord> {
    API_KEY_REGISTRY.get_or_init(std::collections::HashMap::new)
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
fn api_key_auth_payload(
    method: &Method,
    path: &str,
    query: &str,
    api_key: &str,
    subject: &str,
    role: PrincipalRole,
    timestamp: i64,
    request_id: &str,
    body_hash: &str,
) -> String {
    let role = match role {
        PrincipalRole::User => "user",
        PrincipalRole::Admin => "admin",
    };
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        method.as_str(),
        path,
        query,
        api_key,
        subject,
        role,
        timestamp,
        request_id,
        body_hash
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
    let client_role = parse_role(&role_raw)
        .ok_or_else(|| reject_api(StatusCode::UNAUTHORIZED, "invalid internal auth role"))?;
    // Server-side role resolution: override client-provided role if server mapping exists
    let role = resolve_role(&subject, client_role);
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
    if (now - timestamp).abs() > crate::internal_auth_max_skew_seconds() {
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
        // Track auth failure for brute-force mitigation
        if let Some(tracker) = AUTH_FAILURE_TRACKER.get() {
            // We don't have IP here directly, but subject can be tracked
            tracker.record_failure(&subject);
        }
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

#[allow(clippy::too_many_arguments)]
fn verify_api_key_principal(
    method: Method,
    path: String,
    query: String,
    api_key: Option<String>,
    timestamp: Option<String>,
    signature: Option<String>,
    request_id: Option<String>,
    body_hash: Option<String>,
) -> Result<AuthenticatedPrincipal, Rejection> {
    let api_key = api_key
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| reject_api(StatusCode::UNAUTHORIZED, "missing x-api-key"))?;
    let record = api_key_registry()
        .get(&api_key)
        .cloned()
        .ok_or_else(|| reject_api(StatusCode::UNAUTHORIZED, "unknown api key"))?;
    if !record.enabled {
        return Err(reject_api(StatusCode::UNAUTHORIZED, "api key disabled"));
    }
    let timestamp_raw = timestamp
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| reject_api(StatusCode::UNAUTHORIZED, "missing x-api-timestamp"))?;
    let timestamp = timestamp_raw
        .parse::<i64>()
        .map_err(|_| reject_api(StatusCode::UNAUTHORIZED, "invalid x-api-timestamp"))?;
    let signature = signature
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| reject_api(StatusCode::UNAUTHORIZED, "missing x-api-signature"))?;
    let request_id = request_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| reject_api(StatusCode::UNAUTHORIZED, "missing x-request-id"))?;
    let now = Utc::now().timestamp();
    if (now - timestamp).abs() > crate::internal_auth_max_skew_seconds() {
        return Err(reject_api(
            StatusCode::UNAUTHORIZED,
            "api key timestamp outside allowed skew",
        ));
    }
    let body_hash = if matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) {
        body_hash.unwrap_or_default()
    } else {
        body_hash
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| reject_api(StatusCode::UNAUTHORIZED, "missing x-api-body-sha256"))?
    };
    let payload = api_key_auth_payload(
        &method,
        &path,
        &query,
        &api_key,
        &record.subject,
        record.role,
        timestamp,
        &request_id,
        &body_hash,
    );
    let signature_bytes = hex::decode(signature)
        .map_err(|_| reject_api(StatusCode::UNAUTHORIZED, "invalid x-api-signature"))?;
    let mut mac = HmacSha256::new_from_slice(record.secret.as_bytes()).map_err(|_| {
        reject_api(
            StatusCode::INTERNAL_SERVER_ERROR,
            "api key auth init failed",
        )
    })?;
    mac.update(payload.as_bytes());
    mac.verify_slice(&signature_bytes)
        .map_err(|_| reject_api(StatusCode::UNAUTHORIZED, "api key verification failed"))?;
    let session_id = record.session_id.clone().or_else(|| Some(api_key.clone()));
    if should_enforce_authenticated_write_replay_guard(&method) {
        let role = match record.role {
            PrincipalRole::User => "user",
            PrincipalRole::Admin => "admin",
        };
        authenticated_write_replay_guard().register(
            authenticated_write_replay_key(
                &method,
                &path,
                &query,
                &record.subject,
                role,
                session_id.as_deref().unwrap_or_default(),
                &request_id,
            ),
            &request_id,
        )?;
    }
    Ok(AuthenticatedPrincipal {
        subject: record.subject,
        role: record.role,
        session_id,
    })
}

#[allow(clippy::too_many_arguments)]
fn verify_optional_api_key_principal(
    method: Method,
    path: String,
    query: String,
    api_key: Option<String>,
    timestamp: Option<String>,
    signature: Option<String>,
    request_id: Option<String>,
    body_hash: Option<String>,
) -> Result<Option<AuthenticatedPrincipal>, Rejection> {
    let auth_present = [
        api_key.as_deref(),
        timestamp.as_deref(),
        signature.as_deref(),
    ]
    .into_iter()
    .any(|value| value.is_some_and(|inner| !inner.trim().is_empty()));
    if !auth_present {
        return Ok(None);
    }
    verify_api_key_principal(
        method, path, query, api_key, timestamp, signature, request_id, body_hash,
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
        .and(warp::header::optional::<String>("x-api-key"))
        .and(warp::header::optional::<String>("x-api-timestamp"))
        .and(warp::header::optional::<String>("x-api-signature"))
        .and(warp::header::optional::<String>("x-api-body-sha256"))
        .and_then(
            |method: Method,
             path: warp::path::FullPath,
             query: String,
             subject: Option<String>,
             role: Option<String>,
             session_id: Option<String>,
             timestamp: Option<String>,
             signature: Option<String>,
             request_id: Option<String>,
             api_key: Option<String>,
             api_timestamp: Option<String>,
             api_signature: Option<String>,
             api_body_hash: Option<String>| async move {
                let internal_present = [
                    subject.as_deref(),
                    role.as_deref(),
                    timestamp.as_deref(),
                    signature.as_deref(),
                ]
                .into_iter()
                .any(|value| value.is_some_and(|inner| !inner.trim().is_empty()));
                if internal_present {
                    return verify_internal_principal(
                        method,
                        path.as_str().to_string(),
                        query,
                        subject,
                        role,
                        session_id,
                        timestamp,
                        signature,
                        request_id,
                    );
                }
                verify_api_key_principal(
                    method,
                    path.as_str().to_string(),
                    query,
                    api_key,
                    api_timestamp,
                    api_signature,
                    request_id,
                    api_body_hash,
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
        .and(warp::header::optional::<String>("x-api-key"))
        .and(warp::header::optional::<String>("x-api-timestamp"))
        .and(warp::header::optional::<String>("x-api-signature"))
        .and(warp::header::optional::<String>("x-api-body-sha256"))
        .and_then(
            |method: Method,
             path: warp::path::FullPath,
             query: String,
             subject: Option<String>,
             role: Option<String>,
             session_id: Option<String>,
             timestamp: Option<String>,
             signature: Option<String>,
             request_id: Option<String>,
             api_key: Option<String>,
             api_timestamp: Option<String>,
             api_signature: Option<String>,
             api_body_hash: Option<String>| async move {
                let internal_present = [
                    subject.as_deref(),
                    role.as_deref(),
                    timestamp.as_deref(),
                    signature.as_deref(),
                ]
                .into_iter()
                .any(|value| value.is_some_and(|inner| !inner.trim().is_empty()));
                if internal_present {
                    return verify_optional_internal_principal(
                        method,
                        path.as_str().to_string(),
                        query,
                        subject,
                        role,
                        session_id,
                        timestamp,
                        signature,
                        request_id,
                    );
                }
                verify_optional_api_key_principal(
                    method,
                    path.as_str().to_string(),
                    query,
                    api_key,
                    api_timestamp,
                    api_signature,
                    request_id,
                    api_body_hash,
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
                "missing request body sha256 header",
            )
        })?;
    let actual_hash = body_sha256_hex(body.as_ref());
    if provided_hash.trim().to_ascii_lowercase() != actual_hash {
        return Err(reject_api(
            StatusCode::UNAUTHORIZED,
            "request body sha256 mismatch",
        ));
    }
    serde_json::from_slice(body.as_ref())
        .map_err(|_| reject_api(StatusCode::BAD_REQUEST, "invalid json body"))
}

pub(crate) fn verified_json_body<T>() -> impl Filter<Extract = (T,), Error = Rejection> + Clone
where
    T: DeserializeOwned + Send + 'static,
{
    warp::header::optional::<String>("x-auth-body-sha256")
        .and(warp::header::optional::<String>("x-api-body-sha256"))
        .and(warp::header::optional::<String>(
            "x-internal-auth-body-sha256",
        ))
        .and(warp::body::bytes())
        .and_then(
            |body_hash: Option<String>,
             api_body_hash: Option<String>,
             internal_body_hash: Option<String>,
             body: Bytes| async move {
                verify_json_body::<T>(body_hash.or(api_body_hash).or(internal_body_hash), body)
            },
        )
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

    fn ensure_test_api_key_registry() {
        let mut registry = std::collections::HashMap::new();
        registry.insert(
            "test-api-key".to_string(),
            ApiKeyRecord {
                subject: "user-1".to_string(),
                secret: "test-api-secret-which-is-long-enough-123".to_string(),
                role: PrincipalRole::User,
                session_id: Some("api-session-1".to_string()),
                enabled: true,
            },
        );
        let _ = API_KEY_REGISTRY.set(registry);
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

    #[test]
    fn configured_secret_file_path_prefers_env_override() {
        std::env::set_var(SECRET_FILE_ENV, "custom/secret.file");
        assert_eq!(configured_secret_file_path(), "custom/secret.file");
        std::env::remove_var(SECRET_FILE_ENV);
    }

    #[test]
    fn configured_role_mapping_path_prefers_env_override() {
        std::env::set_var(ROLE_MAPPING_FILE_ENV, "config/roles.json");
        assert_eq!(configured_role_mapping_path(), "config/roles.json");
        std::env::remove_var(ROLE_MAPPING_FILE_ENV);
    }

    #[test]
    fn configured_api_key_registry_path_prefers_env_override() {
        std::env::set_var(API_KEY_REGISTRY_FILE_ENV, "config/api-keys.json");
        assert_eq!(configured_api_key_registry_path(), "config/api-keys.json");
        std::env::remove_var(API_KEY_REGISTRY_FILE_ENV);
    }

    #[test]
    fn verify_api_key_principal_accepts_valid_signature() {
        let _guard = auth_test_lock().lock().expect("auth test lock");
        authenticated_write_replay_guard().clear();
        ensure_test_api_key_registry();
        let timestamp = Utc::now().timestamp();
        let request_id = "req-api-key-post";
        let body_hash = "abcd1234";
        let payload = api_key_auth_payload(
            &Method::POST,
            "/submit-order",
            "",
            "test-api-key",
            "user-1",
            PrincipalRole::User,
            timestamp,
            request_id,
            body_hash,
        );
        let signature = sign_payload(&payload, "test-api-secret-which-is-long-enough-123");
        let principal = verify_api_key_principal(
            Method::POST,
            "/submit-order".to_string(),
            "".to_string(),
            Some("test-api-key".to_string()),
            Some(timestamp.to_string()),
            Some(signature),
            Some(request_id.to_string()),
            Some(body_hash.to_string()),
        )
        .expect("api key auth accepted");
        assert_eq!(principal.subject, "user-1");
        assert_eq!(principal.role, PrincipalRole::User);
        assert_eq!(principal.session_id.as_deref(), Some("api-session-1"));
    }

    #[test]
    fn verify_api_key_principal_rejects_wrong_path() {
        let _guard = auth_test_lock().lock().expect("auth test lock");
        authenticated_write_replay_guard().clear();
        ensure_test_api_key_registry();
        let timestamp = Utc::now().timestamp();
        let request_id = "req-api-key-get";
        let payload = api_key_auth_payload(
            &Method::GET,
            "/orders/user-1",
            "market_id=btc-usdt",
            "test-api-key",
            "user-1",
            PrincipalRole::User,
            timestamp,
            request_id,
            "",
        );
        let signature = sign_payload(&payload, "test-api-secret-which-is-long-enough-123");
        let result = verify_api_key_principal(
            Method::GET,
            "/orders/user-1/wrong".to_string(),
            "market_id=btc-usdt".to_string(),
            Some("test-api-key".to_string()),
            Some(timestamp.to_string()),
            Some(signature),
            Some(request_id.to_string()),
            Some(String::new()),
        );
        assert!(result.is_err());
    }
}
