//! Exact per-shape ray casting, and the branchless table that dispatches it.
//!
//! Every supported shape is tested against its **true geometry**, not a bounding
//! volume, and every one of them is **rotation-aware**: the owning body's
//! rotation is passed in and genuinely used, so a ray strikes a turned box on its
//! real tilted face and a tipped capsule on its real shaft.
//!
//! | kind        | test                                                        |
//! |-------------|-------------------------------------------------------------|
//! | Sphere      | the ray/sphere quadratic (rotation-invariant)               |
//! | Box         | [`axiom_math::Obb::raycast`] — exact oriented slab entry     |
//! | Capsule     | [`axiom_math::Capsule::raycast`] — shaft plus both caps      |
//! | Plane       | analytic half-space intersection                            |
//! | Heightfield | **unsupported** — never hit (see below)                     |
//!
//! ## Convention
//! A ray whose origin is already inside a shape reports an immediate hit at
//! distance `0` with `front_face = false` — the convention
//! [`axiom_math::Ray::intersect_aabb_entry`], `Obb::raycast` and
//! `Capsule::raycast` all share. That is deliberately *not* a miss: a caster that
//! begins inside solid geometry needs to know it, and reporting nothing would
//! make "no wall here" and "I am inside the wall" the same answer.
//!
//! ## Heightfield is explicitly unsupported, not approximated
//! An exact heightfield ray cast requires marching the ray across the grid's
//! cells, which the flat per-shape signature here cannot reach (the grid lives on
//! the collider, not on the shape) and which is unbounded work per cast. Rather
//! than silently fall back to the heightfield's bounding box — a false-positive
//! generator that would report hits on empty sky above a valley — a heightfield
//! collider is **excluded**: a ray never hits it. The narrow phase makes the same
//! split (its heightfield contacts are generated outside the table); closing this
//! gap means giving the query dispatch access to the collider, not loosening the
//! test.

use axiom_math::{Hit, Quat, Ray, Vec3};

use crate::collider_capsule::world_capsule;
use crate::collider_obb::world_obb;
use crate::physics_collider_shape::PhysicsColliderShape;
use crate::physics_shape_kind::PhysicsShapeKind;
use crate::query_hit::QueryHit;

/// A ray/plane that is closer to parallel than this is treated as a miss.
const PLANE_PARALLEL_EPSILON: f32 = 1.0e-7;

/// The exact ray function for one shape kind.
type RayFn = fn(PhysicsColliderShape, Vec3, Quat, &Ray) -> Option<QueryHit>;

/// Exact per-kind ray functions, indexed by `kind().index()`. The length comes
/// from [`PhysicsShapeKind::COUNT`], so a new shape kind cannot be added without
/// this initializer failing to compile — the drift that once let a heightfield
/// collider index past the end of a four-entry table and panic.
const RAY_TABLE: [RayFn; PhysicsShapeKind::COUNT] = [
    ray_sphere,
    ray_box,
    ray_capsule,
    ray_plane,
    ray_heightfield,
];

/// The exact ray hit on a collider, or `None` for a miss, dispatched branchlessly
/// on the shape kind.
pub(crate) fn ray_shape(
    shape: PhysicsColliderShape,
    center: Vec3,
    rotation: Quat,
    ray: &Ray,
) -> Option<QueryHit> {
    RAY_TABLE[shape.kind().index()](shape, center, rotation, ray)
}

/// Exact ray/sphere intersection. Solves `|origin + t·dir − center|² = r²` for
/// the nearest non-negative root, clamping an origin-inside hit to `0`. A
/// negative discriminant (the line misses) or both roots behind the origin is a
/// miss — so a ray that only clips the sphere's AABB returns `None`. The normal
/// is the outward radial direction at the contact; an origin exactly at the
/// centre has no radial direction and falls back to facing the caster.
fn ray_sphere(
    shape: PhysicsColliderShape,
    center: Vec3,
    _rotation: Quat,
    ray: &Ray,
) -> Option<QueryHit> {
    let oc = ray.origin().subtract(center);
    let r = shape.radius();
    let b = oc.dot(ray.direction());
    let c = oc.dot(oc) - r * r;
    let discriminant = b * b - c;
    // `max(0.0)` keeps the sqrt finite when the line misses; `hits` is false then.
    let root = discriminant.max(0.0).sqrt();
    let hits = (discriminant >= 0.0) & ((root - b) >= 0.0);
    let entry = (-b - root).max(0.0);
    let point = ray.point_at(entry);
    let normal = point
        .subtract(center)
        .normalize()
        .unwrap_or(ray.direction().mul_scalar(-1.0));
    hits.then(|| QueryHit::new(Hit::new(entry, point, normal), c > 0.0))
}

