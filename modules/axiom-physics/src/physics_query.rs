//! Deterministic spatial queries over a world's colliders.
//!
//! Both queries are pure reads — they take `&PhysicsWorld` and never mutate it —
//! and both skip disabled bodies/colliders. Results are deterministic functions of
//! world state with explicit tie-breaking and ordering.
//!
//! ## Exact per shape kind (no bounding approximations)
//! Every supported shape is tested against its **true geometry**, not a bounding
//! volume, so a query never reports a hit a tighter test would reject. Dispatch is
//! branchless: each shape `kind().index()` selects an exact per-kind function from
//! a fixed table (`Sphere = 0, Box = 1, Capsule = 2, Plane = 3`), the same
//! dispatch-table idiom the narrow phase uses.
//!
//! | kind    | `raycast`                        | `overlap_sphere`                       |
//! |---------|----------------------------------|----------------------------------------|
//! | Sphere  | exact ray/sphere quadratic       | exact sphere/sphere (centre distance)  |
//! | Box     | exact ray/AABB slab entry        | exact closest-point-on-AABB distance   |
//! | Capsule | **unsupported** — never hit      | **unsupported** — never reported       |
//! | Plane   | analytic ray/plane intersection  | signed distance of the surface         |
//!
//! ### Capsule is explicitly unsupported, not approximated
//! There is no exact closed-form ray/capsule or capsule/sphere test reachable from
//! the primitives this module depends on, so rather than silently fall back to the
//! capsule's AABB / bounding sphere (a false-positive generator), a capsule
//! collider is **excluded** from both queries: a ray never hits it and
//! `overlap_sphere` never reports it. Exact capsule queries are a documented
//! later-phase item (see `ROADMAP.md`), tracked alongside capsule contact
//! generation in the narrow phase.
//!
//! ## `raycast`
//! Returns the **nearest** solid body hit by a ray, or `None`. Among hits within
//! `max_distance`, the smallest entry distance wins, ties broken by the **smaller
//! body handle**. Triggers are **excluded** (a ray reports solid geometry only).
//!
//! ## `overlap_sphere`
//! Returns the bodies whose colliders overlap the query sphere, as a
//! **sorted, de-duplicated** handle list. Triggers are **included** (overlap is a
//! presence query). A non-finite query origin/centre is a deterministic
//! miss/empty.

use axiom_kernel::Meters;
use axiom_math::{Quat, Ray, Vec3};

use crate::collider_bounds::world_aabb;
use crate::physics_body_handle::PhysicsBodyHandle;
use crate::physics_collider_shape::PhysicsColliderShape;
use crate::physics_world::PhysicsWorld;

/// A ray/plane that is closer to parallel than this is treated as a miss.
const PLANE_PARALLEL_EPSILON: f32 = 1.0e-7;

/// A collider resolved against its owning body for querying.
struct Resolved {
    shape: PhysicsColliderShape,
    center: Vec3,
    active: bool,
    is_trigger: bool,
    body: PhysicsBodyHandle,
}

/// A read-only spatial query over a world.
pub(crate) struct PhysicsQuery<'a> {
    world: &'a PhysicsWorld,
}

impl<'a> PhysicsQuery<'a> {
    /// Begin querying `world`.
    pub(crate) fn new(world: &'a PhysicsWorld) -> Self {
        PhysicsQuery { world }
    }

    /// Resolve every collider against its owning body. A collider always
    /// references a live body, so `find` always matches.
    fn resolved(&self) -> Vec<Resolved> {
        self.world
            .colliders()
            .iter()
            .filter_map(|c| {
                self.world
                    .bodies()
                    .iter()
                    .find(|b| b.handle() == c.body())
                    .map(|b| Resolved {
                        shape: c.shape(),
                        center: b.transform().translation,
                        active: c.enabled() & b.enabled(),
                        is_trigger: c.is_trigger(),
                        body: c.body(),
                    })
            })
            .collect()
    }

