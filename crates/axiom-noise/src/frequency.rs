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
