//! **The canonical golden run** — the agent plays the shipping course, and five
//! deterministic checkpoints along that one race are the app's regression
//! baseline.
//!
//! # What this is, and why it is not another capture slice
//!
//! [`crate::capture`] poses the game *at* a moment: it teleports the car to a
//! known point on the course, launches it at a known speed, and photographs what
//! that looks like. Each slice is a still life, and each is independent of every
//! other. That is the right shape for a visual reference, and the wrong shape
//! for a regression baseline — a teleport skips the game.
//!
//! This module is the other thing: **one continuous race**, from the grid to the
//! finish arch, driven end to end by the real [`crate::agent`] driver through
//! `axiom-agent`, with five checkpoints read out of that single run. Nothing is
//! placed, nothing is launched, no command is hand-written. The car reaches
//! 3 800 steps because it *drove* there, past every corner, every traffic car
//! and every boost pad on the way, and the frame captured at that step is
//! therefore evidence about the whole preceding race rather than about a pose.
//!
//! That is what makes it usable as a before/after fixture for a change that
//! moves work between lifecycle phases: if the same seed and the same driver
//! produce a different car, a different course or a different frame at any
//! checkpoint, the change altered the game.
//!
//! # What is pinned
//!
//! Everything needed to reproduce the run is a constant in this file —
//! [`GOLDEN_SEED`], [`GOLDEN_PROFILE`], [`GOLDEN_TUNING`], the framebuffer, the
//! driver technique, the step limit and the checkpoint stops. The run takes no
//! parameters, reads no clock, and consumes no input: `driven_to(stop)` is a
//! pure function of the constants below.
//!
//! `tests/agent_golden.rs` encodes each checkpoint's **simulation state** and
//! **render boundary** into committed golden bytes; the checkpoints are also
//! registered in `axiom-shot`, so the same five moments can be rendered as real
//! pixels through the real backend.

use axiom::prelude::RunningApp;

use crate::agent::{drive_one_step, DriverTuning};
use crate::app::BurntRubber;
use crate::sim::RacePhase;
use crate::tuning::Tuning;
use crate::PlayProfile;

/// The course everyone drives. The golden run is the shipping game, not a
/// fixture course invented for the test.
pub const GOLDEN_SEED: u64 = crate::DEFAULT_SEED;

/// The wheel game — continuous steering, the profile the desktop build plays and
/// the one the agent's [`DriverTuning::FAST`] technique was measured on.
pub const GOLDEN_PROFILE: PlayProfile = PlayProfile::Wheel;

/// The shipping feel. A tuning edit is a game change and must move the goldens.
pub const GOLDEN_TUNING: Tuning = Tuning::DEFAULT;

/// The capture framebuffer, matched to `axiom-shot`'s so the baked camera aspect
/// is not stretched by the capture surface. Same numbers as
/// [`crate::capture::CAPTURE_WIDTH`] for the same reason.
pub const GOLDEN_WIDTH: u32 = crate::capture::CAPTURE_WIDTH;
/// See [`GOLDEN_WIDTH`].
pub const GOLDEN_HEIGHT: u32 = crate::capture::CAPTURE_HEIGHT;

/// The driver's technique. The agent races on the same numbers the shipping
/// ghost does.
pub const GOLDEN_DRIVER: DriverTuning = DriverTuning::FAST;

/// Five minutes of simulated racing — far beyond the ~90 s the agent needs, so a
/// run that hits this cap is a genuine failure ("the agent never finished"),
/// never a tight budget.
pub const GOLDEN_STEP_LIMIT: u32 = 60 * 60 * 5;

/// Where a checkpoint stops the run.
///
/// Four are fixed step indices — a *logical* simulation coordinate, not a wall
/// clock — and the last is a game-state condition, so the final checkpoint is
/// always the real end of the race however the driver's lap time moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoldenStop {
    /// Stop after exactly this many fixed 60 Hz steps.
    Step(u32),
    /// Stop the step the race reaches [`RacePhase::Finished`].
    Finish,
}

