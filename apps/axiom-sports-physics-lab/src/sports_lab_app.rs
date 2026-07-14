//! The deterministic lab core: one physics world, the player rig, the camera
//! rig, the interaction state, and the object registry — engine-agnostic and
//! fully native-testable. `SportsPhysicsLab::step(Intent)` advances one fixed
//! 60 Hz step; the wasm edge only decodes browser events into an [`Intent`]
//! and renders the resulting state.

use axiom::prelude::Vec3;
use axiom_math::Transform;
use axiom_physics::{PhysicsApi, PhysicsBodyHandle};

use super::sports_lab_balls::{BallKind, BallPreset, BALLS};
use super::sports_lab_camera::{CameraMode, CameraRig};
use super::sports_lab_humanoid::{DUMMY_GRAB_RADIUS, FIGURE_CENTER_Y};
use super::sports_lab_interaction::InteractionState;
use super::sports_lab_physics::{self, runtime_step, MAX_ANGULAR_SPEED, MAX_LINEAR_SPEED};
use super::sports_lab_player::PlayerRig;

/// Where the T-pose dummy stands.
pub const DUMMY_X: f32 = 4.5;
pub const DUMMY_Z: f32 = -2.5;

/// Held/edge input for one fixed step. Edges (`primary`, `secondary`,
/// `toggle_view`, `reset`) fire once per press; deltas are this step's totals.
#[derive(Debug, Clone, Copy, Default)]
pub struct Intent {
    /// W — walk along the look direction (ground plane).
    pub forward: bool,
    /// S — walk backward.
    pub backward: bool,
    /// A — strafe left.
    pub strafe_left: bool,
    /// D — strafe right.
    pub strafe_right: bool,
    /// Mouse-look deltas this step, already in radians.
    pub look_yaw: f32,
    pub look_pitch: f32,
    /// Left click: pick up when empty-handed, toss when holding.
    pub primary: bool,
    /// Right click: set the held object down gently.
    pub secondary: bool,
    /// V — flip first/third person.
    pub toggle_view: bool,
    /// Wheel notches: positive zooms out (into third person), negative in.
    pub zoom: f32,
    /// R — reset every object to its spawn.
    pub reset: bool,
}

/// What an interactable is (drives the render shape + HUD name).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabObjectKind {
    Ball(BallKind),
    Dummy,
}

/// One interactable's registry row: the physics handle plus the state the
/// renderer, interaction, and tests read (mirrored from the snapshot each step).
#[derive(Debug, Clone, Copy)]
pub struct LabObject {
    pub kind: LabObjectKind,
    pub name: &'static str,
    pub body: PhysicsBodyHandle,
    /// Bounding-sphere radius for reticle targeting.
    pub grab_radius: f32,
    /// Visual scale (full extents) for the render mesh.
    pub visual_scale: Vec3,
    /// Mass (scales the toss).
    pub mass: f32,
    /// Spawn transform (restored by reset).
    pub initial: Transform,
    /// Mirrored world state (position / rotation quat xyzw / velocities).
    pub pos: Vec3,
    pub rot: [f32; 4],
    pub vel: Vec3,
    pub ang: Vec3,
}

/// Minimal HUD state the web edge paints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabHud {
    pub held: Option<&'static str>,
    pub hover: Option<&'static str>,
    pub mode: CameraMode,
    pub step: u64,
    pub physics_step: u64,
}

/// The deterministic lab.
#[derive(Debug)]
pub struct SportsPhysicsLab {
    physics: PhysicsApi,
    player: PlayerRig,
    camera: CameraRig,
    interaction: InteractionState,
    objects: Vec<LabObject>,
    step_n: u64,
}

impl SportsPhysicsLab {
    /// A fresh lab: arena + ball lineup + dummy + player, everything at spawn.
    pub fn new() -> Self {
        let mut physics = sports_lab_physics::world();
        sports_lab_physics::add_arena(&mut physics);

        let mut objects = Vec::with_capacity(BALLS.len() + 1);
        for preset in &BALLS {
            let body = sports_lab_physics::add_ball(&mut physics, preset);
            objects.push(object_for_ball(preset, body));
        }
        let dummy_body = sports_lab_physics::add_dummy(&mut physics, DUMMY_X, DUMMY_Z);
        objects.push(LabObject {
            kind: LabObjectKind::Dummy,
            name: "Humanoid Dummy",
            body: dummy_body,
            grab_radius: DUMMY_GRAB_RADIUS,
            visual_scale: Vec3::ONE,
            mass: sports_lab_physics::DUMMY_MASS,
            initial: sports_lab_physics::dummy_spawn_transform(DUMMY_X, DUMMY_Z),
            pos: Vec3::new(DUMMY_X, FIGURE_CENTER_Y, DUMMY_Z),
            rot: [0.0, 0.0, 0.0, 1.0],
            vel: Vec3::ZERO,
            ang: Vec3::ZERO,
        });

        let player = PlayerRig::new(&mut physics);
        SportsPhysicsLab {
            physics,
            player,
            camera: CameraRig::new(),
            interaction: InteractionState::default(),
            objects,
            step_n: 0,
        }
    }

