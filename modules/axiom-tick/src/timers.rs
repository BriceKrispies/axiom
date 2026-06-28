//! Tick-scheduled timers projected over the kernel's [`TickSchedule`].
//!
//! `Timers` is a thin projection of the generic kernel schedule: each timer is an
//! ascending [`TimerId`] armed to fire at a future tick, carrying a
//! `(interval, repeating)` payload. One-shot timers (`after`) fire once;
//! repeating timers (`every`) re-arm themselves at `now + interval` the moment
//! they fire. The current tick is supplied each call — the timers read no clock.

use axiom_kernel::{Tick, TickDelta, TickSchedule};

use crate::ids::TimerId;

/// A deterministic set of tick-scheduled timers.
///
/// The payload is `(interval, repeating)`: `interval` is the original delay (and,
/// for a repeating timer, its cadence), `repeating` selects one-shot vs.
/// self-rescheduling. Ids are minted ascending so two timers due on the same tick
/// fire in allocation order.
#[derive(Debug, Clone)]
pub struct Timers {
    schedule: TickSchedule<TimerId, (TickDelta, bool)>,
    next: u64,
}

impl Timers {
    /// An empty timer set. The first minted timer has id 1.
    pub fn new() -> Self {
        Timers {
            schedule: TickSchedule::new(),
            next: 1,
        }
    }

    /// Mint the next ascending timer id.
    fn mint(&mut self) -> TimerId {
        let id = TimerId::from_raw(self.next);
        self.next += 1;
        id
    }

    /// Arm a one-shot timer to fire `ticks` after `now`. `after(0)` fires on the
    /// next [`due`](Timers::due) at `now`.
    pub fn after(&mut self, now: Tick, ticks: TickDelta) -> TimerId {
        let id = self.mint();
        self.schedule.schedule(id, now.add(ticks), (ticks, false));
        id
    }

    /// Arm a repeating timer with cadence `ticks` (clamped to `>= 1` so it can
    /// never busy-fire within a tick), first firing `interval` after `now`.
    pub fn every(&mut self, now: Tick, ticks: TickDelta) -> TimerId {
        let interval = TickDelta::new(ticks.raw().max(1));
        let id = self.mint();
        self.schedule.schedule(id, now.add(interval), (interval, true));
        id
    }

    /// Cancel a timer. Returns whether it was pending — a clean `false` for an
    /// unknown or already-fired id.
    pub fn cancel(&mut self, timer: TimerId) -> bool {
        self.schedule.cancel(timer)
    }

    /// The timers firing at or before `now`, ascending by `(tick, id)`. Repeating
    /// timers re-arm themselves at `now + interval` here, so the returned ids are
    /// the one-shot-and-this-cycle fires; a repeating timer's next fire is no
    /// earlier than `now + 1`, never within this same pass.
    pub fn due(&mut self, now: Tick) -> Vec<TimerId> {
        let fired = self.schedule.pop_due(now);
        fired.iter().for_each(|&(id, (interval, repeating))| {
            repeating.then(|| {
                self.schedule
                    .schedule(id, now.add(interval), (interval, repeating))
            });
        });
        fired.into_iter().map(|(id, _)| id).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(raw: u64) -> Tick {
        Tick::new(raw)
    }

    fn delta(raw: u64) -> TickDelta {
        TickDelta::new(raw)
    }

    #[test]
    fn after_fires_once_at_the_deadline() {
        let mut timers = Timers::new();
        let id = timers.after(at(10), delta(3));
        assert_eq!(id, TimerId::from_raw(1));
        // Not due before the deadline.
        assert_eq!(timers.due(at(12)), vec![]);
        // Fires exactly at now + 3, then never again.
        assert_eq!(timers.due(at(13)), vec![id]);
        assert_eq!(timers.due(at(13)), vec![]);
        assert_eq!(timers.due(at(100)), vec![]);
    }

    #[test]
    fn after_zero_fires_on_the_current_tick() {
        let mut timers = Timers::new();
        let id = timers.after(at(5), delta(0));
        assert_eq!(timers.due(at(5)), vec![id]);
    }

    #[test]
    fn every_reschedules_no_earlier_than_next_tick() {
        let mut timers = Timers::new();
        let id = timers.every(at(0), delta(2));
        // First fire at 0 + 2.
        assert_eq!(timers.due(at(2)), vec![id]);
        // Re-armed at 2 + 2 = 4; not due again until then.
        assert_eq!(timers.due(at(3)), vec![]);
        assert_eq!(timers.due(at(4)), vec![id]);
    }

    #[test]
    fn every_clamps_zero_cadence_to_one() {
        let mut timers = Timers::new();
        let id = timers.every(at(0), delta(0));
        // Clamped to interval 1: first fire at tick 1, not a busy-fire at tick 0.
        assert_eq!(timers.due(at(0)), vec![]);
        assert_eq!(timers.due(at(1)), vec![id]);
        assert_eq!(timers.due(at(2)), vec![id]);
    }

    #[test]
    fn simultaneous_timers_fire_in_ascending_id_order() {
        let mut timers = Timers::new();
        let a = timers.after(at(0), delta(5));
        let b = timers.after(at(0), delta(5));
        assert_eq!(a, TimerId::from_raw(1));
        assert_eq!(b, TimerId::from_raw(2));
        assert_eq!(timers.due(at(5)), vec![a, b]);
    }

    #[test]
    fn cancel_removes_and_is_clean_when_absent() {
        let mut timers = Timers::new();
        let id = timers.after(at(0), delta(3));
        assert!(timers.cancel(id));
        assert_eq!(timers.due(at(3)), vec![]);
        assert!(!timers.cancel(id));
        assert!(!timers.cancel(TimerId::from_raw(999)));
    }

    #[test]
    fn debug_and_clone_are_available() {
        let mut timers = Timers::new();
        timers.after(at(0), delta(1));
        let cloned = timers.clone();
        assert!(format!("{cloned:?}").contains("Timers"));
    }
}
