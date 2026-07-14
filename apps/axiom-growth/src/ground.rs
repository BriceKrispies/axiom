//! Native first-person ground-walk simulation — the player's walk lifted out of
//! the wasm-only viewer so it can run **headlessly** (tests, the agent driver) and
//! be the single source of truth the browser viewer also uses.
//! The browser `web` module's per-frame closure and this module share one movement
//! and height integration ([`step_first_person`]): horizontal motion is the engine's
//! yaw-rotated step, and the eye rides the vista-composited terrain
//! ([`crate::gameworld::sample_height_m_lod_vista`] at full detail). Keeping that in
//! one native function means the headless walk is byte-for-byte the walk the player
//! sees in the browser, and the agent that drives it climbs the very same mountain.
//! [`GroundSim`] wraps that step with the real engine [`RunningApp`] (camera +
//! `Controller`, driven by `tick_with_controls` — the same authority path the
//! browser player uses) and the generated planet + vista, so a caller just feeds
//! movement axes and reads back the pose and **height**.

use axiom::prelude::*;
use axiom_kernel::Radians;
use axiom_math::Quat;

use crate::gameworld::sample_height_m_lod_vista;
use crate::model_planet::PlanetSurfaceAtlas;
use crate::model_world::GameWorldLocalMap;
use crate::presets::PlanetPreset;
use crate::vista::{MountainVistaPlan, VistaConfig, VistaDirector};
use crate::Growth;

/// Metres walked per held movement tick (matches the browser viewer).
pub const MOVE_SPEED: f32 = 0.6;
/// Radians turned per held turn tick (matches the browser viewer).
pub const TURN_SPEED: f32 = 0.03;
/// Eye height above the terrain surface (human ~1.7 m).
pub const EYE_HEIGHT_M: f32 = 1.7;

/// A linear colour channel from a known-finite literal.
pub fn ch(value: f32) -> Ratio {
    Ratio::new(value).expect("authored colour channel is finite")
}

/// Map a normalised overworld map click `(u, v)` in `[0,1]` (u left→right, v
/// top→bottom) to a unit direction on the planet — the inverse of the
/// equirectangular projection the overworld map draws with.
pub fn map_pick_to_dir(u: f32, v: f32) -> Vec3 {
    let lat = std::f32::consts::FRAC_PI_2 - v.clamp(0.0, 1.0) * std::f32::consts::PI;
    let lon = -std::f32::consts::PI + u.clamp(0.0, 1.0) * std::f32::consts::TAU;
    axiom_math::unit_dir_from_lat_lon(
        Radians::new(lat).expect("map-pick latitude is finite"),
        Radians::new(lon).expect("map-pick longitude is finite"),
    )
}

/// The walking player's authoritative state (owned by the app, not the engine's
/// free-fly controller): world position `(x, z)` and look `yaw`/`pitch`. The
/// vertical seat is no longer mirrored here — the engine seats the eye absolutely
/// from the [`FirstPersonInput::with_seat_y`] the step emits, so there is no
/// shadow-Y to keep in lock-step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerState {
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
    pub pitch: f32,
}

/// One integrated tick: the engine input to apply, plus the surface height read.
#[derive(Clone, Copy, Debug)]
pub struct StepOutput {
    /// The first-person input to feed the engine: the horizontal step (look
    /// deltas) plus an absolute vertical seat (`with_seat_y`) that rides the
    /// terrain surface.
    pub control: FirstPersonInput,
    /// Absolute terrain height (metres) under the new position.
    pub ground_height_m: f32,
    /// The recentred eye Y (engine space) seated on that surface.
    pub eye_y: f32,
}

