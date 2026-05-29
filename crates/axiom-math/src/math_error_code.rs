//! Machine-readable math-error code.

/// The reason a checked math operation failed.
///
/// These are layer-02 codes; the kernel error model's enums are closed, so
/// math defines its own identity in the same shape (`(code, optional kernel
/// cause)`). Two errors with the same code compare equal regardless of
/// message, so error checks stay machine-stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum MathErrorCode {
    /// Divisor was zero in a checked scalar / vector / transform operation.
    DivideByZero = 1,
    /// Attempted to normalize a zero-length vector or quaternion.
    NormalizeZeroLength = 2,
    /// A scalar argument was `NaN` or `±Inf` where finiteness is required.
    NonFiniteScalar = 3,
    /// An AABB was constructed with `min > max` in some component.
    InvalidAabbBounds = 4,
    /// A sphere was constructed with a negative or non-finite radius.
    InvalidSphereRadius = 5,
    /// A ray was constructed with a zero-length direction.
    InvalidRayDirection = 6,
    /// A matrix or transform operation could not be performed deterministically
    /// (e.g. inverse with a zero scale axis, look_at with collinear vectors).
    InvalidMatrixOperation = 7,
    /// Deserialization could not be completed; the wrapped `KernelError`
    /// preserves the kernel binary-reader cause.
    DeserializationFailed = 8,
}

impl MathErrorCode {
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
        assert_eq!(MathErrorCode::DivideByZero.raw(), 1);
        assert_eq!(MathErrorCode::DeserializationFailed.raw(), 8);
    }

    #[test]
    fn codes_are_distinct_and_ordered() {
        assert_ne!(
            MathErrorCode::NormalizeZeroLength,
            MathErrorCode::NonFiniteScalar
        );
        assert!(MathErrorCode::DivideByZero < MathErrorCode::DeserializationFailed);
    }
}
