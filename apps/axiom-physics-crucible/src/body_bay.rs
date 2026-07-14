//! Station 1 — Body Bay.
//! The body-kind catalogue: a static floor, a dynamic sphere that falls and
//! settles, an immovable static box, a kinematic box that ignores gravity, and a
//! dynamic body that is *disabled* on the first step and then holds its position
//! despite gravity. It proves the four facade body states (static / dynamic /
//! kinematic / disabled) behave distinctly, all driven through `PhysicsApi`.

use axiom::prelude::Vec3;

use crate::crucible_scenario::{BodySpec, Station};
use crate::crucible_station::CrucibleStation;
use crate::debug_geometry::DebugShape;
use crate::physics_crucible_app::CrucibleWorld;

/// Spawn index (within the bay) of the dynamic body that gets disabled.
const DISABLED_INDEX: usize = 4;

/// Station 1 — the body-kind catalogue.
#[derive(Debug, Default)]
pub struct BodyBay;

impl Station for BodyBay {
    fn id(&self) -> CrucibleStation {
        CrucibleStation::BodyBay
    }

    fn populate(&self, world: &mut CrucibleWorld) {
        // 0: ground plane.
        world.spawn(self.id(), BodySpec::static_plane(Vec3::UNIT_Y, 0.0));
        // 1: a dynamic sphere that falls and settles on the floor.
        world.spawn(
            self.id(),
            BodySpec::dynamic_sphere(Vec3::new(0.0, 6.0, 0.0), 0.5, 1.0),
        );
        // 2: an immovable static box.
        world.spawn(
            self.id(),
            BodySpec::static_box(Vec3::new(4.0, 1.0, 0.0), Vec3::ONE),
        );
        // 3: a kinematic box — ignores gravity, never integrated from velocity.
        world.spawn(
            self.id(),
            BodySpec::kinematic_box(Vec3::new(-4.0, 2.0, 0.0), Vec3::ONE),
        );
        // 4: a dynamic body disabled on step 0 — holds position despite gravity.
        world.spawn(
            self.id(),
            BodySpec::dynamic_sphere(Vec3::new(2.0, 4.0, 0.0), 0.5, 1.0),
        );
    }

    fn script(&self, world: &mut CrucibleWorld, step: u64) {
        // Disable the catalogue's disabled body before the first integration.
        if step == 0 {
            if let Some(body) = world.nth_body(self.id(), DISABLED_INDEX) {
                world.disable(body);
            }
        }
    }

    fn debug_shapes(&self, world: &CrucibleWorld) -> Vec<DebugShape> {
        // A velocity arrow on the falling sphere (spawn index 1).
        world
            .nth_body(self.id(), 1)
            .and_then(|h| {
                let pos = world.position_of(h)?;
                let vel = world
                    .body_states()
                    .into_iter()
                    .find(|s| s.handle == h)?
                    .linear_velocity;
                Some(vec![DebugShape::Velocity {
                    origin: pos,
                    velocity: vel,
                }])
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crucible_scenario::CrucibleShape;

    fn drive(steps: u64) -> CrucibleWorld {
        let bay = BodyBay;
        let mut world = CrucibleWorld::new();
        bay.populate(&mut world);
        for n in 0..steps {
            bay.script(&mut world, n);
            world.step(n);
        }
        world
    }

    fn y_of(world: &CrucibleWorld, index: usize) -> f32 {
        let h = world.nth_body(CrucibleStation::BodyBay, index).unwrap();
        world.position_of(h).unwrap().y
    }

    #[test]
    fn populates_the_full_body_kind_catalogue() {
        let world = drive(0);
        assert_eq!(world.station_bodies(CrucibleStation::BodyBay).len(), 5);
    }

    #[test]
    fn dynamic_sphere_falls_and_settles_on_the_floor() {
        // Free-fall from y=6 to the floor takes ~127 steps at 1/120 s; run well past
        // that so the sphere is fully settled, not mid-air.
        let world = drive(240);
        let y = y_of(&world, 1);
        // Rests near the sphere radius (0.5) on the plane at y=0.
        assert!(y < 0.9, "expected the sphere to settle low, got y={y}");
        assert!(y > 0.3, "expected it to rest on the floor, got y={y}");
    }

    #[test]
    fn static_box_never_moves() {
        let world = drive(60);
        assert_eq!(y_of(&world, 2), 1.0);
    }

    #[test]
    fn kinematic_box_ignores_gravity() {
        let world = drive(60);
        assert_eq!(y_of(&world, 3), 2.0);
    }

    #[test]
    fn disabled_body_holds_position_despite_gravity() {
        let world = drive(60);
        // Disabled before the first integration, so it never falls from y=4.0.
        assert_eq!(y_of(&world, DISABLED_INDEX), 4.0);
    }

    #[test]
    fn the_disabled_body_would_otherwise_have_fallen() {
        // Same body, never disabled: it must drop, proving the disable is load-bearing.
        let bay = BodyBay;
        let mut world = CrucibleWorld::new();
        bay.populate(&mut world);
        for n in 0..60 {
            world.step(n); // no script => never disabled
        }
        assert!(y_of(&world, DISABLED_INDEX) < 4.0);
    }

    #[test]
    fn a_re_enabled_body_resumes_falling() {
        let bay = BodyBay;
        let mut world = CrucibleWorld::new();
        bay.populate(&mut world);
        let h = world
            .nth_body(CrucibleStation::BodyBay, DISABLED_INDEX)
            .unwrap();
        world.disable(h);
        world.step(0);
        let y_while_disabled = world.position_of(h).unwrap().y;
        assert_eq!(y_while_disabled, 4.0, "stays put while disabled");
        world.enable(h);
        for n in 1..30 {
            world.step(n);
        }
        assert!(
            world.position_of(h).unwrap().y < y_while_disabled,
            "a re-enabled body must resume falling"
        );
    }

    #[test]
    fn a_capsule_body_can_be_created_through_the_facade() {
        let mut world = CrucibleWorld::new();
        let h = world.spawn(
            CrucibleStation::BodyBay,
            BodySpec::dynamic_capsule(Vec3::new(0.0, 5.0, 0.0), 0.4, 0.8, 1.0),
        );
        assert!(h.is_valid());
        assert!(matches!(
            world.body(h).unwrap().shape,
            CrucibleShape::Capsule { .. }
        ));
    }
}