/// Integrate one tick of first-person movement against the vista-composited
/// terrain, updating `state` and returning the engine input + the height read.
/// `forward_axis`/`strafe_axis` are in `[-1, 1]` (scaled by [`MOVE_SPEED`]);
/// `yaw_delta`/`pitch_delta` are radians. This is the exact integration the
/// browser viewer's per-frame closure performs — the single source of truth so the
/// headless walk and the live walk never diverge.
#[allow(clippy::too_many_arguments)] // app-tier sim step, moved as-is in the gallery de-merge
pub fn step_first_person(
    state: &mut PlayerState,
    forward_axis: f32,
    strafe_axis: f32,
    yaw_delta: f32,
    pitch_delta: f32,
    atlas: &PlanetSurfaceAtlas,
    localmap: &GameWorldLocalMap,
    seed: u64,
    plan: &MountainVistaPlan,
    anchor_h: f32,
) -> StepOutput {
    // 1. Look: accumulate yaw, clamp pitch like the engine does.
    state.yaw += yaw_delta;
    state.pitch = (state.pitch + pitch_delta).clamp(-1.5, 1.5);

    // 2. Horizontal step: local -Z forward / +X right, yaw-rotated (yaw only, so it
    // stays horizontal), mirrored into (x, z) to stay in lock-step with the engine.
    let forward = forward_axis * MOVE_SPEED;
    let strafe = strafe_axis * MOVE_SPEED;
    let move_local = Vec3::new(strafe, 0.0, -forward);
    let yh = state.yaw * 0.5;
    let yaw_q = Quat::new(0.0, yh.sin(), 0.0, yh.cos());
    let world_step = yaw_q.rotate(move_local);
    state.x += world_step.x;
    state.z += world_step.z;

    // 3. Sample the vista-composited surface and seat the eye on it.
    let sampled =
        sample_height_m_lod_vista(atlas, localmap, seed, state.x, state.z, 0.0, Some(plan));
    let desired_y = sampled - anchor_h + EYE_HEIGHT_M;

    // 4. Build the engine input: the horizontal step (move_local carries no
    // vertical component) plus an explicit absolute vertical seat, so the engine
    // seats the eye exactly on the surface this tick — no shadow-Y delta needed.
    let control = FirstPersonInput::new(
        0,
        move_local,
        Angle::radians(yaw_delta),
        Angle::radians(pitch_delta),
    )
    .with_seat_y(Meters::finite_or_zero(desired_y));
    StepOutput {
        control,
        ground_height_m: sampled,
        eye_y: desired_y,
    }
}

/// Build the first-person engine app: one terrain renderable (so the engine
/// produces the terrain's MVP), a camera at the spawn facing -Z (the vista's
/// authored mountain heading), and a directional light. The far plane reaches past
/// the scenic mountain. Shared by the browser viewer and the headless sim.
pub fn build_first_person_app(
    surface_id: &str,
    width: u32,
    height: u32,
    spawn_x: f32,
    spawn_z: f32,
    eye_y: f32,
) -> RunningApp {
    let clear = Color::linear_rgb(ch(0.45), ch(0.62), ch(0.85)); // sky
    App::new()
        .window(
            Window::new(width, height)
                .with_surface_id(surface_id)
                .with_clear_color(clear),
        )
        .add_plugins(DefaultPlugins)
        .setup(move |world, meshes, materials| {
            let mesh = meshes.add(Mesh::cube());
            let material = materials.add(Material::lit(Color::WHITE));
            world.spawn((Transform::IDENTITY, Renderable { mesh, material }));
            world.spawn((
                Transform::from_translation(Vec3::new(spawn_x, eye_y, spawn_z)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(70.0),
                    near: Meters::new(0.3).expect("near plane is finite"),
                    far: Meters::new(24000.0).expect("far plane is finite"),
                }),
                Controller::new(0),
            ));
            world.spawn((
                Transform::IDENTITY,
                DirectionalLight {
                    direction: Vec3::new(0.35, -1.0, 0.25),
                    color: Color::WHITE,
                    intensity: ch(1.0),
                },
            ));
        })
        .build()
}

