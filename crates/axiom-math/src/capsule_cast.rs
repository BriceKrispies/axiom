//! Casting a moving point at a capsule — the quadratic every swept test in
//! this layer is ultimately built from.

use crate::capsule::Capsule;
use crate::hit::Hit;
use crate::ray::Ray;
use crate::vec3::Vec3;

/// The smallest denominator these quadratics will divide by; the validity flag
/// beside each division decides the answer, so a guarded quotient is never
/// consulted, only kept finite.
const SAFE_DENOMINATOR: f32 = f32::MIN_POSITIVE;

/// The smallest `t` in `[0, t_max]` at which the point `origin + t * motion`
/// enters `capsule`, or `f32::INFINITY` when the path never enters it.
///
/// `motion` is in the caller's units — a unit ray direction with an unbounded
/// `t_max`, or a whole frame's displacement with `t_max = 1` — and `t` comes
/// back in those same units.
///
/// **Entry, not overlap.** A point that starts *inside* the capsule has a
/// negative entry parameter and is reported as `INFINITY`, because the earliest
/// crossing on its path is behind it. Callers that must react to an initial
/// overlap test for it directly; that keeps this solve to one meaning.
///
/// The three surfaces a capsule is made of are solved independently and merged
/// by `min`: the infinite cylinder about the axis (clipped to the axis span),
/// and the two cap spheres, each of which lies wholly inside the capsule and so
/// can never claim an entry the capsule does not have.
pub(crate) fn entry_param(capsule: &Capsule, origin: Vec3, motion: Vec3, t_max: f32) -> f32 {
    let axis = capsule.segment();
    let along = axis.delta();
    let to_start = origin.subtract(axis.start());
    let radius = capsule.radius();
    let axis_length_squared = along.length_squared();
    let speed_squared = motion.length_squared();
    let start_along = to_start.dot(along);
    let motion_along = motion.dot(along);
    let a = axis_length_squared * speed_squared - motion_along * motion_along;
    let b = axis_length_squared * to_start.dot(motion) - start_along * motion_along;
    let c = axis_length_squared * (to_start.length_squared() - radius * radius)
        - start_along * start_along;
    let discriminant = b * b - a * c;
    let t = (-b - discriminant.max(0.0).sqrt()) / a.max(SAFE_DENOMINATOR);
    let axial = start_along + t * motion_along;
    let on_shaft = (a > 0.0)
        & (discriminant >= 0.0)
        & (t >= 0.0)
        & (t <= t_max)
        & (axial >= 0.0)
        & (axial <= axis_length_squared);
    [f32::INFINITY, t][usize::from(on_shaft)]
        .min(cap_param(to_start, motion, speed_squared, radius, t_max))
        .min(cap_param(
            origin.subtract(axis.end()),
            motion,
            speed_squared,
            radius,
            t_max,
        ))
}

/// The smallest `t` in `[0, t_max]` at which `to_center + t * motion` enters the
/// sphere of `radius` about the origin of that offset, or `f32::INFINITY`.
/// A motionless point never enters anything, which is what kills the division.
fn cap_param(to_center: Vec3, motion: Vec3, speed_squared: f32, radius: f32, t_max: f32) -> f32 {
    let b = to_center.dot(motion);
    let c = to_center.length_squared() - radius * radius;
    let discriminant = b * b - speed_squared * c;
    let t = (-b - discriminant.max(0.0).sqrt()) / speed_squared.max(SAFE_DENOMINATOR);
    let entered = (speed_squared > 0.0) & (discriminant >= 0.0) & (t >= 0.0) & (t <= t_max);
    [f32::INFINITY, t][usize::from(entered)]
}

