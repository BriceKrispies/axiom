//! A state failure: a machine identity, the state it concerns, and a cause.

use axiom_kernel::KernelError;

use crate::state_error_code::StateErrorCode;
use crate::state_id::StateId;

/// One state failure.
///
/// Following the kernel's rule, the **identity** of an error is its machine
/// data — the code, the state it concerns, and any wrapped cause. The
/// `&'static str` message is for humans and never participates in equality, so a
/// test asserts on what the failure *is* rather than on how it is worded.
#[derive(Debug, Clone, Copy)]
pub struct StateError {
    code: StateErrorCode,
    state: StateId,
    message: &'static str,
    cause: Option<KernelError>,
}

impl StateError {
    /// A failure not tied to one particular state.
    pub const fn new(code: StateErrorCode, message: &'static str) -> Self {
        StateError {
            code,
            state: StateId::NULL,
            message,
            cause: None,
        }
    }

    /// A failure concerning one particular state.
    pub const fn at(code: StateErrorCode, state: StateId, message: &'static str) -> Self {
        StateError {
            code,
            state,
            message,
            cause: None,
        }
    }

    /// Attach the lower-layer failure that caused this one.
    pub const fn caused_by(self, cause: KernelError) -> Self {
        StateError {
            cause: Some(cause),
            ..self
        }
    }

    /// Locate an existing failure at a state.
    ///
    /// A decode helper does not know which state it was decoding; the caller
    /// does, and stamps it on the way out so the diagnostic names the slot.
    pub const fn about(self, state: StateId) -> Self {
        StateError { state, ..self }
    }

    /// The machine identity.
    pub const fn code(self) -> StateErrorCode {
        self.code
    }

    /// The state this failure concerns, or [`StateId::NULL`].
    pub const fn state(self) -> StateId {
        self.state
    }

    /// The human-readable explanation. Never part of identity.
    pub const fn message(self) -> &'static str {
        self.message
    }

    /// The wrapped lower-layer cause, when there was one.
    pub const fn cause(self) -> Option<KernelError> {
        self.cause
    }
}

/// Identity is `(code, state, cause)` — never the message. `&` rather than `&&`
/// because the Branchless Law forbids the short-circuiting form and both sides
/// are pure comparisons that are always safe to evaluate.
impl PartialEq for StateError {
    fn eq(&self, other: &Self) -> bool {
        (self.code == other.code) & (self.state == other.state) & (self.cause == other.cause)
    }
}

impl Eq for StateError {}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::{KernelErrorCode, KernelErrorScope};

    fn kernel_cause() -> KernelError {
        KernelError::new(
            KernelErrorScope::Binary,
            KernelErrorCode::TruncatedData,
            "ran out of bytes",
        )
    }

    #[test]
    fn a_plain_failure_names_no_state_and_wraps_no_cause() {
        let error = StateError::new(StateErrorCode::InvalidSchema, "schema name is empty");
        assert_eq!(error.code(), StateErrorCode::InvalidSchema);
        assert_eq!(error.state(), StateId::NULL);
        assert_eq!(error.message(), "schema name is empty");
        assert_eq!(error.cause(), None);
    }

    #[test]
    fn a_located_failure_names_its_state() {
        let id = StateId::of_path("puzzle/tick");
        let error = StateError::at(StateErrorCode::UnknownStateIdentity, id, "not declared");
        assert_eq!(error.state(), id);
    }

    #[test]
    fn a_cause_is_carried_through() {
        let error = StateError::new(StateErrorCode::CorruptedSnapshot, "truncated")
            .caused_by(kernel_cause());
        assert_eq!(error.cause(), Some(kernel_cause()));
    }

    #[test]
    fn the_message_is_not_part_of_identity() {
        let one = StateError::new(StateErrorCode::InvalidPatch, "one wording");
        let other = StateError::new(StateErrorCode::InvalidPatch, "a completely different wording");
        assert_eq!(one, other);
    }

    #[test]
    fn a_failure_can_be_located_at_a_state_after_the_fact() {
        let id = StateId::of_path("puzzle/ghosts");
        let located = StateError::new(StateErrorCode::StateTypeMismatch, "bad bytes").about(id);
        assert_eq!(located.state(), id);
        assert_eq!(located.code(), StateErrorCode::StateTypeMismatch);
    }

    #[test]
    fn the_code_the_state_and_the_cause_are_all_part_of_identity() {
        let base = StateError::new(StateErrorCode::InvalidPatch, "m");
        assert_ne!(base, StateError::new(StateErrorCode::InvalidSchema, "m"));
        assert_ne!(
            base,
            StateError::at(StateErrorCode::InvalidPatch, StateId::of_path("a"), "m")
        );
        assert_ne!(base, base.caused_by(kernel_cause()));
    }
}