/// A headless first-person walk: the generated planet + scenic vista, the real
/// engine app, and the player state — driven one tick at a time by movement axes.
/// It reads height every tick, so a caller (a test, or the agent driver) can watch
/// the player climb the mountain without a browser.
#[derive(Debug)]
pub struct GroundSim {
    growth: Growth,
    localmap: GameWorldLocalMap,
    plan: MountainVistaPlan,
    anchor_h: f32,
    seed: u64,
    state: PlayerState,
    app: RunningApp,
    tick: u64,
    ground_height_m: f32,
}

impl GroundSim {
    /// A dummy surface id for the headless app (no surface is ever realized — the
    /// sim drives the scene, not a GPU present).
    const HEADLESS_SURFACE_ID: &'static str = "axiom-growth-headless";

    /// Generate a planet, compose the scenic vista at the map pick `(u, v)`, and
    /// stand the player on the flat spawn shelf facing the mountain.
    pub fn new(seed_str: &str, preset: PlanetPreset, sites: u32, u: f32, v: f32) -> Self {
        let growth = Growth::generate(seed_str, preset, sites);
        let seed = growth.seed.value;
        let dir = map_pick_to_dir(u, v);
        let localmap = GameWorldLocalMap::anchored_at(&growth.atlas, dir);
        let plan = VistaDirector::plan(&growth.atlas, &localmap, seed, VistaConfig::default());
        let anchor_h = plan.shelf_height_m;
        let (spawn_x, spawn_z) = plan.spawn_xz;
        let view_yaw = plan.view_yaw;
        let eye_y = EYE_HEIGHT_M; // shelf recentred to 0.
        let app =
            build_first_person_app(Self::HEADLESS_SURFACE_ID, 960, 600, spawn_x, spawn_z, eye_y);
        let ground_height_m = sample_height_m_lod_vista(
            &growth.atlas,
            &localmap,
            seed,
            spawn_x,
            spawn_z,
            0.0,
            Some(&plan),
        );
        Self {
            growth,
            localmap,
            plan,
            anchor_h,
            seed,
            state: PlayerState {
                x: spawn_x,
                z: spawn_z,
                yaw: view_yaw,
                pitch: 0.0,
            },
            app,
            tick: 0,
            ground_height_m,
        }
    }

    /// The world seed of the generated planet (for telemetry/repro).
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Advance one tick by movement axes: `forward_axis` (+1 forward / -1 back),
    /// `strafe_axis` (+1 right / -1 left), `turn_axis` (+1 left / -1 right). Drives
    /// the real engine via `tick_with_controls`, the browser player's path.
    pub fn step(&mut self, forward_axis: f32, strafe_axis: f32, turn_axis: f32) {
        let yaw_delta = turn_axis * TURN_SPEED;
        let out = step_first_person(
            &mut self.state,
            forward_axis,
            strafe_axis,
            yaw_delta,
            0.0,
            &self.growth.atlas,
            &self.localmap,
            self.seed,
            &self.plan,
            self.anchor_h,
        );
        let _ = self.app.tick_with_controls(self.tick, &[], &[out.control]);
        self.tick += 1;
        self.ground_height_m = out.ground_height_m;
    }

    /// `(x, z, yaw, pitch)` of the player.
    pub fn pose(&self) -> (f32, f32, f32, f32) {
        (self.state.x, self.state.z, self.state.yaw, self.state.pitch)
    }

    /// The player's current world-forward direction `(x, z)` (unit), for goal
    /// seeking — the engine's -Z forward rotated by yaw.
    pub fn forward_xz(&self) -> (f32, f32) {
        let yh = self.state.yaw * 0.5;
        let yaw_q = Quat::new(0.0, yh.sin(), 0.0, yh.cos());
        let f = yaw_q.rotate(Vec3::new(0.0, 0.0, -1.0));
        (f.x, f.z)
    }

    /// Absolute terrain height (metres) under the player.
    pub fn ground_height_m(&self) -> f32 {
        self.ground_height_m
    }

