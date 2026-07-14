//! The crucible's headless-friendly status overlay.
//! The engine debug-overlay module is browser/DOM-only, so a headless screenshot
//! cannot show text. This overlay therefore renders a report's key facts as
//! *geometry*: a replay-match beacon (green = match, red = diverged) and a row of
//! marker cubes counting live contacts. It also re-exposes the report's text lines
//! for the headless `main` and for tests. Pure translation — no physics, no state.

use axiom::prelude::Vec3;

use crate::crucible_report::CrucibleReport;
use crate::debug_geometry::{palette, RenderInstance};

/// The maximum number of contact tally markers drawn (keeps the bar bounded).
const MAX_TALLY: u32 = 12;

/// Status geometry for a report, anchored at `at` (world space): a replay-match
/// beacon plus a left-to-right tally of live contacts.
pub fn status_markers(report: &CrucibleReport, at: Vec3) -> Vec<RenderInstance> {
    let beacon_color = if report.replay_match() {
        palette::REPLAY_OK
    } else {
        palette::REPLAY_FAIL
    };
    let mut out = vec![RenderInstance::marker(at, beacon_color, 0.6)];
    let tally = report.live_contact_count().min(MAX_TALLY);
    for i in 0..tally {
        let position = at.add(Vec3::new(1.0 + i as f32 * 0.5, 0.0, 0.0));
        out.push(RenderInstance::marker(position, palette::CONTACT_POINT, 0.25));
    }
    out
}

/// The report's text lines (for the headless `main` and tests).
pub fn overlay_lines(report: &CrucibleReport) -> Vec<String> {
    report.lines()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crucible_report::{BodyState, ContactInfo, StepCounts};
    use axiom_physics::PhysicsBodyHandle;

    fn counts() -> StepCounts {
        StepCounts {
            step_index: 1,
            body_count: 1,
            collider_count: 1,
            dynamic_body_count: 1,
            command_count: 0,
            event_count: 1,
            integration_count: 1,
            broad_phase_pair_count: 0,
            contact_pair_count: 0,
            solved_contact_count: 0,
            frictioned_contact_count: 0,
            solver_iteration_count: 8,
            substep_count: 1,
        }
    }

    fn contact() -> ContactInfo {
        ContactInfo {
            body_a: PhysicsBodyHandle::from_raw(1),
            body_b: PhysicsBodyHandle::from_raw(2),
            normal: Vec3::UNIT_Y,
            depth: 0.1,
            point: Vec3::ZERO,
        }
    }

    fn state() -> BodyState {
        BodyState {
            handle: PhysicsBodyHandle::from_raw(1),
            translation: Vec3::ZERO,
            linear_velocity: Vec3::ZERO,
            rotation: [0.0, 0.0, 0.0, 1.0],
            angular: Vec3::ZERO,
            enabled: true,
        }
    }

    #[test]
    fn beacon_is_green_on_match_and_red_on_divergence() {
        let matched = CrucibleReport::build(1, &[state()], &counts(), &[], 0, true);
        assert_eq!(status_markers(&matched, Vec3::ZERO)[0].color, palette::REPLAY_OK);
        let diverged = CrucibleReport::build(1, &[state()], &counts(), &[], 0, false);
        assert_eq!(
            status_markers(&diverged, Vec3::ZERO)[0].color,
            palette::REPLAY_FAIL
        );
    }

    #[test]
    fn tally_has_one_marker_per_live_contact_plus_the_beacon() {
        let report = CrucibleReport::build(1, &[state()], &counts(), &[contact(), contact()], 0, true);
        let markers = status_markers(&report, Vec3::ZERO);
        assert_eq!(markers.len(), 3);
    }

    #[test]
    fn overlay_lines_delegate_to_the_report() {
        let report = CrucibleReport::build(1, &[state()], &counts(), &[], 0, true);
        assert_eq!(overlay_lines(&report), report.lines());
    }
}
