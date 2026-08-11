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

use std::sync::Arc;

use axiom::prelude::{App, DefaultPlugins, FrameAccumulator, FrameOutcome, RunningApp, Window};

use crate::audio_cues::RaceAudio;
use crate::command::DriveCommand;
use crate::debug_view::DebugView;
use crate::diagnostics::Diagnostics;
use crate::hud::HudModel;
use crate::render::RaceScene;
use crate::sim::{RaceEvent, RacePhase, RaceSim};
use crate::start_screen::{StartCommand, StartOutcome, StartScreen};
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

/// What the app is doing: waiting on the start screen, or racing.
///
/// The stage sits **above** the simulation rather than inside it, and that is
/// the whole design of the pre-race screen. `RaceSim` has exactly one job —
/// advance a race by one fixed step — and adding a "not started yet" phase to it
/// would have made every one of its callers ask "is this a phase that moves?"
/// before every step. Instead the app simply does not step it: while the screen
/// is up the frozen race is still *posed* every frame, which is what puts the
/// night road behind the title at no cost and with nothing to keep in sync.
#[derive(Debug)]
enum Stage {
    /// The start screen is up and the race is frozen on the grid.
    Waiting(StartScreen),
    /// The race is running.
    Racing,
}

/// The whole app: simulation, scene, engine and the step accumulator.
#[derive(Debug)]
pub struct BurntRubber {
    sim: RaceSim,
    /// The agent's run, advancing one step for every step the player takes.
    /// `None` until a race starts. It owns its own `RaceSim`, so it is not in
    /// the player's world and cannot touch the player's car — see
    /// [`crate::ghost`].
    ghost: Option<crate::ghost::GhostRun>,
    scene: RaceScene,
    running: RunningApp,
    accumulator: FrameAccumulator,
    audio: RaceAudio,
    debug: DebugView,
    diagnostics: Diagnostics,
    alpha: f32,
    frame: u64,
    events: Vec<RaceEvent>,
    stage: Stage,
    /// The build parameters, so starting a race can rebuild the simulation
    /// without the app having to be rebuilt around it.
    profile: crate::PlayProfile,
    /// The course compiled by the startup preparation phase, retained so a
    /// restart and the ghost reuse it instead of recompiling. Before this the
    /// course was compiled four times per construction-plus-restart cycle.
    plan: Arc<crate::course::runtime::CoursePlan>,
    /// The viewport the start screen is laid out for.
    viewport: (f32, f32),
}

impl BurntRubber {
    /// **The shipping app**: the night road on screen, the race frozen on the
    /// grid, and the start screen up.
    ///
    /// This is the one constructor that opens on the start screen, because it is
    /// the one that models what a player actually gets. [`BurntRubber::with`]
    /// and its siblings skip straight to a race — which is what the
    /// deterministic tests and the capture slices drive, none of which are about
    /// the screen.
    pub fn new() -> BurntRubber {
        let mut app = BurntRubber::with(DEFAULT_SEED, Tuning::DEFAULT, WIDTH, HEIGHT);
        app.open_start_screen();
        app
    }

    /// An app for a given seed, tuning and framebuffer, already racing.
    pub fn with(seed: u64, tuning: Tuning, width: u32, height: u32) -> BurntRubber {
        BurntRubber::with_profile(seed, tuning, width, height, crate::PlayProfile::Wheel)
    }

    /// Cull road paint to the near field, or stop doing so — the Canvas 2D
    /// adaptation. See
    /// [`crate::render::chunks::RoadChunks::set_paint_near_field_only`].
    pub fn set_paint_near_field_only(&mut self, limited: bool) {
        self.scene.set_paint_near_field_only(limited);
    }

