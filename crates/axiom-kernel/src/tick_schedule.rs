//! A deterministic `(tick, id)`-ordered schedule with one pending entry per id.

use std::collections::BTreeMap;

use crate::tick::Tick;

/// A deterministic tick-keyed wake/priority schedule.
///
/// `TickSchedule` holds **at most one** pending entry per `Id`, indexed by
/// `(Tick, Id)`. [`schedule`](TickSchedule::schedule) arms (or re-arms) an id to
/// wake at a future tick with a caller-defined payload;
/// [`pop_due`](TickSchedule::pop_due) removes and returns every entry due at or
/// before a supplied tick in ascending `(tick, id)` order;
/// [`cancel`](TickSchedule::cancel) removes a pending entry. The schedule reads
/// **no clock** — the current tick is always supplied as data — owns no domain
/// meaning, and is generic over an opaque id and an opaque `Copy` payload.
///
/// It is the shared deterministic-scheduling primitive two engine consumers build
/// on (sim-core's process wake queue and the `axiom-tick` timers facade): pure
/// tick + identity data ordered by a total order on `Id`, the kernel's sanctioned
/// remit of "deterministic time/tick primitives over stable IDs". Ties on the
/// same tick break by ascending `Id` — never by insertion or hash order — so two
/// independently-built schedules fed the same operations produce byte-identical
/// due sequences.
///
/// It is type-generic for the same reason [`crate::replay_timeline::ReplayTimeline`]
/// is: the id and payload belong to the caller (a process id and wake reason, a
/// timer id and its one-shot/repeating plan), not the kernel, so the meaning
/// stays with the caller and the kernel owns only the ordering and bookkeeping.
#[derive(Debug, Clone)]
pub struct TickSchedule<Id, P> {
    /// `(tick, id) -> payload`, the ordered due index.
    index: BTreeMap<(Tick, Id), P>,
    /// `id -> its single currently-pending tick`, for re-arm and cancel.
    pending: BTreeMap<Id, Tick>,
}

impl<Id, P> TickSchedule<Id, P> {
    /// An empty schedule.
    pub fn new() -> Self {
        TickSchedule {
            index: BTreeMap::new(),
            pending: BTreeMap::new(),
        }
    }
}

impl<Id, P> Default for TickSchedule<Id, P> {
    fn default() -> Self {
        TickSchedule::new()
    }
}

impl<Id: Ord + Copy, P: Copy> TickSchedule<Id, P> {
    /// Arm (or re-arm) `id` to wake at `at` carrying `payload`. Any prior pending
    /// entry for `id` is removed first, so there is exactly one entry per id and
    /// repeated scheduling is stable (the latest wins).
    pub fn schedule(&mut self, id: Id, at: Tick, payload: P) {
        let previous = self.pending.insert(id, at);
        previous.map(|old| self.index.remove(&(old, id)));
        self.index.insert((at, id), payload);
    }

    /// Cancel `id`'s pending entry. Returns whether one was pending — a clean
    /// `false` if the id was unknown or already fired.
    pub fn cancel(&mut self, id: Id) -> bool {
        let previous = self.pending.remove(&id);
        previous.map(|old| self.index.remove(&(old, id)));
        previous.is_some()
    }

    /// The pending wake tick of `id`, if it has one.
    pub fn pending(&self, id: Id) -> Option<Tick> {
        self.pending.get(&id).copied()
    }

    /// Remove and return every entry due at or before `now`, ascending by
    /// `(tick, id)`. Future entries (tick > `now`) are never returned and stay
    /// pending. Re-running `pop_due` for the same `now` yields nothing more —
    /// fired entries do not re-fire.
    pub fn pop_due(&mut self, now: Tick) -> Vec<(Id, P)> {
        let due: Vec<((Tick, Id), P)> = self
            .index
            .iter()
            .take_while(|((at, _), _)| *at <= now)
            .map(|(key, payload)| (*key, *payload))
            .collect();
        due.iter().for_each(|((at, id), _)| {
            self.index.remove(&(*at, *id));
            self.pending.remove(id);
        });
        due.into_iter()
            .map(|((_, id), payload)| (id, payload))
            .collect()
    }

