//! Deterministic spatial queries over a world's colliders.
//!
//! Every query here is a pure read — it takes `&PhysicsWorld` and never mutates
//! it — and every one skips disabled bodies and disabled colliders. Results are
//! deterministic functions of world state with explicit tie-breaking and
//! ordering.
//!
//! This file owns the *orchestration*: resolving colliders against their bodies,
//! screening inputs, ordering results and tagging them with identity. The exact
//! per-shape geometry lives beside it, one relation per file —
//! [`crate::query_ray`] (casts), [`crate::query_overlap`] (discrete overlap) and
//! [`crate::query_sweep`] (shape casts) — each dispatching branchlessly through a
//! table sized by `PhysicsShapeKind::COUNT`.
//!
//! ## Rotation-aware, exactly
//! A collider is resolved with its owning body's **rotation** as well as its
//! position, so a turned box is queried on its true tilted faces and a tipped
//! capsule on its true shaft. (The earlier queries passed the identity rotation
//! and documented the gap; there is no gap now.)
//!
//! ## The four queries
//! - **`raycast`** — the nearest solid hit within `max_distance`, as a
//!   [`PhysicsHit`]. Triggers are excluded: a ray reports solid geometry only.
//! - **`raycast_all`** — *every* hit along the ray, nearest first. This is what
//!   a projectile that penetrates several surfaces needs; collapsing to the
//!   nearest hit throws away the rest of the trace, and re-casting from just past
//!   each impact both costs another full query and depends on an epsilon nudge.
//! - **`overlap_capsule`** (and `overlap_sphere`, its zero-length special case) —
//!   the bodies whose colliders overlap a query volume, as a sorted, deduplicated
//!   handle list. Triggers are **included**: overlap is a presence query.
//! - **`capsule_cast`** — the nearest contact a capsule meets while travelling a
//!   motion vector. Triggers are excluded, like a ray.
//!
//! ## Ordering
//! Nearest first; ties broken by the smaller **body** handle, then by the smaller
//! **collider** handle. Both keys are needed: one body may carry several
//! colliders, and `raycast_all` can return more than one hit on the same body.

use axiom_kernel::Meters;
use axiom_math::{Capsule, Quat, Ray, Segment, Vec3};

use crate::physics_body_handle::PhysicsBodyHandle;
use crate::physics_collider_handle::PhysicsColliderHandle;
use crate::physics_collider_shape::PhysicsColliderShape;
use crate::physics_hit::PhysicsHit;
use crate::physics_world::PhysicsWorld;
use crate::query_hit::QueryHit;
use crate::query_overlap::overlaps_capsule;
use crate::query_ray::ray_shape;
use crate::query_sweep::sweep_shape;

