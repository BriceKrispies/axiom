//! **Gravix** — a marble-roll platformer. Steer a physics marble with
//! camera-relative roll torque across procedurally-generated floating platform
//! courses, over ramps and across jump gaps, collecting coins and reaching the
//! finish pad. Endless levels, three falls per run.
//!
//! The core (`GravixGame`) is engine-agnostic, deterministic, and native-tested:
//! it owns one `axiom-physics` world, steers the marble by torque (which the
//! engine's contact-point friction converts to real rolling), reads back the
//! marble pose from the physics snapshot each step, and drives the win / fall /
//! coin / camera logic. The thin `wasm32` edge (`web`) captures input, drives the
//! windowing render loop, and paints the HUD. This is an app-tier composition
//! root — it is the only place that translates between the physics module's
//! contract and the renderer's.

pub mod camera;
pub mod level;
pub mod procgen;
pub mod settings;
#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
pub use web::gravix_start;

use axiom::prelude::{
    Angle, App, Assets, Camera, Color, DefaultPlugins, DirectionalLight, Material, Mesh,
    PerspectiveProjection, PointLight, Renderable, RunningApp, SceneCommands, Transform, Vec3,
    Window,
};
use axiom_kernel::{FrameIndex, Meters, Ratio, Tick};
use axiom_math::Quat;
use axiom_physics::{PhysicsApi, PhysicsBodyHandle};
use axiom_runtime::RuntimeStep;

use crate::gravix::camera::OrbitCamera;
use crate::gravix::level::{LevelDescriptor, Platform, SurfaceKind};

/// The canvas id the browser demo binds its surface to (matches the gallery
/// manifest's `canvasId`).
pub const CANVAS_ID: &str = "axiom-gravix-canvas";

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;

/// The live per-instance buffer capacity — comfortably above the tile + coin +
/// marble count of any generated course.
pub const LIVE_CAPACITY: u32 = 4096;

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).expect("gravix authored a finite ratio")
}

fn meters(v: f32) -> Meters {
    Meters::new(v).expect("gravix authored a finite length")
}

fn color3(rgb: [f32; 3]) -> Color {
    Color::linear_rgb(ratio(rgb[0]), ratio(rgb[1]), ratio(rgb[2]))
}

/// Build the explicit fixed runtime step for physics step `n`.
fn runtime_step(n: u64) -> RuntimeStep {
    RuntimeStep::new(FrameIndex::new(n), Tick::new(n), settings::FIXED_STEP_NANOS, n)
}

/// The primitive mesh a render instance uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GravixMesh {
    Cube,
    Sphere,
}

/// One thing to draw this frame: an oriented transform, a primitive, and a colour.
#[derive(Clone, Copy, Debug)]
pub struct RenderInstance {
    pub transform: Transform,
    pub mesh: GravixMesh,
    pub color: [f32; 3],
}

/// The neon palette (purple/magenta course, teal/orange pads, gold coins, bright
/// marble). Exposed as a fixed ordered set so the live re-author loop registers a
/// stable material id per colour every frame (the windowing backend captures the
/// material set once).
mod palette {
    pub const PATH: [f32; 3] = [0.30, 0.10, 0.46];
    pub const PATH_WIDE: [f32; 3] = [0.62, 0.18, 0.70];
    pub const PLAZA: [f32; 3] = [0.48, 0.16, 0.62];
    pub const RAMP: [f32; 3] = [0.70, 0.22, 0.66];
    pub const LATTICE: [f32; 3] = [0.05, 0.55, 0.60];
    pub const MARBLE: [f32; 3] = [1.0, 0.92, 0.45];
    pub const COIN: [f32; 3] = [1.0, 0.72, 0.16];
    pub const START: [f32; 3] = [0.05, 0.72, 0.78];
    pub const END: [f32; 3] = [1.0, 0.45, 0.16];

    /// Every palette colour in a fixed order — one material is registered per
    /// entry, so ids stay stable across re-authors.
    pub const ALL: [[f32; 3]; 9] =
        [PATH, PATH_WIDE, PLAZA, RAMP, LATTICE, MARBLE, COIN, START, END];

    /// The palette index of a colour (an instance colour is always a palette
    /// entry; an unknown colour falls back to index 0).
    pub fn index(color: [f32; 3]) -> usize {
        ALL.iter().position(|c| *c == color).unwrap_or(0)
    }
}

