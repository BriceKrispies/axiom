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
        let all_finite = [
            origin.x,
            origin.y,
            origin.z,
            direction.x,
            direction.y,
            direction.z,
        ]
        .into_iter()
        .all(|component| component.is_finite());
        all_finite
            .then_some(())
            .ok_or_else(|| MathError::non_finite_scalar("Ray components must be finite"))
            .and_then(|()| {
                direction
                    .normalize()
                    .map_err(|_| MathError::invalid_ray_direction("Ray direction must be non-zero"))
                    .map(|dir| Ray {
                        origin,
                        direction: dir,
                    })
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

    /// The ray/Aabb slab overlap range `(tmin, tmax)` when the ray's line
    /// crosses the box, or `None` on a miss. The shared core of
    /// [`Self::intersect_aabb`] (boolean form) and [`Self::intersect_aabb_entry`]
    /// (entry-distance form), so the slab arithmetic lives in exactly one place.
    fn slab_range(&self, aabb: &Aabb) -> Option<(f32, f32)> {
        let min = aabb.min();
        let max = aabb.max();
        let ds = [self.direction.x, self.direction.y, self.direction.z];
        let os = [self.origin.x, self.origin.y, self.origin.z];
        let los = [min.x, min.y, min.z];
        let his = [max.x, max.y, max.z];
        // Folds the slab test over the three axes, carrying (tmin, tmax); a miss
        // short-circuits via `Err(())`.
        (0..3)
            .try_fold((0.0f32, f32::INFINITY), |(tmin, tmax), i| {
                let d = ds[i];
                let o = os[i];
                let lo = los[i];
                let hi = his[i];
                let parallel = d.abs() < 1.0e-20;
                let parallel_miss = parallel & ((o < lo) | (o > hi));
                let inv = 1.0 / d;
                let raw_t1 = (lo - o) * inv;
                let raw_t2 = (hi - o) * inv;
                let t1 = raw_t1.min(raw_t2);
                let t2 = raw_t1.max(raw_t2);
                let updated_tmin = tmin.max(t1);
                let updated_tmax = tmax.min(t2);
                let next_tmin = [updated_tmin, tmin][usize::from(parallel)];
                let next_tmax = [updated_tmax, tmax][usize::from(parallel)];
                let slab_miss = !parallel & (next_tmin > next_tmax);
                [Ok((next_tmin, next_tmax)), Err(())][usize::from(parallel_miss | slab_miss)]
            })
            .ok()
    }

    /// Slab-test ray/Aabb intersection. Returns `true` when the ray enters the
    /// box at a non-negative parameter.
    pub fn intersect_aabb(&self, aabb: &Aabb) -> bool {
        self.slab_range(aabb).is_some_and(|(_, tmax)| tmax >= 0.0)
    }

    /// The distance along the ray at which it first enters `aabb`, or `None` if
    /// it never does. Clamped to `0` when the origin is already inside the box,
    /// so the result is always a non-negative entry distance — the form a
    /// nearest-hit query folds over to pick the closest of several boxes.
    pub fn intersect_aabb_entry(&self, aabb: &Aabb) -> Option<f32> {
        self.slab_range(aabb)
            .and_then(|(tmin, tmax)| (tmax >= 0.0).then_some(tmin.max(0.0)))
    }

    /// Geometric ray/sphere intersection test.
    pub fn intersect_sphere(&self, sphere: &Sphere) -> bool {
        let oc = self.origin.subtract(sphere.center());
        let b = oc.dot(self.direction);
        let c = oc.dot(oc) - sphere.radius() * sphere.radius();
        let discriminant = b * b - c;
        // Equivalent to: inside (c <= 0) hits; else heading away (b > 0) misses;
        // else hit iff discriminant >= 0.
        let inside = c <= 0.0;
        let heading_away = b > 0.0;
        inside | (!heading_away & (discriminant >= 0.0))
    }
}

impl ApproxEq for Ray {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.origin.approx_eq(&other.origin, epsilon)
            & self.direction.approx_eq(&other.direction, epsilon)
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
        assert!(r.point_at(2.5).approx_eq(&Vec3::new(3.5, 2.0, 3.0), eps()));
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
        let r = Ray::new(Vec3::ZERO, Vec3::UNIT_Y).unwrap();
        assert!(!r.intersect_aabb(&aabb));
    }

    #[test]
    fn ray_aabb_miss_behind() {
        let aabb = Aabb::new(Vec3::new(1.0, -1.0, -1.0), Vec3::new(2.0, 1.0, 1.0)).unwrap();
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

#[cfg(test)]
mod cov {
    use super::*;

    fn ray(o: Vec3, d: Vec3) -> Ray {
        Ray::new(o, d).unwrap()
    }

    fn cube() -> Aabb {
        Aabb::new(Vec3::ZERO, Vec3::ONE).unwrap()
    }

    #[test]
    fn origin_accessor() {
        let r = ray(Vec3::new(1.0, 2.0, 3.0), Vec3::new(1.0, 0.0, 0.0));
        assert!(r
            .origin()
            .approx_eq(&Vec3::new(1.0, 2.0, 3.0), Epsilon::DEFAULT));
    }

    #[test]
    fn parallel_axis_origin_below_box_misses() {
        let r = ray(Vec3::new(-5.0, -2.0, 0.5), Vec3::new(1.0, 0.0, 0.0));
        assert!(!r.intersect_aabb(&cube()));
    }

    #[test]
    fn parallel_axis_origin_above_box_misses() {
        let r = ray(Vec3::new(-5.0, 2.0, 0.5), Vec3::new(1.0, 0.0, 0.0));
        assert!(!r.intersect_aabb(&cube()));
    }

    #[test]
    fn parallel_axis_origin_inside_then_hits() {
        let r = ray(Vec3::new(-5.0, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0));
        assert!(r.intersect_aabb(&cube()));
    }

    #[test]
    fn tmax_tightened_by_later_axis() {
        let r = ray(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
        assert!(r.intersect_aabb(&cube()));
    }

    #[test]
    fn approx_eq_direction_differs() {
        let a = ray(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        let b = ray(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0));
        assert!(!a.approx_eq(&b, Epsilon::DEFAULT));
    }

    #[test]
    fn approx_eq_origin_differs() {
        let a = ray(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        let b = ray(Vec3::new(5.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        assert!(!a.approx_eq(&b, Epsilon::DEFAULT));
    }

    #[test]
    fn intersect_sphere_glancing_miss_reaches_discriminant() {
        let sphere = Sphere::new(Vec3::new(0.0, 5.0, 0.0), 1.0).unwrap();
        let r = ray(Vec3::new(3.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        assert!(!r.intersect_sphere(&sphere));
    }

    #[test]
    fn intersect_sphere_inside_depends_on_radius_term() {
        let sphere = Sphere::new(Vec3::ZERO, 2.5).unwrap();
        let r = ray(Vec3::new(2.0, 1.0, 1.0), Vec3::new(2.0, 1.0, 1.0));
        assert!(r.intersect_sphere(&sphere));
    }

    #[test]
    fn intersect_aabb_diagonal_hit_exact_params() {
        let aabb = Aabb::new(Vec3::new(2.0, 2.0, 2.0), Vec3::new(4.0, 4.0, 4.0)).unwrap();
        let r = ray(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0));
        assert!(r.intersect_aabb(&aabb));
    }

    #[test]
    fn intersect_aabb_slab_disjoint_miss() {
        let aabb = Aabb::new(Vec3::new(5.0, 0.0, -1.0), Vec3::new(6.0, 1.0, 1.0)).unwrap();
        let r = ray(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 5.0, 0.0));
        assert!(!r.intersect_aabb(&aabb));
    }

    #[test]
    fn intersect_aabb_negative_direction_swaps_t() {
        let aabb = Aabb::new(Vec3::new(-4.0, -1.0, -1.0), Vec3::new(-2.0, 1.0, 1.0)).unwrap();
        let r = ray(Vec3::new(0.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0));
        assert!(r.intersect_aabb(&aabb));
    }

    #[test]
    fn intersect_aabb_behind_origin_miss_via_tmax() {
        let aabb = Aabb::new(Vec3::new(-6.0, -1.0, -1.0), Vec3::new(-4.0, 1.0, 1.0)).unwrap();
        let r = ray(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        assert!(!r.intersect_aabb(&aabb));
    }

    #[test]
    fn intersect_aabb_parallel_axis_outside_misses() {
        let aabb = Aabb::new(Vec3::ZERO, Vec3::ONE).unwrap();
        let r = ray(Vec3::new(2.0, 0.5, 0.5), Vec3::new(0.0, 1.0, 0.0));
        assert!(!r.intersect_aabb(&aabb));
        let r2 = ray(Vec3::new(-1.0, 0.5, 0.5), Vec3::new(0.0, 1.0, 0.0));
        assert!(!r2.intersect_aabb(&aabb));
    }

    #[test]
    fn intersect_aabb_parallel_axis_inside_band_hits() {
        let aabb = Aabb::new(Vec3::ZERO, Vec3::ONE).unwrap();
        let r = ray(Vec3::new(0.5, -2.0, 0.5), Vec3::new(0.0, 1.0, 0.0));
        assert!(r.intersect_aabb(&aabb));
    }

    #[test]
    fn intersect_aabb_parallel_origin_on_upper_face_hits() {
        let aabb = Aabb::new(Vec3::ZERO, Vec3::ONE).unwrap();
        let r = ray(Vec3::new(1.0, -2.0, 0.5), Vec3::new(0.0, 1.0, 0.0));
        assert!(r.intersect_aabb(&aabb));
    }

    #[test]
    fn intersect_aabb_parallel_origin_on_lower_face_hits() {
        let aabb = Aabb::new(Vec3::ZERO, Vec3::ONE).unwrap();
        let r = ray(Vec3::new(0.0, -2.0, 0.5), Vec3::new(0.0, 1.0, 0.0));
        assert!(r.intersect_aabb(&aabb));
    }

    #[test]
    fn intersect_aabb_corner_graze_is_a_hit() {
        let aabb = Aabb::new(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0)).unwrap();
        let r = ray(Vec3::new(-2.0, -2.0, 1.0), Vec3::new(1.0, 1.0, 0.0));
        assert!(r.intersect_aabb(&aabb));
    }

    #[test]
    fn intersect_aabb_exit_param_uses_multiply_not_divide() {
        let aabb = Aabb::new(Vec3::new(3.0, 1.0, -1.0), Vec3::new(5.0, 4.0, 1.0)).unwrap();
        let r = ray(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0));
        assert!(r.intersect_aabb(&aabb));
    }

    #[test]
    fn intersect_aabb_corner_tangent_tmin_equals_tmax_is_hit() {
        let aabb = Aabb::new(Vec3::new(2.0, 0.0, -1.0), Vec3::new(4.0, 2.0, 3.0)).unwrap();
        let r = ray(Vec3::new(0.0, 2.0, 1.0), Vec3::new(1.0, -1.0, 0.0));
        assert!(r.intersect_aabb(&aabb));
    }

    #[test]
    fn intersect_aabb_axis_ray_exact_entry_exit() {
        let aabb = Aabb::new(Vec3::new(3.0, -1.0, -1.0), Vec3::new(7.0, 1.0, 1.0)).unwrap();
        let r = ray(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        assert!(r.intersect_aabb(&aabb));
        let r2 = ray(Vec3::new(8.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        assert!(!r2.intersect_aabb(&aabb));
    }

    #[test]
    fn intersect_aabb_entry_returns_front_face_distance() {
        let aabb = Aabb::new(Vec3::new(3.0, -1.0, -1.0), Vec3::new(7.0, 1.0, 1.0)).unwrap();
        let r = ray(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(r.intersect_aabb_entry(&aabb), Some(3.0));
    }

    #[test]
    fn intersect_aabb_entry_clamps_to_zero_when_inside() {
        let aabb = Aabb::new(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0)).unwrap();
        let r = ray(Vec3::new(1.0, 1.0, 1.0), Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(r.intersect_aabb_entry(&aabb), Some(0.0));
    }

    #[test]
    fn intersect_aabb_entry_miss_is_none() {
        let aabb = Aabb::new(Vec3::new(1.0, -1.0, -1.0), Vec3::new(2.0, 1.0, 1.0)).unwrap();
        let r = ray(Vec3::ZERO, Vec3::UNIT_Y);
        assert_eq!(r.intersect_aabb_entry(&aabb), None);
    }

    #[test]
    fn intersect_aabb_entry_behind_origin_is_none() {
        let aabb = Aabb::new(Vec3::new(-6.0, -1.0, -1.0), Vec3::new(-4.0, 1.0, 1.0)).unwrap();
        let r = ray(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(r.intersect_aabb_entry(&aabb), None);
    }
}