/// Exact ray/oriented-box intersection: the box's true tilted faces, with the
/// outward normal of the face the ray entered through.
fn ray_box(
    shape: PhysicsColliderShape,
    center: Vec3,
    rotation: Quat,
    ray: &Ray,
) -> Option<QueryHit> {
    world_obb(shape, center, rotation).and_then(|obb| {
        obb.raycast(ray)
            .map(|hit| QueryHit::new(hit, !obb.contains_point(ray.origin())))
    })
}

/// Exact ray/capsule intersection: the shaft and both hemispherical caps, on the
/// capsule's true rotated axis.
fn ray_capsule(
    shape: PhysicsColliderShape,
    center: Vec3,
    rotation: Quat,
    ray: &Ray,
) -> Option<QueryHit> {
    world_capsule(shape, center, rotation).and_then(|capsule| {
        capsule
            .raycast(ray)
            .map(|hit| QueryHit::new(hit, !capsule.contains_point(ray.origin())))
    })
}

/// Analytic ray/plane intersection. The plane is the solid half-space
/// `n · x <= offset`; its body centre and rotation are irrelevant. A ray starting
/// on the solid side is already inside and reports distance `0`; otherwise a ray
/// within [`PLANE_PARALLEL_EPSILON`] of parallel, or whose intersection lies
/// behind the origin, is a miss. The normal is always the plane's own outward
/// normal — the direction out of the solid.
fn ray_plane(
    shape: PhysicsColliderShape,
    _center: Vec3,
    _rotation: Quat,
    ray: &Ray,
) -> Option<QueryHit> {
    let normal = shape.normal();
    let signed = normal.dot(ray.origin()) - shape.offset();
    let inside = signed < 0.0;
    let denominator = normal.dot(ray.direction());
    let crossing = -signed / denominator;
    let enters = !inside & (denominator.abs() > PLANE_PARALLEL_EPSILON) & (crossing >= 0.0);
    let t = [crossing, 0.0][usize::from(inside)];
    (enters | inside).then(|| QueryHit::new(Hit::new(t, ray.point_at(t), normal), !inside))
}

