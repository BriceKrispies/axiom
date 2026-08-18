//! Sweeping a capsule through the world: the exact per-shape swept tests, and
//! the branchless table that dispatches them.
//!
//! This is the query a character controller asks once per step — *what will my
//! body hit on the way there* — and the one a discrete overlap test can never
//! answer, because a fast body is on one side of a wall before the step and the
//! other side after it, and overlaps it at neither instant.
//!
//! | kind        | test                                                            |
//! |-------------|-----------------------------------------------------------------|
//! | Sphere      | [`axiom_math::Capsule::sweep_capsule`] against a zero-length capsule |
//! | Box         | [`axiom_math::Capsule::sweep_triangle`] over the box's twelve surface triangles, nearest wins |
//! | Capsule     | [`axiom_math::Capsule::sweep_capsule`]                           |
//! | Plane       | analytic: the leading end of the axis reaching the half-space    |
//! | Heightfield | **unsupported** — never hit                                      |
//!
//! ## Time, and the already-overlapping case
//! [`Hit::time`] is a **fraction of the motion vector**, in `[0, 1]` — not a
//! distance. A sweep that begins already overlapping reports `t = 0` with a
//! usable normal, which is exactly what a controller needs to escape a wall it is
//! standing in; the caller tells that case from a genuine mid-step contact by
//! [`QueryHit::front_face`], which is computed here from the very same overlap
//! relation the discrete query uses ([`overlaps_capsule`]), so the two can never
//! disagree about whether a body is inside something.

use axiom_math::{Capsule, Hit, Quat, Segment, Triangle, Vec3};

use crate::collider_capsule::world_capsule;
use crate::collider_obb::{obb_triangles, world_obb};
use crate::physics_collider_shape::PhysicsColliderShape;
use crate::physics_shape_kind::PhysicsShapeKind;
use crate::query_hit::QueryHit;
use crate::query_overlap::overlaps_capsule;

/// The exact swept function for one shape kind.
type SweepFn = fn(PhysicsColliderShape, Vec3, Quat, &Capsule, Vec3) -> Option<Hit>;

/// Exact per-kind swept functions, indexed by `kind().index()`. Sized by
/// [`PhysicsShapeKind::COUNT`] so it cannot fall behind the enum.
const SWEEP_TABLE: [SweepFn; PhysicsShapeKind::COUNT] = [
    sweep_sphere_shape,
    sweep_box_shape,
    sweep_capsule_shape,
    sweep_plane_shape,
    sweep_heightfield_shape,
];

/// Where `query` first touches a collider while travelling `motion`, dispatched
/// branchlessly on the shape kind.
pub(crate) fn sweep_shape(
    shape: PhysicsColliderShape,
    center: Vec3,
    rotation: Quat,
    query: &Capsule,
    motion: Vec3,
) -> Option<QueryHit> {
    SWEEP_TABLE[shape.kind().index()](shape, center, rotation, query, motion)
        .map(|hit| QueryHit::new(hit, !overlaps_capsule(shape, center, rotation, query)))
}

/// A sphere collider as the zero-length capsule it is, so one swept solve serves
/// both kinds.
fn sweep_sphere_shape(
    shape: PhysicsColliderShape,
    center: Vec3,
    _rotation: Quat,
    query: &Capsule,
    motion: Vec3,
) -> Option<Hit> {
    Segment::new(center, center)
        .and_then(|point| Capsule::new(point, shape.radius()))
        .ok()
        .and_then(|ball| query.sweep_capsule(motion, &ball))
}

/// The nearest of the swept contacts against the box's twelve surface triangles.
/// The minimum is taken over *all* of them rather than stopping at the first,
/// because the triangles are in face order, not in the order the sweep reaches
/// them.
///
/// A query volume **swallowed whole** by the box is the case the triangle sweep
/// alone gets wrong: it touches none of the twelve, so the fold would answer with
/// the far face it eventually leaves through — a contact part-way into the step,
/// reported as if the caster were still outside. A capsule that is inside a solid
/// box is in contact *now*, so containment is answered separately, by
/// [`escape_hit`], at time zero.
fn sweep_box_shape(
    shape: PhysicsColliderShape,
    center: Vec3,
    rotation: Quat,
    query: &Capsule,
    motion: Vec3,
) -> Option<Hit> {
    world_obb(shape, center, rotation).and_then(|obb| {
        let triangles = obb_triangles(&obb);
        let swept = triangles
            .iter()
            .filter_map(|triangle| query.sweep_triangle(motion, triangle))
            .fold(None, |best: Option<Hit>, candidate| {
                let closer = best.map_or(true, |held| candidate.time() < held.time());
                [best, Some(candidate)][usize::from(closer)]
            });
        let axis = query.segment();
        let contained =
            obb.contains_point(axis.start()) | obb.contains_point(axis.end());
        [swept, escape_hit(query, &triangles)][usize::from(contained)]
    })
}

