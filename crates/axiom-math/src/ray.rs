//! A ray with a normalized direction.

use crate::aabb::Aabb;
use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::math_error::MathError;
use crate::math_result::MathResult;
use crate::sphere::Sphere;
use crate::vec3::Vec3;

/// A half-infinite line `origin + t * direction` for `t >= 0`, with `direction`
/// stored as a unit vector.
///
/// [`Ray::new`] normalizes the direction and rejects zero-length / non-finite
/// inputs, so every other method can trust the unit-direction invariant.
#[derive(Debug, Clone, Copy)]
pub struct Ray {
    origin: Vec3,
    direction: Vec3,
}

impl Ray {
    /// Construct from an origin and a non-zero, finite direction. The
    /// direction is normalized on the way in.
    pub fn new(origin: Vec3, direction: Vec3) -> MathResult<Ray> {
        for component in [
            origin.x, origin.y, origin.z, direction.x, direction.y, direction.z,
        ] {
            if !component.is_finite() {
                return Err(MathError::non_finite_scalar(
                    "Ray components must be finite",
                ));
            }
        }
        let dir = direction.normalize().map_err(|_| {
            MathError::invalid_ray_direction("Ray direction must be non-zero")
        })?;
        Ok(Ray {
            origin,
            direction: dir,
        })
    }

    /// Origin point.
    pub const fn origin(&self) -> Vec3 {
        self.origin
    }

    /// Unit direction.
    pub const fn direction(&self) -> Vec3 {
        self.direction
    }

    /// `origin + t * direction`.
    pub fn point_at(&self, t: f32) -> Vec3 {
        self.origin.add(self.direction.mul_scalar(t))
    }

    /// Slab-test ray/Aabb intersection. Returns `true` when the ray enters the
    /// box at a non-negative parameter.
    pub fn intersect_aabb(&self, aabb: &Aabb) -> bool {
        let mut tmin: f32 = 0.0;
        let mut tmax: f32 = f32::INFINITY;
        let min = aabb.min();
        let max = aabb.max();
        let ds = [self.direction.x, self.direction.y, self.direction.z];
        let os = [self.origin.x, self.origin.y, self.origin.z];
        let los = [min.x, min.y, min.z];
        let his = [max.x, max.y, max.z];
        for i in 0..3 {
            let d = ds[i];
            let o = os[i];
            let lo = los[i];
            let hi = his[i];
            if d.abs() < 1.0e-20 {
                if o < lo || o > hi {
                    return false;
                }
            } else {
                let inv = 1.0 / d;
                let mut t1 = (lo - o) * inv;
                let mut t2 = (hi - o) * inv;
                if t1 > t2 {
                    core::mem::swap(&mut t1, &mut t2);
                }
                if t1 > tmin {
                    tmin = t1;
                }
                if t2 < tmax {
                    tmax = t2;
                }
                if tmin > tmax {
                    return false;
                }
            }
        }
        tmax >= 0.0
    }

    /// Geometric ray/sphere intersection test.
    pub fn intersect_sphere(&self, sphere: &Sphere) -> bool {
        let oc = self.origin.subtract(sphere.center());
        let b = oc.dot(self.direction);
        let c = oc.dot(oc) - sphere.radius() * sphere.radius();
        // Origin already inside the sphere counts as a hit.
        if c <= 0.0 {
            return true;
        }
        // Origin outside but heading away.
        if b > 0.0 {
            return false;
        }
        let discriminant = b * b - c;
        discriminant >= 0.0
    }
}

impl ApproxEq for Ray {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.origin.approx_eq(&other.origin, epsilon)
            && self.direction.approx_eq(&other.direction, epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    #[test]
    fn new_normalizes_direction() {
        let r = Ray::new(Vec3::ZERO, Vec3::new(0.0, 0.0, 5.0)).unwrap();
        assert!(r.direction().approx_eq(&Vec3::UNIT_Z, eps()));
        assert!((r.direction().length() - 1.0).abs() <= eps().value());
    }

    #[test]
    fn new_rejects_zero_direction() {
        let err = Ray::new(Vec3::ZERO, Vec3::ZERO).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::InvalidRayDirection);
    }