    /// Cast a ray and return the nearest solid body hit within `max_distance`.
    /// A non-finite origin (like a zero-length/non-finite direction) is a
    /// deterministic miss.
    pub(crate) fn raycast(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: Meters,
    ) -> Option<PhysicsBodyHandle> {
        let max = max_distance.get();
        vec3_is_finite(origin)
            .then(|| {
                Ray::new(origin, direction).ok().and_then(|ray| {
                    let hits: Vec<(f32, PhysicsBodyHandle)> = self
                        .resolved()
                        .iter()
                        .filter(|r| r.active & !r.is_trigger)
                        .filter_map(|r| ray_distance(r.shape, r.center, &ray).map(|t| (t, r.body)))
                        .filter(|(t, _)| *t <= max)
                        .collect();
                    hits.into_iter()
                        .min_by(|a, b| {
                            a.0.partial_cmp(&b.0)
                                .unwrap_or(core::cmp::Ordering::Equal)
                                .then(a.1.cmp(&b.1))
                        })
                        .map(|(_, handle)| handle)
                })
            })
            .flatten()
    }

    /// Find the bodies overlapping a query sphere, as sorted unique handles. A
    /// non-finite query centre returns a deterministic empty list.
    pub(crate) fn overlap_sphere(&self, center: Vec3, radius: Meters) -> Vec<PhysicsBodyHandle> {
        let r = radius.get();
        vec3_is_finite(center)
            .then(|| {
                let mut handles: Vec<PhysicsBodyHandle> = self
                    .resolved()
                    .iter()
                    .filter(|res| res.active)
                    .filter(|res| overlaps_query(res.shape, res.center, center, r))
                    .map(|res| res.body)
                    .collect();
                handles.sort();
                handles.dedup();
                handles
            })
            .unwrap_or_default()
    }
}

/// `true` iff every component of `v` is finite — a query input screen so a
/// non-finite origin/centre yields a deterministic miss/empty rather than
/// relying on NaN-comparison semantics.
fn vec3_is_finite(v: Vec3) -> bool {
    v.x.is_finite() & v.y.is_finite() & v.z.is_finite()
}

/// The exact ray entry-distance function for one shape kind.
type RayFn = fn(PhysicsColliderShape, Vec3, &Ray) -> Option<f32>;

/// Exact per-kind ray functions, indexed by `kind().index()`
/// (`Sphere = 0, Box = 1, Capsule = 2, Plane = 3`). Capsule has no exact
/// closed-form ray test with the available primitives, so it is excluded
/// (`ray_unsupported` → always a miss) rather than approximated by its AABB.
const RAY_TABLE: [RayFn; 4] = [ray_sphere, ray_box, ray_unsupported, ray_plane];

/// The exact ray entry distance to a collider, or `None` for a miss, dispatched
/// branchlessly on the shape kind.
fn ray_distance(shape: PhysicsColliderShape, center: Vec3, ray: &Ray) -> Option<f32> {
    RAY_TABLE[shape.kind().index()](shape, center, ray)
}

/// Exact ray/sphere intersection. Solves `|origin + t·dir − center|² = r²` for the
/// nearest non-negative root; clamps an origin-inside hit to entry distance `0`
/// (matching `Ray::intersect_aabb_entry`). A negative discriminant (the ray's line
/// misses the sphere) or both roots behind the origin is a miss — so a ray that
/// only clips the sphere's AABB but misses its surface returns `None`.
fn ray_sphere(shape: PhysicsColliderShape, center: Vec3, ray: &Ray) -> Option<f32> {
    let oc = ray.origin().subtract(center);
    let r = shape.radius();
    let b = oc.dot(ray.direction());
    let c = oc.dot(oc) - r * r;
    let discriminant = b * b - c;
    // `max(0.0)` keeps the sqrt finite when the line misses; `hits` is false then.
    let root = discriminant.max(0.0).sqrt();
    let hits = (discriminant >= 0.0) & ((root - b) >= 0.0);
    let entry = (-b - root).max(0.0);
    hits.then_some(entry)
}

/// Exact ray/box intersection for an axis-aligned box: a box *is* its world
/// axis-aligned extents, so the AABB slab entry distance is exact. Queries remain
/// rotation-unaware (they pass the identity rotation) — exact ray/OBB casting is a
/// documented later-phase item (see `ROADMAP.md`), tracked alongside capsule
/// queries; the rotation-aware bound belongs to the broad phase, which must not
/// miss a candidate, whereas a query must not over-report.
fn ray_box(shape: PhysicsColliderShape, center: Vec3, ray: &Ray) -> Option<f32> {
    world_aabb(shape, center, Quat::IDENTITY).and_then(|aabb| ray.intersect_aabb_entry(&aabb))
}

