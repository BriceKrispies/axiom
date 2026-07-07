//! **Gravix** — a physics-driven rolling-ball speed game inspired by Monkey Ball.
//!
//! Roll a dynamic physics sphere down a course of **shallow half-pipe** track
//! segments: descend to build real speed, bank on the curved surface, brake and
//! **spin-launch** (Sonic-style) to rocket up the final ramp to the finish gate.
//!
//! The core ([`GravixGame`]) is engine-agnostic, deterministic, and native-tested:
//! it owns one `axiom-physics` world whose track is built from **heightfield
//! colliders** (the module's static curved-surface collider) generated from the
//! same half-pipe height function that meshes the visible surface, so the ball
//! rolls exactly where the track looks. It drives the ball with camera-relative
//! force/torque through the physics facade (never teleporting), runs the
//! [`spin`]-launch state machine, follows with a [`chase_camera`], and resolves
//! finish / fall / reset. The `wasm32` edge ([`web`]) captures input, installs the
//! meshed scene once, and updates the ball + camera each frame. This is an
//! app-tier composition root — the only place physics, the terrain mesher, and the
//! renderer meet.

pub mod chase_camera;
pub mod course;
pub mod halfpipe;
pub mod settings;
pub mod spin;
pub use spin::SpinState;
#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
pub use web::gravix_start;

use axiom::prelude::{
    Angle, App, Camera, Color, DefaultPlugins, DirectionalLight, Entity, Material, Mesh, MeshData,
    PerspectiveProjection, PointLight, RunningApp, Spawn, Transform, Vec3, Window,
};
use axiom_kernel::{FrameIndex, Meters, Ratio, Tick};
use axiom_math::Quat;
use axiom_physics::{PhysicsApi, PhysicsBodyHandle};
use axiom_runtime::RuntimeStep;

use crate::gravix::chase_camera::ChaseCamera;
use crate::gravix::course::Course;
use crate::gravix::spin::SpinController;

/// The canvas id the browser demo binds its surface to.
pub const CANVAS_ID: &str = "axiom-gravix-canvas";

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

/// The live per-instance buffer capacity — above the track + marker + ball count.
pub const LIVE_CAPACITY: u32 = 8192;

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).expect("gravix authored a finite ratio")
}

fn meters(v: f32) -> Meters {
    Meters::finite_or_zero(v)
}

fn runtime_step(n: u64) -> RuntimeStep {
    RuntimeStep::new(FrameIndex::new(n), Tick::new(n), settings::FIXED_STEP_NANOS, n)
}

fn horizontal_speed(v: Vec3) -> f32 {
    (v.x * v.x + v.z * v.z).sqrt()
}

/// Held / edge input for one frame. Movement (WASD) is relative to where the
/// camera looks; the camera is aimed only by the mouse (`yaw_delta`/`pitch_delta`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Intent {
    /// W — drive away from the camera.
    pub forward: bool,
    /// S — drive toward the camera.
    pub back: bool,
    /// A — strafe the ball left (relative to the camera).
    pub left: bool,
    /// D — strafe the ball right (relative to the camera).
    pub right: bool,
    /// Shift — brake (and, when nearly stopped, gate spin charging).
    pub brake: bool,
    /// A move key was *pressed this frame* (edge) — charges spin while braked.
    pub tap: bool,
    /// Mouse motion this frame (pixels) that orbits the camera — horizontal /
    /// vertical. The camera responds to nothing else.
    pub yaw_delta: f32,
    pub pitch_delta: f32,
    /// Restart the run (acted on any time; primarily after finish / fall-out).
    pub restart: bool,
}

/// The high-level game phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Rolling,
    Finished,
    FellOut,
}

/// The deterministic game core: one physics world, the ball, the course, the spin
/// controller, the chase camera, and the run state.
#[derive(Debug)]
pub struct GravixGame {
    physics: PhysicsApi,
    ball: PhysicsBodyHandle,
    course: Course,
    spin: SpinController,
    cam: ChaseCamera,
    phase: Phase,
    step_n: u64,
    ball_pos: Vec3,
    ball_rot: [f32; 4],
    finish_ticks: u32,
}

impl GravixGame {
    /// A fresh game at the start of the course.
    pub fn new() -> Self {
        let course = course::generate();
        let (physics, ball) = build_world(&course);
        let cam = ChaseCamera::new(initial_yaw(&course), course.spawn);
        let spawn = course.spawn;
        GravixGame {
            physics,
            ball,
            course,
            spin: SpinController::new(),
            cam,
            phase: Phase::Rolling,
            step_n: 0,
            ball_pos: spawn,
            ball_rot: [0.0, 0.0, 0.0, 1.0],
            finish_ticks: 0,
        }
    }