    /// As [`BurntRubber::with`], for whichever game `profile` names.
    ///
    /// This is the only place the profile enters the simulation half; everything
    /// downstream reads it from the sim. See [`crate::PlayProfile`].
    pub fn with_profile(
        seed: u64,
        tuning: Tuning,
        width: u32,
        height: u32,
        profile: crate::PlayProfile,
    ) -> BurntRubber {
        // The chase rig is a *composition*, and `width x height` is the frame it
        // has to compose in. A perspective camera's horizontal field is its
        // vertical field scaled by the aspect, so the same rig that fills a 16:9
        // frame has barely a lane of road either side of the car in an upright
        // phone frame — see `CameraTuning::framed_for_aspect`, which re-solves
        // the arm and the eye height for this frame and returns the authored
        // numbers untouched for any frame at least as wide as the one they were
        // authored in.
        //
        // Done here, once, at construction, rather than per frame: the camera is
        // deterministic simulation state, and a rig that changed shape mid-race
        // would make a replay depend on when the window was resized. Everything
        // downstream — the sim, the ghost, a reset — reads this tuning.
        let tuning = Tuning {
            camera: tuning
                .camera
                .framed_for_aspect(width.max(1) as f32 / height.max(1) as f32),
            ..tuning
        };
        // ── the startup preparation phase ──
        //
        // The course, the three albedos and the whole road's geometry are
        // produced here, inside `App::build()`'s `Runtime::prepare`, before the
        // runtime is allowed to reach `Running`. Nothing below re-generates any
        // of it: the sim, the ghost, a restart and the scene all read what came
        // out of the barrier.
        let prepared = crate::preparation::RacePreparation::new();
        let mut builder = App::new()
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
            .setup(|_world, _meshes, _materials| {});
        builder = prepared
            .tasks(seed, &tuning)
            .into_iter()
            .fold(builder, |builder, (name, task)| builder.prepare_with(name, task));
        // `build()` runs the phase and crosses the barrier. Past this line the
        // runtime is `Running`, which it could not be if any task had failed.
        let mut running = builder.build();

        let course = prepared
            .course
            .borrow_mut()
            .take()
            .expect("preparation compiled the course");
        let textures = prepared
            .textures
            .borrow_mut()
            .take()
            .expect("preparation synthesised the albedos");
        let meshes = prepared
            .meshes
            .borrow_mut()
            .take()
            .expect("preparation cut the road");

        let plan = course.plan();
        let sim = RaceSim::from_plan(Arc::clone(&plan), tuning, profile);
        let scene =
            RaceScene::install_prepared(&mut running, &sim, width, height, &textures, meshes);
        let debug = DebugView::install(&mut running);
        let accumulator = FrameAccumulator::new(FIXED_STEP_NANOS)
            .expect("the fixed step is a valid, non-zero duration");
        BurntRubber {
            sim,
            // Every race has a ghost, however it was built — the constructors
            // that skip the title screen (the tests and the capture slices) are
            // still races. `open_start_screen` clears it again.
            ghost: Some(crate::ghost::GhostRun::from_plan(
                RaceSim::from_plan(Arc::clone(&plan), tuning, profile),
                profile,
            )),
            scene,
            running,
            accumulator,
            audio: RaceAudio::new(),
            debug,
            diagnostics: Diagnostics::new(),
            alpha: 0.0,
            frame: 0,
            events: Vec::new(),
            stage: Stage::Racing,
            profile,
            plan,
            viewport: (width as f32, height as f32),
        }
    }

    // ---------------------------------------------------------------------
    // The pre-race screen.
    //
    // Four calls, and between them they are the whole of it: put the screen up,
    // tell it how big the viewport is, feed it a frame of input, and — when it
    // says so — start the race. Nothing about the screen is on the fixed step.
    // ---------------------------------------------------------------------

    /// Put the start screen up, freezing the race where it stands.
    pub fn open_start_screen(&mut self) {
        self.stage = Stage::Waiting(StartScreen::open(self.viewport.0, self.viewport.1));
        // No race, no ghost: the title screen shows the empty grid, not an
        // agent sitting on it.
        self.ghost = None;
    }

    /// The start screen, while it is up.
    pub fn start_screen(&self) -> Option<&StartScreen> {
        match &self.stage {
            Stage::Waiting(screen) => Some(screen),
            Stage::Racing => None,
        }
    }

    /// Whether the start screen is up.
    pub fn waiting(&self) -> bool {
        self.start_screen().is_some()
    }

