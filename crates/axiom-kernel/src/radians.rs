//! `Radians` — a finite angle, in radians.

use crate::error::KernelError;
use crate::error_code::KernelErrorCode;
use crate::error_scope::KernelErrorScope;
use crate::result::KernelResult;

/// An angle in radians.
///
/// A kernel quantity primitive: a public API that takes `Radians` can no longer
/// be handed a length, a duration, or a degrees value by mistake — the unit
/// (radians, not degrees) is part of the type. The inner scalar is always finite
/// — [`Radians::new`] is the only constructor and it rejects NaN / infinity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radians(f32);

impl Radians {
    /// Construct an angle, rejecting non-finite scalars (NaN / ±infinity).
    pub const fn new(value: f32) -> KernelResult<Self> {
        if value.is_finite() {
            Ok(Radians(value))
        } else {
            Err(KernelError::new(
                KernelErrorScope::Scalar,
                KernelErrorCode::NonFiniteScalar,
                "Radians must be finite",
            ))
        }
    }

    /// The underlying scalar value, in radians.
    pub const fn get(self) -> f32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_finite() {
        assert_eq!(Radians::new(1.25).unwrap().get(), 1.25);
    }

    #[test]
    fn new_rejects_nan() {
        let e = Radians::new(f32::NAN).unwrap_err();
        assert_eq!(e.scope(), KernelErrorScope::Scalar);
        assert_eq!(e.code(), KernelErrorCode::NonFiniteScalar);
    }

    #[test]
    fn new_rejects_infinity() {
        assert_eq!(
            Radians::new(f32::NEG_INFINITY).unwrap_err().code(),
            KernelErrorCode::NonFiniteScalar
        );
    }
}
