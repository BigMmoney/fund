use chrono::{DateTime, Utc};
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use persistence::{InMemoryWal, WalStore};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;
use types::{
    Command, CommandLifecycle, CommandMetadata, OrderTraceEvent, OrderTraceStage, TraceEmitter,
};

/// Controls how sequence gaps are handled during WAL recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequencerRecoveryPolicy {
    /// Abort recovery on any gap (default, strict).
    Strict,
    /// Log gaps as critical warnings and continue from max_seq + 1.
    AllowGaps,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequencedCommandRecord {
    pub request_id: String,
    pub command_seq: u64,
    pub command: Command,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SequencerError {
    #[error("invalid request_id: empty")]
    InvalidRequestId,
    #[error("duplicate request_id: {request_id}")]
    DuplicateRequest {
        request_id: String,
        existing_seq: Option<u64>,
    },
    #[error("unknown request_id: {0}")]
    UnknownRequestId(String),
    #[error("unsupported command update for request_id: {0}")]
    UnsupportedCommandUpdate(String),
    #[error("wal append failed: {0}")]
    WalAppendFailed(String),
    #[error("wal read failed: {0}")]
    WalReadFailed(String),
    #[error("invalid lifecycle transition for {request_id}: {from:?} -> {to:?}")]
    InvalidLifecycleTransition {
        request_id: String,
        from: CommandLifecycle,
        to: CommandLifecycle,
    },
    #[error("sequence gap detected during recovery: {gap_count} missing entries")]
    RecoveryGap { gap_count: usize },
}

pub struct Sequencer {
    next_seq: AtomicU64,
    record_by_request: DashMap<String, SequencedCommandRecord>,
    wal_store: Arc<dyn WalStore<SequencedCommandRecord>>,
    /// Observer-only sink. When `Some`, the sequencer emits
    /// `sequencer_accepted` + `sequencer_persisted` on the success path of
    /// `sequence_and_append`, and `sequencer_accepted` from `sequence`.
    /// Failures (WAL append failure, duplicate request) emit nothing.
    /// See `docs/MONITOR_DESIGN.md`.
    trace_emitter: Option<Arc<dyn TraceEmitter>>,
}

impl std::fmt::Debug for Sequencer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sequencer")
            .field("next_seq", &self.next_seq.load(Ordering::SeqCst))
            .field("record_by_request_len", &self.record_by_request.len())
            .finish_non_exhaustive()
    }
}

impl Sequencer {
    pub fn new(start_seq: u64) -> Self {
        Self::with_wal(start_seq, Arc::new(InMemoryWal::new()))
    }

    pub fn with_wal(start_seq: u64, wal_store: Arc<dyn WalStore<SequencedCommandRecord>>) -> Self {
        Self::with_wal_and_emitter(start_seq, wal_store, None)
    }

    /// Construct a Sequencer with an optional `TraceEmitter` for the order
    /// flow monitor. When `trace_emitter` is `Some`, the sequencer emits
    /// trace events on the success path of `sequence` and
    /// `sequence_and_append`. Failures emit nothing.
    pub fn with_wal_and_emitter(
        start_seq: u64,
        wal_store: Arc<dyn WalStore<SequencedCommandRecord>>,
        trace_emitter: Option<Arc<dyn TraceEmitter>>,
    ) -> Self {
        Self {
            next_seq: AtomicU64::new(start_seq),
            record_by_request: DashMap::new(),
            wal_store,
            trace_emitter,
        }
    }

    fn maybe_emit(&self, event: OrderTraceEvent) {
        if let Some(emitter) = &self.trace_emitter {
            emitter.emit(event);
        }
    }

    pub fn recover_from_wal(&self) -> Result<usize, SequencerError> {
        self.recover_from_wal_with_policy(SequencerRecoveryPolicy::Strict)
    }

