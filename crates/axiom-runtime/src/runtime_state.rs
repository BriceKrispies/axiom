//! The lifecycle state of a [`crate::runtime::Runtime`].

/// Every valid lifecycle state of a runtime.
///
/// Transitions are enforced by [`crate::runtime::Runtime`]; any illegal one
/// returns [`crate::runtime_error_code::RuntimeErrorCode::InvalidLifecycleTransition`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum RuntimeState {
    /// The runtime exists but has not been initialized.
    Created = 0,
    /// `initialize` succeeded; ready to start.
    Initialized = 1,
    /// `start` succeeded; `step` is allowed.
    Running = 2,
    /// Temporarily paused; `step` is rejected, `start` resumes.
    Paused = 3,
    /// Terminal state reached via `stop`.
    Stopped = 4,
    /// Terminal state reached via a system failure or unrecoverable error.
    Failed = 5,
}

impl RuntimeState {
    /// Whether this state is terminal (no further transitions are possible
    /// other than reading or dropping the runtime).
    pub const fn is_terminal(self) -> bool {
        matches!(self, RuntimeState::Stopped | RuntimeState::Failed)
    }

    /// The stable numeric discriminant.
    pub const fn raw(self) -> u8 {
        self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_are_stable_and_ordered() {
        assert_eq!(RuntimeState::Created.raw(), 0);
        assert_eq!(RuntimeState::Failed.raw(), 5);
        assert!(RuntimeState::Created < RuntimeState::Running);
    }

    #[test]
    fn terminal_states_are_stopped_or_failed() {
        assert!(RuntimeState::Stopped.is_terminal());
        assert!(RuntimeState::Failed.is_terminal());
        for s in [
            RuntimeState::Created,
            RuntimeState::Initialized,
            RuntimeState::Running,
            RuntimeState::Paused,
        ] {
            assert!(!s.is_terminal());
        }
    }
}