    /// Advance one fixed step under `intent`.
    pub fn step(&mut self, intent: Intent) {
        if intent.reset {
            self.reset_objects();
        }
        self.player.step(&mut self.physics, &intent);
        self.camera.apply(intent.toggle_view, intent.zoom);

        let eye = self.player.eye();
        let look = self.player.look_dir();
        self.interaction.update_hover(eye, look, &self.objects);
        if intent.primary {
            self.interaction
                .primary(&mut self.physics, &self.objects, look);
        }
        if intent.secondary {
            self.interaction
                .secondary(&mut self.physics, &self.objects, look);
        }
        self.interaction
            .drive_held(&mut self.physics, &self.objects, eye, look);

        self.physics
            .step(runtime_step(self.step_n))
            .expect("physics step");
        let _ = self.physics.drain_events();
        self.mirror_objects();
        self.clamp_velocities();
        self.step_n += 1;
    }

    /// Put every object back at its spawn, at rest, and empty the hands.
    pub fn reset_objects(&mut self) {
        self.interaction.release();
        for object in &self.objects {
            self.physics
                .set_body_transform(object.body, object.initial)
                .expect("reset object transform");
            self.physics
                .set_body_velocity(object.body, Vec3::ZERO, Vec3::ZERO)
                .expect("reset object velocity");
        }
        for object in &mut self.objects {
            object.pos = object.initial.translation;
            let r = object.initial.rotation;
            object.rot = [r.x, r.y, r.z, r.w];
            object.vel = Vec3::ZERO;
            object.ang = Vec3::ZERO;
        }
    }

    fn mirror_objects(&mut self) {
        let snapshot = self.physics.snapshot();
        for object in &mut self.objects {
            if let Some(body) = snapshot.bodies().iter().find(|b| b.handle() == object.body) {
                let t = body.transform();
                object.pos = t.translation;
                object.rot = [t.rotation.x, t.rotation.y, t.rotation.z, t.rotation.w];
                object.vel = body.linear_velocity();
                object.ang = body.angular_velocity();
            }
        }
    }

    /// App-side safety caps (the physics module has none by design). The
    /// mirrors are updated with the clamped values so readers never see an
    /// over-cap velocity.
    fn clamp_velocities(&mut self) {
        for object in &mut self.objects {
            let lin = object.vel.length();
            let ang = object.ang.length();
            if lin > MAX_LINEAR_SPEED || ang > MAX_ANGULAR_SPEED {
                let lin_k = if lin > MAX_LINEAR_SPEED {
                    MAX_LINEAR_SPEED / lin
                } else {
                    1.0
                };
                let ang_k = if ang > MAX_ANGULAR_SPEED {
                    MAX_ANGULAR_SPEED / ang
                } else {
                    1.0
                };
                object.vel = object.vel.mul_scalar(lin_k);
                object.ang = object.ang.mul_scalar(ang_k);
                self.physics
                    .set_body_velocity(object.body, object.vel, object.ang)
                    .expect("velocity safety cap");
            }
        }
    }

    // --- read surface -----------------------------------------------------------

    /// The interactable registry (mirrored world state included).
    pub fn objects(&self) -> &[LabObject] {
        &self.objects
    }

    /// The player rig.
    pub fn player(&self) -> &PlayerRig {
        &self.player
    }

    /// The current camera view.
    pub fn camera_mode(&self) -> CameraMode {
        self.camera.mode()
    }

    /// This step's `(eye, target)` camera pair (advances third-person smoothing).
    pub fn camera_eye_target(&mut self) -> (Vec3, Vec3) {
        let feet = self.player.feet();
        let eye = self.player.eye();
        let look = self.player.look_dir();
        self.camera.eye_target(feet, eye, look)
    }

    /// Index of the held object.
    pub fn held(&self) -> Option<usize> {
        self.interaction.held()
    }

    /// Index of the reticle-hovered object.
    pub fn hover(&self) -> Option<usize> {
        self.interaction.hover()
    }

    /// The HUD state.
    pub fn hud(&self) -> LabHud {
        LabHud {
            held: self.interaction.held().map(|i| self.objects[i].name),
            hover: self.interaction.hover().map(|i| self.objects[i].name),
            mode: self.camera.mode(),
            step: self.step_n,
            physics_step: self.physics.latest_step_record().step_index(),
        }
    }

    /// A deterministic digest of the dynamic state (for replay tests).
    pub fn state_digest(&self) -> Vec<(f32, f32, f32)> {
        self.objects
            .iter()
            .map(|o| (o.pos.x, o.pos.y, o.pos.z))
            .collect()
    }
}

impl Default for SportsPhysicsLab {
    fn default() -> Self {
        SportsPhysicsLab::new()
    }
}

fn object_for_ball(preset: &BallPreset, body: PhysicsBodyHandle) -> LabObject {
    let initial = sports_lab_physics::ball_spawn_transform(preset);
    LabObject {
        kind: LabObjectKind::Ball(preset.kind),
        name: preset.name,
        body,
        grab_radius: preset.radius.max(0.16),
        visual_scale: preset.visual_scale,
        mass: preset.mass,
        initial,
        pos: initial.translation,
        rot: [
            initial.rotation.x,
            initial.rotation.y,
            initial.rotation.z,
            initial.rotation.w,
        ],
        vel: Vec3::ZERO,
        ang: Vec3::ZERO,
    }
}
