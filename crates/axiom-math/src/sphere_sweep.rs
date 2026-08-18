//! Sweeping a sphere along a motion vector at a triangle.

use crate::capsule::Capsule;
use crate::capsule_cast::entry_param;
use crate::hit::Hit;
use crate::sphere::Sphere;
use crate::triangle::Triangle;
use crate::vec3::Vec3;

/// The fraction of `motion` at which a sphere of `radius` centred at `center`
/// first touches `triangle`, or `f32::INFINITY` when it never does.
///
/// The set of centre positions that touch a triangle is the triangle grown by
/// `radius`: two offset copies of the face, and a capsule of that radius around
/// each of the three edges. The centre travels a straight line through that
/// set, so the whole sweep is a face solve merged with three point-versus-
/// capsule entries — the same solve a raycast uses.
///
/// A sphere that *starts* touching has no entry on its path and is reported as
/// `INFINITY`; callers detect that state directly, exactly as
/// [`entry_param`] documents.
pub(crate) fn sweep_param(center: Vec3, radius: f32, motion: Vec3, triangle: &Triangle) -> f32 {
    let boundary = triangle
        .edges()
        .into_iter()
        .fold(f32::INFINITY, |best, edge| {
            best.min(entry_param(
                &Capsule::from_parts(edge, radius),
                center,
                motion,
                1.0,
            ))
        });
    face_param(center, radius, motion, triangle).min(boundary)
}

/// The fraction of `motion` at which the sphere lands flat on the triangle's
/// face, or `f32::INFINITY` when it lands beside the face or not at all.
///
/// The sphere must start clear of the plane (`|height| >= radius`) for the
/// landing to be an entry; a sphere already straddling the plane is either
/// touching the triangle already or approaching one of its edges, and both are
/// answered elsewhere. A motion parallel to the plane, and a triangle so
/// degenerate it has no plane, drive the division to an infinity or a `NaN`,
/// which every comparison here then rejects.
fn face_param(center: Vec3, radius: f32, motion: Vec3, triangle: &Triangle) -> f32 {
    let normal = triangle.normal().unwrap_or(Vec3::ZERO);
    let height = normal.dot(center.subtract(triangle.a()));
    let side = height.signum();
    let t = (side * radius - height) / normal.dot(motion);
    let contact = center
        .add(motion.mul_scalar(t))
        .subtract(normal.mul_scalar(side * radius));
    let landed =
        (height.abs() >= radius) & (t >= 0.0) & (t <= 1.0) & triangle.contains_projection(contact);
    [f32::INFINITY, t][usize::from(landed)]
}

/// The contact record for a sphere that has reached `triangle` after `t` of its
/// motion: the closest point of the triangle to the swept centre is the contact,
/// and the direction from it out to that centre is the normal.
///
/// The fallback normal is the triangle's own, for a centre that ends up exactly
/// *on* the surface — a zero-radius sphere, or a sweep that started touching
/// with its centre in the face.
fn contact(center: Vec3, motion: Vec3, triangle: &Triangle, t: f32) -> Hit {
    let arrived = center.add(motion.mul_scalar(t));
    let surface = triangle.closest_point_to(arrived);
    let normal = arrived
        .subtract(surface)
        .normalize()
        .unwrap_or(triangle.normal().unwrap_or(Vec3::ZERO));
    Hit::new(t, surface, normal)
}

