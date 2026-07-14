//! The composition root: wires the headless [`ShowcaseRun`] (simulation +
//! controller + camera director + juice) to the engine's `RunningApp` scene
//! and the deterministic input sampler. `build_end_zone` is the repo-standard
//! capture builder.

use axiom::prelude::{App, Color, DefaultPlugins, FrameOutcome, RunningApp, Vec2, Window};
use axiom_input::{ActionId, DeviceFrame, InputState, KeyToken};
use axiom_kernel::Ratio;
use axiom_kernel::Tick;

use crate::ai::AssignmentKind;
use crate::camera::CameraMode;
use crate::config::EndZoneConfig;
use crate::debug::{self, DebugInstance};
use crate::scene::EndZoneScene;
use crate::showcase::{DiagnosticCommand, ShowcaseRun};

/// The canvas id the browser page binds the surface to.
pub const CANVAS_ID: &str = "axiom-end-zone-canvas";
/// Render surface size.
pub const WIDTH: u32 = 1280;
pub const HEIGHT: u32 = 720;

// Diagnostic input actions.
const ACTION_START: ActionId = ActionId::new(1);
const ACTION_RESET: ActionId = ActionId::new(2);
const ACTION_CAM_FORMATION: ActionId = ActionId::new(3);
const ACTION_CAM_QB: ActionId = ActionId::new(4);
const ACTION_CAM_FLIGHT: ActionId = ActionId::new(5);
const ACTION_CAM_CARRIER: ActionId = ActionId::new(6);
const ACTION_CAM_AUTO: ActionId = ActionId::new(7);
const ACTION_DEBUG: ActionId = ActionId::new(8);
// Player-control actions (the keyboard twin of the touch stick + A button).
const ACTION_PRIMARY: ActionId = ActionId::new(9);
const ACTION_UP: ActionId = ActionId::new(10);
const ACTION_DOWN: ActionId = ActionId::new(11);
const ACTION_LEFT: ActionId = ActionId::new(12);
const ACTION_RIGHT: ActionId = ActionId::new(13);

/// One frame of touch input from the platform edge: the virtual joystick
/// vector (`x` right, `y` up/downfield, each `-1..=1`) plus the button edges
/// (already debounced to a single frame by the edge).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TouchInput {
    pub stick_x: f32,
    pub stick_y: f32,
    pub primary: bool,
    pub reset: bool,
}

/// The complete End Zone app.
#[derive(Debug)]
pub struct EndZoneApp {
    pub run: ShowcaseRun,
    running: RunningApp,
    scene: EndZoneScene,
    input: InputState,
    /// Static per-player world route waypoints (cloned once for debug draw).
    routes: Vec<Vec<axiom::prelude::Vec3>>,
    debug_markers: Vec<DebugInstance>,
    frame_n: u64,
    last_camera_mode: CameraMode,
    last_forced: bool,
}

impl EndZoneApp {
    /// Build the app: engine scene installed, input bound, showcase armed.
    pub fn new(config: EndZoneConfig) -> Self {
        // Sky clear color — it is also what the renderer's distance cue fogs
        // toward, so it must be daylight, never black.
        let sky = Color::linear_rgb(
            Ratio::finite_or_zero(0.50),
            Ratio::finite_or_zero(0.67),
            Ratio::finite_or_zero(0.88),
        );
        let mut running = App::new()
            .window(
                Window::new(WIDTH, HEIGHT)
                    .with_surface_id(CANVAS_ID)
                    .with_clear_color(sky),
            )
            .add_plugins(DefaultPlugins)
            .setup(|_world, _meshes, _materials| {})
            .build();
        let scene = EndZoneScene::install(&mut running);
        let run = ShowcaseRun::new(config);
        let routes = run
            .sim
            .assignments
            .iter()
            .map(|assignment| match assignment.kind {
                AssignmentKind::Route { .. } => assignment.route.clone(),
                _ => Vec::new(),
            })
            .collect();

        let mut input = InputState::new();
        input.bind_action(ACTION_START, &[KeyToken::new("Space")]);
        input.bind_action(ACTION_RESET, &[KeyToken::new("KeyR")]);
        input.bind_action(ACTION_CAM_FORMATION, &[KeyToken::new("Digit1")]);
        input.bind_action(ACTION_CAM_QB, &[KeyToken::new("Digit2")]);
        input.bind_action(ACTION_CAM_FLIGHT, &[KeyToken::new("Digit3")]);
        input.bind_action(ACTION_CAM_CARRIER, &[KeyToken::new("Digit4")]);
        input.bind_action(ACTION_CAM_AUTO, &[KeyToken::new("Digit5")]);
        input.bind_action(ACTION_DEBUG, &[KeyToken::new("F1")]);
        input.bind_action(ACTION_PRIMARY, &[KeyToken::new("Enter")]);
        input.bind_action(
            ACTION_UP,
            &[KeyToken::new("KeyW"), KeyToken::new("ArrowUp")],
        );
        input.bind_action(
            ACTION_DOWN,
            &[KeyToken::new("KeyS"), KeyToken::new("ArrowDown")],
        );
        input.bind_action(
            ACTION_LEFT,
            &[KeyToken::new("KeyA"), KeyToken::new("ArrowLeft")],
        );
        input.bind_action(
            ACTION_RIGHT,
            &[KeyToken::new("KeyD"), KeyToken::new("ArrowRight")],
        );

        EndZoneApp {
            run,
            running,
            scene,
            input,
            routes,
            debug_markers: Vec::new(),
            frame_n: 0,
            last_camera_mode: CameraMode::FormationWide,
            last_forced: false,
        }
    }

