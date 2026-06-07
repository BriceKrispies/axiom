//! `Ratio` — a finite, dimensionless ratio.

use crate::error::KernelError;
use crate::error_code::KernelErrorCode;
use crate::error_scope::KernelErrorScope;
use crate::result::KernelResult;

/// A dimensionless ratio.
///
/// A kernel quantity primitive for the genuinely *unitless* scalars that are
/// nonetheless not arbitrary floats — an aspect ratio (width / height), a DPI
/// scale factor, a normalized fraction. Typing them as `Ratio` says "this is a
/// ratio, not some unknown number," which is the point: it stops a bare `f32`
/// from standing in for a quantity whose meaning the caller would have to guess.
/// The inner scalar is always finite — [`Ratio::new`] is the only constructor
/// and it rejects NaN / infinity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ratio(f32);

impl Ratio {
    /// Construct a ratio, rejecting non-finite scalars (NaN / ±infinity).
    pub const fn new(value: f32) -> KernelResult<Self> {
        if value.is_finite() {
            Ok(Ratio(value))
        } else {
            Err(KernelError::new(
                KernelErrorScope::Scalar,
                KernelErrorCode::NonFiniteScalar,
                "Ratio must be finite",
            ))
        }
    }

    /// The underlying dimensionless value.
    pub const fn get(self) -> f32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_finite() {
        assert_eq!(Ratio::new(1.7777).unwrap().get(), 1.7777);
    }

    #[test]
    fn new_rejects_nan() {
        let e = Ratio::new(f32::NAN).unwrap_err();
        assert_eq!(e.scope(), KernelErrorScope::Scalar);
        assert_eq!(e.code(), KernelErrorCode::NonFiniteScalar);
    }

    #[test]
    fn new_rejects_infinity() {
        assert_eq!(
            Ratio::new(f32::INFINITY).unwrap_err().code(),
            KernelErrorCode::NonFiniteScalar
        );
    }
}
