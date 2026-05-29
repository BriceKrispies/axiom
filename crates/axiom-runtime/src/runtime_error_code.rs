//! Machine-readable runtime-error code.

/// The reason a runtime operation failed.
///
/// These are layer-01 codes; kernel-originated failures are wrapped via
/// [`crate::runtime_error::RuntimeError::with_kernel`] and retain their kernel
/// `(scope, code)` identity for downstream matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum RuntimeErrorCode {
    /// A lifecycle transition was attempted from a state that does not allow it.
    InvalidLifecycleTransition = 1,
    /// Two systems registered with the same stable id.
    DuplicateSystemId = 2,
    /// Two systems registered with the same order value (no implicit tie-breaker).
    DuplicateSystemOrder = 3,
    /// A registered system returned an error from its `run` method.
    SystemFailed = 4,
    /// `step()` was called when the runtime was not in `Running`.
    StepWhileNotRunning = 5,
    /// The runtime config was rejected (e.g. zero fixed step).
    InvalidConfig = 6,
    /// A kernel call inside the runtime returned a `KernelError` (which is
    /// preserved on the wrapping [`crate::runtime_error::RuntimeError`]).
    KernelFailure = 7,
}

impl RuntimeErrorCode {
    /// The stable numeric discriminant.
    pub const fn raw(self) -> u16 {
        self as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_are_stable() {
        assert_eq!(RuntimeErrorCode::InvalidLifecycleTransition.raw(), 1);
        assert_eq!(RuntimeErrorCode::KernelFailure.raw(), 7);
    }

    #[test]
    fn codes_are_distinct_and_orderable() {
        assert_ne!(
            RuntimeErrorCode::DuplicateSystemId,
            RuntimeErrorCode::DuplicateSystemOrder
        );
        assert!(RuntimeErrorCode::InvalidLifecycleTransition < RuntimeErrorCode::SystemFailed);
    }
}
