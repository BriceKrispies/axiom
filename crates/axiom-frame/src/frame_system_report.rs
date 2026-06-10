//! Per-system outcome carried into the engine frame.

use axiom_runtime::SystemOutcome;

/// A frame-owned, value-typed summary of one runtime [`SystemOutcome`].
///
/// The runtime records, for every system it ran in a step, the system's
/// stable id, its static name, the order it was registered with, and whether
/// it succeeded (and if not, the error code). Layer 04 used to discard all of
/// this at the frame boundary; carrying it here is what lets a future
/// introspection layer answer "which systems ran, in order, and which one
/// failed and why" without reaching around the frame contract into the
/// runtime.
///
/// `error_code` is `None` for a system that succeeded, and `Some(code)` —
/// the raw [`axiom_runtime::RuntimeErrorCode`] value — for one that failed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FrameSystemReport {
    system_id: u64,
    name: &'static str,
    order: i32,
    succeeded: bool,
    error_code: Option<u16>,
}

impl FrameSystemReport {
    /// Summarize one runtime system outcome.
    pub fn from_outcome(outcome: &SystemOutcome) -> Self {
        FrameSystemReport {
            system_id: outcome.id().raw(),
            name: outcome.name(),
            order: outcome.order(),
            succeeded: outcome.succeeded(),
            error_code: outcome.result().as_ref().err().map(|e| e.code().raw()),
        }
    }

    /// The stable kernel handle id of the system, as a raw `u64`.
    pub const fn system_id(&self) -> u64 {
        self.system_id
    }

    /// The system's static registered name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The order value the system was registered with.
    pub const fn order(&self) -> i32 {
        self.order
    }

    /// Whether the system's `run` returned `Ok`.
    pub const fn succeeded(&self) -> bool {
        self.succeeded
    }

    /// The raw runtime error code if the system failed, else `None`.
    pub const fn error_code(&self) -> Option<u16> {
        self.error_code
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::HandleId;
    use axiom_runtime::{RuntimeError, RuntimeErrorCode};

    #[test]
    fn success_outcome_has_no_error_code() {
        let outcome = SystemOutcome::new(HandleId::from_raw(7), "physics", 10, Ok(()));
        let report = FrameSystemReport::from_outcome(&outcome);
        assert_eq!(report.system_id(), 7);
        assert_eq!(report.name(), "physics");
        assert_eq!(report.order(), 10);
        assert!(report.succeeded());
        assert_eq!(report.error_code(), None);
    }

    #[test]
    fn failed_outcome_carries_raw_error_code() {
        let outcome = SystemOutcome::new(
            HandleId::from_raw(3),
            "boom",
            -2,
            Err(RuntimeError::new(RuntimeErrorCode::SystemFailed, "x")),
        );
        let report = FrameSystemReport::from_outcome(&outcome);
        assert_eq!(report.system_id(), 3);
        assert_eq!(report.name(), "boom");
        assert_eq!(report.order(), -2);
        assert!(!report.succeeded());
        assert_eq!(
            report.error_code(),
            Some(RuntimeErrorCode::SystemFailed.raw())
        );
    }

    #[test]
    fn identical_outcomes_produce_equal_reports() {
        let make = || SystemOutcome::new(HandleId::from_raw(1), "a", 1, Ok(()));
        assert_eq!(
            FrameSystemReport::from_outcome(&make()),
            FrameSystemReport::from_outcome(&make())
        );
    }
}
