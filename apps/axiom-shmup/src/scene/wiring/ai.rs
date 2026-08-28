//! **The AI seam** — what it takes to actually run [`crate::ai::system::AiCore`]
//! against this scene, and nothing more.
//!
//! [`crate::ai::system`] is a complete port of Claude-of-Duty
//! `src/ai/index.js:1-1107`. It is also, until this file, entirely unreferenced:
//! nothing in the crate constructs an `AiCore`, so the navigation grid is never
//! built, the garrison is never spawned, and the squad/behaviour tiers never
//! run. This module is the composition step that closes that — the same tier as
//! [`crate::scene::game`] and [`crate::scene::app::drive_viewmodel`], and it
//! invents no behaviour of its own.
//!
//! ## What `AiCore` needs that `Game` does not hold
//!
//! `ai/index.js` reaches its collaborators through `ctx`; the port names each
//! one narrowly. Five of them resolve against state that already exists here,
//! and four do not:
//!
//! | seam | resolved from | how |
//! |---|---|---|
//! | rays (`nav::WorldProbe`) | [`crate::physics::probe::PhysicsWorld`] | it already implements the trait |
//! | `createCharacter` ([`AiCharacters`]) | the same `PhysicsWorld` | it already implements that too |
//! | `ctx.peek('world')` ([`WorldInfo`]) | [`crate::scene::level::Level`] | [`world_info`] |
//! | `ctx.peek('sky')` ([`SkyState`]) | [`crate::scene::wiring::look::SkyDriver`] | [`sky_state`] |
//! | `ctx.camera` ([`CameraState`]) | [`CameraPose`] | [`camera_state`] — see below |
//! | `ctx.peek('player')` | `Game::movement.position` (the FEET) | an explicit argument |
//! | `phys.fireBullet` ([`AiBallistics`]) | **nothing yet** | an explicit `Option` argument |
//! | `phys.addRigidBody` ([`GrenadeBodies`]) | **nothing yet** | an explicit `Option` argument |
//! | `ctx.events` (`AiSystem::wire_events`) | **unreachable** | see "What is not wired" |
//!
//! Everything in the "explicit argument" rows is a parameter of [`AiWiring::new`]
//! or [`AiWiring::frame`], never a value this file makes up.
//!
//! ## The camera is computed here because nothing else computes it
//!
//! [`crate::ai::system::AiCore::update_relevance`] culls actors against the real
//! view frustum, and `debug_stage_firefight` places its tableau in camera space;
//! both read `ctx.camera`'s projection and world matrices. This port carries a
//! [`CameraPose`] (eye + Euler + FOV) and no matrices at all, and the engine's
//! own `Camera::perspective` produces an `axiom` matrix in `f32` with its own
//! conventions. So [`camera_state`] transcribes THREE's own
//! `PerspectiveCamera.updateProjectionMatrix` + `Matrix4.makePerspective` and
//! `Quaternion.setFromEuler(…, 'YXZ')`, which is what the source's camera is
//! (`core/engine.js:29-30`: `new PerspectiveCamera(config.fov, 1, 0.05, 1200)`
//! with `rotation.order = 'YXZ'`). Handing the AI an identity camera instead
//! would mark every actor irrelevant on frame one — the LOD sweep would silently
//! switch the whole garrison off.
//!
//! ## Determinism — where the fork must go
//!
//! `AiCore::new` takes an already-forked stream: `ai/index.js:55`'s
//! `this.rng = ctx.rng.fork()`. Which *draw* of the root stream that is depends
//! entirely on where it sits among the other subsystems' forks, and the source
//! fixes that order precisely. `main.js:36` registers in source order but notes
//! "Registration order is irrelevant — Registry topo-sorts on static deps", and
//! `core/registry.js:46-63` is a depth-first topological sort over `static deps`
//! visited in insertion order. For the eleven registered subsystems that
//! resolves to:
//!
//! ```text
//! render, materials, sky, physics, world, player, weapons, fx, AI, ui, audio
//! ```
//!
//! `engine.init()` runs `init(ctx)` in exactly that order, and of those, the ones
//! that fork `ctx.rng` are: render, physics, world, player, weapons, fx, **ai**,
//! ui, audio (materials and sky never touch `ctx.rng`). So the AI's fork is the
//! **seventh** draw off the root, and in particular it comes **after** the
//! world/player/weapons/fx forks and **before** the ui (HUD) fork.
//!
//! In `Game::new` today the only root draws are `build_level`'s (the world slot)
//! and `Hud::new(root.fork())` (the ui slot). So the AI's fork belongs between
//! them, as late as possible: after the level and after any player/weapons/fx
//! fork a sibling slice adds, and immediately before the HUD's. Taking it
//! anywhere else silently reshuffles every value the HUD — and everything
//! forked after it — ever draws.
//!
//! ## What is not wired, and exactly what blocks it
//!
//! * **The event bus.** `AiSystem::wire_events` needs a [`crate::engine::Ctx`],
//!   whose `registry` field is private to `crate::engine`; a `Ctx` cannot be
//!   built outside an `Engine`. `Game` owns an [`crate::events::EventBus`] but
//!   no `Engine`. So the five `on(…)` subscriptions are unreachable and the
//!   handlers ([`AiCore::on_weapon_fire`] and friends) must be called directly.
//!   [`AiWiring::core_mut`] is how. Three of the five could not be complete
//!   anyway — see `AiSystem::wire_events`'s own table.
//! * **Ballistics and grenade bodies.** Passed through as `Option`s. An
//!   implementor exists in principle (`crate::physics::system::PhysicsSystem`
//!   has `fire_bullet` and `add_rigid_body`), but nothing constructs one and it
//!   is a sibling slice's file; adapting an unconstructed facade here would be
//!   inventing the binding, not making it. With both `None`, `AiCore` takes the
//!   source's own `if (!phys)` path: agents aim, move, take cover and reload,
//!   and their shots resolve to no impact.
//! * **Drawing the soldiers.** No longer unwired — that is
//!   [`crate::scene::wiring::soldier_draw`], which reads `core().actors` for
//!   each posed skeleton and submits it through the engine's skinning path. See
//!   [`ActorPose`] for why the pose stream is not what draws them.

