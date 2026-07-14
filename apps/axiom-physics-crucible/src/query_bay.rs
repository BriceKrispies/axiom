//! Station 4 — Query Bay: raycast hit, raycast miss, a raycast that passes
//! *through* a trigger to the solid body behind it (triggers are excluded from
//! raycasts), and an overlap-sphere probe tallied each frame.

use axiom::prelude::Vec3;

use crate::crucible_scenario::{BodySpec, Station};
use crate::crucible_station::CrucibleStation;
use crate::debug_geometry::DebugShape;
use crate::physics_crucible_app::CrucibleWorld;

/// The standing overlap probe, in the bay's local frame.
pub const PROBE_LOCAL: Vec3 = Vec3::new(4.0, 1.0, 3.0);
/// Kept just under the probe's 1.0 height above the floor so it cleanly catches
/// the two parked spheres and *not* the infinite ground planes (an infinite
/// plane overlaps any query sphere whose centre is within `radius` of it —
/// correct, but it would inflate the tally).
pub const PROBE_RADIUS: f32 = 0.9;

/// The standing overlap probe in world space (centre, radius).
pub fn probe_world() -> (Vec3, f32) {
    (
        CrucibleStation::QueryBay.origin().add(PROBE_LOCAL),
        PROBE_RADIUS,
    )
}

/// Station 4 — spatial queries.
#[derive(Debug, Default)]
pub struct QueryBay;

impl QueryBay {
    /// The world-space ray that hits the target box (origin, direction, max).
    fn target_ray() -> (Vec3, Vec3, f32) {
        let origin = CrucibleStation::QueryBay
            .origin()
            .add(Vec3::new(0.0, 1.0, 9.0));
        (origin, Vec3::new(0.0, 0.0, -1.0), 20.0)
    }

    /// A world-space ray aimed over the top of the box — a deliberate miss.
    #[cfg(test)]
    fn miss_ray() -> (Vec3, Vec3, f32) {
        let origin = CrucibleStation::QueryBay
            .origin()
            .add(Vec3::new(0.0, 8.0, 9.0));
        (origin, Vec3::new(0.0, 0.0, -1.0), 20.0)
    }

    /// A world-space ray that crosses a trigger before the solid body behind it.
    #[cfg(test)]
    fn through_trigger_ray() -> (Vec3, Vec3, f32) {
        let origin = CrucibleStation::QueryBay
            .origin()
            .add(Vec3::new(-4.0, 1.0, 9.0));
        (origin, Vec3::new(0.0, 0.0, -1.0), 20.0)
    }
}

impl Station for QueryBay {
    fn id(&self) -> CrucibleStation {
        CrucibleStation::QueryBay
    }

    fn populate(&self, world: &mut CrucibleWorld) {
        // 0: the raycast target — a solid static box.
        world.spawn(
            self.id(),
            BodySpec::static_box(Vec3::new(0.0, 1.0, 0.0), Vec3::ONE),
        );
        // 1,2: two static spheres parked inside the overlap probe (no gravity
        // drift, so the probe finds them at any step).
        world.spawn(self.id(), BodySpec::static_sphere(PROBE_LOCAL, 0.5));
        world.spawn(
            self.id(),
            BodySpec::static_sphere(PROBE_LOCAL.add(Vec3::new(0.8, 0.0, 0.0)), 0.5),
        );
        // 3: a body well outside the probe.
        world.spawn(
            self.id(),
            BodySpec::static_box(Vec3::new(4.0, 1.0, -6.0), Vec3::new(0.4, 0.4, 0.4)),
        );
        // 4: a trigger sphere in front of the solid body.
        world.spawn(
            self.id(),
            BodySpec::trigger_sphere(Vec3::new(-4.0, 1.0, 3.0), 0.8),
        );
        // 5: a solid box behind the trigger (the through-trigger ray must hit this).
        world.spawn(
            self.id(),
            BodySpec::static_box(Vec3::new(-4.0, 1.0, -2.0), Vec3::ONE),
        );
    }

    fn debug_shapes(&self, world: &CrucibleWorld) -> Vec<DebugShape> {
        let (ro, rd, rmax) = QueryBay::target_ray();
        let hit = world
            .raycast(ro, rd, rmax)
            .and_then(|h| world.position_of(h));
        let (center, radius) = probe_world();
        vec![
            DebugShape::Ray {
                origin: ro,
                direction: rd,
                length: rmax,
                hit,
            },
            DebugShape::OverlapSphere { center, radius },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn populated() -> CrucibleWorld {
        let bay = QueryBay;
        let mut world = CrucibleWorld::new();
        bay.populate(&mut world);
        world
    }

    #[test]
    fn raycast_hits_the_target_box() {
        let world = populated();
        let target = world.nth_body(CrucibleStation::QueryBay, 0).unwrap();
        let (o, d, m) = QueryBay::target_ray();
        assert_eq!(world.raycast(o, d, m), Some(target));
    }

    #[test]
    fn raycast_over_the_box_misses() {
        let world = populated();
        let (o, d, m) = QueryBay::miss_ray();
        assert_eq!(world.raycast(o, d, m), None);
    }

    #[test]
    fn overlap_sphere_finds_the_two_bodies_in_range() {
        let world = populated();
        let (center, radius) = probe_world();
        let hits = world.overlap_sphere(center, radius);
        assert!(
            hits.len() >= 2,
            "expected >=2 overlap hits, got {}",
            hits.len()
        );
    }

    #[test]
    fn overlap_sphere_excludes_a_distant_body() {
        let world = populated();
        let far = world.nth_body(CrucibleStation::QueryBay, 3).unwrap();
        let (center, radius) = probe_world();
        assert!(!world.overlap_sphere(center, radius).contains(&far));
    }

    #[test]
    fn a_raycast_passes_through_a_trigger_to_the_solid_body_behind_it() {
        let world = populated();
        let trigger = world.nth_body(CrucibleStation::QueryBay, 4).unwrap();
        let solid = world.nth_body(CrucibleStation::QueryBay, 5).unwrap();
        let (o, d, m) = QueryBay::through_trigger_ray();
        let hit = world.raycast(o, d, m);
        assert_eq!(
            hit,
            Some(solid),
            "ray should skip the trigger and hit the solid box"
        );
        assert_ne!(hit, Some(trigger));
    }
}
