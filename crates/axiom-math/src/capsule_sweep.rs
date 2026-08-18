//! Sweeping a capsule along a motion vector at a triangle or another capsule —
//! the query a character controller asks once per step.

use crate::capsule::Capsule;
use crate::capsule_cast::entry_param;
use crate::hit::Hit;
use crate::segment::Segment;
use crate::sphere_sweep::sweep_param;
use crate::triangle::Triangle;
use crate::vec3::Vec3;

/// The fraction of `motion` at which the moving segment `axis` and the static
/// segment `other` first come within `reach` of one another **in each other's
/// interiors**, or `f32::INFINITY`.
///
/// This is the one contact two segments can have that neither one's endpoints
/// witness: two skew shafts crossing. It is also the easiest to solve, because
/// the distance between two skew *lines* is measured along their common
/// perpendicular, which the motion changes at a constant rate — one linear
/// equation, not a quadratic.
///
/// The interior test is what keeps that shortcut honest: it confirms that at the
/// solved time the closest pair really is inside both segments, and so is the
/// distance the line solve measured. Parallel shafts have no common
/// perpendicular; their separation vector collapses to zero, the solve yields a
/// non-finite time, and the interior test rejects it — correctly, because two
/// parallel capsules always touch at an endpoint, which the endpoint casts
/// already own.
fn crossing_param(axis: &Segment, reach: f32, motion: Vec3, other: &Segment) -> f32 {
    let separation = axis
        .delta()
        .cross(other.delta())
        .normalize()
        .unwrap_or(Vec3::ZERO);
    let gap = axis.start().subtract(other.start()).dot(separation);
    let t = (gap.signum() * reach - gap) / motion.dot(separation);
    let (mine, theirs) = axis
        .translated(motion.mul_scalar(t))
        .closest_params_to_segment(other);
    let interior = (mine > 0.0) & (mine < 1.0) & (theirs > 0.0) & (theirs < 1.0);
    let crossed = interior & (t >= 0.0) & (t <= 1.0);
    [f32::INFINITY, t][usize::from(crossed)]
}

/// The contact record for a capsule that has reached `triangle` after `t` of its
/// motion. The closest pair between the arrived axis and the triangle names both
/// the contact point (on the triangle) and the normal (pointing back at the
/// axis); the triangle's own normal stands in when the axis has been driven
/// exactly onto the surface.
fn triangle_contact(capsule: &Capsule, motion: Vec3, triangle: &Triangle, t: f32) -> Hit {
    let arrived = capsule.segment().translated(motion.mul_scalar(t));
    let (mine, theirs) = arrived.closest_points_to_triangle(triangle);
    let normal = mine
        .subtract(theirs)
        .normalize()
        .unwrap_or(triangle.normal().unwrap_or(Vec3::ZERO));
    Hit::new(t, theirs, normal)
}

/// The contact record for a capsule that has reached `other` after `t` of its
/// motion. The closest pair between the two axes gives the normal, and stepping
/// one of `other`'s radii along it gives the point on `other`'s surface. Two
/// axes lying exactly on top of one another have no separating direction, so the
/// mover is pushed back the way it came.
fn capsule_contact(capsule: &Capsule, motion: Vec3, other: &Capsule, t: f32) -> Hit {
    let arrived = capsule.segment().translated(motion.mul_scalar(t));
    let (mine, theirs) = arrived.closest_points_to_segment(&other.segment());
    let normal = mine.subtract(theirs).normalize().unwrap_or(
        motion
            .normalize()
            .map(|forward| forward.mul_scalar(-1.0))
            .unwrap_or(Vec3::ZERO),
    );
    Hit::new(t, theirs.add(normal.mul_scalar(other.radius())), normal)
}

