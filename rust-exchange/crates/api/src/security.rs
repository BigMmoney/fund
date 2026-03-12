use super::*;

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

fn internal_auth_payload(
    method: &Method,
    path: &str,
    subject: &str,
    role: &str,
    session_id: &str,
    timestamp: i64,
    request_id: &str,
) -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}",
        method.as_str(),
        path,
        subject,
        role,
        session_id,
        timestamp,
        request_id
    )
}

fn verify_internal_principal(
    method: Method,
    path: String,
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

fn verify_optional_internal_principal(
    method: Method,
    path: String,
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
        method, path, subject, role, session_id, timestamp, signature, request_id,
    )
    .map(Some)
}

pub(crate) fn with_principal(
) -> impl Filter<Extract = (AuthenticatedPrincipal,), Error = Rejection> + Clone {
    warp::method()
        .and(warp::path::full())
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
             subject: Option<String>,
             role: Option<String>,
             session_id: Option<String>,
             timestamp: Option<String>,
             signature: Option<String>,
             request_id: Option<String>| async move {
                verify_internal_principal(
                    method,
                    path.as_str().to_string(),
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
             subject: Option<String>,
             role: Option<String>,
             session_id: Option<String>,
             timestamp: Option<String>,
             signature: Option<String>,
             request_id: Option<String>| async move {
                verify_optional_internal_principal(
                    method,
                    path.as_str().to_string(),
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
            Some("user-1".to_string()),
            Some("user".to_string()),
            Some("session-1".to_string()),
            Some(timestamp.to_string()),
            Some(signature),
            Some("req-1".to_string()),
        );
        assert!(result.is_err());
    }
}
