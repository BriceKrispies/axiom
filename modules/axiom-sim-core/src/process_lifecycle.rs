//! The process lifecycle: statuses and the legal transitions between them.

use crate::cause::CauseRef;
use crate::ids::ProcessId;
use crate::sim_tick::SimTick;

/// Where a process sits in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProcessStatus {
    /// Registered but not yet placed on the wake queue.
    Scheduled,
    /// Waiting on the wake queue for a future tick.
    Sleeping,
    /// Due to run on the current/next step.
    Ready,
    /// Executing its handler this step.
    Running,
    /// Finished; terminal.
    Completed,
    /// Aborted; terminal.
    Canceled,
    /// Errored (e.g. its effects failed); terminal.
    Failed,
}

impl ProcessStatus {
    /// The status's deterministic code.
    pub fn code(self) -> u8 {
        self as u8
    }
}

// Whether a transition `from -> to` is legal, indexed `[from][to]`.
// Terminal states (Completed/Canceled/Failed) permit no outgoing transition.
const LEGAL: [[bool; 7]; 7] = [
    //            Sched  Sleep  Ready  Run    Done   Cancel Fail
    /* Sched  */
    [false, true, true, false, false, true, false],
    /* Sleep  */ [false, false, true, false, false, true, false],
    /* Ready  */ [false, false, false, true, false, true, false],
    /* Run    */ [false, true, false, false, true, true, true],
    /* Done   */ [false, false, false, false, false, false, false],
    /* Cancel */ [false, false, false, false, false, false, false],
    /* Fail   */ [false, false, false, false, false, false, false],
];

/// A recorded status change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessTransition {
    from: ProcessStatus,
    to: ProcessStatus,
    cause: Option<CauseRef>,
    tick: SimTick,
}

impl ProcessTransition {
    /// The prior status.
    pub const fn from(&self) -> ProcessStatus {
        self.from
    }
    /// The new status.
    pub const fn to(&self) -> ProcessStatus {
        self.to
    }
    /// What caused the transition, if recorded.
    pub const fn cause(&self) -> Option<CauseRef> {
        self.cause
    }
    /// The tick the transition occurred on.
    pub const fn tick(&self) -> SimTick {
        self.tick
    }
}

/// A process's current lifecycle status, enforcing legal transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessLifecycle {
    status: ProcessStatus,
}

impl ProcessLifecycle {
    /// A freshly registered process (status [`ProcessStatus::Scheduled`]).
    pub const fn new() -> Self {
        ProcessLifecycle {
            status: ProcessStatus::Scheduled,
        }
    }

    /// The current status.
    pub const fn status(self) -> ProcessStatus {
        self.status
    }

    /// Whether the current status may transition to `to`.
    pub fn can_transition(self, to: ProcessStatus) -> bool {
        LEGAL[self.status.code() as usize][to.code() as usize]
    }

    /// Attempt a transition to `to` at `tick`. On success the status advances and
    /// a [`ProcessTransition`] record is returned; on an illegal transition the
    /// status is unchanged and `None` is returned (clean rejection).
    pub fn transition(
        &mut self,
        to: ProcessStatus,
        cause: Option<CauseRef>,
        tick: SimTick,
    ) -> Option<ProcessTransition> {
        let from = self.status;
        let legal = self.can_transition(to);
        legal.then(|| self.status = to);
        legal.then_some(ProcessTransition {
            from,
            to,
            cause,
            tick,
        })
    }
}

impl Default for ProcessLifecycle {
    fn default() -> Self {
        ProcessLifecycle::new()
    }
}

/// A record of one execution of a process by the scheduler: how many effects its
/// handler produced and the lifecycle [`ProcessTransition`] the boundary resolved
/// it through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessExecutionRecord {
    process: ProcessId,
    produced_effects: usize,
    transition: ProcessTransition,
}

