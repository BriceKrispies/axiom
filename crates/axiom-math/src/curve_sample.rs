//! One evaluated point on a [`crate::Curve`].

use axiom_kernel::{Meters, Ratio};

use crate::vec3::Vec3;

/// A curve evaluated at one parameter, carrying everything a downstream
/// consumer (a sweep, a spline follower, a debug draw) needs without having to
/// re-evaluate the curve.
///
/// The four facts travel together on purpose: `parameter` says *where on the
/// curve's parameterization* the sample came from, while `distance` says *how
/// far along the curve* it is. They are not interchangeable — a curve's
/// parameter is not proportional to its arc length, which is exactly why
/// [`crate::Curve::sample_uniform`] exists.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurveSample {
    position: Vec3,
    tangent: Vec3,
    parameter: Ratio,
    distance: Meters,
}

impl CurveSample {
    /// Assemble a sample. Crate-private: only curve evaluation may mint one, so
    /// a `CurveSample` in a caller's hands is always self-consistent (the
    /// tangent really is the unit derivative at `parameter`, and `distance`
    /// really is the arc length from the curve's start).
    pub(crate) const fn new(
        position: Vec3,
        tangent: Vec3,
        parameter: Ratio,
        distance: Meters,
    ) -> Self {
        CurveSample {
            position,
            tangent,
            parameter,
            distance,
        }
    }

    /// The point on the curve.
    pub const fn position(&self) -> Vec3 {
        self.position
    }

    /// The unit-length curve direction at this sample.
    pub const fn tangent(&self) -> Vec3 {
        self.tangent
    }

    /// The curve parameter (`0 ..= 1` over the whole curve) this sample came
    /// from. Not proportional to [`CurveSample::distance`].
    pub const fn parameter(&self) -> Ratio {
        self.parameter
    }

    /// Cumulative arc length from the curve's start. The first sample of a
    /// sampling run is always `0`.
    pub const fn distance(&self) -> Meters {
        self.distance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> CurveSample {
        CurveSample::new(
            Vec3::new(1.0, 2.0, 3.0),
            Vec3::UNIT_X,
            Ratio::new(0.25).unwrap(),
            Meters::new(4.5).unwrap(),
        )
    }

    #[test]
    fn accessors_return_the_constructed_facts() {
        let s = sample();
        assert_eq!(s.position(), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(s.tangent(), Vec3::UNIT_X);
        assert_eq!(s.parameter().get(), 0.25);
        assert_eq!(s.distance().get(), 4.5);
    }

    #[test]
    fn samples_compare_on_every_field() {
        let a = sample();
        let b = sample();
        assert_eq!(a, b);
        let moved = CurveSample::new(
            Vec3::ZERO,
            Vec3::UNIT_X,
            Ratio::new(0.25).unwrap(),
            Meters::new(4.5).unwrap(),
        );
        assert_ne!(a, moved);
    }

    #[test]
    fn debug_names_the_type() {
        assert!(format!("{:?}", sample()).starts_with("CurveSample"));
    }
}
