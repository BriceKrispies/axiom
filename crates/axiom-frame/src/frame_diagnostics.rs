//! Per-frame deterministic diagnostic summary.

use axiom_host::HostSkipReason;

use crate::frame_lifecycle_state::FrameLifecycleState;

/// Deterministic per-frame diagnostic summary.
///
/// Plain data. Carries the small set of facts a future debug overlay,
/// replay tool, or test harness needs to reason about a frame **without**
/// walking the runtime step records or the host plan. The frame layer
/// itself does not log to a console; this struct is the only diagnostic
/// surface it owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameDiagnostics {
    skipped: bool,
    skip_reason: Option<HostSkipReason>,
    runtime_step_count: u32,
    command_count: u32,
    validation_failure_count: u32,
    lifecycle: FrameLifecycleState,
}

impl FrameDiagnostics {
    /// Build a diagnostics summary from explicit, already-validated values.
    pub const fn new(
        skipped: bool,
        skip_reason: Option<HostSkipReason>,
        runtime_step_count: u32,
        command_count: u32,
        validation_failure_count: u32,
        lifecycle: FrameLifecycleState,
    ) -> Self {
        FrameDiagnostics {
            skipped,
            skip_reason,
            runtime_step_count,
            command_count,
            validation_failure_count,
            lifecycle,
        }
    }

    pub const fn skipped(&self) -> bool {
        self.skipped
    }

    pub const fn skip_reason(&self) -> Option<HostSkipReason> {
        self.skip_reason
    }

    pub const fn runtime_step_count(&self) -> u32 {
        self.runtime_step_count
    }

    pub const fn command_count(&self) -> u32 {
        self.command_count
    }

    pub const fn validation_failure_count(&self) -> u32 {
        self.validation_failure_count
    }

    pub const fn lifecycle(&self) -> FrameLifecycleState {
        self.lifecycle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip_constructed_values() {
        let d = FrameDiagnostics::new(false, None, 3, 2, 0, FrameLifecycleState::Active);
        assert!(!d.skipped());
        assert!(d.skip_reason().is_none());
        assert_eq!(d.runtime_step_count(), 3);
        assert_eq!(d.command_count(), 2);
        assert_eq!(d.validation_failure_count(), 0);
        assert_eq!(d.lifecycle(), FrameLifecycleState::Active);
    }

    #[test]
    fn skipped_diagnostics_are_explicit() {
        let d = FrameDiagnostics::new(
            true,
            Some(HostSkipReason::LifecycleHidden),
            0,
            0,
            0,
            FrameLifecycleState::Hidden,
        );
        assert!(d.skipped());
        assert_eq!(d.skip_reason(), Some(HostSkipReason::LifecycleHidden));
    }

    #[test]
    fn identical_inputs_produce_identical_diagnostics() {
        let a = FrameDiagnostics::new(false, None, 2, 1, 0, FrameLifecycleState::Active);
        let b = FrameDiagnostics::new(false, None, 2, 1, 0, FrameLifecycleState::Active);
        assert_eq!(a, b);
    }

    #[test]
    fn validation_failure_count_is_independent() {
        let a = FrameDiagnostics::new(false, None, 1, 0, 0, FrameLifecycleState::Active);
        let b = FrameDiagnostics::new(false, None, 1, 0, 7, FrameLifecycleState::Active);
        assert_ne!(a, b);
        assert_eq!(b.validation_failure_count(), 7);
    }

    #[test]
    fn command_count_is_preserved() {
        let d = FrameDiagnostics::new(false, None, 0, 42, 0, FrameLifecycleState::Active);
        assert_eq!(d.command_count(), 42);
    }
}