    /// How far above the flat spawn shelf the player currently stands (metres) —
    /// the climb metric: 0 at the spawn, ≈ the prominence at the summit.
    pub fn height_above_spawn_m(&self) -> f32 {
        self.ground_height_m - self.plan.shelf_height_m
    }

    /// Absolute eye height (metres).
    pub fn eye_height_m(&self) -> f32 {
        self.ground_height_m + EYE_HEIGHT_M
    }

    /// Summit horizontal position (metres).
    pub fn peak_xz(&self) -> (f32, f32) {
        self.plan.peak_xz
    }

    /// Spawn (shelf) horizontal position (metres) — where the climb sets out from.
    pub fn spawn_xz(&self) -> (f32, f32) {
        self.plan.spawn_xz
    }

    /// Absolute terrain height (metres) of the flat spawn shelf.
    pub fn shelf_height_m(&self) -> f32 {
        self.plan.shelf_height_m
    }

    /// Sample the absolute composited terrain height (metres) at a world point —
    /// used to seat a tagged ground point on the actual surface.
    pub fn ground_abs_at(&self, x: f32, z: f32) -> f32 {
        sample_height_m_lod_vista(
            &self.growth.atlas,
            &self.localmap,
            self.seed,
            x,
            z,
            0.0,
            Some(&self.plan),
        )
    }

    /// Absolute summit altitude (metres).
    pub fn peak_height_m(&self) -> f32 {
        self.plan.peak_height_m
    }

    /// The mountain prominence (relief, metres).
    pub fn prominence_m(&self) -> f32 {
        self.plan.prominence_m
    }

    /// Horizontal distance (metres) from the player to the summit.
    pub fn distance_to_peak_m(&self) -> f32 {
        let (px, pz) = self.plan.peak_xz;
        ((self.state.x - px).powi(2) + (self.state.z - pz).powi(2)).sqrt()
    }

    /// The current tick count.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Whether the player has effectively reached the top: within a step of the
    /// summit horizontally, or within 1% of the full prominence in height.
    pub fn reached_summit(&self) -> bool {
        self.distance_to_peak_m() <= MOVE_SPEED * 1.5
            || self.height_above_spawn_m() >= self.plan.prominence_m * 0.99
    }
}

/// The neutral render data for one off-screen capture — plain values only, **no
/// GPU types**. The `agent` bin feeds this to `axiom-gpu-backend`'s off-screen
/// path (the same one `tools/axiom-shot` uses) to produce the pixels. Keeping the
/// wgpu call in the bin (not the lib) keeps wgpu's symbols out of the crate's
/// wasm `cdylib`, exactly as the retro FPS agent bin does.
#[cfg(feature = "agent")]
#[derive(Debug, Clone)]
pub struct CaptureInputs {
    pub width: u32,
    pub height: u32,
    /// Terrain mesh in the standard 12-float layout (one identity-world instance).
    pub vertices: Vec<f32>,
    pub indices: Vec<u32>,
    /// The engine's camera view-projection for the current pose (the instance MVP,
    /// since the terrain world transform is identity).
    pub view_proj: [f32; 16],
    pub light_view_proj: [f32; 16],
    /// `(kind, direction/position, colour, intensity)` per light.
    pub lights: Vec<(u32, [f32; 3], [f32; 3], f32)>,
    pub clear: [f32; 4],
    /// The biome-atlas material `(width, height, rgba8)`.
    pub material: (u32, u32, Vec<u8>),
}

/// Off-screen capture data (the `agent` feature): everything the bin needs to
/// render the composed world from the camera's current pose, with no GPU types in
/// the lib.
#[cfg(feature = "agent")]
impl GroundSim {
    /// Capture width/height. Matches the headless app window's 1.6 aspect so the
    /// engine's `camera_view_proj` frames the shot without stretch.
    pub const SHOT_W: u32 = 960;
    pub const SHOT_H: u32 = 600;