impl Capsule {
    /// Where this capsule first touches `triangle` while travelling `motion`,
    /// with [`Hit::time`] the fraction of `motion` travelled.
    ///
    /// Three families of contact can be first, and all three are solved:
    /// * a **cap** reaching the triangle — each end of the axis swept as a
    ///   sphere, which covers the face, the edges and the vertices under it;
    /// * a **vertex** reaching the shaft — each vertex cast at the capsule from
    ///   the capsule's own frame, where the vertex is what moves;
    /// * a **shaft** crossing an **edge** — the skew-line solve above, which is
    ///   the case no endpoint ever witnesses (a long capsule laid across a
    ///   triangle touches its edges before either cap touches anything).
    ///
    /// A capsule that already overlaps reports an immediate hit at time `0`.
    pub fn sweep_triangle(&self, motion: Vec3, triangle: &Triangle) -> Option<Hit> {
        let axis = self.segment();
        let caps = sweep_param(axis.start(), self.radius(), motion, triangle).min(sweep_param(
            axis.end(),
            self.radius(),
            motion,
            triangle,
        ));
        let reversed = motion.mul_scalar(-1.0);
        let vertices = [triangle.a(), triangle.b(), triangle.c()]
            .into_iter()
            .fold(f32::INFINITY, |best, vertex| {
                best.min(entry_param(self, vertex, reversed, 1.0))
            });
        let edges = triangle
            .edges()
            .into_iter()
            .fold(f32::INFINITY, |best, edge| {
                best.min(crossing_param(&axis, self.radius(), motion, &edge))
            });
        let swept = caps.min(vertices).min(edges);
        let t = [swept, 0.0][usize::from(self.overlaps_triangle(triangle))];
        t.is_finite()
            .then(|| triangle_contact(self, motion, triangle, t))
    }

    /// Where this capsule first touches the stationary `other` while travelling
    /// `motion`, with [`Hit::time`] the fraction of `motion` travelled.
    ///
    /// Two capsules touch when their axes come within the sum of their radii, so
    /// every candidate below is that one distance solved against a different
    /// pair of features: each of this capsule's endpoints cast at `other` grown
    /// by both radii, each of `other`'s endpoints cast at *this* capsule grown
    /// the same way (in this capsule's frame, where they are what moves), and
    /// the two shafts crossing.
    ///
    /// A capsule that already overlaps reports an immediate hit at time `0`.
    pub fn sweep_capsule(&self, motion: Vec3, other: &Capsule) -> Option<Hit> {
        let reach = self.radius() + other.radius();
        let mine = self.segment();
        let theirs = other.segment();
        let grown_other = Capsule::from_parts(theirs, reach);
        let grown_self = Capsule::from_parts(mine, reach);
        let reversed = motion.mul_scalar(-1.0);
        let ends = entry_param(&grown_other, mine.start(), motion, 1.0)
            .min(entry_param(&grown_other, mine.end(), motion, 1.0))
            .min(entry_param(&grown_self, theirs.start(), reversed, 1.0))
            .min(entry_param(&grown_self, theirs.end(), reversed, 1.0));
        let swept = ends.min(crossing_param(&mine, reach, motion, &theirs));
        let t = [swept, 0.0][usize::from(self.overlaps(other))];
        t.is_finite()
            .then(|| capsule_contact(self, motion, other, t))
    }
}