    pub fn recover_from_wal_with_policy(
        &self,
        policy: SequencerRecoveryPolicy,
    ) -> Result<usize, SequencerError> {
        let records = self
            .wal_store
            .entries()
            .map_err(|error| SequencerError::WalReadFailed(error.to_string()))?;

        self.record_by_request.clear();

        let mut max_seq = 0u64;
        let mut seen_seqs = std::collections::BTreeSet::new();
        for record in &records {
            self.record_by_request
                .insert(record.request_id.clone(), record.clone());
            if record.command_seq > 0 {
                seen_seqs.insert(record.command_seq);
            }
            max_seq = max_seq.max(record.command_seq);
        }

        // Detect sequence gaps and log warnings.
        if !seen_seqs.is_empty() {
            let min_seq = *seen_seqs.iter().next().unwrap();
            let expected_count = (max_seq - min_seq + 1) as usize;
            if seen_seqs.len() < expected_count {
                let mut gaps = Vec::new();
                for seq in min_seq..=max_seq {
                    if !seen_seqs.contains(&seq) {
                        gaps.push(seq);
                        if gaps.len() >= 20 {
                            break; // limit log output
                        }
                    }
                }
                tracing::error!(
                    gap_count = expected_count - seen_seqs.len(),
                    first_gaps = ?gaps,
                    "sequence gap detected during WAL recovery"
                );
                if policy == SequencerRecoveryPolicy::Strict {
                    return Err(SequencerError::RecoveryGap {
                        gap_count: expected_count - seen_seqs.len(),
                    });
                }
                tracing::warn!(
                    gap_count = expected_count - seen_seqs.len(),
                    "AllowGaps policy: continuing recovery despite sequence gaps"
                );
            }
        }

        let next_seq = if records.is_empty() {
            self.next_seq.load(Ordering::SeqCst)
        } else {
            max_seq + 1
        };
        self.next_seq.store(next_seq, Ordering::SeqCst);

        Ok(records.len())
    }

    pub fn sequence(&self, mut command: Command) -> Result<Command, SequencerError> {
        let request_id = command.request_id().trim().to_string();
        if request_id.is_empty() {
            return Err(SequencerError::InvalidRequestId);
        }

        // Atomic check-and-insert: claim the slot first, then assign sequence number
        // under the same shard lock to prevent TOCTOU races.
        match self.record_by_request.entry(request_id.clone()) {
            Entry::Occupied(entry) => Err(SequencerError::DuplicateRequest {
                request_id,
                existing_seq: Some(entry.get().command_seq),
            }),
            Entry::Vacant(entry) => {
                let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
                let metadata = command.metadata_mut();
                metadata.command_seq = Some(seq);
                metadata.advance(CommandLifecycle::Sequenced);

                let record = SequencedCommandRecord {
                    request_id: request_id.clone(),
                    command_seq: seq,
                    command: command.clone(),
                    recorded_at: Utc::now(),
                };
                entry.insert(record);

                // Observer: emit `sequencer_accepted`. No `sequencer_persisted`
                // here — this path does not touch a durable WAL.
                if let Some(ev) = order_trace_for(
                    &command,
                    OrderTraceStage::SequencerAccepted,
                    seq,
                    CommandLifecycle::Sequenced,
                ) {
                    self.maybe_emit(ev);
                }

                Ok(command)
            }
        }
    }

    pub fn sequence_and_append(&self, mut command: Command) -> Result<Command, SequencerError> {
        let request_id = command.request_id().trim().to_string();
        if request_id.is_empty() {
            return Err(SequencerError::InvalidRequestId);
        }

        // Atomic check-and-insert: claim the slot first, then assign sequence number
        // and WAL-append under the same shard lock to prevent TOCTOU races.
        match self.record_by_request.entry(request_id.clone()) {
            Entry::Occupied(entry) => Err(SequencerError::DuplicateRequest {
                request_id,
                existing_seq: Some(entry.get().command_seq),
            }),
            Entry::Vacant(entry) => {
                let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
                let metadata = command.metadata_mut();
                metadata.command_seq = Some(seq);
                metadata.advance(CommandLifecycle::Sequenced);
                metadata.advance(CommandLifecycle::WalAppended);

                let record = SequencedCommandRecord {
                    request_id: request_id.clone(),
                    command_seq: seq,
                    command: command.clone(),
                    recorded_at: Utc::now(),
                };

                self.wal_store.append(&record).map_err(|e| {
                    // Rollback sequence number on WAL failure
                    self.next_seq.fetch_sub(1, Ordering::SeqCst);
                    SequencerError::WalAppendFailed(e.to_string())
                })?;

                entry.insert(record);

                // Observer: emit `sequencer_accepted` then `sequencer_persisted`
                // only after the success path commits. WAL failure rolls back
                // the seq and emits nothing — the api layer will emit
                // `api_rejected` for the caller.
                if let Some(ev) = order_trace_for(
                    &command,
                    OrderTraceStage::SequencerAccepted,
                    seq,
                    CommandLifecycle::Sequenced,
                ) {
                    self.maybe_emit(ev);
                }
                if let Some(ev) = order_trace_for(
                    &command,
                    OrderTraceStage::SequencerPersisted,
                    seq,
                    CommandLifecycle::WalAppended,
                ) {
                    self.maybe_emit(ev);
                }

                Ok(command)
            }
        }
    }