    /// A **portrait** of the mountain from a vantage on one side: place the camera
    /// `distance` m out from the peak along the outward horizontal direction
    /// `(dir_x, dir_z)` (a unit vector), aim it at the peak, and gather the render
    /// inputs. This frames the whole Everest-scale spire rising against the sky —
    /// the natural "shoot the mountain's side" shot for each cardinal direction.
    /// The camera *position* is set at app build (only an initial *rotation* fails
    /// to stick — the engine's controller zeroes camera yaw each tick — so look is
    /// driven by the first-person yaw/pitch input, exactly as the live viewer aims).
    pub fn capture_portrait(&mut self, dir_x: f32, dir_z: f32, distance: f32) -> CaptureInputs {
        let (peak_x, peak_z) = self.plan.peak_xz;
        let cam_x = peak_x + dir_x * distance;
        let cam_z = peak_z + dir_z * distance;

        // Lift the camera to an aerial vantage above the terrain so intervening
        // ridges (e.g. the long flat vegetated spawn-approach shelf) never occlude
        // the peak — the spire frames consistently from every side.
        const ELEVATION_M: f32 = 700.0;
        let ground_abs = sample_height_m_lod_vista(
            &self.growth.atlas,
            &self.localmap,
            self.seed,
            cam_x,
            cam_z,
            0.0,
            Some(&self.plan),
        );
        let eye_abs = ground_abs + ELEVATION_M;
        let eye_y = eye_abs - self.anchor_h;
        let mut app = build_first_person_app(
            Self::HEADLESS_SURFACE_ID,
            Self::SHOT_W,
            Self::SHOT_H,
            cam_x,
            cam_z,
            eye_y,
        );

        // Aim at the peak: at yaw Y the forward dir is (-sin Y, -cos Y); facing the
        // peak (the inward direction -dir) gives Y = atan2(dir_x, dir_z). Pitch up
        // toward ~70% of the spire's rise above the (elevated) eye so base and
        // summit both sit in frame.
        let yaw = dir_x.atan2(dir_z);
        let pitch = ((self.plan.peak_height_m - eye_abs) / distance).atan() * 0.55;
        let look = FirstPersonInput::new(
            0,
            Vec3::new(0.0, 0.0, 0.0),
            Angle::radians(yaw),
            Angle::radians(pitch),
        );
        let outcome = app.tick_with_controls(0, &[], &[look]);

        let (vertices, indices) = crate::terrain_mesh::build_snapshot_mesh(
            &self.growth,
            &self.localmap,
            self.seed,
            &self.plan,
            self.anchor_h,
            cam_x,
            cam_z,
        );

        // Light the portrait with a single directional "key" placed over the
        // camera's shoulder so the flank facing the camera is lit on every side.
        // The shader's light vec is the **to-light** direction (surface → source),
        // so it points back toward the camera side `(dir_x, dir_z)` and up; the
        // live viewer's near-field point "torch" can't reach a kilometres-distant
        // spire, so without this a backlit flank reads near-black.
        let key = normalize3([dir_x, 0.85, dir_z]);
        let lights = vec![(0u32, key, [1.0, 0.98, 0.94], 1.3)];

        // Render the shadow depth from the CAMERA's viewpoint (not the sun's): the
        // visible flank is then nearest to the "light" and never self-shadows, so
        // the side facing the camera isn't dropped into the sun's shadow and read
        // near-black. (The scene sun's shadow map would shade every backlit flank.)
        let view_proj = outcome.camera_view_proj();

        CaptureInputs {
            width: Self::SHOT_W,
            height: Self::SHOT_H,
            vertices,
            indices,
            view_proj,
            light_view_proj: view_proj,
            lights,
            clear: outcome.clear_color(),
            material: (2, 2, vec![255u8; 2 * 2 * 4]),
        }
    }