    /// Reset the ball to the spawn at rest and resume rolling.
    pub fn restart(&mut self) {
        self.physics
            .set_body_transform(self.ball, Transform::from_translation(self.course.spawn))
            .expect("teleport ball to spawn");
        self.physics
            .set_body_velocity(self.ball, Vec3::ZERO, Vec3::ZERO)
            .expect("reset ball velocity");
        self.spin = SpinController::new();
        self.cam = ChaseCamera::new(initial_yaw(&self.course), self.course.spawn);
        self.phase = Phase::Rolling;
        self.ball_pos = self.course.spawn;
        self.ball_rot = [0.0, 0.0, 0.0, 1.0];
        self.finish_ticks = 0;
    }

    /// The current phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The current spin-launch state (for the HUD / debug).
    pub fn spin_state(&self) -> spin::SpinState {
        self.spin.state()
    }

    /// The ball's world position.
    pub fn ball_position(&self) -> Vec3 {
        self.ball_pos
    }

    /// The ball's current horizontal speed.
    pub fn speed(&self) -> f32 {
        horizontal_speed(self.ball_velocity().0)
    }

    /// The camera eye + look target for the current framing.
    pub fn camera(&self) -> (Vec3, Vec3) {
        self.cam.eye_target(self.ball_pos)
    }

    /// Refresh the meshed scene (ball pose + camera) for the current game state.
    pub fn render(&self, running: &mut RunningApp, scene: &mut GravixScene) {
        let (eye, target) = self.camera();
        scene.update(running, self.ball_pos, self.ball_rot, eye, target);
    }

    /// Advance one fixed step under `intent`.
    pub fn step(&mut self, intent: Intent) {
        let dt = settings::FIXED_STEP_NANOS as f32 / 1_000_000_000.0;
        if intent.restart {
            self.restart();
            return;
        }
        match self.phase {
            Phase::Rolling => self.step_rolling(intent, dt),
            // Finished / fell out: freeze the sim, keep the camera live, auto-reset a
            // fall after a moment (finish waits for restart).
            Phase::FellOut => {
                self.finish_ticks += 1;
                self.cam.update(self.ball_pos, intent.yaw_delta, intent.pitch_delta, dt);
                if self.finish_ticks > 45 {
                    self.restart();
                }
            }
            Phase::Finished => {
                self.finish_ticks += 1;
                self.cam.update(self.ball_pos, intent.yaw_delta, intent.pitch_delta, dt);
            }
        }
    }

    fn step_rolling(&mut self, intent: Intent, dt: f32) {
        let (fwd, right) = self.cam.ground_basis();
        let drive = intent_direction(intent, fwd, right);
        let grounded = self.grounded();
        let (lin, ang) = self.ball_velocity();
        let speed = horizontal_speed(lin);

        let tap = intent.tap.then_some(drive.unwrap_or(fwd));
        let out = self.spin.update(intent.brake, tap, speed, dt);

        // Exactly one physical drive applies per step (mutually exclusive states).
        match out.launch {
            Some((dir, charge)) => self.apply_launch(dir, charge),
            None => {
                if out.spin_visual > 0.0 {
                    self.apply_charge_spin(out.spin_visual, lin, dt);
                } else if out.braking {
                    self.apply_brake(lin, ang, dt);
                } else if out.allow_move {
                    drive.into_iter().for_each(|d| self.apply_drive(d, grounded, speed, dt));
                }
            }
        }

        self.physics.step(runtime_step(self.step_n)).expect("physics step");
        self.step_n += 1;
        self.read_ball();
        self.clamp_speed();
        self.cam.update(self.ball_pos, intent.yaw_delta, intent.pitch_delta, dt);
        self.check_finish();
        self.check_fall();
    }

    /// Camera-relative drive: a linear accelerator (primary) + a roll torque
    /// (visible spin/bank), attenuated toward a soft top speed and reduced airborne.
    fn apply_drive(&mut self, dir: Vec3, grounded: bool, speed: f32, _dt: f32) {
        let atten = 1.0 / (1.0 + (speed / settings::DRIVE_SPEED_REFERENCE.max(0.001)).powf(settings::DRIVE_SPEED_EXPONENT));
        let control = if grounded { 1.0 } else { settings::AIR_CONTROL };
        // Split the input direction into forward-ness (drive) and lateral (steer)
        // magnitudes so straight-ahead uses the stronger DRIVE_FORCE.
        let force = settings::DRIVE_FORCE * atten * control;
        self.physics.apply_force(self.ball, dir.mul_scalar(force)).expect("drive force");
        let axis = Vec3::new(dir.z, 0.0, -dir.x);
        self.physics
            .apply_torque(self.ball, axis.mul_scalar(settings::ROLL_TORQUE * atten * control))
            .expect("roll torque");
    }