/// The immediate (`t = 0`) contact for a query volume already inside the box: the
/// nearest point of the box's surface, with the normal pointing from that surface
/// back at the caster — the direction it has to move to get out.
fn escape_hit(query: &Capsule, triangles: &[Triangle]) -> Option<Hit> {
    let axis = query.segment();
    triangles
        .iter()
        .fold(None, |best: Option<(Vec3, Vec3)>, triangle| {
            let candidate = axis.closest_points_to_triangle(triangle);
            let closer = best.map_or(true, |held| {
                candidate.0.subtract(candidate.1).length_squared()
                    < held.0.subtract(held.1).length_squared()
            });
            [best, Some(candidate)][usize::from(closer)]
        })
        .map(|(mine, theirs)| {
            let normal = mine.subtract(theirs).normalize().unwrap_or(Vec3::ZERO);
            Hit::new(0.0, theirs, normal)
        })
}

/// Capsule against capsule, on the collider's true rotated axis.
fn sweep_capsule_shape(
    shape: PhysicsColliderShape,
    center: Vec3,
    rotation: Quat,
    query: &Capsule,
    motion: Vec3,
) -> Option<Hit> {
    world_capsule(shape, center, rotation)
        .and_then(|capsule| query.sweep_capsule(motion, &capsule))
}

/// Analytic sweep against the solid half-space. The signed distance of each end
/// of the query axis changes at the constant rate `normal · motion`, so the
/// touch time solves in one step: the **deepest** end reaches the surface first,
/// at `t = (radius - deepest) / (normal · motion)`. A query already within a
/// radius of the surface (or past it, in the solid) reports `t = 0`.
///
/// The contact point is the deepest end's arrival, projected onto the surface. A
/// query lying exactly parallel touches along its whole length rather than at a
/// point; the deterministic choice of its `start` end is the same single-point
/// limitation every manifold in this module carries.
fn sweep_plane_shape(
    shape: PhysicsColliderShape,
    _center: Vec3,
    _rotation: Quat,
    query: &Capsule,
    motion: Vec3,
) -> Option<Hit> {
    let normal = shape.normal();
    let axis = query.segment();
    let (first, second) = (
        normal.dot(axis.start()) - shape.offset(),
        normal.dot(axis.end()) - shape.offset(),
    );
    let deepest = first.min(second);
    let leading = [axis.end(), axis.start()][usize::from(first <= second)];
    let speed = normal.dot(motion);
    let solved = (query.radius() - deepest) / speed;
    let already = deepest <= query.radius();
    let reaches = (speed < 0.0) & (solved >= 0.0) & (solved <= 1.0);
    let t = [solved, 0.0][usize::from(already)];
    let arrived = leading.add(motion.mul_scalar(t));
    let point = arrived.subtract(normal.mul_scalar(normal.dot(arrived) - shape.offset()));
    (already | reaches).then(|| Hit::new(t, point, normal))
}