fn platform_color(kind: SurfaceKind) -> [f32; 3] {
    match kind {
        SurfaceKind::Plaza => palette::PLAZA,
        SurfaceKind::Path => palette::PATH,
        SurfaceKind::PathWide => palette::PATH_WIDE,
        SurfaceKind::Ramp => palette::RAMP,
        SurfaceKind::Lattice => palette::LATTICE,
    }
}

/// Held/edge input for one frame (camera-relative; the marble steers by roll).
#[derive(Clone, Copy, Debug, Default)]
pub struct Intent {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub brake: bool,
    pub jump: bool,
    pub yaw_left: bool,
    pub yaw_right: bool,
    pub pitch_up: bool,
    pub pitch_down: bool,
    /// Edge-triggered: restart the run (only acted on when the run is over).
    pub restart: bool,
}

/// The high-level game phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Playing,
    LevelComplete,
    Dead,
    RunOver,
}

/// The deterministic game core: one physics world, the marble, the current
/// course, and the run state.
#[derive(Debug)]
pub struct GravixGame {
    physics: PhysicsApi,
    marble: PhysicsBodyHandle,
    level_index: u32,
    descriptor: LevelDescriptor,
    coin_taken: Vec<bool>,
    run_score: u32,
    falls: u32,
    started: bool,
    phase: Phase,
    cam: OrbitCamera,
    step_n: u64,
    /// Frames the current non-playing phase has been shown (auto-advances).
    phase_timer: u32,
    marble_pos: Vec3,
    marble_rot: [f32; 4],
}

impl GravixGame {
    /// A fresh game at level 0.
    pub fn new() -> Self {
        let descriptor = procgen::generate(0);
        let (physics, marble) = build_world(&descriptor);
        let cam = OrbitCamera::new(course_yaw(&descriptor));
        let spawn = descriptor.spawn;
        let coin_taken = vec![false; descriptor.coins.len()];
        GravixGame {
            physics,
            marble,
            level_index: 0,
            descriptor,
            coin_taken,
            run_score: 0,
            falls: 0,
            started: false,
            phase: Phase::Playing,
            cam,
            step_n: 0,
            phase_timer: 0,
            marble_pos: spawn,
            marble_rot: [0.0, 0.0, 0.0, 1.0],
        }
    }

    /// Load `index` as the current course, rebuilding the physics world.
    fn load_level(&mut self, index: u32) {
        self.descriptor = procgen::generate(index);
        let (physics, marble) = build_world(&self.descriptor);
        self.physics = physics;
        self.marble = marble;
        self.level_index = index;
        self.coin_taken = vec![false; self.descriptor.coins.len()];
        self.started = false;
        self.phase = Phase::Playing;
        self.phase_timer = 0;
        self.cam = OrbitCamera::new(course_yaw(&self.descriptor));
        self.marble_pos = self.descriptor.spawn;
        self.marble_rot = [0.0, 0.0, 0.0, 1.0];
    }

    /// Reset the whole run to level 0 with a full life count.
    fn restart_run(&mut self) {
        self.run_score = 0;
        self.falls = 0;
        self.load_level(0);
    }

    /// Teleport the marble back to the spawn point at rest (fall recovery).
    fn respawn_marble(&mut self) {
        self.physics
            .set_body_transform(self.marble, Transform::from_translation(self.descriptor.spawn))
            .expect("teleport marble to spawn");
        self.physics
            .set_body_velocity(self.marble, Vec3::ZERO, Vec3::ZERO)
            .expect("reset marble velocity");
        self.marble_pos = self.descriptor.spawn;
        self.started = false;
    }

    /// The current level (1-based, for display).
    pub fn level_number(&self) -> u32 {
        self.level_index + 1
    }

    /// The current phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Coins collected on the current level.
    pub fn coins_collected(&self) -> u32 {
        self.coin_taken.iter().filter(|c| **c).count() as u32
    }

    /// Total coins on the current level.
    pub fn coins_total(&self) -> u32 {
        self.descriptor.coins.len() as u32
    }

    /// Total coins banked this run.
    pub fn run_score(&self) -> u32 {
        self.run_score
    }

    /// Falls used this run.
    pub fn falls(&self) -> u32 {
        self.falls
    }

    /// The eye/target for the current camera framing.
    pub fn camera(&self) -> (Vec3, Vec3) {
        self.cam.eye_target(self.marble_pos)
    }

