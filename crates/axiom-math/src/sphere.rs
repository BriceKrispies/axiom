//! Bounding sphere.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::aabb::Aabb;
use crate::math_error::MathError;
use crate::math_result::MathResult;
use crate::ray::Ray;
use crate::vec3::Vec3;

/// A sphere with a finite, non-negative radius and a finite center.
/// Constructed via [`Sphere::new`], which validates both. Containment is
/// inclusive of the surface; overlap is inclusive of touching spheres.
#[derive(Debug, Clone, Copy)]
pub struct Sphere {
    center: Vec3,
    radius: f32,
}

impl Sphere {
    /// Construct from a finite center and a non-negative finite radius.
    pub fn new(center: Vec3, radius: f32) -> MathResult<Sphere> {
        let center_finite = [center.x, center.y, center.z]
            .into_iter()
            .all(|component| component.is_finite());
        // Error precedence mirrors the original: a non-finite center component
        // first, then a non-finite radius, then a negative radius.
        (!center_finite)
            .then_some(Err(MathError::non_finite_scalar(
                "sphere center components must be finite",
            )))
            .or_else(|| {
                (center_finite & !radius.is_finite()).then_some(Err(MathError::non_finite_scalar(
                    "sphere radius must be finite",
                )))
            })
            .or_else(|| {
                (center_finite & radius.is_finite() & (radius < 0.0)).then_some(Err(
                    MathError::invalid_sphere_radius("sphere radius must be non-negative"),
                ))
            })
            .unwrap_or(Ok(Sphere { center, radius }))
    }

    /// Center.
    pub const fn center(&self) -> Vec3 {
        self.center
    }

    /// Radius.
    pub const fn radius(&self) -> f32 {
        self.radius
    }

    /// Inclusive point containment.
    pub fn contains_point(&self, p: Vec3) -> bool {
        let d2 = p.subtract(self.center).length_squared();
        d2 <= self.radius * self.radius
    }

    /// Whether `self` and `other` share any point.
    pub fn overlaps(&self, other: &Sphere) -> bool {
        let d2 = other.center.subtract(self.center).length_squared();
        let sum = self.radius + other.radius;
        d2 <= sum * sum
    }

    /// Whether `ray` enters this sphere at a non-negative parameter `t >= 0`.
    pub fn intersects_ray(&self, ray: &Ray) -> bool {
        ray.intersect_sphere(self)
    }

    /// Whether this sphere intersects `aabb` — true iff the closest point on the
    /// box to the sphere center lies within the radius (touching counts). The
    /// box-aware companion to [`Self::overlaps`] (sphere–sphere), used by the
    /// scene's radial overlap query.
    pub fn intersects_aabb(&self, aabb: &Aabb) -> bool {
        let (min, max) = (aabb.min(), aabb.max());
        let closest = Vec3::new(
            self.center.x.clamp(min.x, max.x),
            self.center.y.clamp(min.y, max.y),
            self.center.z.clamp(min.z, max.z),
        );
        closest.subtract(self.center).length_squared() <= self.radius * self.radius
    }

    /// Append `center` (three `f32`) then `radius` (one `f32`).
    pub fn write_to(self, writer: &mut BinaryWriter) {
        self.center.write_to(writer);
        writer.write_f32(self.radius);
    }

    /// Read `center` then `radius` and revalidate.
    pub fn read_from(reader: &mut BinaryReader<'_>) -> MathResult<Sphere> {
        Vec3::read_from(reader)
            .map_err(|cause| MathError::deserialization_failed("Sphere.center read failed", cause))
            .and_then(|center| read_f32(reader).and_then(|radius| Sphere::new(center, radius)))
    }
}

fn read_f32(reader: &mut BinaryReader<'_>) -> MathResult<f32> {
    let kernel_result: KernelResult<f32> = reader.read_f32();
    kernel_result
        .map_err(|cause| MathError::deserialization_failed("Sphere.radius read failed", cause))
}

impl ApproxEq for Sphere {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.center.approx_eq(&other.center, epsilon)
            & self.radius.approx_eq(&other.radius, epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;
    use axiom_kernel::KernelApi;

    fn eps() -> Epsilon {
        Epsilon::DEFAULT
    }

    #[test]
    fn new_accepts_valid_inputs() {
        let s = Sphere::new(Vec3::new(1.0, 2.0, 3.0), 0.5).unwrap();
        assert!(s.center().approx_eq(&Vec3::new(1.0, 2.0, 3.0), eps()));
        assert_eq!(s.radius(), 0.5);
    }

    #[test]
    fn new_rejects_negative_radius() {
        let err = Sphere::new(Vec3::ZERO, -0.1).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::InvalidSphereRadius);
    }

    #[test]
    fn new_rejects_non_finite() {
        assert_eq!(
            Sphere::new(Vec3::new(f32::NAN, 0.0, 0.0), 1.0)
                .unwrap_err()
                .code(),
            MathErrorCode::NonFiniteScalar
        );
        assert_eq!(
            Sphere::new(Vec3::ZERO, f32::INFINITY).unwrap_err().code(),
            MathErrorCode::NonFiniteScalar
        );
    }

