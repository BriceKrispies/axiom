//! The free-standing world queries — `physics.raycast` / `capsuleCast` /
//! `checkCapsule` / `groundHeight` / `queryAabb`.
//!
//! Ported from Claude-of-Duty `src/physics/index.js:411-678` — the query half of
//! `PhysicsSystem`'s public surface, the part that is a thin, decision-free
//! wrapper over [`StaticWorld`]. (`index.js`'s *other* halves — the rigid-body
//! world, the ballistics solver, the collider registry — are separate slices and
//! are not ported.)
//!
//! ## Why this file exists at all
//!
//! Four subsystems in this port each named a narrow trait for "the one thing I
//! need from physics", because physics landed as a concurrent slice and none of
//! them could name a type that did not exist yet:
//!
//! | seam                                    | named by |
//! |-----------------------------------------|----------|
//! | [`crate::player::mantle::WorldProbe`]   | `mantle.js`'s ledge probe + `movement.js`'s lean probe |
//! | [`crate::audio::spatial::WorldProbe`]   | `spatial.js`'s occlusion ray |
//! | [`crate::fx::decals::DecalWorld`]       | `atlas.js`'s decal triangle clipper |
//! | [`crate::fx::world::FxWorld`]           | `impacts.js`/`index.js`'s bounce + ground probes |
//!
//! [`PhysicsWorld`] is one type that satisfies all four, because in the source
//! they are all one object: `ctx.get('physics')`. Nothing else in the port needs
//! to know that — each subsystem still speaks only to its own trait.
//!
//! `weapons::ballistics::RaycastWorld` is **not** bound here: its second method
//! is `fire_bullet`, which needs the penetration solver
//! (`src/physics/penetration.js`, not ported), so binding it would mean
//! inventing behaviour. See the notes file.

use std::rc::Rc;

use crate::audio::spatial::{RayHit as AudioRayHit, RayMask, WorldProbe as AudioProbe};
use crate::fx::decals::DecalWorld;
use crate::fx::world::{FxHit, FxWorld};
use crate::physics::bvh::StaticWorld;
use crate::physics::surfaces::mask;
use crate::player::mantle::{CapsuleHit, ProbeMask, RayHit, WorldProbe};
use crate::world::palette::Surface;

/// The static collision world, shared with every subsystem that probes it.
///
/// [`StaticWorld`] is immutable once built (the level's geometry is registered
/// and `build()` run exactly once), so this is an [`Rc`] and not a
/// `RefCell` — the same handle the character controller
/// ([`crate::physics::character::Character`]) holds.
#[derive(Clone)]
pub struct PhysicsWorld {
    world: Rc<StaticWorld>,
}

impl PhysicsWorld {
    pub fn new(world: Rc<StaticWorld>) -> Self {
        PhysicsWorld { world }
    }

    /// The shared world handle, for a caller that needs the BVH directly (the
    /// character controller takes one of these).
    pub fn world(&self) -> Rc<StaticWorld> {
        Rc::clone(&self.world)
    }

    /// `raycast(ox, oy, oz, dx, dy, dz, maxDist, mask)`. `index.js:415-433` —
    /// the direction is normalised here exactly as the source does, and a
    /// degenerate direction is a miss (the source returns a non-`hit` record).
    pub fn raycast(
        &self,
        origin: [f64; 3],
        dir: [f64; 3],
        max_dist: f64,
        mask: u16,
    ) -> Option<crate::physics::math::HitRecord> {
        let l = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        if l < 1e-9 {
            return None;
        }
        let hit = self.world.raycast(
            origin[0],
            origin[1],
            origin[2],
            dir[0] / l,
            dir[1] / l,
            dir[2] / l,
            max_dist,
            mask,
            -1,
        );
        hit.hit.then_some(hit)
    }

    /// `capsuleCast(p0, p1, radius, dir, maxDist, mask)`. `index.js:629-656`.
    #[allow(clippy::too_many_arguments)]
    pub fn capsule_cast(
        &self,
        p0: [f64; 3],
        p1: [f64; 3],
        radius: f64,
        dir: [f64; 3],
        max_dist: f64,
        mask: u16,
    ) -> Option<crate::physics::math::HitRecord> {
        let l = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        if l < 1e-9 {
            return None;
        }
        let hit = self.world.sweep_capsule(
            p0[0],
            p0[1],
            p0[2],
            p1[0],
            p1[1],
            p1[2],
            radius,
            dir[0] / l,
            dir[1] / l,
            dir[2] / l,
            max_dist,
            mask,
        );
        hit.hit.then_some(hit)
    }

