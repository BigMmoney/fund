use super::*;

/// Sanitize internal errors before sending to clients.
/// Internal errors may contain file paths, SQL details, or other sensitive
/// implementation details. This maps them to a generic client-safe message
/// while logging the original error at WARN level for operators.
pub(crate) fn sanitize_internal_error(error_msg: &str) -> String {
    // Log the full error for operators
    tracing::warn!(error = error_msg, "internal server error");
    // Return a generic message to the client
    "internal server error — please contact support".to_string()
}

/// Convenience: reject with INTERNAL_SERVER_ERROR using a sanitized message.
pub(crate) fn reject_internal_error(error: impl std::fmt::Display) -> Rejection {
    reject_api(
        StatusCode::INTERNAL_SERVER_ERROR,
        sanitize_internal_error(&error.to_string()),
    )
}

pub(crate) fn normalize_request_id(request_id: Option<String>) -> String {
    request_id
        .filter(|request_id| !request_id.trim().is_empty())
        .unwrap_or_else(|| types::generate_op_id("req"))
}

pub(crate) fn normalize_client_order_id(client_order_id: Option<String>) -> String {
    client_order_id
        .filter(|client_order_id| !client_order_id.trim().is_empty())
        .unwrap_or_else(types::generate_id)
}

pub(crate) fn audit(action: &str, request_id: &str, principal: &AuthenticatedPrincipal) {
    // Use trace! instead of info! to avoid synchronous tracing contention under high concurrency.
    // The default log level is now "warn" so this is suppressed entirely in production.
    if let Err(error) = append_admin_action_audit(action, request_id, principal) {
        tracing::warn!(action, request_id, error = %error, "admin action audit append failed");
    }
    tracing::trace!(
        action = action,
        request_id = request_id,
        subject = %principal.subject,
        role = ?principal.role,
        session_id = ?principal.session_id,
        "audit event"
    );
}

pub(crate) fn update_lifecycle_after_submit(
    sequencer: &Sequencer,
    request_id: &str,
    result: &matching::SubmitOrderResult,
) {
    if let Err(e) = sequencer.mark_risk_reserved(request_id) {
        tracing::warn!(request_id, error = %e, "lifecycle: mark_risk_reserved failed");
    }
    if let Err(e) = sequencer.mark_routed(request_id) {
        tracing::warn!(request_id, error = %e, "lifecycle: mark_routed failed");
    }
    if let Err(e) = sequencer.mark_partition_accepted(request_id) {
        tracing::warn!(request_id, error = %e, "lifecycle: mark_partition_accepted failed");
    }
    if !result.fills.is_empty() {
        if let Err(e) = sequencer.mark_executed(request_id) {
            tracing::warn!(request_id, error = %e, "lifecycle: mark_executed failed");
        }
        if let Err(e) = sequencer.mark_settled(request_id) {
            tracing::warn!(request_id, error = %e, "lifecycle: mark_settled failed");
        }
    }
    if result.state != types::OrderState::Active {
        if let Err(e) = sequencer.mark_completed(request_id) {
            tracing::warn!(request_id, error = %e, "lifecycle: mark_completed failed");
        }
    }
}

pub(crate) fn update_lifecycle_after_cancel(sequencer: &Sequencer, request_id: &str) {
    if let Err(e) = sequencer.mark_routed(request_id) {
        tracing::warn!(request_id, error = %e, "lifecycle cancel: mark_routed failed");
    }
    if let Err(e) = sequencer.mark_executed(request_id) {
        tracing::warn!(request_id, error = %e, "lifecycle cancel: mark_executed failed");
    }
    if let Err(e) = sequencer.mark_completed(request_id) {
        tracing::warn!(request_id, error = %e, "lifecycle cancel: mark_completed failed");
    }
}

pub(crate) fn update_lifecycle_after_admin(sequencer: &Sequencer, request_id: &str) {
    if let Err(e) = sequencer.mark_routed(request_id) {
        tracing::warn!(request_id, error = %e, "lifecycle admin: mark_routed failed");
    }
    if let Err(e) = sequencer.mark_executed(request_id) {
        tracing::warn!(request_id, error = %e, "lifecycle admin: mark_executed failed");
    }
    if let Err(e) = sequencer.mark_completed(request_id) {
        tracing::warn!(request_id, error = %e, "lifecycle admin: mark_completed failed");
    }
}
