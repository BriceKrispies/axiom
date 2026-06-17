//! The math layer's deterministic error value.

use axiom_kernel::KernelError;

use crate::math_error_code::MathErrorCode;

/// A deterministic Layer-02 math error.
///
/// Identity is `(code, kernel-cause-identity)`. Two errors with the same
/// [`MathErrorCode`] and the same wrapped [`KernelError`] identity compare
/// equal regardless of the static human message — error checks stay
/// machine-stable across builds and replays.
#[derive(Debug, Clone, Copy)]
pub struct MathError {
    code: MathErrorCode,
    message: &'static str,
    kernel: Option<KernelError>,
}

impl MathError {
    /// A math-only error without a wrapped kernel cause.
    pub const fn new(code: MathErrorCode, message: &'static str) -> Self {
        MathError {
            code,
            message,
            kernel: None,
        }
    }

    /// A math error that wraps a kernel failure (e.g. a binary-reader fault).
    pub const fn with_kernel(
        code: MathErrorCode,
        message: &'static str,
        kernel: KernelError,
    ) -> Self {
        MathError {
            code,
            message,
            kernel: Some(kernel),
        }
    }

    /// Shorthand for [`MathErrorCode::DivideByZero`].
    pub const fn divide_by_zero(message: &'static str) -> Self {
        MathError::new(MathErrorCode::DivideByZero, message)
    }

    /// Shorthand for [`MathErrorCode::NormalizeZeroLength`].
    pub const fn normalize_zero_length(message: &'static str) -> Self {
        MathError::new(MathErrorCode::NormalizeZeroLength, message)
    }

    /// Shorthand for [`MathErrorCode::NonFiniteScalar`].
    pub const fn non_finite_scalar(message: &'static str) -> Self {
        MathError::new(MathErrorCode::NonFiniteScalar, message)
    }

    /// Shorthand for [`MathErrorCode::InvalidAabbBounds`].
    pub const fn invalid_aabb_bounds(message: &'static str) -> Self {
        MathError::new(MathErrorCode::InvalidAabbBounds, message)
    }

    /// Shorthand for [`MathErrorCode::InvalidSphereRadius`].
    pub const fn invalid_sphere_radius(message: &'static str) -> Self {
        MathError::new(MathErrorCode::InvalidSphereRadius, message)
    }

    /// Shorthand for [`MathErrorCode::InvalidRayDirection`].
    pub const fn invalid_ray_direction(message: &'static str) -> Self {
        MathError::new(MathErrorCode::InvalidRayDirection, message)
    }

    /// Shorthand for [`MathErrorCode::InvalidMatrixOperation`].
    pub const fn invalid_matrix_operation(message: &'static str) -> Self {
        MathError::new(MathErrorCode::InvalidMatrixOperation, message)
    }

    /// Shorthand for [`MathErrorCode::DeserializationFailed`] that preserves
    /// the kernel binary-reader cause.
    pub const fn deserialization_failed(message: &'static str, cause: KernelError) -> Self {
        MathError::with_kernel(MathErrorCode::DeserializationFailed, message, cause)
    }

    /// The machine-readable math error code.
    pub const fn code(&self) -> MathErrorCode {
        self.code
    }

    /// The static human message. Never used for comparison.
    pub const fn message(&self) -> &'static str {
        self.message
    }

    /// The wrapped kernel cause, if this failure originated there.
    pub const fn kernel(&self) -> Option<KernelError> {
        self.kernel
    }
}

/// Equality on machine identity only.
impl PartialEq for MathError {
    fn eq(&self, other: &Self) -> bool {
        (self.code == other.code) & (self.kernel == other.kernel)
    }
}

impl Eq for MathError {}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::{KernelApi, KernelErrorCode};

    #[test]
    fn identity_ignores_message() {
        let a = MathError::new(MathErrorCode::DivideByZero, "x");
        let b = MathError::new(MathErrorCode::DivideByZero, "totally different");
        assert_eq!(a, b);
    }

    #[test]
    fn different_code_is_not_equal() {
        let a = MathError::new(MathErrorCode::DivideByZero, "");
        let b = MathError::new(MathErrorCode::NormalizeZeroLength, "");
        assert_ne!(a, b);
    }

    #[test]
    fn shorthand_constructors_use_their_codes() {
        assert_eq!(
            MathError::divide_by_zero("").code(),
            MathErrorCode::DivideByZero
        );
        assert_eq!(
            MathError::normalize_zero_length("").code(),
            MathErrorCode::NormalizeZeroLength
        );
        assert_eq!(
            MathError::non_finite_scalar("").code(),
            MathErrorCode::NonFiniteScalar
        );
        assert_eq!(
            MathError::invalid_aabb_bounds("").code(),
            MathErrorCode::InvalidAabbBounds
        );
        assert_eq!(
            MathError::invalid_sphere_radius("").code(),
            MathErrorCode::InvalidSphereRadius
        );
        assert_eq!(
            MathError::invalid_ray_direction("").code(),
            MathErrorCode::InvalidRayDirection
        );
        assert_eq!(
            MathError::invalid_matrix_operation("").code(),
            MathErrorCode::InvalidMatrixOperation
        );
    }

    #[test]
    fn wraps_a_kernel_error_and_preserves_identity() {
        let api = KernelApi::new();
        let kernel_err = api.fixed_step(0).unwrap_err();
        let wrapped = MathError::deserialization_failed("binary read failed", kernel_err);
        assert_eq!(wrapped.code(), MathErrorCode::DeserializationFailed);
        assert_eq!(wrapped.kernel(), Some(kernel_err));
        assert_eq!(
            wrapped.kernel().unwrap().code(),
            KernelErrorCode::InvalidFixedStep
        );
    }

    #[test]
    fn message_is_preserved_but_not_part_of_identity() {
        let e = MathError::new(MathErrorCode::DivideByZero, "denominator was zero");
        assert_eq!(e.message(), "denominator was zero");
    }

    #[test]
    fn wrapped_and_unwrapped_are_not_equal() {
        let api = KernelApi::new();
        let cause = api.fixed_step(0).unwrap_err();
        let bare = MathError::new(MathErrorCode::DeserializationFailed, "x");
        let wrapped = MathError::with_kernel(MathErrorCode::DeserializationFailed, "x", cause);
        assert_ne!(bare, wrapped);
    }
}
