// Per-API-key IP allow-list + scope enforcement.
//
// The existing api-key infrastructure in `security.rs` only checks
// (api_key → subject + secret + role + enabled). For institutional
// deployments, two more controls are non-negotiable:
//
//   1. IP allow-list. An API key leak from a customer's laptop is
//      neutralised if the key only works from their colocation /
//      office IP ranges.
//   2. Scope. A "trade-only" key must NOT be able to call
//      `/v2/wallet/withdraw`. A "read-only" key must NOT submit
//      orders. Scope mismatch = 403 before the action runs.
//
// Why a separate module instead of modifying `ApiKeyFileEntry`:
//
//   * The 775-test surface around `security.rs` is exactly the kind
//     of thing where a "small" struct-shape change cascades into
//     dozens of unrelated tests via serialization. Keeping the scope
//     metadata in a sidecar registry lets us ship the gate without
//     touching the auth happy path.
//
//   * Operators run BOTH paths today: HMAC-internal (server-to-server,
//     trusted) and API-key (customer-facing, untrusted). Only the
//     latter needs scope; this module makes that distinction explicit.
//
// File format: JSON at `data/api_key_scopes.json` by default
// (override via `API_KEY_SCOPE_FILE`). Schema:
//
//   [
//     {
//       "subject": "user-42",
//       "ip_whitelist": ["203.0.113.0/24", "2001:db8::/32"],
//       "scopes": ["read", "trade"]
//     }
//   ]
//
// Behaviour when a subject is NOT in the file:
//   * `check_ip_allowed`  → true (no restriction)
//   * `require_scope`     → ok (no restriction)
//
// This is the backwards-compatible default. To enforce, operators
// populate the file. Production keys SHOULD have an entry; absence is
// detected by a `/admin/api-key-scopes/unscoped` audit endpoint.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct ApiKeyScopeEntry {
    pub(crate) subject: String,
    #[serde(default)]
    pub(crate) ip_whitelist: Vec<String>,
    #[serde(default)]
    pub(crate) scopes: Vec<String>,
}

#[derive(Debug, Clone)]
struct CompiledEntry {
    nets: Vec<IpNet>,
    scopes: HashSet<String>,
}

#[derive(Default)]
pub(crate) struct ScopeRegistry {
    by_subject: RwLock<HashMap<String, CompiledEntry>>,
}

impl ScopeRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Construct from a JSON file. Missing file is treated as empty —
    /// the registry is purely additive over the existing api-key path.
    pub(crate) fn from_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let registry = Self::default();
        registry.reload_from_file(path)?;
        Ok(registry)
    }

    /// Read the file and replace the in-memory map atomically. Safe
    /// to call from an admin endpoint for hot-reload.
    pub(crate) fn reload_from_file(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let path = path.as_ref();
        if !path.exists() {
            *self.by_subject.write() = HashMap::new();
            return Ok(());
        }
        let body = std::fs::read_to_string(path)?;
        let entries: Vec<ApiKeyScopeEntry> = serde_json::from_str(&body)?;
        let mut by_subject = HashMap::new();
        for entry in entries {
            let mut nets = Vec::with_capacity(entry.ip_whitelist.len());
            for raw in &entry.ip_whitelist {
                match IpNet::parse(raw) {
                    Some(net) => nets.push(net),
                    None => anyhow::bail!(
                        "invalid CIDR `{raw}` for subject `{}`",
                        entry.subject
                    ),
                }
            }
            by_subject.insert(
                entry.subject.clone(),
                CompiledEntry {
                    nets,
                    scopes: entry.scopes.into_iter().collect(),
                },
            );
        }
        *self.by_subject.write() = by_subject;
        Ok(())
    }

    /// True when (subject, remote_ip) is allowed:
    ///   * subject not in registry → always true (no restriction)
    ///   * subject in registry with empty ip_whitelist → always true
    ///   * subject in registry with non-empty list → true iff remote
    ///     matches at least one network
    pub(crate) fn check_ip_allowed(&self, subject: &str, remote: IpAddr) -> bool {
        let guard = self.by_subject.read();
        let Some(entry) = guard.get(subject) else {
            return true;
        };
        if entry.nets.is_empty() {
            return true;
        }
        entry.nets.iter().any(|net| net.contains(remote))
    }

    /// True when subject has the requested scope (or no scope entry —
    /// backwards compatible).
    pub(crate) fn has_scope(&self, subject: &str, scope: &str) -> bool {
        let guard = self.by_subject.read();
        let Some(entry) = guard.get(subject) else {
            return true;
        };
        if entry.scopes.is_empty() {
            return true;
        }
        entry.scopes.contains(scope)
    }

    pub(crate) fn list_unscoped(&self, known_subjects: &[String]) -> Vec<String> {
        let guard = self.by_subject.read();
        known_subjects
            .iter()
            .filter(|s| !guard.contains_key(*s))
            .cloned()
            .collect()
    }

    pub(crate) fn entry_count(&self) -> usize {
        self.by_subject.read().len()
    }
}