    /// `checkCapsule(p0, p1, radius, mask)`. `index.js:664-666` — true when
    /// clear.
    pub fn check_capsule(&self, p0: [f64; 3], p1: [f64; 3], radius: f64, mask: u16) -> bool {
        self.world
            .overlap_capsule(
                p0[0], p0[1], p0[2], p1[0], p1[1], p1[2], radius, mask, 0.0,
            )
            .count()
            == 0
    }

    /// `groundHeight(x, z, fromY, mask)`. `index.js:675-678`. The source
    /// returns `-Infinity` for "no floor"; this returns `None`, which is what
    /// every caller's `Number.isFinite(gy)` guard actually tests.
    pub fn ground_height(&self, x: f64, z: f64, from_y: f64) -> Option<f64> {
        self.raycast([x, from_y, z], [0.0, -1.0, 0.0], 1000.0, mask::WORLD)
            .map(|h| h.py)
    }

    /// `raycastAny(ox, oy, oz, dx, dy, dz, maxDist, mask)`. `index.js:596-613`
    /// — the cheap occlusion test: no ordering, no hit record, just "does
    /// anything block this ray." Direction is normalised here exactly as the
    /// source does; a degenerate direction is a miss.
    pub fn raycast_any(&self, origin: [f64; 3], dir: [f64; 3], max_dist: f64, mask: u16) -> bool {
        let l = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
        if l < 1e-9 {
            return false;
        }
        self.world.raycast_any(
            origin[0],
            origin[1],
            origin[2],
            dir[0] / l,
            dir[1] / l,
            dir[2] / l,
            max_dist,
            mask,
        )
    }
}

/// `phys.MASK.*` — the two masks `mantle.js` queries against.
fn mask_bits(mask: ProbeMask) -> u16 {
    match mask {
        ProbeMask::Character => mask::CHARACTER,
        ProbeMask::World => mask::WORLD,
    }
}

/// The ledge/lean probe seam. `mantle.js`'s three physics calls.
impl WorldProbe for PhysicsWorld {
    fn raycast(
        &self,
        origin: [f64; 3],
        dir: [f64; 3],
        max_dist: f64,
        mask: ProbeMask,
    ) -> Option<RayHit> {
        PhysicsWorld::raycast(self, origin, dir, max_dist, mask_bits(mask)).map(|h| RayHit {
            point: [h.px, h.py, h.pz],
            normal: [h.nx, h.ny, h.nz],
            surface: Surface::from_index(h.surface),
        })
    }

    fn capsule_cast(
        &self,
        p0: [f64; 3],
        p1: [f64; 3],
        radius: f64,
        dir: [f64; 3],
        max_dist: f64,
        mask: ProbeMask,
    ) -> Option<CapsuleHit> {
        PhysicsWorld::capsule_cast(self, p0, p1, radius, dir, max_dist, mask_bits(mask)).map(|h| {
            CapsuleHit {
                normal: [h.nx, h.ny, h.nz],
                distance: h.t,
                surface: Surface::from_index(h.surface),
            }
        })
    }

    fn check_capsule_segment(
        &self,
        p0: [f64; 3],
        p1: [f64; 3],
        radius: f64,
        mask: ProbeMask,
    ) -> bool {
        // `mantle.js`/`movement.js` call `phys.checkCapsule`, which is true when
        // CLEAR (`index.js:664-666`).
        PhysicsWorld::check_capsule(self, p0, p1, radius, mask_bits(mask))
    }
}

/// The navigation ray-sampling seam. `nav.js`'s `phys.raycast`/`phys.raycastAny`
/// calls, used to build the walkability grid and cover map, and by
/// `CoverMap::peek_offset`'s line-of-sight probe.
impl crate::ai::nav::WorldProbe for PhysicsWorld {
    fn raycast(&self, origin: [f64; 3], dir: [f64; 3], max_dist: f64, mask: u16) -> Option<crate::ai::nav::RayHit> {
        PhysicsWorld::raycast(self, origin, dir, max_dist, mask).map(|h| crate::ai::nav::RayHit {
            point: [h.px, h.py, h.pz],
            normal: [h.nx, h.ny, h.nz],
            distance: h.t,
        })
    }

