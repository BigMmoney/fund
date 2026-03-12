use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use types::Event;

const CHANNEL_CAPACITY: usize = 1000;

#[derive(Clone)]
pub struct EventBus {
    channels: Arc<RwLock<HashMap<String, broadcast::Sender<Event>>>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn publish(&self, event: Event) {
        let event_type = Self::event_type_name(&event);
        let channels = self.channels.read();

        if let Some(sender) = channels.get(&event_type) {
            let _ = sender.send(event);
        }
    }

    pub fn subscribe(&self, event_type: &str) -> broadcast::Receiver<Event> {
        let mut channels = self.channels.write();

        let sender = channels
            .entry(event_type.to_string())
            .or_insert_with(|| broadcast::channel(CHANNEL_CAPACITY).0);

        sender.subscribe()
    }

    fn event_type_name(event: &Event) -> String {
        match event {
            Event::IntentReceived(_) => "intent.received",
            Event::IntentCancelled(_) => "intent.cancelled",
            Event::FillCreated(_) => "fill.created",
            Event::LedgerCommitted(_) => "ledger.committed",
            Event::LedgerRejected { .. } => "ledger.rejected",
        }
        .to_string()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
