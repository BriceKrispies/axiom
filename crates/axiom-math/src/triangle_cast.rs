//! Casting a ray or a segment at a triangle (Möller–Trumbore).

use crate::hit::Hit;
use crate::ray::Ray;
use crate::segment::Segment;
use crate::triangle::Triangle;
use crate::vec3::Vec3;

/// Where `origin + t * direction` crosses `triangle`, for the smallest
/// `t` in `[0, t_max]`, or `None` when the path misses it.
///
/// `direction` need not be a unit vector: a ray passes its unit direction and an
/// unbounded `t_max`, a segment passes its full `end - start` delta and
/// `t_max = 1`, and in both cases `t` comes back in the caller's own units.
///
/// The determinant is the triangle's projected area along `direction`, so it is
/// exactly zero when the path runs parallel to the plane or the triangle is
/// degenerate. Only that exact zero is guarded: a merely *small* determinant
/// pushes the barycentric coordinates far outside `[0, 1]` on its own, and the
/// containment test then rejects the cast without any tolerance to tune. When
/// the guard does fire, the substituted divisor keeps the arithmetic finite and
/// the miss is decided by the flag, not by the numbers.
pub(crate) fn cast(triangle: &Triangle, origin: Vec3, direction: Vec3, t_max: f32) -> Option<Hit> {
    let ab = triangle.edge_ab();
    let ac = triangle.edge_ac();
    let across = direction.cross(ac);
    let determinant = ab.dot(across);
    let degenerate = determinant == 0.0;
    let divisor = [determinant, 1.0][usize::from(degenerate)];
    let to_origin = origin.subtract(triangle.a());
    let along = to_origin.cross(ab);
    let v = to_origin.dot(across) / divisor;
    let w = direction.dot(along) / divisor;
    let t = ac.dot(along) / divisor;
    let inside = (v >= 0.0) & (w >= 0.0) & (v + w <= 1.0);
    let hit = !degenerate & inside & (t >= 0.0) & (t <= t_max);
    // `ab x ac` faces the winding side; `signum` of the determinant flips it to
    // the side the cast arrives from, so the reported normal always faces the
    // mover. The zero-length fallback is unreachable through this `then`: a
    // triangle with no normal has a zero determinant and never reports a hit.
    let facing = ab
        .cross(ac)
        .normalize()
        .unwrap_or(Vec3::ZERO)
        .mul_scalar(determinant.signum());
    hit.then(|| Hit::new(t, origin.add(direction.mul_scalar(t)), facing))
}

impl Triangle {
    /// The first crossing of this triangle by `ray`, with
    /// [`Hit::time`] a distance along the ray.
    ///
    /// The triangle is two-sided: a ray arriving from behind the winding normal
    /// hits it just the same, and [`Hit::normal`] is flipped to face the ray.
    pub fn raycast(&self, ray: &Ray) -> Option<Hit> {
        cast(self, ray.origin(), ray.direction(), f32::INFINITY)
    }

    /// Where `segment` crosses this triangle, with [`Hit::time`] the fraction
    /// of the segment travelled. Two-sided, exactly as [`Self::raycast`].
    pub fn intersect_segment(&self, segment: &Segment) -> Option<Hit> {
        cast(self, segment.start(), segment.delta(), 1.0)
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

    /// `(0,0,0) (0,0,4) (4,0,0)`: the y = 0 floor, wound so its normal is +Y.
    fn floor_triangle() -> Triangle {
        Triangle::new(
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, 4.0),
            Vec3::new(4.0, 0.0, 0.0),
        )
        .unwrap()
    }

    fn ray(origin: Vec3, direction: Vec3) -> Ray {
        Ray::new(origin, direction).unwrap()
    }

    #[test]
    fn ray_through_the_face_reports_distance_point_and_normal() {
        let hit = floor_triangle()
            .raycast(&ray(Vec3::new(1.0, 3.0, 1.0), Vec3::new(0.0, -1.0, 0.0)))
            .unwrap();
        assert!(hit.time().approx_eq(&3.0, eps()));
        assert!(hit.point().approx_eq(&Vec3::new(1.0, 0.0, 1.0), eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_Y, eps()));
    }