    fn raycast_any(&self, origin: [f64; 3], dir: [f64; 3], max_dist: f64, mask: u16) -> bool {
        PhysicsWorld::raycast_any(self, origin, dir, max_dist, mask)
    }
}

/// The audio occlusion seam. `spatial.js:207-208`'s one call — note the
/// direction is deliberately un-normalised by the caller, which
/// [`PhysicsWorld::raycast`] handles the same way the source does.
impl AudioProbe for PhysicsWorld {
    fn raycast(
        &self,
        origin: [f64; 3],
        dir: [f64; 3],
        max_dist: f64,
        mask: RayMask,
    ) -> Option<AudioRayHit> {
        let bits = match mask {
            RayMask::Sight => mask::SIGHT,
            RayMask::World => mask::WORLD,
        };
        PhysicsWorld::raycast(self, origin, dir, max_dist, bits).map(|h| AudioRayHit {
            distance: h.t,
            surface: Surface::from_index(h.surface),
        })
    }
}

/// The decal clipper's triangle-soup seam. `atlas.js:220-236`.
impl DecalWorld for PhysicsWorld {
    fn tri_count(&self) -> usize {
        self.world.tri_count()
    }

    fn query_aabb(&self, min: [f64; 3], max: [f64; 3], mask: u16) -> Vec<u32> {
        self.world
            .query_aabb(min[0], min[1], min[2], max[0], max[1], max[2], mask)
    }

    fn triangle(&self, tri: u32) -> ([[f64; 3]; 3], [f64; 3]) {
        (self.world.triangle_of(tri), self.world.normal_of(tri))
    }
}

/// The FX seam — the decal clipper plus `impacts.js`'s spark-bounce raycast and
/// `index.js`'s `scorch`/`bloodSpatterBehind`/`onActorDeath`/`onLand` ground
/// probes.
impl FxWorld for PhysicsWorld {
    fn raycast(
        &self,
        origin: (f64, f64, f64),
        dir: (f64, f64, f64),
        max_dist: f64,
        mask: u16,
    ) -> Option<FxHit> {
        PhysicsWorld::raycast(
            self,
            [origin.0, origin.1, origin.2],
            [dir.0, dir.1, dir.2],
            max_dist,
            mask,
        )
        .map(|h| FxHit {
            point: (h.px, h.py, h.pz),
            normal: (h.nx, h.ny, h.nz),
            distance: h.t,
            surface: Surface::from_index(h.surface),
        })
    }