    #[test]
    fn new_rejects_non_finite() {
        let err = Ray::new(Vec3::new(f32::NAN, 0.0, 0.0), Vec3::UNIT_X).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NonFiniteScalar);
    }

    #[test]
    fn point_at_advances_along_direction() {
        let r = Ray::new(Vec3::new(1.0, 2.0, 3.0), Vec3::UNIT_X).unwrap();
        assert!(r
            .point_at(2.5)
            .approx_eq(&Vec3::new(3.5, 2.0, 3.0), eps()));
    }

    #[test]
    fn ray_aabb_hit() {
        let aabb = Aabb::new(Vec3::new(1.0, -1.0, -1.0), Vec3::new(2.0, 1.0, 1.0)).unwrap();
        let r = Ray::new(Vec3::ZERO, Vec3::UNIT_X).unwrap();
        assert!(r.intersect_aabb(&aabb));
    }

    #[test]
    fn ray_aabb_miss_parallel() {
        let aabb = Aabb::new(Vec3::new(1.0, -1.0, -1.0), Vec3::new(2.0, 1.0, 1.0)).unwrap();
        // Ray going up along Y from origin never crosses the X-slab [1, 2].
        let r = Ray::new(Vec3::ZERO, Vec3::UNIT_Y).unwrap();
        assert!(!r.intersect_aabb(&aabb));
    }

    #[test]
    fn ray_aabb_miss_behind() {
        let aabb = Aabb::new(Vec3::new(1.0, -1.0, -1.0), Vec3::new(2.0, 1.0, 1.0)).unwrap();
        // Ray going in -X never crosses x=1.
        let r = Ray::new(Vec3::ZERO, Vec3::new(-1.0, 0.0, 0.0)).unwrap();
        assert!(!r.intersect_aabb(&aabb));
    }

    #[test]
    fn ray_sphere_hit() {
        let sphere = Sphere::new(Vec3::new(0.0, 0.0, 5.0), 1.0).unwrap();
        let r = Ray::new(Vec3::ZERO, Vec3::UNIT_Z).unwrap();
        assert!(r.intersect_sphere(&sphere));
    }

    #[test]
    fn ray_sphere_miss_glancing() {
        let sphere = Sphere::new(Vec3::new(0.0, 0.0, 5.0), 1.0).unwrap();
        // Aimed off to the side.
        let r = Ray::new(Vec3::new(5.0, 0.0, 0.0), Vec3::UNIT_X).unwrap();
        assert!(!r.intersect_sphere(&sphere));
    }

    #[test]
    fn ray_sphere_origin_inside_hits() {
        let sphere = Sphere::new(Vec3::ZERO, 1.0).unwrap();
        let r = Ray::new(Vec3::ZERO, Vec3::UNIT_X).unwrap();
        assert!(r.intersect_sphere(&sphere));
    }

    #[test]
    fn ray_sphere_behind_misses() {
        let sphere = Sphere::new(Vec3::new(0.0, 0.0, 5.0), 1.0).unwrap();
        let r = Ray::new(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0)).unwrap();
        assert!(!r.intersect_sphere(&sphere));
    }

    #[test]
    fn approx_eq_compares_origin_and_direction() {
        let a = Ray::new(Vec3::ZERO, Vec3::UNIT_X).unwrap();
        let b = Ray::new(Vec3::ZERO, Vec3::UNIT_X).unwrap();
        let c = Ray::new(Vec3::ZERO, Vec3::UNIT_Y).unwrap();
        assert!(a.approx_eq(&b, eps()));
        assert!(!a.approx_eq(&c, eps()));
    }
}
