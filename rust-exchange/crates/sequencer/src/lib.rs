use chrono::{DateTime, Utc};
use dashmap::{mapref::entry::Entry, DashMap};
use parking_lot::Mutex;
use persistence::{InMemoryWal, WalStore};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;
use types::{Command, CommandLifecycle, CommandMetadata};

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
}

pub struct Sequencer {
    next_seq: AtomicU64,
    record_by_request: DashMap<String, SequencedCommandRecord>,
    wal_store: Arc<dyn WalStore<SequencedCommandRecord>>,
    write_lock: Mutex<()>,
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
        Self {
            next_seq: AtomicU64::new(start_seq),
            record_by_request: DashMap::new(),
            wal_store,
            write_lock: Mutex::new(()),
        }
    }

    pub fn recover_from_wal(&self) -> Result<usize, SequencerError> {
        let _guard = self.write_lock.lock();
        let records = self
            .wal_store
            .entries()
            .map_err(|error| SequencerError::WalReadFailed(error.to_string()))?;

        self.record_by_request.clear();

        let mut max_seq = 0u64;
        for record in &records {
            self.record_by_request
                .insert(record.request_id.clone(), record.clone());
            max_seq = max_seq.max(record.command_seq);
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
        let _guard = self.write_lock.lock();
        self.sequence_internal(&mut command, false)?;
        Ok(command)
    }

    pub fn sequence_and_append(&self, mut command: Command) -> Result<Command, SequencerError> {
        let _guard = self.write_lock.lock();
        self.sequence_internal(&mut command, true)?;
        Ok(command)
    }

    fn sequence_internal(
        &self,
        command: &mut Command,
        append_wal: bool,
    ) -> Result<(), SequencerError> {
        let request_id = command.request_id().trim().to_string();
        if request_id.is_empty() {
            return Err(SequencerError::InvalidRequestId);
        }

        match self.record_by_request.entry(request_id.clone()) {
            Entry::Occupied(entry) => Err(SequencerError::DuplicateRequest {
                request_id,
                existing_seq: entry.get().command.metadata().command_seq,
            }),
            Entry::Vacant(entry) => {
                let seq = self.next_seq.fetch_add(1, Ordering::SeqCst);
                {
                    let metadata = command.metadata_mut();
                    metadata.command_seq = Some(seq);
                    metadata.advance(CommandLifecycle::Sequenced);
                    if append_wal {
                        metadata.advance(CommandLifecycle::WalAppended);
                    }
                }

                let record = SequencedCommandRecord {
                    request_id: request_id.clone(),
                    command_seq: seq,
                    command: command.clone(),
                    recorded_at: Utc::now(),
                };

                if append_wal {
                    self.wal_store
                        .append(&record)
                        .map_err(|error| SequencerError::WalAppendFailed(error.to_string()))?;
                }

                entry.insert(record);
                Ok(())
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

    pub fn advance_lifecycle(
        &self,
        request_id: &str,
        next: CommandLifecycle,
    ) -> Result<CommandMetadata, SequencerError> {
        let _guard = self.write_lock.lock();
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
        Side, TimeInForce,
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
}