use std::rc::Rc;

use crate::ai::nav::WorldProbe;
use crate::ai::system::{
    AiBallistics, AiCharacters, AiCore, AiEffect, AiStats, CameraState, GrenadeBodies, SkyState,
    SpawnPoint as AiSpawnPoint, WorldInfo, DEFAULT_GRAVITY,
};
use crate::config::Config;
use crate::physics::bvh::Aabb;
use crate::physics::probe::PhysicsWorld;
use crate::rng::Rng;
use crate::scene::game::CameraPose;
use crate::scene::level::Level;

use crate::weapons::rig_math::{M4, Q, V3};
use crate::world::system::{BOUNDS_MAX, BOUNDS_MIN, LEVEL_TX, LEVEL_TZ, LEVEL_YAW};

/// `new THREE.PerspectiveCamera(config.fov, 1, 0.05, 1200)` — `core/engine.js:29`.
///
/// Deliberately **not** [`crate::scene::app`]'s `NEAR`/`FAR` (0.05 / 400): those
/// are the engine camera's clip planes, chosen for this port's draw distance,
/// and they are private to that module. The AI's frustum cull must use the
/// numbers the *source's* camera uses, or its far-plane rejection differs from
/// the original's.
pub const CAMERA_NEAR: f64 = 0.05;
/// See [`CAMERA_NEAR`].
pub const CAMERA_FAR: f64 = 1200.0;

/* ================================================================== */
/* What the frame can read back                                       */
/* ================================================================== */

