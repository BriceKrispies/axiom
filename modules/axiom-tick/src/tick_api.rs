//! The single behavioral facade: timers + tick-driven state machines.

use axiom_kernel::{Tick, TickDelta};

use crate::ids::{StateMachineId, TimerId};
use crate::machines::Machines;
use crate::state_event::StateEvent;
use crate::timers::Timers;

/// The game-API timers and state-machine facade.
/// `TickApi` is the deterministic, wall-clock-free half of "time & state": tick
/// scheduled `after` / `every` / `cancel` timers and author-defined state
/// machines, both projected over the kernel's `TickSchedule` / `Tick` / `TickDelta`.
/// The current tick is **supplied** on each call, so the facade reads no clock and
/// owns no domain meaning. [`due`](TickApi::due) and
/// [`drain_events`](TickApi::drain_events) return *data* (fired timer ids, state
/// events); the runtime app turns that data into the author's timer and
/// `onEnter`/`onUpdate`/`onExit` closures — no closure is stored in sim state.
#[derive(Debug)]
pub struct TickApi {
    timers: Timers,
    machines: Machines,
}

impl TickApi {
    /// An empty facade: no timers, no machines.
    pub fn new() -> Self {
        TickApi {
            timers: Timers::new(),
            machines: Machines::new(),
        }
    }

    /// Schedule a one-shot timer to fire `ticks` after `now`.
    pub fn after(&mut self, now: Tick, ticks: TickDelta) -> TimerId {
        self.timers.after(now, ticks)
    }

    /// Schedule a repeating timer with cadence `ticks` (clamped to `>= 1`).
    pub fn every(&mut self, now: Tick, ticks: TickDelta) -> TimerId {
        self.timers.every(now, ticks)
    }

    /// Cancel a timer; `false` if it was unknown or already fired.
    pub fn cancel(&mut self, timer: TimerId) -> bool {
        self.timers.cancel(timer)
    }

    /// The timers firing at or before `now`, ascending by `(tick, id)`; repeating
    /// timers re-arm themselves here.
    pub fn due(&mut self, now: Tick) -> Vec<TimerId> {
        self.timers.due(now)
    }

    /// Create a state machine of `states` dense states starting in `initial` at
    /// `now`.
    pub fn create_machine(&mut self, states: u32, initial: u32, now: Tick) -> StateMachineId {
        self.machines.create(states, initial, now)
    }

    /// Transition machine `m` to `to` at `now`.
    pub fn transition(&mut self, m: StateMachineId, to: u32, now: Tick) {
        self.machines.transition(m, to, now)
    }

    /// The current state of machine `m`, if it exists.
    pub fn current(&self, m: StateMachineId) -> Option<u32> {
        self.machines.current(m)
    }

    /// The ticks machine `m` has been in its current state as of `now`.
    pub fn ticks_in_state(&self, m: StateMachineId, now: Tick) -> Option<TickDelta> {
        self.machines.ticks_in_state(m, now)
    }

    /// The state events at `now`, in deterministic `(machine id, Exit-before-Enter)`
    /// order.
    pub fn drain_events(&mut self, now: Tick) -> Vec<StateEvent> {
        self.machines.drain_events(now)
    }
}

impl Default for TickApi {
    fn default() -> Self {
        TickApi::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_event::StateEventKind;

    fn at(raw: u64) -> Tick {
        Tick::new(raw)
    }

    fn delta(raw: u64) -> TickDelta {
        TickDelta::new(raw)
    }

    fn run_timers() -> Vec<(u64, Vec<TimerId>)> {
        let mut api = TickApi::new();
        let one_shot = api.after(at(0), delta(3));
        let repeating = api.every(at(0), delta(5));
        let canceled = api.after(at(0), delta(4));
        assert!(api.cancel(canceled));
        (0..12)
            .map(|t| (t, api.due(at(t))))
            .filter(|(_, fired)| !fired.is_empty())
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(t, fired)| {
                assert!(!fired.contains(&canceled));
                let _ = (one_shot, repeating);
                (t, fired)
            })
            .collect()
    }

    #[test]
    fn timer_sequence_is_deterministic_and_replayable() {
        let first = run_timers();
        let second = run_timers();
        assert_eq!(
            first, second,
            "timer fire sequence must replay byte-identically"
        );
        assert_eq!(
            first,
            vec![
                (3, vec![TimerId::from_raw(1)]),
                (5, vec![TimerId::from_raw(2)]),
                (10, vec![TimerId::from_raw(2)]),
            ]
        );
    }

    fn run_machine() -> Vec<(u64, Vec<(u32, StateEventKind)>)> {
        let mut api = TickApi::new();
        let m = api.create_machine(3, 0, at(0));
        (0..6)
            .map(|t| {
                (t == 3).then(|| api.transition(m, 2, at(t)));
                let events: Vec<(u32, StateEventKind)> = api
                    .drain_events(at(t))
                    .into_iter()
                    .map(|e| (e.state(), e.kind()))
                    .collect();
                (t, events)
            })
            .collect()
    }

    #[test]
    fn machine_event_stream_is_deterministic_and_replayable() {
        let first = run_machine();
        let second = run_machine();
        assert_eq!(
            first, second,
            "state event stream must replay byte-identically"
        );
        assert_eq!(
            first,
            vec![
                (0, vec![(0, StateEventKind::Enter)]),
                (1, vec![(0, StateEventKind::Update)]),
                (2, vec![(0, StateEventKind::Update)]),
                (
                    3,
                    vec![(0, StateEventKind::Exit), (2, StateEventKind::Enter)]
                ),
                (4, vec![(2, StateEventKind::Update)]),
                (5, vec![(2, StateEventKind::Update)]),
            ]
        );
    }

    #[test]
    fn default_is_an_empty_facade() {
        let mut api = TickApi::default();
        assert_eq!(api.due(at(100)), vec![]);
        assert_eq!(api.current(StateMachineId::from_raw(1)), None);
    }

    #[test]
    fn facade_queries_and_debug() {
        let mut api = TickApi::new();
        let m = api.create_machine(4, 1, at(2));
        assert_eq!(api.current(m), Some(1));
        assert_eq!(api.ticks_in_state(m, at(7)), Some(TickDelta::new(5)));
        assert_eq!(api.current(StateMachineId::from_raw(42)), None);
        assert!(format!("{api:?}").contains("TickApi"));
    }
}
