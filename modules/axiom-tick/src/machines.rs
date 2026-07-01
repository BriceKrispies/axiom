//! The state-machine collection: ascending [`StateMachineId`]s over ordered data.
//!
//! Machines live in a `BTreeMap` keyed by their ascending id, so iteration — and
//! therefore the per-tick event order across machines — is deterministically id
//! ordered. Ids are minted ascending; the tick is supplied each call.

use std::collections::BTreeMap;

use axiom_kernel::{Tick, TickDelta};

use crate::ids::StateMachineId;
use crate::state_event::StateEvent;
use crate::state_machine::StateMachine;

/// A deterministic collection of author-defined state machines.
#[derive(Debug, Clone)]
pub struct Machines {
    machines: BTreeMap<StateMachineId, StateMachine>,
    next: u64,
}

impl Machines {
    /// An empty collection. The first created machine has id 1.
    pub fn new() -> Self {
        Machines {
            machines: BTreeMap::new(),
            next: 1,
        }
    }

    /// Create a machine of `states` dense states starting in `initial` at `now`.
    pub fn create(&mut self, states: u32, initial: u32, now: Tick) -> StateMachineId {
        let id = StateMachineId::from_raw(self.next);
        self.next += 1;
        self.machines
            .insert(id, StateMachine::new(states, initial, now));
        id
    }

    /// Transition machine `id` to `to` at `now`. A no-op for an unknown id.
    pub fn transition(&mut self, id: StateMachineId, to: u32, now: Tick) {
        self.machines
            .get_mut(&id)
            .into_iter()
            .for_each(|m| m.transition(to, now));
    }

    /// The current state of machine `id`, if it exists.
    pub fn current(&self, id: StateMachineId) -> Option<u32> {
        self.machines.get(&id).map(StateMachine::current)
    }

    /// The ticks machine `id` has been in its current state as of `now`, if it
    /// exists.
    pub fn ticks_in_state(&self, id: StateMachineId, now: Tick) -> Option<TickDelta> {
        self.machines.get(&id).map(|m| m.ticks_in_state(now))
    }

    /// The events every machine emits at `now`, ordered by ascending machine id
    /// (and `Exit` before `Enter` within a transitioning machine).
    pub fn drain_events(&self, now: Tick) -> Vec<StateEvent> {
        self.machines
            .iter()
            .flat_map(|(id, machine)| machine.drain(*id, now))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_event::StateEvent;

    fn at(raw: u64) -> Tick {
        Tick::new(raw)
    }

    #[test]
    fn create_and_query_machines() {
        let mut machines = Machines::new();
        let a = machines.create(3, 0, at(0));
        let b = machines.create(2, 1, at(0));
        assert_eq!(a, StateMachineId::from_raw(1));
        assert_eq!(b, StateMachineId::from_raw(2));
        assert_eq!(machines.current(a), Some(0));
        assert_eq!(machines.current(b), Some(1));
        assert_eq!(machines.ticks_in_state(a, at(4)), Some(TickDelta::new(4)));
        let unknown = StateMachineId::from_raw(99);
        assert_eq!(machines.current(unknown), None);
        assert_eq!(machines.ticks_in_state(unknown, at(4)), None);
    }

    #[test]
    fn transition_is_a_noop_for_unknown_ids() {
        let mut machines = Machines::new();
        let a = machines.create(3, 0, at(0));
        machines.transition(StateMachineId::from_raw(99), 1, at(1));
        assert_eq!(machines.current(a), Some(0));
        machines.transition(a, 2, at(1));
        assert_eq!(machines.current(a), Some(2));
    }

    #[test]
    fn drain_events_are_ordered_by_machine_id() {
        let mut machines = Machines::new();
        let a = machines.create(3, 0, at(0));
        let b = machines.create(3, 1, at(0));
        let created = machines.drain_events(at(0));
        assert_eq!(
            created,
            vec![StateEvent::enter(a, 0), StateEvent::enter(b, 1)]
        );
        let updates = machines.drain_events(at(1));
        assert_eq!(
            updates,
            vec![StateEvent::update(a, 0), StateEvent::update(b, 1)]
        );
    }

    #[test]
    fn debug_and_clone_are_available() {
        let mut machines = Machines::new();
        machines.create(2, 0, at(0));
        let cloned = machines.clone();
        assert!(format!("{cloned:?}").contains("Machines"));
    }
}