    pub fn wal_entries(&self) -> Result<Vec<SequencedCommandRecord>, SequencerError> {
        self.wal_store
            .entries()
            .map_err(|error| SequencerError::WalReadFailed(error.to_string()))
    }

    pub fn latest_records(&self) -> Vec<SequencedCommandRecord> {
        let mut records: Vec<_> = self
            .record_by_request
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        records.sort_by_key(|record| record.command_seq);
        records
    }

    pub fn metadata(&self, request_id: &str) -> Option<CommandMetadata> {
        self.record_by_request
            .get(request_id)
            .map(|record| record.command.metadata().clone())
    }

    pub fn command(&self, request_id: &str) -> Option<Command> {
        self.record_by_request
            .get(request_id)
            .map(|record| record.command.clone())
    }

    pub fn mark_wal_appended(&self, request_id: &str) -> Result<CommandMetadata, SequencerError> {
        self.advance_lifecycle(request_id, CommandLifecycle::WalAppended)
    }

    pub fn mark_risk_reserved(&self, request_id: &str) -> Result<CommandMetadata, SequencerError> {
        self.advance_lifecycle(request_id, CommandLifecycle::RiskReserved)
    }

    pub fn mark_routed(&self, request_id: &str) -> Result<CommandMetadata, SequencerError> {
        self.advance_lifecycle(request_id, CommandLifecycle::Routed)
    }

    pub fn mark_partition_accepted(
        &self,
        request_id: &str,
    ) -> Result<CommandMetadata, SequencerError> {
        self.advance_lifecycle(request_id, CommandLifecycle::PartitionAccepted)
    }

    pub fn mark_executed(&self, request_id: &str) -> Result<CommandMetadata, SequencerError> {
        self.advance_lifecycle(request_id, CommandLifecycle::Executed)
    }

    pub fn mark_settled(&self, request_id: &str) -> Result<CommandMetadata, SequencerError> {
        self.advance_lifecycle(request_id, CommandLifecycle::Settled)
    }

    pub fn mark_completed(&self, request_id: &str) -> Result<CommandMetadata, SequencerError> {
        self.advance_lifecycle(request_id, CommandLifecycle::Completed)
    }

    pub fn mark_cancelled(&self, request_id: &str) -> Result<CommandMetadata, SequencerError> {
        self.advance_lifecycle(request_id, CommandLifecycle::Cancelled)
    }

    pub fn mark_rejected(&self, request_id: &str) -> Result<CommandMetadata, SequencerError> {
        self.advance_lifecycle(request_id, CommandLifecycle::Rejected)
    }

