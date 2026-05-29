//! Deterministic FIFO queue of frame-local commands.

use std::collections::VecDeque;

use crate::frame_command::FrameCommand;

/// A first-in, first-out queue of [`FrameCommand`]s.
///
/// The queue assigns each command a strictly-monotonic sequence number
/// (starting at `1`), so two identical insertion sequences produce
/// byte-identical drained outputs across runs. There is no priority, no
/// hashing, and no out-of-band reordering — insertion order is the only
/// ordering.
#[derive(Debug, Clone)]
pub struct FrameCommandQueue {
    items: VecDeque<FrameCommand>,
    next_sequence: u64,
}

impl Default for FrameCommandQueue {
    fn default() -> Self {
        FrameCommandQueue::new()
    }
}

impl FrameCommandQueue {
    /// Create an empty queue. The next sequence number assigned will be `1`.
    pub fn new() -> Self {
        FrameCommandQueue {
            items: VecDeque::new(),
            next_sequence: 1,
        }
    }

    /// Push a command with `kind` and `payload`; the queue assigns the
    /// monotonic sequence number and returns it.
    pub fn push(&mut self, kind: u32, payload: Vec<u8>) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.items
            .push_back(FrameCommand::new(sequence, kind, payload));
        sequence
    }

    /// Drain every command in FIFO order, leaving the queue empty. The
    /// internal sequence counter is **not** reset, so two queues that are
    /// pushed-and-drained in the same shape produce identical output and
    /// identical *next* sequences.
    pub fn drain(&mut self) -> Vec<FrameCommand> {
        let mut out = Vec::with_capacity(self.items.len());
        while let Some(c) = self.items.pop_front() {
            out.push(c);
        }
        out
    }

    /// Discard every queued command without producing them. The sequence
    /// counter is preserved for the same reason `drain` preserves it.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Number of queued commands.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The sequence number that will be assigned to the *next* push.
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_queue_is_empty_and_starts_at_one() {
        let q = FrameCommandQueue::new();
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
        assert_eq!(q.next_sequence(), 1);
    }

    #[test]
    fn push_returns_monotonic_sequence_numbers() {
        let mut q = FrameCommandQueue::new();
        assert_eq!(q.push(10, vec![]), 1);
        assert_eq!(q.push(20, vec![]), 2);
        assert_eq!(q.push(30, vec![]), 3);
        assert_eq!(q.len(), 3);
    }

    #[test]
    fn drain_returns_commands_in_fifo_order() {
        let mut q = FrameCommandQueue::new();
        q.push(1, vec![10]);
        q.push(2, vec![20]);
        q.push(3, vec![30]);
        let out = q.drain();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].kind(), 1);
        assert_eq!(out[1].kind(), 2);
        assert_eq!(out[2].kind(), 3);
        assert!(q.is_empty());
    }

    #[test]
    fn drain_preserves_assigned_sequence_ids() {
        let mut q = FrameCommandQueue::new();
        q.push(0, vec![]);
        q.push(0, vec![]);
        q.push(0, vec![]);
        let out = q.drain();
        let seqs: Vec<u64> = out.iter().map(|c| c.sequence()).collect();
        assert_eq!(seqs, vec![1, 2, 3]);
    }

    #[test]
    fn clear_empties_the_queue() {
        let mut q = FrameCommandQueue::new();
        q.push(1, vec![]);
        q.push(2, vec![]);
        q.clear();
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn identical_insertion_produces_identical_drain_output() {
        let make = || {
            let mut q = FrameCommandQueue::new();
            q.push(7, vec![1, 2, 3]);
            q.push(8, vec![4, 5]);
            q.push(9, Vec::new());
            q.drain()
        };
        assert_eq!(make(), make());
    }

    #[test]
    fn sequence_is_not_reset_by_drain_or_clear() {
        let mut q = FrameCommandQueue::new();
        q.push(0, vec![]);
        q.drain();
        assert_eq!(q.next_sequence(), 2);
        q.push(0, vec![]);
        q.clear();
        assert_eq!(q.next_sequence(), 3);
    }

    #[test]
    fn default_matches_new() {
        let a = FrameCommandQueue::default();
        let b = FrameCommandQueue::new();
        assert_eq!(a.len(), b.len());
        assert_eq!(a.next_sequence(), b.next_sequence());
    }
}
