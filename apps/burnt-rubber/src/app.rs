//! [`BurntRubber`] — the seam between the deterministic race and its presentation.
//!
//! This is the only type that owns both halves, and the only place they meet.
//! It holds the [`RaceSim`], the [`RaceScene`], the engine's [`RunningApp`], and
//! the fixed-step accumulator that banks a variable browser frame into whole
//! 60 Hz simulation steps.
//!
//! ## The frame contract
//!
//! ```text
//! advance(elapsed_nanos, command)
//!   → FrameAccumulator::advance  →  N whole fixed steps
//!       → RaceSim::step(command)          (deterministic)
//!       → RaceScene::step(sim)            (presentation clocks)
//!   → alpha = remainder / fixed_step      (presentation only)
//! present()
//!   → RaceScene::pose(app, sim, alpha)    (interpolated)
//!   → RunningApp::tick                    (renders)
//! ```
//!
//! The elapsed time enters *here*, at the very outside, and is immediately
//! converted into an integer count of steps. Nothing below this point ever sees
//! a duration: the simulation is advanced a whole number of times or not at all.
//! That is what makes the game frame-rate independent and exactly replayable
//! from a command list, and it is why the accumulator lives at the app root
//! rather than inside the simulation.

use axiom::prelude::{App, DefaultPlugins, FrameAccumulator, FrameOutcome, RunningApp, Window};

use crate::audio_cues::RaceAudio;
use crate::command::DriveCommand;
use crate::debug_view::DebugView;
use crate::diagnostics::Diagnostics;
use crate::hud::HudModel;
use crate::render::RaceScene;
use crate::sim::{RaceEvent, RaceSim};
use crate::tuning::{Tuning, FIXED_STEP_NANOS};
use crate::{CANVAS_ID, DEFAULT_SEED, HEIGHT, WIDTH};

/// The most fixed steps one rendered frame may run.
///
/// A frame that has fallen further behind than this drops the excess rather than
/// spiralling: catching up on two hundred steps in one frame would take longer
/// than the frame that caused the debt, and the debt would grow.
///
/// It also sets the frame rate below which the game goes into **slow motion**,
/// which is the more important of the two. A frame can only advance the
/// simulation this many steps, so real time is held only while
/// `fps >= 60 / MAX_STEPS_PER_FRAME`. At five steps that floor is 12 fps — well
/// inside the range a software rasterizer on a phone actually runs at, which
/// would make the *game* slower on one render backend than another. Twelve
/// steps puts the floor at 5 fps.
///
/// Raising it is only safe because [`MAX_FRAME_NANOS`] caps the input: the work
/// per frame is bounded whatever the clock says, so a slow frame can never buy
/// itself an even slower one.
pub const MAX_STEPS_PER_FRAME: u32 = 12;

/// The most real time one frame may hand to the accumulator.
///
/// This is the guard against a stall turning into a fast-forward, and it has to
/// live here because of how [`FrameAccumulator`] is specified: it *banks*
/// everything it is given, and whole steps clamped away by `max_steps` "also
/// stay banked (never dropped)". That is the right contract for an accumulator —
/// silently losing time would make the sub-step remainder meaningless — but it
/// means the step cap limits the *rate* of catch-up and not the *total debt*.
///
/// So a two-second hitch (a backgrounded tab, a long pause on the crash screen,
/// a browser hiccup) banks 120 steps of debt, and the next twenty-four frames
/// each run the full five: the simulation runs at **five times real time** until
/// the debt drains. Every acceleration, every speed, the whole world moves at
/// 5×, which is exactly what a player reads as "the car suddenly accelerates
/// insanely fast".
///
/// A racing game is real-time: time lost to a stall should be *lost*, not
/// replayed at quintuple speed. Clamping the input to what a single frame is
/// willing to spend is what makes that true, and it keeps the accumulator's own
/// contract intact — it still banks everything it is handed, it is simply never
/// handed more than it can use.
pub const MAX_FRAME_NANOS: u64 = MAX_STEPS_PER_FRAME as u64 * FIXED_STEP_NANOS;

