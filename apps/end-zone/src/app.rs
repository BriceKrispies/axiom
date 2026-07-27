//! The composition root: wires the headless [`ShowcaseRun`] (simulation + run
//! loop + camera director + juice) to the engine's `RunningApp` scene and the
//! game's input map. `build_end_zone` is the repo-standard capture builder.
//!
//! One rendered frame is NOT one simulation tick any more. Input is sampled
//! every frame; simulation ticks are spent according to
//! [`ShowcaseRun::time_scale`], which is how the decision window's slow motion
//! happens without the simulation ever learning about wall-clock time.

use axiom::prelude::{App, Color, DefaultPlugins, FrameOutcome, RunningApp, Vec2, Window};
use axiom_input::KeyToken;
use axiom_kernel::Ratio;

use crate::ai::AssignmentKind;
use crate::camera::{CameraMode, CameraPose};
use crate::config::EndZoneConfig;
use crate::controls::GameInput;
use crate::debug::{self, DebugInstance};
use crate::presentation::interpolate;
use crate::scene::EndZoneScene;
use crate::showcase::{ShowcaseRun, StepOutput};
use crate::state::SimState;

pub use crate::controls::TouchInput;

/// The canvas id the browser page binds the surface to.
pub const CANVAS_ID: &str = "axiom-end-zone-canvas";
/// Render surface size.
pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;

/// Bounds on the run's requested time scale, so a bad value can neither freeze
/// the simulation forever nor spin the step loop.
const MIN_TIME_SCALE: f32 = 0.02;
const MAX_TIME_SCALE: f32 = 4.0;

/// The complete End Zone app.
#[derive(Debug)]
pub struct EndZoneApp {
    pub run: ShowcaseRun,
    running: RunningApp,
    scene: EndZoneScene,
    input: GameInput,
    /// Static per-player world route waypoints (cloned once for debug draw).
    routes: Vec<Vec<axiom::prelude::Vec3>>,
    debug_markers: Vec<DebugInstance>,
    frame_n: u64,
    /// Fractional simulation ticks owed. One whole credit buys one tick, so a
    /// 0.16x decision window advances the sim once every ~6 rendered frames.
    sim_credit: f32,
    last_camera_mode: CameraMode,
    last_forced: bool,
    /// The most recent step's outputs — what `present` renders (and what a
    /// frozen/paused frame re-renders without advancing the sim).
    last_output: Option<StepOutput>,
    /// The step before it. Dilated frames are drawn between the two, so slow
    /// motion is smooth instead of a held-and-jump slideshow.
    prev_output: Option<StepOutput>,
}

fn routes_of(sim: &SimState) -> Vec<Vec<axiom::prelude::Vec3>> {
    sim.assignments
        .iter()
        .map(|assignment| match assignment.kind {
            AssignmentKind::Route { .. } => assignment.route.clone(),
            _ => Vec::new(),
        })
        .collect()
}

impl EndZoneApp {
    /// Build the app at the default render size: engine scene installed, input
    /// bound, showcase armed.
    pub fn new(config: EndZoneConfig) -> Self {
        Self::new_sized(config, WIDTH, HEIGHT)
    }

    /// Build the app at an explicit render size — the native capture harness
    /// ([`crate::capture`]) uses this so the baked camera projection aspect
    /// matches the screenshot framebuffer instead of being distorted by it.
    pub fn new_sized(config: EndZoneConfig, width: u32, height: u32) -> Self {
        // Sky clear color — also the renderer's distance-fog target, so it must
        // be daylight, never black. A saturated cerulean (red/green dropped, blue
        // held) reads as punchy daylight and deepens the far field instead of
        // bleaching it to a flat pastel.
        let sky = Color::linear_rgb(
            Ratio::finite_or_zero(0.18),
            Ratio::finite_or_zero(0.45),
            Ratio::finite_or_zero(0.90),
        );
        let mut running = App::new()
            .window(
                Window::new(width, height)
                    .with_surface_id(CANVAS_ID)
                    .with_clear_color(sky),
            )
            .add_plugins(DefaultPlugins)
            .setup(|_world, _meshes, _materials| {})
            .build();
        let scene = EndZoneScene::install(&mut running);
        let run = ShowcaseRun::new(config);
        let routes = routes_of(&run.sim);

        EndZoneApp {
            run,
            running,
            scene,
            input: GameInput::new(),
            routes,
            debug_markers: Vec::new(),
            frame_n: 0,
            sim_credit: 0.0,
            last_camera_mode: CameraMode::FormationWide,
            last_forced: false,
            last_output: None,
            prev_output: None,
        }
    }

    /// Swap in a different showcase run (a launched/restarted match, or the
    /// ambient menu showcase) without touching the engine scene.
    pub fn replace_run(&mut self, run: ShowcaseRun) {
        self.routes = routes_of(&run.sim);
        self.run = run;
        self.last_output = None;
        self.prev_output = None;
        // A swapped run starts from a clean input/time state: no latched press
        // and no fractional tick may survive into the new session.
        self.sim_credit = 0.0;
        self.input.clear();
    }

    /// One frame: sample input (keyboard + touch) → commands + stick → fixed
    /// sim step → snapshot → camera + juice → scene sync → engine tick.
    pub fn frame(&mut self, keys_down: &[KeyToken], touch: TouchInput) -> FrameOutcome {
        self.advance(keys_down, touch);
        self.present()
    }