/// One checkpoint of the golden run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoldenCheckpoint {
    /// The golden artifact's stem, and the checkpoint's name in the test output.
    pub name: &'static str,
    /// The name `axiom-shot` renders this checkpoint under.
    pub slice: &'static str,
    /// Where in the run it is taken.
    pub stop: GoldenStop,
}

/// The five checkpoints, in run order.
///
/// The step indices are chosen off the measured race (the agent finishes the
/// nine kilometres in ~5 400 steps) so that each lands in a different part of
/// the course and a different part of the driver's behaviour — not at even
/// intervals for their own sake:
///
/// | Checkpoint | Step | Where the agent is |
/// |---|---|---|
/// | `grid` | 0 | Held on the grid, counting in — the first stable frame, before any driving |
/// | `opening` | 700 | Off the line and up to speed on the coastal sweepers |
/// | `esses` | 2200 | Mid-run, working through the ridge crests and the technical bends |
/// | `canyon` | 3800 | Late, flat out, deep in the boost meter's spend |
/// | `finish` | — | The step it crosses the line |
pub const CHECKPOINTS: [GoldenCheckpoint; 5] = [
    GoldenCheckpoint {
        name: "grid",
        slice: "burnt-rubber-golden-grid",
        stop: GoldenStop::Step(0),
    },
    GoldenCheckpoint {
        name: "opening",
        slice: "burnt-rubber-golden-opening",
        stop: GoldenStop::Step(700),
    },
    GoldenCheckpoint {
        name: "esses",
        slice: "burnt-rubber-golden-esses",
        stop: GoldenStop::Step(2200),
    },
    GoldenCheckpoint {
        name: "canyon",
        slice: "burnt-rubber-golden-canyon",
        stop: GoldenStop::Step(3800),
    },
    GoldenCheckpoint {
        name: "finish",
        slice: "burnt-rubber-golden-finish",
        stop: GoldenStop::Finish,
    },
];

/// The simulation state at a checkpoint — the *gameplay* half of the baseline.
///
/// Every field is read from a public accessor the game itself uses, so this
/// record cannot drift away from what the game does. It is deliberately wider
/// than "did it crash": a change that leaves the car in the same place but
/// spends a different amount of boost getting there, or threads two fewer cars,
/// has changed the game and must fail.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GoldenState {
    /// Fixed steps run to reach this checkpoint.
    pub steps: u32,
    /// The simulation's own step counter (the countdown is included).
    pub sim_steps: u64,
    /// Race phase, as its position in the declaration order.
    pub phase: u32,
    /// Race time in seconds — a step count, never a clock reading.
    pub elapsed_seconds: f32,
    /// Arc length along the course (m).
    pub distance: f32,
    /// Signed offset from the road centre (m).
    pub lateral: f32,
    /// Chassis heading (radians).
    pub yaw: f32,
    /// Ground speed (m/s).
    pub speed: f32,
    /// Chassis centre in world space.
    pub position: [f32; 3],
    /// Course progress, `0..1`.
    pub progress: f32,
    /// The section the car is in, as its index in `SectionKind::ALL`.
    pub section: u32,
    /// Boost meter charge, `0..1`, and whether it is lit.
    pub boost_charge: f32,
    pub boost_active: bool,
    /// Traffic threaded, and things hit, since the start of the race.
    pub near_misses: u32,
    pub impacts: u32,
    /// The highest ground speed reached so far (m/s).
    pub top_speed: f32,
    /// The chase camera, at zero interpolation.
    pub camera_eye: [f32; 3],
    pub camera_target: [f32; 3],
    pub camera_fov_degrees: f32,
    pub camera_roll: f32,
    /// How far ahead of the agent's ghost the player is (m), or `None` if there
    /// is no ghost. The player *is* the agent here, so this pins that the ghost
    /// tracks the same driver.
    ///
    /// Honestly `Option`, not a NaN sentinel. `GoldenState` derives `PartialEq`
    /// and is compared by `the_golden_run_replays_identically`; a NaN in a
    /// `PartialEq` struct is never equal to itself, so a run that lost its ghost
    /// would fail that test with "checkpoint is not deterministic" while the byte
    /// goldens passed — a maximally confusing diagnosis pointing at the wrong
    /// thing.
    pub ghost_delta: Option<f32>,
}