    /// Advance one fixed step under `intent`. Drives steering, physics, camera,
    /// coins, and the win / fall state machine.
    pub fn step(&mut self, intent: Intent) {
        let dt = settings::FIXED_STEP_NANOS as f32 / 1_000_000_000.0;
        // Camera orbit is always live (it frames the action in every phase).
        self.steer_camera(intent, dt);

        match self.phase {
            Phase::Playing => self.step_playing(intent, dt),
            Phase::LevelComplete => self.advance_after_delay(90, |g| {
                let next = g.level_index + 1;
                g.load_level(next);
            }),
            Phase::Dead => self.advance_after_delay(60, |g| {
                g.respawn_marble();
                g.phase = Phase::Playing;
                g.phase_timer = 0;
            }),
            Phase::RunOver => {
                // Wait for an explicit restart, or auto-restart after a while.
                self.phase_timer += 1;
                if intent.restart || self.phase_timer > 300 {
                    self.restart_run();
                }
            }
        }
    }

    /// Run one playing step.
    fn step_playing(&mut self, intent: Intent, dt: f32) {
        self.apply_steering(intent, dt);
        self.physics.step(runtime_step(self.step_n)).expect("physics step");
        self.step_n += 1;
        self.read_marble();
        self.collect_coins();
        self.check_win();
        self.check_fall();
    }

    /// Camera-relative roll torque + brake + jump.
    fn apply_steering(&mut self, intent: Intent, dt: f32) {
        let (fwd, right) = self.cam.ground_basis(self.marble_pos);
        let mut roll = Vec3::ZERO;
        if intent.forward {
            roll = roll.add(fwd);
        }
        if intent.back {
            roll = roll.subtract(fwd);
        }
        if intent.right {
            roll = roll.add(right);
        }
        if intent.left {
            roll = roll.subtract(right);
        }

        let (lin, ang) = self.marble_velocity();
        let speed_xz = (lin.x * lin.x + lin.z * lin.z).sqrt();
        let atten = 1.0
            / (1.0
                + (speed_xz / settings::ROLL_SPEED_REFERENCE.max(0.001))
                    .powf(settings::ROLL_SPEED_EXPONENT));

        let roll_len = (roll.x * roll.x + roll.z * roll.z).sqrt();
        if roll_len > 1.0e-5 {
            let dir = Vec3::new(roll.x / roll_len, 0.0, roll.z / roll_len);
            let brake_scale = if intent.brake {
                settings::BRAKE_TORQUE_SCALE
            } else {
                1.0
            };
            // Direct linear drive is the primary accelerator (responsive from a
            // standstill), scaled by the soft-top-speed attenuation.
            let force = settings::ROLL_FORCE * atten * brake_scale;
            self.physics
                .apply_force(self.marble, dir.mul_scalar(force))
                .expect("apply roll force");
            // Torque about worldUp × rollDir gives the visible forward roll.
            let axis = Vec3::new(dir.z, 0.0, -dir.x);
            let torque = settings::ROLL_TORQUE * atten * brake_scale;
            self.physics
                .apply_torque(self.marble, axis.mul_scalar(torque))
                .expect("apply roll torque");
        }

        if intent.brake {
            let kl = (-settings::BRAKE_LINEAR_DECAY * dt).exp();
            let ka = (-settings::BRAKE_ANGULAR_DECAY * dt).exp();
            self.physics
                .set_body_velocity(self.marble, lin.mul_scalar(kl), ang.mul_scalar(ka))
                .expect("brake marble");
        }

        if intent.jump && self.grounded() {
            self.physics
                .apply_impulse(self.marble, Vec3::new(0.0, settings::JUMP_IMPULSE, 0.0))
                .expect("apply jump");
        }
    }

    /// Read the marble's linear + angular velocity from the snapshot.
    fn marble_velocity(&self) -> (Vec3, Vec3) {
        self.physics
            .snapshot()
            .bodies()
            .iter()
            .find(|b| b.handle() == self.marble)
            .map(|b| (b.linear_velocity(), b.angular_velocity()))
            .unwrap_or((Vec3::ZERO, Vec3::ZERO))
    }

    /// Read the marble's world position + orientation from the snapshot.
    fn read_marble(&mut self) {
        if let Some(b) = self
            .physics
            .snapshot()
            .bodies()
            .iter()
            .find(|b| b.handle() == self.marble)
        {
            let t = b.transform();
            self.marble_pos = t.translation;
            let r = t.rotation;
            self.marble_rot = [r.x, r.y, r.z, r.w];
        }
    }

