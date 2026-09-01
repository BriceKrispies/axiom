//! Two-component double-precision vector.

use crate::approx_eq::ApproxEq;
use crate::dvec3::DVec3;
use crate::epsilon::Epsilon;
use crate::math_error::MathError;
use crate::math_result::MathResult;

/// A deterministic two-component `f64` vector.
///
/// The double-precision sibling of [`crate::Vec2`], and the planar counterpart
/// to [`DVec3`]. It is the sample position of a 2D procedural field — a texture
/// lattice, a cellular basis, a domain warp — where the same argument applies as
/// for [`DVec3`]: splitting a coordinate into its integer cell and fractional
/// offset is the first thing every such basis does, and doing it at single
/// precision is where a coarse coordinate loses the fraction entirely.
///
/// See [`crate::Scalar`] for the rule this family exists to give a vocabulary
/// to, and [`DVec2::to_single`] for the one narrowing point back to the
/// interchange scalar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DVec2 {
    pub x: f64,
    pub y: f64,
}

impl DVec2 {
    /// `(0, 0)`.
    pub const ZERO: DVec2 = DVec2 { x: 0.0, y: 0.0 };
    /// `(1, 1)`.
    pub const ONE: DVec2 = DVec2 { x: 1.0, y: 1.0 };

    /// Component constructor.
    pub const fn new(x: f64, y: f64) -> Self {
        DVec2 { x, y }
    }

    /// Both components set to `s`.
    pub const fn splat(s: f64) -> Self {
        DVec2 { x: s, y: s }
    }

    /// Component-wise sum.
    pub const fn add(self, other: DVec2) -> DVec2 {
        DVec2::new(self.x + other.x, self.y + other.y)
    }

    /// Component-wise difference.
    pub const fn subtract(self, other: DVec2) -> DVec2 {
        DVec2::new(self.x - other.x, self.y - other.y)
    }

    /// Component-wise product.
    pub const fn mul_componentwise(self, other: DVec2) -> DVec2 {
        DVec2::new(self.x * other.x, self.y * other.y)
    }

    /// Scale by a scalar.
    pub const fn mul_scalar(self, k: f64) -> DVec2 {
        DVec2::new(self.x * k, self.y * k)
    }

    /// Add a scalar to both components.
    pub const fn add_scalar(self, k: f64) -> DVec2 {
        DVec2::new(self.x + k, self.y + k)
    }

    /// Divide by a scalar, returning [`crate::math_error_code::MathErrorCode::DivideByZero`]
    /// if `k` is `0.0` and [`crate::math_error_code::MathErrorCode::NonFiniteScalar`]
    /// if `k` is not finite.
    pub fn div_scalar(self, k: f64) -> MathResult<DVec2> {
        (!k.is_finite())
            .then_some(Err(MathError::non_finite_scalar(
                "dvec2 scalar divisor must be finite",
            )))
            .or_else(|| {
                (k == 0.0).then_some(Err(MathError::divide_by_zero(
                    "dvec2 scalar divisor was zero",
                )))
            })
            .unwrap_or_else(|| Ok(DVec2::new(self.x / k, self.y / k)))
    }

    /// Dot product.
    pub const fn dot(self, other: DVec2) -> f64 {
        self.x * other.x + self.y * other.y
    }

    /// Squared length.
    pub const fn length_squared(self) -> f64 {
        self.dot(self)
    }

    /// Euclidean length.
    pub fn length(self) -> f64 {
        self.length_squared().sqrt()
    }

    /// Unit-length copy. Fails with
    /// [`crate::math_error_code::MathErrorCode::NormalizeZeroLength`] for the
    /// zero vector.
    pub fn normalize(self) -> MathResult<DVec2> {
        let len = self.length();
        let valid = (len != 0.0) & len.is_finite();
        valid
            .then_some(len)
            .map(|len| DVec2::new(self.x / len, self.y / len))
            .ok_or_else(|| MathError::normalize_zero_length("cannot normalize zero-length DVec2"))
    }

    /// Euclidean distance between `self` and `other`.
    pub fn distance(self, other: DVec2) -> f64 {
        self.subtract(other).length()
    }

    /// Component-wise floor.
    pub fn floor(self) -> DVec2 {
        DVec2::new(self.x.floor(), self.y.floor())
    }

    /// The fractional part, `self - self.floor()`, always in `[0, 1)`.
    ///
    /// GLSL's `fract`, **not** Rust's [`f64::fract`], which keeps the sign of
    /// its input. See [`DVec3::fract`] for why the difference matters on a
    /// lattice.
    pub fn fract(self) -> DVec2 {
        self.subtract(self.floor())
    }

    /// Component-wise modulo with a non-negative result, `self - m * floor(self / m)`.
    ///
    /// GLSL's `mod`, not Rust's `%`. Rust's remainder keeps the sign of the
    /// dividend, so `-0.25 % 1.0` is `-0.25` where this gives `0.75`. On a
    /// **periodic** lattice that difference is the whole ballgame: wrapping a
    /// negative coordinate with `%` folds it onto the wrong cell and the
    /// texture stops tiling on half its domain.
    pub fn rem_euclid_componentwise(self, modulus: DVec2) -> DVec2 {
        self.subtract(
            modulus.mul_componentwise(DVec2::new(
                (self.x / modulus.x).floor(),
                (self.y / modulus.y).floor(),
            )),
        )
    }

    /// Promote to three components, with `z`.
    pub const fn extend(self, z: f64) -> DVec3 {
        DVec3::new(self.x, self.y, z)
    }

