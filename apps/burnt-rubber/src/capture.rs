//! Deterministic capture slices for `axiom-shot`.
//!
//! Each slice poses the game at a specific, reproducible moment and hands the
//! posed [`RunningApp`] to the harness, which drives the single engine tick that
//! renders it. No browser, no wall clock, no input: the slice is built by
//! placing the car on a known part of the course, launching it at a known speed,
//! and running a known number of fixed steps under a known command. The same
//! slice therefore produces the same frame every time — which is what makes the
//! captures usable as visual-convergence references and as regression evidence.
//!
//! The slices are chosen to show the things the game is *about*: how fast it
//! looks, what the camera does under load, what a drift and a boost look like,
//! and how the enclosed sections read.

use axiom::prelude::RunningApp;

use crate::app::BurntRubber;
use crate::command::DriveCommand;
use crate::script;
use crate::sim::RacePhase;
use crate::track::SectionKind;
use crate::tuning::Tuning;

/// `axiom-shot` renders every registered slice at its own framebuffer size.
/// Building the app to match keeps the baked camera aspect from being stretched
/// by the capture framebuffer.
pub const CAPTURE_WIDTH: u32 = 960;
pub const CAPTURE_HEIGHT: u32 = 600;

/// Build the app framed at the start line, mid-countdown — the opening shot.
pub fn build_burnt_rubber_start_line() -> RunningApp {
    let mut app = sized();
    app.advance_steps(30, DriveCommand::IDLE);
    app.pose();
    app.into_running()
}

/// The opening straight at speed: the plainest possible read of how fast the
/// game looks, with nothing but road, posts and lane dashes in frame.
pub fn build_burnt_rubber_straight() -> RunningApp {
    let mut app = at_section(SectionKind::StartStraight, 78.0);
    app.advance_steps(45, DriveCommand::FLAT_OUT);
    app.pose();
    app.into_running()
}

/// Mid-corner in the coastal sweepers, on the racing line: the chase camera
/// doing its anticipation and roll.
pub fn build_burnt_rubber_sweeping_turn() -> RunningApp {
    let mut app = at_sharpest_corner(82.0);
    autopilot_for(&mut app, 40);
    app.pose();
    app.into_running()
}

/// A handbrake drift: the car pointing into the slide while travelling along
/// its velocity, with tyre smoke laid down behind it.
pub fn build_burnt_rubber_drift() -> RunningApp {
    let mut app = at_section(SectionKind::TechnicalBends, 62.0);
    app.advance_steps(
        50,
        DriveCommand {
            handbrake: true,
            ..DriveCommand::turning(0.9)
        },
    );
    app.pose();
    app.into_running()
}

/// Inside the tunnel: the enclosed section, its walls close and its ceiling
/// lights strobing past.
pub fn build_burnt_rubber_tunnel() -> RunningApp {
    let mut app = at_section(SectionKind::Tunnel, 84.0);
    autopilot_for(&mut app, 40);
    app.pose();
    app.into_running()
}

/// Threading traffic on the long straight, at the moment of a near miss.
pub fn build_burnt_rubber_traffic() -> RunningApp {
    let mut app = at_section(SectionKind::HighSpeedStraight, 88.0);
    // Run until a near miss registers, or give up and pose the traffic anyway —
    // the frame is worth capturing either way, and an unbounded wait is not.
    for _ in 0..NEAR_MISS_SEARCH_STEPS {
        let command = script::autopilot(app.sim().car(), app.sim().track());
        app.advance_steps(1, command);
        if app.sim().near_miss_notice() > 0 {
            break;
        }
    }
    app.pose();
    app.into_running()
}

/// Full boost through the canyon: the widest field of view, the most streaks,
/// and the walls closest to the camera.
pub fn build_burnt_rubber_boost() -> RunningApp {
    let mut app = at_section(SectionKind::Canyon, 86.0);
    for _ in 0..70 {
        let line = script::autopilot(app.sim().car(), app.sim().track());
        app.advance_steps(1, DriveCommand { boost: true, ..line });
    }
    app.pose();
    app.into_running()
}

/// How long the traffic slice hunts for a near miss before posing regardless.
const NEAR_MISS_SEARCH_STEPS: u32 = 900;

/// A capture-sized app on the shipping course.
fn sized() -> BurntRubber {
    BurntRubber::with(
        crate::DEFAULT_SEED,
        Tuning::DEFAULT,
        CAPTURE_WIDTH,
        CAPTURE_HEIGHT,
    )
}

/// An app placed partway into `section`, moving at `speed`, past the countdown.
fn at_section(section: SectionKind, speed: f32) -> BurntRubber {
    let mut app = sized();
    let distance = section_midpoint(&app, section);
    release_countdown(&mut app);
    app.sim_mut().place_at(distance);
    app.sim_mut().launch_at(speed);
    // A few steps so the traffic pool fills and the camera settles behind the
    // car rather than being snapped exactly onto it.
    autopilot_for(&mut app, 12);
    app
}

/// An app arriving at the course's sharpest corner at `speed`.
fn at_sharpest_corner(speed: f32) -> BurntRubber {
    let mut app = sized();
    let sharpest = app
        .sim()
        .track()
        .samples()
        .iter()
        .max_by(|a, b| a.curvature.abs().total_cmp(&b.curvature.abs()))
        .map(|s| s.distance)
        .unwrap_or(0.0);
    release_countdown(&mut app);
    app.sim_mut().place_at((sharpest - CORNER_ENTRY).max(0.0));
    app.sim_mut().launch_at(speed);
    app
}

/// How far before the apex a corner capture starts (m).
const CORNER_ENTRY: f32 = 55.0;