    fn apply_brake(&mut self, lin: Vec3, ang: Vec3, dt: f32) {
        let kl = (-settings::BRAKE_LINEAR_DECAY * dt).exp();
        let ka = (-settings::BRAKE_ANGULAR_DECAY * dt).exp();
        self.physics
            .set_body_velocity(self.ball, lin.mul_scalar(kl), ang.mul_scalar(ka))
            .expect("brake");
    }

    /// While charging: bleed the linear velocity toward a stop (so the ball holds
    /// its spot) while spinning it in place about the charge axis (visible wind-up).
    fn apply_charge_spin(&mut self, spin_rate: f32, lin: Vec3, dt: f32) {
        let kl = (-settings::BRAKE_LINEAR_DECAY * dt).exp();
        let dir = self.spin_dir();
        let axis = Vec3::new(dir.z, 0.0, -dir.x);
        self.physics
            .set_body_velocity(self.ball, lin.mul_scalar(kl), axis.mul_scalar(spin_rate))
            .expect("charge spin");
    }

    /// Convert stored spin charge into a launch: forward linear velocity + a
    /// matching roll angular velocity along the charged direction.
    fn apply_launch(&mut self, dir: Vec3, charge: f32) {
        let lin = dir.mul_scalar(settings::SPIN_LAUNCH_LINEAR * charge);
        let axis = Vec3::new(dir.z, 0.0, -dir.x);
        let ang = axis.mul_scalar(settings::SPIN_LAUNCH_ANGULAR * charge);
        self.physics.set_body_velocity(self.ball, lin, ang).expect("launch");
    }

    /// The current camera-relative forward (the charge default direction).
    fn spin_dir(&self) -> Vec3 {
        self.cam.facing()
    }

    fn ball_velocity(&self) -> (Vec3, Vec3) {
        self.physics
            .snapshot()
            .bodies()
            .iter()
            .find(|b| b.handle() == self.ball)
            .map(|b| (b.linear_velocity(), b.angular_velocity()))
            .unwrap_or((Vec3::ZERO, Vec3::ZERO))
    }

    fn read_ball(&mut self) {
        if let Some(b) = self.physics.snapshot().bodies().iter().find(|b| b.handle() == self.ball) {
            let t = b.transform();
            self.ball_pos = t.translation;
            let r = t.rotation;
            self.ball_rot = [r.x, r.y, r.z, r.w];
        }
    }

    /// Safety cap: rescale the horizontal velocity if it exceeds `MAX_SPEED`.
    fn clamp_speed(&mut self) {
        let (lin, ang) = self.ball_velocity();
        let speed = horizontal_speed(lin);
        if speed > settings::MAX_SPEED {
            let k = settings::MAX_SPEED / speed;
            let capped = Vec3::new(lin.x * k, lin.y, lin.z * k);
            self.physics.set_body_velocity(self.ball, capped, ang).expect("cap speed");
        }
    }

    /// Whether the ball rests on a surface (an upward contact normal), gating air
    /// control.
    fn grounded(&self) -> bool {
        self.physics.latest_contacts().iter().any(|c| {
            let up = if c.body_b() == self.ball {
                c.normal().y
            } else if c.body_a() == self.ball {
                -c.normal().y
            } else {
                return false;
            };
            up >= settings::GROUNDED_NORMAL_Y
        })
    }

    fn check_finish(&mut self) {
        let d = self.ball_pos.subtract(self.course.finish);
        let horiz = (d.x * d.x + d.z * d.z).sqrt();
        if horiz <= settings::FINISH_RADIUS && d.y.abs() < 4.0 {
            self.phase = Phase::Finished;
            self.finish_ticks = 0;
        }
    }

    fn check_fall(&mut self) {
        if self.ball_pos.y < self.course.kill_plane_y {
            self.phase = Phase::FellOut;
            self.finish_ticks = 0;
        }
    }