impl Sphere {
    /// Where this sphere first touches `triangle` while travelling `motion`,
    /// with [`Hit::time`] the fraction of `motion` travelled.
    ///
    /// A sphere that already overlaps the triangle reports an immediate hit at
    /// time `0`, so a caller which has been pushed into a wall still gets the
    /// normal it needs to get out. A degenerate triangle has no face, but its
    /// collapsed edges are still real geometry and a sweep can still touch
    /// them — the same reading [`Triangle::closest_point_to`] takes.
    pub fn sweep_triangle(&self, motion: Vec3, triangle: &Triangle) -> Option<Hit> {
        let radius = self.radius();
        let center = self.center();
        let reach = triangle.closest_point_to(center).subtract(center);
        let overlapped = reach.length_squared() <= radius * radius;
        let swept = sweep_param(center, radius, motion, triangle);
        let t = [swept, 0.0][usize::from(overlapped)];
        t.is_finite().then(|| contact(center, motion, triangle, t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approx_eq::ApproxEq;
    use crate::epsilon::Epsilon;

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    /// The right triangle `(0,0,0) (0,0,4) (4,0,0)` in the y = 0 plane, wound so
    /// its normal is +Y.
    fn floor() -> Triangle {
        Triangle::new(
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, 4.0),
            Vec3::new(4.0, 0.0, 0.0),
        )
        .unwrap()
    }

    fn sphere(center: Vec3, radius: f32) -> Sphere {
        Sphere::new(center, radius).unwrap()
    }

    #[test]
    fn falling_sphere_lands_on_the_face() {
        let hit = sphere(Vec3::new(1.0, 5.0, 1.0), 1.0)
            .sweep_triangle(Vec3::new(0.0, -10.0, 0.0), &floor())
            .unwrap();
        assert!(hit.time().approx_eq(&0.4, eps()));
        assert!(hit.point().approx_eq(&Vec3::new(1.0, 0.0, 1.0), eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_Y, eps()));
    }

    #[test]
    fn sphere_rising_from_below_lands_on_the_other_face() {
        let hit = sphere(Vec3::new(1.0, -5.0, 1.0), 1.0)
            .sweep_triangle(Vec3::new(0.0, 10.0, 0.0), &floor())
            .unwrap();
        assert!(hit.time().approx_eq(&0.4, eps()));
        assert!(hit.normal().approx_eq(&Vec3::new(0.0, -1.0, 0.0), eps()));
    }

    #[test]
    fn sphere_falling_beside_the_triangle_misses() {
        assert!(sphere(Vec3::new(5.0, 5.0, 5.0), 1.0)
            .sweep_triangle(Vec3::new(0.0, -10.0, 0.0), &floor())
            .is_none());
    }

    #[test]
    fn sphere_stopping_short_of_the_face_misses() {
        assert!(sphere(Vec3::new(1.0, 5.0, 1.0), 1.0)
            .sweep_triangle(Vec3::new(0.0, -2.0, 0.0), &floor())
            .is_none());
    }

    #[test]
    fn sphere_moving_away_from_the_face_misses() {
        assert!(sphere(Vec3::new(1.0, 5.0, 1.0), 1.0)
            .sweep_triangle(Vec3::new(0.0, 10.0, 0.0), &floor())
            .is_none());
    }

    #[test]
    fn sphere_sliding_parallel_above_the_face_misses() {
        assert!(sphere(Vec3::new(1.0, 5.0, 1.0), 1.0)
            .sweep_triangle(Vec3::new(2.0, 0.0, 0.0), &floor())
            .is_none());
    }

    #[test]
    fn sphere_that_starts_overlapping_hits_at_time_zero() {
        let hit = sphere(Vec3::new(1.0, 0.5, 1.0), 1.0)
            .sweep_triangle(Vec3::new(0.0, -10.0, 0.0), &floor())
            .unwrap();
        assert_eq!(hit.time(), 0.0);
        assert!(hit.point().approx_eq(&Vec3::new(1.0, 0.0, 1.0), eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_Y, eps()));
    }

    #[test]
    fn a_motionless_sphere_hits_only_what_it_already_touches() {
        let touching = sphere(Vec3::new(1.0, 0.5, 1.0), 1.0)
            .sweep_triangle(Vec3::ZERO, &floor())
            .unwrap();
        assert_eq!(touching.time(), 0.0);
        assert!(sphere(Vec3::new(1.0, 5.0, 1.0), 1.0)
            .sweep_triangle(Vec3::ZERO, &floor())
            .is_none());
    }

    #[test]
    fn sphere_swept_into_an_edge_from_the_side_hits_that_edge() {
        // Travelling in the plane of the triangle, so the face solve is inert
        // and the edge capsule is the only thing that can stop it.
        let hit = sphere(Vec3::new(-3.0, 0.0, 2.0), 1.0)
            .sweep_triangle(Vec3::new(4.0, 0.0, 0.0), &floor())
            .unwrap();
        assert!(hit.time().approx_eq(&0.5, eps()));
        assert!(hit.point().approx_eq(&Vec3::new(0.0, 0.0, 2.0), eps()));
        assert!(hit.normal().approx_eq(&Vec3::new(-1.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn sphere_swept_into_a_vertex_hits_that_vertex() {
        let hit = sphere(Vec3::new(0.0, 0.0, -3.0), 1.0)
            .sweep_triangle(Vec3::new(0.0, 0.0, 3.0), &floor())
            .unwrap();
        assert!(hit.time().approx_eq(&(2.0 / 3.0), eps()));
        assert!(hit.point().approx_eq(&Vec3::ZERO, eps()));
        assert!(hit.normal().approx_eq(&Vec3::new(0.0, 0.0, -1.0), eps()));
    }

    #[test]
    fn sphere_landing_just_past_the_edge_is_caught_by_the_edge_not_the_face() {
        // The face solve puts the contact outside the triangle, so it is
        // rejected; the sphere still clips the hypotenuse on its way down.
        let hit = sphere(Vec3::new(2.2, 5.0, 2.2), 1.0)
            .sweep_triangle(Vec3::new(0.0, -10.0, 0.0), &floor())
            .unwrap();
        assert!(hit.time() > 0.4);
        assert!(hit.point().approx_eq(&Vec3::new(2.0, 0.0, 2.0), eps()));
        assert!(hit.normal().y > 0.0);
    }

    #[test]
    fn a_zero_radius_sphere_crossing_the_face_reports_the_plane_normal() {
        let hit = sphere(Vec3::new(1.0, 5.0, 1.0), 0.0)
            .sweep_triangle(Vec3::new(0.0, -10.0, 0.0), &floor())
            .unwrap();
        assert!(hit.time().approx_eq(&0.5, eps()));
        assert!(hit.point().approx_eq(&Vec3::new(1.0, 0.0, 1.0), eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_Y, eps()));
    }

    #[test]
    fn a_degenerate_triangle_has_no_face_but_keeps_its_collapsed_edges() {
        let collinear = Triangle::new(
            Vec3::ZERO,
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
        )
        .unwrap();
        // Straight down onto the collapsed line: the face solve cannot answer,
        // the edge capsules can.
        let hit = sphere(Vec3::new(1.0, 5.0, 0.0), 1.0)
            .sweep_triangle(Vec3::new(0.0, -10.0, 0.0), &collinear)
            .unwrap();
        assert!(hit.time().approx_eq(&0.4, eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_Y, eps()));
        // Beside the collapsed line, nothing is there to hit.
        assert!(sphere(Vec3::new(9.0, 5.0, 0.0), 1.0)
            .sweep_triangle(Vec3::new(0.0, -10.0, 0.0), &collinear)
            .is_none());
    }
}
