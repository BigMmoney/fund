// P2-SEC-3: defensive HTTP response headers applied to every reply.
//
// Scope: the warp `routes` chain in `main.rs` wraps every response —
// both REST JSON and the static frontend under `./frontend/` — with
// this header set. Per-route opt-out is intentionally not provided;
// the launch checklist row says "all REST responses".
//
// Headers:
//
//   X-Content-Type-Options: nosniff        — block MIME sniffing on
//       error pages and any response with a wrong/missing Content-Type
//
//   X-Frame-Options: DENY                  — block embedding in iframes
//       (anti-clickjacking; harmless on JSON responses, hard-blocks
//       embedding the legacy console).
//
//   Referrer-Policy: no-referrer           — don't leak request URLs
//       through cross-origin navigations or fetches.
//
//   Strict-Transport-Security              — HTTPS-only enforcement.
//       Sent unconditionally; HTTP clients ignore. `max-age=31536000`
//       is the OWASP-recommended 1 year. No `preload` until the host
//       is registered with hstspreload.org.
//
//   Content-Security-Policy                — script/style/connect origin
//       restrictions. Default is the safe pragmatic policy that works
//       for the legacy `frontend/` console (uses inline `<style>` but
//       only external `<script src="…">`). Override via env var
//       `API_CSP`; set to empty string to suppress CSP entirely (last
//       resort, e.g. local debugging with a remote tool).
//
// Why these headers and not e.g. `X-XSS-Protection`: that header is
// deprecated and most modern browsers ignore it. CSP supersedes it.
//
// Why CSP allows `'unsafe-inline'` for style: the legacy console at
// `frontend/index.html` embeds a `<style>` block. Tightening this to
// nonces or hashes is a frontend refactor, tracked as future work.
// Scripts use `'self'` only — no `'unsafe-inline'`, no `'unsafe-eval'`.

use std::sync::OnceLock;

use warp::http::header::{HeaderMap, HeaderName, HeaderValue};

const DEFAULT_CSP: &str = "default-src 'self'; \
script-src 'self'; \
style-src 'self' 'unsafe-inline'; \
connect-src 'self' ws: wss: https:; \
img-src 'self' data:; \
font-src 'self' data:; \
object-src 'none'; \
frame-ancestors 'none'; \
base-uri 'none'; \
form-action 'self'";

const HSTS: &str = "max-age=31536000; includeSubDomains";

static HEADERS: OnceLock<HeaderMap> = OnceLock::new();

/// Build (once) and return the static HeaderMap that `routes.with(
/// warp::reply::with::headers(...))` attaches to every response.
pub(crate) fn security_headers_map() -> HeaderMap {
    HEADERS
        .get_or_init(|| build_headers_with_csp(resolved_csp().as_deref()))
        .clone()
}

/// Pure function: tests pass `csp` directly so they can run in parallel
/// without racing on the `API_CSP` env var.
fn build_headers_with_csp(csp: Option<&str>) -> HeaderMap {
    let mut map = HeaderMap::new();
    insert(&mut map, "x-content-type-options", "nosniff");
    insert(&mut map, "x-frame-options", "DENY");
    insert(&mut map, "referrer-policy", "no-referrer");
    insert(&mut map, "strict-transport-security", HSTS);
    if let Some(value) = csp {
        insert(&mut map, "content-security-policy", value);
    }
    map
}

fn insert(map: &mut HeaderMap, name: &'static str, value: &str) {
    if let (Ok(n), Ok(v)) = (
        HeaderName::from_static_checked(name),
        HeaderValue::from_str(value),
    ) {
        map.insert(n, v);
    }
}

/// Read `API_CSP` once at startup. Three cases:
///   unset           → `DEFAULT_CSP`
///   set to ""       → no CSP header (escape hatch)
///   set to <value>  → use as-is (operator-tuned)
fn resolved_csp() -> Option<String> {
    match std::env::var("API_CSP") {
        Ok(v) if v.is_empty() => None,
        Ok(v) => Some(v),
        Err(_) => Some(DEFAULT_CSP.to_string()),
    }
}

// `HeaderName::from_static` panics on invalid input. We only feed it
// hard-coded ASCII-lowercase header names from this module, so a panic
// would be a programming error — but route this through `try_from` so
// a typo surfaces as a missing header rather than a crash at startup.
trait HeaderNameFromStaticChecked {
    fn from_static_checked(s: &'static str) -> Result<HeaderName, ()>;
}
impl HeaderNameFromStaticChecked for HeaderName {
    fn from_static_checked(s: &'static str) -> Result<HeaderName, ()> {
        HeaderName::from_bytes(s.as_bytes()).map_err(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_csp_is_emitted_when_csp_is_default() {
        let map = build_headers_with_csp(Some(DEFAULT_CSP));
        let csp = map.get("content-security-policy").unwrap().to_str().unwrap();
        assert!(csp.contains("default-src 'self'"));
        assert!(csp.contains("frame-ancestors 'none'"));
    }

    #[test]
    fn none_suppresses_csp_header() {
        let map = build_headers_with_csp(None);
        assert!(map.get("content-security-policy").is_none());
    }

    #[test]
    fn custom_csp_is_used_verbatim() {
        let map = build_headers_with_csp(Some("default-src 'self' https://example.com"));
        let csp = map.get("content-security-policy").unwrap().to_str().unwrap();
        assert_eq!(csp, "default-src 'self' https://example.com");
    }

    #[test]
    fn static_headers_are_always_set_regardless_of_csp() {
        let map = build_headers_with_csp(None);
        assert_eq!(
            map.get("x-content-type-options").unwrap().to_str().unwrap(),
            "nosniff"
        );
        assert_eq!(
            map.get("x-frame-options").unwrap().to_str().unwrap(),
            "DENY"
        );
        assert_eq!(
            map.get("referrer-policy").unwrap().to_str().unwrap(),
            "no-referrer"
        );
        let hsts = map
            .get("strict-transport-security")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(hsts.contains("max-age=31536000"));
        assert!(hsts.contains("includeSubDomains"));
    }
}
