//! Chainalysis sanctions adapter (gate **P0-SEC-6**).
//!
//! Hits the Chainalysis Sanctions API
//! (`https://public.chainalysis.com/api/v1/address/{address}`) per
//! address with header `X-API-Key: $CHAINALYSIS_API_KEY`. Maps the
//! response to `SanctionsCheckResult`:
//!
//! - HTTP 200 with empty `identifications` → `Clear`
//! - HTTP 200 with one+ `identifications` → `Hit` (carries the matched
//!   list name + matched entity)
//! - Network / HTTP error / parse error → `Error` (soft block; the
//!   customer wallet handler treats this as 503 SanctionsUnavailable)
//!
//! Synchronous on `ureq` so the `SanctionsProvider` trait stays sync
//! and callers don't need an async runtime. Production tunings:
//! - `CHAINALYSIS_TIMEOUT_MS` per call (default 2000)
//! - `CHAINALYSIS_MAX_RETRIES` (default 2)
//! - `CHAINALYSIS_API_URL` for staging override

#![cfg(feature = "chainalysis")]

use std::time::Duration;

use chrono::Utc;
use serde::Deserialize;

use crate::sanctions::SanctionsProvider;
use crate::secrets::{Secret, SecretLoader};
use crate::types::{ChainId, SanctionsCheckResult, SanctionsHit, SanctionsScreenStatus};

const PROVIDER_ID: &str = "chainalysis";
const DEFAULT_API_URL: &str = "https://public.chainalysis.com/api/v1/address";
const DEFAULT_TIMEOUT_MS: u64 = 2_000;
const DEFAULT_MAX_RETRIES: u32 = 2;

/// Loaded once at startup. The API key is held as a `Secret` so it
/// never appears in logs, panics, or core dumps; it's exposed only
/// inside `screen()` while building the request header.
pub struct ChainalysisProvider {
    api_url: String,
    api_key: Secret,
    timeout: Duration,
    max_retries: u32,
    agent: ureq::Agent,
}