/// Where one soldier is this frame — a flat, cloneable pose read-out.
///
/// ## This is no longer what draws them
///
/// The paragraph that used to sit here said "Axiom has no skinning" and
/// concluded that an animated soldier would need CPU linear-blend skinning and a
/// per-frame mesh re-upload. **That was wrong**, and it was wrong when it was
/// written: `RunningApp::submit_skinned_draw`
/// (`modules/axiom/src/app/authoring.rs:88`) is per-frame immediate-mode
/// skinning against a bake-once mesh authored by `MeshData::new_skinned`, and
/// the whole GPU path behind it — the 20-float skinned vertex stream, the
/// `vs_skinned` stage, the joint-palette texture — already existed. Nothing had
/// to be added to the engine; the claim survived only because nobody re-checked
/// it.
///
/// [`crate::scene::wiring::soldier_draw`] is the real seam: it registers each
/// built variant's material groups as skinned meshes at install and submits one
/// draw per group per visible actor per frame, driving the palette from
/// [`crate::ai::animator::Animator::joint_palette`]. It reads the actors
/// directly (it needs each one's posed [`crate::ai::animator::Skeleton`], which
/// does not fit in a value type), so this pose stream is now what its name says
/// — a summary for anything that wants positions without the bodies.
#[derive(Debug, Clone, PartialEq)]
pub struct ActorPose {
    /// `agent.id` — 1-based, stable for the actor's whole life.
    pub id: i32,
    /// Which `VARIANTS` entry built its body.
    pub variant: String,
    /// The actor group's world position: the source's `group.position`.
    pub position: [f64; 3],
    /// `group.rotation.y`.
    pub yaw: f64,
    /// The variant's uniform scale.
    pub scale: f64,
    pub crouch: bool,
    pub alive: bool,
    /// `_updateRelevance` said neither this actor nor its shadow can be seen.
    pub lod_irrelevant: bool,
    /// `mesh.userData.owNoShadow` (`index.js:855`).
    pub no_shadow: bool,
}

/* ================================================================== */
/* Seam adapters                                                      */
/* ================================================================== */

/// `A.xform` — `trs(LEVEL_TX, 0, LEVEL_TZ, LEVEL_YAW, 1, 1, 1, 0, 0)`.
///
/// The same matrix `crate::world::system`'s private `level_xform` builds, and it
/// has to be rebuilt here rather than borrowed: that function is private, and
/// [`crate::scene::level`] (which is what `Game` actually builds the level with)
/// keeps no transform at all. A yaw-only rotation composes identically under
/// every Euler order, so `from_euler_xyz` is exact.
fn level_xform() -> M4 {
    M4::compose(
        V3::new(LEVEL_TX, 0.0, LEVEL_TZ),
        Q::from_euler_xyz(0.0, LEVEL_YAW, 0.0),
        V3::new(1.0, 1.0, 1.0),
    )
}

/// `world.bounds` — `new Box3((-62,-2,-62), (62,26,62)).applyMatrix4(A.xform)`
/// (`world/index.js:149-152`), which `_buildNav` then expands by 2 to size the
/// navigation grid.
///
/// `Box3.applyMatrix4` transforms all eight corners and re-bounds; transforming
/// only min and max would be wrong for any rotation, and this one is rotated by
/// `LEVEL_YAW`.
#[must_use]
pub fn level_bounds() -> Aabb {
    let m = level_xform();
    // `Box3.js`'s own corner order, kept so the folds see the same sequence.
    let corners = [
        V3::new(BOUNDS_MIN[0], BOUNDS_MIN[1], BOUNDS_MIN[2]),
        V3::new(BOUNDS_MIN[0], BOUNDS_MIN[1], BOUNDS_MAX[2]),
        V3::new(BOUNDS_MIN[0], BOUNDS_MAX[1], BOUNDS_MIN[2]),
        V3::new(BOUNDS_MIN[0], BOUNDS_MAX[1], BOUNDS_MAX[2]),
        V3::new(BOUNDS_MAX[0], BOUNDS_MIN[1], BOUNDS_MIN[2]),
        V3::new(BOUNDS_MAX[0], BOUNDS_MIN[1], BOUNDS_MAX[2]),
        V3::new(BOUNDS_MAX[0], BOUNDS_MAX[1], BOUNDS_MIN[2]),
        V3::new(BOUNDS_MAX[0], BOUNDS_MAX[1], BOUNDS_MAX[2]),
    ];
    corners.iter().fold(
        Aabb {
            minx: f64::INFINITY,
            miny: f64::INFINITY,
            minz: f64::INFINITY,
            maxx: f64::NEG_INFINITY,
            maxy: f64::NEG_INFINITY,
            maxz: f64::NEG_INFINITY,
        },
        |b, c| {
            let p = c.apply_matrix4(m);
            Aabb {
                minx: b.minx.min(p.x),
                miny: b.miny.min(p.y),
                minz: b.minz.min(p.z),
                maxx: b.maxx.max(p.x),
                maxy: b.maxy.max(p.y),
                maxz: b.maxz.max(p.z),
            }
        },
    )
}