/// CIDR-style IP network — handles both v4 and v6 without pulling
/// in `ipnet` as a crate dep (the api crate already has a thin
/// surface; one more transitive dep was not worth the simplicity).
#[derive(Debug, Clone)]
struct IpNet {
    base: u128,
    prefix: u8,
    is_v6: bool,
}

impl IpNet {
    fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        let (addr_part, prefix_part) = match trimmed.split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (trimmed, None),
        };
        let addr: IpAddr = addr_part.parse().ok()?;
        let (base, max_prefix, is_v6) = match addr {
            IpAddr::V4(v4) => (u128::from(u32::from(v4)), 32u8, false),
            IpAddr::V6(v6) => (u128::from_be_bytes(v6.octets()), 128u8, true),
        };
        let prefix = match prefix_part {
            Some(p) => p.parse::<u8>().ok()?,
            None => max_prefix,
        };
        if prefix > max_prefix {
            return None;
        }
        // Mask the base so trailing host bits are zeroed.
        let host_bits = (max_prefix - prefix) as u32;
        let mask = if host_bits >= 128 {
            0u128
        } else {
            !0u128 << host_bits
        };
        Some(Self {
            base: base & mask,
            prefix,
            is_v6,
        })
    }

    fn contains(&self, addr: IpAddr) -> bool {
        let (candidate, candidate_is_v6) = match addr {
            IpAddr::V4(v4) => (u128::from(u32::from(v4)), false),
            IpAddr::V6(v6) => (u128::from_be_bytes(v6.octets()), true),
        };
        if candidate_is_v6 != self.is_v6 {
            return false;
        }
        let max_prefix = if self.is_v6 { 128 } else { 32 };
        let host_bits = (max_prefix - self.prefix) as u32;
        let mask = if host_bits >= 128 {
            0u128
        } else {
            !0u128 << host_bits
        };
        (candidate & mask) == self.base
    }
}

pub(crate) type SharedScopeRegistry = Arc<ScopeRegistry>;

pub(crate) fn configured_scope_path() -> PathBuf {
    std::env::var("API_KEY_SCOPE_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data/api_key_scopes.json"))
}

// ── HTTP routes ────────────────────────────────────────────────

#[allow(unused_imports)]
use super::*;