    #[test]
    fn ray_from_behind_hits_and_flips_the_normal() {
        let hit = floor_triangle()
            .raycast(&ray(Vec3::new(1.0, -3.0, 1.0), Vec3::UNIT_Y))
            .unwrap();
        assert!(hit.time().approx_eq(&3.0, eps()));
        assert!(hit.normal().approx_eq(&Vec3::new(0.0, -1.0, 0.0), eps()));
    }

    #[test]
    fn ray_past_the_edge_misses() {
        let tri = floor_triangle();
        assert!(tri
            .raycast(&ray(Vec3::new(3.0, 3.0, 3.0), Vec3::new(0.0, -1.0, 0.0)))
            .is_none());
        assert!(tri
            .raycast(&ray(Vec3::new(-1.0, 3.0, 1.0), Vec3::new(0.0, -1.0, 0.0)))
            .is_none());
        assert!(tri
            .raycast(&ray(Vec3::new(1.0, 3.0, -1.0), Vec3::new(0.0, -1.0, 0.0)))
            .is_none());
    }

    #[test]
    fn ray_aimed_away_misses() {
        assert!(floor_triangle()
            .raycast(&ray(Vec3::new(1.0, 3.0, 1.0), Vec3::UNIT_Y))
            .is_none());
    }

    #[test]
    fn ray_parallel_to_the_plane_misses() {
        assert!(floor_triangle()
            .raycast(&ray(Vec3::new(-5.0, 0.0, 1.0), Vec3::UNIT_X))
            .is_none());
    }

    #[test]
    fn degenerate_triangle_is_never_hit() {
        let collinear = Triangle::new(
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
        )
        .unwrap();
        assert!(collinear
            .raycast(&ray(Vec3::new(0.5, 3.0, 0.0), Vec3::new(0.0, -1.0, 0.0)))
            .is_none());
    }

    #[test]
    fn ray_exactly_through_a_vertex_and_an_edge_hits() {
        let tri = floor_triangle();
        let down = Vec3::new(0.0, -1.0, 0.0);
        assert!(tri.raycast(&ray(Vec3::new(0.0, 2.0, 0.0), down)).is_some());
        assert!(tri.raycast(&ray(Vec3::new(2.0, 2.0, 2.0), down)).is_some());
        assert!(tri.raycast(&ray(Vec3::new(2.0, 2.0, 0.0), down)).is_some());
    }

    #[test]
    fn segment_crossing_the_face_reports_a_fraction_of_itself() {
        let segment = Segment::new(Vec3::new(1.0, 2.0, 1.0), Vec3::new(1.0, -2.0, 1.0)).unwrap();
        let hit = floor_triangle().intersect_segment(&segment).unwrap();
        assert!(hit.time().approx_eq(&0.5, eps()));
        assert!(hit.point().approx_eq(&Vec3::new(1.0, 0.0, 1.0), eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_Y, eps()));
    }

    #[test]
    fn segment_stopping_short_of_the_face_misses() {
        let segment = Segment::new(Vec3::new(1.0, 2.0, 1.0), Vec3::new(1.0, 1.0, 1.0)).unwrap();
        assert!(floor_triangle().intersect_segment(&segment).is_none());
    }

    #[test]
    fn segment_starting_beyond_the_face_misses() {
        let segment = Segment::new(Vec3::new(1.0, -1.0, 1.0), Vec3::new(1.0, -5.0, 1.0)).unwrap();
        assert!(floor_triangle().intersect_segment(&segment).is_none());
    }

    #[test]
    fn segment_touching_the_face_with_its_end_hits_at_one() {
        let segment = Segment::new(Vec3::new(1.0, 2.0, 1.0), Vec3::new(1.0, 0.0, 1.0)).unwrap();
        let hit = floor_triangle().intersect_segment(&segment).unwrap();
        assert!(hit.time().approx_eq(&1.0, eps()));
    }

    #[test]
    fn degenerate_segment_on_the_face_does_not_cross_it() {
        let point = Segment::new(Vec3::new(1.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 1.0)).unwrap();
        assert!(floor_triangle().intersect_segment(&point).is_none());
    }
}
