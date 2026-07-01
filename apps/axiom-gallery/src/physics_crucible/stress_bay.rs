//! Station 5 — Stress Bay: a 4×4 grid of dynamic spheres dropped onto a floor
//! so they collide, stack, and settle, exercising the broad phase, contact
//! solver, and substepping. The same drop run twice must be byte-identical,
//! and no body may tunnel through the floor.

use axiom::prelude::Vec3;

use crate::physics_crucible::crucible_scenario::{BodySpec, Station};
use crate::physics_crucible::crucible_station::CrucibleStation;
use crate::physics_crucible::debug_geometry::DebugShape;
use crate::physics_crucible::physics_crucible_app::CrucibleWorld;

/// Grid edge length (so the pile is `GRID × GRID` spheres).
const GRID: i32 = 4;
/// Sphere radius in the pile.
const RADIUS: f32 = 0.45;

/// Station 5 — the stress pile.
#[derive(Debug, Default)]
pub struct StressBay;

impl StressBay {
    /// The number of bodies the bay creates (the pile plus the floor).
    pub const BODY_COUNT: usize = (GRID * GRID) as usize + 1;
}

impl Station for StressBay {
    fn id(&self) -> CrucibleStation {
        CrucibleStation::StressBay
    }

    fn populate(&self, world: &mut CrucibleWorld) {
        world.spawn(self.id(), BodySpec::static_plane(Vec3::UNIT_Y, 0.0));
        for ix in 0..GRID {
            for iz in 0..GRID {
                let x = (ix as f32 - 1.5) * 1.05;
                let z = (iz as f32 - 1.5) * 1.05;
                // Slight per-column height stagger so the pile settles deterministically.
                let y = 2.0 + ((ix + iz) as f32) * 0.6;
                world.spawn(
                    self.id(),
                    BodySpec::dynamic_sphere(Vec3::new(x, y, z), RADIUS, 1.0),
                );
            }
        }
    }

    fn debug_shapes(&self, world: &CrucibleWorld) -> Vec<DebugShape> {
        world
            .contacts()
            .into_iter()
            .map(|c| DebugShape::ContactPoint { position: c.point })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(steps: u64) -> CrucibleWorld {
        let bay = StressBay;
        let mut world = CrucibleWorld::new();
        bay.populate(&mut world);
        for n in 0..steps {
            world.step(n);
        }
        world
    }

    #[test]
    fn creates_the_pile_and_the_floor() {
        let world = run(0);
        assert_eq!(
            world.station_bodies(CrucibleStation::StressBay).len(),
            StressBay::BODY_COUNT
        );
    }

    #[test]
    fn the_broad_phase_generates_candidate_pairs() {
        let world = run(60);
        assert!(
            world.step_counts().broad_phase_pair_count > 0,
            "a settling pile must produce broad-phase pairs"
        );
    }

    #[test]
    fn the_solver_resolves_contacts_in_the_pile() {
        let world = run(200);
        assert!(world.step_counts().solved_contact_count > 0);
    }

    #[test]
    fn no_body_tunnels_through_the_floor_and_all_state_is_finite() {
        let world = run(150);
        for s in world.body_states() {
            assert!(s.translation.x.is_finite() && s.translation.y.is_finite());
            assert!(
                s.translation.y > -0.2,
                "a body tunnelled the floor: y={}",
                s.translation.y
            );
        }
    }

    #[test]
    fn the_same_drop_run_twice_is_byte_identical() {
        let first = run(120);
        let second = run(120);
        assert_eq!(
            first.body_states(),
            second.body_states(),
            "the stress pile must be deterministic"
        );
    }
}
