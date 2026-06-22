//! The generic process model: deterministic, tick-scheduled simulation activity.

use std::collections::BTreeMap;

use axiom_ecs::EntityHandle;

use crate::cause::CauseRef;
use crate::ids::ProcessId;

/// The domain-defined *kind* of a process, as an opaque deterministic code.
/// sim-core gives it no behavior — later phases map codes to activities
/// (evaporation, healing, hauling, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessKind(u32);

impl ProcessKind {
    /// A process kind from a deterministic code.
    pub const fn new(code: u32) -> Self {
        ProcessKind(code)
    }

    /// The raw code.
    pub const fn code(self) -> u32 {
        self.0
    }
}

/// An opaque, domain-defined process state code (the meaning is the domain's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessState(u32);

impl ProcessState {
    /// A process state from a deterministic code.
    pub const fn new(code: u32) -> Self {
        ProcessState(code)
    }

    /// The raw code.
    pub const fn code(self) -> u32 {
        self.0
    }
}

/// A logical tick at which a process should next wake. Wall-clock time is never
/// used; this is a simulation tick supplied by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WakeTick(u64);

impl WakeTick {
    /// A wake tick from a raw logical tick.
    pub const fn new(tick: u64) -> Self {
        WakeTick(tick)
    }

    /// The raw logical tick.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// A scheduled simulation activity over a subject entity.
///
/// A process reads facts/relations and (via effects) mutates the world when it
/// wakes. It carries a [`ProcessKind`], a [`ProcessState`], its next
/// [`WakeTick`], an optional [`cause`](Self::cause), and a [`ProcessId`] ordering
/// key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Process {
    id: ProcessId,
    kind: ProcessKind,
    subject: EntityHandle,
    state: ProcessState,
    wake: WakeTick,
    cause: Option<CauseRef>,
}

impl Process {
    /// This process's stable id (its deterministic ordering key).
    pub const fn id(&self) -> ProcessId {
        self.id
    }

    /// The process's kind.
    pub const fn kind(&self) -> ProcessKind {
        self.kind
    }

    /// The subject entity.
    pub const fn subject(&self) -> EntityHandle {
        self.subject
    }

    /// The current state code.
    pub const fn state(&self) -> ProcessState {
        self.state
    }

    /// The next wake tick.
    pub const fn wake(&self) -> WakeTick {
        self.wake
    }

    /// What caused this process, if recorded.
    pub const fn cause(&self) -> Option<CauseRef> {
        self.cause
    }
}

/// A deterministic process queue with a wake index keyed by `(WakeTick,
/// ProcessId)`, so due processes are found by range — never by scanning all
/// processes — and always wake in `(tick, id)` order.
#[derive(Debug, Clone, Default)]
pub struct ProcessQueue {
    processes: BTreeMap<ProcessId, Process>,
    wake_index: BTreeMap<(WakeTick, ProcessId), ()>,
    next: u64,
}

impl ProcessQueue {
    /// Create an empty queue. The first scheduled process has id 1.
    pub fn new() -> Self {
        ProcessQueue {
            processes: BTreeMap::new(),
            wake_index: BTreeMap::new(),
            next: 1,
        }
    }

    /// Schedule a new process, minting and returning its deterministic id.
    pub fn schedule(
        &mut self,
        kind: ProcessKind,
        subject: EntityHandle,
        state: ProcessState,
        wake: WakeTick,
        cause: Option<CauseRef>,
    ) -> ProcessId {
        let id = ProcessId::from_raw(self.next);
        self.next += 1;
        self.processes.insert(
            id,
            Process {
                id,
                kind,
                subject,
                state,
                wake,
                cause,
            },
        );
        self.wake_index.insert((wake, id), ());
        id
    }

    /// Cancel a process, removing it from the queue and the wake index. Returns
    /// whether it existed (a clean `false` if absent).
    pub fn cancel(&mut self, id: ProcessId) -> bool {
        self.processes
            .remove(&id)
            .map(|process| self.wake_index.remove(&(process.wake, id)))
            .is_some()
    }

    /// Move a process's next wake to `wake`, re-indexing it. Returns whether the
    /// process existed (a clean `false` if absent).
    pub fn reschedule(&mut self, id: ProcessId, wake: WakeTick) -> bool {
        let previous = self.processes.get(&id).map(Process::wake);
        previous
            .map(|old| {
                let _ = self
                    .processes
                    .get_mut(&id)
                    .map(|process| process.wake = wake);
                self.wake_index.remove(&(old, id));
                self.wake_index.insert((wake, id), ());
            })
            .is_some()
    }

    /// Wake every process due at or before `tick`, in `(wake, id)` order, and
    /// remove them from the wake index (they are now awake; the caller reschedules
    /// or cancels them). Future processes (wake > tick) are never returned.
    pub fn wake_due(&mut self, tick: u64) -> Vec<ProcessId> {
        let upper = (WakeTick::new(tick), ProcessId::from_raw(u64::MAX));
        let due: Vec<(WakeTick, ProcessId)> = self
            .wake_index
            .range(..=upper)
            .map(|(key, _)| *key)
            .collect();
        due.iter().for_each(|key| {
            self.wake_index.remove(key);
        });
        due.into_iter().map(|(_, id)| id).collect()
    }

