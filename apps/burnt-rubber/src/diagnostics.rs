//! App-level telemetry: the counters that answer "why is this frame slow" and
//! "is the simulation still sane".
//!
//! These are **structured values**, not printed lines. Nothing here writes to a
//! console: the browser arm renders them into the debug overlay, the tests
//! assert on them, and the capture harness can record them. That is the whole
//! difference between telemetry and `println!` — a number you can assert on
//! versus a string you can only read.
//!
//! Everything is a read of state that already exists. Collecting diagnostics
//! must never change what the frame does, and must never be the reason a value
//! is computed, or the "diagnostics off" build would behave differently from the
//! "diagnostics on" one.

use crate::render::{RaceScene, SceneCounters};
use crate::sim::{RacePhase, RaceSim};

/// One frame's telemetry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Diagnostics {
    /// What the scene drew.
    pub scene: SceneCounters,
    /// Live traffic vehicles.
    pub active_traffic: usize,
    /// Fixed simulation steps taken this run.
    pub simulation_steps: u64,
    /// Current ground speed (m/s).
    pub speed_ms: f32,
    /// Course progress, `0..1`.
    pub progress: f32,
    /// Boost charge, `0..1`.
    pub boost: f32,
    /// Distance along the course (m).
    pub distance_m: f32,
    /// Lateral offset from the road centre (m).
    pub lateral_m: f32,
    /// The run phase.
    pub phase: RacePhase,
    /// Near misses so far.
    pub near_misses: u32,
    /// Impacts so far.
    pub impacts: u32,
    /// Whether the car is airborne.
    pub airborne: bool,
    /// Whether the car is drifting.
    pub drifting: bool,
}

impl Diagnostics {
    /// Empty telemetry, before the first frame.
    pub const fn new() -> Diagnostics {
        Diagnostics {
            scene: SceneCounters {
                road_draws: 0,
                total_road_draws: 0,
                road_triangles: 0,
                scenery_instances: 0,
                cached_scenery_chunks: 0,
                effect_instances: 0,
                traffic_slots: 0,
                pickup_bodies: 0,
            },
            active_traffic: 0,
            simulation_steps: 0,
            speed_ms: 0.0,
            progress: 0.0,
            boost: 0.0,
            distance_m: 0.0,
            lateral_m: 0.0,
            phase: RacePhase::Countdown,
            near_misses: 0,
            impacts: 0,
            airborne: false,
            drifting: false,
        }
    }

    /// Read this frame's telemetry out of the simulation and the scene.
    pub fn observe(&mut self, sim: &RaceSim, scene: &RaceScene) {
        let car = sim.car();
        *self = Diagnostics {
            scene: scene.counters(),
            active_traffic: sim.traffic().active_count(),
            simulation_steps: sim.step_count(),
            speed_ms: car.speed(),
            progress: sim.progress(),
            boost: sim.boost().charge(),
            distance_m: car.distance,
            lateral_m: car.lateral,
            phase: sim.phase(),
            near_misses: sim.near_miss_count(),
            impacts: sim.impact_count(),
            airborne: !car.grounded,
            drifting: car.drifting,
        };
    }

    /// The telemetry as ordered `(label, value)` rows — the shape the debug
    /// overlay consumes, and a stable order so the overlay never reflows.
    pub fn rows(&self) -> Vec<(String, String)> {
        vec![
            ("speed".into(), format!("{:.1} m/s", self.speed_ms)),
            (
                "progress".into(),
                format!("{:.1}% ({:.0} m)", self.progress * 100.0, self.distance_m),
            ),
            ("lateral".into(), format!("{:+.2} m", self.lateral_m)),
            ("boost".into(), format!("{:.0}%", self.boost * 100.0)),
            ("phase".into(), format!("{:?}", self.phase)),
            ("steps".into(), self.simulation_steps.to_string()),
            (
                "chunks".into(),
                format!("{}/{}", self.scene.road_draws, self.scene.total_road_draws),
            ),
            (
                "road tris".into(),
                format!("{} total", self.scene.road_triangles),
            ),
            (
                "scenery".into(),
                format!(
                    "{} drawn / {} chunks",
                    self.scene.scenery_instances, self.scene.cached_scenery_chunks
                ),
            ),
            (
                "traffic".into(),
                format!("{}/{}", self.active_traffic, self.scene.traffic_slots),
            ),
            ("effects".into(), self.scene.effect_instances.to_string()),
            (
                "near miss / impact".into(),
                format!("{} / {}", self.near_misses, self.impacts),
            ),
            (
                "state".into(),
                format!(
                    "{}{}",
                    if self.airborne { "air " } else { "" },
                    if self.drifting { "drift" } else { "grip" }
                ),
            ),
        ]
    }
}