/// A collider resolved against its owning body for querying.
struct Resolved {
    shape: PhysicsColliderShape,
    center: Vec3,
    rotation: Quat,
    active: bool,
    is_trigger: bool,
    body: PhysicsBodyHandle,
    collider: PhysicsColliderHandle,
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
                        rotation: b.transform().rotation,
                        active: c.enabled() & b.enabled(),
                        is_trigger: c.is_trigger(),
                        body: c.body(),
                        collider: c.handle(),
                    })
            })
            .collect()
    }

    /// Every solid hit along a ray within `max_distance`, nearest first. A
    /// non-finite origin or a zero-length/non-finite direction is a deterministic
    /// empty result.
    pub(crate) fn raycast_all(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: Meters,
    ) -> Vec<PhysicsHit> {
        let max = max_distance.get();
        Ray::new(origin, direction)
            .ok()
            .map(|ray| {
                let mut hits: Vec<PhysicsHit> = self
                    .resolved()
                    .iter()
                    .filter(|r| r.active & !r.is_trigger)
                    .filter_map(|r| {
                        ray_shape(r.shape, r.center, r.rotation, &ray)
                            .filter(|found| found.hit().time() <= max)
                            .map(|found| tag(r, &found, 1.0))
                    })
                    .collect();
                hits.sort_by(nearest_first);
                hits
            })
            .unwrap_or_default()
    }

    /// The nearest solid hit along a ray within `max_distance`, or `None`.
    pub(crate) fn raycast(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: Meters,
    ) -> Option<PhysicsHit> {
        self.raycast_all(origin, direction, max_distance)
            .into_iter()
            .next()
    }

    /// The bodies overlapping a query sphere, as sorted unique handles — the
    /// zero-length case of [`PhysicsQuery::overlap_capsule`].
    pub(crate) fn overlap_sphere(&self, center: Vec3, radius: Meters) -> Vec<PhysicsBodyHandle> {
        self.overlap_capsule(center, Quat::IDENTITY, radius, Meters::finite_or_zero(0.0))
    }

    /// The bodies overlapping a query capsule (axis along the rotated local Y),
    /// as sorted unique handles. A non-finite centre or a negative radius returns
    /// a deterministic empty list.
    pub(crate) fn overlap_capsule(
        &self,
        center: Vec3,
        rotation: Quat,
        radius: Meters,
        half_height: Meters,
    ) -> Vec<PhysicsBodyHandle> {
        query_capsule(center, rotation, radius, half_height)
            .map(|query| {
                let mut handles: Vec<PhysicsBodyHandle> = self
                    .resolved()
                    .iter()
                    .filter(|r| r.active)
                    .filter(|r| overlaps_capsule(r.shape, r.center, r.rotation, &query))
                    .map(|r| r.body)
                    .collect();
                handles.sort();
                handles.dedup();
                handles
            })
            .unwrap_or_default()
    }

    /// The nearest solid contact a query capsule meets while travelling `motion`,
    /// or `None`. Triggers are excluded, as for a ray. A non-finite centre,
    /// negative radius or non-finite motion is a deterministic miss.
    pub(crate) fn capsule_cast(
        &self,
        center: Vec3,
        rotation: Quat,
        radius: Meters,
        half_height: Meters,
        motion: Vec3,
    ) -> Option<PhysicsHit> {
        let travel = motion.length();
        query_capsule(center, rotation, radius, half_height)
            .filter(|_| travel.is_finite())
            .and_then(|query| {
                let mut hits: Vec<PhysicsHit> = self
                    .resolved()
                    .iter()
                    .filter(|r| r.active & !r.is_trigger)
                    .filter_map(|r| {
                        sweep_shape(r.shape, r.center, r.rotation, &query, motion)
                            .map(|found| tag(r, &found, travel))
                    })
                    .collect();
                hits.sort_by(nearest_first);
                hits.into_iter().next()
            })
    }
}

/// The query volume for an overlap or a shape cast: a capsule whose axis runs
/// along the rotated local Y, exactly as a capsule *collider*'s does, so a query
/// and a collider of the same dimensions describe the same volume. `None` for
/// geometry the math layer refuses — a non-finite centre, or a negative radius.
fn query_capsule(
    center: Vec3,
    rotation: Quat,
    radius: Meters,
    half_height: Meters,
) -> Option<Capsule> {
    let up = rotation.rotate(Vec3::new(0.0, half_height.get(), 0.0));
    Segment::new(center.subtract(up), center.add(up))
        .and_then(|axis| Capsule::new(axis, radius.get()))
        .ok()
}

/// Tag a per-shape result with the collider and body it belongs to, converting
/// the query's own time parameter into metres travelled: `scale` is `1` for a ray
/// (whose time already *is* a distance, its direction being unit-length) and the
/// motion's length for a sweep (whose time is a fraction of it).
fn tag(resolved: &Resolved, found: &QueryHit, scale: f32) -> PhysicsHit {
    PhysicsHit::new(
        resolved.body,
        resolved.collider,
        Meters::finite_or_zero(found.hit().time() * scale),
        found.hit().point(),
        found.hit().normal(),
        found.front_face(),
    )
}