    /// Borrow a process by id, if present.
    pub fn get(&self, id: ProcessId) -> Option<&Process> {
        self.processes.get(&id)
    }

    /// All processes, in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = &Process> {
        self.processes.values()
    }

    /// The number of live processes.
    pub fn len(&self) -> usize {
        self.processes.len()
    }

    /// Whether there are no live processes.
    pub fn is_empty(&self) -> bool {
        self.processes.is_empty()
    }

    /// The number of processes currently scheduled to wake (in the wake index).
    pub fn scheduled(&self) -> usize {
        self.wake_index.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_ecs::EntityRegistry;

    fn subject(reg: &mut EntityRegistry) -> EntityHandle {
        reg.spawn_handle()
    }

    #[test]
    fn new_and_default_are_empty() {
        assert!(ProcessQueue::new().is_empty());
        assert_eq!(ProcessQueue::new().len(), 0);
        assert_eq!(ProcessQueue::new().scheduled(), 0);
        assert!(ProcessQueue::default().is_empty());
    }

    #[test]
    fn schedule_get_and_cancel() {
        let mut reg = EntityRegistry::new();
        let a = subject(&mut reg);
        let mut queue = ProcessQueue::new();
        let id = queue.schedule(
            ProcessKind::new(1),
            a,
            ProcessState::new(0),
            WakeTick::new(5),
            None,
        );
        assert_eq!(id.raw(), 1);
        let process = queue.get(id).unwrap();
        assert_eq!(process.kind(), ProcessKind::new(1));
        assert_eq!(process.subject(), a);
        assert_eq!(process.state(), ProcessState::new(0));
        assert_eq!(process.wake(), WakeTick::new(5));
        assert_eq!(process.cause(), None);
        assert_eq!(queue.scheduled(), 1);
        assert!(queue.cancel(id));
        assert!(queue.get(id).is_none());
        assert_eq!(queue.scheduled(), 0);
        assert!(
            !queue.cancel(id),
            "cancelling a missing process is a clean false"
        );
    }

    #[test]
    fn wake_due_returns_due_in_order_and_skips_future() {
        let mut reg = EntityRegistry::new();
        let a = subject(&mut reg);
        let mut queue = ProcessQueue::new();
        // Schedule out of order; the wake index must order by (tick, id).
        let p_late = queue.schedule(
            ProcessKind::new(1),
            a,
            ProcessState::new(0),
            WakeTick::new(10),
            None,
        );
        let p_early = queue.schedule(
            ProcessKind::new(1),
            a,
            ProcessState::new(0),
            WakeTick::new(2),
            None,
        );
        let p_mid = queue.schedule(
            ProcessKind::new(1),
            a,
            ProcessState::new(0),
            WakeTick::new(5),
            None,
        );
        // Two processes due at the same tick must wake in id order.
        let p_tie = queue.schedule(
            ProcessKind::new(1),
            a,
            ProcessState::new(0),
            WakeTick::new(2),
            None,
        );

        let due = queue.wake_due(5);
        assert_eq!(
            due,
            vec![p_early, p_tie, p_mid],
            "ordered by (tick, id); future excluded"
        );
        assert!(!due.contains(&p_late), "a future process is not woken");
        // Woken processes left the wake index; only the future one remains scheduled.
        assert_eq!(queue.scheduled(), 1);
        // Waking again at the same tick yields nothing (they already woke).
        assert!(queue.wake_due(5).is_empty());
    }

    #[test]
    fn reschedule_moves_the_wake() {
        let mut reg = EntityRegistry::new();
        let a = subject(&mut reg);
        let mut queue = ProcessQueue::new();
        let id = queue.schedule(
            ProcessKind::new(1),
            a,
            ProcessState::new(0),
            WakeTick::new(3),
            None,
        );
        // Not due at tick 1.
        assert!(queue.wake_due(1).is_empty());
        // Reschedule earlier; now due at tick 1.
        assert!(queue.reschedule(id, WakeTick::new(1)));
        assert_eq!(queue.get(id).unwrap().wake(), WakeTick::new(1));
        assert_eq!(queue.wake_due(1), vec![id]);
        // Rescheduling a missing process is a clean false.
        assert!(!queue.reschedule(ProcessId::from_raw(999), WakeTick::new(0)));
    }

    #[test]
    fn iter_is_ascending_by_id() {
        let mut reg = EntityRegistry::new();
        let a = subject(&mut reg);
        let mut queue = ProcessQueue::new();
        queue.schedule(
            ProcessKind::new(1),
            a,
            ProcessState::new(0),
            WakeTick::new(9),
            None,
        );
        queue.schedule(
            ProcessKind::new(2),
            a,
            ProcessState::new(0),
            WakeTick::new(1),
            None,
        );
        let ids: Vec<u64> = queue.iter().map(|p| p.id().raw()).collect();
        assert_eq!(ids, vec![1, 2]);
        assert_eq!(queue.len(), 2);
    }
}