/// The whole app: simulation, scene, engine and the step accumulator.
#[derive(Debug)]
pub struct BurntRubber {
    sim: RaceSim,
    scene: RaceScene,
    running: RunningApp,
    accumulator: FrameAccumulator,
    audio: RaceAudio,
    debug: DebugView,
    diagnostics: Diagnostics,
    alpha: f32,
    frame: u64,
    events: Vec<RaceEvent>,
}

impl BurntRubber {
    /// The shipping app at the default framebuffer size.
    pub fn new() -> BurntRubber {
        BurntRubber::with(DEFAULT_SEED, Tuning::DEFAULT, WIDTH, HEIGHT)
    }

    /// An app for a given seed, tuning and framebuffer.
    pub fn with(seed: u64, tuning: Tuning, width: u32, height: u32) -> BurntRubber {
        let sim = RaceSim::new(seed, tuning);
        let mut running = App::new()
            .window(
                Window::new(width, height)
                    .with_surface_id(CANVAS_ID)
                    .with_clear_color(axiom::prelude::Color::linear_rgb(
                        crate::render::palette::ratio(crate::render::palette::SKY[0]),
                        crate::render::palette::ratio(crate::render::palette::SKY[1]),
                        crate::render::palette::ratio(crate::render::palette::SKY[2]),
                    )),
            )
            .add_plugins(DefaultPlugins)
            .fixed_timestep_nanos(FIXED_STEP_NANOS)
            .setup(|_world, _meshes, _materials| {})
            .build();
        let scene = RaceScene::install(&mut running, &sim, width, height);
        let debug = DebugView::install(&mut running);
        let accumulator = FrameAccumulator::new(FIXED_STEP_NANOS)
            .expect("the fixed step is a valid, non-zero duration");
        BurntRubber {
            sim,
            scene,
            running,
            accumulator,
            audio: RaceAudio::new(),
            debug,
            diagnostics: Diagnostics::new(),
            alpha: 0.0,
            frame: 0,
            events: Vec::new(),
        }
    }

    /// The race.
    pub const fn sim(&self) -> &RaceSim {
        &self.sim
    }

    /// The race, mutably — the capture harness poses specific moments with this.
    pub fn sim_mut(&mut self) -> &mut RaceSim {
        &mut self.sim
    }

    /// The scene.
    pub const fn scene(&self) -> &RaceScene {
        &self.scene
    }

    /// The engine app.
    pub fn running(&mut self) -> &mut RunningApp {
        &mut self.running
    }

    /// The HUD model for the current state.
    pub fn hud(&self) -> HudModel {
        HudModel::of(&self.sim)
    }

    /// Diagnostics for the last presented frame.
    pub const fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// The sound bank.
    ///
    /// Owned here rather than by the browser arm because its cadence is a
    /// *simulation* cadence: the engine note is a grain every
    /// [`crate::audio_cues::GRAIN_STEPS`] fixed steps. Ticking it from the
    /// render loop instead makes the note's rate — and so its pitch sampling —
    /// a function of the frame rate, which means the game sounds different on a
    /// slow backend than on a fast one.
    pub const fn audio(&self) -> &RaceAudio {
        &self.audio
    }

    /// The sound bank, mutably — the platform arm enables it and realizes its
    /// batch, and does nothing else to it.
    pub fn audio_mut(&mut self) -> &mut RaceAudio {
        &mut self.audio
    }

    /// Whether the visual debug overlay is showing.
    pub const fn debug_enabled(&self) -> bool {
        self.debug.enabled()
    }

    /// Show or hide the visual debug overlay.
    pub fn set_debug(&mut self, enabled: bool) {
        self.debug.set_enabled(enabled);
    }

    /// The debug overlay, for inspecting its markers.
    pub const fn debug(&self) -> &DebugView {
        &self.debug
    }

    /// The events emitted by the steps run in the last [`Self::advance`].
    pub fn events(&self) -> &[RaceEvent] {
        &self.events
    }