/// Drive the golden run to `stop` and return the app there, unposed.
///
/// One race, one driver, one command per step, and the command is the only thing
/// the simulation is given — exactly as [`crate::agent::race`] does, but through
/// the whole app so the scene, the ghost and the render boundary advance with
/// it.
pub fn driven_to(stop: GoldenStop) -> BurntRubber {
    driven_with_count(stop).0
}

/// Whether the run has reached `stop` after `steps` steps.
///
/// The step limit is enforced for both stops, so no golden capture can hang: a
/// `Finish` that never arrives ends at the cap and the state record shows a
/// non-finished phase, which the test asserts against.
fn stopped(app: &BurntRubber, stop: GoldenStop, steps: u32) -> bool {
    if steps >= GOLDEN_STEP_LIMIT {
        return true;
    }
    match stop {
        GoldenStop::Step(target) => steps >= target,
        GoldenStop::Finish => app.sim().phase() == RacePhase::Finished,
    }
}

/// Read the deterministic simulation state of a driven app.
pub fn state_of(app: &BurntRubber, steps: u32) -> GoldenState {
    let sim = app.sim();
    let car = sim.car();
    let camera = sim.camera_pose(0.0);
    GoldenState {
        steps,
        sim_steps: sim.step_count(),
        phase: phase_index(sim.phase()),
        elapsed_seconds: sim.elapsed_seconds(),
        distance: car.distance,
        lateral: car.lateral,
        yaw: car.yaw,
        speed: car.speed(),
        position: [car.position.x, car.position.y, car.position.z],
        progress: sim.progress(),
        section: section_index(sim.section()),
        boost_charge: sim.boost().charge(),
        boost_active: sim.boost().active(),
        near_misses: sim.near_miss_count(),
        impacts: sim.impact_count(),
        top_speed: sim.top_speed_seen(),
        camera_eye: [camera.eye.x, camera.eye.y, camera.eye.z],
        camera_target: [camera.target.x, camera.target.y, camera.target.z],
        camera_fov_degrees: camera.fov_degrees,
        camera_roll: camera.roll,
        ghost_delta: app.ghost_delta_metres(),
    }
}

/// Drive to `stop` and read the state there — the pairing the test wants.
pub fn state_at(stop: GoldenStop) -> GoldenState {
    let (app, steps) = driven_with_count(stop);
    state_of(&app, steps)
}

/// [`driven_to`], also reporting how many steps it took (the `Finish` stop's
/// step count is the lap, and is itself part of the baseline).
pub fn driven_with_count(stop: GoldenStop) -> (BurntRubber, u32) {
    let mut app = BurntRubber::with_profile(
        GOLDEN_SEED,
        GOLDEN_TUNING,
        GOLDEN_WIDTH,
        GOLDEN_HEIGHT,
        GOLDEN_PROFILE,
    );
    let mut steps = 0u32;
    while !stopped(&app, stop, steps) {
        let command = drive_one_step(app.sim(), &GOLDEN_DRIVER, u64::from(steps));
        app.advance_steps(1, command.0);
        steps += 1;
    }
    (app, steps)
}

/// `RacePhase`'s position in its declaration order — a stable integer for the
/// golden encoding, so a reordered enum is caught rather than silently re-blessed.
const fn phase_index(phase: RacePhase) -> u32 {
    match phase {
        RacePhase::Countdown => 0,
        RacePhase::Racing => 1,
        RacePhase::Finished => 2,
        RacePhase::Paused => 3,
    }
}

