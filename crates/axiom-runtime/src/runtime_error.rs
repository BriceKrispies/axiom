//! A runtime error, optionally wrapping a kernel error.

use axiom_kernel::KernelError;

use crate::runtime_error_code::RuntimeErrorCode;

/// A deterministic runtime error.
///
/// Identity is the pair `(code, kernel-cause-identity)`. Two errors with the
/// same `RuntimeErrorCode` and the same wrapped `KernelError` identity compare
/// equal regardless of human message, so error checks in tests and replays are
/// machine-stable.
#[derive(Debug, Clone, Copy)]
pub struct RuntimeError {
    code: RuntimeErrorCode,
    message: &'static str,
    kernel: Option<KernelError>,
}

impl RuntimeError {
    /// A runtime-only error without a kernel cause.
    pub const fn new(code: RuntimeErrorCode, message: &'static str) -> Self {
        RuntimeError {
            code,
            message,
            kernel: None,
        }
    }

    /// A runtime error that wraps a kernel failure.
    pub const fn with_kernel(
        code: RuntimeErrorCode,
        message: &'static str,
        kernel: KernelError,
    ) -> Self {
        RuntimeError {
            code,
            message,
            kernel: Some(kernel),
        }
    }

    /// The machine-readable runtime error code.
    pub const fn code(&self) -> RuntimeErrorCode {
        self.code
    }

    /// The static human message. Never used for comparison.
    pub const fn message(&self) -> &'static str {
        self.message
    }

    /// The wrapped kernel error, if this failure originated there.
    pub const fn kernel(&self) -> Option<KernelError> {
        self.kernel
    }
}

/// Equality on machine identity only: runtime code plus optional kernel
/// identity. The human message is metadata.
impl PartialEq for RuntimeError {
    fn eq(&self, other: &Self) -> bool {
        (self.code == other.code) & (self.kernel == other.kernel)
    }
}

impl Eq for RuntimeError {}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::{KernelApi, KernelErrorCode};

    #[test]
    fn identity_ignores_message() {
        let a = RuntimeError::new(RuntimeErrorCode::DuplicateSystemId, "x");
        let b = RuntimeError::new(RuntimeErrorCode::DuplicateSystemId, "totally different");
        assert_eq!(a, b);
    }

    #[test]
    fn different_code_is_not_equal() {
        let a = RuntimeError::new(RuntimeErrorCode::DuplicateSystemId, "");
        let b = RuntimeError::new(RuntimeErrorCode::DuplicateSystemOrder, "");
        assert_ne!(a, b);
    }

    #[test]
    fn wraps_a_kernel_error_and_preserves_its_identity() {
        let api = KernelApi::new();
        let kernel_err = api.fixed_step(0).unwrap_err();
        assert_eq!(kernel_err.code(), KernelErrorCode::InvalidFixedStep);

        let wrapped = RuntimeError::with_kernel(
            RuntimeErrorCode::InvalidConfig,
            "invalid fixed step",
            kernel_err,
        );
        assert_eq!(wrapped.code(), RuntimeErrorCode::InvalidConfig);
        assert_eq!(wrapped.kernel(), Some(kernel_err));
    }

    #[test]
    fn wrapped_and_unwrapped_are_not_equal() {
        let api = KernelApi::new();
        let kernel_err = api.fixed_step(0).unwrap_err();
        let bare = RuntimeError::new(RuntimeErrorCode::InvalidConfig, "x");
        let wrapped = RuntimeError::with_kernel(RuntimeErrorCode::InvalidConfig, "x", kernel_err);
        assert_ne!(bare, wrapped);
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use axiom_kernel::{KernelError, KernelErrorCode, KernelErrorScope};

    #[test]
    fn kernel_accessor_and_equality_branches() {
        let bare = RuntimeError::new(RuntimeErrorCode::SystemFailed, "x");
        assert_eq!(bare.kernel(), None);
        assert_eq!(bare.message(), "x");
        let k = KernelError::new(KernelErrorScope::Time, KernelErrorCode::RangeOverflow, "o");
        let wrapped = RuntimeError::with_kernel(RuntimeErrorCode::KernelFailure, "x", k);
        assert_eq!(wrapped.kernel(), Some(k));
        assert!(bare == RuntimeError::new(RuntimeErrorCode::SystemFailed, "y"));
        assert!(bare != RuntimeError::new(RuntimeErrorCode::InvalidConfig, "y"));
        assert!(RuntimeError::new(RuntimeErrorCode::KernelFailure, "z") != wrapped);
    }
}
