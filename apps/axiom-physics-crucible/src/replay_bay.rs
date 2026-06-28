//! Station 6 — Replay Bay.
//!
//! Determinism made visible. The bay itself is a simple scripted scenario (a
//! sphere shoved sideways by an impulse, then falling onto a floor), but its real
//! job is to be run inside the [`Crucible`]'s two-world harness: the visible world
//! and the hidden replay world receive byte-identical inputs and therefore stay in
//! perfect sync — and a deliberate perturbation in the replay world is *detected*
//! (the projected states stop matching). This is the proof that "deterministic"
//! means same-binary replay, the property the physics module actually guarantees.

use axiom::prelude::Vec3;

use crate::crucible_scenario::{BodySpec, Station};
use crate::crucible_station::CrucibleStation;
use crate::debug_geometry::DebugShape;
use crate::physics_crucible_app::CrucibleWorld;

/// The step at which the bay shoves its sphere sideways (deterministic input).
const SHOVE_STEP: u64 = 2;

/// Station 6 — the replay proof scenario.
#[derive(Debug, Default)]
pub struct ReplayBay;

impl Station for ReplayBay {
    fn id(&self) -> CrucibleStation {
        CrucibleStation::ReplayBay
    }

    fn populate(&self, world: &mut CrucibleWorld) {
        world.spawn(self.id(), BodySpec::static_plane(Vec3::UNIT_Y, 0.0));
        world.spawn(
            self.id(),
            BodySpec::dynamic_sphere(Vec3::new(-2.0, 4.0, 0.0), 0.5, 1.0),
        );
    }

    fn script(&self, world: &mut CrucibleWorld, step: u64) {
        if step == SHOVE_STEP {
            if let Some(ball) = world.nth_body(self.id(), 1) {
                world.apply_impulse(ball, Vec3::new(3.0, 0.0, 0.0));
            }
        }
    }

    fn debug_shapes(&self, world: &CrucibleWorld) -> Vec<DebugShape> {
        world
            .nth_body(self.id(), 1)
            .and_then(|h| world.position_of(h))
            .map(|p| vec![DebugShape::Marker {
                position: p,
                color: crate::debug_geometry::palette::REPLAY_OK,
                size: 0.3,
            }])
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics_crucible_app::Crucible;

    fn only_replay() -> Vec<Box<dyn Station>> {
        vec![Box::new(ReplayBay)]
    }

    #[test]
    fn the_scripted_sphere_moves_sideways_and_falls() {
        let bay = ReplayBay;
        let mut world = CrucibleWorld::new();
        bay.populate(&mut world);
        let ball = world.nth_body(CrucibleStation::ReplayBay, 1).unwrap();
        let start = world.position_of(ball).unwrap();
        for n in 0..40 {
            bay.script(&mut world, n);
            world.step(n);
        }
        let end = world.position_of(ball).unwrap();
        assert!(end.x > start.x, "the impulse should move it +X");
        assert!(end.y < start.y, "gravity should pull it down");
    }

    #[test]
    fn identical_worlds_stay_in_perfect_sync() {
        let mut crucible = Crucible::new(only_replay());
        crucible.run();
        assert!(
            crucible.replay_matches(),
            "two identically-driven worlds must agree"
        );
    }

    #[test]
    fn a_replay_perturbation_is_detected() {
        let mut crucible = Crucible::new(only_replay());
        crucible.perturb_replay_at(SHOVE_STEP + 1);
        crucible.run();
        assert!(
            !crucible.replay_matches(),
            "a perturbed replay world must diverge from the visible one"
        );
    }

    #[test]
    fn the_report_shows_a_matching_replay_for_an_unperturbed_run() {
        let mut crucible = Crucible::new(only_replay());
        crucible.run();
        assert!(crucible.report().replay_match());
    }

    #[test]
    fn the_report_digest_is_stable_across_two_runs() {
        let mut a = Crucible::new(only_replay());
        let mut b = Crucible::new(only_replay());
        a.run();
        b.run();
        assert_eq!(a.report().digest(), b.report().digest());
    }
}
