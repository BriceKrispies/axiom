//! [`Lacunarity`] — the per-octave frequency multiplier of an FBM.

use axiom_kernel::{KernelError, KernelErrorCode, KernelErrorScope, KernelResult};

/// The lacunarity of a fractal Brownian motion field: the factor by which the
/// sampling frequency grows from one octave to the next (classically `2.0`).
///
/// A typed FBM knob rather than a naked `f32`, in the kernel
/// [`axiom_kernel::Meters`] shape: [`Lacunarity::new`] rejects non-finite scalars,
/// and [`Lacunarity::DOUBLING`] is the canonical octave-doubling default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lacunarity(f32);

impl Lacunarity {
    /// The canonical FBM lacunarity: each octave doubles the frequency.
    pub const DOUBLING: Lacunarity = Lacunarity(2.0);

    /// Construct a lacunarity, rejecting non-finite scalars (NaN / ±infinity).
    pub const fn new(value: f32) -> KernelResult<Self> {
        [
            Err(KernelError::new(
                KernelErrorScope::Scalar,
                KernelErrorCode::NonFiniteScalar,
                "Lacunarity must be finite",
            )),
            Ok(Lacunarity(value)),
        ][value.is_finite() as usize]
    }

    /// The underlying per-octave frequency multiplier.
    pub const fn get(self) -> f32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubling_default_is_two() {
        assert_eq!(Lacunarity::DOUBLING.get(), 2.0);
    }

    #[test]
    fn new_accepts_finite() {
        assert_eq!(Lacunarity::new(2.5).unwrap().get(), 2.5);
    }

    #[test]
    fn new_rejects_non_finite() {
        assert_eq!(
            Lacunarity::new(f32::INFINITY).unwrap_err().code(),
            KernelErrorCode::NonFiniteScalar
        );
    }
}