/// Nearest first, ties broken by the smaller body handle and then the smaller
/// collider handle — a total, deterministic order that does not depend on
/// collider insertion order.
fn nearest_first(a: &PhysicsHit, b: &PhysicsHit) -> core::cmp::Ordering {
    a.distance()
        .get()
        .partial_cmp(&b.distance().get())
        .unwrap_or(core::cmp::Ordering::Equal)
        .then(a.body().cmp(&b.body()))
        .then(a.collider().cmp(&b.collider()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics_body_desc::PhysicsBodyDesc;
    use crate::physics_config::PhysicsConfig;
    use crate::physics_material::PhysicsMaterial;
    use crate::physics_shape_kind::PhysicsShapeKind;
    use axiom_kernel::Ratio;
    use axiom_math::Transform;
    use core::f32::consts::FRAC_PI_2;

    fn material() -> PhysicsMaterial {
        PhysicsMaterial::new(
            Ratio::new(0.0).unwrap(),
            Ratio::new(0.0).unwrap(),
            Ratio::new(1.0).unwrap(),
        )
        .unwrap()
    }

    fn world() -> PhysicsWorld {
        PhysicsWorld::new(PhysicsConfig::default_config())
    }

    fn spawn(world: &mut PhysicsWorld, at: Vec3) -> PhysicsBodyHandle {
        world
            .create_body(PhysicsBodyDesc::static_body(Transform::from_translation(at)).unwrap())
            .unwrap()
    }

    fn spawn_rotated(world: &mut PhysicsWorld, at: Vec3, rotation: Quat) -> PhysicsBodyHandle {
        let mut transform = Transform::from_translation(at);
        transform.rotation = rotation;
        world
            .create_body(PhysicsBodyDesc::static_body(transform).unwrap())
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
        world
            .attach_collider(body, shape, material(), trigger)
            .unwrap()
    }

    fn attach_flat_heightfield(world: &mut PhysicsWorld, body: PhysicsBodyHandle) {
        let grid = crate::physics_heightfield::Heightfield::new(3, 3, 1.0, 1.0, vec![0.0; 9]);
        let shape = PhysicsColliderShape::heightfield_shape(grid.half_extents()).unwrap();
        world
            .attach_heightfield_collider(body, shape, material(), false, grid)
            .unwrap();
    }

    fn far() -> Meters {
        Meters::new(100.0).unwrap()
    }

    fn meters(v: f32) -> Meters {
        Meters::new(v).unwrap()
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
            .step(RuntimeStep::new(
                FrameIndex::new(0),
                Tick::new(0),
                16_666_667,
                0,
            ))
            .unwrap();
    }

    #[test]
    fn a_world_holding_a_heightfield_collider_can_be_queried_without_panicking() {
        // The regression this file exists to prevent: `Heightfield` is the fifth
        // shape kind, and the ray and overlap tables used to have four entries.
        // Every query below indexed past the end of them and panicked.
        let mut w = world();
        let ground = spawn(&mut w, Vec3::ZERO);
        attach_flat_heightfield(&mut w, ground);
        let ball = spawn(&mut w, Vec3::new(0.0, 5.0, 0.0));
        attach(&mut w, ball, sphere(1.0), false);

        let q = PhysicsQuery::new(&w);
        let down = q.raycast(Vec3::new(0.0, 20.0, 0.0), Vec3::new(0.0, -1.0, 0.0), far());
        assert_eq!(
            down.map(|h| h.body()),
            Some(ball),
            "the sphere is hit; the heightfield is excluded, not a panic"
        );
        assert_eq!(
            q.raycast_all(Vec3::new(0.0, 20.0, 0.0), Vec3::new(0.0, -1.0, 0.0), far())
                .len(),
            1
        );
        assert_eq!(q.overlap_sphere(Vec3::ZERO, far()), vec![ball]);
        assert_eq!(
            q.overlap_capsule(Vec3::ZERO, Quat::IDENTITY, meters(1.0), meters(10.0)),
            vec![ball]
        );
        assert!(q
            .capsule_cast(
                Vec3::new(0.0, 20.0, 0.0),
                Quat::IDENTITY,
                meters(0.5),
                meters(1.0),
                Vec3::new(0.0, -30.0, 0.0)
            )
            .is_some());
    }

    #[test]
    fn raycast_reports_the_struck_geometry_not_just_the_body() {
        let mut w = world();
        let b = spawn(&mut w, Vec3::ZERO);
        let c = attach(&mut w, b, sphere(1.0), false);
        let (o, d) = ray_x();
        let hit = PhysicsQuery::new(&w)
            .raycast(o, d, far())
            .expect("the ray strikes the sphere");
        assert_eq!(hit.body(), b);
        assert_eq!(hit.collider(), c);
        assert!((hit.distance().get() - 9.0).abs() < 1.0e-5);
        assert!(hit.point().subtract(Vec3::new(-1.0, 0.0, 0.0)).length() < 1.0e-5);
        assert!(hit.normal().subtract(Vec3::new(-1.0, 0.0, 0.0)).length() < 1.0e-5);
        assert!(hit.front_face());
    }

    #[test]
    fn a_ray_beginning_inside_geometry_reports_a_back_face_at_zero() {
        let mut w = world();
        let b = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, b, sphere(2.0), false);
        let hit = PhysicsQuery::new(&w)
            .raycast(Vec3::ZERO, Vec3::UNIT_X, meters(0.0))
            .expect("an origin inside the sphere is a hit");
        assert_eq!(hit.distance().get(), 0.0);
        assert!(!hit.front_face());
    }

    #[test]
    fn raycast_all_returns_every_layer_the_ray_passes_through_nearest_first() {
        // Three walls in the ray's path. `raycast` collapses to the first; a
        // bullet that penetrates needs all three, in order.
        let mut w = world();
        let handles: Vec<PhysicsBodyHandle> = [1.0_f32, 4.0, 7.0]
            .into_iter()
            .map(|x| {
                let b = spawn(&mut w, Vec3::new(x, 0.0, 0.0));
                attach(&mut w, b, box_shape(0.25, 2.0, 2.0), false);
                b
            })
            .collect();
        let q = PhysicsQuery::new(&w);
        let all = q.raycast_all(Vec3::new(-5.0, 0.0, 0.0), Vec3::UNIT_X, far());
        assert_eq!(all.len(), 3);
        assert_eq!(
            all.iter().map(|h| h.body()).collect::<Vec<_>>(),
            handles,
            "hits must be ordered nearest first"
        );
        assert!(all
            .windows(2)
            .all(|p| p[0].distance().get() < p[1].distance().get()));
        assert!(all.iter().all(|h| h.front_face()));
        assert_eq!(
            q.raycast(Vec3::new(-5.0, 0.0, 0.0), Vec3::UNIT_X, far())
                .map(|h| h.body()),
            Some(handles[0]),
            "raycast is raycast_all's first result"
        );
    }

    #[test]
    fn raycast_all_reports_each_collider_of_a_multi_collider_body_separately() {
        let mut w = world();
        let b = spawn(&mut w, Vec3::ZERO);
        let inner = attach(&mut w, b, sphere(1.0), false);
        let outer = attach(&mut w, b, sphere(3.0), false);
        let all = PhysicsQuery::new(&w).raycast_all(Vec3::new(-10.0, 0.0, 0.0), Vec3::UNIT_X, far());
        assert_eq!(all.len(), 2);
        // Nearest first: the outer shell is entered before the inner one.
        assert_eq!(all[0].collider(), outer);
        assert_eq!(all[1].collider(), inner);
    }

    #[test]
    fn raycast_respects_max_distance_and_excludes_triggers() {
        let mut w = world();
        let solid = spawn(&mut w, Vec3::new(50.0, 0.0, 0.0));
        attach(&mut w, solid, sphere(1.0), false);
        let (o, d) = ray_x();
        // Entry at x = 49 (distance 59): a max of 5 rejects it, 100 accepts it.
        assert!(PhysicsQuery::new(&w).raycast(o, d, meters(5.0)).is_none());
        assert_eq!(
            PhysicsQuery::new(&w).raycast(o, d, far()).map(|h| h.body()),
            Some(solid)
        );

        let mut wt = world();
        let trigger = spawn(&mut wt, Vec3::ZERO);
        attach(&mut wt, trigger, sphere(1.0), true);
        assert!(
            PhysicsQuery::new(&wt).raycast(o, d, far()).is_none(),
            "raycast excludes triggers"
        );
        assert_eq!(
            PhysicsQuery::new(&wt).overlap_sphere(Vec3::ZERO, meters(0.5)),
            vec![trigger],
            "overlap includes triggers"
        );
    }

    #[test]
    fn equal_distance_hits_tie_break_by_body_then_collider_handle() {
        // Two unit spheres at (0, +/-0.5, 0): a +X ray along y = 0 enters each at
        // the identical distance, so the smaller handle wins.
        let mut w = world();
        let first = spawn(&mut w, Vec3::new(0.0, 0.5, 0.0));
        attach(&mut w, first, sphere(1.0), false);
        let second = spawn(&mut w, Vec3::new(0.0, -0.5, 0.0));
        attach(&mut w, second, sphere(1.0), false);
        let (o, d) = ray_x();
        assert!(first < second);
        assert_eq!(
            PhysicsQuery::new(&w).raycast(o, d, far()).map(|h| h.body()),
            Some(first)
        );

        // Same body, two identical colliders: the collider handle breaks the tie.
        let mut wc = world();
        let body = spawn(&mut wc, Vec3::ZERO);
        let low = attach(&mut wc, body, sphere(1.0), false);
        let high = attach(&mut wc, body, sphere(1.0), false);
        assert!(low < high);
        let all = PhysicsQuery::new(&wc).raycast_all(o, d, far());
        assert_eq!(all.iter().map(|h| h.collider()).collect::<Vec<_>>(), vec![low, high]);
    }

    #[test]
    fn a_rotated_box_collider_is_queried_on_its_real_faces() {
        // A slab yawed a quarter turn about Y reaches |z| = 4 and only |x| = 1.
        let yaw = Quat::from_axis_angle(Vec3::UNIT_Y, FRAC_PI_2).unwrap();
        let mut w = world();
        let b = spawn_rotated(&mut w, Vec3::ZERO, yaw);
        attach(&mut w, b, box_shape(4.0, 1.0, 1.0), false);
        let q = PhysicsQuery::new(&w);
        assert!(
            q.raycast(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -1.0), far())
                .is_some(),
            "the turned slab reaches z = 4"
        );
        assert!(
            q.raycast(Vec3::new(3.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -1.0), far())
                .is_none(),
            "and only reaches x = 1"
        );
        assert_eq!(q.overlap_sphere(Vec3::new(0.0, 0.0, 3.5), meters(0.25)), vec![b]);
        assert!(q
            .overlap_sphere(Vec3::new(3.5, 0.0, 0.0), meters(0.25))
            .is_empty());
    }

    #[test]
    fn capsule_colliders_are_hit_by_rays_and_reported_by_overlap() {
        // Both used to be documented as unsupported: a ray never hit a capsule
        // and overlap never reported one.
        let mut w = world();
        let b = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, b, capsule(), false);
        let (o, d) = ray_x();
        let q = PhysicsQuery::new(&w);
        let hit = q.raycast(o, d, far()).expect("a capsule is hit now");
        assert_eq!(hit.body(), b);
        assert!((hit.distance().get() - 9.5).abs() < 1.0e-4);
        assert_eq!(q.overlap_sphere(Vec3::ZERO, meters(0.1)), vec![b]);
    }

    #[test]
    fn overlap_capsule_reaches_along_its_axis_where_a_sphere_cannot() {
        // A body 4 units up: a unit query sphere at the origin misses it, but a
        // query capsule reaching up to it does not.
        let mut w = world();
        let b = spawn(&mut w, Vec3::new(0.0, 4.0, 0.0));
        attach(&mut w, b, sphere(0.5), false);
        let q = PhysicsQuery::new(&w);
        assert!(q.overlap_sphere(Vec3::ZERO, meters(1.0)).is_empty());
        assert_eq!(
            q.overlap_capsule(Vec3::ZERO, Quat::IDENTITY, meters(1.0), meters(3.0)),
            vec![b]
        );
        // Rotated flat, the same query capsule reaches along X instead and no
        // longer reaches the body above.
        let flat = Quat::from_axis_angle(Vec3::UNIT_Z, FRAC_PI_2).unwrap();
        assert!(q
            .overlap_capsule(Vec3::ZERO, flat, meters(1.0), meters(3.0))
            .is_empty());
    }

    #[test]
    fn overlap_results_are_sorted_deduplicated_and_skip_disabled_bodies() {
        let mut w = world();
        let a = spawn(&mut w, Vec3::new(0.3, 0.0, 0.0));
        attach(&mut w, a, sphere(1.0), false);
        attach(&mut w, a, sphere(1.0), true); // same body, deduplicated
        let b = spawn(&mut w, Vec3::new(-0.3, 0.0, 0.0));
        attach(&mut w, b, sphere(1.0), false);
        assert_eq!(
            PhysicsQuery::new(&w).overlap_sphere(Vec3::ZERO, meters(0.5)),
            vec![a, b]
        );
        assert!(a < b);

        // A disabled body drops out of every query.
        let mut wd = world();
        let d = spawn_dynamic(&mut wd, Vec3::ZERO);
        attach(&mut wd, d, sphere(1.0), false);
        wd.enqueue_disable(d).unwrap();
        step_once(&mut wd);
        let (o, dir) = ray_x();
        let q = PhysicsQuery::new(&wd);
        assert!(q.raycast(o, dir, far()).is_none());
        assert!(q.overlap_sphere(Vec3::ZERO, meters(0.5)).is_empty());
        assert!(q
            .capsule_cast(o, Quat::IDENTITY, meters(0.5), meters(0.0), Vec3::new(20.0, 0.0, 0.0))
            .is_none());
    }

    #[test]
    fn capsule_cast_reports_the_nearest_contact_in_metres_travelled() {
        // A wall at x = 5 (half-extent 1, so its near face is x = 4). A capsule
        // of radius 0.5 starting at x = -5 and travelling 20 along +X touches at
        // x = 3.5, i.e. 8.5 metres in.
        let mut w = world();
        let wall = spawn(&mut w, Vec3::new(5.0, 0.0, 0.0));
        let collider = attach(&mut w, wall, box_shape(1.0, 2.0, 2.0), false);
        let hit = PhysicsQuery::new(&w)
            .capsule_cast(
                Vec3::new(-5.0, 0.0, 0.0),
                Quat::IDENTITY,
                meters(0.5),
                meters(1.0),
                Vec3::new(20.0, 0.0, 0.0),
            )
            .expect("the swept capsule reaches the wall");
        assert_eq!(hit.body(), wall);
        assert_eq!(hit.collider(), collider);
        assert!(
            (hit.distance().get() - 8.5).abs() < 1.0e-3,
            "distance was {}",
            hit.distance().get()
        );
        assert!(hit.normal().subtract(Vec3::new(-1.0, 0.0, 0.0)).length() < 1.0e-4);
        assert!(hit.front_face());
    }

    #[test]
    fn capsule_cast_stops_at_the_nearest_of_several_obstacles() {
        let mut w = world();
        let near = spawn(&mut w, Vec3::new(3.0, 0.0, 0.0));
        attach(&mut w, near, box_shape(0.5, 2.0, 2.0), false);
        let distant = spawn(&mut w, Vec3::new(8.0, 0.0, 0.0));
        attach(&mut w, distant, box_shape(0.5, 2.0, 2.0), false);
        let hit = PhysicsQuery::new(&w)
            .capsule_cast(
                Vec3::new(-5.0, 0.0, 0.0),
                Quat::IDENTITY,
                meters(0.5),
                meters(1.0),
                Vec3::new(20.0, 0.0, 0.0),
            )
            .expect("the sweep is stopped");
        assert_eq!(hit.body(), near);
    }

    #[test]
    fn capsule_cast_that_reaches_nothing_or_starts_overlapping() {
        let mut w = world();
        let wall = spawn(&mut w, Vec3::new(50.0, 0.0, 0.0));
        attach(&mut w, wall, box_shape(1.0, 2.0, 2.0), false);
        let q = PhysicsQuery::new(&w);
        assert!(
            q.capsule_cast(
                Vec3::ZERO,
                Quat::IDENTITY,
                meters(0.5),
                meters(1.0),
                Vec3::new(1.0, 0.0, 0.0)
            )
            .is_none(),
            "a step that never reaches the wall is a miss"
        );

        // Starting inside the wall: an immediate hit with a usable normal, which
        // is exactly what a controller needs to push itself back out.
        let stuck = q
            .capsule_cast(
                Vec3::new(50.0, 0.0, 0.0),
                Quat::IDENTITY,
                meters(0.5),
                meters(1.0),
                Vec3::new(20.0, 0.0, 0.0),
            )
            .expect("an overlapping cast is an immediate hit");
        assert_eq!(stuck.distance().get(), 0.0);
        assert!(!stuck.front_face());
        assert!(stuck.normal().length() > 0.5, "the escape normal must be usable");
    }

    #[test]
    fn capsule_cast_lands_on_a_ground_plane() {
        let mut w = world();
        let ground = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, ground, plane(Vec3::UNIT_Y, 0.0), false);
        let hit = PhysicsQuery::new(&w)
            .capsule_cast(
                Vec3::new(0.0, 6.0, 0.0),
                Quat::IDENTITY,
                meters(0.5),
                meters(1.0),
                Vec3::new(0.0, -10.0, 0.0),
            )
            .expect("the falling capsule lands");
        assert_eq!(hit.body(), ground);
        // The lower end starts at y = 5 and stops at y = 0.5: 4.5 metres.
        assert!(
            (hit.distance().get() - 4.5).abs() < 1.0e-4,
            "distance was {}",
            hit.distance().get()
        );
        assert!(hit.normal().subtract(Vec3::UNIT_Y).length() < 1.0e-5);
    }

    #[test]
    fn non_finite_and_degenerate_query_inputs_are_deterministic_misses() {
        let mut w = world();
        let b = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, b, sphere(1.0), false);
        let q = PhysicsQuery::new(&w);
        let nan = Vec3::new(f32::NAN, 0.0, 0.0);

        assert!(q.raycast(nan, Vec3::UNIT_X, far()).is_none());
        assert!(q.raycast_all(nan, Vec3::UNIT_X, far()).is_empty());
        // A zero-length direction is not a ray.
        assert!(q.raycast(Vec3::new(-10.0, 0.0, 0.0), Vec3::ZERO, far()).is_none());

        assert!(q.overlap_sphere(nan, far()).is_empty());
        assert!(q
            .overlap_capsule(nan, Quat::IDENTITY, meters(1.0), meters(1.0))
            .is_empty());
        // A negative radius describes no volume.
        assert!(q
            .overlap_capsule(Vec3::ZERO, Quat::IDENTITY, meters(-1.0), meters(1.0))
            .is_empty());

        assert!(q
            .capsule_cast(nan, Quat::IDENTITY, meters(1.0), meters(1.0), Vec3::UNIT_X)
            .is_none());
        assert!(q
            .capsule_cast(
                Vec3::new(-10.0, 0.0, 0.0),
                Quat::IDENTITY,
                meters(1.0),
                meters(1.0),
                nan
            )
            .is_none());
    }

    #[test]
    fn queries_do_not_change_the_world() {
        let mut w = world();
        let b = spawn(&mut w, Vec3::ZERO);
        attach(&mut w, b, sphere(1.0), false);
        let before = w.snapshot();
        let q = PhysicsQuery::new(&w);
        let (o, d) = ray_x();
        let _ = q.raycast(o, d, far());
        let _ = q.raycast_all(o, d, far());
        let _ = q.overlap_sphere(Vec3::ZERO, far());
        let _ = q.overlap_capsule(Vec3::ZERO, Quat::IDENTITY, meters(1.0), meters(1.0));
        let _ = q.capsule_cast(
            o,
            Quat::IDENTITY,
            meters(0.5),
            meters(1.0),
            Vec3::new(20.0, 0.0, 0.0),
        );
        assert_eq!(before, w.snapshot(), "queries must not mutate world state");
    }

    #[test]
    fn the_dispatch_tables_and_the_shape_kinds_agree_in_length() {
        // The structural guard behind the heightfield regression: every table is
        // sized by this constant, so a sixth kind cannot be half-wired.
        assert_eq!(PhysicsShapeKind::COUNT, 5);
    }
}