/// The three `ctx.peek('world')` reads the AI makes, from the level this scene
/// actually built.
///
/// **`ground_height` is `None`, and that is a known port divergence, not a
/// choice made here.** The source's fallback is `world.groundHeight(x, z)`
/// (`world/index.js:423-426` — the analytic road camber,
/// `crate::world::dressing::occupancy::ground_y` in level space), but
/// [`WorldInfo::ground_height`] is typed `Option<f64>`: a *constant*, not a
/// function of position. There is no value that can stand for a camber, so the
/// honest one is `None`, which is the source's own `?? 0`. It only fires when
/// an 80 m downward ray misses the world entirely — off the 168 m ground plate
/// — so nothing in the garrison reaches it. The fix is in `ai/system.rs`:
/// `WorldInfo::ground_height` should be a `Option<Rc<dyn Fn(f64, f64) -> f64>>`
/// threaded through `ground_at_with`, `GroundQuery` and `index.js:1042`'s call
/// site. Four edits, all in that file.
#[must_use]
pub fn world_info(level: &Level) -> WorldInfo {
    WorldInfo {
        bounds: Some(level_bounds()),
        spawn_points: level
            .spawns
            .iter()
            .map(|s| AiSpawnPoint {
                position: s.position,
                yaw: s.yaw,
            })
            .collect(),
        ground_height: None,
    }
}

/// The two `ctx.peek('sky')` reads: `sunAltitude` (`index.js:553`, the daylight
/// term behind the muzzle-flash gain) and `sunDirection` (`index.js:798`, the
/// axis the relevance sweep casts shadow spheres along).
#[must_use]
pub fn sky_state(sky: &crate::scene::wiring::look::SkyDriver) -> SkyState {
    let d = sky.sun_direction();
    SkyState {
        // `SkySystem` publishes the direction; the altitude is its y component,
        // which is what `sky_look` stored separately.
        sun_altitude: Some(f64::from(d.y).asin()),
        sun_direction: Some([f64::from(d.x), f64::from(d.y), f64::from(d.z)]),
    }
}

/// `Quaternion.setFromEuler(new Euler(x, y, z, 'YXZ'))` — `Quaternion.js`'s
/// `case 'YXZ'` branch, which is the order `core/engine.js:30` puts the camera
/// in. It differs from [`Q::from_euler_xyz`] in exactly two signs (`_z` and
/// `_w`), and getting it wrong banks the AI's view frustum against the player's
/// — the same class of bug `scene::app::combined_yaw_and_pitch_introduce_no_roll`
/// pins for the render camera.
fn quat_from_euler_yxz(x: f64, y: f64, z: f64) -> Q {
    let (c1, c2, c3) = ((x * 0.5).cos(), (y * 0.5).cos(), (z * 0.5).cos());
    let (s1, s2, s3) = ((x * 0.5).sin(), (y * 0.5).sin(), (z * 0.5).sin());
    Q::new(
        s1 * c2 * c3 + c1 * s2 * s3,
        c1 * s2 * c3 - s1 * c2 * s3,
        c1 * c2 * s3 - s1 * s2 * c3,
        c1 * c2 * c3 + s1 * s2 * s3,
    )
}

