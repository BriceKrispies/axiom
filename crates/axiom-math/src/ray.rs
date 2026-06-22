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

    /// Slab-test ray/Aabb intersection. Returns `true` when the ray enters the
    /// box at a non-negative parameter.
    pub fn intersect_aabb(&self, aabb: &Aabb) -> bool {
        let min = aabb.min();
        let max = aabb.max();
        let ds = [self.direction.x, self.direction.y, self.direction.z];
        let os = [self.origin.x, self.origin.y, self.origin.z];
        let los = [min.x, min.y, min.z];
        let his = [max.x, max.y, max.z];
        // Fold the slab test over the three axes, carrying (tmin, tmax). Any
        // axis that proves a miss short-circuits via `Err(false)`, mirroring the
        // original mid-loop `return false`. All scalar conditionals are replaced
        // by `min`/`max` (bit-identical for the finite/inf values that arise
        // here, since no operand is NaN) and boolean algebra.
        let folded = (0..3).try_fold((0.0f32, f32::INFINITY), |(tmin, tmax), i| {
            let d = ds[i];
            let o = os[i];
            let lo = los[i];
            let hi = his[i];
            let parallel = d.abs() < 1.0e-20;
            let parallel_miss = parallel & ((o < lo) | (o > hi));
            // Non-parallel slab update. Computed unconditionally (pure, no
            // panic); only applied when `!parallel`.
            let inv = 1.0 / d;
            let raw_t1 = (lo - o) * inv;
            let raw_t2 = (hi - o) * inv;
            let t1 = raw_t1.min(raw_t2); // post-swap lower bound
            let t2 = raw_t1.max(raw_t2); // post-swap upper bound
            let updated_tmin = tmin.max(t1);
            let updated_tmax = tmax.min(t2);
            let next_tmin = parallel.then_some(tmin).unwrap_or(updated_tmin);
            let next_tmax = parallel.then_some(tmax).unwrap_or(updated_tmax);
            let slab_miss = !parallel & (next_tmin > next_tmax);
            (parallel_miss | slab_miss)
                .then_some(Err(false))
                .unwrap_or(Ok((next_tmin, next_tmax)))
        });
        folded.map_or(false, |(_, tmax)| tmax >= 0.0)
    }

    /// Geometric ray/sphere intersection test.
    pub fn intersect_sphere(&self, sphere: &Sphere) -> bool {
        let oc = self.origin.subtract(sphere.center());
        let b = oc.dot(self.direction);
        let c = oc.dot(oc) - sphere.radius() * sphere.radius();
        let discriminant = b * b - c;
        // Equivalent to: inside (c <= 0) -> hit; else outside heading away
        // (b > 0) -> miss; else hit iff discriminant >= 0. All operands are
        // finite and side-effect free, so this evaluates the same boolean.
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
        // Direction along x; y component near-zero, origin.y below box.
        let r = ray(Vec3::new(-5.0, -2.0, 0.5), Vec3::new(1.0, 0.0, 0.0));
        assert!(!r.intersect_aabb(&cube()));
    }

    #[test]
    fn parallel_axis_origin_above_box_misses() {
        // Direction along x; y component near-zero, origin.y above box.
        let r = ray(Vec3::new(-5.0, 2.0, 0.5), Vec3::new(1.0, 0.0, 0.0));
        assert!(!r.intersect_aabb(&cube()));
    }

    #[test]
    fn parallel_axis_origin_inside_then_hits() {
        // Direction along x; y,z near-zero but origin within box on those axes.
        let r = ray(Vec3::new(-5.0, 0.5, 0.5), Vec3::new(1.0, 0.0, 0.0));
        assert!(r.intersect_aabb(&cube()));
    }

    #[test]
    fn tmax_tightened_by_later_axis() {
        // Diagonal ray so multiple axes update tmin/tmax.
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

    // Kills intersect_sphere 113:34 (`b*b - c` -> `b*b + c`). A glancing MISS
    // where the origin is outside (c > 0) and heading toward the sphere (b < 0)
    // so the discriminant branch is reached: b*b - c < 0 (miss) while the
    // mutated b*b + c is always positive (would falsely hit).
    #[test]
    fn intersect_sphere_glancing_miss_reaches_discriminant() {
        let sphere = Sphere::new(Vec3::new(0.0, 5.0, 0.0), 1.0).unwrap();
        // origin (3,0,0), dir +Y: oc=(3,-5,0), b=-5 (<0), c=33 (>0),
        // discriminant = 25 - 33 = -8 < 0 -> miss.
        let r = ray(Vec3::new(3.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));
        assert!(!r.intersect_sphere(&sphere));
    }

    // ---- intersect_sphere: kills 104:28 (`- r*r` -> `/ r*r`) and 104:46
    // (`r*r` -> `r+r` / `r/r`). Origin lies INSIDE the sphere (c <= 0) only when
    // c is computed as `oc.dot(oc) - r*r`. The mutated c values are positive,
    // so combined with an outward-heading ray (b > 0) the mutants miss. ----
    #[test]
    fn intersect_sphere_inside_depends_on_radius_term() {
        let sphere = Sphere::new(Vec3::ZERO, 2.5).unwrap();
        // oc.dot(oc) = 4 + 1 + 1 = 6; r*r = 6.25 -> c = -0.25 (inside -> hit).
        // r+r = 5 -> c = 1 > 0; r/r = 1 -> c = 5 > 0; (6)/(6.25) ~ 0.96 > 0.
        let r = ray(Vec3::new(2.0, 1.0, 1.0), Vec3::new(2.0, 1.0, 1.0)); // heading outward
        assert!(r.intersect_sphere(&sphere));
    }

    // ---- intersect_aabb battery ----
    // A clean diagonal hit with exact slab parameters and a clean miss pin the
    // slab arithmetic (80 `1.0/d`, 81/82 `(lo-o)*inv`, 83 swap, 86/89/92 the
    // tmin/tmax updates and overlap test).
    #[test]
    fn intersect_aabb_diagonal_hit_exact_params() {
        // Box [2,4]^3. Ray from origin along (1,1,1): enters all slabs at t=2,
        // exits at t=4. tmin=2, tmax=4, hit.
        let aabb = Aabb::new(Vec3::new(2.0, 2.0, 2.0), Vec3::new(4.0, 4.0, 4.0)).unwrap();
        let r = ray(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0));
        assert!(r.intersect_aabb(&aabb));
    }

    #[test]
    fn intersect_aabb_slab_disjoint_miss() {
        // Box where the X-slab [5,6] is entered AFTER the Y-slab [0,1] is left,
        // so tmin > tmax must trigger a miss (pins 86/89/92 and the swap at 83).
        let aabb = Aabb::new(Vec3::new(5.0, 0.0, -1.0), Vec3::new(6.0, 1.0, 1.0)).unwrap();
        // Ray aimed so it crosses the X band far later than the Y band.
        let r = ray(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 5.0, 0.0));
        assert!(!r.intersect_aabb(&aabb));
    }

    #[test]
    fn intersect_aabb_negative_direction_swaps_t() {
        // Heading -X toward a box behind +X start: requires the t1>t2 swap (83)
        // and correct (lo-o)*inv / (hi-o)*inv signs (81/82) to register the hit.
        let aabb = Aabb::new(Vec3::new(-4.0, -1.0, -1.0), Vec3::new(-2.0, 1.0, 1.0)).unwrap();
        let r = ray(Vec3::new(0.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0));
        assert!(r.intersect_aabb(&aabb));
    }

    #[test]
    fn intersect_aabb_behind_origin_miss_via_tmax() {
        // Box entirely behind the ray origin along its direction: tmax < 0 so
        // the final `tmax >= 0` test must reject it (pins 89 and the return).
        let aabb = Aabb::new(Vec3::new(-6.0, -1.0, -1.0), Vec3::new(-4.0, 1.0, 1.0)).unwrap();
        let r = ray(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        assert!(!r.intersect_aabb(&aabb));
    }

    // Kills the parallel-axis guard at 75 (`d.abs() < 1e-20` -> `<=` / `==`) and
    // the in-slab bounds test at 76 (`o < lo` -> `<=`, `o > hi` -> `>=`).
    #[test]
    fn intersect_aabb_parallel_axis_outside_misses() {
        let aabb = Aabb::new(Vec3::ZERO, Vec3::ONE).unwrap();
        // Exactly parallel to X (d.x == 0). Origin x = 2.0 is OUTSIDE [0,1]:
        // parallel branch must return false. (`== 1e-20` mutant would instead
        // take the slab branch with inv = 1/0 and behave differently.)
        let r = ray(Vec3::new(2.0, 0.5, 0.5), Vec3::new(0.0, 1.0, 0.0));
        assert!(!r.intersect_aabb(&aabb));
        // Origin x below the box, still parallel: also a miss.
        let r2 = ray(Vec3::new(-1.0, 0.5, 0.5), Vec3::new(0.0, 1.0, 0.0));
        assert!(!r2.intersect_aabb(&aabb));
    }

    #[test]
    fn intersect_aabb_parallel_axis_inside_band_hits() {
        let aabb = Aabb::new(Vec3::ZERO, Vec3::ONE).unwrap();
        // Parallel to X with origin x = 0.5 inside [0,1]; the moving Y axis then
        // carries the ray through the box -> hit. Distinguishes 75/76 from the
        // mutants that would mishandle the in-band parallel case.
        let r = ray(Vec3::new(0.5, -2.0, 0.5), Vec3::new(0.0, 1.0, 0.0));
        assert!(r.intersect_aabb(&aabb));
    }

    // Parallel axis with origin EXACTLY on the far face boundary (o == hi).
    // Pins 76:32 (`o > hi` -> `>=`): with `>`, o == hi stays in-band (continue);
    // with `>=` it would (wrongly) reject. The ray then hits, so `>=` flips it.
    #[test]
    fn intersect_aabb_parallel_origin_on_upper_face_hits() {
        let aabb = Aabb::new(Vec3::ZERO, Vec3::ONE).unwrap();
        // o.x == 1.0 == hi exactly; parallel to X; moving along Y through the box.
        let r = ray(Vec3::new(1.0, -2.0, 0.5), Vec3::new(0.0, 1.0, 0.0));
        assert!(r.intersect_aabb(&aabb));
    }

    // Parallel axis with origin EXACTLY on the near face (o == lo). Pins 76:22
    // (`o < lo` -> `<=`/`==`): with `<`, o == lo stays in-band; mutated forms
    // would reject and the ray would miss.
    #[test]
    fn intersect_aabb_parallel_origin_on_lower_face_hits() {
        let aabb = Aabb::new(Vec3::ZERO, Vec3::ONE).unwrap();
        let r = ray(Vec3::new(0.0, -2.0, 0.5), Vec3::new(0.0, 1.0, 0.0));
        assert!(r.intersect_aabb(&aabb));
    }

    // A ray that just grazes a corner: the slab where tmin == tmax (edge hit).
    // Pins 92:25 (`tmin > tmax` -> `>=`): with `>`, tmin == tmax is kept (a
    // touching hit); with `>=` it would be rejected.
    #[test]
    fn intersect_aabb_corner_graze_is_a_hit() {
        // Box [0,2]^3. Ray entering the +X face at the exact top edge so the
        // x-slab and y-slab close at the same parameter.
        let aabb = Aabb::new(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0)).unwrap();
        // From (-2, 0, 1) along +X: enters x at t=2 (x=0), and y stays at 0
        // (lo edge). Use a 45-degree approach in XY to force tmin==tmax.
        let r = ray(Vec3::new(-2.0, -2.0, 1.0), Vec3::new(1.0, 1.0, 0.0));
        // Enters x-slab [0,2] over t in [2, 4]/|d|, y-slab [0,2] over the same
        // window; tmin and tmax coincide at the shared diagonal -> touching hit.
        assert!(r.intersect_aabb(&aabb));
    }

    // Kills 82:39 (`t2 = (hi - o) * inv` -> `/ inv`). A 45-degree ray in XY has
    // inv = sqrt(2) != 1, so `* inv` and `/ inv` genuinely differ. The box is
    // placed so the correct exit parameters keep tmin < tmax (hit), but the
    // mutated (shrunken) t2 of the Y slab drops below tmin -> tmin > tmax (miss).
    #[test]
    fn intersect_aabb_exit_param_uses_multiply_not_divide() {
        // dir (1,1,0) normalized -> d = 1/sqrt2 on x,y; inv = sqrt2.
        let aabb = Aabb::new(Vec3::new(3.0, 1.0, -1.0), Vec3::new(5.0, 4.0, 1.0)).unwrap();
        let r = ray(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0));
        // Correct: x-slab t in [3*sqrt2, 5*sqrt2], y-slab t in [1*sqrt2, 4*sqrt2];
        // overlap [3*sqrt2, 4*sqrt2] -> hit. With `/ inv` the y exit collapses
        // below the x entry, forcing tmin > tmax -> the mutant misses.
        assert!(r.intersect_aabb(&aabb));
    }

    // Kills 92:25 (`tmin > tmax` -> `>=`). A ray tangent to a corner so that
    // tmin == tmax EXACTLY (the entry of one slab equals the exit of another,
    // computed by mirror-symmetric arithmetic). `>` keeps the touching hit;
    // `>=` rejects it. The equality is bit-exact: 2*sqrt2 vs (-2)*(-sqrt2).
    #[test]
    fn intersect_aabb_corner_tangent_tmin_equals_tmax_is_hit() {
        // dir (1,-1,0) normalized; box x[2,4], y[0,2]; origin (0,2,1).
        let aabb = Aabb::new(Vec3::new(2.0, 0.0, -1.0), Vec3::new(4.0, 2.0, 3.0)).unwrap();
        let r = ray(Vec3::new(0.0, 2.0, 1.0), Vec3::new(1.0, -1.0, 0.0));
        // x entry = 2*sqrt2; y exit (after swap) = (0 - 2) * (-sqrt2) = 2*sqrt2.
        // tmin == tmax -> touching corner -> hit under the correct `>`.
        assert!(r.intersect_aabb(&aabb));
    }

    // Exact interior hit with computable parameters to pin the slab arithmetic
    // (80 `1.0/d`, 81/82 `(lo-o)*inv` and `(hi-o)*inv`) and the tmin/tmax update
    // comparisons 86/89. Axis-aligned +X ray: only the X slab constrains t.
    #[test]
    fn intersect_aabb_axis_ray_exact_entry_exit() {
        // Box x in [3,7], y,z in [-1,1]. Ray from origin along +X.
        let aabb = Aabb::new(Vec3::new(3.0, -1.0, -1.0), Vec3::new(7.0, 1.0, 1.0)).unwrap();
        let r = ray(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        // Enters at t=3, exits at t=7, both positive -> hit.
        assert!(r.intersect_aabb(&aabb));
        // Same box, ray starting past it heading further +X -> tmax < 0 region
        // is not it; instead start beyond x=7 heading +X: never re-enters.
        let r2 = ray(Vec3::new(8.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0));
        assert!(!r2.intersect_aabb(&aabb));
    }
}