    /// Sample this frame's input and advance the simulation by the run's
    /// current time scale (no engine tick).
    ///
    /// Simulation ticks are bought with fractional *credit*, so a 0.13× decision
    /// window advances the sim once every ~7.7 rendered frames while the input
    /// map keeps sampling every one of them. At the normal 1.0× scale this is
    /// exactly one tick per frame, unchanged.
    ///
    /// The leftover credit is the render **alpha**: how far this frame sits
    /// between the last two ticks. `present` draws there, which is what stops
    /// the dilated frames from being a held-and-jump slideshow.
    pub fn advance(&mut self, keys_down: &[KeyToken], touch: TouchInput) {
        let size = Vec2::new(WIDTH as f32, HEIGHT as f32);
        let stick = self.input.sample(size, keys_down, touch);

        self.sim_credit += self.run.time_scale().clamp(MIN_TIME_SCALE, MAX_TIME_SCALE);
        while self.sim_credit >= 1.0 {
            self.sim_credit -= 1.0;
            self.run.set_user_stick(stick);
            let commands = self.input.drain();
            let output = self.run.step(&commands);
            self.last_camera_mode = output.camera_mode;
            self.last_forced = output.camera_mode != self.run.director.mode();
            self.prev_output = self.last_output.take();
            self.last_output = Some(output);
        }
    }

    /// What this frame should actually draw.
    ///
    /// At full speed there is one tick per frame, so the newest step is drawn
    /// directly and gameplay keeps zero added latency. Only while time is
    /// dilated — where several frames share a tick and each would otherwise be
    /// a duplicate — is the frame blended between the previous and current
    /// ticks at the leftover credit.
    fn presented(&self) -> Option<StepOutput> {
        let output = self.last_output.as_ref()?;
        let dilated = self.run.time_scale() < 1.0;
        let Some(prev) = self.prev_output.as_ref().filter(|_| dilated) else {
            return Some(output.clone());
        };
        let alpha = self.sim_credit.clamp(0.0, 1.0);
        Some(StepOutput {
            snapshot: interpolate::snapshot(&prev.snapshot, &output.snapshot, alpha),
            camera: CameraPose::lerp(prev.camera, output.camera, alpha),
            poses: interpolate::poses(&prev.poses, &output.poses, alpha),
            camera_mode: output.camera_mode,
            events: output.events.clone(),
        })
    }

    /// Render the most recent step (advancing once first if none exists yet):
    /// pose the scene → engine tick. A paused frame calls only this, so the
    /// frozen sim/camera/juice state re-presents unchanged.
    pub fn present(&mut self) -> FrameOutcome {
        self.pose_scene();
        let outcome = self.running.tick(self.frame_n);
        self.frame_n += 1;
        outcome
    }

    /// Pose the most recent step into the engine scene (debug markers → scene
    /// sync) **without** ticking the engine — the native capture harness
    /// ([`crate::capture`]) poses here and lets the renderer drive the tick.
    pub fn pose_scene(&mut self) {
        self.pose(None);
    }

    /// Pose the most recent step with an explicit camera in place of the
    /// director's — the development-only field inspection views
    /// ([`crate::field::inspect`]) use this to hold the camera-driven field
    /// paint under a fixed set of framings. Nothing in the shipping loop calls
    /// it, and it does not touch the director, so the next ordinary frame is
    /// framed exactly as it would have been.
    pub fn pose_scene_from(&mut self, camera: CameraPose) {
        self.pose(Some(camera));
    }

    fn pose(&mut self, camera_override: Option<CameraPose>) {
        if self.last_output.is_none() {
            self.advance(&[], TouchInput::default());
        }
        let Some(mut output) = self.presented() else {
            return;
        };
        output.camera = camera_override.unwrap_or(output.camera);
        if self.run.debug_enabled {
            debug::build_markers(
                &output.snapshot,
                &output.poses,
                &self.routes,
                &output.camera,
                &mut self.debug_markers,
            );
        } else {
            self.debug_markers.clear();
        }
        self.scene.update(
            &mut self.running,
            &output.snapshot,
            &output.poses,
            &self.run.juice,
            &output.camera,
            &self.debug_markers,
        );
    }

    /// The overlay rows for this frame's state.
    pub fn overlay_rows(&self) -> Vec<(String, String)> {
        let snapshot = crate::presentation::snapshot::capture(&self.run.sim);
        let selected = snapshot.quarterback.index();
        debug::overlay_rows(
            &snapshot,
            self.run.locomotion.sample(selected),
            self.last_camera_mode,
            self.last_forced,
            self.run.director.active_impulses(),
            self.run.debug_enabled,
        )
    }

    /// The wrapped engine app (mesh/material upload lanes for the web loop).
    pub fn running(&mut self) -> &mut RunningApp {
        &mut self.running
    }

    /// The frame counter the engine tick is driven with.
    pub fn frame_index(&self) -> u64 {
        self.frame_n
    }

    /// Consume into the engine app (capture-harness convention).
    pub fn into_running(self) -> RunningApp {
        self.running
    }
}

/// Repo-standard capture builder: the composed app advanced one frame so the
/// formation scene is posed.
pub fn build_end_zone() -> RunningApp {
    let mut app = EndZoneApp::new(EndZoneConfig::default());
    let _ = app.frame(&[], TouchInput::default());
    app.into_running()
}