/// A capsule is explicitly unsupported by `raycast` — never a hit.
fn ray_unsupported(_shape: PhysicsColliderShape, _center: Vec3, _ray: &Ray) -> Option<f32> {
    None
}

/// Analytic ray/plane intersection. The plane is the half-space `n · x = offset`;
/// its body centre is irrelevant. A ray within `PLANE_PARALLEL_EPSILON` of
/// parallel, or whose intersection is behind the origin, is a miss.
fn ray_plane(shape: PhysicsColliderShape, _center: Vec3, ray: &Ray) -> Option<f32> {
    let normal = shape.normal();
    let denominator = normal.dot(ray.direction());
    let t = (shape.offset() - normal.dot(ray.origin())) / denominator;
    let hits = (denominator.abs() > PLANE_PARALLEL_EPSILON) & (t >= 0.0);
    hits.then_some(t)
}

/// The exact sphere-overlap function for one shape kind.
type OverlapFn = fn(PhysicsColliderShape, Vec3, Vec3, f32) -> bool;

/// Exact per-kind overlap functions, indexed by `kind().index()`. Capsule has no
/// exact closed-form sphere-overlap test with the available primitives, so it is
/// excluded (`overlap_unsupported` → never reported) rather than approximated by a
/// bounding sphere.
const OVERLAP_TABLE: [OverlapFn; 4] = [
    overlap_sphere_shape,
    overlap_box_shape,
    overlap_unsupported,
    overlap_plane_shape,
];

/// Whether a collider exactly overlaps the query sphere, dispatched branchlessly
/// on the shape kind.
fn overlaps_query(
    shape: PhysicsColliderShape,
    center: Vec3,
    query_center: Vec3,
    query_radius: f32,
) -> bool {
    OVERLAP_TABLE[shape.kind().index()](shape, center, query_center, query_radius)
}

/// Exact sphere/sphere overlap: centre distance ≤ sum of radii (inclusive of
/// touching). Compared squared to avoid a `sqrt`.
fn overlap_sphere_shape(
    shape: PhysicsColliderShape,
    center: Vec3,
    query_center: Vec3,
    query_radius: f32,
) -> bool {
    let sum = shape.radius() + query_radius;
    center.subtract(query_center).length_squared() <= sum * sum
}

/// Exact sphere/AABB overlap: squared distance from the query centre to the
/// closest point on the box ≤ radius².
fn overlap_box_shape(
    shape: PhysicsColliderShape,
    center: Vec3,
    query_center: Vec3,
    query_radius: f32,
) -> bool {
    let he = shape.half_extents();
    let d = query_center.subtract(center);
    let closest = center.add(Vec3::new(
        d.x.clamp(-he.x, he.x),
        d.y.clamp(-he.y, he.y),
        d.z.clamp(-he.z, he.z),
    ));
    query_center.subtract(closest).length_squared() <= query_radius * query_radius
}

/// A capsule is explicitly unsupported by `overlap_sphere` — never reported.
fn overlap_unsupported(
    _shape: PhysicsColliderShape,
    _center: Vec3,
    _query_center: Vec3,
    _query_radius: f32,
) -> bool {
    false
}