    /// Whether the marble is resting on something (a contact with a sufficiently
    /// upward normal), which gates jumping.
    fn grounded(&self) -> bool {
        self.physics.latest_contacts().iter().any(|c| {
            // Normal points from body_a to body_b (ascending handle order). Resolve
            // it to "away from the marble" and check it points up.
            let up = if c.body_b() == self.marble {
                c.normal().y
            } else if c.body_a() == self.marble {
                -c.normal().y
            } else {
                return false;
            };
            up >= settings::GROUNDED_NORMAL_Y
        })
    }

    /// Collect any coin the marble is now touching.
    fn collect_coins(&mut self) {
        let reach = settings::MARBLE_RADIUS + settings::COIN_PICKUP_RADIUS;
        let reach_sq = reach * reach;
        for (i, coin) in self.descriptor.coins.iter().enumerate() {
            if self.coin_taken[i] {
                continue;
            }
            if self.marble_pos.subtract(coin.position).length_squared() <= reach_sq {
                self.coin_taken[i] = true;
                self.run_score += 1;
            }
        }
    }

    /// Start-then-finish zone win test (touch the start pad, then the finish pad).
    fn check_win(&mut self) {
        if touches_zone(self.marble_pos, self.descriptor.start_zone.position, self.descriptor.start_zone.radius) {
            self.started = true;
        }
        if self.started
            && touches_zone(
                self.marble_pos,
                self.descriptor.end_zone.position,
                self.descriptor.end_zone.radius,
            )
        {
            self.phase = Phase::LevelComplete;
            self.phase_timer = 0;
        }
    }

    /// Fall-death test: below the kill plane costs a life (or ends the run).
    fn check_fall(&mut self) {
        if self.marble_pos.y >= self.descriptor.kill_plane_y {
            return;
        }
        self.falls += 1;
        if self.falls >= settings::RUN_MAX_FALLS {
            self.phase = Phase::RunOver;
            self.phase_timer = 0;
        } else {
            self.phase = Phase::Dead;
            self.phase_timer = 0;
        }
    }

    /// Apply orbit-camera input.
    fn steer_camera(&mut self, intent: Intent, dt: f32) {
        let mut yaw = 0.0;
        let mut pitch = 0.0;
        if intent.yaw_left {
            yaw += settings::CAMERA_YAW_SPEED * dt;
        }
        if intent.yaw_right {
            yaw -= settings::CAMERA_YAW_SPEED * dt;
        }
        if intent.pitch_up {
            pitch += settings::CAMERA_PITCH_SPEED * dt;
        }
        if intent.pitch_down {
            pitch -= settings::CAMERA_PITCH_SPEED * dt;
        }
        self.cam.steer(yaw, pitch);
    }

    /// Hold a non-playing phase for `frames`, then run `then` once.
    fn advance_after_delay(&mut self, frames: u32, then: impl FnOnce(&mut Self)) {
        self.phase_timer += 1;
        if self.phase_timer >= frames {
            then(self);
        }
    }

    /// The renderables for the current frame: platforms, coins, zones, and the
    /// marble at its live pose.
    pub fn render_instances(&self) -> Vec<RenderInstance> {
        let mut out = Vec::with_capacity(self.descriptor.platforms.len() + self.descriptor.coins.len() + 3);
        for p in &self.descriptor.platforms {
            out.push(platform_instance(p));
        }
        // Zones as thin discs.
        out.push(zone_instance(&self.descriptor.start_zone, palette::START));
        out.push(zone_instance(&self.descriptor.end_zone, palette::END));
        // Coins (uncollected) as small spheres.
        for (i, coin) in self.descriptor.coins.iter().enumerate() {
            if !self.coin_taken[i] {
                out.push(RenderInstance {
                    transform: Transform::new(coin.position, Quat::IDENTITY, Vec3::new(0.4, 0.4, 0.4)),
                    mesh: GravixMesh::Sphere,
                    color: palette::COIN,
                });
            }
        }
        // The marble at its live pose.
        let d = settings::MARBLE_RADIUS * 2.0;
        out.push(RenderInstance {
            transform: Transform::new(
                self.marble_pos,
                Quat::new(self.marble_rot[0], self.marble_rot[1], self.marble_rot[2], self.marble_rot[3]),
                Vec3::new(d, d, d),
            ),
            mesh: GravixMesh::Sphere,
            color: palette::MARBLE,
        });
        out
    }
}