    /// Tell the start screen how big the viewport is.
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport = (width.max(1.0), height.max(1.0));
        if let Stage::Waiting(screen) = &mut self.stage {
            screen.resize(self.viewport.0, self.viewport.1);
        }
    }

    /// Fold one frame of start-screen input. Does nothing at all while racing.
    ///
    /// The cue is scheduled here rather than by the caller because it is a
    /// *consequence* of the outcome, and a second place deciding what starting a
    /// race sounds like is a second place for the two to disagree.
    pub fn update_start_screen(&mut self, command: StartCommand) -> StartOutcome {
        let Stage::Waiting(screen) = &self.stage else {
            return StartOutcome::Idle;
        };
        let outcome = screen.update(command);
        if outcome == StartOutcome::Started {
            self.audio.on_race_start();
            self.start_race();
        }
        outcome
    }

    /// Build the race at the start line, counting in, and leave the screen.
    ///
    /// A fresh [`RaceSim`] rather than a mutated one: the screen's answer is
    /// "go", and the cleanest way to honour it is the same one the constructor
    /// already takes.
    pub fn start_race(&mut self) {
        // The prepared course, reused. A restart rebuilds the *race*, not the
        // road: the road is the same nine kilometres it was a moment ago.
        self.sim = RaceSim::from_plan(
            Arc::clone(&self.plan),
            *self.sim.tuning(),
            self.profile,
        );
        self.restart_ghost();
        self.scene.reset();
        self.stage = Stage::Racing;
    }

    /// Park the car `distance` metres along the course at `speed` m/s, leaving
    /// the start screen and the countdown behind — the diagnosis probe's
    /// placement (see [`crate::probe`]).
    ///
    /// This exists because a rendering defect that only appears in motion has to
    /// be observed at a *chosen* point on the course, at a *chosen* speed, twice,
    /// identically. Reaching 300 km/h on a given straight by driving there is not
    /// reproducible; placing the car there is. It reuses `start_race` rather than
    /// mutating the current stage so a placement from the title screen and one
    /// mid-race land in the same state.
    pub fn place_for_probe(&mut self, distance: f32, speed: f32) {
        if self.waiting() {
            self.start_race();
        }
        // Past the countdown, or the car is held on the grid and cannot move.
        while self.sim.phase() == RacePhase::Countdown {
            self.sim.step(DriveCommand::IDLE);
        }
        self.sim.place_at(distance);
        self.sim.launch_at(speed);
    }

    /// Put the ghost back on the grid alongside a freshly built race.
    ///
    /// Its own simulation is rebuilt from the same seed, tuning and profile as
    /// the player's, which is what makes the two runs comparable: the same
    /// course, the same traffic stream, the same car.
    fn restart_ghost(&mut self) {
        self.ghost = Some(crate::ghost::GhostRun::from_plan(
            RaceSim::from_plan(Arc::clone(&self.plan), *self.sim.tuning(), self.profile),
            self.profile,
        ));
    }

    /// The agent's run, if a race is under way.
    pub const fn ghost(&self) -> Option<&crate::ghost::GhostRun> {
        self.ghost.as_ref()
    }

    /// How far ahead of the ghost the player is, in metres (negative = behind).
    /// `None` before a race starts.
    pub fn ghost_delta_metres(&self) -> Option<f32> {
        self.ghost
            .as_ref()
            .map(|ghost| self.sim.car().distance - ghost.distance())
    }

    /// Advance the ghost by the steps the player's simulation just took.
    ///
    /// Two rules keep the two runs honest against each other. A **restart**
    /// rebuilds the ghost, because the player's race was rebuilt (the sim
    /// consumes `restart` internally and never reports it, so it is read off the
    /// command here). A **paused** race advances neither, because a ghost that
    /// kept driving while the player was in the pause menu would not be a
    /// ghost, it would be a penalty.
    fn advance_ghost(&mut self, steps: u32, restarted: bool) {
        restarted.then(|| self.restart_ghost());
        let running = self.sim.phase() != RacePhase::Paused;
        self.ghost
            .as_mut()
            .filter(|_| running & !restarted)
            .map(|ghost| (0..steps).for_each(|_| ghost.step()));
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
        HudModel::of(&self.sim).with_ghost_delta(self.ghost_delta_metres())
    }

    /// **The course authoring surface**: what the compiled course knows about
    /// where the player is now.
    ///
    /// Ordered, labelled `(label, value)` rows — the seed, the section and its
    /// primitive, the local curvature/grade/bank, the active traffic zone and
    /// encounter, what is ahead, the traversability classification, the boost
    /// verdict, and the validation counts. The browser telemetry panel appends
    /// them under the frame counters; a test asserts on them directly.
    pub fn course_rows(&self) -> Vec<(String, String)> {
        crate::course::runtime::inspect::rows(
            self.sim.plan(),
            self.sim.car().distance,
            crate::course::runtime::inspect::DEFAULT_LOOKAHEAD_M,
        )
    }

    /// The compiled course as deterministic text — the whole plan, for a test
    /// to diff or an agent to read.
    pub fn dump_course(&self) -> String {
        self.sim.plan().dump()
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
        // **The start screen holds the world still.** Not by pausing the race,
        // and not by a phase inside the simulation, but by simply not running
        // any steps: no car, no traffic, no timer, no boost, no progress. The
        // accumulator is not fed either, so the time the player spent looking at
        // the title does not come back as banked steps the moment they go.
        if self.waiting() {
            self.events.clear();
            self.alpha = 0.0;
            return 0;
        }
        // Never hand the accumulator more than a frame can spend — see
        // [`MAX_FRAME_NANOS`]. Without this the excess is banked and replayed at
        // five times real time.
        let elapsed = elapsed_nanos.min(MAX_FRAME_NANOS);
        let budget = self.accumulator.advance(elapsed, MAX_STEPS_PER_FRAME);
        self.events.clear();
        let restarted = command.restart;
        let mut held = command;
        for _ in 0..budget.steps() {
            self.sim.step(held);
            self.scene.step(&self.sim);
            self.audio.step(&self.sim);
            for event in self.sim.events() {
                self.audio.on_event(event);
            }
            self.events.extend_from_slice(self.sim.events());
            held = held.spent();
        }
        self.advance_ghost(budget.steps(), restarted);
        self.alpha = budget.remainder_nanos() as f32 / budget.fixed_step_nanos() as f32;
        budget.steps()
    }

    /// Run exactly `steps` fixed steps under `command`, ignoring real time.
    /// The deterministic path the tests and the capture harness drive.
    pub fn advance_steps(&mut self, steps: u32, command: DriveCommand) {
        self.events.clear();
        if self.waiting() {
            return;
        }
        let restarted = command.restart;
        let mut held = command;
        for _ in 0..steps {
            self.sim.step(held);
            self.scene.step(&self.sim);
            self.audio.step(&self.sim);
            for event in self.sim.events() {
                self.audio.on_event(event);
            }
            self.events.extend_from_slice(self.sim.events());
            held = held.spent();
        }
        self.advance_ghost(steps, restarted);
        self.alpha = 0.0;
    }

    /// Pose the scene for the current state without ticking the engine.
    ///
    /// The capture harness poses here and lets `axiom-shot` drive the single
    /// engine tick that renders the frame, so the host frame sequence is
    /// advanced exactly once.
    pub fn pose(&mut self) {
        let alpha = self.alpha;
        self.scene
            .pose(&mut self.running, &self.sim, self.ghost.as_ref(), alpha);
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
    use axiom_math::Vec2;

    /// The shipping app with the start screen already answered — what every
    /// test below drives, because none of them is about the screen.
    fn racing() -> BurntRubber {
        BurntRubber::with(DEFAULT_SEED, Tuning::DEFAULT, WIDTH, HEIGHT)
    }

    #[test]
    fn a_new_app_builds_a_renderable_scene() {
        let mut app = racing();
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
        let mut app = racing();
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
        let mut app = racing();
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
        let mut app = racing();
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
            let mut app = racing();
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
        let mut app = racing();
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
        let mut app = racing();
        // Exactly the budget still runs the full cap.
        assert_eq!(app.advance(MAX_FRAME_NANOS, DriveCommand::IDLE), MAX_STEPS_PER_FRAME);
        // And anything beyond it is discarded rather than banked.
        assert_eq!(app.advance(MAX_FRAME_NANOS * 100, DriveCommand::IDLE), MAX_STEPS_PER_FRAME);
        assert_eq!(app.advance(FIXED_STEP_NANOS, DriveCommand::IDLE), 1);
    }

    /// A single key press is a single action however many steps the frame banks.
    #[test]
    fn edge_triggered_input_fires_once_per_frame_not_once_per_step() {
        let mut app = racing();
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

    /// The same rule, on the channel that actually broke it.
    ///
    /// This test existed for `reset` alone and passed the whole time the phone
    /// was hopping two lanes per tap: `lane_step` was added to `DriveCommand`
    /// without being added to either step loop's hand-copied list of one-shot
    /// channels. Naming one channel is not testing the rule, so this drives the
    /// hop — and it is a *long* frame, because that is the only frame that ever
    /// showed the bug. On a phone that frame arrived when the render got
    /// heavier, not when the input code changed.
    #[test]
    fn one_tap_is_one_lane_however_many_steps_the_frame_banks() {
        let mut app = BurntRubber::with_profile(
            DEFAULT_SEED,
            Tuning::DEFAULT,
            WIDTH,
            HEIGHT,
            crate::PlayProfile::Rails,
        );
        while app.sim().phase() == RacePhase::Countdown {
            app.advance(FIXED_STEP_NANOS, DriveCommand::IDLE);
        }
        // Measured where the player sees it — the car's lateral offset — rather
        // than through the rails state, which is private and should stay so.
        let lane_width = app.sim().track().lane_width();
        let start = app.sim().car().lateral;

        // One tap, on a frame worth four fixed steps, then long enough under no
        // input for the car to settle on whichever lane it was aimed at.
        app.advance(
            FIXED_STEP_NANOS * 4,
            DriveCommand {
                lane_step: 1,
                ..DriveCommand::FLAT_OUT
            },
        );
        app.advance_steps(120, DriveCommand::FLAT_OUT);

        let moved = (app.sim().car().lateral - start).abs();
        assert!(
            (moved - lane_width).abs() < lane_width * 0.25,
            "one tap moved {moved:.2} m — a lane is {lane_width:.2} m, so this is {:.1} lanes",
            moved / lane_width
        );
    }

    #[test]
    fn stepping_deterministically_ignores_the_clock_entirely() {
        let run = || {
            let mut app = racing();
            app.advance_steps(900, DriveCommand::FLAT_OUT);
            (*app.sim().car(), app.sim().step_count())
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn presenting_twice_renders_the_same_frame_and_advances_nothing() {
        let mut app = racing();
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
            let mut app = racing();
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
        let mut app = racing();
        assert!(!app.audio().enabled(), "silent until the page has been touched");
        app.audio_mut().enable(true);
        assert!(app.audio().enabled());
    }

    #[test]
    fn the_hud_and_the_diagnostics_track_the_run() {
        let mut app = racing();
        while app.sim().phase() == RacePhase::Countdown {
            app.advance_steps(1, DriveCommand::IDLE);
        }
        app.advance_steps(600, DriveCommand::FLAT_OUT);
        app.present();

        let hud = app.hud();
        assert!(hud.speed_kmh > 100, "moving: {} km/h", hud.speed_kmh);
        assert!(hud.progress > 0.0);
        let d = app.diagnostics();
        assert!(d.scene.road_draws > 0);
        assert_eq!(d.simulation_steps, app.sim().step_count());
        assert!((d.speed_ms - app.sim().car().speed()).abs() < 1.0e-4);
    }

    #[test]
    fn events_from_a_frame_are_collected_and_then_cleared() {
        let mut app = racing();
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
        assert!(a.waiting(), "and the shipping app opens on the start screen");
    }

    // ---------------------------------------------------------------------
    // The pre-race screen.
    // ---------------------------------------------------------------------

    /// The shipping app opens on the start screen, over a night road that is
    /// genuinely drawn.
    #[test]
    fn the_shipping_app_opens_on_the_start_screen_over_the_road() {
        let mut app = BurntRubber::new();
        assert!(app.waiting());
        assert!(app.start_screen().is_some());

        let outcome = app.present();
        assert!(!outcome.draws().is_empty(), "the night road is drawn");
        assert!(!outcome.lights().is_empty());
    }

    /// **The headline rule of the pre-race flow**: nothing moves while the start
    /// screen is up. Not the car, not the traffic, not the clock, not the boost.
    #[test]
    fn the_simulation_does_not_advance_while_the_start_screen_is_up() {
        let mut app = BurntRubber::new();
        let before = *app.sim().car();
        let boost = app.sim().boost().charge();

        // Real time, deterministic steps, and a full-throttle boosting command:
        // none of it may move anything.
        for _ in 0..120 {
            assert_eq!(
                app.advance(
                    FIXED_STEP_NANOS,
                    DriveCommand {
                        boost: true,
                        ..DriveCommand::FLAT_OUT
                    }
                ),
                0,
                "a frame ran steps with the start screen up"
            );
        }
        app.advance_steps(600, DriveCommand::FLAT_OUT);

        assert_eq!(app.sim().step_count(), 0, "no steps were taken");
        assert_eq!(*app.sim().car(), before, "the car did not move");
        assert_eq!(app.sim().boost().charge(), boost, "the meter did not move");
        assert_eq!(app.sim().elapsed_seconds(), 0.0, "the clock did not start");
        assert_eq!(app.sim().traffic().active_count(), 0, "no traffic spawned");
        assert!(app.events().is_empty());
    }

    /// The flow, in order: press START RACE, the screen exits, the countdown
    /// begins, the race runs.
    #[test]
    fn starting_the_race_leaves_the_screen_and_begins_the_countdown() {
        let mut app = BurntRubber::new();
        assert_eq!(app.update_start_screen(StartCommand::IDLE), StartOutcome::Idle);
        assert!(app.waiting(), "an idle frame does not start the race");

        assert_eq!(
            app.update_start_screen(StartCommand::CONFIRM),
            StartOutcome::Started
        );
        assert!(!app.waiting(), "the screen exits");
        assert_eq!(app.sim().phase(), RacePhase::Countdown, "the countdown begins");
        assert_eq!(app.sim().step_count(), 0);

        app.advance_steps(30, DriveCommand::FLAT_OUT);
        assert_eq!(app.sim().step_count(), 30, "and the race now runs");
    }

    /// A tap on the button starts the race; a tap on the road behind it does
    /// not.
    #[test]
    fn only_a_press_on_the_button_starts_the_race() {
        let mut app = BurntRubber::new();
        app.set_viewport(1280.0, 720.0);
        assert_eq!(
            app.update_start_screen(StartCommand::tap(Vec2::new(4.0, 4.0))),
            StartOutcome::Idle
        );
        assert!(app.waiting());

        let button = app.start_screen().expect("the screen").layout().start;
        assert_eq!(
            app.update_start_screen(StartCommand::tap(button.centre())),
            StartOutcome::Started
        );
        assert!(!app.waiting());
    }

    #[test]
    fn the_screen_relays_out_when_the_viewport_changes_and_ignores_race_input() {
        let mut app = BurntRubber::new();
        app.set_viewport(390.0, 844.0);
        assert_eq!(
            app.start_screen().expect("the screen").layout().viewport,
            Vec2::new(390.0, 844.0)
        );
        app.set_viewport(1600.0, 900.0);
        assert_eq!(
            app.start_screen().expect("the screen").layout().viewport,
            Vec2::new(1600.0, 900.0)
        );

        // And a start command while racing is simply ignored.
        app.start_race();
        assert_eq!(
            app.update_start_screen(StartCommand::CONFIRM),
            StartOutcome::Idle
        );
        assert!(!app.waiting());
    }

    /// A quick restart stays in the race — it does not send the player back to
    /// the start screen.
    #[test]
    fn a_quick_restart_stays_in_the_race() {
        let mut app = BurntRubber::new();
        app.start_race();
        app.advance_steps(400, DriveCommand::FLAT_OUT);
        assert!(app.sim().car().distance > 10.0);

        app.advance(
            FIXED_STEP_NANOS,
            DriveCommand {
                restart: true,
                ..DriveCommand::IDLE
            },
        );
        assert!(!app.waiting(), "a restart does not go back to the screen");
        assert_eq!(app.sim().phase(), RacePhase::Countdown);
        assert_eq!(app.sim().step_count(), 0);
    }

    #[test]
    fn the_debug_overlay_is_off_by_default_and_toggles() {
        let mut app = racing();
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
        let mut app = racing();
        app.sim_mut().place_at(4_000.0);
        assert!((app.sim().car().distance - 4_000.0).abs() < 5.0);
        app.pose();
        assert!(app.diagnostics().scene.road_draws > 0);
    }
}
