//! Replay/audit-friendly record of one completed runtime step.

use crate::runtime_diagnostics::RuntimeDiagnostics;
use crate::runtime_state::RuntimeState;
use crate::runtime_step::RuntimeStep;

/// A replay-ready summary of one completed runtime step.
///
/// Plain data: identity of the step, the diagnostics gathered during it, the
/// resulting runtime state, and how many commands/events remain in the queues
/// *after* the step's drain boundary (always zero in the current design — kept
/// in the schema so future layers that drain at different boundaries do not
/// have to change the record shape).
#[derive(Debug, Clone)]
pub struct RuntimeStepRecord {
    step: RuntimeStep,
    diagnostics: RuntimeDiagnostics,
    state_after: RuntimeState,
    commands_remaining: usize,
    events_remaining: usize,
}

impl RuntimeStepRecord {
    pub fn new(
        step: RuntimeStep,
        diagnostics: RuntimeDiagnostics,
        state_after: RuntimeState,
        commands_remaining: usize,
        events_remaining: usize,
    ) -> Self {
        RuntimeStepRecord {
            step,
            diagnostics,
            state_after,
            commands_remaining,
            events_remaining,
        }
    }

    pub fn step(&self) -> RuntimeStep {
        self.step
    }

    pub fn diagnostics(&self) -> &RuntimeDiagnostics {
        &self.diagnostics
    }

    pub fn state_after(&self) -> RuntimeState {
        self.state_after
    }

    pub fn commands_remaining(&self) -> usize {
        self.commands_remaining
    }

    pub fn events_remaining(&self) -> usize {
        self.events_remaining
    }

    /// `true` iff every system in this step succeeded.
    pub fn succeeded(&self) -> bool {
        self.diagnostics.errors().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_error::RuntimeError;
    use crate::runtime_error_code::RuntimeErrorCode;
    use crate::system_outcome::SystemOutcome;
    use axiom_kernel::{FrameIndex, HandleId, Tick};

    fn step() -> RuntimeStep {
        RuntimeStep::new(FrameIndex::new(1), Tick::new(1), 1_000, 1)
    }

    #[test]
    fn fields_round_trip_through_accessors() {
        let r = RuntimeStepRecord::new(
            step(),
            RuntimeDiagnostics::new(step()),
            RuntimeState::Running,
            0,
            0,
        );
        assert_eq!(r.step().tick(), Tick::new(1));
        assert_eq!(r.state_after(), RuntimeState::Running);
        assert_eq!(r.commands_remaining(), 0);
        assert_eq!(r.events_remaining(), 0);
        assert!(r.succeeded());
    }

    #[test]
    fn succeeded_is_false_when_diagnostics_have_errors() {
        let mut d = RuntimeDiagnostics::new(step());
        d.record_outcomes(vec![SystemOutcome::new(
            HandleId::from_raw(1),
            "boom",
            1,
            Err(RuntimeError::new(RuntimeErrorCode::SystemFailed, "x")),
        )]);
        let r = RuntimeStepRecord::new(step(), d, RuntimeState::Failed, 0, 0);
        assert!(!r.succeeded());
    }
}
