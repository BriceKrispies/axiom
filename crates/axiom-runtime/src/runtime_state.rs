//! The lifecycle state of a [`crate::runtime::Runtime`].

/// Every valid lifecycle state of a runtime.
///
/// Transitions are enforced by [`crate::runtime::Runtime`]; any illegal one
/// returns [`crate::runtime_error_code::RuntimeErrorCode::InvalidLifecycleTransition`].
///
/// Discriminants are **appended, never renumbered**, because
/// [`RuntimeState::raw`] is a stable identity byte surfaced through
/// [`crate::runtime_step_record::RuntimeStepRecord::state_after`]. That makes
/// the numbering an insertion history, not a lifecycle ranking — which is why
/// this enum deliberately does **not** derive `PartialOrd`/`Ord`. Nothing in
/// the engine orders or sorts a `RuntimeState`; comparing two of them for
/// progression would read a total order that does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RuntimeState {
    /// The runtime exists but has not been initialized.
    Created = 0,
    /// `initialize` succeeded; ready to prepare.
    Initialized = 1,
    /// `start` succeeded; `step` is allowed.
    Running = 2,
    /// Temporarily paused; `step` is rejected, `start` resumes.
    Paused = 3,
    /// Terminal state reached via `stop`.
    Stopped = 4,
    /// Terminal state reached via a system failure or unrecoverable error.
    Failed = 5,
    /// Every task in the startup preparation phase completed successfully;
    /// ready to start. Reached from `Initialized`, and — despite its higher
    /// discriminant — sits *before* `Running` in the lifecycle.
    Prepared = 6,
}

impl RuntimeState {
    /// Whether this state is terminal (no further transitions are possible
    /// other than reading or dropping the runtime).
    pub const fn is_terminal(self) -> bool {
        (self as u8 == RuntimeState::Stopped as u8) | (self as u8 == RuntimeState::Failed as u8)
    }

    /// The stable numeric discriminant.
    ///
    /// This byte is an **identity**, not a rank. Variants are numbered in the
    /// order they were added to the enum, so a larger `raw()` says nothing
    /// about lifecycle progression — `Prepared` is `6` yet precedes `Running`
    /// (`2`). Compare states with `==`; never with `<`.
    pub const fn raw(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_are_stable() {
        assert_eq!(RuntimeState::Created.raw(), 0);
        assert_eq!(RuntimeState::Initialized.raw(), 1);
        assert_eq!(RuntimeState::Running.raw(), 2);
        assert_eq!(RuntimeState::Paused.raw(), 3);
        assert_eq!(RuntimeState::Stopped.raw(), 4);
        assert_eq!(RuntimeState::Failed.raw(), 5);
        assert_eq!(RuntimeState::Prepared.raw(), 6);
    }

    #[test]
    fn terminal_states_are_stopped_or_failed() {
        assert!(RuntimeState::Stopped.is_terminal());
        assert!(RuntimeState::Failed.is_terminal());
        for s in [
            RuntimeState::Created,
            RuntimeState::Initialized,
            RuntimeState::Prepared,
            RuntimeState::Running,
            RuntimeState::Paused,
        ] {
            assert!(!s.is_terminal());
        }
    }
}