/// `SectionKind`'s index in `SectionKind::ALL`.
fn section_index(section: crate::SectionKind) -> u32 {
    crate::SectionKind::ALL
        .iter()
        .position(|&s| s == section)
        .unwrap_or(usize::MAX) as u32
}

/// Drive to `stop`, pose the scene, and hand the engine app to a capture
/// harness — the shape every `axiom-shot` slice builder has.
fn posed(stop: GoldenStop) -> RunningApp {
    let mut app = driven_to(stop);
    app.pose();
    app.into_running()
}

/// Checkpoint 1 — held on the grid, counting in. The first stable frame, before
/// any meaningful driving.
pub fn build_golden_grid() -> RunningApp {
    posed(CHECKPOINTS[0].stop)
}

/// Checkpoint 2 — early race: off the line and up to speed on the sweepers.
pub fn build_golden_opening() -> RunningApp {
    posed(CHECKPOINTS[1].stop)
}

/// Checkpoint 3 — mid-run: the ridge crests and the technical bends.
pub fn build_golden_esses() -> RunningApp {
    posed(CHECKPOINTS[2].stop)
}

/// Checkpoint 4 — late run at high speed, deep into the boost meter.
pub fn build_golden_canyon() -> RunningApp {
    posed(CHECKPOINTS[3].stop)
}