    /// The interpolation fraction the next [`Self::present`] will use.
    pub const fn alpha(&self) -> f32 {
        self.alpha
    }

    /// Bank `elapsed_nanos` of real time into whole fixed steps and run them,
    /// each under `command`. Returns how many steps ran.
    ///
    /// `command` is applied to every step in the frame, which is correct: a
    /// frame's held input *is* the input for every step it covers. The
    /// edge-triggered fields (reset, pause, restart) are consumed by the first
    /// step and cleared for the rest, so a single key press is a single action
    /// regardless of how many steps the frame happened to bank.
    pub fn advance(&mut self, elapsed_nanos: u64, command: DriveCommand) -> u32 {
        // Never hand the accumulator more than a frame can spend — see
        // [`MAX_FRAME_NANOS`]. Without this the excess is banked and replayed at
        // five times real time.
        let elapsed = elapsed_nanos.min(MAX_FRAME_NANOS);
        let budget = self.accumulator.advance(elapsed, MAX_STEPS_PER_FRAME);
        self.events.clear();
        let mut held = command;
        for _ in 0..budget.steps() {
            self.sim.step(held);
            self.scene.step(&self.sim);
            self.audio.step(&self.sim);
            for event in self.sim.events() {
                self.audio.on_event(event);
            }
            self.events.extend_from_slice(self.sim.events());
            held = DriveCommand {
                reset: false,
                pause: false,
                restart: false,
                ..held
            };
        }
        self.alpha = budget.remainder_nanos() as f32 / budget.fixed_step_nanos() as f32;
        budget.steps()
    }

    /// Run exactly `steps` fixed steps under `command`, ignoring real time.
    /// The deterministic path the tests and the capture harness drive.
    pub fn advance_steps(&mut self, steps: u32, command: DriveCommand) {
        self.events.clear();
        let mut held = command;
        for _ in 0..steps {
            self.sim.step(held);
            self.scene.step(&self.sim);
            self.audio.step(&self.sim);
            for event in self.sim.events() {
                self.audio.on_event(event);
            }
            self.events.extend_from_slice(self.sim.events());
            held = DriveCommand {
                reset: false,
                pause: false,
                restart: false,
                ..held
            };
        }
        self.alpha = 0.0;
    }

    /// Pose the scene for the current state without ticking the engine.
    ///
    /// The capture harness poses here and lets `axiom-shot` drive the single
    /// engine tick that renders the frame, so the host frame sequence is
    /// advanced exactly once.
    pub fn pose(&mut self) {
        let alpha = self.alpha;
        self.scene.pose(&mut self.running, &self.sim, alpha);
        self.debug.update(&mut self.running, &self.sim);
        self.diagnostics.observe(&self.sim, &self.scene);
    }

    /// Pose and render one frame.
    pub fn present(&mut self) -> FrameOutcome {
        self.pose();
        let outcome = self.running.tick(self.frame);
        self.frame += 1;
        outcome
    }

    /// Consume the app, handing the posed engine app to a capture harness.
    pub fn into_running(self) -> RunningApp {
        self.running
    }
}

impl Default for BurntRubber {
    fn default() -> Self {
        BurntRubber::new()
    }
}