/// `PerspectiveCamera.updateProjectionMatrix()` feeding
/// `Matrix4.makePerspective(left, right, top, bottom, near, far,
/// WebGLCoordinateSystem)`, with `zoom = 1` and no view offset — the state the
/// source's camera is always in. Column-major, as `Matrix4.elements` is.
fn make_perspective(fov_degrees: f64, aspect: f64, near: f64, far: f64) -> [f64; 16] {
    // `Math.tan(DEG2RAD * 0.5 * this.fov)`, in that association order.
    let top = near * ((std::f64::consts::PI / 180.0) * 0.5 * fov_degrees).tan();
    let height = 2.0 * top;
    let width = aspect * height;
    let left = -0.5 * width;
    let right = left + width;
    let bottom = top - height;

    let x = 2.0 * near / (right - left);
    let y = 2.0 * near / (top - bottom);
    let a = (right + left) / (right - left);
    let b = (top + bottom) / (top - bottom);
    let c = -(far + near) / (far - near);
    let d = -2.0 * far * near / (far - near);
    [
        x, 0.0, 0.0, 0.0, //
        0.0, y, 0.0, 0.0, //
        a, b, c, -1.0, //
        0.0, 0.0, d, 0.0,
    ]
}

/// `ctx.camera`, as `_updateRelevance` and `_stageSlot` read it, built from the
/// pose [`crate::scene::game::Game::frame`] already resolves.
///
/// `aspect` is not something `Game` knows — the canvas owns it — so it is a
/// parameter. `CAMERA_NEAR`/`CAMERA_FAR` are the source camera's own.
#[must_use]
pub fn camera_state(pose: CameraPose, aspect: f64) -> CameraState {
    let q = quat_from_euler_yxz(pose.rotation.pitch, pose.rotation.yaw, pose.rotation.roll);
    // `Object3D.updateMatrixWorld` on a camera parented to the scene root:
    // its world matrix IS `compose(position, quaternion, scale)`.
    let world = M4::compose(
        V3::new(pose.eye[0], pose.eye[1], pose.eye[2]),
        q,
        V3::new(1.0, 1.0, 1.0),
    );
    CameraState {
        position: pose.eye,
        quaternion: [q.x, q.y, q.z, q.w],
        fov: pose.fov_degrees,
        aspect,
        projection_matrix: make_perspective(pose.fov_degrees, aspect, CAMERA_NEAR, CAMERA_FAR),
        matrix_world: world.e,
        matrix_world_inverse: world.invert().e,
    }
}

/* ================================================================== */
/* The seam                                                           */
/* ================================================================== */

/// A booted, steppable AI, and the small amount of translation that takes.
///
/// Construct it once in `Game::new` (see the module doc for *where* in the RNG
/// stream), step it once per rendered frame from [`AiWiring::frame`], and read
/// [`AiWiring::actor_poses`] for what to draw.
pub struct AiWiring {
    core: AiCore,
}

impl AiWiring {
    /// Everything `AiSystem::init` does except `wire_events`, which is
    /// unreachable (see the module doc): set the seams, build the navigation
    /// grid and cover map, garrison the level, and enumerate the character
    /// materials.
    ///
    /// `rng` must be the AI's own fork of the engine root stream, taken at the
    /// point the module doc names. `player_feet` is `ctx.peek('player').position`
    /// — the FEET, which `playerPosition()` lifts by 1.35 to the chest; the
    /// garrison is ranked by distance from it, so it must be the *spawned*
    /// player position and not the origin.
    ///
    /// **This is not cheap.** `_bootNav` walks a 0.8 m grid over the expanded
    /// level bounds (roughly 160 x 160 cells) raycasting each one, then builds
    /// the cover map over it. That cost is the source's too, and it is paid once
    /// at level build.
    #[must_use]
    pub fn new(
        rng: Rng,
        config: &Config,
        level: &Level,
        physics: &PhysicsWorld,
        player_feet: [f64; 3],
    ) -> AiWiring {
        // One handle satisfies both physics seams: `PhysicsWorld` implements
        // `ai::nav::WorldProbe` AND `ai::system::AiCharacters`, which is exactly
        // the source's single `ctx.peek('physics')`. `Rc` because the foot-IK
        // probe handed to every animator outlives any borrow.
        let shared = Rc::new(physics.clone());
        let mut core = AiCore::new(rng, config.q.anisotropy);
        core.deterministic = config.deterministic;
        core.set_physics(
            Some(Rc::clone(&shared) as Rc<dyn WorldProbe>),
            Some(shared as Rc<dyn AiCharacters>),
        );
        core.set_world(world_info(level));
        core.set_player(Some(player_feet));
        // `init`'s tail, in order: `_bootNav(ctx)` then `prewarmMaterials()`.
        // `_bootNav` is what draws from `rng` (every `populate` jitter, every
        // agent fork, every squad fork), so it must run here and not lazily on
        // the first frame.
        core.boot_nav();
        core.prewarm_materials();
        AiWiring { core }
    }