    #[test]
    fn contains_point_handles_boundary() {
        let s = Sphere::new(Vec3::ZERO, 1.0).unwrap();
        assert!(s.contains_point(Vec3::ZERO));
        assert!(s.contains_point(Vec3::UNIT_X));
        assert!(!s.contains_point(Vec3::new(1.5, 0.0, 0.0)));
    }

    #[test]
    fn overlaps_touching_spheres_returns_true() {
        let a = Sphere::new(Vec3::ZERO, 1.0).unwrap();
        let b = Sphere::new(Vec3::new(2.0, 0.0, 0.0), 1.0).unwrap();
        assert!(a.overlaps(&b));
    }

    #[test]
    fn overlaps_disjoint_spheres_returns_false() {
        let a = Sphere::new(Vec3::ZERO, 1.0).unwrap();
        let b = Sphere::new(Vec3::new(3.0, 0.0, 0.0), 1.0).unwrap();
        assert!(!a.overlaps(&b));
    }

    #[test]
    fn ray_intersection_routes_through_ray() {
        let s = Sphere::new(Vec3::new(0.0, 0.0, 5.0), 1.0).unwrap();
        let hit = Ray::new(Vec3::ZERO, Vec3::UNIT_Z).unwrap();
        let miss = Ray::new(Vec3::ZERO, Vec3::UNIT_X).unwrap();
        assert!(s.intersects_ray(&hit));
        assert!(!s.intersects_ray(&miss));
    }

    #[test]
    fn binary_round_trip_preserves_state() {
        let api = KernelApi::new();
        let s = Sphere::new(Vec3::new(1.0, -2.0, 3.0), 0.25).unwrap();
        let mut writer = api.binary_writer();
        s.write_to(&mut writer);
        let bytes = writer.into_bytes();
        let mut reader = api.binary_reader(&bytes);
        let back = Sphere::read_from(&mut reader).unwrap();
        assert!(back.approx_eq(&s, eps()));
    }

    #[test]
    fn read_from_rejects_serialized_negative_radius() {
        let api = KernelApi::new();
        let mut writer = api.binary_writer();
        Vec3::ZERO.write_to(&mut writer);
        writer.write_f32(-1.0);
        let bytes = writer.into_bytes();
        let mut reader = api.binary_reader(&bytes);
        let err = Sphere::read_from(&mut reader).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::InvalidSphereRadius);
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use axiom_kernel::BinaryReader;

    #[test]
    fn read_from_center_truncated() {
        assert!(Sphere::read_from(&mut BinaryReader::new(&[])).is_err());
    }

    #[test]
    fn read_from_radius_truncated() {
        assert!(Sphere::read_from(&mut BinaryReader::new(&[0u8; 12])).is_err());
    }

    #[test]
    fn approx_eq_radius_differs() {
        let a = Sphere::new(Vec3::ZERO, 1.0).unwrap();
        let b = Sphere::new(Vec3::ZERO, 2.0).unwrap();
        assert!(!a.approx_eq(&b, Epsilon::DEFAULT));
        assert!(a.approx_eq(&a, Epsilon::DEFAULT));
    }

    #[test]
    fn approx_eq_center_differs() {
        let a = Sphere::new(Vec3::ZERO, 1.0).unwrap();
        let b = Sphere::new(Vec3::new(5.0, 0.0, 0.0), 1.0).unwrap();
        assert!(!a.approx_eq(&b, Epsilon::DEFAULT));
    }

    #[test]
    fn contains_point_radius_squared_term() {
        let s = Sphere::new(Vec3::ZERO, 3.0).unwrap();
        assert!(s.contains_point(Vec3::new(2.0, 2.0, 0.0)));
        assert!(!s.contains_point(Vec3::new(5.0, 0.0, 0.0)));
    }

    #[test]
    fn overlaps_sum_of_radii_term() {
        let a = Sphere::new(Vec3::ZERO, 1.0).unwrap();
        let b = Sphere::new(Vec3::new(2.0, 2.0, 2.0), 3.0).unwrap();
        assert!(a.overlaps(&b));
    }

    #[test]
    fn intersects_aabb_covers_inside_near_and_far() {
        let box_ = Aabb::from_center_extents(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap();
        assert!(Sphere::new(Vec3::ZERO, 0.1).unwrap().intersects_aabb(&box_));
        assert!(Sphere::new(Vec3::new(1.4, 0.0, 0.0), 0.5)
            .unwrap()
            .intersects_aabb(&box_));
        assert!(Sphere::new(Vec3::new(1.5, 0.0, 0.0), 0.5)
            .unwrap()
            .intersects_aabb(&box_));
        assert!(!Sphere::new(Vec3::new(3.0, 0.0, 0.0), 0.5)
            .unwrap()
            .intersects_aabb(&box_));
    }
}