/// Build the app as a posed [`RunningApp`], for `axiom-shot` and for tooling.
///
/// This is the entry point the repository's capture harness and app registry
/// name. It builds the shipping course, holds the car on the start line, and
/// poses the opening frame.
pub fn build_burnt_rubber() -> RunningApp {
    let mut app = BurntRubber::new();
    app.pose();
    app.into_running()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::RacePhase;

    #[test]
    fn a_new_app_builds_a_renderable_scene() {
        let mut app = BurntRubber::new();
        let outcome = app.present();
        assert!(!outcome.draws().is_empty(), "the opening frame draws");
        assert_eq!(app.sim().phase(), RacePhase::Countdown);
        assert_eq!(app.hud().countdown, crate::sim::COUNTDOWN_NUMBERS);
    }

    #[test]
    fn the_registry_entry_point_returns_a_posed_app() {
        let mut running = build_burnt_rubber();
        let outcome = running.tick(0);
        assert!(!outcome.draws().is_empty());
        assert!(!outcome.lights().is_empty());
    }

    /// The frame contract: real time is converted to whole steps at the outside
    /// and never reaches the simulation.
    #[test]
    fn real_time_is_banked_into_whole_fixed_steps() {
        let mut app = BurntRubber::new();
        // Exactly one step's worth.
        assert_eq!(app.advance(FIXED_STEP_NANOS, DriveCommand::IDLE), 1);
        assert_eq!(app.sim().step_count(), 1);
        // Half a step banks nothing but leaves a remainder.
        assert_eq!(app.advance(FIXED_STEP_NANOS / 2, DriveCommand::IDLE), 0);
        assert_eq!(app.sim().step_count(), 1);
        assert!(app.alpha() > 0.4 && app.alpha() < 0.6, "alpha {}", app.alpha());
        // The banked remainder completes the next step.
        assert_eq!(app.advance(FIXED_STEP_NANOS / 2 + 10, DriveCommand::IDLE), 1);
        assert_eq!(app.sim().step_count(), 2);
    }

    #[test]
    fn a_stalled_frame_is_capped_rather_than_spiralling() {
        let mut app = BurntRubber::new();
        // Two whole seconds in one frame.
        let steps = app.advance(2_000_000_000, DriveCommand::IDLE);
        assert_eq!(steps, MAX_STEPS_PER_FRAME, "the catch-up is capped");
        assert_eq!(app.sim().step_count(), MAX_STEPS_PER_FRAME as u64);
    }

    /// The regression test for "the car suddenly accelerates insanely fast".
    ///
    /// A stall must not leave the simulation running faster than real time
    /// afterwards. Capping the steps per frame is not enough on its own: the
    /// accumulator banks whatever it is handed, so the excess comes back as five
    /// steps a frame until it drains.
    #[test]
    fn a_stall_does_not_leave_the_simulation_running_fast() {
        let mut app = BurntRubber::new();
        for _ in 0..10 {
            app.advance(FIXED_STEP_NANOS, DriveCommand::IDLE);
        }
        // A two-second hitch: a backgrounded tab, a pause on the crash screen.
        app.advance(2_000_000_000, DriveCommand::IDLE);

        // Twenty ordinary frames afterwards are twenty steps. Not a hundred.
        let before = app.sim().step_count();
        for frame in 0..20 {
            let steps = app.advance(FIXED_STEP_NANOS, DriveCommand::IDLE);
            assert_eq!(
                steps, 1,
                "frame {frame} after the stall ran {steps} steps, not one"
            );
        }
        assert_eq!(
            app.sim().step_count() - before,
            20,
            "the simulation is back in real time immediately"
        );
    }

    /// And the same thing measured where the player would feel it: the car must
    /// not accelerate faster after a stall than it does from a standing start.
    #[test]
    fn acceleration_after_a_stall_matches_acceleration_without_one() {
        let launch = |stall: bool| {
            let mut app = BurntRubber::new();
            while app.sim().phase() == crate::sim::RacePhase::Countdown {
                app.advance(FIXED_STEP_NANOS, DriveCommand::IDLE);
            }
            if stall {
                app.advance(3_000_000_000, DriveCommand::IDLE);
            }
            // One second of wall-clock throttle, delivered as ordinary frames.
            for _ in 0..60 {
                app.advance(FIXED_STEP_NANOS, DriveCommand::FLAT_OUT);
            }
            app.sim().car().speed()
        };
        let clean = launch(false);
        let after_stall = launch(true);
        assert!(
            (after_stall - clean).abs() < 2.0,
            "a second of throttle reached {after_stall} m/s after a stall but {clean} m/s without one"
        );
    }

    /// Gameplay speed must not depend on how fast the renderer is. A backend
    /// that manages only ten frames a second still advances the simulation at
    /// real time.
    #[test]
    fn a_slow_renderer_still_runs_the_game_at_real_time() {
        // Ten frames a second: 100 ms of real time per frame.
        let frame = FIXED_STEP_NANOS * 6;
        let mut app = BurntRubber::new();
        for _ in 0..10 {
            app.advance(frame, DriveCommand::IDLE);
        }
        // One second of wall clock is sixty steps of simulation.
        assert_eq!(
            app.sim().step_count(),
            60,
            "ten slow frames covered {} steps of the 60 in a second",
            app.sim().step_count()
        );
        assert!(
            (app.sim().elapsed_seconds() - 1.0).abs() < 0.05,
            "and the race clock agrees: {} s",
            app.sim().elapsed_seconds()
        );
    }

    #[test]
    fn the_slow_motion_floor_is_low_enough_for_a_software_rasterizer() {
        let floor_fps = 60.0 / MAX_STEPS_PER_FRAME as f32;
        assert!(
            floor_fps <= 6.0,
            "the game drops into slow motion below {floor_fps} fps"
        );
    }

    #[test]
    fn the_frame_budget_is_the_step_cap_expressed_in_time() {
        assert_eq!(MAX_FRAME_NANOS, MAX_STEPS_PER_FRAME as u64 * FIXED_STEP_NANOS);
        let mut app = BurntRubber::new();
        // Exactly the budget still runs the full cap.
        assert_eq!(app.advance(MAX_FRAME_NANOS, DriveCommand::IDLE), MAX_STEPS_PER_FRAME);
        // And anything beyond it is discarded rather than banked.
        assert_eq!(app.advance(MAX_FRAME_NANOS * 100, DriveCommand::IDLE), MAX_STEPS_PER_FRAME);
        assert_eq!(app.advance(FIXED_STEP_NANOS, DriveCommand::IDLE), 1);
    }

    /// A single key press is a single action however many steps the frame banks.
    #[test]
    fn edge_triggered_input_fires_once_per_frame_not_once_per_step() {
        let mut app = BurntRubber::new();
        while app.sim().phase() == RacePhase::Countdown {
            app.advance(FIXED_STEP_NANOS, DriveCommand::IDLE);
        }
        app.advance_steps(600, DriveCommand::FLAT_OUT);

        // Four steps in one frame, with `reset` held.
        let before = app.sim().car().distance;
        app.advance(
            FIXED_STEP_NANOS * 4,
            DriveCommand {
                reset: true,
                ..DriveCommand::FLAT_OUT
            },
        );
        let resets = app
            .events()
            .iter()
            .filter(|e| matches!(e, RaceEvent::Reset))
            .count();
        assert_eq!(resets, 1, "one press, one reset");
        assert!(app.sim().car().distance < before);
    }

    #[test]
    fn stepping_deterministically_ignores_the_clock_entirely() {
        let run = || {
            let mut app = BurntRubber::new();
            app.advance_steps(900, DriveCommand::FLAT_OUT);
            (*app.sim().car(), app.sim().step_count())
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn presenting_twice_renders_the_same_frame_and_advances_nothing() {
        let mut app = BurntRubber::new();
        app.advance_steps(300, DriveCommand::FLAT_OUT);
        let steps = app.sim().step_count();
        let first = app.present();
        let draws = first.draws().len();
        let camera = first.camera_view_proj();
        let second = app.present();
        assert_eq!(second.draws().len(), draws);
        assert_eq!(second.camera_view_proj(), camera);
        assert_eq!(app.sim().step_count(), steps, "rendering advances nothing");
    }

    /// The regression test for "the music is different on the other backend".
    ///
    /// Audio is clocked by the *simulation*, so the same number of fixed steps
    /// must produce the same sound however many frames delivered them. Ticking
    /// it per rendered frame instead made the engine note's rate — and its pitch
    /// sampling — a function of the frame rate.
    #[test]
    fn the_same_steps_sound_the_same_however_many_frames_delivered_them() {
        let run = |frames: u32, steps_per_frame: u32| {
            let mut app = BurntRubber::new();
            app.audio_mut().enable(true);
            for _ in 0..frames {
                app.advance(
                    FIXED_STEP_NANOS * steps_per_frame as u64,
                    DriveCommand::FLAT_OUT,
                );
            }
            let steps = app.sim().step_count();
            (steps, format!("{:?}", app.audio_mut().api().take_pending()))
        };

        // Sixty steps delivered as sixty smooth frames, and as twelve slow ones.
        let smooth = run(60, 1);
        let stuttering = run(12, 5);
        assert_eq!(smooth.0, stuttering.0, "both ran the same number of steps");
        assert_eq!(
            smooth.1, stuttering.1,
            "and scheduled byte-identical audio"
        );
    }

    #[test]
    fn the_sound_bank_starts_silent_and_is_reachable() {
        let mut app = BurntRubber::new();
        assert!(!app.audio().enabled(), "silent until the page has been touched");
        app.audio_mut().enable(true);
        assert!(app.audio().enabled());
    }

    #[test]
    fn the_hud_and_the_diagnostics_track_the_run() {
        let mut app = BurntRubber::new();
        while app.sim().phase() == RacePhase::Countdown {
            app.advance_steps(1, DriveCommand::IDLE);
        }
        app.advance_steps(600, DriveCommand::FLAT_OUT);
        app.present();

        let hud = app.hud();
        assert!(hud.speed_kmh > 100, "moving: {} km/h", hud.speed_kmh);
        assert!(hud.progress > 0.0);
        let d = app.diagnostics();
        assert!(d.scene.active_chunks > 0);
        assert_eq!(d.simulation_steps, app.sim().step_count());
        assert!((d.speed_ms - app.sim().car().speed()).abs() < 1.0e-4);
    }

    #[test]
    fn events_from_a_frame_are_collected_and_then_cleared() {
        let mut app = BurntRubber::new();
        // The countdown emits ticks and a GO.
        let total = app.sim().tuning().race.countdown_steps * crate::sim::COUNTDOWN_NUMBERS;
        app.advance(FIXED_STEP_NANOS * total as u64, DriveCommand::IDLE);
        // Capped at MAX_STEPS_PER_FRAME, so run the rest.
        while app.sim().phase() == RacePhase::Countdown {
            app.advance(FIXED_STEP_NANOS, DriveCommand::IDLE);
        }
        assert!(app.events().iter().any(|e| matches!(e, RaceEvent::Go)));
        app.advance(FIXED_STEP_NANOS, DriveCommand::IDLE);
        assert!(!app.events().iter().any(|e| matches!(e, RaceEvent::Go)));
    }

    #[test]
    fn a_custom_seed_and_size_build_their_own_course() {
        let a = BurntRubber::with(11, Tuning::DEFAULT, 320, 180);
        let b = BurntRubber::with(12, Tuning::DEFAULT, 320, 180);
        assert_ne!(a.sim().track().samples(), b.sim().track().samples());
        assert_eq!(a.sim().track().seed(), 11);
    }

    #[test]
    fn the_default_app_is_the_shipping_app() {
        let a = BurntRubber::default();
        assert_eq!(a.sim().track().seed(), DEFAULT_SEED);
    }

    #[test]
    fn the_debug_overlay_is_off_by_default_and_toggles() {
        let mut app = BurntRubber::new();
        assert!(!app.debug_enabled());
        app.present();
        assert!(app.debug().markers().is_empty(), "nothing is drawn while off");

        app.set_debug(true);
        assert!(app.debug_enabled());
        app.advance_steps(120, DriveCommand::FLAT_OUT);
        app.present();
        assert!(!app.debug().markers().is_empty(), "and something is while on");

        app.set_debug(false);
        app.present();
        assert!(app.debug().markers().is_empty());
    }

    #[test]
    fn the_simulation_is_reachable_for_scripted_posing() {
        let mut app = BurntRubber::new();
        app.sim_mut().place_at(4_000.0);
        assert!((app.sim().car().distance - 4_000.0).abs() < 5.0);
        app.pose();
        assert!(app.diagnostics().scene.active_chunks > 0);
    }
}
