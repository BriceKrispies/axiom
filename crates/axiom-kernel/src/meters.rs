//! `Meters` — a finite length, in metres.

use crate::error::KernelError;
use crate::error_code::KernelErrorCode;
use crate::error_scope::KernelErrorScope;
use crate::result::KernelResult;

/// A length in metres.
///
/// A kernel quantity primitive: the typed boundary where a raw `f32` becomes a
/// dimensioned length, so layers above stop passing naked floats whose unit a
/// caller has to guess. The inner scalar is always finite — [`Meters::new`] is
/// the only constructor and it rejects NaN / infinity.
///
/// (Metres are Axiom's world-space length unit. If a future decision makes the
/// world unit configurable, this is the one type to rename.)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meters(f32);

impl Meters {
    /// Construct a length, rejecting non-finite scalars (NaN / ±infinity).
    pub const fn new(value: f32) -> KernelResult<Self> {
        [
            Err(KernelError::new(
                KernelErrorScope::Scalar,
                KernelErrorCode::NonFiniteScalar,
                "Meters must be finite",
            )),
            Ok(Meters(value)),
        ][value.is_finite() as usize]
    }

    /// The underlying scalar value, in metres.
    pub const fn get(self) -> f32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_finite() {
        assert_eq!(Meters::new(2.5).unwrap().get(), 2.5);
    }

    #[test]
    fn new_rejects_nan() {
        let e = Meters::new(f32::NAN).unwrap_err();
        assert_eq!(e.scope(), KernelErrorScope::Scalar);
        assert_eq!(e.code(), KernelErrorCode::NonFiniteScalar);
    }

    #[test]
    fn new_rejects_infinity() {
        assert_eq!(
            Meters::new(f32::INFINITY).unwrap_err().code(),
            KernelErrorCode::NonFiniteScalar
        );
    }
}
