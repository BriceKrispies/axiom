//! The racing HUD, as **data**.
//!
//! [`HudModel`] is a pure value computed from simulation state: no DOM, no
//! strings baked into the simulation, no browser anywhere. The `wasm32` arm
//! renders it into a small overlay; the native tests assert on it directly.
//! That split is what lets "the speedometer reads the actual simulation speed"
//! and "the progress bar reaches 100% at the finish" be tested at all.
//!
//! The unit conversion lives here and only here. The simulation thinks in metres
//! per second because everything else in the engine does; the player is shown
//! km/h because a number that reads 331 at full boost is more exciting than one
//! that reads 92, and excitement is the entire brief.

use crate::sim::{RacePhase, RaceSim};
use crate::track::SectionKind;

/// Metres per second to kilometres per hour.
pub const KMH_PER_MS: f32 = 3.6;

/// Everything the HUD shows this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HudModel {
    /// Speed in km/h, rounded for display.
    pub speed_kmh: u32,
    /// Boost charge, `0..1`.
    pub boost: f32,
    /// Course progress, `0..1`.
    pub progress: f32,
    /// The section the car is in.
    pub section: SectionKind,
    /// Elapsed race time in seconds.
    pub elapsed: f32,
    /// The countdown number to show (`0` = none).
    pub countdown: u32,
    /// Whether the "GO" banner should be up.
    pub go: bool,
    /// Whether a near-miss notification is showing.
    pub near_miss: bool,
    /// Total near misses this run.
    pub near_miss_count: u32,
    /// Whether the car is drifting (the drift-boost indicator).
    pub drifting: bool,
    /// Whether boost is being spent.
    pub boosting: bool,
    /// Whether the car is off the tarmac.
    pub off_road: bool,
    /// Whether the "press R to reset" prompt should be up.
    pub stuck: bool,
    /// The run phase.
    pub phase: RacePhase,
    /// Whether this is early enough in the run to still show the controls hint.
    pub show_controls_hint: bool,
    /// How far ahead of the agent's ghost the player is, in metres — negative
    /// when the ghost is winning. `None` when no ghost is running.
    pub ghost_delta_metres: Option<f32>,
}

impl HudModel {
    /// Read the HUD out of the simulation.
    pub fn of(sim: &RaceSim) -> HudModel {
        let car = sim.car();
        HudModel {
            speed_kmh: (car.speed() * KMH_PER_MS).round().max(0.0) as u32,
            boost: sim.boost().charge(),
            progress: sim.progress(),
            section: sim.section(),
            elapsed: sim.elapsed_seconds(),
            countdown: sim.countdown_number(),
            go: sim.phase() == RacePhase::Racing && sim.go_banner() > 0,
            near_miss: sim.near_miss_notice() > 0,
            near_miss_count: sim.near_miss_count(),
            drifting: car.drifting,
            boosting: sim.boost().active(),
            off_road: car.surface.is_off_road(),
            stuck: sim.is_stuck(),
            phase: sim.phase(),
            show_controls_hint: sim.step_count() < CONTROLS_HINT_STEPS,
            // Filled in by `with_ghost_delta` — the ghost is not in this sim.
            ghost_delta_metres: None,
        }
    }

    /// The elapsed time as `M:SS.mmm`, the format the finish panel shows.
    /// The same model with the ghost gap filled in. The gap is the one number
    /// on the HUD that does not come from the player's simulation — the ghost
    /// runs in its own — so it is attached here rather than smuggled into
    /// [`Self::of`].
    pub const fn with_ghost_delta(mut self, delta: Option<f32>) -> HudModel {
        self.ghost_delta_metres = delta;
        self
    }

    /// The ghost gap as it is read out: `+12 m` when the player leads, `-12 m`
    /// when the ghost does. `None` when there is no ghost.
    pub fn formatted_ghost_delta(&self) -> Option<String> {
        self.ghost_delta_metres
            .map(|d| format!("{}{:.0} m", ["-", "+"][usize::from(d >= 0.0)], d.abs()))
    }

    pub fn formatted_time(&self) -> String {
        let total = self.elapsed.max(0.0);
        let minutes = (total / 60.0).floor() as u32;
        let seconds = total - minutes as f32 * 60.0;
        format!("{minutes}:{seconds:06.3}")
    }

    /// Progress as a whole percentage.
    pub fn progress_percent(&self) -> u32 {
        (self.progress * 100.0).round().clamp(0.0, 100.0) as u32
    }

    /// The single banner line, if any, that should sit across the middle of the
    /// screen. Kept to one line so the HUD never covers the road.
    pub fn banner(&self) -> Option<String> {
        match self.phase {
            RacePhase::Paused => Some("PAUSED".to_string()),
            RacePhase::Finished => Some(format!("FINISH  {}", self.formatted_time())),
            _ if self.countdown > 0 => Some(self.countdown.to_string()),
            _ if self.go => Some("GO".to_string()),
            // A near miss deliberately does *not* banner. It is the most
            // frequent event in the game — 47 of them in a clean lap — and a
            // 64px word across the middle of the screen every time you thread a
            // car is the one piece of the HUD that actively covers the road you
            // are threading it on. The boost meter filling is the feedback.
            _ if self.stuck => Some("PRESS R TO RESET".to_string()),
            _ => None,
        }
    }
}

/// How long the "GO" banner stays up after the countdown (steps).
pub const GO_BANNER_STEPS: u32 = 60;

/// How long the first-run controls hint stays up (steps).
pub const CONTROLS_HINT_STEPS: u64 = 600;

