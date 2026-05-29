//! The deterministic approximate-equality trait used across Layer 02.

use crate::epsilon::Epsilon;

/// Compare two values for engine-deterministic approximate equality.
///
/// All implementations reject non-finite operands: if any component of
/// `self` or `other` is `NaN` or `±Inf`, the comparison returns `false`.
/// This is intentional — math values that are not finite cannot be
/// meaningfully compared and must surface earlier through a validation path.
///
/// `f32`'s impl is the base case; vectors, matrices, quaternions, transforms
/// and AABBs reduce to component-wise `f32` comparisons against the same
/// [`Epsilon`].
pub trait ApproxEq {
    /// Whether `self` and `other` are equal within `epsilon`.
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool;
}

impl ApproxEq for f32 {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        if !self.is_finite() || !other.is_finite() {
            return false;
        }
        (self - other).abs() <= epsilon.value()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eps() -> Epsilon {
        Epsilon::DEFAULT
    }

    #[test]
    fn equal_scalars_compare_equal() {
        assert!(1.0f32.approx_eq(&1.0, eps()));
    }

    #[test]
    fn nearby_scalars_compare_equal_within_epsilon() {
        let a: f32 = 1.0;
        let b: f32 = 1.0 + 1.0e-7;
        assert!(a.approx_eq(&b, eps()));
    }

    #[test]
    fn distant_scalars_compare_not_equal() {
        let a: f32 = 1.0;
        let b: f32 = 1.0 + 1.0e-3;
        assert!(!a.approx_eq(&b, eps()));
    }

    #[test]
    fn nan_is_never_approx_equal_to_anything() {
        let nan: f32 = f32::NAN;
        assert!(!nan.approx_eq(&0.0, eps()));
        assert!(!(0.0f32).approx_eq(&nan, eps()));
        assert!(!nan.approx_eq(&nan, eps()));
    }

    #[test]
    fn infinity_is_never_approx_equal_to_anything() {
        let inf: f32 = f32::INFINITY;
        assert!(!inf.approx_eq(&inf, eps()));
        assert!(!inf.approx_eq(&0.0, eps()));
    }
}