    /// A deterministic debug read of the ball + control state at the current tick,
    /// for inspecting the run without a renderer.
    pub fn debug_readout(&self) -> GravixReadout {
        GravixReadout {
            step: self.step_n,
            phase: self.phase,
            spin: self.spin.state(),
            charge: self.spin.charge(),
            ball: self.ball_pos,
            speed: self.speed(),
            grounded: self.grounded(),
        }
    }
}

impl Default for GravixGame {
    fn default() -> Self {
        GravixGame::new()
    }
}

/// A one-tick debug snapshot of the game (see [`GravixGame::debug_readout`]).
#[derive(Debug, Clone, Copy)]
pub struct GravixReadout {
    pub step: u64,
    pub phase: Phase,
    pub spin: spin::SpinState,
    pub charge: f32,
    pub ball: Vec3,
    pub speed: f32,
    pub grounded: bool,
}

/// The initial camera yaw (radians) that looks down the first segment — so the
/// run starts framed along the course before the player takes the mouse.
fn initial_yaw(course: &Course) -> f32 {
    let f = course.segments[0].forward;
    f.x.atan2(f.z)
}

/// The camera-relative move direction from held input (`None` if no key held).
fn intent_direction(intent: Intent, fwd: Vec3, right: Vec3) -> Option<Vec3> {
    let mut d = Vec3::ZERO;
    if intent.forward {
        d = d.add(fwd);
    }
    if intent.back {
        d = d.subtract(fwd);
    }
    if intent.right {
        d = d.add(right);
    }
    if intent.left {
        d = d.subtract(right);
    }
    let len = (d.x * d.x + d.z * d.z).sqrt();
    (len > 1.0e-4).then(|| Vec3::new(d.x / len, 0.0, d.z / len))
}

/// Build the physics world for a course: one static **heightfield collider** per
/// half-pipe segment (the track), and the dynamic ball sphere.
fn build_world(course: &Course) -> (PhysicsApi, PhysicsBodyHandle) {
    let mut physics = PhysicsApi::with_config(
        settings::GRAVITY,
        settings::SOLVER_ITERATIONS,
        4096,
        4096,
        settings::MAX_SUBSTEPS,
        true,
        ratio(settings::LINEAR_DAMPING),
        ratio(settings::ANGULAR_DAMPING),
    )
    .expect("valid physics config");

    let track_mat = PhysicsApi::material(ratio(settings::TRACK_FRICTION), ratio(settings::TRACK_RESTITUTION), ratio(1.0))
        .expect("track material");
    for seg in &course.segments {
        let body = physics.create_static_body(seg.transform()).expect("static segment body");
        let grid = seg.params.collider_grid();
        let heights: Vec<Meters> = grid.heights.iter().map(|h| meters(*h)).collect();
        physics
            .attach_heightfield_collider(body, grid.nx, grid.nz, meters(grid.spacing_x), meters(grid.spacing_z), &heights, track_mat, false)
            .expect("segment heightfield collider");
    }

    let ball_mat = PhysicsApi::material(ratio(settings::BALL_FRICTION), ratio(settings::BALL_RESTITUTION), ratio(1.0))
        .expect("ball material");
    let ball = physics
        .create_dynamic_body(Transform::from_translation(course.spawn), ratio(settings::BALL_MASS))
        .expect("ball body");
    physics
        .attach_sphere_collider(ball, meters(settings::BALL_RADIUS), ball_mat, false)
        .expect("ball collider");
    (physics, ball)
}

// --- rendering (persistent meshed scene: install once, update per frame) -----

/// The neon palette.
mod palette {
    pub const TRACK: [f32; 3] = [0.20, 0.10, 0.42];
    pub const LIP: [f32; 3] = [0.62, 0.20, 0.72];
    pub const STRIPE: [f32; 3] = [0.95, 0.85, 0.35];
    pub const BALL: [f32; 3] = [1.0, 0.55, 0.15];
    pub const FINISH: [f32; 3] = [0.10, 0.85, 0.70];
}

fn color3(rgb: [f32; 3]) -> Color {
    Color::linear_rgb(ratio(rgb[0]), ratio(rgb[1]), ratio(rgb[2]))
}

/// Convert a terrain `GridMesh` into engine `MeshData` (no UVs — flat-lit track).
fn to_mesh_data(mesh: &axiom_terrain_mesh::GridMesh) -> MeshData {
    MeshData::new(mesh.positions().to_vec(), mesh.normals().to_vec(), Vec::new(), mesh.indices().to_vec())
}

