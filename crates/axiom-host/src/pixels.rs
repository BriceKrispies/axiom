//! A host-owned pixel-space scalar quantity.

use crate::host_error::HostError;
use crate::host_result::HostResult;

/// A length in device pixels — the viewport's pixel-space scalar.
///
/// `Pixels` carries a single finite `f32`. The logical/physical distinction is
/// **not** encoded in the type; it is carried by the conversion *method names*
/// on [`crate::HostViewport`] (`logical_to_physical` / `physical_to_logical`).
/// A `Pixels` is just a validated, finite pixel length whose meaning depends on
/// which side of a conversion it sits on.
///
/// Finiteness is enforced at construction: a non-finite value (NaN or infinity)
/// can never become a `Pixels`, so downstream pixel arithmetic never has to
/// re-check for it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pixels(f32);

impl Pixels {
    /// Construct a pixel length, rejecting any non-finite value (NaN or
    /// infinity) with [`HostError::non_finite_pixels`].
    pub fn new(value: f32) -> HostResult<Self> {
        if !value.is_finite() {
            return Err(HostError::non_finite_pixels(
                "a pixel length must be a finite value",
            ));
        }
        Ok(Pixels(value))
    }

    /// The underlying finite pixel length.
    pub const fn get(self) -> f32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_error_code::HostErrorCode;

    #[test]
    fn new_accepts_a_finite_value_and_get_returns_it() {
        let p = Pixels::new(42.5).unwrap();
        assert_eq!(p.get(), 42.5);
    }

    #[test]
    fn new_rejects_nan() {
        let err = Pixels::new(f32::NAN).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::NonFinitePixels);
    }

    #[test]
    fn new_rejects_infinity() {
        let err = Pixels::new(f32::INFINITY).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::NonFinitePixels);
    }
}
