//! A deterministic process wake queue keyed by `(SimTick, ProcessId)`.

use std::collections::BTreeMap;

use crate::ids::ProcessId;
use crate::sim_tick::SimTick;

/// Why a process was placed on the wake queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WakeReason {
    /// Scheduled explicitly at registration/scheduling time.
    Scheduled,
    /// Re-scheduled to a new tick.
    Rescheduled,
    /// Woken because a dependency it subscribed to went dirty.
    DirtyDependency,
    /// Woken by an explicit external request.
    Explicit,
    /// An unclassified wake.
    Generic,
}

const WAKE_REASONS: [WakeReason; 5] = [
    WakeReason::Scheduled,
    WakeReason::Rescheduled,
    WakeReason::DirtyDependency,
    WakeReason::Explicit,
    WakeReason::Generic,
];

impl WakeReason {
    /// Construct from a code, `None` if out of range.
    pub fn from_code(code: u8) -> Option<WakeReason> {
        WAKE_REASONS.get(code as usize).copied()
    }

    /// The reason's deterministic code.
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// A due wake: the process and why it woke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WakeEntry {
    process: ProcessId,
    reason: WakeReason,
}

impl WakeEntry {
    /// The process to wake.
    pub const fn process(&self) -> ProcessId {
        self.process
    }
    /// Why it was woken.
    pub const fn reason(&self) -> WakeReason {
        self.reason
    }
}

/// A wake queue: at most one pending wake per process, ordered by `(tick, id)`.
///
/// Due work is found by a range query, never by scanning all processes. A second
/// schedule for the same process replaces its pending wake (so repeated
/// rescheduling is stable). Canceling removes the pending wake.
#[derive(Debug, Clone, Default)]
pub struct ProcessWakeQueue {
    // (tick, process) -> reason, the ordered wake index.
    index: BTreeMap<(SimTick, ProcessId), WakeReason>,
    // process -> its single currently-pending tick, for reschedule/cancel.
    pending: BTreeMap<ProcessId, SimTick>,
}

impl ProcessWakeQueue {
    /// Create an empty wake queue.
    pub fn new() -> Self {
        ProcessWakeQueue {
            index: BTreeMap::new(),
            pending: BTreeMap::new(),
        }
    }

    /// Schedule (or move) a process's wake to `tick` with `reason`. Any prior
    /// pending wake for the process is removed first, so there is exactly one.
    pub fn schedule(&mut self, process: ProcessId, tick: SimTick, reason: WakeReason) {
        let previous = self.pending.insert(process, tick);
        previous.map(|old| self.index.remove(&(old, process)));
        self.index.insert((tick, process), reason);
    }

    /// Cancel a process's pending wake. Returns whether it was pending.
    pub fn cancel(&mut self, process: ProcessId) -> bool {
        let previous = self.pending.remove(&process);
        previous.map(|old| self.index.remove(&(old, process)));
        previous.is_some()
    }

    /// The pending wake tick of a process, if any.
    pub fn pending_tick(&self, process: ProcessId) -> Option<SimTick> {
        self.pending.get(&process).copied()
    }

    /// Remove and return every wake due at or before `tick`, in `(tick, id)`
    /// order. Future wakes (tick > `tick`) are never returned.
    pub fn pop_due(&mut self, tick: SimTick) -> Vec<WakeEntry> {
        let upper = (tick, ProcessId::from_raw(u64::MAX));
        let due: Vec<((SimTick, ProcessId), WakeReason)> = self
            .index
            .range(..=upper)
            .map(|(key, reason)| (*key, *reason))
            .collect();
        due.iter().for_each(|((due_tick, process), _)| {
            self.index.remove(&(*due_tick, *process));
            self.pending.remove(process);
        });
        due.into_iter()
            .map(|((_, process), reason)| WakeEntry { process, reason })
            .collect()
    }