/// A heightfield is explicitly unsupported by ray casting — never a hit. See the
/// module docs for why this is an exclusion rather than an approximation.
fn ray_heightfield(
    _shape: PhysicsColliderShape,
    _center: Vec3,
    _rotation: Quat,
    _ray: &Ray,
) -> Option<QueryHit> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Meters;
    use core::f32::consts::{FRAC_PI_2, FRAC_PI_4};

    fn sphere(r: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::sphere(Meters::new(r).unwrap()).unwrap()
    }

    fn box_shape(x: f32, y: f32, z: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::box_shape(Vec3::new(x, y, z)).unwrap()
    }

    fn capsule(r: f32, hh: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::capsule(Meters::new(r).unwrap(), Meters::new(hh).unwrap()).unwrap()
    }

    fn plane(normal: Vec3, distance: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::plane(normal, Meters::new(distance).unwrap()).unwrap()
    }

    fn heightfield() -> PhysicsColliderShape {
        PhysicsColliderShape::heightfield_shape(Vec3::new(4.0, 1.0, 4.0)).unwrap()
    }

    fn ray(origin: Vec3, direction: Vec3) -> Ray {
        Ray::new(origin, direction).unwrap()
    }

    fn id() -> Quat {
        Quat::IDENTITY
    }

    fn approx(a: Vec3, b: Vec3) {
        assert!(
            a.subtract(b).length_squared() < 1.0e-8,
            "expected {b:?}, got {a:?}"
        );
    }

    #[test]
    fn a_ray_into_a_sphere_reports_distance_point_and_outward_normal() {
        let hit = ray_shape(
            sphere(1.0),
            Vec3::ZERO,
            id(),
            &ray(Vec3::new(-10.0, 0.0, 0.0), Vec3::UNIT_X),
        )
        .expect("the ray strikes the sphere");
        assert!((hit.hit().time() - 9.0).abs() < 1.0e-5);
        approx(hit.hit().point(), Vec3::new(-1.0, 0.0, 0.0));
        approx(hit.hit().normal(), Vec3::new(-1.0, 0.0, 0.0));
        assert!(hit.front_face());
    }

    #[test]
    fn a_ray_that_only_clips_the_sphere_aabb_misses_but_the_box_of_the_same_size_hits() {
        // sqrt(0.9^2 + 0.9^2) ~ 1.273 > 1: inside the AABB, outside the sphere.
        let grazing = ray(Vec3::new(-10.0, 0.9, 0.9), Vec3::UNIT_X);
        assert!(ray_shape(sphere(1.0), Vec3::ZERO, id(), &grazing).is_none());
        assert!(ray_shape(box_shape(1.0, 1.0, 1.0), Vec3::ZERO, id(), &grazing).is_some());
    }

    #[test]
    fn a_ray_aimed_away_from_a_sphere_misses() {
        assert!(ray_shape(
            sphere(1.0),
            Vec3::ZERO,
            id(),
            &ray(Vec3::new(-10.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0))
        )
        .is_none());
    }

    #[test]
    fn a_ray_starting_inside_a_sphere_hits_at_zero_on_its_back_face() {
        let inside = ray_shape(
            sphere(2.0),
            Vec3::ZERO,
            id(),
            &ray(Vec3::new(0.5, 0.0, 0.0), Vec3::UNIT_X),
        )
        .expect("an origin inside the sphere is a hit");
        assert_eq!(inside.hit().time(), 0.0);
        approx(inside.hit().normal(), Vec3::UNIT_X);
        assert!(!inside.front_face());

        // Exactly at the centre there is no radial direction: face the caster.
        let centred = ray_shape(sphere(2.0), Vec3::ZERO, id(), &ray(Vec3::ZERO, Vec3::UNIT_X))
            .expect("the centre is inside");
        approx(centred.hit().normal(), Vec3::new(-1.0, 0.0, 0.0));
        assert!(!centred.front_face());
    }

    #[test]
    fn a_ray_strikes_a_turned_box_on_its_real_face() {
        // A long slab yawed a quarter turn about Y now reaches |z| = 4 and only
        // |x| = 1: a ray down -Z strikes its end cap at z = 4, and a ray along X
        // at x = 3 (which the unturned slab would have hit) now misses.
        let yaw = Quat::from_axis_angle(Vec3::UNIT_Y, FRAC_PI_2).unwrap();
        let slab = box_shape(4.0, 1.0, 1.0);
        let hit = ray_shape(
            slab,
            Vec3::ZERO,
            yaw,
            &ray(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -1.0)),
        )
        .expect("the turned slab reaches z = 4");
        assert!((hit.hit().time() - 6.0).abs() < 1.0e-4);
        approx(hit.hit().normal(), Vec3::UNIT_Z);
        assert!(ray_shape(
            slab,
            Vec3::ZERO,
            yaw,
            &ray(Vec3::new(3.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -1.0))
        )
        .is_none());
    }

    #[test]
    fn a_ray_misses_a_box_it_clears_and_starts_inside_one_it_is_within() {
        let cube = box_shape(1.0, 1.0, 1.0);
        assert!(ray_shape(
            cube,
            Vec3::ZERO,
            id(),
            &ray(Vec3::new(-10.0, 5.0, 0.0), Vec3::UNIT_X)
        )
        .is_none());
        let inside = ray_shape(cube, Vec3::ZERO, id(), &ray(Vec3::ZERO, Vec3::UNIT_X))
            .expect("an origin inside the box is a hit");
        assert_eq!(inside.hit().time(), 0.0);
        assert!(!inside.front_face());
    }

    #[test]
    fn an_unliftable_box_is_a_miss() {
        assert!(ray_shape(
            box_shape(1.0, 1.0, 1.0),
            Vec3::new(f32::NAN, 0.0, 0.0),
            id(),
            &ray(Vec3::ZERO, Vec3::UNIT_X)
        )
        .is_none());
    }

    #[test]
    fn a_ray_strikes_a_capsule_on_its_shaft_and_its_caps() {
        // r = 1, half-height 1 about the origin: the shaft spans y in [-1, 1] and
        // the caps reach y = +/-2.
        let shaft = ray_shape(
            capsule(1.0, 1.0),
            Vec3::ZERO,
            id(),
            &ray(Vec3::new(-10.0, 0.0, 0.0), Vec3::UNIT_X),
        )
        .expect("the shaft is struck");
        assert!((shaft.hit().time() - 9.0).abs() < 1.0e-5);
        approx(shaft.hit().normal(), Vec3::new(-1.0, 0.0, 0.0));
        assert!(shaft.front_face());

        let cap = ray_shape(
            capsule(1.0, 1.0),
            Vec3::ZERO,
            id(),
            &ray(Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, -1.0, 0.0)),
        )
        .expect("the upper cap is struck");
        assert!((cap.hit().time() - 8.0).abs() < 1.0e-5);
        approx(cap.hit().normal(), Vec3::UNIT_Y);
    }

    #[test]
    fn a_tipped_capsule_is_struck_along_its_rotated_axis() {
        // Tipped a quarter turn about Z the shaft lies along X and reaches x = 4,
        // so a ray along -X at y = 0 strikes it far from where an upright capsule
        // would have been, and a ray that would have hit the upright shaft at
        // y = 3 now misses entirely.
        let tipped = Quat::from_axis_angle(Vec3::UNIT_Z, FRAC_PI_2).unwrap();
        let post = capsule(1.0, 3.0);
        let hit = ray_shape(
            post,
            Vec3::ZERO,
            tipped,
            &ray(Vec3::new(10.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0)),
        )
        .expect("the tipped shaft reaches x = 4");
        assert!((hit.hit().time() - 6.0).abs() < 1.0e-4);
        assert!(ray_shape(
            post,
            Vec3::ZERO,
            tipped,
            &ray(Vec3::new(-10.0, 3.0, 0.0), Vec3::UNIT_X)
        )
        .is_none());
    }

    #[test]
    fn a_ray_starting_inside_a_capsule_hits_at_zero() {
        let inside = ray_shape(
            capsule(2.0, 1.0),
            Vec3::ZERO,
            id(),
            &ray(Vec3::new(0.5, 0.0, 0.0), Vec3::UNIT_X),
        )
        .expect("an origin inside the capsule is a hit");
        assert_eq!(inside.hit().time(), 0.0);
        assert!(!inside.front_face());
    }

    #[test]
    fn an_unliftable_capsule_is_a_miss() {
        assert!(ray_shape(
            capsule(1.0, 1.0),
            Vec3::new(0.0, f32::INFINITY, 0.0),
            id(),
            &ray(Vec3::ZERO, Vec3::UNIT_X)
        )
        .is_none());
    }

    #[test]
    fn a_ray_meets_a_plane_from_its_empty_side() {
        // Ground plane y = 0, solid below: a downward ray from y = 5 lands on it.
        let hit = ray_shape(
            plane(Vec3::UNIT_Y, 0.0),
            Vec3::ZERO,
            id(),
            &ray(Vec3::new(0.0, 5.0, 0.0), Vec3::new(0.0, -1.0, 0.0)),
        )
        .expect("the downward ray reaches the ground");
        assert!((hit.hit().time() - 5.0).abs() < 1.0e-5);
        approx(hit.hit().point(), Vec3::ZERO);
        approx(hit.hit().normal(), Vec3::UNIT_Y);
        assert!(hit.front_face());
    }

    #[test]
    fn a_ray_parallel_to_a_plane_or_pointing_away_from_it_misses() {
        let ground = plane(Vec3::UNIT_Y, 0.0);
        // Parallel, on the empty side.
        assert!(ray_shape(
            ground,
            Vec3::ZERO,
            id(),
            &ray(Vec3::new(0.0, 5.0, 0.0), Vec3::UNIT_X)
        )
        .is_none());
        // Pointing away from the surface, on the empty side.
        assert!(ray_shape(
            ground,
            Vec3::ZERO,
            id(),
            &ray(Vec3::new(0.0, 5.0, 0.0), Vec3::UNIT_Y)
        )
        .is_none());
    }

    #[test]
    fn a_ray_starting_in_the_solid_half_space_is_already_inside_it() {
        // The plane is a *solid* half-space, so an origin below y = 0 is inside
        // the ground, not in front of it: distance 0, back face.
        let hit = ray_shape(
            plane(Vec3::UNIT_Y, 0.0),
            Vec3::ZERO,
            id(),
            &ray(Vec3::new(0.0, -5.0, 0.0), Vec3::UNIT_X),
        )
        .expect("an origin inside the solid is a hit");
        assert_eq!(hit.hit().time(), 0.0);
        approx(hit.hit().normal(), Vec3::UNIT_Y);
        assert!(!hit.front_face());
    }

    #[test]
    fn a_planes_offset_moves_its_surface() {
        // Plane x = 5 with the empty side at x > 5: a ray from x = 0 heading +X
        // starts *inside* the solid, while one from x = 10 heading -X enters at 5.
        let wall = plane(Vec3::UNIT_X, 5.0);
        let from_inside = ray_shape(wall, Vec3::ZERO, id(), &ray(Vec3::ZERO, Vec3::UNIT_X))
            .expect("x = 0 is on the solid side of x = 5");
        assert_eq!(from_inside.hit().time(), 0.0);
        let entering = ray_shape(
            wall,
            Vec3::ZERO,
            id(),
            &ray(Vec3::new(10.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0)),
        )
        .expect("the ray reaches the surface at x = 5");
        assert!((entering.hit().time() - 5.0).abs() < 1.0e-5);
        assert!(entering.front_face());
    }

    #[test]
    fn a_heightfield_is_never_hit() {
        assert!(ray_shape(
            heightfield(),
            Vec3::ZERO,
            id(),
            &ray(Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, -1.0, 0.0))
        )
        .is_none());
    }

    #[test]
    fn every_shape_kind_has_a_table_entry() {
        // The table is sized by `PhysicsShapeKind::COUNT`, and every kind's index
        // addresses it — the invariant whose absence made a heightfield query
        // panic out of bounds.
        assert_eq!(RAY_TABLE.len(), PhysicsShapeKind::COUNT);
        let shapes = [
            sphere(1.0),
            box_shape(1.0, 1.0, 1.0),
            capsule(1.0, 1.0),
            plane(Vec3::UNIT_Y, 0.0),
            heightfield(),
        ];
        let probe = ray(Vec3::new(0.0, 20.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
        // Dispatching every kind must not panic; four of the five are struck.
        let struck = shapes
            .into_iter()
            .filter(|s| ray_shape(*s, Vec3::ZERO, id(), &probe).is_some())
            .count();
        assert_eq!(struck, 4, "only the heightfield is excluded");
    }

    #[test]
    fn a_pitched_box_and_a_pitched_capsule_are_both_struck_off_their_flat_axis() {
        // Unrotated, a long thin slab and a long thin capsule at the origin are
        // both struck at y ~ 0 by a downward ray at x = 2. Pitched 45 degrees,
        // the same ray meets each one metres away from that plane — the proof
        // that the rotation argument is genuinely used and not discarded.
        let pitch = Quat::from_axis_angle(Vec3::UNIT_Z, FRAC_PI_4).unwrap();
        let down = ray(Vec3::new(2.0, 10.0, 0.0), Vec3::new(0.0, -1.0, 0.0));
        let flat_box = ray_shape(box_shape(4.0, 0.25, 0.25), Vec3::ZERO, id(), &down)
            .expect("the flat slab spans x = 2");
        assert!(flat_box.hit().point().y.abs() < 0.5);
        let tilted_box = ray_shape(box_shape(4.0, 0.25, 0.25), Vec3::ZERO, pitch, &down)
            .expect("the pitched slab still spans x = 2");
        assert!(
            tilted_box.hit().point().y.abs() > 1.0,
            "a pitched slab is struck far off y = 0, got {:?}",
            tilted_box.hit().point()
        );
        let tilted_capsule = ray_shape(capsule(0.25, 4.0), Vec3::ZERO, pitch, &down)
            .expect("the pitched capsule spans x = 2");
        assert!(
            tilted_capsule.hit().point().y.abs() > 1.0,
            "a pitched capsule is struck far off y = 0, got {:?}",
            tilted_capsule.hit().point()
        );
    }
}