    /// Narrow to the engine's interchange scalar. The one named narrowing
    /// point — see [`DVec3::to_single`].
    pub fn to_single(self) -> crate::vec2::Vec2 {
        crate::vec2::Vec2::new(self.x as f32, self.y as f32)
    }

    /// Widen from the engine's interchange scalar. Exact.
    pub fn from_single(v: crate::vec2::Vec2) -> DVec2 {
        DVec2::new(f64::from(v.x), f64::from(v.y))
    }
}

impl ApproxEq for DVec2 {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.x.approx_eq(&other.x, epsilon) & self.y.approx_eq(&other.y, epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;

    fn eps() -> Epsilon {
        Epsilon::DEFAULT_DOUBLE
    }

    #[test]
    fn constants_and_splat_match_documentation() {
        assert!(DVec2::ZERO.approx_eq(&DVec2::new(0.0, 0.0), eps()));
        assert!(DVec2::ONE.approx_eq(&DVec2::new(1.0, 1.0), eps()));
        assert_eq!(DVec2::splat(2.5), DVec2::new(2.5, 2.5));
    }

    #[test]
    fn arithmetic_is_component_wise() {
        let a = DVec2::new(1.0, 2.0);
        let b = DVec2::new(3.0, 5.0);
        assert_eq!(a.add(b), DVec2::new(4.0, 7.0));
        assert_eq!(b.subtract(a), DVec2::new(2.0, 3.0));
        assert_eq!(a.mul_componentwise(b), DVec2::new(3.0, 10.0));
        assert_eq!(a.mul_scalar(2.0), DVec2::new(2.0, 4.0));
        assert_eq!(a.add_scalar(1.0), DVec2::new(2.0, 3.0));
    }

    #[test]
    fn div_scalar_divides_and_rejects_degenerate_divisors() {
        assert_eq!(
            DVec2::new(2.0, -4.0).div_scalar(2.0).unwrap(),
            DVec2::new(1.0, -2.0)
        );
        assert_eq!(
            DVec2::ONE.div_scalar(0.0).unwrap_err().code(),
            MathErrorCode::DivideByZero
        );
        assert_eq!(
            DVec2::ONE.div_scalar(f64::NAN).unwrap_err().code(),
            MathErrorCode::NonFiniteScalar
        );
    }

    #[test]
    fn dot_length_and_distance_agree() {
        assert_eq!(DVec2::new(3.0, 4.0).length_squared(), 25.0);
        assert_eq!(DVec2::new(3.0, 4.0).length(), 5.0);
        assert_eq!(DVec2::new(1.0, 0.0).dot(DVec2::new(0.0, 1.0)), 0.0);
        assert_eq!(DVec2::ZERO.distance(DVec2::new(3.0, 4.0)), 5.0);
    }

    #[test]
    fn normalize_produces_unit_length_and_rejects_the_degenerate_cases() {
        assert!(DVec2::new(0.0, 7.0)
            .normalize()
            .unwrap()
            .approx_eq(&DVec2::new(0.0, 1.0), eps()));
        assert_eq!(
            DVec2::ZERO.normalize().unwrap_err().code(),
            MathErrorCode::NormalizeZeroLength
        );
        assert!(DVec2::new(f64::MAX, f64::MAX).normalize().is_err());
    }

    #[test]
    fn floor_and_fract_split_a_position_into_cell_and_offset() {
        let v = DVec2::new(3.25, -1.75);
        assert_eq!(v.floor(), DVec2::new(3.0, -2.0));
        assert_eq!(v.fract(), DVec2::new(0.25, 0.25));
        assert_eq!(v.floor().add(v.fract()), v);
    }

    #[test]
    fn fract_is_non_negative_for_negative_inputs_unlike_rusts() {
        assert_eq!((-0.3_f64).fract(), -0.3);
        assert_eq!(DVec2::splat(-0.3).fract().x, 0.7);
    }

    /// The property periodic noise is built on: wrapping must be non-negative,
    /// so a negative coordinate lands on the cell it tiles onto.
    #[test]
    fn the_modulo_wraps_negatives_the_way_a_periodic_lattice_needs() {
        let period = DVec2::splat(4.0);
        assert_eq!(
            DVec2::new(-0.25, 5.5).rem_euclid_componentwise(period),
            DVec2::new(3.75, 1.5)
        );
        assert_eq!(-0.25_f64 % 4.0, -0.25, "Rust's remainder, which this is not");
        // A point and its translate by one period agree.
        let p = DVec2::new(1.3, 2.7);
        assert!(p
            .rem_euclid_componentwise(period)
            .approx_eq(&p.add(period).rem_euclid_componentwise(period), eps()));
    }

    #[test]
    fn extend_promotes_to_three_components() {
        assert_eq!(
            DVec2::new(1.0, 2.0).extend(3.0),
            crate::dvec3::DVec3::new(1.0, 2.0, 3.0)
        );
    }

    #[test]
    fn to_single_and_from_single_are_the_named_narrowing_boundary() {
        let v = DVec2::new(1.5, -2.25);
        assert_eq!(v.to_single(), crate::vec2::Vec2::new(1.5, -2.25));
        assert_eq!(DVec2::from_single(v.to_single()), v);
        // And the narrowing really does narrow.
        assert_ne!(f64::from(DVec2::splat(0.1).to_single().x), 0.1);
    }

    #[test]
    fn approx_eq_rejects_nan_components() {
        assert!(!DVec2::new(f64::NAN, 0.0).approx_eq(&DVec2::ZERO, eps()));
        assert!(!DVec2::new(0.0, f64::NAN).approx_eq(&DVec2::ZERO, eps()));
        assert!(DVec2::ZERO.approx_eq(&DVec2::ZERO, eps()));
    }
}