impl Default for Diagnostics {
    fn default() -> Self {
        Diagnostics::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::BurntRubber;
    use crate::command::DriveCommand;

    #[test]
    fn fresh_diagnostics_are_empty() {
        let d = Diagnostics::new();
        assert_eq!(d.simulation_steps, 0);
        assert_eq!(d.scene.road_draws, 0);
        assert_eq!(d, Diagnostics::default());
    }

    #[test]
    fn observing_reads_the_whole_frame() {
        let mut app = BurntRubber::with(crate::DEFAULT_SEED, crate::Tuning::DEFAULT, crate::WIDTH, crate::HEIGHT);
        while app.sim().phase() == RacePhase::Countdown {
            app.advance_steps(1, DriveCommand::IDLE);
        }
        app.advance_steps(900, DriveCommand::FLAT_OUT);
        app.present();

        let d = *app.diagnostics();
        assert_eq!(d.simulation_steps, app.sim().step_count());
        assert!((d.speed_ms - app.sim().car().speed()).abs() < 1.0e-4);
        assert!((d.progress - app.sim().progress()).abs() < 1.0e-6);
        assert_eq!(d.phase, app.sim().phase());
        assert_eq!(d.near_misses, app.sim().near_miss_count());
        assert!(d.scene.road_draws > 0);
        assert!(d.scene.road_triangles > 0);
        assert!(d.active_traffic > 0, "the traffic is counted");
    }

    /// Collecting telemetry must not change the frame.
    #[test]
    fn observing_does_not_disturb_the_simulation() {
        let mut app = BurntRubber::with(crate::DEFAULT_SEED, crate::Tuning::DEFAULT, crate::WIDTH, crate::HEIGHT);
        app.advance_steps(400, DriveCommand::FLAT_OUT);
        let before = *app.sim().car();
        app.pose();
        app.pose();
        app.pose();
        assert_eq!(*app.sim().car(), before);
    }

    #[test]
    fn the_rows_are_stable_labelled_and_complete() {
        let mut app = BurntRubber::with(crate::DEFAULT_SEED, crate::Tuning::DEFAULT, crate::WIDTH, crate::HEIGHT);
        app.advance_steps(300, DriveCommand::FLAT_OUT);
        app.present();
        let rows = app.diagnostics().rows();
        assert!(rows.len() >= 12, "every counter is reported: {}", rows.len());
        for (label, value) in &rows {
            assert!(!label.is_empty());
            assert!(!value.is_empty(), "{label} has no value");
        }
        // The order is stable, so the overlay does not reflow between frames.
        let labels: Vec<&str> = rows.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels[0], "speed");
        assert_eq!(labels[1], "progress");
        app.present();
        let again: Vec<String> = app.diagnostics().rows().into_iter().map(|(l, _)| l).collect();
        assert_eq!(labels, again.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    }

    #[test]
    fn the_state_row_reports_drifting_and_airborne() {
        let mut d = Diagnostics::new();
        d.drifting = true;
        assert!(d.rows().iter().any(|(_, v)| v.contains("drift")));
        d.drifting = false;
        assert!(d.rows().iter().any(|(_, v)| v.contains("grip")));
        d.airborne = true;
        assert!(d.rows().iter().any(|(_, v)| v.contains("air")));
    }
}
