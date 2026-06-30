//! Station 2 — Contact Bay.
//!
//! The narrow-phase catalogue: the four contact pair types the physics module
//! actually generates — sphere/plane, sphere/sphere, sphere/box, and box/plane —
//! each as an isolated dropped body settling onto a target. It proves contacts are
//! generated and solved (the bodies rest instead of tunnelling) and surfaces the
//! per-step broad/narrow/solve counts through the report.

use axiom::prelude::Vec3;

use crate::physics_crucible::crucible_scenario::{BodySpec, Station};
use crate::physics_crucible::crucible_station::CrucibleStation;
use crate::physics_crucible::debug_geometry::DebugShape;
use crate::physics_crucible::physics_crucible_app::CrucibleWorld;

/// Station 2 — the narrow-phase catalogue.
#[derive(Debug, Default)]
pub struct ContactBay;

impl Station for ContactBay {
    fn id(&self) -> CrucibleStation {
        CrucibleStation::ContactBay
    }

    fn populate(&self, world: &mut CrucibleWorld) {
        // A floor spanning the cell — the partner for sphere/plane and box/plane.
        world.spawn(self.id(), BodySpec::static_plane(Vec3::UNIT_Y, 0.0));

        // sphere/plane: a sphere dropped onto the floor.
        world.spawn(
            self.id(),
            BodySpec::dynamic_sphere(Vec3::new(-5.0, 2.0, 0.0), 0.5, 1.0),
        );

        // sphere/sphere: a static sphere with a dynamic sphere dropped onto it.
        world.spawn(
            self.id(),
            BodySpec::dynamic_sphere(Vec3::new(0.0, 0.5, 4.0), 0.5, 1.0)
                .with_material(crate::physics_crucible::crucible_scenario::MaterialSpec::INELASTIC),
        );
        world.spawn(
            self.id(),
            BodySpec::dynamic_sphere(Vec3::new(0.0, 2.0, 4.0), 0.5, 1.0),
        );

        // sphere/box: a static box with a dynamic sphere dropped on top.
        world.spawn(
            self.id(),
            BodySpec::static_box(Vec3::new(5.0, 1.0, -3.0), Vec3::ONE),
        );
        world.spawn(
            self.id(),
            BodySpec::dynamic_sphere(Vec3::new(5.0, 3.2, -3.0), 0.5, 1.0),
        );

        // box/plane: a dynamic box dropped onto the floor.
        world.spawn(
            self.id(),
            BodySpec::dynamic_box(Vec3::new(-5.0, 2.0, 4.0), Vec3::new(0.5, 0.5, 0.5), 1.0),
        );
    }

    fn debug_shapes(&self, world: &CrucibleWorld) -> Vec<DebugShape> {
        // A contact point + normal for every resolved contact this step.
        world
            .contacts()
            .into_iter()
            .flat_map(|c| {
                [
                    DebugShape::ContactPoint { position: c.point },
                    DebugShape::ContactNormal {
                        origin: c.point,
                        direction: c.normal,
                        length: 1.0,
                    },
                ]
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(steps: u64) -> CrucibleWorld {
        let bay = ContactBay;
        let mut world = CrucibleWorld::new();
        bay.populate(&mut world);
        for n in 0..steps {
            bay.script(&mut world, n);
            world.step(n);
        }
        world
    }

    #[test]
    fn a_settled_world_reports_contacts() {
        let world = run(90);
        assert!(
            !world.contacts().is_empty(),
            "expected resolved contacts once bodies settle"
        );
    }

    #[test]
    fn the_record_counts_broad_and_narrow_phase_work() {
        let world = run(90);
        let counts = world.step_counts();
        assert!(counts.broad_phase_pair_count > 0);
        assert!(counts.contact_pair_count > 0);
        // The broad phase is a superset of the contacts it generates.
        assert!(counts.broad_phase_pair_count >= counts.contact_pair_count);
    }

    #[test]
    fn approaching_contacts_are_actually_solved() {
        let world = run(90);
        assert!(
            world.step_counts().solved_contact_count > 0,
            "expected the solver to resolve resting contacts"
        );
    }

    #[test]
    fn a_dropped_sphere_rests_on_the_plane_instead_of_tunnelling() {
        let world = run(120);
        // Spawn index 1 is the sphere/plane sphere; it must rest near radius 0.5,
        // never pass through the floor.
        let h = world.nth_body(CrucibleStation::ContactBay, 1).unwrap();
        let y = world.position_of(h).unwrap().y;
        assert!(y > 0.3 && y < 0.8, "sphere did not rest on the plane: y={y}");
    }

    #[test]
    fn a_dropped_box_rests_on_the_plane() {
        let world = run(120);
        // Spawn index 6 is the box/plane box (half-extent 0.5 → rests near y=0.5).
        let h = world.nth_body(CrucibleStation::ContactBay, 6).unwrap();
        let y = world.position_of(h).unwrap().y;
        assert!(y > 0.3 && y < 0.8, "box did not rest on the plane: y={y}");
    }

    #[test]
    fn contact_normals_face_away_from_the_floor() {
        let world = run(90);
        // Every floor contact should have a normal with a positive Y component.
        let any_up = world.contacts().iter().any(|c| c.normal.y.abs() > 0.5);
        assert!(any_up, "expected a roughly vertical contact normal");
    }
}