impl Default for GravixGame {
    fn default() -> Self {
        GravixGame::new()
    }
}

/// Whether `pos` overlaps a flat circular zone (horizontal disc, a little Y slack).
fn touches_zone(pos: Vec3, center: Vec3, radius: f32) -> bool {
    let dx = pos.x - center.x;
    let dz = pos.z - center.z;
    let horiz = (dx * dx + dz * dz).sqrt();
    let reach = radius + settings::MARBLE_RADIUS * 0.92;
    horiz <= reach && pos.y >= center.y - 0.6 && pos.y <= center.y + 3.0
}

/// The course heading (yaw) from spawn toward the finish, plus the framing offset.
fn course_yaw(d: &LevelDescriptor) -> f32 {
    let dir = d.end_zone.position.subtract(d.spawn);
    let len = (dir.x * dir.x + dir.z * dir.z).sqrt();
    if len < 1.0e-3 {
        0.0
    } else {
        dir.x.atan2(dir.z) + settings::CAMERA_COURSE_YAW_OFFSET
    }
}

/// A platform's render instance (a unit cube scaled by full extents, oriented).
fn platform_instance(p: &Platform) -> RenderInstance {
    RenderInstance {
        transform: Transform::new(p.position, p.rotation, p.half_extents.mul_scalar(2.0)),
        mesh: GravixMesh::Cube,
        color: platform_color(p.kind),
    }
}

/// A zone disc render instance (a thin flat cube).
fn zone_instance(zone: &crate::gravix::level::Zone, color: [f32; 3]) -> RenderInstance {
    RenderInstance {
        transform: Transform::new(
            Vec3::new(zone.position.x, zone.position.y + 0.06, zone.position.z),
            Quat::IDENTITY,
            Vec3::new(zone.radius * 2.0, 0.1, zone.radius * 2.0),
        ),
        mesh: GravixMesh::Cube,
        color,
    }
}

/// Build a physics world for a descriptor: one static box per solid platform, and
/// the dynamic marble sphere (created last, so it holds the highest handle).
fn build_world(descriptor: &LevelDescriptor) -> (PhysicsApi, PhysicsBodyHandle) {
    let mut physics = PhysicsApi::with_config(
        settings::GRAVITY,
        settings::SOLVER_ITERATIONS,
        512,
        512,
        settings::MAX_SUBSTEPS,
        true,
        ratio(settings::LINEAR_DAMPING),
        ratio(settings::ANGULAR_DAMPING),
    )
    .expect("valid physics config");

    let platform_mat = PhysicsApi::material(
        ratio(settings::PLATFORM_FRICTION),
        ratio(settings::PLATFORM_RESTITUTION),
        ratio(1.0),
    )
    .expect("platform material");

    for p in &descriptor.platforms {
        if !p.kind.solid() {
            continue;
        }
        let body = physics
            .create_static_body(Transform::new(p.position, p.rotation, Vec3::ONE))
            .expect("static platform body");
        physics
            .attach_box_collider(body, p.half_extents, platform_mat, false)
            .expect("platform collider");
    }

    let marble_mat = PhysicsApi::material(
        ratio(settings::MARBLE_FRICTION),
        ratio(settings::MARBLE_RESTITUTION),
        ratio(1.0),
    )
    .expect("marble material");
    let marble = physics
        .create_dynamic_body(
            Transform::from_translation(descriptor.spawn),
            ratio(settings::MARBLE_MASS),
        )
        .expect("marble body");
    physics
        .attach_sphere_collider(marble, meters(settings::MARBLE_RADIUS), marble_mat, false)
        .expect("marble collider");

    (physics, marble)
}

