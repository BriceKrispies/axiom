//! [`WarpStrength`] — the magnitude of an FBM domain-warp offset.

use axiom_kernel::{KernelError, KernelErrorCode, KernelErrorScope, KernelResult};

/// The strength of a domain warp: how far (in FBM input units) the sample point is
/// displaced by the vector-valued warp field before the base field is sampled.
///
/// A typed warp knob rather than a naked `f32`, in the kernel
/// [`axiom_kernel::Meters`] shape: [`WarpStrength::new`] rejects non-finite
/// scalars. A strength of `0.0` is the identity (no displacement).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WarpStrength(f32);

impl WarpStrength {
    /// Construct a warp strength, rejecting non-finite scalars (NaN / ±infinity).
    pub const fn new(value: f32) -> KernelResult<Self> {
        [
            Err(KernelError::new(
                KernelErrorScope::Scalar,
                KernelErrorCode::NonFiniteScalar,
                "WarpStrength must be finite",
            )),
            Ok(WarpStrength(value)),
        ][value.is_finite() as usize]
    }

    /// The underlying warp magnitude, in FBM input units.
    pub const fn get(self) -> f32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_finite() {
        assert_eq!(WarpStrength::new(0.55).unwrap().get(), 0.55);
        assert_eq!(WarpStrength::new(0.0).unwrap().get(), 0.0);
    }

    #[test]
    fn new_rejects_non_finite() {
        assert_eq!(
            WarpStrength::new(f32::NAN).unwrap_err().code(),
            KernelErrorCode::NonFiniteScalar
        );
    }
}