    /// The entries due at or before `now`, ascending by `(tick, id)`, **without**
    /// removing them (inspection only).
    pub fn peek_due(&self, now: Tick) -> Vec<(Id, P)> {
        self.index
            .iter()
            .take_while(|((at, _), _)| *at <= now)
            .map(|((_, id), payload)| (*id, *payload))
            .collect()
    }

    /// The number of pending entries.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Whether the schedule holds no pending entries.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(raw: u64) -> Tick {
        Tick::new(raw)
    }

    #[test]
    fn new_and_default_are_empty() {
        let a: TickSchedule<u64, u32> = TickSchedule::new();
        let b: TickSchedule<u64, u32> = TickSchedule::default();
        assert!(a.is_empty());
        assert_eq!(a.len(), 0);
        assert!(b.is_empty());
    }

    #[test]
    fn schedule_pending_and_pop_in_tick_then_id_order() {
        let mut s: TickSchedule<u64, u32> = TickSchedule::new();
        s.schedule(2, at(10), 200);
        s.schedule(1, at(5), 100);
        s.schedule(3, at(5), 300);
        assert_eq!(s.len(), 3);
        assert!(!s.is_empty());
        assert_eq!(s.pending(1), Some(at(5)));
        assert_eq!(s.pending(2), Some(at(10)));
        // Peek does not consume; ordered (tick, id); the future entry excluded.
        assert_eq!(s.peek_due(at(5)), vec![(1, 100), (3, 300)]);
        assert_eq!(s.len(), 3);
        // Pop the two due-at-5 entries, ascending id; the tick-10 entry stays.
        let due = s.pop_due(at(5));
        assert_eq!(due, vec![(1, 100), (3, 300)]);
        assert_eq!(s.len(), 1);
        assert_eq!(s.pending(1), None);
        assert_eq!(s.pending(3), None);
        // Already-popped entries do not re-fire.
        assert_eq!(s.pop_due(at(5)), vec![]);
        // The future entry fires at its tick.
        assert_eq!(s.pop_due(at(10)), vec![(2, 200)]);
        assert!(s.is_empty());
    }

    #[test]
    fn rescheduling_replaces_the_single_pending_entry() {
        let mut s: TickSchedule<u64, u32> = TickSchedule::new();
        s.schedule(1, at(3), 10);
        s.schedule(1, at(8), 20);
        assert_eq!(s.len(), 1);
        assert_eq!(s.pending(1), Some(at(8)));
        // Not due at the old tick anymore.
        assert_eq!(s.pop_due(at(3)), vec![]);
        // Move earlier with a fresh payload.
        s.schedule(1, at(2), 30);
        assert_eq!(s.pending(1), Some(at(2)));
        assert_eq!(s.pop_due(at(2)), vec![(1, 30)]);
    }

    #[test]
    fn cancel_removes_and_is_clean_when_absent() {
        let mut s: TickSchedule<u64, u32> = TickSchedule::new();
        s.schedule(1, at(3), 10);
        assert!(s.cancel(1));
        assert_eq!(s.pending(1), None);
        assert!(s.is_empty());
        // Canceling an unknown / already-fired id is a clean false.
        assert!(!s.cancel(1));
        assert!(!s.cancel(99));
    }

    #[test]
    fn pop_due_on_empty_and_all_future_returns_nothing() {
        let mut s: TickSchedule<u64, u32> = TickSchedule::new();
        assert_eq!(s.pop_due(at(100)), vec![]);
        s.schedule(1, at(50), 1);
        // Everything is in the future: nothing due, the entry stays pending.
        assert_eq!(s.pop_due(at(10)), vec![]);
        assert_eq!(s.peek_due(at(10)), vec![]);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn debug_and_clone_are_available() {
        let mut s: TickSchedule<u64, u32> = TickSchedule::new();
        s.schedule(1, at(1), 7);
        let cloned = s.clone();
        assert_eq!(cloned.peek_due(at(1)), vec![(1, 7)]);
        assert!(format!("{s:?}").contains("TickSchedule"));
    }
}