    pub fn record_generated_replace_order_id(
        &self,
        request_id: &str,
        generated_order_id: &str,
    ) -> Result<CommandMetadata, SequencerError> {
        let mut record = self
            .record_by_request
            .get_mut(request_id)
            .ok_or_else(|| SequencerError::UnknownRequestId(request_id.to_string()))?;

        let Command::ReplaceOrder(command) = record.command.clone() else {
            return Err(SequencerError::UnsupportedCommandUpdate(
                request_id.to_string(),
            ));
        };

        if command.new_client_order_id.as_deref() == Some(generated_order_id) {
            return Ok(command.metadata.clone());
        }

        match record.command.metadata().lifecycle {
            CommandLifecycle::Completed
            | CommandLifecycle::Cancelled
            | CommandLifecycle::Rejected => {
                return Err(SequencerError::InvalidLifecycleTransition {
                    request_id: request_id.to_string(),
                    from: record.command.metadata().lifecycle,
                    to: record.command.metadata().lifecycle,
                });
            }
            _ => {}
        }

        let Command::ReplaceOrder(command) = &mut record.command else {
            return Err(SequencerError::UnsupportedCommandUpdate(
                request_id.to_string(),
            ));
        };
        command.new_client_order_id = Some(generated_order_id.to_string());
        command.metadata.updated_at = Utc::now();
        record.recorded_at = Utc::now();
        let updated_record = record.clone();
        drop(record);

        self.wal_store
            .append(&updated_record)
            .map_err(|error| SequencerError::WalAppendFailed(error.to_string()))?;

        Ok(updated_record.command.metadata().clone())
    }

    pub fn advance_lifecycle(
        &self,
        request_id: &str,
        next: CommandLifecycle,
    ) -> Result<CommandMetadata, SequencerError> {
        let mut record = self
            .record_by_request
            .get_mut(request_id)
            .ok_or_else(|| SequencerError::UnknownRequestId(request_id.to_string()))?;

        let current = record.command.metadata().lifecycle;
        if !is_valid_transition(current, next) {
            return Err(SequencerError::InvalidLifecycleTransition {
                request_id: request_id.to_string(),
                from: current,
                to: next,
            });
        }

        record.command.metadata_mut().advance(next);
        record.recorded_at = Utc::now();
        let updated_record = record.clone();
        drop(record);

        self.wal_store
            .append(&updated_record)
            .map_err(|error| SequencerError::WalAppendFailed(error.to_string()))?;

        Ok(updated_record.command.metadata().clone())
    }
}

impl Default for Sequencer {
    fn default() -> Self {
        Self::new(1)
    }
}

/// Build an `OrderTraceEvent` for a sequencer-emitted stage. Returns
/// `None` for command kinds that do not correspond to a single tracked
/// order (mass-cancel, admin) — those are not on the order-flow timeline
/// the monitor visualizes.
///
/// For `NewOrderCommand` the event is constructed via
/// [`OrderTraceEvent::new_unbound`]: the sequencer assigns a
/// `command_seq` but not a canonical `order_id` — the projector will
/// bind this event to the eventual order via `request_id` once a
/// downstream stage (matching) emits with both fields populated. See
/// `docs/MONITOR_DESIGN.md` §3.3.1.
fn order_trace_for(
    command: &Command,
    stage: OrderTraceStage,
    command_seq: u64,
    lifecycle: CommandLifecycle,
) -> Option<OrderTraceEvent> {
    match command {
        Command::NewOrder(c) => {
            let mut ev = OrderTraceEvent::new_unbound(stage);
            ev.client_order_id = Some(c.client_order_id.clone());
            ev.user_id = Some(c.user_id.clone());
            ev.session_id = c.session_id.clone();
            ev.request_id = Some(c.metadata.request_id.clone());
            ev.command_seq = Some(command_seq);
            ev.market_id = Some(c.market_id.clone());
            ev.outcome = Some(c.outcome);
            ev.side = Some(c.side);
            ev.price = c.price;
            ev.amount = Some(c.amount);
            ev.lifecycle = Some(lifecycle);
            Some(ev)
        }
        Command::CancelOrder(c) => {
            let mut ev = OrderTraceEvent::new(stage, c.order_id.clone());
            ev.client_order_id = c.client_order_id.clone();
            ev.user_id = Some(c.user_id.clone());
            ev.request_id = Some(c.metadata.request_id.clone());
            ev.command_seq = Some(command_seq);
            ev.market_id = Some(c.market_id.clone());
            ev.outcome = c.outcome;
            ev.lifecycle = Some(lifecycle);
            Some(ev)
        }
        Command::ReplaceOrder(c) => {
            let mut ev = OrderTraceEvent::new(stage, c.order_id.clone());
            ev.client_order_id = c.new_client_order_id.clone();
            ev.user_id = Some(c.user_id.clone());
            ev.request_id = Some(c.metadata.request_id.clone());
            ev.command_seq = Some(command_seq);
            ev.market_id = Some(c.market_id.clone());
            ev.outcome = c.outcome;
            ev.lifecycle = Some(lifecycle);
            Some(ev)
        }
        // Mass-cancel and admin commands are not single-order events.
        Command::MassCancelByUser(_)
        | Command::MassCancelBySession(_)
        | Command::MassCancelByMarket(_)
        | Command::Admin(_) => None,
    }
}