/// Routes:
///   POST /admin/api-key-scopes/reload         — reload file into memory
///   GET  /admin/api-key-scopes/unscoped       — subjects without an entry
///   GET  /admin/api-key-scopes/check?subject= — show effective gates
pub(crate) fn build_routes(
    registry: SharedScopeRegistry,
    scope_path: PathBuf,
    ip_rate_limiter: Arc<FixedWindowRateLimiter>,
    admin_rate_limiter: Arc<FixedWindowRateLimiter>,
) -> JsonRoute {
    let reg1 = registry.clone();
    let path1 = scope_path.clone();
    let ip1 = ip_rate_limiter.clone();
    let adm1 = admin_rate_limiter.clone();
    let reload_route = warp::path!("admin" / "api-key-scopes" / "reload")
        .and(warp::post())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let reg = reg1.clone();
                let path = path1.clone();
                let ip_rl = ip1.clone();
                let adm_rl = adm1.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    adm_rl.check(&format!("admin:{}", principal.subject), 10)?;
                    reg.reload_from_file(&path)
                        .map_err(reject_internal_error)?;
                    tracing::info!(
                        admin = %principal.subject,
                        loaded = reg.entry_count(),
                        path = %path.display(),
                        "api-key scope registry reloaded"
                    );
                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "loaded_entries": reg.entry_count(),
                        "path": path.to_string_lossy(),
                    })))
                }
            },
        );

    let reg2 = registry.clone();
    let ip2 = ip_rate_limiter.clone();
    let unscoped_route = warp::path!("admin" / "api-key-scopes" / "unscoped")
        .and(warp::get())
        .and(with_principal())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal, remote: Option<SocketAddr>| {
                let reg = reg2.clone();
                let ip_rl = ip2.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    // Enumerate api-key registry subjects via the existing
                    // global accessor. Subjects WITHOUT a scope entry are
                    // currently unrestricted; ops uses this to audit.
                    let known = crate::security::known_api_key_subjects();
                    let unscoped = reg.list_unscoped(&known);
                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "known_subjects": known.len(),
                        "scoped_entries": reg.entry_count(),
                        "unscoped_subjects": unscoped,
                    })))
                }
            },
        );

    #[derive(Debug, serde::Deserialize)]
    struct CheckQuery {
        subject: String,
        scope: Option<String>,
        remote_ip: Option<String>,
    }

    let reg3 = registry;
    let ip3 = ip_rate_limiter;
    let check_route = warp::path!("admin" / "api-key-scopes" / "check")
        .and(warp::get())
        .and(with_principal())
        .and(warp::query::<CheckQuery>())
        .and(remote_ip())
        .and_then(
            move |principal: AuthenticatedPrincipal,
                  query: CheckQuery,
                  remote: Option<SocketAddr>| {
                let reg = reg3.clone();
                let ip_rl = ip3.clone();
                async move {
                    require_admin(&principal)?;
                    let ip_key = remote
                        .map(|v| v.ip().to_string())
                        .unwrap_or_else(|| format!("user:{}", principal.subject));
                    ip_rl.check(&format!("ip:{ip_key}"), 60)?;
                    let probe_ip: Option<std::net::IpAddr> =
                        query.remote_ip.as_deref().and_then(|s| s.parse().ok());
                    let ip_allowed = match probe_ip {
                        Some(addr) => Some(reg.check_ip_allowed(&query.subject, addr)),
                        None => None,
                    };
                    let scope_allowed = query
                        .scope
                        .as_deref()
                        .map(|s| reg.has_scope(&query.subject, s));
                    Ok::<_, Rejection>(warp::reply::json(&serde_json::json!({
                        "status": "ok",
                        "subject": query.subject,
                        "ip_allowed": ip_allowed,
                        "scope_allowed": scope_allowed,
                    })))
                }
            },
        );

    reload_route
        .or(unscoped_route)
        .unify()
        .or(check_route)
        .unify()
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn write_registry(content: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "api_key_scopes_{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn cidr_parse_and_contains_v4() {
        let net = IpNet::parse("10.0.0.0/8").unwrap();
        assert!(net.contains("10.1.2.3".parse().unwrap()));
        assert!(!net.contains("11.0.0.1".parse().unwrap()));
        // Host-bit address parses and masks correctly.
        let net2 = IpNet::parse("10.0.0.5/24").unwrap();
        assert!(net2.contains("10.0.0.1".parse().unwrap()));
        assert!(!net2.contains("10.0.1.1".parse().unwrap()));
    }

    #[test]
    fn cidr_parse_and_contains_v6() {
        let net = IpNet::parse("2001:db8::/32").unwrap();
        assert!(net.contains("2001:db8::1".parse().unwrap()));
        assert!(net.contains("2001:db8:1:2::3".parse().unwrap()));
        assert!(!net.contains("2001:dead::1".parse().unwrap()));
    }

    #[test]
    fn v4_does_not_match_v6_net_and_vice_versa() {
        let net4 = IpNet::parse("10.0.0.0/8").unwrap();
        assert!(!net4.contains("2001:db8::1".parse().unwrap()));
        let net6 = IpNet::parse("2001:db8::/32").unwrap();
        assert!(!net6.contains("10.0.0.1".parse().unwrap()));
    }

    #[test]
    fn host_address_without_prefix_acts_as_exact_match() {
        let net = IpNet::parse("203.0.113.7").unwrap();
        assert!(net.contains("203.0.113.7".parse().unwrap()));
        assert!(!net.contains("203.0.113.8".parse().unwrap()));
    }

    #[test]
    fn invalid_cidr_rejected() {
        assert!(IpNet::parse("203.0.113.7/33").is_none());
        assert!(IpNet::parse("not-an-ip").is_none());
        assert!(IpNet::parse("2001:db8::/129").is_none());
    }

    #[test]
    fn registry_without_entry_allows_everything() {
        let reg = ScopeRegistry::new();
        let any_ip = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        assert!(reg.check_ip_allowed("unknown-subject", any_ip));
        assert!(reg.has_scope("unknown-subject", "withdraw"));
    }

    #[test]
    fn registry_with_empty_lists_allows_everything() {
        let path = write_registry(r#"[{"subject":"u","ip_whitelist":[],"scopes":[]}]"#);
        let reg = ScopeRegistry::from_file(&path).unwrap();
        let any_ip = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        assert!(reg.check_ip_allowed("u", any_ip));
        assert!(reg.has_scope("u", "withdraw"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn registry_enforces_ip_whitelist() {
        let path = write_registry(
            r#"[{"subject":"u","ip_whitelist":["10.0.0.0/8"],"scopes":["trade"]}]"#,
        );
        let reg = ScopeRegistry::from_file(&path).unwrap();
        assert!(reg.check_ip_allowed("u", "10.0.5.1".parse().unwrap()));
        assert!(!reg.check_ip_allowed("u", "11.0.0.1".parse().unwrap()));
        assert!(reg.has_scope("u", "trade"));
        assert!(!reg.has_scope("u", "withdraw"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_treated_as_empty() {
        let reg =
            ScopeRegistry::from_file(std::env::temp_dir().join("does-not-exist-jknk5")).unwrap();
        assert_eq!(reg.entry_count(), 0);
        // Any subject is allowed.
        assert!(reg.check_ip_allowed("anyone", "10.0.0.1".parse().unwrap()));
    }

    #[test]
    fn invalid_cidr_in_file_returns_err() {
        let path = write_registry(r#"[{"subject":"u","ip_whitelist":["bad"],"scopes":[]}]"#);
        let result = ScopeRegistry::from_file(&path);
        assert!(result.is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn list_unscoped_finds_keys_without_entry() {
        let path = write_registry(r#"[{"subject":"in","ip_whitelist":[],"scopes":[]}]"#);
        let reg = ScopeRegistry::from_file(&path).unwrap();
        let unscoped = reg.list_unscoped(&[
            "in".to_string(),
            "out-1".to_string(),
            "out-2".to_string(),
        ]);
        assert_eq!(unscoped, vec!["out-1".to_string(), "out-2".to_string()]);
        let _ = std::fs::remove_file(&path);
    }
}