impl ProcessExecutionRecord {
    /// Build an execution record.
    pub(crate) const fn new(
        process: ProcessId,
        produced_effects: usize,
        transition: ProcessTransition,
    ) -> Self {
        ProcessExecutionRecord {
            process,
            produced_effects,
            transition,
        }
    }
    /// The executed process.
    pub const fn process(&self) -> ProcessId {
        self.process
    }
    /// The tick it executed on (the transition tick).
    pub const fn tick(&self) -> SimTick {
        self.transition.tick()
    }
    /// How many effects its handler produced.
    pub const fn produced_effects(&self) -> usize {
        self.produced_effects
    }
    /// The lifecycle transition the boundary resolved it through (its `to()` is the
    /// final status).
    pub const fn transition(&self) -> ProcessTransition {
        self.transition
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_are_stable() {
        assert_eq!(ProcessStatus::Scheduled.code(), 0);
        assert_eq!(ProcessStatus::Ready.code(), 2);
        assert_eq!(ProcessStatus::Failed.code(), 6);
    }

    #[test]
    fn lifecycle_follows_legal_path() {
        let mut life = ProcessLifecycle::new();
        assert_eq!(life.status(), ProcessStatus::Scheduled);
        let t = life
            .transition(ProcessStatus::Sleeping, None, SimTick::new(0))
            .unwrap();
        assert_eq!(
            (t.from(), t.to()),
            (ProcessStatus::Scheduled, ProcessStatus::Sleeping)
        );
        assert!(life
            .transition(ProcessStatus::Ready, None, SimTick::new(1))
            .is_some());
        assert!(life
            .transition(
                ProcessStatus::Running,
                Some(CauseRef::Command),
                SimTick::new(2)
            )
            .is_some());
        let done = life
            .transition(ProcessStatus::Completed, None, SimTick::new(3))
            .unwrap();
        assert_eq!(done.to(), ProcessStatus::Completed);
        assert_eq!(done.tick(), SimTick::new(3));
        assert_eq!(done.cause(), None);
    }

    #[test]
    fn illegal_transitions_are_rejected_cleanly() {
        let mut life = ProcessLifecycle::new();
        assert!(!life.can_transition(ProcessStatus::Running));
        assert!(life
            .transition(ProcessStatus::Running, None, SimTick::new(0))
            .is_none());
        assert_eq!(
            life.status(),
            ProcessStatus::Scheduled,
            "status unchanged after a rejected transition"
        );
        life.transition(ProcessStatus::Canceled, None, SimTick::new(0));
        assert_eq!(life.status(), ProcessStatus::Canceled);
        assert!(life
            .transition(ProcessStatus::Ready, None, SimTick::new(1))
            .is_none());
    }

    #[test]
    fn running_may_sleep_fail_or_cancel() {
        let drive = |to: ProcessStatus| {
            let mut life = ProcessLifecycle::new();
            life.transition(ProcessStatus::Ready, None, SimTick::new(0));
            life.transition(ProcessStatus::Running, None, SimTick::new(0));
            life.transition(to, None, SimTick::new(1))
        };
        assert!(drive(ProcessStatus::Sleeping).is_some());
        assert!(drive(ProcessStatus::Failed).is_some());
        assert!(drive(ProcessStatus::Canceled).is_some());
        assert!(drive(ProcessStatus::Completed).is_some());
        assert!(drive(ProcessStatus::Scheduled).is_none());
    }

    #[test]
    fn execution_record_carries_fields_and_default_lifecycle() {
        let transition = ProcessTransition {
            from: ProcessStatus::Running,
            to: ProcessStatus::Completed,
            cause: Some(CauseRef::Command),
            tick: SimTick::new(4),
        };
        let record = ProcessExecutionRecord::new(ProcessId::from_raw(3), 2, transition);
        assert_eq!(record.process(), ProcessId::from_raw(3));
        assert_eq!(record.tick(), SimTick::new(4));
        assert_eq!(record.produced_effects(), 2);
        assert_eq!(record.transition().to(), ProcessStatus::Completed);
        assert_eq!(record.transition().from(), ProcessStatus::Running);
        assert_eq!(record.transition().cause(), Some(CauseRef::Command));
        assert_eq!(
            ProcessLifecycle::default().status(),
            ProcessStatus::Scheduled
        );
    }
}
