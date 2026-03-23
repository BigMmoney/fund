use super::*;

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
    tracing::info!(
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
    let _ = sequencer.mark_routed(request_id);
    let _ = sequencer.mark_executed(request_id);
    let _ = sequencer.mark_completed(request_id);
}

pub(crate) fn update_lifecycle_after_admin(sequencer: &Sequencer, request_id: &str) {
    let _ = sequencer.mark_routed(request_id);
    let _ = sequencer.mark_executed(request_id);
    let _ = sequencer.mark_completed(request_id);
}
