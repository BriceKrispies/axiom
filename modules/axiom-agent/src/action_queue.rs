//! A bounded, deterministic FIFO queue of [`ActionIntent`]s.

use axiom_kernel::{KernelError, KernelErrorCode, KernelErrorScope, KernelResult};

use crate::action_intent::ActionIntent;

/// A fixed-capacity, insertion-ordered queue of action intents.
///
/// It is a plain `Vec` with an explicit `capacity`. `push` appends to the back
/// and fails deterministically (a kernel [`KernelErrorCode::OutOfBounds`]) when
/// full; `pop` removes from the front, so intents leave in exactly the order
/// they were enqueued.
#[derive(Debug, Clone)]
pub struct ActionQueue {
    intents: Vec<ActionIntent>,
    capacity: usize,
}

impl ActionQueue {
    /// An empty queue bounded to at most `capacity` intents.
    pub fn empty_with_capacity(capacity: usize) -> Self {
        ActionQueue {
            intents: Vec::new(),
            capacity,
        }
    }

    /// Build a queue that already holds `intents`, sized exactly to fit them.
    /// The agent runtime uses this to hand back a brain's emissions in order.
    pub(crate) fn from_intents(intents: Vec<ActionIntent>) -> Self {
        let capacity = intents.len();
        ActionQueue { intents, capacity }
    }

    /// Enqueue `intent` at the back, or fail if the queue is full.
    pub fn push(&mut self, intent: ActionIntent) -> KernelResult<()> {
        let room = self.intents.len() < self.capacity;
        room.then(|| self.intents.push(intent)).ok_or_else(|| {
            KernelError::new(
                KernelErrorScope::Memory,
                KernelErrorCode::OutOfBounds,
                "action queue capacity exceeded",
            )
        })
    }

    /// Dequeue the front intent, or `None` if the queue is empty.
    pub fn pop(&mut self) -> Option<ActionIntent> {
        let has = !self.intents.is_empty();
        has.then(|| self.intents.remove(0))
    }

    /// The number of queued intents.
    pub fn len(&self) -> usize {
        self.intents.len()
    }

    /// Whether the queue holds no intents.
    pub fn is_empty(&self) -> bool {
        self.intents.is_empty()
    }

    /// The bound on how many intents may be queued.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The queued intents, in FIFO order (front first).
    pub fn intents(&self) -> &[ActionIntent] {
        &self.intents
    }

    /// The bitwise-OR of every queued intent's control code — the tick's combined
    /// held controls. This folds a whole multi-intent decision into one bitmask,
    /// so a brain that emits several `press_control` intents in one tick holds
    /// them all (e.g. forward + turn), not just the first. An empty queue yields
    /// `0` (no controls held).
    pub fn combined_control_code(&self) -> u32 {
        self.intents
            .iter()
            .fold(0, |acc, intent| acc | intent.control_code())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_queue_reports_empty() {
        let mut q = ActionQueue::empty_with_capacity(2);
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
        assert_eq!(q.capacity(), 2);
        assert!(q.pop().is_none());
    }

    #[test]
    fn push_and_pop_preserve_fifo_order() {
        let mut q = ActionQueue::empty_with_capacity(3);
        q.push(ActionIntent::press_control(1)).unwrap();
        q.push(ActionIntent::press_control(2)).unwrap();
        q.push(ActionIntent::press_control(3)).unwrap();
        assert_eq!(q.len(), 3);
        assert_eq!(q.intents()[0].control_code(), 1);
        assert_eq!(q.pop().unwrap().control_code(), 1);
        assert_eq!(q.pop().unwrap().control_code(), 2);
        assert_eq!(q.pop().unwrap().control_code(), 3);
        assert!(q.pop().is_none());
    }

    #[test]
    fn push_overflow_fails_deterministically() {
        let mut q = ActionQueue::empty_with_capacity(1);
        assert!(q.push(ActionIntent::noop()).is_ok());
        let err = q.push(ActionIntent::noop()).unwrap_err();
        assert_eq!(err.code(), KernelErrorCode::OutOfBounds);
        assert_eq!(err.scope(), KernelErrorScope::Memory);
    }

    #[test]
    fn from_intents_sizes_to_fit() {
        let q = ActionQueue::from_intents(vec![ActionIntent::noop(), ActionIntent::wait_ticks(2)]);
        assert_eq!(q.len(), 2);
        assert_eq!(q.capacity(), 2);
        assert_eq!(q.intents()[1].ticks(), 2);
    }

    #[test]
    fn derives_are_exercised() {
        let q = ActionQueue::empty_with_capacity(1);
        let c = q.clone();
        assert_eq!(c.capacity(), 1);
        assert!(format!("{q:?}").contains("ActionQueue"));
    }

    #[test]
    fn combined_control_code_ors_every_queued_intent() {
        // Empty queue → no controls held.
        assert_eq!(ActionQueue::empty_with_capacity(2).combined_control_code(), 0);
        // One intent → exactly its control code.
        assert_eq!(
            ActionQueue::from_intents(vec![ActionIntent::press_control(0b0100)]).combined_control_code(),
            0b0100,
        );
        // Distinct bits OR together (forward + turn held in one tick); a repeated
        // bit is idempotent, so the fold both ORs new bits and re-ORs a present one.
        let q = ActionQueue::from_intents(vec![
            ActionIntent::press_control(0b0001),
            ActionIntent::press_control(0b0100),
            ActionIntent::press_control(0b0001),
        ]);
        assert_eq!(q.combined_control_code(), 0b0101);
    }
}