#[cfg(test)]
mod triangle_tests {
    use super::*;
    use crate::approx_eq::ApproxEq;
    use crate::epsilon::Epsilon;

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    /// The right triangle `(0,0,0) (0,0,4) (4,0,0)` in the y = 0 plane, normal
    /// +Y.
    fn floor() -> Triangle {
        Triangle::new(
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, 4.0),
            Vec3::new(4.0, 0.0, 0.0),
        )
        .unwrap()
    }

    fn capsule(start: Vec3, end: Vec3, radius: f32) -> Capsule {
        Capsule::new(Segment::new(start, end).unwrap(), radius).unwrap()
    }

    #[test]
    fn standing_capsule_lands_on_the_face_with_its_lower_cap() {
        let body = capsule(Vec3::new(1.0, 5.0, 1.0), Vec3::new(1.0, 7.0, 1.0), 1.0);
        let hit = body
            .sweep_triangle(Vec3::new(0.0, -10.0, 0.0), &floor())
            .unwrap();
        assert!(hit.time().approx_eq(&0.4, eps()));
        assert!(hit.point().approx_eq(&Vec3::new(1.0, 0.0, 1.0), eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_Y, eps()));
    }

    #[test]
    fn capsule_laid_across_the_triangle_lands_on_an_edge() {
        // Both caps hang far past the triangle, so only the shaft-versus-edge
        // crossing can stop this one.
        let bar = capsule(Vec3::new(-5.0, 5.0, 1.0), Vec3::new(5.0, 5.0, 1.0), 1.0);
        let hit = bar
            .sweep_triangle(Vec3::new(0.0, -10.0, 0.0), &floor())
            .unwrap();
        assert!(hit.time().approx_eq(&0.4, eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_Y, eps()));
        assert!(hit.point().y.approx_eq(&0.0, eps()));
    }

    #[test]
    fn capsule_shaft_swept_onto_a_vertex_stops_on_it() {
        // The caps sit two units clear of the triangle's plane, so only the
        // vertex-against-shaft cast can see this contact.
        let post = capsule(Vec3::new(10.0, -2.0, 0.0), Vec3::new(10.0, 2.0, 0.0), 1.0);
        let hit = post
            .sweep_triangle(Vec3::new(-10.0, 0.0, 0.0), &floor())
            .unwrap();
        assert!(hit.time().approx_eq(&0.5, eps()));
        assert!(hit.point().approx_eq(&Vec3::new(4.0, 0.0, 0.0), eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_X, eps()));
    }

    #[test]
    fn capsule_stopping_short_of_the_face_misses() {
        let body = capsule(Vec3::new(1.0, 5.0, 1.0), Vec3::new(1.0, 7.0, 1.0), 1.0);
        assert!(body
            .sweep_triangle(Vec3::new(0.0, -2.0, 0.0), &floor())
            .is_none());
    }

    #[test]
    fn capsule_swept_beside_the_triangle_misses() {
        let body = capsule(Vec3::new(9.0, 5.0, 9.0), Vec3::new(9.0, 7.0, 9.0), 1.0);
        assert!(body
            .sweep_triangle(Vec3::new(0.0, -10.0, 0.0), &floor())
            .is_none());
    }

    #[test]
    fn capsule_already_resting_on_the_face_hits_at_time_zero() {
        let body = capsule(Vec3::new(1.0, 0.5, 1.0), Vec3::new(1.0, 2.5, 1.0), 1.0);
        let hit = body
            .sweep_triangle(Vec3::new(0.0, -10.0, 0.0), &floor())
            .unwrap();
        assert_eq!(hit.time(), 0.0);
        assert!(hit.point().approx_eq(&Vec3::new(1.0, 0.0, 1.0), eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_Y, eps()));
    }

    #[test]
    fn a_motionless_capsule_hits_only_what_it_already_touches() {
        let resting = capsule(Vec3::new(1.0, 0.5, 1.0), Vec3::new(1.0, 2.5, 1.0), 1.0);
        assert_eq!(
            resting.sweep_triangle(Vec3::ZERO, &floor()).unwrap().time(),
            0.0
        );
        let clear = capsule(Vec3::new(1.0, 5.0, 1.0), Vec3::new(1.0, 7.0, 1.0), 1.0);
        assert!(clear.sweep_triangle(Vec3::ZERO, &floor()).is_none());
    }

    #[test]
    fn capsule_pierced_by_the_face_reports_the_overlap_immediately() {
        let skewer = capsule(Vec3::new(1.0, -3.0, 1.0), Vec3::new(1.0, 3.0, 1.0), 0.1);
        let hit = skewer
            .sweep_triangle(Vec3::new(0.0, -10.0, 0.0), &floor())
            .unwrap();
        assert_eq!(hit.time(), 0.0);
    }
}

#[cfg(test)]
mod capsule_tests {
    use super::*;
    use crate::approx_eq::ApproxEq;
    use crate::epsilon::Epsilon;

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    fn capsule(start: Vec3, end: Vec3, radius: f32) -> Capsule {
        Capsule::new(Segment::new(start, end).unwrap(), radius).unwrap()
    }