    /// One rendered frame: `update(dt, ctx)` then `lateUpdate()`, the two phases
    /// `AiSystem::phases` declares.
    ///
    /// Call it **after** the player has resolved this frame's pose, which is the
    /// order the source's topological sort gives (`player` inits and updates
    /// before `ai`) — `_updateRelevance` reads `ctx.camera` and the whole
    /// perception tier reads the player's position, both of which the player's
    /// own `update` wrote.
    ///
    /// `ballistics` and `bodies` are `None` until something implements them; see
    /// the module doc for what that costs.
    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        dt: f64,
        frame_index: u64,
        elapsed: f64,
        pose: CameraPose,
        aspect: f64,
        sky: &crate::scene::wiring::look::SkyDriver,
        player_feet: [f64; 3],
        ballistics: Option<&mut dyn AiBallistics>,
        bodies: Option<&mut dyn GrenadeBodies>,
    ) {
        self.core.set_clock(frame_index, elapsed);
        self.core.set_camera(camera_state(pose, aspect));
        self.core.set_sky(sky_state(sky));
        self.core.set_player(Some(player_feet));
        self.core.update(dt, ballistics, bodies, DEFAULT_GRAVITY);
        self.core.late_update();
    }

    /// The whole public API of `ai/index.js`, for a caller that needs more than
    /// the pose stream — the event handlers (`on_weapon_fire`,
    /// `on_damage_dealt`, …), `debug_stage_firefight`, `spawn`, `grenades`,
    /// `shadow_placements`.
    #[must_use]
    pub fn core(&self) -> &AiCore {
        &self.core
    }

    /// Mutable access, for the same reason. This is how the event handlers get
    /// called while `wire_events` remains unreachable.
    pub fn core_mut(&mut self) -> &mut AiCore {
        &mut self.core
    }

    /// `this.stats` — `{ agents, alive, coverPts, walkable, pathsDeferred,
    /// lodIrrelevant }`.
    #[must_use]
    pub fn stats(&self) -> AiStats {
        self.core.stats
    }

    /// Drain the ordered journal of everything the AI emitted this frame — the
    /// six `ctx.events.emit` sites plus `player.onNearMiss`. Nothing consumes
    /// these yet (the bus vocabulary is forked; see `AiSystem::wire_events`),
    /// but they are the observable that proves the tier ran.
    pub fn take_effects(&mut self) -> Vec<AiEffect> {
        self.core.take_effects()
    }

    /// Where every soldier is, this frame. See [`ActorPose`] for what still
    /// stands between this and a drawn body.
    #[must_use]
    pub fn actor_poses(&self) -> Vec<ActorPose> {
        self.core
            .actors
            .iter()
            .map(|a| ActorPose {
                id: a.agent.id,
                variant: a.agent.variant_name.clone(),
                position: a.agent.position,
                yaw: a.agent.yaw,
                scale: a.agent.scale,
                crouch: a.agent.crouch,
                alive: a.agent.alive,
                lod_irrelevant: a.agent.lod_irrelevant,
                no_shadow: a.no_shadow,
            })
            .collect()
    }

    /// The variants this level actually built, in build order, with their
    /// geometry — the read-only half of `AiCore::variant`.
    ///
    /// Use this and never `AiCore::variant` from render code:
    /// [`AiCore::variant`] takes `&mut self` because it *builds* an unseen
    /// variant on demand, and building forks the AI's stream
    /// (`ai/index.js:377`). Asking it for a name the garrison did not spawn
    /// would insert a fork mid-frame and reshuffle every draw after it.
    #[must_use]
    pub fn built_variants(&self) -> &[(String, crate::ai::soldier::SoldierBuild)] {
        self.core.built_variants()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::CAPTURE_SEED;
    use crate::physics::surfaces::mask;
    use crate::scene::level::build_level;
    use crate::config::Quality;

    /// Build the same three things `Game::new` builds, in the same order, so a
    /// test exercises the seam against the real level rather than a fixture.
    fn booted() -> (AiWiring, Level, crate::scene::wiring::look::SkyDriver) {
        let mut root = Rng::new(CAPTURE_SEED);
        let level = build_level(&mut root);
        let physics = PhysicsWorld::new(level.world.clone());
        let spawn = level.spawn(0);
        let feet_y = physics
            .ground_height(spawn.position[0], spawn.position[2], spawn.position[1] + 6.0)
            .map_or(spawn.position[1] + 0.2, |gy| gy + 0.03);
        let feet = [spawn.position[0], feet_y, spawn.position[2]];
        let ai = AiWiring::new(
            root.fork(),
            &Config::default(),
            &level,
            &physics,
            feet,
        );
        (
            ai,
            level,
            crate::scene::wiring::look::SkyDriver::new(
                Quality::High,
                crate::scene::wiring::look::HOUR,
            ),
        )
    }

    fn pose(level: &Level) -> CameraPose {
        let spawn = level.spawn(0);
        CameraPose {
            eye: [spawn.position[0], spawn.position[1] + 1.66, spawn.position[2]],
            rotation: crate::player::camera::Euler {
                pitch: 0.0,
                yaw: spawn.yaw,
                roll: 0.0,
            },
            fov_degrees: 80.0,
        }
    }

    #[test]
    fn the_level_bounds_are_the_sources_box_carried_through_the_level_transform() {
        let b = level_bounds();
        // A 124 x 28 x 124 box rotated about Y by LEVEL_YAW re-bounds wider in
        // X and Z and unchanged in Y.
        assert!((b.maxy - BOUNDS_MAX[1] - 0.0).abs() < 1e-9, "Y is untouched by a yaw");
        assert!((b.miny - BOUNDS_MIN[1]).abs() < 1e-9);
        assert!(b.maxx - b.minx > 124.0, "a yawed box re-bounds wider in X");
        assert!(b.maxz - b.minz > 124.0, "and in Z");
        // And it is offset by the level translation.
        assert!(((b.minx + b.maxx) * 0.5 - LEVEL_TX).abs() < 1e-6);
        assert!(((b.minz + b.maxz) * 0.5 - LEVEL_TZ).abs() < 1e-6);
    }

    #[test]
    fn the_camera_state_is_a_real_frustum_that_contains_what_is_in_front_of_it() {
        let cam = camera_state(
            CameraPose {
                eye: [0.0, 2.0, 0.0],
                rotation: crate::player::camera::Euler {
                    pitch: 0.0,
                    yaw: 0.0,
                    roll: 0.0,
                },
                fov_degrees: 80.0,
            },
            16.0 / 9.0,
        );
        // Yaw 0 looks down -Z (Three's convention), so the world matrix is the
        // identity rotation at the eye.
        assert!((cam.matrix_world[12] - 0.0).abs() < 1e-12);
        assert!((cam.matrix_world[13] - 2.0).abs() < 1e-12);
        // The inverse really is one.
        let m = M4 { e: cam.matrix_world };
        let back = m.invert();
        let p = V3::new(1.0, 3.0, -5.0)
            .apply_matrix4(m)
            .apply_matrix4(M4 { e: back.e });
        assert!((p.x - 1.0).abs() < 1e-9 && (p.y - 3.0).abs() < 1e-9 && (p.z + 5.0).abs() < 1e-9);

        // The frustum built from it accepts a point ten metres ahead and
        // rejects one ten metres behind.
        let mvp = crate::ai::animator::Mat4::multiply_matrices(
            &crate::ai::animator::Mat4 {
                e: cam.projection_matrix,
            },
            &crate::ai::animator::Mat4 {
                e: cam.matrix_world_inverse,
            },
        );
        let f = crate::ai::system::Frustum::from_projection_matrix(&mvp.e);
        assert!(f.intersects_sphere([0.0, 2.0, -10.0], 1.0), "ahead is visible");
        assert!(!f.intersects_sphere([0.0, 2.0, 10.0], 1.0), "behind is not");
    }

    #[test]
    fn booting_garrisons_the_level_and_stepping_drives_the_soldiers() {
        let (mut ai, level, sky) = booted();

        // `_bootNav` built navigation and `populate(2, 3)` filled it.
        assert!(ai.stats().walkable > 0, "the nav grid found no floor");
        assert!(ai.stats().cover_pts > 0, "the cover map found no cover");
        let start = ai.actor_poses();
        assert!(
            !start.is_empty() && start.len() <= 6,
            "populate(2, 3) garrisoned {} soldiers",
            start.len()
        );
        assert!(
            !ai.built_variants().is_empty(),
            "soldiers spawned but no variant body was built"
        );
        // `stats.agents` is written at the END of `update`, so it is the
        // cleanest proof that the frame loop — not just the boot — ran.
        assert_eq!(ai.stats().agents, 0, "nothing has stepped yet");

        // Every soldier is somewhere real, not at the origin and not NaN.
        for a in &start {
            assert!(a.alive);
            assert!(
                a.position.iter().all(|c| c.is_finite()),
                "actor {} is at {:?}",
                a.id,
                a.position
            );
            assert!(
                a.position[0].hypot(a.position[2]) > 1.0,
                "actor {} spawned on top of the origin",
                a.id
            );
            // The physics seam is live: there is ground under it.
            let physics = PhysicsWorld::new(level.world.clone());
            assert!(
                crate::ai::nav::WorldProbe::raycast(
                    &physics,
                    [a.position[0], a.position[1] + 4.0, a.position[2]],
                    [0.0, -1.0, 0.0],
                    24.0,
                    mask::WORLD,
                )
                .is_some(),
                "actor {} is over a hole",
                a.id
            );
        }

        // Five seconds: long enough for an Idle agent with a patrol route to
        // reach `state_time > 2.5`, take a destination and walk toward it
        // (`agent.js:1008-1022`).
        let p = pose(&level);
        let mut elapsed = 0.0;
        for frame in 1..=300u64 {
            elapsed += 1.0 / 60.0;
            ai.frame(1.0 / 60.0, frame, elapsed, p, 16.0 / 9.0, &sky, p.eye, None, None);
        }

        assert_eq!(ai.stats().agents, start.len(), "`update` ran");
        assert_eq!(ai.stats().alive, start.len(), "nothing killed them");
        // `lateUpdate` ran too: every actor contributed a contact shadow.
        let (body, _feet) = ai.core().shadow_placements();
        assert_eq!(body.len(), start.len(), "`late_update` posed every actor");

        let moved = ai
            .actor_poses()
            .iter()
            .zip(&start)
            .any(|(now, was)| now.position != was.position || now.yaw != was.yaw);
        assert!(moved, "five seconds of AI and not one soldier stirred");
    }

    #[test]
    fn the_ai_is_deterministic_for_the_same_seed() {
        let run = || {
            let (mut ai, level, sky) = booted();
            let p = pose(&level);
            let mut elapsed = 0.0;
            for frame in 1..=60u64 {
                elapsed += 1.0 / 60.0;
                ai.frame(1.0 / 60.0, frame, elapsed, p, 16.0 / 9.0, &sky, p.eye, None, None);
            }
            (ai.actor_poses(), ai.stats())
        };
        let (a_poses, a_stats) = run();
        let (b_poses, b_stats) = run();
        assert_eq!(a_poses, b_poses);
        assert_eq!(a_stats, b_stats);
    }
}