/// Step past the countdown so the car is free to drive.
fn release_countdown(app: &mut BurntRubber) {
    while app.sim().phase() == RacePhase::Countdown {
        app.advance_steps(1, DriveCommand::IDLE);
    }
}

/// Drive on the autopilot for `steps` fixed steps.
fn autopilot_for(app: &mut BurntRubber, steps: u32) {
    for _ in 0..steps {
        let command = script::autopilot(app.sim().car(), app.sim().track());
        app.advance_steps(1, command);
    }
}

/// The middle of the first stretch of `section` on the course.
fn section_midpoint(app: &BurntRubber, section: SectionKind) -> f32 {
    let track = app.sim().track();
    let matching: Vec<f32> = track
        .samples()
        .iter()
        .filter(|s| s.section == section)
        .map(|s| s.distance)
        .collect();
    matching
        .first()
        .zip(matching.last())
        .map(|(first, last)| (first + last) * 0.5)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every slice, paired with its registry name.
    fn slices() -> Vec<(&'static str, fn() -> RunningApp)> {
        vec![
            ("burnt-rubber", crate::app::build_burnt_rubber as fn() -> RunningApp),
            ("burnt-rubber-start-line", build_burnt_rubber_start_line),
            ("burnt-rubber-straight", build_burnt_rubber_straight),
            ("burnt-rubber-sweeping-turn", build_burnt_rubber_sweeping_turn),
            ("burnt-rubber-drift", build_burnt_rubber_drift),
            ("burnt-rubber-tunnel", build_burnt_rubber_tunnel),
            ("burnt-rubber-traffic", build_burnt_rubber_traffic),
            ("burnt-rubber-boost", build_burnt_rubber_boost),
        ]
    }

    #[test]
    fn every_slice_builds_a_frame_with_geometry_and_light() {
        for (name, build) in slices() {
            let mut app = build();
            let outcome = app.tick(0);
            assert!(!outcome.draws().is_empty(), "{name} drew nothing");
            assert!(!outcome.lights().is_empty(), "{name} is unlit");
            assert_ne!(outcome.camera_view_proj(), [0.0f32; 16], "{name} has no camera");
        }
    }

    /// The capture guarantee: rendering the same slice twice produces the same
    /// frame. This is what makes a screenshot a regression test rather than a
    /// picture.
    #[test]
    fn every_slice_renders_identically_twice() {
        for (name, build) in slices() {
            let first = {
                let mut app = build();
                let outcome = app.tick(0);
                (
                    outcome.draws().len(),
                    outcome.camera_view_proj(),
                    outcome.clear_color(),
                    outcome.instance_floats(),
                )
            };
            let second = {
                let mut app = build();
                let outcome = app.tick(0);
                (
                    outcome.draws().len(),
                    outcome.camera_view_proj(),
                    outcome.clear_color(),
                    outcome.instance_floats(),
                )
            };
            assert_eq!(first.0, second.0, "{name}: draw count differs");
            assert_eq!(first.1, second.1, "{name}: camera differs");
            assert_eq!(first.2, second.2, "{name}: clear colour differs");
            assert_eq!(
                first.3, second.3,
                "{name}: the instance data is not byte-identical"
            );
        }
    }

    #[test]
    fn each_slice_is_framed_where_it_says_it_is() {
        let mut app = at_section(SectionKind::Tunnel, 80.0);
        assert_eq!(app.sim().section(), SectionKind::Tunnel);
        assert!(app.sim().car().speed() > 40.0, "and it is moving");
        assert_eq!(app.sim().phase(), RacePhase::Racing);
        app.pose();

        let drift = at_section(SectionKind::TechnicalBends, 62.0);
        assert_eq!(drift.sim().section(), SectionKind::TechnicalBends);
    }

    #[test]
    fn the_drift_slice_is_actually_drifting() {
        let mut app = at_section(SectionKind::TechnicalBends, 62.0);
        app.advance_steps(
            50,
            DriveCommand {
                handbrake: true,
                ..DriveCommand::turning(0.9)
            },
        );
        assert!(app.sim().car().drifting, "the drift slice shows a drift");
        assert!(app.sim().car().slide_ratio() > 0.05);
    }

    #[test]
    fn the_boost_slice_is_actually_boosting() {
        let mut app = at_section(SectionKind::Canyon, 86.0);
        for _ in 0..70 {
            let line = script::autopilot(app.sim().car(), app.sim().track());
            app.advance_steps(1, DriveCommand { boost: true, ..line });
        }
        // The meter starts partly charged, so a 70-step boost is genuinely on.
        assert!(
            app.sim().boost().active() || app.sim().boost().charge() == 0.0,
            "the boost slice spent the meter"
        );
        let fov = app.sim().camera_pose(1.0).fov_degrees;
        assert!(fov > Tuning::DEFAULT.camera.fov_low + 10.0, "and the view widened: {fov}");
    }

    #[test]
    fn the_traffic_slice_terminates_whether_or_not_it_finds_a_near_miss() {
        // The search is bounded, so this returning at all is the assertion.
        let mut app = build_burnt_rubber_traffic();
        let outcome = app.tick(0);
        assert!(!outcome.draws().is_empty());
    }

    #[test]
    fn section_lookup_finds_every_section_and_falls_back_safely() {
        let app = sized();
        for section in SectionKind::ALL {
            let midpoint = section_midpoint(&app, section);
            assert!(midpoint >= 0.0 && midpoint <= app.sim().track().length());
            assert_eq!(
                app.sim().track().sample_at(midpoint).section,
                section,
                "{section:?} midpoint lands in its own section"
            );
        }
    }

    #[test]
    fn the_capture_framebuffer_is_the_harness_size() {
        assert_eq!((CAPTURE_WIDTH, CAPTURE_HEIGHT), (960, 600));
    }
}
