//! [`Frequency`] — a finite spatial frequency for a noise field.

use axiom_kernel::{KernelError, KernelErrorCode, KernelErrorScope, KernelResult};

/// A spatial frequency (cycles per world unit) for sampling a noise field.
///
/// The typed boundary where a raw `f32` becomes an FBM base frequency, so the
/// public noise API never takes a naked scalar whose meaning a caller must guess.
/// Follows the kernel's [`axiom_kernel::Meters`] shape: [`Frequency::new`] is the
/// only constructor and it rejects non-finite scalars (NaN / ±infinity).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frequency(f32);

impl Frequency {
    /// Construct a frequency, rejecting non-finite scalars (NaN / ±infinity).
    pub const fn new(value: f32) -> KernelResult<Self> {
        [
            Err(KernelError::new(
                KernelErrorScope::Scalar,
                KernelErrorCode::NonFiniteScalar,
                "Frequency must be finite",
            )),
            Ok(Frequency(value)),
        ][value.is_finite() as usize]
    }

    /// Construct a frequency from a *computed* or *constant* scalar, mapping any
    /// non-finite result (NaN / ±infinity) to `0.0` so the constructor is
    /// **total** — it never fails and never panics. This is the sanctioned path
    /// for a frequency built from an already-finite constant or from arithmetic,
    /// where a fallible [`Frequency::new`] would leave an unreachable error arm
    /// (and force an `unwrap`/`expect` at the call site). Mirrors
    /// [`axiom_kernel::Meters::finite_or_zero`] and [`axiom_kernel::Ratio::finite_or_zero`].
    pub const fn finite_or_zero(value: f32) -> Self {
        Frequency([0.0, value][value.is_finite() as usize])
    }

    /// The underlying frequency, in cycles per world unit.
    pub const fn get(self) -> f32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_finite() {
        assert_eq!(Frequency::new(1.8).unwrap().get(), 1.8);
    }

    #[test]
    fn finite_or_zero_passes_finite_and_zeroes_non_finite() {
        assert_eq!(Frequency::finite_or_zero(1.8).get(), 1.8);
        assert_eq!(Frequency::finite_or_zero(0.0).get(), 0.0);
        assert_eq!(Frequency::finite_or_zero(f32::NAN).get(), 0.0);
        assert_eq!(Frequency::finite_or_zero(f32::INFINITY).get(), 0.0);
        assert_eq!(Frequency::finite_or_zero(f32::NEG_INFINITY).get(), 0.0);
    }

    #[test]
    fn new_rejects_non_finite() {
        assert_eq!(
            Frequency::new(f32::NAN).unwrap_err().code(),
            KernelErrorCode::NonFiniteScalar
        );
        assert_eq!(
            Frequency::new(f32::INFINITY).unwrap_err().scope(),
            KernelErrorScope::Scalar
        );
    }
}