    fn ground_height(&self, x: f64, z: f64, from_y: f64) -> Option<f64> {
        PhysicsWorld::ground_height(self, x, z, from_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::surfaces::layer;

    /// One floor quad at `y = 0` spanning `[-10, 10]^2`, and a wall at `x = 2`.
    fn probe_world() -> PhysicsWorld {
        let mut world = StaticWorld::new();
        // Wound so the floor's geometric normal points up...
        let floor = vec![
            -10.0, 0.0, 10.0, 10.0, 0.0, 10.0, 10.0, 0.0, -10.0, //
            -10.0, 0.0, 10.0, 10.0, 0.0, -10.0, -10.0, 0.0, -10.0,
        ];
        world.add_triangles(&floor, 2, Surface::Concrete, layer::STATIC, "floor");
        // ...and the wall's back at -X.
        let wall = vec![
            2.0, 0.0, 10.0, 2.0, 3.0, 10.0, 2.0, 3.0, -10.0, //
            2.0, 0.0, 10.0, 2.0, 3.0, -10.0, 2.0, 0.0, -10.0,
        ];
        world.add_triangles(&wall, 2, Surface::Metal, layer::STATIC, "wall");
        world.build();
        PhysicsWorld::new(Rc::new(world))
    }

    #[test]
    fn a_downward_ray_finds_the_floor_and_reports_its_surface() {
        let p = probe_world();
        let hit = WorldProbe::raycast(&p, [0.0, 5.0, 0.0], [0.0, -1.0, 0.0], 100.0, ProbeMask::World)
            .expect("the floor is under the origin");
        assert!((hit.point[1]).abs() < 1e-9);
        assert!(hit.normal[1] > 0.99);
        assert_eq!(hit.surface, Surface::Concrete);
    }

    #[test]
    fn a_degenerate_direction_is_a_miss_rather_than_a_panic() {
        let p = probe_world();
        assert!(WorldProbe::raycast(&p, [0.0, 5.0, 0.0], [0.0, 0.0, 0.0], 100.0, ProbeMask::World)
            .is_none());
        assert!(p
            .capsule_cast([0.0; 3], [0.0, 1.0, 0.0], 0.3, [0.0; 3], 5.0, mask::CHARACTER)
            .is_none());
    }

    #[test]
    fn ground_height_is_the_floors_y_and_none_off_the_edge() {
        let p = probe_world();
        assert_eq!(p.ground_height(0.0, 0.0, 6.0), Some(0.0));
        assert_eq!(p.ground_height(500.0, 0.0, 6.0), None);
        assert_eq!(FxWorld::ground_height(&p, 0.0, 0.0, 6.0), Some(0.0));
    }

    #[test]
    fn a_capsule_cast_toward_the_wall_stops_at_it_with_a_facing_normal() {
        let p = probe_world();
        let hit = WorldProbe::capsule_cast(
            &p,
            [0.0, 0.4, 0.0],
            [0.0, 1.5, 0.0],
            0.3,
            [1.0, 0.0, 0.0],
            5.0,
            ProbeMask::Character,
        )
        .expect("the wall is 2 m ahead");
        assert!(hit.distance > 1.0 && hit.distance < 2.0, "d = {}", hit.distance);
        assert!(hit.normal[0] < -0.9, "the wall faces -X back at us");
        assert_eq!(hit.surface, Surface::Metal);
    }

    #[test]
    fn check_capsule_segment_is_true_when_clear_and_false_inside_the_wall() {
        let p = probe_world();
        assert!(WorldProbe::check_capsule_segment(
            &p,
            [0.0, 0.5, 0.0],
            [0.0, 1.5, 0.0],
            0.3,
            ProbeMask::Character
        ));
        assert!(!WorldProbe::check_capsule_segment(
            &p,
            [2.0, 0.5, 0.0],
            [2.0, 1.5, 0.0],
            0.3,
            ProbeMask::Character
        ));
    }

    #[test]
    fn the_audio_probe_reports_the_occluder_distance() {
        let p = probe_world();
        let hit = AudioProbe::raycast(&p, [0.0, 1.0, 0.0], [4.0, 0.0, 0.0], 4.0, RayMask::Sight)
            .expect("the wall occludes");
        assert!((hit.distance - 2.0).abs() < 1e-6, "d = {}", hit.distance);
        assert_eq!(hit.surface, Surface::Metal);
        // Nothing at all in the other direction.
        assert!(
            AudioProbe::raycast(&p, [0.0, 1.0, 0.0], [-4.0, 0.0, 0.0], 4.0, RayMask::World)
                .is_none()
        );
    }

    #[test]
    fn the_decal_seam_returns_real_triangles_from_the_soup() {
        let p = probe_world();
        assert_eq!(DecalWorld::tri_count(&p), 4);
        let cands = DecalWorld::query_aabb(&p, [-1.0, -1.0, -1.0], [1.0, 1.0, 1.0], layer::STATIC);
        assert!(!cands.is_empty(), "the floor is in that box");
        let (verts, normal) = DecalWorld::triangle(&p, cands[0]);
        assert!(verts.iter().all(|v| v[1].abs() < 1e-9), "a floor triangle");
        assert!(normal[1].abs() > 0.99);
    }

    #[test]
    fn the_fx_seam_forwards_a_raycast_with_its_distance() {
        let p = probe_world();
        let hit = FxWorld::raycast(&p, (0.0, 1.0, 0.0), (1.0, 0.0, 0.0), 5.0, mask::WORLD)
            .expect("the wall is ahead");
        assert!((hit.distance - 2.0).abs() < 1e-6);
        assert_eq!(hit.surface, Surface::Metal);
        assert!(hit.normal.0 < -0.9);
    }

    #[test]
    fn the_shared_world_handle_is_the_same_bvh() {
        let p = probe_world();
        assert_eq!(p.world().tri_count(), 4);
    }
}
