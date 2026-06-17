//! Axis-aligned bounding box.

use axiom_kernel::{BinaryReader, BinaryWriter};

use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::math_error::MathError;
use crate::math_result::MathResult;
use crate::vec3::Vec3;

/// A non-empty axis-aligned bounding box defined by inclusive `min`/`max`
/// corners.
///
/// Invariants enforced at construction: `min.x <= max.x`,
/// `min.y <= max.y`, `min.z <= max.z`, and all components are finite. There
/// is no notion of an "empty AABB" — that is the caller's responsibility, so
/// boolean tests in this type stay deterministic.
#[derive(Debug, Clone, Copy)]
pub struct Aabb {
    min: Vec3,
    max: Vec3,
}

impl Aabb {
    /// Construct from explicit corners. Fails if `min > max` on any axis or
    /// if any component is not finite.
    pub fn new(min: Vec3, max: Vec3) -> MathResult<Aabb> {
        let all_finite = [min.x, min.y, min.z, max.x, max.y, max.z]
            .into_iter()
            .all(|component| component.is_finite());
        let inverted = (min.x > max.x) | (min.y > max.y) | (min.z > max.z);
        (!all_finite)
            .then_some(Err(MathError::non_finite_scalar(
                "Aabb corner components must be finite",
            )))
            .or_else(|| {
                (all_finite & inverted).then_some(Err(MathError::invalid_aabb_bounds(
                    "Aabb min must be component-wise <= max",
                )))
            })
            .unwrap_or(Ok(Aabb { min, max }))
    }

    /// Construct from a center and non-negative half-extents. Fails when any
    /// extent is negative or non-finite.
    pub fn from_center_extents(center: Vec3, extents: Vec3) -> MathResult<Aabb> {
        // First offending extent (in x, y, z order) decides the error, with
        // non-finite taking priority over negative for a given component.
        let extent_error: Option<MathError> = [extents.x, extents.y, extents.z]
            .into_iter()
            .find_map(|component| {
                (!component.is_finite())
                    .then_some(MathError::non_finite_scalar("Aabb extents must be finite"))
                    .or_else(|| {
                        (component < 0.0).then_some(MathError::invalid_aabb_bounds(
                            "Aabb extents must be non-negative",
                        ))
                    })
            });
        extent_error
            .map(Err)
            .unwrap_or_else(|| Aabb::new(center.subtract(extents), center.add(extents)))
    }

    /// Minimum corner.
    pub const fn min(&self) -> Vec3 {
        self.min
    }

    /// Maximum corner.
    pub const fn max(&self) -> Vec3 {
        self.max
    }

    /// Center (`(min + max) / 2`).
    pub fn center(&self) -> Vec3 {
        self.min.add(self.max).mul_scalar(0.5)
    }

    /// Half-extents (`(max - min) / 2`).
    pub fn extents(&self) -> Vec3 {
        self.max.subtract(self.min).mul_scalar(0.5)
    }

    /// Inclusive point containment.
    pub fn contains_point(&self, p: Vec3) -> bool {
        (p.x >= self.min.x)
            & (p.x <= self.max.x)
            & (p.y >= self.min.y)
            & (p.y <= self.max.y)
            & (p.z >= self.min.z)
            & (p.z <= self.max.z)
    }

    /// Whether `other` is fully inside `self`.
    pub fn contains_aabb(&self, other: &Aabb) -> bool {
        self.contains_point(other.min) & self.contains_point(other.max)
    }

    /// Whether `self` and `other` share any point.
    pub fn overlaps(&self, other: &Aabb) -> bool {
        (self.min.x <= other.max.x)
            & (self.max.x >= other.min.x)
            & (self.min.y <= other.max.y)
            & (self.max.y >= other.min.y)
            & (self.min.z <= other.max.z)
            & (self.max.z >= other.min.z)
    }

    /// Union: smallest AABB containing both `self` and `other`.
    pub fn merge(&self, other: &Aabb) -> Aabb {
        let min = Vec3::new(
            self.min.x.min(other.min.x),
            self.min.y.min(other.min.y),
            self.min.z.min(other.min.z),
        );
        let max = Vec3::new(
            self.max.x.max(other.max.x),
            self.max.y.max(other.max.y),
            self.max.z.max(other.max.z),
        );
        Aabb { min, max }
    }

