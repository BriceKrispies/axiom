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
        for component in [
            min.x, min.y, min.z, max.x, max.y, max.z,
        ] {
            if !component.is_finite() {
                return Err(MathError::non_finite_scalar(
                    "Aabb corner components must be finite",
                ));
            }
        }
        if min.x > max.x || min.y > max.y || min.z > max.z {
            return Err(MathError::invalid_aabb_bounds(
                "Aabb min must be component-wise <= max",
            ));
        }
        Ok(Aabb { min, max })
    }

    /// Construct from a center and non-negative half-extents. Fails when any
    /// extent is negative or non-finite.
    pub fn from_center_extents(center: Vec3, extents: Vec3) -> MathResult<Aabb> {
        for component in [extents.x, extents.y, extents.z] {
            if !component.is_finite() {
                return Err(MathError::non_finite_scalar(
                    "Aabb extents must be finite",
                ));
            }
            if component < 0.0 {
                return Err(MathError::invalid_aabb_bounds(
                    "Aabb extents must be non-negative",
                ));
            }
        }
        Aabb::new(center.subtract(extents), center.add(extents))
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
        p.x >= self.min.x
            && p.x <= self.max.x
            && p.y >= self.min.y
            && p.y <= self.max.y
            && p.z >= self.min.z
            && p.z <= self.max.z
    }

    /// Whether `other` is fully inside `self`.
    pub fn contains_aabb(&self, other: &Aabb) -> bool {
        self.contains_point(other.min) && self.contains_point(other.max)
    }

    /// Whether `self` and `other` share any point.
    pub fn overlaps(&self, other: &Aabb) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
            && self.min.z <= other.max.z
            && self.max.z >= other.min.z
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
        let min = Vec3::read_from(reader)
            .map_err(|cause| MathError::deserialization_failed("Aabb.min read failed", cause))?;
        let max = Vec3::read_from(reader)
            .map_err(|cause| MathError::deserialization_failed("Aabb.max read failed", cause))?;
        Aabb::new(min, max)
    }
}

impl ApproxEq for Aabb {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.min.approx_eq(&other.min, epsilon) && self.max.approx_eq(&other.max, epsilon)
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
        let a = Aabb::from_center_extents(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.5, 0.5, 0.5))
            .unwrap();
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
