//! Secret loading + redaction (gate **P0-SEC-1**).
//!
//! Production sensitive material — HMAC shared secret, ETH hot-wallet
//! private key, sanctions API keys — must NOT live as plain env vars
//! in the launch image. This module defines the loader contract that
//! the `api` and `wallet` crates use, ships an `EnvSecretLoader`
//! suitable for dev / single-node staging, and exposes the integration
//! point for a real KMS-backed loader.
//!
//! Design rules:
//! 1. `SecretLoader::load(name)` returns a `Secret<String>` whose
//!    `Debug` impl REDACTS the value. Logging a Secret prints
//!    `Secret(redacted, len=N)` — safe for inclusion in
//!    error contexts that may flow to log aggregators.
//! 2. The underlying byte buffer is zeroed on drop. Without this, a
//!    core dump or process-memory inspection during a credential
//!    incident leaks the value indefinitely.
//! 3. Loaders MUST fail loudly on missing required keys at startup
//!    rather than at first use. A misconfigured production node
//!    should refuse to boot, not surprise an operator at 02:00 with
//!    a quiet 401.
//! 4. The KMS-backed loader scaffold (`KmsSecretLoader`) is documented
//!    as the production target. Its body — AWS KMS / HashiCorp Vault /
//!    Google Secret Manager — is provider-specific and fills in once
//!    the rotation runbook is drafted.

use std::sync::Arc;

/// In-memory secret. Constructed by a `SecretLoader`; `Debug` is
/// redacted; the raw value is exposed only via `expose()` so call
/// sites grep cleanly. The buffer is zeroed when the last `Arc<Secret>`
/// drops.
pub struct Secret {
    inner: String,
}

impl Secret {
    pub fn new(value: String) -> Self {
        Self { inner: value }
    }

    /// Caller MUST treat the returned string as ephemeral — do NOT
    /// log it, do NOT format it into structured output, do NOT
    /// persist it. Bind the slice to the narrowest possible scope.
    pub fn expose(&self) -> &str {
        &self.inner
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret(redacted, len={})", self.inner.len())
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        // Manual zeroize so a core dump after a credential incident
        // doesn't leak the value indefinitely. Not perfect — the
        // compiler may have written copies onto the stack during
        // formatting/parsing — but it's the right floor.
        // SAFETY: we own the String; overwriting in place before the
        // String is dropped is safe.
        unsafe {
            let bytes = self.inner.as_mut_vec();
            for b in bytes.iter_mut() {
                std::ptr::write_volatile(b, 0);
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("required secret not found: {0}")]
    Missing(String),
    #[error("secret backend error: {0}")]
    Backend(String),
}

/// Loader contract. Implementations are `Send + Sync` so they can be
/// shared across the api process and the worker tasks.
pub trait SecretLoader: Send + Sync {
    /// Stable identifier: `env`, `aws-kms`, `vault`, etc. Surfaced on
    /// the `/admin/me/permissions` and reconciliation reports so an
    /// operator can verify the production node is using the right
    /// backend.
    fn provider_id(&self) -> &str;

    /// Load a secret by logical name. Implementations MAY cache
    /// per-name; the `api` boot loads each secret once.
    fn load(&self, name: &str) -> Result<Secret, SecretError>;
}

/// Plain-env loader. Suitable for dev and single-node staging where
/// secrets live in a sealed `.env` file or systemd `EnvironmentFile=`
/// directive owned by a dedicated user. Production should use
/// `KmsSecretLoader` (gate P0-SEC-1).
pub struct EnvSecretLoader;

impl SecretLoader for EnvSecretLoader {
    fn provider_id(&self) -> &str {
        "env"
    }

    fn load(&self, name: &str) -> Result<Secret, SecretError> {
        std::env::var(name)
            .map(Secret::new)
            .map_err(|_| SecretError::Missing(name.to_string()))
    }
}

/// KMS-backed loader scaffold. The real implementation calls AWS KMS
/// `Decrypt` (or HashiCorp Vault / Google Secret Manager equivalent)
/// for each named entry. The configuration carries the KMS key id and
/// per-secret ciphertext blobs.
///
/// Disabled in v1: returns `SecretError::Backend("scaffold")` from
/// every `load`, so a misconfigured production node fails closed at
/// boot. Operators flip to this loader once the rotation runbook
/// (P2-SEC-1) is drafted and tested.
pub struct KmsSecretLoader {
    #[allow(dead_code)]
    kms_key_id: String,
    /// Map of logical secret name to base64-encoded KMS ciphertext.
    /// Loaded once at startup from a sealed config file.
    #[allow(dead_code)]
    ciphertexts: std::collections::HashMap<String, String>,
}

impl KmsSecretLoader {
    pub fn new(
        kms_key_id: impl Into<String>,
        ciphertexts: std::collections::HashMap<String, String>,
    ) -> Self {
        Self {
            kms_key_id: kms_key_id.into(),
            ciphertexts,
        }
    }
}

impl SecretLoader for KmsSecretLoader {
    fn provider_id(&self) -> &str {
        "aws-kms"
    }

    fn load(&self, name: &str) -> Result<Secret, SecretError> {
        Err(SecretError::Backend(format!(
            "kms loader is a scaffold — name={name}; see docs/KMS_SECRETS_RUNBOOK.md"
        )))
    }
}

/// Convenience: pick the loader from `WALLET_SECRET_BACKEND`. Default
/// is `env`. `kms` returns the scaffold (which fails closed). Other
/// values panic so misspelled config can't silently fall through.
pub fn loader_from_env() -> Arc<dyn SecretLoader> {
    match std::env::var("WALLET_SECRET_BACKEND")
        .unwrap_or_else(|_| "env".to_string())
        .as_str()
    {
        "env" => Arc::new(EnvSecretLoader),
        "kms" => {
            let kms_key_id = std::env::var("WALLET_SECRET_KMS_KEY_ID")
                .expect("WALLET_SECRET_KMS_KEY_ID must be set when WALLET_SECRET_BACKEND=kms");
            Arc::new(KmsSecretLoader::new(
                kms_key_id,
                std::collections::HashMap::new(),
            ))
        }
        other => panic!("unknown WALLET_SECRET_BACKEND: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_value() {
        let s = Secret::new("super-secret-value".to_string());
        let dbg = format!("{:?}", s);
        assert!(!dbg.contains("super-secret"));
        assert!(dbg.contains("redacted"));
        assert!(dbg.contains("len=18"));
    }

    #[test]
    fn expose_returns_inner_value() {
        let s = Secret::new("plain".into());
        assert_eq!(s.expose(), "plain");
    }

    #[test]
    fn env_loader_finds_set_var() {
        std::env::set_var("WALLET_SECRET_TEST_KEY", "value-1");
        let loader = EnvSecretLoader;
        let secret = loader.load("WALLET_SECRET_TEST_KEY").unwrap();
        assert_eq!(secret.expose(), "value-1");
        std::env::remove_var("WALLET_SECRET_TEST_KEY");
    }

    #[test]
    fn env_loader_errors_on_missing() {
        let loader = EnvSecretLoader;
        match loader.load("WALLET_SECRET_DEFINITELY_NOT_SET") {
            Err(SecretError::Missing(name)) => {
                assert_eq!(name, "WALLET_SECRET_DEFINITELY_NOT_SET");
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn kms_scaffold_fails_closed() {
        let loader = KmsSecretLoader::new("alias/exchange-prod", Default::default());
        assert!(matches!(
            loader.load("HMAC_KEY"),
            Err(SecretError::Backend(_))
        ));
        assert_eq!(loader.provider_id(), "aws-kms");
    }
}
