//! A deterministic FIFO queue of message envelopes.

use crate::message_envelope::MessageEnvelope;
use std::collections::VecDeque;

/// A first-in, first-out queue of [`MessageEnvelope`]s.
///
/// Ordering is strictly insertion order: `pop` returns envelopes in exactly the
/// order they were `push`ed. No hashing or wall-clock influences the order, so
/// the same push sequence always produces the same pop sequence.
#[derive(Debug, Clone, Default)]
pub struct MessageQueue {
    items: VecDeque<MessageEnvelope>,
}

impl MessageQueue {
    /// Create an empty queue.
    pub fn new() -> Self {
        MessageQueue {
            items: VecDeque::new(),
        }
    }

    /// Append an envelope to the back of the queue.
    pub fn push(&mut self, envelope: MessageEnvelope) {
        self.items.push_back(envelope);
    }

    /// Remove and return the front envelope, or `None` if empty.
    pub fn pop(&mut self) -> Option<MessageEnvelope> {
        self.items.pop_front()
    }

    /// Borrow the front envelope without removing it.
    pub fn peek(&self) -> Option<&MessageEnvelope> {
        self.items.front()
    }

    /// The number of queued envelopes.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the queue holds no envelopes.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Remove all envelopes.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_id::MessageId;
    use crate::message_kind::MessageKind;
    use crate::tick::Tick;

    fn envelope(id: u64) -> MessageEnvelope {
        MessageEnvelope::new(
            MessageId::from_raw(id),
            MessageKind::new(0),
            Tick::new(id),
            vec![],
        )
    }

    #[test]
    fn new_queue_is_empty() {
        let q = MessageQueue::new();
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
        assert!(q.peek().is_none());
    }

    #[test]
    fn default_queue_is_empty() {
        assert!(MessageQueue::default().is_empty());
    }

    #[test]
    fn pop_returns_envelopes_in_fifo_order() {
        let mut q = MessageQueue::new();
        for id in 1..=4 {
            q.push(envelope(id));
        }
        assert_eq!(q.len(), 4);

        let popped: Vec<u64> = std::iter::from_fn(|| q.pop().map(|e| e.id().raw())).collect();
        assert_eq!(popped, vec![1, 2, 3, 4]);
        assert!(q.is_empty());
    }

    #[test]
    fn peek_does_not_consume() {
        let mut q = MessageQueue::new();
        q.push(envelope(7));
        assert_eq!(q.peek().unwrap().id(), MessageId::from_raw(7));
        assert_eq!(q.len(), 1, "peek must not remove the front");
        assert_eq!(q.pop().unwrap().id(), MessageId::from_raw(7));
    }

    #[test]
    fn pop_on_empty_is_none() {
        let mut q = MessageQueue::new();
        assert!(q.pop().is_none());
    }

    #[test]
    fn clear_empties_the_queue() {
        let mut q = MessageQueue::new();
        q.push(envelope(1));
        q.push(envelope(2));
        q.clear();
        assert!(q.is_empty());
        assert!(q.pop().is_none());
    }
}