    /// Smallest AABB containing `self` and the point `p`.
    pub fn expand(&self, p: Vec3) -> Aabb {
        let min = Vec3::new(
            self.min.x.min(p.x),
            self.min.y.min(p.y),
            self.min.z.min(p.z),
        );
        let max = Vec3::new(
            self.max.x.max(p.x),
            self.max.y.max(p.y),
            self.max.z.max(p.z),
        );
        Aabb { min, max }
    }

    /// Append `min` then `max` (six `f32`).
    pub fn write_to(self, writer: &mut BinaryWriter) {
        self.min.write_to(writer);
        self.max.write_to(writer);
    }

    /// Read `min` then `max` and validate the resulting AABB.
    pub fn read_from(reader: &mut BinaryReader<'_>) -> MathResult<Aabb> {
        Vec3::read_from(reader)
            .map_err(|cause| MathError::deserialization_failed("Aabb.min read failed", cause))
            .and_then(|min| {
                Vec3::read_from(reader)
                    .map_err(|cause| {
                        MathError::deserialization_failed("Aabb.max read failed", cause)
                    })
                    .and_then(|max| Aabb::new(min, max))
            })
    }
}

impl ApproxEq for Aabb {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.min.approx_eq(&other.min, epsilon) & self.max.approx_eq(&other.max, epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;
    use axiom_kernel::KernelApi;

    fn unit_cube() -> Aabb {
        Aabb::new(Vec3::ZERO, Vec3::ONE).unwrap()
    }

    fn eps() -> Epsilon {
        Epsilon::DEFAULT
    }

    #[test]
    fn new_accepts_valid_bounds() {
        let a = unit_cube();
        assert!(a.min().approx_eq(&Vec3::ZERO, eps()));
        assert!(a.max().approx_eq(&Vec3::ONE, eps()));
    }

    #[test]
    fn new_rejects_inverted_bounds() {
        let err = Aabb::new(Vec3::ONE, Vec3::ZERO).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::InvalidAabbBounds);
    }

