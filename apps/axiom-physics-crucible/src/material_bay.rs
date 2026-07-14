//! Station 3 — Material Bay.
//! Material response: a restitution ladder (three spheres dropped from the same
//! height with restitution `0.0`, `0.5`, `0.9`) proving the solver's combined
//! restitution makes a bouncy body rebound, and a mass test proving an
//! instantaneous impulse changes a heavy body's velocity less than a light one's
//! (`Δv = impulse · inverse_mass`). All through `PhysicsApi`.

use axiom::prelude::Vec3;

use crate::crucible_scenario::{BodySpec, MaterialSpec, Station};
use crate::crucible_station::CrucibleStation;
use crate::debug_geometry::DebugShape;
use crate::physics_crucible_app::CrucibleWorld;

/// The restitution ladder rendered in the bay.
const LADDER: [f32; 3] = [0.0, 0.5, 0.9];

/// Station 3 — material response.
#[derive(Debug, Default)]
pub struct MaterialBay;

impl Station for MaterialBay {
    fn id(&self) -> CrucibleStation {
        CrucibleStation::MaterialBay
    }

    fn populate(&self, world: &mut CrucibleWorld) {
        world.spawn(self.id(), BodySpec::static_plane(Vec3::UNIT_Y, 0.0));
        for (i, restitution) in LADDER.iter().enumerate() {
            let x = (i as f32 - 1.0) * 3.0;
            world.spawn(
                self.id(),
                BodySpec::dynamic_sphere(Vec3::new(x, 3.0, 0.0), 0.5, 1.0)
                    .with_material(MaterialSpec::INELASTIC.with_restitution(*restitution)),
            );
        }
    }

    fn debug_shapes(&self, world: &CrucibleWorld) -> Vec<DebugShape> {
        // A velocity arrow over each ladder sphere (spawn indices 1..=3).
        (1..=LADDER.len())
            .filter_map(|i| {
                let h = world.nth_body(self.id(), i)?;
                let pos = world.position_of(h)?;
                let vel = world
                    .body_states()
                    .into_iter()
                    .find(|s| s.handle == h)?
                    .linear_velocity;
                Some(DebugShape::Velocity {
                    origin: pos,
                    velocity: vel,
                })
            })
            .collect()
    }
}

/// Drop one sphere of the given restitution onto an inelastic floor and return its
/// vertical velocity at the first step a contact is resolved (the post-impulse
/// rebound velocity). A pure, deterministic probe used by the rebound tests.
#[cfg(test)]
fn rebound_velocity(restitution: f32) -> f32 {
    let mut world = CrucibleWorld::new();
    world.spawn(
        CrucibleStation::MaterialBay,
        BodySpec::static_plane(Vec3::UNIT_Y, 0.0),
    );
    let ball = world.spawn(
        CrucibleStation::MaterialBay,
        BodySpec::dynamic_sphere(Vec3::new(0.0, 3.0, 0.0), 0.5, 1.0)
            .with_material(MaterialSpec::INELASTIC.with_restitution(restitution)),
    );
    for n in 0..240 {
        world.step(n);
        if !world.contacts().is_empty() {
            break;
        }
    }
    world
        .body_states()
        .into_iter()
        .find(|s| s.handle == ball)
        .map(|s| s.linear_velocity.y)
        .unwrap_or(0.0)
}

/// The speed gained by a single dynamic sphere of `mass` after one fixed impulse.
#[cfg(test)]
fn impulse_speed(mass: f32) -> f32 {
    let mut world = CrucibleWorld::new();
    let ball = world.spawn(
        CrucibleStation::MaterialBay,
        BodySpec::dynamic_sphere(Vec3::new(0.0, 10.0, 0.0), 0.5, mass),
    );
    world.apply_impulse(ball, Vec3::new(10.0, 0.0, 0.0));
    world.step(0);
    world
        .body_states()
        .into_iter()
        .find(|s| s.handle == ball)
        .map(|s| s.linear_velocity.x)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn populates_a_floor_and_the_restitution_ladder() {
        let bay = MaterialBay;
        let mut world = CrucibleWorld::new();
        bay.populate(&mut world);
        assert_eq!(world.station_bodies(CrucibleStation::MaterialBay).len(), 4);
    }

    #[test]
    fn an_elastic_sphere_rebounds_upward_and_an_inelastic_one_does_not() {
        let elastic = rebound_velocity(0.9);
        let inelastic = rebound_velocity(0.0);
        assert!(elastic > 0.1, "elastic sphere should rebound up, got {elastic}");
        assert!(
            inelastic < elastic - 0.1,
            "inelastic ({inelastic}) should rebound far less than elastic ({elastic})"
        );
    }

    #[test]
    fn rebound_speed_increases_with_restitution() {
        let low = rebound_velocity(0.0);
        let mid = rebound_velocity(0.5);
        let high = rebound_velocity(0.9);
        assert!(mid > low, "0.5 ({mid}) should out-bounce 0.0 ({low})");
        assert!(high > mid, "0.9 ({high}) should out-bounce 0.5 ({mid})");
    }

    #[test]
    fn a_heavier_body_gains_less_speed_from_the_same_impulse() {
        let light = impulse_speed(1.0);
        let heavy = impulse_speed(8.0);
        assert!(light > 0.0);
        assert!(
            heavy < light,
            "heavy body ({heavy}) should gain less than light ({light})"
        );
        // Δv scales with inverse mass: 8× mass → ~1/8 the speed.
        assert!((light / heavy - 8.0).abs() < 0.5, "expected ~8× ratio");
    }

    #[test]
    fn material_validation_rejects_restitution_above_one() {
        use axiom_kernel::Ratio;
        use axiom_physics::PhysicsApi;
        let bad = PhysicsApi::material(
            Ratio::new(0.5).unwrap(),
            Ratio::new(1.5).unwrap(),
            Ratio::new(1.0).unwrap(),
        );
        assert!(bad.is_err(), "restitution > 1 must be rejected");
    }
}
