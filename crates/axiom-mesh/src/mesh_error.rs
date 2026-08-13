//! The mesh layer's deterministic error value.

use axiom_kernel::KernelError;

use crate::mesh_error_code::MeshErrorCode;

/// A deterministic mesh-layer error.
///
/// Identity is `(code, kernel-cause-identity)`. Two errors with the same
/// [`MeshErrorCode`] and the same wrapped [`KernelError`] identity compare equal
/// regardless of the static human message, so error checks stay machine-stable
/// across builds and replays.
///
/// Unlike [`axiom_math::MathError`], this type deliberately exposes **no
/// per-code shorthand constructors**. With twenty codes shared across two
/// layers, twenty one-line wrappers would be pure surface — call sites name the
/// code explicitly instead: `MeshError::new(MeshErrorCode::IndexOutOfRange, "…")`.
#[derive(Debug, Clone, Copy)]
pub struct MeshError {
    code: MeshErrorCode,
    message: &'static str,
    kernel: Option<KernelError>,
}

impl MeshError {
    /// A mesh error without a wrapped kernel cause.
    pub const fn new(code: MeshErrorCode, message: &'static str) -> Self {
        MeshError {
            code,
            message,
            kernel: None,
        }
    }

    /// A mesh error that wraps a kernel failure (a binary-reader fault).
    pub const fn with_kernel(
        code: MeshErrorCode,
        message: &'static str,
        kernel: KernelError,
    ) -> Self {
        MeshError {
            code,
            message,
            kernel: Some(kernel),
        }
    }

    /// The machine-readable failure identity.
    pub const fn code(&self) -> MeshErrorCode {
        self.code
    }

    /// The static human-readable description. Not part of the error identity.
    pub const fn message(&self) -> &'static str {
        self.message
    }

    /// The wrapped kernel cause, when this error came from deserialization.
    pub const fn kernel(&self) -> Option<KernelError> {
        self.kernel
    }
}

/// Equality on machine identity only.
impl PartialEq for MeshError {
    fn eq(&self, other: &Self) -> bool {
        (self.code == other.code) & (self.kernel == other.kernel)
    }
}

impl Eq for MeshError {}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::KernelApi;

    fn kernel_error() -> KernelError {
        KernelApi::new().fixed_step(0).unwrap_err()
    }

    #[test]
    fn identity_ignores_message() {
        let a = MeshError::new(MeshErrorCode::EmptyPositions, "x");
        let b = MeshError::new(MeshErrorCode::EmptyPositions, "totally different");
        assert_eq!(a, b);
        assert_eq!(a.code(), MeshErrorCode::EmptyPositions);
        assert_eq!(a.message(), "x");
    }

    #[test]
    fn different_code_is_not_equal() {
        let a = MeshError::new(MeshErrorCode::EmptyPositions, "");
        let b = MeshError::new(MeshErrorCode::IndexOutOfRange, "");
        assert_ne!(a, b);
    }

    #[test]
    fn a_plain_error_carries_no_kernel_cause() {
        assert_eq!(
            MeshError::new(MeshErrorCode::InvalidParameter, "p").kernel(),
            None
        );
    }

    #[test]
    fn a_wrapped_error_carries_and_compares_its_kernel_cause() {
        let cause = kernel_error();
        let a = MeshError::with_kernel(MeshErrorCode::DeserializationFailed, "r", cause);
        assert_eq!(a.kernel(), Some(cause));
        assert_eq!(a.code(), MeshErrorCode::DeserializationFailed);
        // Same code, different cause presence => different identity.
        assert_ne!(
            a,
            MeshError::new(MeshErrorCode::DeserializationFailed, "r")
        );
    }
}