    /// The processes due at or before `tick`, in `(tick, id)` order, **without**
    /// removing them (inspection only).
    pub fn due_at(&self, tick: SimTick) -> Vec<ProcessId> {
        let upper = (tick, ProcessId::from_raw(u64::MAX));
        self.index
            .range(..=upper)
            .map(|((_, process), _)| *process)
            .collect()
    }

    /// The number of pending wakes.
    pub fn len(&self) -> usize {
        self.index.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(raw: u64) -> ProcessId {
        ProcessId::from_raw(raw)
    }

    #[test]
    fn wake_reason_codes_round_trip() {
        assert_eq!(WakeReason::from_code(0), Some(WakeReason::Scheduled));
        assert_eq!(WakeReason::from_code(4), Some(WakeReason::Generic));
        assert_eq!(WakeReason::from_code(5), None);
        assert_eq!(WakeReason::DirtyDependency.code(), 2);
    }

    #[test]
    fn pops_due_in_order_and_skips_future() {
        let mut queue = ProcessWakeQueue::new();
        assert_eq!(queue.len(), 0);
        queue.schedule(p(2), SimTick::new(10), WakeReason::Scheduled);
        queue.schedule(p(1), SimTick::new(5), WakeReason::Scheduled);
        queue.schedule(p(3), SimTick::new(5), WakeReason::DirtyDependency);
        assert_eq!(queue.len(), 3);
        let due = queue.pop_due(SimTick::new(5));
        let ids: Vec<u64> = due.iter().map(|e| e.process().raw()).collect();
        assert_eq!(
            ids,
            vec![1, 3],
            "ordered by (tick, id); future (tick 10) excluded"
        );
        assert_eq!(due[1].reason(), WakeReason::DirtyDependency);
        // Only the future wake remains.
        assert_eq!(queue.len(), 1);
        assert!(
            queue.pop_due(SimTick::new(5)).is_empty(),
            "already-popped wakes do not re-fire"
        );
    }

    #[test]
    fn repeated_schedule_keeps_one_pending() {
        let mut queue = ProcessWakeQueue::new();
        queue.schedule(p(1), SimTick::new(3), WakeReason::Scheduled);
        // Repeated schedule replaces, not duplicates.
        queue.schedule(p(1), SimTick::new(8), WakeReason::Scheduled);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.pending_tick(p(1)), Some(SimTick::new(8)));
        // Not due at 3 anymore (moved to 8).
        assert!(queue.pop_due(SimTick::new(3)).is_empty());
        // Move it earlier via another schedule with a distinct reason.
        queue.schedule(p(1), SimTick::new(2), WakeReason::Rescheduled);
        assert_eq!(queue.pending_tick(p(1)), Some(SimTick::new(2)));
        let due = queue.pop_due(SimTick::new(2));
        assert_eq!(due[0].reason(), WakeReason::Rescheduled);
    }

    #[test]
    fn due_at_peeks_without_removing() {
        let mut queue = ProcessWakeQueue::new();
        queue.schedule(p(2), SimTick::new(5), WakeReason::Scheduled);
        queue.schedule(p(1), SimTick::new(3), WakeReason::Scheduled);
        queue.schedule(p(3), SimTick::new(9), WakeReason::Scheduled);
        let due: Vec<u64> = queue
            .due_at(SimTick::new(5))
            .iter()
            .map(|p| p.raw())
            .collect();
        assert_eq!(due, vec![1, 2], "peek is ordered and excludes the future");
        assert_eq!(queue.len(), 3, "peek does not remove");
    }

    #[test]
    fn cancel_removes_pending() {
        let mut queue = ProcessWakeQueue::new();
        queue.schedule(p(1), SimTick::new(3), WakeReason::Scheduled);
        assert!(queue.cancel(p(1)));
        assert!(queue.pending_tick(p(1)).is_none());
        assert_eq!(queue.len(), 0);
        assert!(
            !queue.cancel(p(1)),
            "canceling a non-pending process is clean"
        );
    }
}