fn is_valid_transition(current: CommandLifecycle, next: CommandLifecycle) -> bool {
    if current == next {
        return true;
    }

    match current {
        CommandLifecycle::Received => matches!(next, CommandLifecycle::Sequenced),
        CommandLifecycle::Sequenced => matches!(
            next,
            CommandLifecycle::WalAppended
                | CommandLifecycle::Rejected
                | CommandLifecycle::Cancelled
        ),
        CommandLifecycle::WalAppended => matches!(
            next,
            CommandLifecycle::RiskReserved
                | CommandLifecycle::Routed
                | CommandLifecycle::Rejected
                | CommandLifecycle::Cancelled
        ),
        CommandLifecycle::RiskReserved => matches!(
            next,
            CommandLifecycle::Routed | CommandLifecycle::Rejected | CommandLifecycle::Cancelled
        ),
        CommandLifecycle::Routed => matches!(
            next,
            CommandLifecycle::PartitionAccepted
                | CommandLifecycle::Rejected
                | CommandLifecycle::Cancelled
        ),
        CommandLifecycle::PartitionAccepted => matches!(
            next,
            CommandLifecycle::Executed
                | CommandLifecycle::Completed
                | CommandLifecycle::Rejected
                | CommandLifecycle::Cancelled
        ),
        CommandLifecycle::Executed => matches!(
            next,
            CommandLifecycle::Settled | CommandLifecycle::Completed
        ),
        CommandLifecycle::Settled => matches!(next, CommandLifecycle::Completed),
        CommandLifecycle::Completed | CommandLifecycle::Cancelled | CommandLifecycle::Rejected => {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use persistence::InMemoryWal;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use types::{
        CancelOrderCommand, CommandMetadata, MassCancelByUserCommand, NewOrderCommand, OrderType,
        ReplaceOrderCommand, Side, TimeInForce,
    };

    fn new_order_command(request_id: &str, client_order_id: &str) -> Command {
        Command::NewOrder(NewOrderCommand {
            metadata: CommandMetadata::new(request_id),
            client_order_id: client_order_id.to_string(),
            user_id: "user-1".to_string(),
            session_id: Some("session-1".to_string()),
            market_id: "btc-usdt".to_string(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
            price: Some(100),
            amount: 10,
            outcome: 0,
            post_only: false,
            reduce_only: false,
            leverage: None,
            expires_at: None,
            stp_mode: types::StpMode::default(),
            trigger_price: None,
            trigger_type: None,
            display_qty: None,
            min_fill_qty: None,
            stp_group_id: None,
            is_market_maker: false,
        })
    }

    fn replace_order_command(request_id: &str, order_id: &str) -> Command {
        Command::ReplaceOrder(ReplaceOrderCommand {
            metadata: CommandMetadata::new(request_id),
            user_id: "user-1".to_string(),
            market_id: "btc-usdt".to_string(),
            outcome: Some(0),
            order_id: order_id.to_string(),
            new_client_order_id: None,
            new_price: Some(101),
            new_amount: Some(9),
            new_time_in_force: None,
            post_only: None,
            reduce_only: None,
            new_leverage: None,
            new_expires_at: None,
            new_display_qty: None,
            new_min_fill_qty: None,
            new_trigger_price: None,
            new_trigger_type: None,
        })
    }

    #[test]
    fn sequence_assigns_monotonic_sequences() {
        let sequencer = Sequencer::default();

        let first = sequencer
            .sequence(new_order_command("req-1", "coid-1"))
            .unwrap();
        let second = sequencer
            .sequence(new_order_command("req-2", "coid-2"))
            .unwrap();

        assert_eq!(first.metadata().command_seq, Some(1));
        assert_eq!(second.metadata().command_seq, Some(2));
        assert_eq!(first.metadata().lifecycle, CommandLifecycle::Sequenced);
        assert_eq!(second.metadata().lifecycle, CommandLifecycle::Sequenced);
    }

    #[test]
    fn sequence_and_append_persists_wal_record() {
        let sequencer = Sequencer::default();
        let command = sequencer
            .sequence_and_append(new_order_command("wal-req", "coid-1"))
            .unwrap();

        assert_eq!(command.metadata().lifecycle, CommandLifecycle::WalAppended);

        let wal_entries = sequencer.wal_entries().unwrap();
        assert_eq!(wal_entries.len(), 1);
        assert_eq!(wal_entries[0].request_id, "wal-req");
        assert_eq!(wal_entries[0].command_seq, 1);
        assert_eq!(
            wal_entries[0].command.metadata().lifecycle,
            CommandLifecycle::WalAppended
        );
    }

    #[test]
    fn recover_from_wal_restores_latest_metadata_and_next_seq() {
        let wal = Arc::new(InMemoryWal::<SequencedCommandRecord>::new());
        let sequencer = Sequencer::with_wal(1, wal.clone());
        sequencer
            .sequence_and_append(new_order_command("recover-1", "coid-1"))
            .unwrap();
        sequencer.mark_routed("recover-1").unwrap();

        let recovered = Sequencer::with_wal(1, wal);
        assert_eq!(recovered.recover_from_wal().unwrap(), 2);
        assert_eq!(
            recovered.metadata("recover-1").unwrap().lifecycle,
            CommandLifecycle::Routed
        );

        let next = recovered
            .sequence(new_order_command("recover-2", "coid-2"))
            .unwrap();
        assert_eq!(next.metadata().command_seq, Some(2));
    }

    #[test]
    fn duplicate_request_id_is_rejected() {
        let sequencer = Sequencer::default();
        sequencer
            .sequence(new_order_command("dup-req", "coid-1"))
            .unwrap();

        let err = sequencer
            .sequence(new_order_command("dup-req", "coid-2"))
            .unwrap_err();

        assert_eq!(
            err,
            SequencerError::DuplicateRequest {
                request_id: "dup-req".to_string(),
                existing_seq: Some(1),
            }
        );
    }

    #[test]
    fn lifecycle_advances_in_valid_order_and_is_durable() {
        let wal = Arc::new(InMemoryWal::<SequencedCommandRecord>::new());
        let sequencer = Sequencer::with_wal(1, wal.clone());
        sequencer
            .sequence_and_append(new_order_command("flow-req", "coid-1"))
            .unwrap();

        sequencer.mark_risk_reserved("flow-req").unwrap();
        sequencer.mark_routed("flow-req").unwrap();
        sequencer.mark_partition_accepted("flow-req").unwrap();
        sequencer.mark_executed("flow-req").unwrap();
        sequencer.mark_settled("flow-req").unwrap();
        let metadata = sequencer.mark_completed("flow-req").unwrap();

        assert_eq!(metadata.lifecycle, CommandLifecycle::Completed);
        assert_eq!(metadata.command_seq, Some(1));

        let recovered = Sequencer::with_wal(1, wal);
        recovered.recover_from_wal().unwrap();
        assert_eq!(
            recovered.metadata("flow-req").unwrap().lifecycle,
            CommandLifecycle::Completed
        );
    }

    #[test]
    fn invalid_transition_is_rejected() {
        let sequencer = Sequencer::default();
        sequencer
            .sequence(new_order_command("bad-flow", "coid-1"))
            .unwrap();

        let err = sequencer.mark_completed("bad-flow").unwrap_err();
        assert_eq!(
            err,
            SequencerError::InvalidLifecycleTransition {
                request_id: "bad-flow".to_string(),
                from: CommandLifecycle::Sequenced,
                to: CommandLifecycle::Completed,
            }
        );

        assert_eq!(
            sequencer.metadata("bad-flow").unwrap().lifecycle,
            CommandLifecycle::Sequenced
        );
    }

    #[test]
    fn concurrent_duplicate_request_id_only_sequences_once() {
        let sequencer = Arc::new(Sequencer::default());
        let barrier = Arc::new(Barrier::new(8));

        let handles: Vec<_> = (0..8)
            .map(|_| {
                let sequencer = sequencer.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    barrier.wait();
                    sequencer
                        .sequence(Command::MassCancelByUser(MassCancelByUserCommand {
                            metadata: CommandMetadata::new("shared-request"),
                            user_id: "user-1".to_string(),
                        }))
                        .is_ok()
                })
            })
            .collect();

        let successes = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|ok| *ok)
            .count();

        assert_eq!(successes, 1);
        assert_eq!(
            sequencer.metadata("shared-request").unwrap().command_seq,
            Some(1)
        );
    }

    #[test]
    fn latest_records_are_sorted_by_command_seq() {
        let sequencer = Sequencer::default();
        sequencer
            .sequence(Command::CancelOrder(CancelOrderCommand {
                metadata: CommandMetadata::new("req-2"),
                user_id: "u-1".to_string(),
                market_id: "m-1".to_string(),
                outcome: Some(0),
                order_id: "o-2".to_string(),
                client_order_id: None,
            }))
            .unwrap();
        sequencer
            .sequence(new_order_command("req-1", "o-1"))
            .unwrap();

        let ordered: Vec<_> = sequencer
            .latest_records()
            .into_iter()
            .map(|record| record.request_id)
            .collect();

        assert_eq!(ordered, vec!["req-2".to_string(), "req-1".to_string()]);
    }

    #[test]
    fn allow_gaps_policy_continues_despite_sequence_gap() {
        let wal = Arc::new(InMemoryWal::<SequencedCommandRecord>::new());
        // Manually append records with a gap: seq 1, 2, 4 (missing 3)
        let make_record = |seq: u64, req: &str| SequencedCommandRecord {
            request_id: req.to_string(),
            command_seq: seq,
            command: new_order_command(req, &format!("o-{seq}")),
            recorded_at: Utc::now(),
        };
        wal.append(&make_record(1, "r1")).unwrap();
        wal.append(&make_record(2, "r2")).unwrap();
        wal.append(&make_record(4, "r4")).unwrap();

        // Strict should fail.
        let seq_strict = Sequencer::with_wal(1, wal.clone());
        assert!(matches!(
            seq_strict.recover_from_wal(),
            Err(SequencerError::RecoveryGap { gap_count: 1 })
        ));

        // AllowGaps should succeed and set next_seq = 5.
        let seq_lenient = Sequencer::with_wal(1, wal);
        let count = seq_lenient
            .recover_from_wal_with_policy(SequencerRecoveryPolicy::AllowGaps)
            .unwrap();
        assert_eq!(count, 3);
        // next_seq should be max_seq + 1 = 5
        assert_eq!(seq_lenient.next_seq.load(Ordering::SeqCst), 5);
    }

    #[test]
    fn generated_replace_order_id_is_persisted_for_replay() {
        let wal = Arc::new(InMemoryWal::<SequencedCommandRecord>::new());
        let sequencer = Sequencer::with_wal(1, wal.clone());
        sequencer
            .sequence_and_append(replace_order_command("replace-flow", "old-1"))
            .unwrap();

        sequencer
            .record_generated_replace_order_id("replace-flow", "generated-1")
            .unwrap();

        let recovered = Sequencer::with_wal(1, wal);
        recovered.recover_from_wal().unwrap();
        let command = recovered.command("replace-flow").unwrap();
        let Command::ReplaceOrder(command) = command else {
            panic!("expected replace command");
        };
        assert_eq!(command.new_client_order_id.as_deref(), Some("generated-1"));
    }

    // ── Order Flow Monitor: trace event emission ─────────────────────────

    /// Record-and-replay sink for sequencer trace events. Used in tests to
    /// assert the sequencer emits the expected `OrderTraceEvent`s in the
    /// expected order.
    #[derive(Default)]
    struct RecordingEmitter {
        events: parking_lot::Mutex<Vec<OrderTraceEvent>>,
    }
    impl TraceEmitter for RecordingEmitter {
        fn emit(&self, event: OrderTraceEvent) {
            self.events.lock().push(event);
        }
    }

    fn new_seq_with_emitter() -> (Sequencer, Arc<RecordingEmitter>) {
        let emitter: Arc<RecordingEmitter> = Arc::new(RecordingEmitter::default());
        let trace: Arc<dyn TraceEmitter> = emitter.clone();
        let seq = Sequencer::with_wal_and_emitter(1, Arc::new(InMemoryWal::new()), Some(trace));
        (seq, emitter)
    }

    #[test]
    fn sequence_and_append_emits_accepted_then_persisted_for_new_order() {
        let (seq, emitter) = new_seq_with_emitter();
        seq.sequence_and_append(new_order_command("req-1", "cli-1"))
            .unwrap();

        let evs = emitter.events.lock().clone();
        assert_eq!(evs.len(), 2, "expected accepted + persisted, got {}", evs.len());
        assert_eq!(evs[0].stage, OrderTraceStage::SequencerAccepted);
        assert_eq!(evs[1].stage, OrderTraceStage::SequencerPersisted);

        // New order: order_id is None at sequencer time (assigned by matching
        // later); request_id and client_order_id carry the correlation.
        for ev in &evs {
            assert!(ev.order_id.is_none());
            assert_eq!(ev.request_id.as_deref(), Some("req-1"));
            assert_eq!(ev.client_order_id.as_deref(), Some("cli-1"));
            assert_eq!(ev.command_seq, Some(1));
            assert_eq!(ev.user_id.as_deref(), Some("user-1"));
            assert_eq!(ev.market_id.as_deref(), Some("btc-usdt"));
            assert_eq!(ev.amount, Some(10));
            assert_eq!(ev.price, Some(100));
        }
        assert_eq!(evs[0].lifecycle, Some(CommandLifecycle::Sequenced));
        assert_eq!(evs[1].lifecycle, Some(CommandLifecycle::WalAppended));
    }

    #[test]
    fn sequence_and_append_emits_with_order_id_for_cancel() {
        let (seq, emitter) = new_seq_with_emitter();
        let cancel = Command::CancelOrder(CancelOrderCommand {
            metadata: CommandMetadata::new("req-cxl"),
            user_id: "user-1".into(),
            market_id: "btc-usdt".into(),
            outcome: Some(0),
            order_id: "ord-target".into(),
            client_order_id: Some("cli-cxl".into()),
        });
        seq.sequence_and_append(cancel).unwrap();

        let evs = emitter.events.lock().clone();
        assert_eq!(evs.len(), 2);
        for ev in &evs {
            assert_eq!(ev.order_id.as_deref(), Some("ord-target"));
            assert_eq!(ev.request_id.as_deref(), Some("req-cxl"));
        }
    }

    #[test]
    fn sequence_emits_accepted_only_no_persisted() {
        let (seq, emitter) = new_seq_with_emitter();
        seq.sequence(new_order_command("req-x", "cli-x")).unwrap();

        let evs = emitter.events.lock().clone();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].stage, OrderTraceStage::SequencerAccepted);
    }

    #[test]
    fn duplicate_request_emits_nothing() {
        let (seq, emitter) = new_seq_with_emitter();
        seq.sequence_and_append(new_order_command("req-dup", "cli"))
            .unwrap();
        emitter.events.lock().clear();

        let _err = seq.sequence_and_append(new_order_command("req-dup", "cli"));
        assert!(
            emitter.events.lock().is_empty(),
            "duplicate request must emit nothing"
        );
    }

    #[test]
    fn mass_cancel_emits_nothing() {
        let (seq, emitter) = new_seq_with_emitter();
        let mass = Command::MassCancelByUser(MassCancelByUserCommand {
            metadata: CommandMetadata::new("req-mass"),
            user_id: "user-1".into(),
        });
        seq.sequence_and_append(mass).unwrap();
        assert!(
            emitter.events.lock().is_empty(),
            "mass-cancel is not a single-order trace; emitter must not fire"
        );
    }

    #[test]
    fn sequencer_without_emitter_does_not_panic() {
        // Default constructors set trace_emitter = None.
        let seq = Sequencer::new(1);
        seq.sequence_and_append(new_order_command("req-noemit", "cli"))
            .unwrap();
        // No assertion needed — test passes if no panic.
    }
}
