//! A state-machine transition event, the data a per-tick drain hands back.
//!
//! `drain_events` returns `Vec<StateEvent>`; the runtime app turns each into the
//! author's `onEnter` / `onUpdate` / `onExit` closure invocation. The event is
//! pure data — no closure lives in sim state.

use crate::ids::StateMachineId;

/// Which kind of state-machine event this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateEventKind {
    /// The machine entered the carried state this tick.
    Enter,
    /// The machine remained in the carried state this tick.
    Update,
    /// The machine left the carried state this tick.
    Exit,
}

/// One state-machine event: which machine, what happened, and the state index it
/// concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateEvent {
    machine: StateMachineId,
    kind: StateEventKind,
    state: u32,
}

impl StateEvent {
    /// An `Enter(state)` event for `machine`.
    pub(crate) const fn enter(machine: StateMachineId, state: u32) -> Self {
        StateEvent {
            machine,
            kind: StateEventKind::Enter,
            state,
        }
    }

    /// An `Update(state)` event for `machine`.
    pub(crate) const fn update(machine: StateMachineId, state: u32) -> Self {
        StateEvent {
            machine,
            kind: StateEventKind::Update,
            state,
        }
    }

    /// An `Exit(state)` event for `machine`.
    pub(crate) const fn exit(machine: StateMachineId, state: u32) -> Self {
        StateEvent {
            machine,
            kind: StateEventKind::Exit,
            state,
        }
    }

    /// The machine this event concerns.
    pub const fn machine(self) -> StateMachineId {
        self.machine
    }

    /// Which kind of event this is.
    pub const fn kind(self) -> StateEventKind {
        self.kind
    }

    /// The dense state index this event concerns.
    pub const fn state(self) -> u32 {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_and_accessors_round_trip() {
        let m = StateMachineId::from_raw(2);
        let enter = StateEvent::enter(m, 1);
        let update = StateEvent::update(m, 1);
        let exit = StateEvent::exit(m, 0);
        assert_eq!(enter.machine(), m);
        assert_eq!(enter.kind(), StateEventKind::Enter);
        assert_eq!(enter.state(), 1);
        assert_eq!(update.kind(), StateEventKind::Update);
        assert_eq!(exit.kind(), StateEventKind::Exit);
        assert_eq!(exit.state(), 0);
        assert_eq!(enter, enter.clone());
        assert_ne!(enter, update);
        assert!(format!("{exit:?}").contains("Exit"));
    }
}