    #[test]
    fn parallel_capsules_meet_shaft_to_shaft() {
        let mover = capsule(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 3.0, 0.0), 1.0);
        let post = capsule(Vec3::new(10.0, 1.0, 0.0), Vec3::new(10.0, 3.0, 0.0), 1.0);
        let hit = mover
            .sweep_capsule(Vec3::new(10.0, 0.0, 0.0), &post)
            .unwrap();
        assert!(hit.time().approx_eq(&0.8, eps()));
        assert!(hit.normal().approx_eq(&Vec3::new(-1.0, 0.0, 0.0), eps()));
        assert!(hit.point().x.approx_eq(&9.0, eps()));
    }

    #[test]
    fn crossing_capsules_meet_in_both_shafts() {
        // Neither capsule's endpoints ever come near the other, so the answer
        // can only come from the two shafts crossing.
        let bar = capsule(Vec3::new(-2.0, 5.0, 0.0), Vec3::new(2.0, 5.0, 0.0), 0.5);
        let rail = capsule(Vec3::new(0.0, 0.0, -2.0), Vec3::new(0.0, 0.0, 2.0), 0.5);
        let hit = bar
            .sweep_capsule(Vec3::new(0.0, -10.0, 0.0), &rail)
            .unwrap();
        assert!(hit.time().approx_eq(&0.4, eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_Y, eps()));
        assert!(hit.point().approx_eq(&Vec3::new(0.0, 0.5, 0.0), eps()));
    }

    #[test]
    fn a_capsule_swept_onto_a_static_endpoint_stops_on_that_cap() {
        // The static capsule's own end is what the mover's shaft runs into.
        let bar = capsule(Vec3::new(10.0, 0.0, -3.0), Vec3::new(10.0, 0.0, 3.0), 0.5);
        let stub = capsule(Vec3::new(0.0, 0.0, 0.0), Vec3::new(-4.0, 0.0, 0.0), 0.5);
        let hit = bar
            .sweep_capsule(Vec3::new(-10.0, 0.0, 0.0), &stub)
            .unwrap();
        assert!(hit.time().approx_eq(&0.9, eps()));
        assert!(hit.normal().approx_eq(&Vec3::UNIT_X, eps()));
        assert!(hit.point().approx_eq(&Vec3::new(0.5, 0.0, 0.0), eps()));
    }

    #[test]
    fn a_capsule_stopping_short_misses() {
        let mover = capsule(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 3.0, 0.0), 1.0);
        let post = capsule(Vec3::new(10.0, 1.0, 0.0), Vec3::new(10.0, 3.0, 0.0), 1.0);
        assert!(mover
            .sweep_capsule(Vec3::new(4.0, 0.0, 0.0), &post)
            .is_none());
    }

    #[test]
    fn a_capsule_swept_past_another_misses() {
        let mover = capsule(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 3.0, 0.0), 1.0);
        let post = capsule(Vec3::new(10.0, 1.0, 0.0), Vec3::new(10.0, 3.0, 0.0), 1.0);
        assert!(mover
            .sweep_capsule(Vec3::new(0.0, 0.0, 10.0), &post)
            .is_none());
    }

    #[test]
    fn capsules_that_already_overlap_hit_at_time_zero() {
        let mover = capsule(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 3.0, 0.0), 1.0);
        let over = capsule(Vec3::new(1.5, 1.0, 0.0), Vec3::new(1.5, 3.0, 0.0), 1.0);
        let hit = mover
            .sweep_capsule(Vec3::new(10.0, 0.0, 0.0), &over)
            .unwrap();
        assert_eq!(hit.time(), 0.0);
        assert!(hit.normal().approx_eq(&Vec3::new(-1.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn a_motionless_capsule_hits_only_what_it_already_touches() {
        let mover = capsule(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 3.0, 0.0), 1.0);
        let over = capsule(Vec3::new(1.5, 1.0, 0.0), Vec3::new(1.5, 3.0, 0.0), 1.0);
        let far = capsule(Vec3::new(10.0, 1.0, 0.0), Vec3::new(10.0, 3.0, 0.0), 1.0);
        assert_eq!(mover.sweep_capsule(Vec3::ZERO, &over).unwrap().time(), 0.0);
        assert!(mover.sweep_capsule(Vec3::ZERO, &far).is_none());
    }

    #[test]
    fn coincident_axes_fall_back_to_pushing_the_mover_back() {
        let mover = capsule(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 3.0, 0.0), 1.0);
        let hit = mover.sweep_capsule(Vec3::UNIT_X, &mover).unwrap();
        assert_eq!(hit.time(), 0.0);
        assert!(hit.normal().approx_eq(&Vec3::new(-1.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn coincident_axes_without_motion_have_no_direction_at_all() {
        let mover = capsule(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 3.0, 0.0), 1.0);
        let hit = mover.sweep_capsule(Vec3::ZERO, &mover).unwrap();
        assert!(hit.normal().approx_eq(&Vec3::ZERO, eps()));
    }
}