impl ChainalysisProvider {
    pub fn new(api_url: String, api_key: Secret, timeout: Duration, max_retries: u32) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout(timeout)
            .timeout_connect(Duration::from_millis(timeout.as_millis() as u64 / 2))
            .build();
        Self {
            api_url,
            api_key,
            timeout,
            max_retries,
            agent,
        }
    }

    /// Build via the `SecretLoader` contract (gate P0-SEC-1). The
    /// secret name is fixed: `CHAINALYSIS_API_KEY`.
    pub fn from_loader(loader: &dyn SecretLoader) -> Self {
        let api_key = loader
            .load("CHAINALYSIS_API_KEY")
            .expect("CHAINALYSIS_API_KEY must be loadable when --features chainalysis");
        let api_url =
            std::env::var("CHAINALYSIS_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_string());
        let timeout_ms: u64 = std::env::var("CHAINALYSIS_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_TIMEOUT_MS);
        let max_retries: u32 = std::env::var("CHAINALYSIS_MAX_RETRIES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_RETRIES);
        Self::new(api_url, api_key, Duration::from_millis(timeout_ms), max_retries)
    }

    fn endpoint(&self, address: &str) -> String {
        format!("{}/{}", self.api_url.trim_end_matches('/'), address)
    }

    /// Single call attempt. `Ok(Some(_))` = response with identifications.
    /// `Ok(None)` = clean response (no identifications). `Err(_)` =
    /// transport / decode error; the caller may retry up to
    /// `max_retries` times before falling back to `Error`.
    fn try_screen(&self, address: &str) -> Result<Option<Vec<Identification>>, AttemptError> {
        let resp = self
            .agent
            .get(&self.endpoint(address))
            .set("Accept", "application/json")
            .set("X-API-Key", self.api_key.expose())
            .call()
            .map_err(|e| AttemptError::Transport(e.to_string()))?;
        if resp.status() != 200 {
            return Err(AttemptError::Http(resp.status()));
        }
        let body: ApiResponse = resp
            .into_json()
            .map_err(|e| AttemptError::Decode(e.to_string()))?;
        Ok(body.identifications.filter(|v| !v.is_empty()))
    }
}

#[derive(Debug)]
enum AttemptError {
    Transport(String),
    Http(u16),
    Decode(String),
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    #[serde(default)]
    identifications: Option<Vec<Identification>>,
}

#[derive(Debug, Deserialize)]
struct Identification {
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

impl SanctionsProvider for ChainalysisProvider {
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    fn screen(&self, _chain: ChainId, address: &str) -> SanctionsCheckResult {
        let now = Utc::now();
        // Up to (1 + max_retries) attempts on transport/5xx errors.
        // Decode errors short-circuit (we don't retry against the same
        // bad payload). On 4xx we also short-circuit — that's a key
        // / quota issue and retrying won't help.
        let mut last_error: Option<String> = None;
        for attempt in 0..=self.max_retries {
            match self.try_screen(address) {
                Ok(None) => {
                    return SanctionsCheckResult {
                        status: SanctionsScreenStatus::Clear,
                        provider: PROVIDER_ID.to_string(),
                        checked_at: now,
                        hit: None,
                        correlation_id: Some(format!("chainalysis-clear-{address}")),
                    };
                }
                Ok(Some(idents)) => {
                    let first = idents.first();
                    let list_name = first
                        .and_then(|i| i.category.clone())
                        .unwrap_or_else(|| "chainalysis-list".to_string());
                    let matched_entity = first
                        .and_then(|i| i.name.clone())
                        .unwrap_or_else(|| address.to_string());
                    let _ = first.and_then(|i| i.description.as_deref());
                    return SanctionsCheckResult {
                        status: SanctionsScreenStatus::Hit,
                        provider: PROVIDER_ID.to_string(),
                        checked_at: now,
                        hit: Some(SanctionsHit {
                            list_name,
                            matched_entity,
                            score: 100,
                            matched_at: now,
                        }),
                        correlation_id: Some(format!("chainalysis-hit-{address}")),
                    };
                }
                Err(AttemptError::Transport(msg)) => {
                    last_error = Some(format!("transport: {msg}"));
                    // continue retry
                }
                Err(AttemptError::Http(code)) if (500..600).contains(&code) => {
                    last_error = Some(format!("http {code}"));
                    // continue retry
                }
                Err(AttemptError::Http(code)) => {
                    // 4xx — don't retry; usually auth / quota / bad
                    // request. Fail closed (Error -> 503 to the caller).
                    last_error = Some(format!("http {code} (no retry)"));
                    break;
                }
                Err(AttemptError::Decode(msg)) => {
                    last_error = Some(format!("decode: {msg} (no retry)"));
                    break;
                }
            }
            // Tiny linear backoff between attempts.
            std::thread::sleep(Duration::from_millis(50 * (attempt as u64 + 1)));
        }
        SanctionsCheckResult {
            status: SanctionsScreenStatus::Error,
            provider: PROVIDER_ID.to_string(),
            checked_at: now,
            hit: None,
            correlation_id: Some(format!(
                "chainalysis-error-{address} ({})",
                last_error.unwrap_or_else(|| "unknown".into())
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider_pointing_at(api_url: &str) -> ChainalysisProvider {
        ChainalysisProvider::new(
            api_url.to_string(),
            Secret::new("test-key".to_string()),
            Duration::from_millis(200),
            0,
        )
    }

    #[test]
    fn unreachable_endpoint_returns_error_status() {
        // A port that's almost certainly closed. We expect a transport
        // error → status Error so the customer-wallet handler hard-soft-
        // blocks rather than failing open.
        let p = provider_pointing_at("http://127.0.0.1:1");
        let r = p.screen(ChainId::Eth, "0xanything");
        assert_eq!(r.status, SanctionsScreenStatus::Error);
        assert_eq!(r.provider, PROVIDER_ID);
        assert!(r
            .correlation_id
            .as_ref()
            .unwrap()
            .starts_with("chainalysis-error-"));
    }

    #[test]
    fn from_loader_uses_secret_loader_for_key() {
        std::env::set_var("CHAINALYSIS_API_KEY_TEST", "from-loader");
        struct StubLoader;
        impl SecretLoader for StubLoader {
            fn provider_id(&self) -> &str {
                "stub"
            }
            fn load(&self, name: &str) -> Result<Secret, crate::secrets::SecretError> {
                if name == "CHAINALYSIS_API_KEY" {
                    Ok(Secret::new("loader-supplied".into()))
                } else {
                    Err(crate::secrets::SecretError::Missing(name.into()))
                }
            }
        }
        // Override the default URL to a closed port so the constructor
        // doesn't actually hit the network during the test.
        std::env::set_var("CHAINALYSIS_API_URL", "http://127.0.0.1:1");
        let provider = ChainalysisProvider::from_loader(&StubLoader);
        assert_eq!(provider.provider_id(), PROVIDER_ID);
        assert_eq!(provider.api_key.expose(), "loader-supplied");
        std::env::remove_var("CHAINALYSIS_API_URL");
        std::env::remove_var("CHAINALYSIS_API_KEY_TEST");
    }
}