    /// One frame: sample input (keyboard + touch) → commands + stick → fixed
    /// sim step → snapshot → camera + juice → scene sync → engine tick.
    pub fn frame(&mut self, keys_down: &[KeyToken], touch: TouchInput) -> FrameOutcome {
        let frame = DeviceFrame::new(Vec2::new(WIDTH as f32, HEIGHT as f32), keys_down, &[]);
        self.input.sample(Tick::new(self.frame_n), &frame);

        let mut commands: Vec<DiagnosticCommand> = Vec::new();
        let pressed: [(ActionId, DiagnosticCommand); 9] = [
            (ACTION_START, DiagnosticCommand::StartPlay),
            (ACTION_RESET, DiagnosticCommand::ResetAll),
            (
                ACTION_CAM_FORMATION,
                DiagnosticCommand::ForceFormationCamera,
            ),
            (ACTION_CAM_QB, DiagnosticCommand::ForceQuarterbackCamera),
            (ACTION_CAM_FLIGHT, DiagnosticCommand::ForceFlightCamera),
            (ACTION_CAM_CARRIER, DiagnosticCommand::ForceCarrierCamera),
            (ACTION_CAM_AUTO, DiagnosticCommand::AutomaticCamera),
            (ACTION_DEBUG, DiagnosticCommand::ToggleDebug),
            (ACTION_PRIMARY, DiagnosticCommand::PrimaryAction),
        ];
        for (action, command) in pressed {
            if self.input.pressed(action) {
                commands.push(command);
            }
        }
        if touch.primary {
            commands.push(DiagnosticCommand::PrimaryAction);
        }
        if touch.reset {
            commands.push(DiagnosticCommand::ResetAll);
        }

        // The movement stick: touch joystick + the keyboard axes, clamped.
        let axis = |negative: ActionId, positive: ActionId| -> f32 {
            f32::from(self.input.is_down(positive)) - f32::from(self.input.is_down(negative))
        };
        let stick_x = (touch.stick_x + axis(ACTION_LEFT, ACTION_RIGHT)).clamp(-1.0, 1.0);
        let stick_y = (touch.stick_y + axis(ACTION_DOWN, ACTION_UP)).clamp(-1.0, 1.0);
        self.run.sim.user_stick = Vec2::new(stick_x, stick_y);

        let output = self.run.step(&commands);
        self.last_camera_mode = output.camera_mode;
        self.last_forced = output.camera_mode != self.run.director.mode();

        if self.run.debug_enabled {
            debug::build_markers(
                &output.snapshot,
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
            &self.run.juice,
            &output.camera,
            &self.debug_markers,
        );

        let outcome = self.running.tick(self.frame_n);
        self.frame_n += 1;
        outcome
    }

    /// The overlay rows for this frame's state.
    pub fn overlay_rows(&self) -> Vec<(String, String)> {
        let snapshot = crate::presentation::snapshot::capture(&self.run.sim);
        debug::overlay_rows(
            &snapshot,
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