/// Checkpoint 5 — the final deterministic checkpoint: the step it crosses the
/// line.
pub fn build_golden_finish() -> RunningApp {
    posed(CHECKPOINTS[4].stop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::DriveCommand;

    /// The run's identity. A change to any of these is a change to what the
    /// baseline *means*, and must be a deliberate edit here rather than a drift.
    #[test]
    fn the_golden_run_is_pinned_to_the_shipping_game() {
        assert_eq!(GOLDEN_SEED, crate::DEFAULT_SEED);
        assert_eq!(GOLDEN_PROFILE, PlayProfile::Wheel);
        assert_eq!((GOLDEN_WIDTH, GOLDEN_HEIGHT), (960, 600));
        assert_eq!(GOLDEN_DRIVER, DriverTuning::FAST);
        assert_eq!(GOLDEN_STEP_LIMIT, 18_000);
    }

    /// Five checkpoints, uniquely named, in strictly increasing run order, and
    /// the last one is the game-state stop rather than a step index.
    #[test]
    fn the_checkpoints_are_ordered_and_distinct() {
        let steps: Vec<u32> = CHECKPOINTS
            .iter()
            .filter_map(|c| match c.stop {
                GoldenStop::Step(n) => Some(n),
                GoldenStop::Finish => None,
            })
            .collect();
        assert_eq!(steps, vec![0, 700, 2200, 3800]);
        assert_eq!(CHECKPOINTS[4].stop, GoldenStop::Finish);
        let names: std::collections::BTreeSet<&str> =
            CHECKPOINTS.iter().map(|c| c.name).collect();
        assert_eq!(names.len(), CHECKPOINTS.len(), "checkpoint names are unique");
        let slices: std::collections::BTreeSet<&str> =
            CHECKPOINTS.iter().map(|c| c.slice).collect();
        assert_eq!(slices.len(), CHECKPOINTS.len(), "slice names are unique");
        assert!(
            CHECKPOINTS.iter().all(|c| c.slice.starts_with("burnt-rubber-golden-")),
            "every checkpoint is registered under the golden prefix"
        );
    }

    /// The grid checkpoint is the *pre-driving* frame: the countdown is still
    /// running, the car has not moved, and no step has been taken.
    #[test]
    fn the_grid_checkpoint_is_before_any_driving() {
        let state = state_at(GoldenStop::Step(0));
        assert_eq!(state.steps, 0);
        assert_eq!(state.phase, phase_index(RacePhase::Countdown));
        assert_eq!(state.speed, 0.0, "the car is held");
        assert_eq!(state.near_misses, 0);
        assert_eq!(state.impacts, 0);
    }

    /// Each step checkpoint is genuinely further into the race than the one
    /// before it, at racing speed, on the road. This is what makes the five
    /// captures five *different* moments rather than five pictures of the grid.
    #[test]
    fn each_checkpoint_is_further_into_a_real_race() {
        let states: Vec<GoldenState> =
            CHECKPOINTS.iter().map(|c| state_at(c.stop)).collect();
        states.windows(2).for_each(|pair| {
            assert!(
                pair[1].distance > pair[0].distance,
                "checkpoints advance down the course: {} then {}",
                pair[0].distance,
                pair[1].distance
            );
            assert!(pair[1].progress >= pair[0].progress);
        });
        // The three mid-race checkpoints are driving, fast, and finite.
        states[1..4].iter().for_each(|s| {
            assert_eq!(s.phase, phase_index(RacePhase::Racing));
            assert!(s.speed > 40.0, "{} m/s is not racing speed", s.speed);
            assert!(s.position.iter().all(|v| v.is_finite()));
            assert!(s.camera_eye.iter().all(|v| v.is_finite()));
        });
    }

    /// The final checkpoint is the finish, reached well inside the cap, having
    /// actually raced: traffic threaded and the course completed.
    #[test]
    fn the_final_checkpoint_is_a_completed_race() {
        let (app, steps) = driven_with_count(GoldenStop::Finish);
        let state = state_of(&app, steps);
        assert_eq!(
            state.phase,
            phase_index(RacePhase::Finished),
            "the agent crossed the line in {steps} steps"
        );
        assert!(steps < GOLDEN_STEP_LIMIT, "and did not hit the cap");
        assert!(state.progress > 0.99);
        assert!(
            state.near_misses > 60,
            "only {} near misses — the agent is not hunting them",
            state.near_misses
        );
    }

    /// The whole point: the same constants produce the same race, twice.
    #[test]
    fn the_golden_run_replays_identically() {
        CHECKPOINTS.iter().for_each(|c| {
            assert_eq!(
                state_at(c.stop),
                state_at(c.stop),
                "checkpoint `{}` is not deterministic",
                c.name
            );
        });
    }

    /// The agent is the only thing driving. Cut it out and the car does not
    /// move: the same number of steps under a hand-written idle command leaves
    /// the car on the grid, so the checkpoints are evidence about the *agent*
    /// and not about the passage of steps.
    #[test]
    fn the_agent_is_what_moves_the_car() {
        let mut idle = BurntRubber::with_profile(
            GOLDEN_SEED,
            GOLDEN_TUNING,
            GOLDEN_WIDTH,
            GOLDEN_HEIGHT,
            GOLDEN_PROFILE,
        );
        idle.advance_steps(700, DriveCommand::IDLE);
        let driven = state_at(GoldenStop::Step(700));
        assert!(
            driven.distance > idle.sim().car().distance + 500.0,
            "the agent covered {} m against the idle run's {} m",
            driven.distance,
            idle.sim().car().distance
        );
    }

    /// Every checkpoint builds a frame with geometry, light and a camera — the
    /// same floor `axiom-shot` needs to render it.
    #[test]
    fn every_checkpoint_renders_a_frame() {
        let builders: [(&str, fn() -> RunningApp); 5] = [
            ("grid", build_golden_grid),
            ("opening", build_golden_opening),
            ("esses", build_golden_esses),
            ("canyon", build_golden_canyon),
            ("finish", build_golden_finish),
        ];
        builders.iter().for_each(|(name, build)| {
            let mut running = build();
            let outcome = running.tick(0);
            assert!(!outcome.draws().is_empty(), "{name} drew nothing");
            assert!(!outcome.lights().is_empty(), "{name} is unlit");
            assert_ne!(
                outcome.camera_view_proj(),
                [0.0f32; 16],
                "{name} has no camera"
            );
        });
    }
}
