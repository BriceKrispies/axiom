//! Deterministic FIFO queue of [`RuntimeCommand`]s.

use std::collections::VecDeque;

use crate::runtime_command::RuntimeCommand;

/// A first-in, first-out queue of runtime commands.
///
/// Insertion order is the *only* ordering: no hashing, no priority. Two runs
/// that push the same commands in the same order pop them in the same order.
/// The runtime drains this queue at explicit step boundaries; systems push
/// into it during a step via [`crate::runtime_context::RuntimeContext`].
#[derive(Debug, Clone, Default)]
pub struct RuntimeCommandQueue {
    items: VecDeque<RuntimeCommand>,
}

impl RuntimeCommandQueue {
    /// Create an empty queue.
    pub fn new() -> Self {
        RuntimeCommandQueue {
            items: VecDeque::new(),
        }
    }

    /// Append a command to the back.
    pub fn push(&mut self, command: RuntimeCommand) {
        self.items.push_back(command);
    }

    /// Remove and return the front command.
    pub fn pop(&mut self) -> Option<RuntimeCommand> {
        self.items.pop_front()
    }

    /// Borrow the front command without removing it.
    pub fn peek(&self) -> Option<&RuntimeCommand> {
        self.items.front()
    }

    /// Number of queued commands.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Drain every command in insertion order, leaving the queue empty.
    pub fn drain_all(&mut self) -> Vec<RuntimeCommand> {
        self.items.drain(..).collect()
    }

    /// Remove every command without returning them.
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Tick;

    fn cmd(kind: u32) -> RuntimeCommand {
        RuntimeCommand::new(kind, Tick::new(0), vec![])
    }

    #[test]
    fn new_and_default_queues_are_empty() {
        assert!(RuntimeCommandQueue::new().is_empty());
        assert!(RuntimeCommandQueue::default().is_empty());
    }

    #[test]
    fn pop_order_matches_push_order() {
        let mut q = RuntimeCommandQueue::new();
        for k in 1..=5 {
            q.push(cmd(k));
        }
        let kinds: Vec<u32> = std::iter::from_fn(|| q.pop().map(|c| c.kind())).collect();
        assert_eq!(kinds, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn peek_does_not_consume() {
        let mut q = RuntimeCommandQueue::new();
        q.push(cmd(9));
        assert_eq!(q.peek().unwrap().kind(), 9);
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn drain_all_returns_everything_in_order_and_empties() {
        let mut q = RuntimeCommandQueue::new();
        q.push(cmd(1));
        q.push(cmd(2));
        let drained: Vec<u32> = q.drain_all().into_iter().map(|c| c.kind()).collect();
        assert_eq!(drained, vec![1, 2]);
        assert!(q.is_empty());
    }
}