    /// A view from the player's current position (e.g. the summit) **looking at a
    /// world point** `(target_x, target_y_abs, target_z)` — the data-driven
    /// generalization of "look at the ground": the aim is derived from the target,
    /// not hard-wired. Stands where the player is, lifts the eye clear above the
    /// terrain, points yaw + pitch straight at the target, and gathers the render
    /// inputs. A target below the eye (the ground) naturally pitches down; a target
    /// above (a far peak) pitches up.
    pub fn capture_lookat(
        &mut self,
        target_x: f32,
        target_y_abs: f32,
        target_z: f32,
    ) -> CaptureInputs {
        // Lift the eye clear above the terrain: from a near-vertical spire the
        // coarse snapshot mesh's peak spikes would engulf an eye at exactly summit
        // height, so a commanding look needs the camera a few hundred metres up.
        const ABOVE_SUMMIT_M: f32 = 250.0;
        let cam_x = self.state.x;
        let cam_z = self.state.z;
        let ground_abs = self.ground_abs_at(cam_x, cam_z);
        let eye_abs = ground_abs + ABOVE_SUMMIT_M;
        let eye_y = eye_abs - self.anchor_h;
        let mut app = build_first_person_app(
            Self::HEADLESS_SURFACE_ID,
            Self::SHOT_W,
            Self::SHOT_H,
            cam_x,
            cam_z,
            eye_y,
        );

        // Aim at the target: forward is (-sin Y, -cos Y), so pointing along
        // (target - cam) gives Y = atan2(-dx, -dz). Pitch is the rise of the target
        // over the horizontal run to it (negative ⇒ below the eye ⇒ look down).
        let dir_x = target_x - cam_x;
        let dir_z = target_z - cam_z;
        let horizontal = (dir_x * dir_x + dir_z * dir_z).sqrt();
        let yaw = (-dir_x).atan2(-dir_z);
        let pitch = (target_y_abs - eye_abs).atan2(horizontal.max(1.0));
        let look = FirstPersonInput::new(
            0,
            Vec3::new(0.0, 0.0, 0.0),
            Angle::radians(yaw),
            Angle::radians(pitch),
        );
        let outcome = app.tick_with_controls(0, &[], &[look]);

        let (vertices, indices) = crate::terrain_mesh::build_snapshot_mesh(
            &self.growth,
            &self.localmap,
            self.seed,
            &self.plan,
            self.anchor_h,
            cam_x,
            cam_z,
        );

        // The view looks down at terrain whose normals face mostly up, so light it
        // from mostly overhead, tilted toward the look direction so the descending
        // near flank (which faces that way) is lit too rather than a dark wedge.
        // The shader light vec is the to-light direction.
        let look_dir = normalize3([dir_x, 0.0, dir_z]);
        let key = normalize3([look_dir[0] * 0.5, 1.0, look_dir[2] * 0.5]);
        let lights = vec![(0u32, key, [1.0, 0.98, 0.94], 1.3)];
        let view_proj = outcome.camera_view_proj();

        CaptureInputs {
            width: Self::SHOT_W,
            height: Self::SHOT_H,
            vertices,
            indices,
            view_proj,
            light_view_proj: view_proj,
            lights,
            clear: outcome.clear_color(),
            material: (2, 2, vec![255u8; 2 * 2 * 4]),
        }
    }

    /// Legacy convenience: a summit look-down toward the spawn shelf, expressed as
    /// a [`Self::capture_lookat`] at the spawn ground point. Kept for the `summit`
    /// CLI subcommand; the data-driven path resolves its own target tag.
    pub fn capture_summit_lookdown(&mut self) -> CaptureInputs {
        let (spawn_x, spawn_z) = self.plan.spawn_xz;
        let spawn_abs = self.ground_abs_at(spawn_x, spawn_z);
        self.capture_lookat(spawn_x, spawn_abs, spawn_z)
    }
}

/// Normalize a 3-vector (returns the input if it is degenerate).
#[cfg(feature = "agent")]
fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / len, v[1] / len, v[2] / len]
}