/// The controls hint, as one line.
pub const CONTROLS_HINT: &str =
    "W/S drive · A/D steer · SPACE handbrake · SHIFT boost · R reset · ESC pause";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::DriveCommand;

    fn racing() -> RaceSim {
        let mut sim = RaceSim::shipping();
        while sim.phase() == RacePhase::Countdown {
            sim.step(DriveCommand::IDLE);
        }
        sim
    }

    #[test]
    fn the_speedometer_reads_the_simulation_speed() {
        let mut sim = racing();
        for _ in 0..600 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        let hud = HudModel::of(&sim);
        let expected = (sim.car().speed() * KMH_PER_MS).round() as u32;
        assert_eq!(hud.speed_kmh, expected);
        assert!(hud.speed_kmh > 150, "and it is an exciting number: {}", hud.speed_kmh);
    }

    #[test]
    fn the_countdown_shows_then_gives_way_to_go() {
        let mut sim = RaceSim::shipping();
        assert_eq!(HudModel::of(&sim).countdown, 3);
        assert_eq!(HudModel::of(&sim).banner().as_deref(), Some("3"));
        while sim.phase() == RacePhase::Countdown {
            sim.step(DriveCommand::IDLE);
        }
        let hud = HudModel::of(&sim);
        assert_eq!(hud.countdown, 0);
        assert!(hud.go);
        assert_eq!(hud.banner().as_deref(), Some("GO"));
    }

    #[test]
    fn progress_runs_from_zero_to_a_hundred() {
        let mut sim = racing();
        assert_eq!(HudModel::of(&sim).progress_percent(), 0);
        for _ in 0..1_200 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        let mid = HudModel::of(&sim).progress_percent();
        assert!(mid > 0 && mid < 100, "partway along: {mid}%");

        sim.place_at(sim.track().length());
        sim.step(DriveCommand::IDLE);
        assert_eq!(HudModel::of(&sim).progress_percent(), 100);
    }

    #[test]
    fn the_section_name_tracks_the_course() {
        let mut sim = racing();
        assert_eq!(HudModel::of(&sim).section, SectionKind::StartStraight);
        assert_eq!(HudModel::of(&sim).section.name(), "OPENING STRAIGHT");
        sim.place_at(sim.track().length() * 0.5);
        sim.step(DriveCommand::IDLE);
        assert_ne!(HudModel::of(&sim).section, SectionKind::StartStraight);
    }

    #[test]
    fn the_time_is_formatted_as_minutes_and_seconds() {
        let mut hud = HudModel::of(&RaceSim::shipping());
        hud.elapsed = 0.0;
        assert_eq!(hud.formatted_time(), "0:00.000");
        hud.elapsed = 9.5;
        assert_eq!(hud.formatted_time(), "0:09.500");
        hud.elapsed = 125.25;
        assert_eq!(hud.formatted_time(), "2:05.250");
        hud.elapsed = -4.0;
        assert_eq!(hud.formatted_time(), "0:00.000", "a negative time is clamped");
    }

    #[test]
    fn the_banner_prioritises_the_most_important_message() {
        let mut hud = HudModel::of(&RaceSim::shipping());
        hud.countdown = 0;
        hud.go = false;
        hud.near_miss = false;
        hud.stuck = false;
        hud.phase = RacePhase::Racing;
        assert_eq!(hud.banner(), None, "ordinary driving shows nothing");

        hud.stuck = true;
        assert_eq!(hud.banner().as_deref(), Some("PRESS R TO RESET"));
        // A near miss never banners — it would cover the road it happens on.
        hud.near_miss = true;
        assert_eq!(
            hud.banner().as_deref(),
            Some("PRESS R TO RESET"),
            "a near miss must not take the banner"
        );
        hud.countdown = 2;
        assert_eq!(hud.banner().as_deref(), Some("2"));

        hud.phase = RacePhase::Paused;
        assert_eq!(hud.banner().as_deref(), Some("PAUSED"), "pause wins everything");
        hud.phase = RacePhase::Finished;
        hud.elapsed = 61.0;
        assert_eq!(hud.banner().as_deref(), Some("FINISH  1:01.000"));
    }

    #[test]
    fn the_controls_hint_shows_early_and_then_stops() {
        let mut sim = racing();
        assert!(HudModel::of(&sim).show_controls_hint);
        assert!(!CONTROLS_HINT.is_empty());
        for _ in 0..CONTROLS_HINT_STEPS {
            sim.step(DriveCommand::FLAT_OUT);
        }
        assert!(!HudModel::of(&sim).show_controls_hint);
    }

    #[test]
    fn progress_percent_is_clamped_at_both_ends() {
        let mut hud = HudModel::of(&RaceSim::shipping());
        hud.progress = -5.0;
        assert_eq!(hud.progress_percent(), 0);
        hud.progress = 5.0;
        assert_eq!(hud.progress_percent(), 100);
    }

    #[test]
    fn the_hud_reports_the_driving_state_flags() {
        let mut sim = racing();
        for _ in 0..240 {
            sim.step(DriveCommand::FLAT_OUT);
        }
        for _ in 0..40 {
            sim.step(DriveCommand {
                handbrake: true,
                ..DriveCommand::turning(1.0)
            });
        }
        let hud = HudModel::of(&sim);
        assert!(hud.drifting, "the handbrake turn is a drift");
        assert_eq!(hud.boost, sim.boost().charge());
        assert_eq!(hud.off_road, sim.car().surface.is_off_road());
        assert_eq!(hud.phase, sim.phase());
    }
}
