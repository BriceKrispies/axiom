//! Deterministic FIFO queue of [`RuntimeEvent`]s.

use std::collections::VecDeque;

use crate::runtime_event::RuntimeEvent;

/// A first-in, first-out queue of runtime events.
///
/// Same insertion-order guarantee as [`crate::runtime_command_queue::RuntimeCommandQueue`].
/// Commands and events are kept in separate queues so semantically distinct
/// streams cannot interleave.
#[derive(Debug, Clone, Default)]
pub struct RuntimeEventQueue {
    items: VecDeque<RuntimeEvent>,
}

impl RuntimeEventQueue {
    /// Create an empty queue.
    pub fn new() -> Self {
        RuntimeEventQueue {
            items: VecDeque::new(),
        }
    }

    pub fn push(&mut self, event: RuntimeEvent) {
        self.items.push_back(event);
    }

    pub fn pop(&mut self) -> Option<RuntimeEvent> {
        self.items.pop_front()
    }

    pub fn peek(&self) -> Option<&RuntimeEvent> {
        self.items.front()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Drain every event in insertion order, leaving the queue empty.
    pub fn drain_all(&mut self) -> Vec<RuntimeEvent> {
        self.items.drain(..).collect()
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Tick;

    fn evt(kind: u32) -> RuntimeEvent {
        RuntimeEvent::new(kind, Tick::new(0), vec![])
    }

    #[test]
    fn new_and_default_queues_are_empty() {
        assert!(RuntimeEventQueue::new().is_empty());
        assert!(RuntimeEventQueue::default().is_empty());
    }

    #[test]
    fn pop_order_matches_push_order() {
        let mut q = RuntimeEventQueue::new();
        for k in 1..=5 {
            q.push(evt(k));
        }
        let kinds: Vec<u32> = std::iter::from_fn(|| q.pop().map(|e| e.kind())).collect();
        assert_eq!(kinds, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn drain_all_preserves_order_and_empties() {
        let mut q = RuntimeEventQueue::new();
        q.push(evt(1));
        q.push(evt(2));
        let drained: Vec<u32> = q.drain_all().into_iter().map(|e| e.kind()).collect();
        assert_eq!(drained, vec![1, 2]);
        assert!(q.is_empty());
    }
}
