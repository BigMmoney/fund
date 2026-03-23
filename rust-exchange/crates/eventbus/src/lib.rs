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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use types::{Intent, IntentStatus, Side};

    fn dummy_intent() -> Intent {
        Intent {
            id: "i-1".into(),
            user_id: "u-1".into(),
            market_id: "m-1".into(),
            side: Side::Buy,
            price: 100,
            amount: 10,
            outcome: 1,
            created_at: Utc::now(),
            expires_at: Utc::now(),
            status: IntentStatus::Pending,
        }
    }

    #[tokio::test]
    async fn publish_delivers_to_subscriber() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe("intent.received");

        bus.publish(Event::IntentReceived(dummy_intent()));

        let event = rx.recv().await.unwrap();
        assert!(matches!(event, Event::IntentReceived(_)));
    }

    #[tokio::test]
    async fn publish_to_unsubscribed_channel_does_not_panic() {
        let bus = EventBus::new();
        // No subscriber – should silently drop
        bus.publish(Event::IntentReceived(dummy_intent()));
    }

    #[tokio::test]
    async fn multiple_subscribers_receive_same_event() {
        let bus = EventBus::new();
        let mut rx1 = bus.subscribe("intent.cancelled");
        let mut rx2 = bus.subscribe("intent.cancelled");

        bus.publish(Event::IntentCancelled(dummy_intent()));

        assert!(matches!(
            rx1.recv().await.unwrap(),
            Event::IntentCancelled(_)
        ));
        assert!(matches!(
            rx2.recv().await.unwrap(),
            Event::IntentCancelled(_)
        ));
    }

    #[tokio::test]
    async fn events_are_routed_to_correct_channel() {
        let bus = EventBus::new();
        let mut rx_received = bus.subscribe("intent.received");
        let mut rx_cancelled = bus.subscribe("intent.cancelled");

        bus.publish(Event::IntentReceived(dummy_intent()));

        // received channel gets the event
        assert!(rx_received.recv().await.is_ok());
        // cancelled channel should be empty (try_recv returns Err)
        assert!(rx_cancelled.try_recv().is_err());
    }

    #[test]
    fn default_creates_valid_instance() {
        let bus = EventBus::default();
        // subscribe should work on a default-constructed bus
        let _rx = bus.subscribe("intent.received");
    }

    #[test]
    fn clone_shares_channels() {
        let bus = EventBus::new();
        let bus2 = bus.clone();
        let mut rx = bus.subscribe("intent.received");

        bus2.publish(Event::IntentReceived(dummy_intent()));
        assert!(rx.try_recv().is_ok());
    }
}