/// Exact plane overlap: the absolute signed distance of the half-space surface
/// from the query centre is within the query radius.
fn overlap_plane_shape(
    shape: PhysicsColliderShape,
    _center: Vec3,
    query_center: Vec3,
    query_radius: f32,
) -> bool {
    (shape.normal().dot(query_center) - shape.offset()).abs() <= query_radius
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics_body_desc::PhysicsBodyDesc;
    use crate::physics_collider_handle::PhysicsColliderHandle;
    use crate::physics_config::PhysicsConfig;
    use crate::physics_material::PhysicsMaterial;
    use axiom_math::Transform;

    fn material() -> PhysicsMaterial {
        PhysicsMaterial::new(
            Ratio::new(0.0).unwrap(),
            Ratio::new(0.0).unwrap(),
            Ratio::new(1.0).unwrap(),
        )
        .unwrap()
    }

    use axiom_kernel::Ratio;

    fn world() -> PhysicsWorld {
        PhysicsWorld::new(PhysicsConfig::default_config())
    }

    fn spawn(world: &mut PhysicsWorld, at: Vec3) -> PhysicsBodyHandle {
        world
            .create_body(PhysicsBodyDesc::static_body(Transform::from_translation(at)).unwrap())
            .unwrap()
    }

    fn spawn_dynamic(world: &mut PhysicsWorld, at: Vec3) -> PhysicsBodyHandle {
        world
            .create_body(
                PhysicsBodyDesc::dynamic_body(
                    Transform::from_translation(at),
                    Ratio::new(1.0).unwrap(),
                )
                .unwrap(),
            )
            .unwrap()
    }

    fn sphere(radius: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::sphere(Meters::new(radius).unwrap()).unwrap()
    }

    fn box_shape(x: f32, y: f32, z: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::box_shape(Vec3::new(x, y, z)).unwrap()
    }

    fn capsule() -> PhysicsColliderShape {
        PhysicsColliderShape::capsule(Meters::new(0.5).unwrap(), Meters::new(1.0).unwrap()).unwrap()
    }

    fn plane(normal: Vec3, distance: f32) -> PhysicsColliderShape {
        PhysicsColliderShape::plane(normal, Meters::new(distance).unwrap()).unwrap()
    }

    fn attach(
        world: &mut PhysicsWorld,
        body: PhysicsBodyHandle,
        shape: PhysicsColliderShape,
        trigger: bool,
    ) -> PhysicsColliderHandle {
        world.attach_collider(body, shape, material(), trigger).unwrap()
    }

    fn far() -> Meters {
        Meters::new(100.0).unwrap()
    }

    fn ray_x() -> (Vec3, Vec3) {
        (Vec3::new(-10.0, 0.0, 0.0), Vec3::UNIT_X)
    }

    /// Advance the world by one fixed step (drains queued commands such as
    /// `enqueue_disable` before integration).
    fn step_once(world: &mut PhysicsWorld) {
        use axiom_kernel::{FrameIndex, Tick};
        use axiom_runtime::RuntimeStep;
        world
            .step(RuntimeStep::new(FrameIndex::new(0), Tick::new(0), 16_666_667, 0))
            .unwrap();
    }

    #[test]
    fn raycast_hits_a_sphere_and_misses_empty_space() {
        let mut w = world();
        let b = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, b, sphere(1.0), false);
        let (o, d) = ray_x();
        assert_eq!(PhysicsQuery::new(&w).raycast(o, d, far()), Some(b));
        // A ray pointing away never hits (far root behind origin).
        assert!(PhysicsQuery::new(&w)
            .raycast(Vec3::new(-10.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0), far())
            .is_none());
    }

    #[test]
    fn raycast_hits_sphere_centerline() {
        let mut w = world();
        let b = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, b, sphere(1.0), false);
        // From (-10,0,0) along +X the entry is at x = -1, i.e. distance 9 < 100.
        assert_eq!(PhysicsQuery::new(&w).raycast(Vec3::new(-10.0, 0.0, 0.0), Vec3::UNIT_X, far()), Some(b));
        // Exact: a max_distance of 9 still reaches the entry (9 <= 9), 8 does not.
        assert_eq!(PhysicsQuery::new(&w).raycast(Vec3::new(-10.0, 0.0, 0.0), Vec3::UNIT_X, Meters::new(9.0).unwrap()), Some(b));
        assert!(PhysicsQuery::new(&w).raycast(Vec3::new(-10.0, 0.0, 0.0), Vec3::UNIT_X, Meters::new(8.0).unwrap()).is_none());
        // Origin inside the sphere clamps the entry distance to 0 (still a hit).
        let mut wi = world();
        let bi = spawn(&mut wi, Vec3::ZERO);
        attach(&mut wi, bi, sphere(2.0), false);
        assert_eq!(PhysicsQuery::new(&wi).raycast(Vec3::ZERO, Vec3::UNIT_X, Meters::new(0.0).unwrap()), Some(bi));
    }

    #[test]
    fn raycast_misses_a_sphere_it_only_clips_the_aabb_of() {
        // A unit sphere at the origin has AABB [-1,1]^3. A ray at y = z = 0.9
        // travels through that AABB (0.9 is within [-1,1] on both axes) yet its
        // perpendicular distance to the centre is sqrt(0.9^2 + 0.9^2) ~ 1.273 > 1,
        // so it misses the sphere surface. The old AABB-bounded raycast reported a
        // false hit here; the exact ray/sphere test must return None.
        let mut w = world();
        let s = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, s, sphere(1.0), false);
        assert_eq!(
            PhysicsQuery::new(&w).raycast(Vec3::new(-10.0, 0.9, 0.9), Vec3::UNIT_X, far()),
            None
        );
        // Proof the AABB really is clipped: a box of the same extents IS hit by the
        // identical ray (this is exactly the over-report the sphere test removes).
        let mut wb = world();
        let b = spawn(&mut wb, Vec3::ZERO);
        attach(&mut wb, b, box_shape(1.0, 1.0, 1.0), false);
        assert_eq!(
            PhysicsQuery::new(&wb).raycast(Vec3::new(-10.0, 0.9, 0.9), Vec3::UNIT_X, far()),
            Some(b)
        );
    }

    #[test]
    fn raycast_sphere_nearest_hit_wins() {
        let mut w = world();
        let near = spawn(&mut w, Vec3::new(0.0, 0.0, 0.0));
        attach(&mut w, near, sphere(1.0), false);
        let far_body = spawn(&mut w, Vec3::new(5.0, 0.0, 0.0));
        attach(&mut w, far_body, sphere(1.0), false);
        let (o, d) = ray_x();
        // near entry at x = -1 (dist 9), far entry at x = 4 (dist 14): near wins.
        assert_eq!(PhysicsQuery::new(&w).raycast(o, d, far()), Some(near));
        assert!(near < far_body);
    }

    #[test]
    fn raycast_sphere_equal_distance_tie_breaks_by_handle() {
        // Two unit spheres at (0, ±0.5, 0): a +X ray along y = 0 enters each at the
        // identical distance (sqrt(0.75) off-axis), so the smaller handle wins.
        let mut w = world();
        let first = spawn(&mut w, Vec3::new(0.0, 0.5, 0.0));
        attach(&mut w, first, sphere(1.0), false);
        let second = spawn(&mut w, Vec3::new(0.0, -0.5, 0.0));
        attach(&mut w, second, sphere(1.0), false);
        let (o, d) = ray_x();
        assert_eq!(PhysicsQuery::new(&w).raycast(o, d, far()), Some(first));
        assert!(first < second);
    }

    #[test]
    fn raycast_hits_a_box_and_a_plane() {
        let mut w = world();
        let b = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, b, box_shape(1.0, 1.0, 1.0), false);
        let (o, d) = ray_x();
        assert_eq!(PhysicsQuery::new(&w).raycast(o, d, far()), Some(b));
        // A ray that clears the box entirely misses (covers the box None arm).
        assert!(PhysicsQuery::new(&w)
            .raycast(Vec3::new(-10.0, 5.0, 0.0), Vec3::UNIT_X, far())
            .is_none());

        let mut wp = world();
        let p = spawn(&mut wp, Vec3::ZERO);
        attach(&mut wp, p, plane(Vec3::UNIT_X, 0.0), false);
        assert_eq!(PhysicsQuery::new(&wp).raycast(o, d, far()), Some(p));
        // Parallel ray (along the plane) misses.
        assert!(PhysicsQuery::new(&wp)
            .raycast(Vec3::new(0.0, 5.0, 0.0), Vec3::UNIT_Y, far())
            .is_none());
        // A plane whose intersection is behind the origin misses (t < 0 arm):
        // plane x = -5, ray from origin heading +X never reaches x = -5.
        let mut wb = world();
        let pb = spawn(&mut wb, Vec3::ZERO);
        attach(&mut wb, pb, plane(Vec3::UNIT_X, -5.0), false);
        assert!(PhysicsQuery::new(&wb)
            .raycast(Vec3::ZERO, Vec3::UNIT_X, far())
            .is_none());
    }

    #[test]
    fn raycast_does_not_hit_a_capsule_unsupported_shape() {
        // Capsule is explicitly excluded from raycast (documented unsupported):
        // a capsule sitting squarely on the ray axis is never hit.
        let mut w = world();
        let c = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, c, capsule(), false);
        let (o, d) = ray_x();
        assert_eq!(PhysicsQuery::new(&w).raycast(o, d, far()), None);
    }

    #[test]
    fn raycast_respects_max_distance() {
        let mut w = world();
        let b = spawn(&mut w, Vec3::new(50.0, 0.0, 0.0));
        attach(&mut w, b, sphere(1.0), false);
        let (o, d) = ray_x();
        // Entry at x = 49 (distance 59): a max of 5 rejects it, 100 accepts it.
        assert!(PhysicsQuery::new(&w).raycast(o, d, Meters::new(5.0).unwrap()).is_none());
        assert_eq!(PhysicsQuery::new(&w).raycast(o, d, far()), Some(b));
    }

    #[test]
    fn trigger_policy_is_explicitly_tested() {
        let mut w = world();
        let b = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, b, sphere(1.0), true); // trigger
        let (o, d) = ray_x();
        assert_eq!(PhysicsQuery::new(&w).raycast(o, d, far()), None, "raycast excludes triggers");
        assert_eq!(
            PhysicsQuery::new(&w).overlap_sphere(Vec3::ZERO, Meters::new(0.5).unwrap()),
            vec![b],
            "overlap includes triggers"
        );
    }

    #[test]
    fn disabled_bodies_and_colliders_are_skipped_by_queries() {
        let mut w = world();
        let b = spawn_dynamic(&mut w, Vec3::ZERO);
        attach(&mut w, b, sphere(1.0), false);
        w.enqueue_disable(b).unwrap();
        step_once(&mut w);
        let (o, d) = ray_x();
        assert_eq!(PhysicsQuery::new(&w).raycast(o, d, far()), None);
        assert!(PhysicsQuery::new(&w)
            .overlap_sphere(Vec3::ZERO, Meters::new(0.5).unwrap())
            .is_empty());
    }

    #[test]
    fn raycast_skips_a_bad_ray() {
        // A zero-length direction is an invalid ray -> no hit.
        let mut w = world();
        let b = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, b, sphere(1.0), false);
        let (o, _) = ray_x();
        assert!(PhysicsQuery::new(&w).raycast(o, Vec3::ZERO, far()).is_none());
    }

    #[test]
    fn non_finite_query_inputs_are_a_deterministic_miss_and_empty() {
        let mut w = world();
        let b = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, b, sphere(1.0), false);
        let q = PhysicsQuery::new(&w);
        let nan = Vec3::new(f32::NAN, 0.0, 0.0);
        assert!(q.raycast(nan, Vec3::UNIT_X, far()).is_none());
        assert!(q.overlap_sphere(nan, far()).is_empty());
    }

    #[test]
    fn overlap_sphere_finds_spheres_boxes_and_planes() {
        let mut w = world();
        let s = spawn(&mut w, Vec3::new(0.0, 0.0, 0.0));
        attach(&mut w, s, sphere(1.0), false);
        let bx = spawn(&mut w, Vec3::new(1.0, 0.0, 0.0));
        attach(&mut w, bx, box_shape(1.0, 1.0, 1.0), false);
        let pl = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, pl, plane(Vec3::UNIT_Y, 0.0), false);

        let hits = PhysicsQuery::new(&w).overlap_sphere(Vec3::ZERO, Meters::new(0.5).unwrap());
        // Sorted, unique body handles; all three overlap the origin query.
        assert_eq!(hits, vec![s, bx, pl]);
    }

    #[test]
    fn overlap_sphere_reports_exact_sphere_overlap() {
        // Unit sphere at (1.5,0,0), unit query at origin: centres 1.5 apart, radii
        // sum to 2.0 > 1.5 -> overlap. Exact sphere/sphere.
        let mut w = world();
        let b = spawn(&mut w, Vec3::new(1.5, 0.0, 0.0));
        attach(&mut w, b, sphere(1.0), false);
        assert_eq!(
            PhysicsQuery::new(&w).overlap_sphere(Vec3::ZERO, Meters::new(1.0).unwrap()),
            vec![b]
        );
    }

    #[test]
    fn overlap_sphere_does_not_report_sphere_outside_exact_radius() {
        // Unit sphere at (2.5,0,0), unit query at origin: centres 2.5 apart, radii
        // sum to 2.0 < 2.5 -> NO overlap. The old sqrt(3)-inflated bounding sphere
        // (half_extents.length() = 1.732) gave 1.732 + 1.0 = 2.732 >= 2.5 and
        // falsely reported it.
        let mut w = world();
        let b = spawn(&mut w, Vec3::new(2.5, 0.0, 0.0));
        attach(&mut w, b, sphere(1.0), false);
        assert!(PhysicsQuery::new(&w)
            .overlap_sphere(Vec3::ZERO, Meters::new(1.0).unwrap())
            .is_empty());
    }

    #[test]
    fn overlap_sphere_box_uses_closest_point_not_bounding_sphere() {
        // Box of half-extents (1,1,1) at the origin (corner at (1,1,1)). A query at
        // (2.2, 2.2, 2.2) radius 1.0 is at distance sqrt(3*1.2^2) ~ 2.078 from the
        // nearest corner (1,1,1) > 1.0 -> no overlap. A query at (1.5,1.5,1.5)
        // radius 1.0 is sqrt(3*0.5^2) ~ 0.866 from the corner < 1.0 -> overlap.
        let mut w = world();
        let b = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, b, box_shape(1.0, 1.0, 1.0), false);
        assert!(PhysicsQuery::new(&w)
            .overlap_sphere(Vec3::new(2.2, 2.2, 2.2), Meters::new(1.0).unwrap())
            .is_empty());
        assert_eq!(
            PhysicsQuery::new(&w).overlap_sphere(Vec3::new(1.5, 1.5, 1.5), Meters::new(1.0).unwrap()),
            vec![b]
        );
    }

    #[test]
    fn overlap_sphere_plane_uses_signed_distance() {
        // Plane y = 0. A query 5 units above with radius 1 does not reach it; a
        // query 0.5 above with radius 1 does.
        let mut w = world();
        let p = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, p, plane(Vec3::UNIT_Y, 0.0), false);
        assert!(PhysicsQuery::new(&w)
            .overlap_sphere(Vec3::new(0.0, 5.0, 0.0), Meters::new(1.0).unwrap())
            .is_empty());
        assert_eq!(
            PhysicsQuery::new(&w).overlap_sphere(Vec3::new(0.0, 0.5, 0.0), Meters::new(1.0).unwrap()),
            vec![p]
        );
    }

    #[test]
    fn overlap_sphere_does_not_report_a_capsule_unsupported_shape() {
        // Capsule is explicitly excluded from overlap (documented unsupported):
        // even a coincident query never reports it.
        let mut w = world();
        let c = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, c, capsule(), false);
        assert!(PhysicsQuery::new(&w)
            .overlap_sphere(Vec3::ZERO, far())
            .is_empty());
    }

    #[test]
    fn overlap_sphere_results_are_sorted() {
        // Spawned out of handle order to confirm the result is ascending by handle.
        let mut w = world();
        let a = spawn(&mut w, Vec3::new(0.3, 0.0, 0.0));
        attach(&mut w, a, sphere(1.0), false);
        let b = spawn(&mut w, Vec3::new(-0.3, 0.0, 0.0));
        attach(&mut w, b, sphere(1.0), false);
        let c = spawn(&mut w, Vec3::new(0.0, 0.3, 0.0));
        attach(&mut w, c, sphere(1.0), false);
        let hits = PhysicsQuery::new(&w).overlap_sphere(Vec3::ZERO, Meters::new(0.5).unwrap());
        assert_eq!(hits, vec![a, b, c]);
        assert!(a < b && b < c);
    }

    #[test]
    fn overlap_sphere_excludes_distant_colliders() {
        let mut w = world();
        let near = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, near, sphere(1.0), false);
        let far_body = spawn(&mut w, Vec3::new(100.0, 0.0, 0.0));
        attach(&mut w, far_body, sphere(1.0), false);
        assert_eq!(
            PhysicsQuery::new(&w).overlap_sphere(Vec3::ZERO, Meters::new(0.5).unwrap()),
            vec![near]
        );
    }

    #[test]
    fn overlap_sphere_dedups_multiple_colliders_on_one_body_and_includes_triggers() {
        let mut w = world();
        let b = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, b, sphere(1.0), false);
        attach(&mut w, b, sphere(1.0), true); // trigger included; same body deduped
        let hits = PhysicsQuery::new(&w).overlap_sphere(Vec3::ZERO, Meters::new(0.5).unwrap());
        assert_eq!(hits, vec![b]);
    }

    #[test]
    fn queries_do_not_change_snapshot() {
        // Both queries are pure reads: the world snapshot is byte-identical after.
        let mut w = world();
        let b = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, b, sphere(1.0), false);
        let before = w.snapshot();
        let q = PhysicsQuery::new(&w);
        let _ = q.raycast(Vec3::new(-10.0, 0.0, 0.0), Vec3::UNIT_X, far());
        let _ = q.overlap_sphere(Vec3::ZERO, far());
        assert_eq!(before, w.snapshot(), "queries must not mutate world state");
    }
}
