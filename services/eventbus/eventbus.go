package eventbus

import (
	"context"
	"log"
	"sync"
	"time"

	"pre_trading/services/types"
)

// EventBus is a simple in-memory event bus
// In production, replace with Redpanda/Kafka
type EventBus struct {
	mu          sync.RWMutex
	subscribers map[string][]*subscription
}

type subscription struct {
	ch chan types.Event
}

func NewEventBus() *EventBus {
	return &EventBus{
		subscribers: make(map[string][]*subscription),
	}
}

// Publish sends an event to all subscribers of the given event type
func (eb *EventBus) Publish(eventType string, payload interface{}) {
	event := types.Event{
		Type:      eventType,
		Payload:   payload,
		Timestamp: time.Now(),
	}

	eb.mu.RLock()
	defer eb.mu.RUnlock()

	subscribers, exists := eb.subscribers[eventType]
	if !exists {
		return
	}

	// Non-blocking send to all subscribers
	for _, sub := range subscribers {
		select {
		case sub.ch <- event:
		default:
			log.Printf("Warning: subscriber channel full for event type %s", eventType)
		}
	}
}

// Subscribe creates a subscription for the given event type
func (eb *EventBus) Subscribe(eventType string, bufferSize int) <-chan types.Event {
	eb.mu.Lock()
	defer eb.mu.Unlock()

	sub := &subscription{
		ch: make(chan types.Event, bufferSize),
	}
	eb.subscribers[eventType] = append(eb.subscribers[eventType], sub)
	return sub.ch
}

// SubscribeMultiple subscribes to multiple event types
func (eb *EventBus) SubscribeMultiple(eventTypes []string, bufferSize int) <-chan types.Event {
	eb.mu.Lock()
	defer eb.mu.Unlock()

	sub := &subscription{
		ch: make(chan types.Event, bufferSize),
	}

	for _, eventType := range eventTypes {
		eb.subscribers[eventType] = append(eb.subscribers[eventType], sub)
	}
	return sub.ch
}

// Unsubscribe removes a subscription (not implemented for simplicity)
func (eb *EventBus) Unsubscribe(eventType string, ch <-chan types.Event) {
	// TODO: implement if needed
}

// WaitFor waits for a specific event with timeout
func (eb *EventBus) WaitFor(ctx context.Context, eventType string, timeout time.Duration) (*types.Event, error) {
	// Create a temporary buffered channel
	tempCh := make(chan types.Event, 1)
	sub := &subscription{ch: tempCh}
	
	eb.mu.Lock()
	eb.subscribers[eventType] = append(eb.subscribers[eventType], sub)
	eb.mu.Unlock()
	
	// Cleanup: remove subscription after use
	defer func() {
		eb.mu.Lock()
		defer eb.mu.Unlock()
		
		if subs, exists := eb.subscribers[eventType]; exists {
			for i, s := range subs {
				if s == sub {
					eb.subscribers[eventType] = append(subs[:i], subs[i+1:]...)
					break
				}
			}
		}
		close(tempCh)
	}()

	ctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()

	select {
	case event := <-tempCh:
		return &event, nil
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}