/// A heightfield is explicitly unsupported by shape casting — never hit. See
/// [`crate::query_ray`] for why this is an exclusion rather than an
/// approximation.
fn sweep_heightfield_shape(
    _shape: PhysicsColliderShape,
    _center: Vec3,
    _rotation: Quat,
    _query: &Capsule,
    _motion: Vec3,
) -> Option<Hit> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Meters;
    use core::f32::consts::FRAC_PI_2;

    fn sphere(r: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::sphere(Meters::new(r).unwrap()).unwrap()
    }

    fn box_shape(x: f32, y: f32, z: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::box_shape(Vec3::new(x, y, z)).unwrap()
    }

    fn capsule_shape(r: f32, hh: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::capsule(Meters::new(r).unwrap(), Meters::new(hh).unwrap()).unwrap()
    }

    fn plane(normal: Vec3, distance: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::plane(normal, Meters::new(distance).unwrap()).unwrap()
    }

    fn heightfield() -> PhysicsColliderShape {
        PhysicsColliderShape::heightfield_shape(Vec3::new(4.0, 1.0, 4.0)).unwrap()
    }

    /// A standing body capsule: axis from `y` to `y + 2`, radius 0.5.
    fn body(x: f32, y: f32) -> Capsule {
        Capsule::new(
            Segment::new(Vec3::new(x, y, 0.0), Vec3::new(x, y + 2.0, 0.0)).unwrap(),
            0.5,
        )
        .unwrap()
    }

    fn id() -> Quat {
        Quat::IDENTITY
    }

    fn approx(a: Vec3, b: Vec3) {
        assert!(
            a.subtract(b).length_squared() < 1.0e-6,
            "expected {b:?}, got {a:?}"
        );
    }

    #[test]
    fn a_body_falling_onto_a_sphere_stops_on_it_part_way_through_the_step() {
        // Sphere r = 1 at the origin; the body's lower cap starts at y = 4.5 and
        // the step carries it 10 down. Contact when the axis end reaches
        // 1 + 0.5 = 1.5, i.e. after 3.5 of the 10 -> t = 0.35.
        let hit = sweep_shape(
            sphere(1.0),
            Vec3::ZERO,
            id(),
            &body(0.0, 5.0),
            Vec3::new(0.0, -10.0, 0.0),
        )
        .expect("the falling body reaches the sphere");
        assert!((hit.hit().time() - 0.35).abs() < 1.0e-4, "t was {}", hit.hit().time());
        approx(hit.hit().normal(), Vec3::UNIT_Y);
        approx(hit.hit().point(), Vec3::new(0.0, 1.0, 0.0));
        assert!(hit.front_face());
    }

    #[test]
    fn a_body_stopping_short_of_a_sphere_misses_it() {
        assert!(sweep_shape(
            sphere(1.0),
            Vec3::ZERO,
            id(),
            &body(0.0, 5.0),
            Vec3::new(0.0, -2.0, 0.0)
        )
        .is_none());
    }

    #[test]
    fn a_body_already_inside_a_sphere_reports_zero_and_a_back_face() {
        let hit = sweep_shape(
            sphere(2.0),
            Vec3::ZERO,
            id(),
            &body(0.0, -1.0),
            Vec3::new(10.0, 0.0, 0.0),
        )
        .expect("an overlapping body is an immediate hit");
        assert_eq!(hit.hit().time(), 0.0);
        assert!(!hit.front_face());
    }

    #[test]
    fn an_unliftable_sphere_centre_is_a_miss() {
        assert!(sweep_shape(
            sphere(1.0),
            Vec3::new(f32::NAN, 0.0, 0.0),
            id(),
            &body(0.0, 5.0),
            Vec3::new(0.0, -10.0, 0.0)
        )
        .is_none());
    }

    #[test]
    fn a_body_walking_into_a_box_stops_on_its_face() {
        // Unit box at the origin; the body walks in along +X from x = -5. Its
        // shaft meets the -X face at x = -1.5, i.e. after 3.5 of a 10 step.
        let hit = sweep_shape(
            box_shape(1.0, 1.0, 1.0),
            Vec3::ZERO,
            id(),
            &body(-5.0, -1.0),
            Vec3::new(10.0, 0.0, 0.0),
        )
        .expect("the body reaches the box");
        assert!((hit.hit().time() - 0.35).abs() < 1.0e-4, "t was {}", hit.hit().time());
        approx(hit.hit().normal(), Vec3::new(-1.0, 0.0, 0.0));
        assert!((hit.hit().point().x + 1.0).abs() < 1.0e-4);
        assert!(hit.front_face());
    }

    #[test]
    fn a_body_swept_past_a_box_misses_it() {
        assert!(sweep_shape(
            box_shape(1.0, 1.0, 1.0),
            Vec3::ZERO,
            id(),
            &body(-5.0, 9.0),
            Vec3::new(10.0, 0.0, 0.0)
        )
        .is_none());
    }

    #[test]
    fn the_nearest_face_of_a_box_wins_not_the_first_one_tested() {
        // Walking in along +X, the box's far (+X) face is also swept — at a much
        // later time. The reported contact must be the near face.
        let hit = sweep_shape(
            box_shape(1.0, 1.0, 1.0),
            Vec3::ZERO,
            id(),
            &body(-5.0, -1.0),
            Vec3::new(20.0, 0.0, 0.0),
        )
        .expect("the body crosses the whole box");
        approx(hit.hit().normal(), Vec3::new(-1.0, 0.0, 0.0));
        assert!(hit.hit().time() < 0.25, "t was {}", hit.hit().time());
    }

    #[test]
    fn a_turned_box_is_swept_on_its_real_extent() {
        // A slab yawed a quarter turn about Y reaches |z| = 4 and only |x| = 1,
        // so a body walking in along +X now passes it, while one walking in along
        // +Z is stopped early.
        let yaw = Quat::from_axis_angle(Vec3::UNIT_Y, FRAC_PI_2).unwrap();
        let slab = box_shape(4.0, 1.0, 1.0);
        assert!(sweep_shape(
            slab,
            Vec3::ZERO,
            yaw,
            &body(-9.0, -1.0),
            Vec3::new(6.0, 0.0, 0.0)
        )
        .is_none());
        let stopped = Capsule::new(
            Segment::new(Vec3::new(0.0, -1.0, -9.0), Vec3::new(0.0, 1.0, -9.0)).unwrap(),
            0.5,
        )
        .unwrap();
        assert!(
            sweep_shape(slab, Vec3::ZERO, yaw, &stopped, Vec3::new(0.0, 0.0, 6.0)).is_some(),
            "the turned slab reaches z = -4"
        );
    }

    #[test]
    fn a_body_swallowed_by_a_box_is_in_contact_now_not_on_its_way_out() {
        // Entirely inside a large box, touching none of its twelve triangles. The
        // swept fold alone would answer with the far face the body eventually
        // leaves through — a contact part-way into the step, as if it were still
        // outside. It is inside a solid: distance zero, back face, and a normal
        // pointing at the nearest way out.
        let hit = sweep_shape(
            box_shape(10.0, 10.0, 10.0),
            Vec3::ZERO,
            id(),
            &body(0.0, -1.0),
            Vec3::new(20.0, 0.0, 0.0),
        )
        .expect("a contained body is an immediate hit");
        assert_eq!(hit.hit().time(), 0.0);
        assert!(!hit.front_face());
        assert!(
            (hit.hit().normal().length() - 1.0).abs() < 1.0e-5,
            "the escape normal must be a usable unit vector, got {:?}",
            hit.hit().normal()
        );
        // The nearest surface to an axis spanning y in [-1, 1] at the origin of a
        // 10-cube is a side face, 10 away — never the +X face 20 along the motion.
        assert!(hit.hit().point().subtract(Vec3::ZERO).length() < 14.2);
    }

    #[test]
    fn an_unliftable_box_is_a_miss() {
        assert!(sweep_shape(
            box_shape(1.0, 1.0, 1.0),
            Vec3::new(f32::NAN, 0.0, 0.0),
            id(),
            &body(-5.0, -1.0),
            Vec3::new(10.0, 0.0, 0.0)
        )
        .is_none());
    }

    #[test]
    fn a_body_walking_into_a_capsule_stops_on_its_shaft() {
        // An upright collider capsule r = 1 about the origin. The body (r = 0.5)
        // walks in along +X from x = -5 and stops when the axes are 1.5 apart,
        // i.e. at x = -1.5, after 3.5 of a 10 step.
        let hit = sweep_shape(
            capsule_shape(1.0, 1.0),
            Vec3::ZERO,
            id(),
            &body(-5.0, -1.0),
            Vec3::new(10.0, 0.0, 0.0),
        )
        .expect("the body reaches the collider");
        assert!((hit.hit().time() - 0.35).abs() < 1.0e-4, "t was {}", hit.hit().time());
        approx(hit.hit().normal(), Vec3::new(-1.0, 0.0, 0.0));
        assert!(hit.front_face());
    }

    #[test]
    fn a_tipped_capsule_collider_is_swept_on_its_rotated_axis() {
        // Tipped along X the collider now blocks a body that would have walked
        // straight past the upright one.
        let tipped = Quat::from_axis_angle(Vec3::UNIT_Z, FRAC_PI_2).unwrap();
        let long = capsule_shape(0.5, 4.0);
        assert!(sweep_shape(
            long,
            Vec3::ZERO,
            id(),
            &body(-9.0, -1.0),
            Vec3::new(6.0, 0.0, 0.0)
        )
        .is_none());
        assert!(sweep_shape(
            long,
            Vec3::ZERO,
            tipped,
            &body(-9.0, -1.0),
            Vec3::new(6.0, 0.0, 0.0)
        )
        .is_some());
    }

    #[test]
    fn an_unliftable_capsule_collider_is_a_miss() {
        assert!(sweep_shape(
            capsule_shape(1.0, 1.0),
            Vec3::new(0.0, f32::INFINITY, 0.0),
            id(),
            &body(-5.0, -1.0),
            Vec3::new(10.0, 0.0, 0.0)
        )
        .is_none());
    }

    #[test]
    fn a_body_falling_onto_a_plane_lands_on_it() {
        // Ground y = 0; the body's lower end starts at y = 5 and falls 10. It
        // lands when that end is one radius up, at y = 0.5: t = 4.5 / 10.
        let hit = sweep_shape(
            plane(Vec3::UNIT_Y, 0.0),
            Vec3::ZERO,
            id(),
            &body(0.0, 5.0),
            Vec3::new(0.0, -10.0, 0.0),
        )
        .expect("the falling body reaches the ground");
        assert!((hit.hit().time() - 0.45).abs() < 1.0e-5, "t was {}", hit.hit().time());
        approx(hit.hit().normal(), Vec3::UNIT_Y);
        approx(hit.hit().point(), Vec3::ZERO);
        assert!(hit.front_face());
    }

    #[test]
    fn a_body_stopping_short_of_a_plane_or_moving_along_it_misses() {
        let ground = plane(Vec3::UNIT_Y, 0.0);
        // Falls, but not far enough.
        assert!(sweep_shape(
            ground,
            Vec3::ZERO,
            id(),
            &body(0.0, 5.0),
            Vec3::new(0.0, -1.0, 0.0)
        )
        .is_none());
        // Travels parallel to the surface, never approaching it.
        assert!(sweep_shape(
            ground,
            Vec3::ZERO,
            id(),
            &body(0.0, 5.0),
            Vec3::new(50.0, 0.0, 0.0)
        )
        .is_none());
        // Moves away from it.
        assert!(sweep_shape(
            ground,
            Vec3::ZERO,
            id(),
            &body(0.0, 5.0),
            Vec3::new(0.0, 50.0, 0.0)
        )
        .is_none());
    }

    #[test]
    fn a_body_already_standing_in_a_plane_reports_zero_and_a_back_face() {
        let hit = sweep_shape(
            plane(Vec3::UNIT_Y, 0.0),
            Vec3::ZERO,
            id(),
            &body(0.0, -3.0),
            Vec3::new(0.0, -10.0, 0.0),
        )
        .expect("a body inside the ground is an immediate hit");
        assert_eq!(hit.hit().time(), 0.0);
        approx(hit.hit().normal(), Vec3::UNIT_Y);
        assert!(!hit.front_face());
    }

    #[test]
    fn a_heightfield_is_never_swept() {
        assert!(sweep_shape(
            heightfield(),
            Vec3::ZERO,
            id(),
            &body(0.0, 5.0),
            Vec3::new(0.0, -10.0, 0.0)
        )
        .is_none());
    }

    #[test]
    fn every_shape_kind_has_a_table_entry() {
        assert_eq!(SWEEP_TABLE.len(), PhysicsShapeKind::COUNT);
        let shapes = [
            sphere(1.0),
            box_shape(1.0, 1.0, 1.0),
            capsule_shape(1.0, 1.0),
            plane(Vec3::UNIT_Y, 0.0),
            heightfield(),
        ];
        let struck = shapes
            .into_iter()
            .filter(|s| {
                sweep_shape(
                    *s,
                    Vec3::ZERO,
                    id(),
                    &body(0.0, 5.0),
                    Vec3::new(0.0, -10.0, 0.0),
                )
                .is_some()
            })
            .count();
        assert_eq!(struck, 4, "only the heightfield is excluded");
    }
}