/// The persistent meshed scene: the static track/finish/stripes are spawned once
/// (custom-mesh handles survive because we never `reauthor`); the ball + camera are
/// refreshed each frame via `despawn`/`spawn` + `set_camera`.
#[derive(Debug)]
pub struct GravixScene {
    ball_mesh: axiom::prelude::Handle<Mesh>,
    ball_mat: axiom::prelude::Handle<Material>,
    ball_entity: Option<Entity>,
}

impl GravixScene {
    fn install(app: &mut RunningApp, course: &Course) -> Self {
        let track_mat = app.add_material(Material::lit(color3(palette::TRACK)));
        // The uphill spin-launch-reward segment gets a distinct lip colour so the
        // player reads it as the "charge and launch" ramp.
        let reward_mat = app.add_material(Material::lit(color3(palette::LIP)));
        let stripe_mat = app.add_material(Material::lit(color3(palette::STRIPE)));
        let finish_mat = app.add_material(Material::lit(color3(palette::FINISH)));
        let ball_mat = app.add_material(Material::lit(color3(palette::BALL)));
        let cube = app.add_mesh(Mesh::cube());
        let sphere = app.add_mesh(Mesh::sphere());

        for seg in &course.segments {
            let mesh = app.add_mesh_data(to_mesh_data(&seg.params.surface_mesh())).expect("segment mesh registers");
            let mat = if seg.is_launch_reward { reward_mat } else { track_mat };
            app.spawn(Spawn::new(seg.transform(), mesh, mat));
            // Speed-stripe markers across the track at intervals along its length.
            spawn_stripes(app, seg, cube, stripe_mat);
        }

        // Finish gate: a bright pillar pair at the finish.
        let up = course.segments.last().unwrap().up;
        let right = Vec3::new(up.z, 0.0, -up.x); // any horizontal perpendicular-ish
        [right.mul_scalar(3.0), right.mul_scalar(-3.0)].iter().for_each(|off| {
            app.spawn(Spawn::new(
                Transform::new(course.finish.add(*off).add(Vec3::new(0.0, 2.5, 0.0)), Quat::IDENTITY, Vec3::new(0.8, 5.0, 0.8)),
                cube,
                finish_mat,
            ));
        });

        // Light rig.
        app.add_light(
            DirectionalLight { direction: Vec3::new(0.35, -1.0, 0.28), color: Color::WHITE, intensity: ratio(1.0) },
            Transform::IDENTITY,
        );
        app.add_point_light(
            PointLight { color: Color::WHITE, intensity: ratio(120.0) },
            Transform::from_translation(course.spawn.add(Vec3::new(0.0, 20.0, 0.0))),
        );

        GravixScene { ball_mesh: sphere, ball_mat, ball_entity: None }
    }

    pub fn update(&mut self, app: &mut RunningApp, ball_pos: Vec3, ball_rot: [f32; 4], eye: Vec3, target: Vec3) {
        app.set_camera(
            Camera::perspective(PerspectiveProjection {
                fov_y: Angle::degrees(60.0),
                near: meters(0.1),
                far: meters(600.0),
            }),
            Transform::from_translation(eye).looking_at(target, Vec3::UNIT_Y).expect("camera aims at the ball"),
        );
        if let Some(e) = self.ball_entity.take() {
            app.despawn(e);
        }
        let d = settings::BALL_RADIUS * 2.0;
        let rot = Quat::new(ball_rot[0], ball_rot[1], ball_rot[2], ball_rot[3]);
        self.ball_entity = Some(app.spawn(Spawn::new(Transform::new(ball_pos, rot, Vec3::new(d, d, d)), self.ball_mesh, self.ball_mat)));
    }
}

/// Spawn evenly-spaced bright stripe markers across a segment's width, so speed is
/// perceptible as they flash past.
fn spawn_stripes(app: &mut RunningApp, seg: &course::Segment, cube: axiom::prelude::Handle<Mesh>, mat: axiom::prelude::Handle<Material>) {
    let (half_x, half_z) = seg.params.half_extents();
    let count = ((2.0 * half_z) / 8.0).round().max(1.0) as i32;
    for i in 0..=count {
        let t = if count == 0 { 0.5 } else { i as f32 / count as f32 };
        let along = -half_z + t * 2.0 * half_z;
        let center = seg.center.add(seg.forward.mul_scalar(along)).add(seg.up.mul_scalar(0.06));
        app.spawn(Spawn::new(
            Transform::new(center, seg.rotation, Vec3::new(2.0 * half_x * 0.96, 0.12, 0.5)),
            cube,
            mat,
        ));
    }
}

