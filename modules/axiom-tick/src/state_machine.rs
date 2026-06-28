//! One author-defined, tick-driven state machine: pure `(current, entered)` data.
//!
//! A machine is a dense state index plus the tick it entered that state, bounded
//! by a declared state count. Its events are **derived** from that data against
//! the supplied tick — never stored — so two machines fed the same transitions
//! produce identical event streams on replay. On the tick a state is entered
//! (creation or transition) it emits `Enter` (and, for a transition, `Exit` of
//! the previous state); on every later tick it emits `Update`.

use axiom_kernel::{Tick, TickDelta};

use crate::ids::StateMachineId;
use crate::state_event::StateEvent;

/// A single state machine's pure data.
#[derive(Debug, Clone)]
pub struct StateMachine {
    states: u32,
    prev: Option<u32>,
    current: u32,
    entered: Tick,
}

/// Clamp a requested state index into `[0, states)` so the machine is total — an
/// out-of-range index lands on the last valid state rather than escaping the
/// declared count.
fn clamp(index: u32, states: u32) -> u32 {
    index.min(states.saturating_sub(1))
}

impl StateMachine {
    /// A machine of `states` dense states, starting in `initial` at `now`.
    pub fn new(states: u32, initial: u32, now: Tick) -> Self {
        StateMachine {
            states,
            prev: None,
            current: clamp(initial, states),
            entered: now,
        }
    }

    /// Transition to `to` at `now`, recording the previous state and resetting
    /// the entered tick. A self-transition (`to == current`) re-emits
    /// `Exit`+`Enter` and resets `ticks_in_state` — one documented rule.
    pub fn transition(&mut self, to: u32, now: Tick) {
        self.prev = Some(self.current);
        self.current = clamp(to, self.states);
        self.entered = now;
    }

    /// The current dense state index.
    pub fn current(&self) -> u32 {
        self.current
    }

    /// How many ticks the machine has been in its current state as of `now`.
    pub fn ticks_in_state(&self, now: Tick) -> TickDelta {
        now.delta_since(self.entered)
    }

    /// The events this machine emits at `now`: `Exit`(prev)+`Enter`(current) on
    /// the tick it was entered (just `Enter` on the creation tick, which has no
    /// previous state), or `Update`(current) on any other tick.
    pub fn drain(&self, id: StateMachineId, now: Tick) -> Vec<StateEvent> {
        let entered_now = self.entered == now;
        let exit = entered_now
            .then_some(self.prev.map(|p| StateEvent::exit(id, p)))
            .flatten();
        let enter = entered_now.then_some(StateEvent::enter(id, self.current));
        let update = (!entered_now).then_some(StateEvent::update(id, self.current));
        exit.into_iter().chain(enter).chain(update).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_event::StateEventKind;

    fn at(raw: u64) -> Tick {
        Tick::new(raw)
    }

    fn id() -> StateMachineId {
        StateMachineId::from_raw(1)
    }

    #[test]
    fn creation_emits_only_enter_on_its_tick() {
        let m = StateMachine::new(3, 0, at(4));
        let events = m.drain(id(), at(4));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], StateEvent::enter(id(), 0));
    }

    #[test]
    fn later_ticks_emit_update() {
        let m = StateMachine::new(3, 2, at(4));
        let events = m.drain(id(), at(7));
        assert_eq!(events, vec![StateEvent::update(id(), 2)]);
        assert_eq!(m.current(), 2);
        assert_eq!(m.ticks_in_state(at(7)), TickDelta::new(3));
    }

    #[test]
    fn transition_emits_exit_then_enter_on_its_tick() {
        let mut m = StateMachine::new(3, 0, at(0));
        m.transition(1, at(5));
        let events = m.drain(id(), at(5));
        assert_eq!(
            events,
            vec![StateEvent::exit(id(), 0), StateEvent::enter(id(), 1)]
        );
        assert_eq!(events[0].kind(), StateEventKind::Exit);
        assert_eq!(m.current(), 1);
        assert_eq!(m.ticks_in_state(at(5)), TickDelta::new(0));
    }

    #[test]
    fn self_transition_re_emits_and_resets() {
        let mut m = StateMachine::new(2, 1, at(0));
        m.transition(1, at(9));
        let events = m.drain(id(), at(9));
        assert_eq!(
            events,
            vec![StateEvent::exit(id(), 1), StateEvent::enter(id(), 1)]
        );
        assert_eq!(m.ticks_in_state(at(9)), TickDelta::new(0));
    }

    #[test]
    fn out_of_range_indices_clamp_to_the_last_state() {
        // initial 5 with only 3 states clamps to 2.
        let mut m = StateMachine::new(3, 5, at(0));
        assert_eq!(m.current(), 2);
        // transition past the range clamps too.
        m.transition(9, at(1));
        assert_eq!(m.current(), 2);
    }
}