/// Author the current frame's scene into the umbrella world: primitive meshes, a
/// lit material per instance, an orbit camera, and the light rig. Re-run every
/// frame by the browser loop via `RunningApp::reauthor`.
fn author_scene(
    world: &mut SceneCommands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<Material>,
    instances: &[RenderInstance],
    eye: Vec3,
    target: Vec3,
) {
    let cube = meshes.add(Mesh::cube());
    let sphere = meshes.add(Mesh::sphere());
    let mats: Vec<_> = palette::ALL
        .iter()
        .map(|c| materials.add(Material::lit(color3(*c))))
        .collect();
    for instance in instances {
        let mesh = match instance.mesh {
            GravixMesh::Cube => cube,
            GravixMesh::Sphere => sphere,
        };
        let material = mats[palette::index(instance.color)];
        world.spawn((instance.transform, Renderable { mesh, material }));
    }
    world.spawn((
        Transform::from_translation(eye)
            .looking_at(target, Vec3::UNIT_Y)
            .expect("camera aims at the marble"),
        Camera::perspective(PerspectiveProjection {
            fov_y: Angle::degrees(58.0),
            near: meters(0.1),
            far: meters(400.0),
        }),
    ));
    world.spawn((
        Transform::IDENTITY,
        DirectionalLight {
            direction: Vec3::new(0.35, -1.0, 0.25),
            color: Color::WHITE,
            intensity: ratio(1.0),
        },
    ));
    world.spawn((
        Transform::from_translation(Vec3::new(0.0, 24.0, 12.0)),
        PointLight {
            color: Color::WHITE,
            intensity: ratio(60.0),
        },
    ));
}

/// Build the initial live `RunningApp` for a game state, framed by its camera.
pub fn live_app(game: &GravixGame) -> RunningApp {
    let instances = game.render_instances();
    let (eye, target) = game.camera();
    App::new()
        .window(
            Window::new(WIDTH, HEIGHT)
                .with_surface_id(CANVAS_ID)
                .with_clear_color(color3([0.02, 0.02, 0.05])),
        )
        .add_plugins(DefaultPlugins)
        .setup(move |world, meshes, materials| {
            author_scene(world, meshes, materials, &instances, eye, target)
        })
        .build()
}

/// Build a headless `RunningApp` of level 0 at its spawn state, for the native
/// capture harness (`axiom-shot`) and tests.
pub fn build_gravix() -> RunningApp {
    live_app(&GravixGame::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_game_starts_playing_at_level_one() {
        let game = GravixGame::new();
        assert_eq!(game.phase(), Phase::Playing);
        assert_eq!(game.level_number(), 1);
        assert!(game.coins_total() > 0, "the course has coins");
    }

    #[test]
    fn the_marble_falls_under_gravity_when_unsupported() {
        // Spawn is above the pad; with no input the marble should at least settle
        // (its Y should not fly upward) after several steps.
        let mut game = GravixGame::new();
        let start_y = game.marble_pos.y;
        for _ in 0..30 {
            game.step(Intent::default());
        }
        assert!(game.marble_pos.y <= start_y + 0.05, "marble does not float up");
    }

    #[test]
    fn the_marble_rests_on_the_spawn_pad() {
        // After settling with no input the marble stays near the pad top, not
        // through it (proves sphere-vs-box collision holds it up).
        let mut game = GravixGame::new();
        for _ in 0..120 {
            game.step(Intent::default());
        }
        let pad_top = game.descriptor.start_zone.position.y;
        assert!(
            game.marble_pos.y > pad_top - 0.2,
            "marble rests on the pad (y {} vs pad top {})",
            game.marble_pos.y,
            pad_top
        );
    }

    #[test]
    fn forward_torque_rolls_the_marble_off_its_start() {
        // With sphere inertia + contact friction, sustained forward torque should
        // move the marble horizontally from where it settled.
        let mut game = GravixGame::new();
        for _ in 0..60 {
            game.step(Intent::default());
        }
        let settled = game.marble_pos;
        let intent = Intent {
            forward: true,
            ..Intent::default()
        };
        for _ in 0..120 {
            game.step(intent);
        }
        let moved = game.marble_pos.subtract(settled);
        let horiz = (moved.x * moved.x + moved.z * moved.z).sqrt();
        assert!(horiz > 0.5, "forward torque rolls the marble (moved {horiz})");
    }

    #[test]
    fn falling_off_the_world_costs_a_life() {
        let mut game = GravixGame::new();
        // Teleport the marble far below the kill plane and step once.
        game.physics
            .set_body_transform(
                game.marble,
                Transform::from_translation(Vec3::new(0.0, game.descriptor.kill_plane_y - 20.0, 0.0)),
            )
            .unwrap();
        game.step(Intent::default());
        assert!(game.falls() >= 1, "a fall was counted");
        assert!(matches!(game.phase(), Phase::Dead | Phase::RunOver));
    }

    #[test]
    fn the_live_app_renders_draws() {
        let game = GravixGame::new();
        let mut app = live_app(&game);
        let outcome = app.tick(0);
        assert!(!outcome.draws().is_empty(), "the course renders draws");
    }
}