    #[test]
    fn new_rejects_non_finite_bounds() {
        let err = Aabb::new(Vec3::new(f32::NAN, 0.0, 0.0), Vec3::ONE).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NonFiniteScalar);
    }

    #[test]
    fn from_center_extents_works() {
        let a =
            Aabb::from_center_extents(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.5, 0.5, 0.5)).unwrap();
        assert!(a.min().approx_eq(&Vec3::new(0.5, 1.5, 2.5), eps()));
        assert!(a.max().approx_eq(&Vec3::new(1.5, 2.5, 3.5), eps()));
    }

    #[test]
    fn from_center_extents_rejects_negative_extents() {
        let err = Aabb::from_center_extents(Vec3::ZERO, Vec3::new(-0.1, 0.0, 0.0)).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::InvalidAabbBounds);
    }

    #[test]
    fn center_and_extents_are_consistent() {
        let a = Aabb::new(Vec3::new(-1.0, -2.0, -3.0), Vec3::new(1.0, 2.0, 3.0)).unwrap();
        assert!(a.center().approx_eq(&Vec3::ZERO, eps()));
        assert!(a.extents().approx_eq(&Vec3::new(1.0, 2.0, 3.0), eps()));
    }

    #[test]
    fn contains_point_handles_boundary() {
        let a = unit_cube();
        assert!(a.contains_point(Vec3::new(0.5, 0.5, 0.5)));
        assert!(a.contains_point(Vec3::ZERO));
        assert!(a.contains_point(Vec3::ONE));
        assert!(!a.contains_point(Vec3::new(-0.1, 0.0, 0.0)));
        assert!(!a.contains_point(Vec3::new(1.1, 0.5, 0.5)));
    }

    #[test]
    fn contains_aabb_is_true_only_when_fully_inside() {
        let outer = Aabb::new(Vec3::new(-2.0, -2.0, -2.0), Vec3::new(2.0, 2.0, 2.0)).unwrap();
        let inner = unit_cube();
        assert!(outer.contains_aabb(&inner));
        assert!(!inner.contains_aabb(&outer));
    }

    #[test]
    fn overlaps_handles_touching_and_disjoint() {
        let a = unit_cube();
        let touching = Aabb::new(Vec3::ONE, Vec3::new(2.0, 2.0, 2.0)).unwrap();
        let disjoint = Aabb::new(Vec3::new(2.0, 2.0, 2.0), Vec3::new(3.0, 3.0, 3.0)).unwrap();
        assert!(a.overlaps(&touching));
        assert!(!a.overlaps(&disjoint));
    }

    #[test]
    fn merge_takes_outer_envelope() {
        let a = Aabb::new(Vec3::ZERO, Vec3::ONE).unwrap();
        let b = Aabb::new(Vec3::new(-1.0, 0.5, 0.0), Vec3::new(2.0, 0.6, 0.0)).unwrap();
        let merged = a.merge(&b);
        assert!(merged.min().approx_eq(&Vec3::new(-1.0, 0.0, 0.0), eps()));
        assert!(merged.max().approx_eq(&Vec3::new(2.0, 1.0, 1.0), eps()));
    }

    #[test]
    fn expand_grows_to_include_point() {
        let a = unit_cube();
        let bigger = a.expand(Vec3::new(2.0, -1.0, 0.5));
        assert!(bigger.min().approx_eq(&Vec3::new(0.0, -1.0, 0.0), eps()));
        assert!(bigger.max().approx_eq(&Vec3::new(2.0, 1.0, 1.0), eps()));
    }

    #[test]
    fn binary_round_trip_preserves_corners() {
        let api = KernelApi::new();
        let a = Aabb::new(Vec3::new(-1.0, -2.0, -3.0), Vec3::new(1.0, 2.0, 3.0)).unwrap();
        let mut writer = api.binary_writer();
        a.write_to(&mut writer);
        let bytes = writer.into_bytes();
        let mut reader = api.binary_reader(&bytes);
        let back = Aabb::read_from(&mut reader).unwrap();
        assert!(back.approx_eq(&a, eps()));
    }

    #[test]
    fn read_from_rejects_inverted_serialized_bounds() {
        let api = KernelApi::new();
        let mut writer = api.binary_writer();
        // Hand-encode min=(1,1,1), max=(0,0,0).
        Vec3::ONE.write_to(&mut writer);
        Vec3::ZERO.write_to(&mut writer);
        let bytes = writer.into_bytes();
        let mut reader = api.binary_reader(&bytes);
        let err = Aabb::read_from(&mut reader).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::InvalidAabbBounds);
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use axiom_kernel::BinaryReader;

    fn cube() -> Aabb {
        Aabb::new(Vec3::ZERO, Vec3::ONE).unwrap()
    }

    #[test]
    fn new_rejects_each_axis_inverted() {
        assert!(Aabb::new(Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 1.0)).is_err());
        assert!(Aabb::new(Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 0.0, 1.0)).is_err());
        assert!(Aabb::new(Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 1.0, 0.0)).is_err());
    }

    #[test]
    fn from_center_extents_rejects_non_finite() {
        assert!(Aabb::from_center_extents(Vec3::ZERO, Vec3::new(f32::NAN, 0.0, 0.0)).is_err());
    }

    #[test]
    fn from_center_extents_rejects_negative() {
        assert!(Aabb::from_center_extents(Vec3::ZERO, Vec3::new(-1.0, 0.0, 0.0)).is_err());
    }

    #[test]
    fn from_center_extents_accepts_valid() {
        assert!(Aabb::from_center_extents(Vec3::ZERO, Vec3::ONE).is_ok());
    }

    #[test]
    fn contains_point_each_axis_outside() {
        let c = cube();
        assert!(c.contains_point(Vec3::new(0.5, 0.5, 0.5)));
        assert!(!c.contains_point(Vec3::new(-1.0, 0.5, 0.5)));
        assert!(!c.contains_point(Vec3::new(2.0, 0.5, 0.5)));
        assert!(!c.contains_point(Vec3::new(0.5, -1.0, 0.5)));
        assert!(!c.contains_point(Vec3::new(0.5, 2.0, 0.5)));
        assert!(!c.contains_point(Vec3::new(0.5, 0.5, -1.0)));
        assert!(!c.contains_point(Vec3::new(0.5, 0.5, 2.0)));
    }

    #[test]
    fn overlaps_each_axis_separated() {
        let c = cube();
        assert!(c.overlaps(&cube()));
        assert!(
            !c.overlaps(&Aabb::new(Vec3::new(2.0, 0.0, 0.0), Vec3::new(3.0, 1.0, 1.0)).unwrap())
        );
        assert!(
            !c.overlaps(&Aabb::new(Vec3::new(-3.0, 0.0, 0.0), Vec3::new(-2.0, 1.0, 1.0)).unwrap())
        );
        assert!(
            !c.overlaps(&Aabb::new(Vec3::new(0.0, 2.0, 0.0), Vec3::new(1.0, 3.0, 1.0)).unwrap())
        );
        assert!(
            !c.overlaps(&Aabb::new(Vec3::new(0.0, -3.0, 0.0), Vec3::new(1.0, -2.0, 1.0)).unwrap())
        );
        assert!(
            !c.overlaps(&Aabb::new(Vec3::new(0.0, 0.0, 2.0), Vec3::new(1.0, 1.0, 3.0)).unwrap())
        );
        assert!(
            !c.overlaps(&Aabb::new(Vec3::new(0.0, 0.0, -3.0), Vec3::new(1.0, 1.0, -2.0)).unwrap())
        );
    }

    #[test]
    fn read_from_min_truncated() {
        let mut r = BinaryReader::new(&[]);
        assert!(Aabb::read_from(&mut r).is_err());
    }

    #[test]
    fn read_from_max_truncated() {
        let mut r = BinaryReader::new(&[0u8; 12]);
        assert!(Aabb::read_from(&mut r).is_err());
    }

    #[test]
    fn approx_eq_max_differs() {
        let a = cube();
        let b = Aabb::new(Vec3::ZERO, Vec3::new(1.0, 1.0, 2.0)).unwrap();
        assert!(!a.approx_eq(&b, Epsilon::DEFAULT));
    }

    #[test]
    fn approx_eq_min_differs() {
        let a = cube();
        let b = Aabb::new(Vec3::new(-1.0, 0.0, 0.0), Vec3::ONE).unwrap();
        assert!(!a.approx_eq(&b, Epsilon::DEFAULT));
    }

    // Kills `replace < with <=` / `replace < with ==` at aabb.rs:54:26.
    // The guard is `extents component < 0.0`. A *zero* extent is the boundary:
    // `< 0.0` accepts it, but `<= 0.0` and `== 0.0` would (wrongly) reject it.
    #[test]
    fn from_center_extents_accepts_exactly_zero_extent() {
        let a =
            Aabb::from_center_extents(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 1.0, 1.0)).unwrap();
        assert!(a
            .min()
            .approx_eq(&Vec3::new(1.0, 1.0, 2.0), Epsilon::DEFAULT));
        assert!(a
            .max()
            .approx_eq(&Vec3::new(1.0, 3.0, 4.0), Epsilon::DEFAULT));
        // And a negative extent must still be rejected (pins the `< 0.0` sense).
        assert!(Aabb::from_center_extents(Vec3::ZERO, Vec3::new(-0.5, 0.0, 0.0)).is_err());
    }

    // Kills `replace && with ||` at aabb.rs:95:40 in `contains_aabb`.
    // `other.min` is inside `self` but `other.max` is outside: `&&` => false,
    // `||` => true.
    #[test]
    fn contains_aabb_requires_both_corners_inside() {
        let outer = cube();
        let partially_outside =
            Aabb::new(Vec3::new(0.5, 0.5, 0.5), Vec3::new(2.0, 2.0, 2.0)).unwrap();
        assert!(outer.contains_point(partially_outside.min()));
        assert!(!outer.contains_point(partially_outside.max()));
        assert!(!outer.contains_aabb(&partially_outside));
        // Mirror case: min outside, max inside.
        let other = Aabb::new(Vec3::new(-2.0, -2.0, -2.0), Vec3::new(0.5, 0.5, 0.5)).unwrap();
        assert!(!outer.contains_aabb(&other));
    }
}