/// The contact record for a point `probe` that has just reached the capsule's
/// surface: the normal is the radial direction from the axis out to `probe`,
/// and the contact point is that direction stepped one radius off the axis.
///
/// `fallback` is the normal for a `probe` sitting exactly *on* the axis, where
/// the radial direction is undefined — reachable only when a cast starts inside
/// the capsule, and answered with the direction the caster came from.
fn surface_hit(capsule: &Capsule, t: f32, probe: Vec3, fallback: Vec3) -> Hit {
    let axis_point = capsule.segment().closest_point_to(probe);
    let normal = probe.subtract(axis_point).normalize().unwrap_or(fallback);
    Hit::new(
        t,
        axis_point.add(normal.mul_scalar(capsule.radius())),
        normal,
    )
}

impl Capsule {
    /// Where `ray` first enters this capsule, with [`Hit::time`] a distance
    /// along the ray.
    ///
    /// A ray whose origin is already inside reports an immediate hit at
    /// distance `0` — the same convention as
    /// [`Ray::intersect_aabb_entry`](crate::Ray::intersect_aabb_entry) — with
    /// the contact taken radially out from the axis through the origin.
    pub fn raycast(&self, ray: &Ray) -> Option<Hit> {
        let entry = entry_param(self, ray.origin(), ray.direction(), f32::INFINITY);
        let inside = self.contains_point(ray.origin());
        let t = [entry, 0.0][usize::from(inside)];
        t.is_finite()
            .then(|| surface_hit(self, t, ray.point_at(t), ray.direction().mul_scalar(-1.0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approx_eq::ApproxEq;
    use crate::epsilon::Epsilon;
    use crate::segment::Segment;

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    /// A unit-radius capsule whose axis runs from `y = 1` to `y = 3` on the
    /// vertical: it occupies `y` in `[0, 4]` and `x`, `z` in `[-1, 1]`.
    fn body() -> Capsule {
        Capsule::new(
            Segment::new(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 3.0, 0.0)).unwrap(),
            1.0,
        )
        .unwrap()
    }

    fn ray(origin: Vec3, direction: Vec3) -> Ray {
        Ray::new(origin, direction).unwrap()
    }

    #[test]
    fn ray_into_the_shaft_hits_the_cylinder_side() {
        let hit = body()
            .raycast(&ray(Vec3::new(-5.0, 2.0, 0.0), Vec3::UNIT_X))
            .unwrap();
        assert!(hit.time().approx_eq(&4.0, eps()));
        assert!(hit.point().approx_eq(&Vec3::new(-1.0, 2.0, 0.0), eps()));
        assert!(hit.normal().approx_eq(&Vec3::new(-1.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn ray_down_the_axis_hits_the_top_cap() {
        let hit = body()
            .raycast(&ray(Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, -1.0, 0.0)))
            .unwrap();
        assert!(hit.time().approx_eq(&6.0, eps()));
        assert!(hit.point().approx_eq(&Vec3::new(0.0, 4.0, 0.0), eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_Y, eps()));
    }

    #[test]
    fn ray_up_the_axis_hits_the_bottom_cap() {
        let hit = body()
            .raycast(&ray(Vec3::new(0.0, -10.0, 0.0), Vec3::UNIT_Y))
            .unwrap();
        assert!(hit.time().approx_eq(&10.0, eps()));
        assert!(hit.point().approx_eq(&Vec3::ZERO, eps()));
        assert!(hit.normal().approx_eq(&Vec3::new(0.0, -1.0, 0.0), eps()));
    }

    #[test]
    fn ray_level_with_a_cap_hits_the_sphere_not_the_shaft() {
        // y = 3.5 is past the end of the axis, so only the cap can be hit; the
        // chord is half a radius up the sphere.
        let hit = body()
            .raycast(&ray(Vec3::new(-5.0, 3.5, 0.0), Vec3::UNIT_X))
            .unwrap();
        let expected_x = -0.75_f32.sqrt();
        assert!(hit.time().approx_eq(&(5.0 + expected_x), eps()));
        assert!(hit
            .point()
            .approx_eq(&Vec3::new(expected_x, 3.5, 0.0), eps()));
        assert!(hit.normal().length().approx_eq(&1.0, eps()));
    }

    #[test]
    fn ray_beside_the_capsule_misses() {
        assert!(body()
            .raycast(&ray(Vec3::new(-5.0, 2.0, 2.0), Vec3::UNIT_X))
            .is_none());
    }

    #[test]
    fn ray_parallel_to_the_axis_and_outside_the_radius_misses() {
        assert!(body()
            .raycast(&ray(Vec3::new(2.0, -10.0, 0.0), Vec3::UNIT_Y))
            .is_none());
    }

    #[test]
    fn ray_aimed_away_from_the_capsule_misses() {
        assert!(body()
            .raycast(&ray(Vec3::new(-5.0, 2.0, 0.0), Vec3::new(-1.0, 0.0, 0.0)))
            .is_none());
    }

    #[test]
    fn ray_beyond_the_caps_along_the_axis_misses() {
        assert!(body()
            .raycast(&ray(Vec3::new(0.0, 10.0, 0.0), Vec3::UNIT_Y))
            .is_none());
    }

    #[test]
    fn ray_starting_inside_hits_immediately_and_reports_the_radial_surface() {
        let hit = body()
            .raycast(&ray(Vec3::new(0.5, 2.0, 0.0), Vec3::UNIT_X))
            .unwrap();
        assert_eq!(hit.time(), 0.0);
        assert!(hit.point().approx_eq(&Vec3::new(1.0, 2.0, 0.0), eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_X, eps()));
    }

    #[test]
    fn ray_starting_on_the_axis_falls_back_to_facing_the_caster() {
        let hit = body()
            .raycast(&ray(Vec3::new(0.0, 2.0, 0.0), Vec3::UNIT_X))
            .unwrap();
        assert_eq!(hit.time(), 0.0);
        assert!(hit.normal().approx_eq(&Vec3::new(-1.0, 0.0, 0.0), eps()));
        assert!(hit.point().approx_eq(&Vec3::new(-1.0, 2.0, 0.0), eps()));
    }

    #[test]
    fn a_degenerate_capsule_casts_as_a_sphere() {
        let ball = Capsule::new(
            Segment::new(Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.0, 2.0, 0.0)).unwrap(),
            1.0,
        )
        .unwrap();
        let hit = ball
            .raycast(&ray(Vec3::new(-5.0, 2.0, 0.0), Vec3::UNIT_X))
            .unwrap();
        assert!(hit.time().approx_eq(&4.0, eps()));
        assert!(hit.normal().approx_eq(&Vec3::new(-1.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn entry_param_reports_infinity_for_a_motionless_point() {
        assert_eq!(
            entry_param(&body(), Vec3::new(-5.0, 2.0, 0.0), Vec3::ZERO, 1.0),
            f32::INFINITY
        );
    }

    #[test]
    fn entry_param_honours_its_upper_bound() {
        let capsule = body();
        let origin = Vec3::new(-5.0, 2.0, 0.0);
        // The full step reaches the surface at t = 0.8 of an five-unit motion.
        assert!(entry_param(&capsule, origin, Vec3::new(5.0, 0.0, 0.0), 1.0).approx_eq(&0.8, eps()));
        // Half that motion stops short of it.
        assert_eq!(
            entry_param(&capsule, origin, Vec3::new(2.0, 0.0, 0.0), 1.0),
            f32::INFINITY
        );
        // A cap approach, bounded the same way.
        assert_eq!(
            entry_param(
                &capsule,
                Vec3::new(0.0, 10.0, 0.0),
                Vec3::new(0.0, -1.0, 0.0),
                1.0
            ),
            f32::INFINITY
        );
    }

    #[test]
    fn entry_param_reports_infinity_from_strictly_inside() {
        // Strictly inside every one of the three surfaces: both cap spheres and
        // the shaft put their entry behind the point, so none of them claims it.
        let fat = Capsule::new(
            Segment::new(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 3.0, 0.0)).unwrap(),
            1.5,
        )
        .unwrap();
        assert_eq!(
            entry_param(&fat, Vec3::new(0.0, 2.0, 0.0), Vec3::UNIT_X, f32::INFINITY),
            f32::INFINITY
        );
    }
}