/// Build a live `RunningApp` for a game state with the meshed scene installed and
/// the ball/camera placed at the current pose.
pub fn live_app(game: &GravixGame) -> (RunningApp, GravixScene) {
    let mut running = App::new()
        .window(
            Window::new(WIDTH, HEIGHT)
                .with_surface_id(CANVAS_ID)
                .with_clear_color(color3([0.02, 0.02, 0.06])),
        )
        .add_plugins(DefaultPlugins)
        .setup(|_world, _meshes, _materials| {})
        .build();
    let mut scene = GravixScene::install(&mut running, &game.course);
    let (eye, target) = game.camera();
    scene.update(&mut running, game.ball_pos, game.ball_rot, eye, target);
    (running, scene)
}

/// Build a headless `RunningApp` of the course, framed at the spawn — for the
/// native capture harness (`axiom-shot`) and tests.
pub fn build_gravix() -> RunningApp {
    let (running, _scene) = live_app(&GravixGame::new());
    running
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_game_starts_rolling_at_the_spawn() {
        let game = GravixGame::new();
        assert_eq!(game.phase(), Phase::Rolling);
        assert_eq!(game.spin_state(), spin::SpinState::NormalRolling);
        assert!(game.course.segments.len() >= 5);
    }

    #[test]
    fn the_ball_rolls_down_the_course_and_gains_speed() {
        let mut game = GravixGame::new();
        // Let it settle onto the track, then it should slide/roll downhill under
        // gravity + the shallow bank, gaining horizontal speed.
        for _ in 0..30 {
            game.step(Intent::default());
        }
        let early = game.speed();
        for _ in 0..90 {
            game.step(Intent::default());
        }
        let late = game.speed();
        assert!(late > early + 1.0, "the ball accelerates downhill: {early} -> {late}");
        // It stays on the track (above the kill plane) rather than falling through.
        assert!(game.ball_position().y > game.course.kill_plane_y, "ball stays on the course");
    }

    #[test]
    fn forward_input_drives_the_ball_camera_relative() {
        let mut game = GravixGame::new();
        for _ in 0..20 {
            game.step(Intent::default());
        }
        let (fwd, _) = game.cam.ground_basis();
        let before = game.ball_position();
        for _ in 0..60 {
            game.step(Intent { forward: true, ..Intent::default() });
        }
        let moved = game.ball_position().subtract(before);
        // Net motion has a positive component along the camera forward.
        assert!(moved.dot(fwd) > 0.5, "forward input drives along camera forward");
    }

    #[test]
    fn falling_off_the_course_resets_after_a_moment() {
        let mut game = GravixGame::new();
        game.physics
            .set_body_transform(game.ball, Transform::from_translation(Vec3::new(0.0, game.course.kill_plane_y - 10.0, 0.0)))
            .unwrap();
        game.step(Intent::default());
        assert_eq!(game.phase(), Phase::FellOut);
        // After the reset delay it returns to rolling at the spawn.
        for _ in 0..60 {
            game.step(Intent::default());
        }
        assert_eq!(game.phase(), Phase::Rolling);
    }

    #[test]
    fn reaching_the_finish_gate_finishes_the_run() {
        let mut game = GravixGame::new();
        // Teleport the ball onto the finish gate; the next step detects it.
        game.physics
            .set_body_transform(game.ball, Transform::from_translation(game.course.finish))
            .unwrap();
        game.step(Intent::default());
        assert_eq!(game.phase(), Phase::Finished);
    }

    #[test]
    fn two_identical_runs_are_deterministic() {
        let script = [
            Intent { forward: true, ..Intent::default() },
            Intent { forward: true, right: true, ..Intent::default() },
            Intent { brake: true, tap: true, ..Intent::default() },
        ];
        let run = || {
            let mut g = GravixGame::new();
            (0..90).for_each(|i| g.step(script[i % script.len()]));
            g.debug_readout().ball
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn the_debug_readout_reports_the_run_state() {
        let mut game = GravixGame::new();
        for _ in 0..10 {
            game.step(Intent::default());
        }
        let r = game.debug_readout();
        assert_eq!(r.phase, Phase::Rolling);
        assert!(r.step >= 10);
        assert_eq!(r.ball, game.ball_position());
    }

    #[test]
    fn the_course_builds_a_renderable_app() {
        let mut app = build_gravix();
        let outcome = app.tick(0);
        assert!(!outcome.draws().is_empty(), "the course renders draws");
    }
}


