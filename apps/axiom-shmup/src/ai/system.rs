//! Ported from Claude-of-Duty `src/ai/index.js:1-1107` — the whole file.
//!
//! `AiSystem` is the AI slice's orchestration tier: it boots navigation,
//! prewarms the character materials, garrisons the level, wires the frame-wide
//! events, rations A* to a per-frame budget, decides per-actor LOD relevance,
//! and stages the capture tableaus.
//!
//! ```text
//! PUBLIC API   const ai = ctx.get('ai')
//!   ai.spawn(variant, position, yaw, opts) -> Agent
//!   ai.agents                              live Agent list
//!   ai.debugStage('firefight')             staged combat tableau for captures
//!   ai.prewarmMaterials()                  build every character material
//!   ai.grid / ai.cover                     navigation + cover queries
//!   ai.stats                               { agents, alive, coverPts,
//!                                            pathsDeferred, lodIrrelevant }
//! ```
//!
//! Everything the source composes is ported: [`super::nav`], [`super::agent`],
//! [`super::squad`], [`super::grounding`], [`super::soldier`],
//! [`super::textures`], [`super::animator`], [`super::rig`]. This module is the
//! only place they meet.
//!
//! ## The five seams
//!
//! `ai/index.js` reaches five collaborators through `ctx`, and none of them is
//! a thing this crate owns. Each is named as narrowly as the source uses it —
//! the precedent set by [`super::agent::AgentAnimator`],
//! [`super::grounding::FootSource`] and `crate::audio::spatial::WorldProbe`.
//!
//! 1. **Rays** — `phys.raycast` / `phys.raycastAny`, already named by
//!    [`super::nav::WorldProbe`] and implemented by
//!    [`crate::physics::probe::PhysicsWorld`]. Held as an `Rc` because the
//!    animator's foot-IK probe (`ai/index.js:433`, handed to every `Agent` at
//!    `agent.js:138`) outlives any borrow this module could hand it.
//! 2. **Ballistics** — `phys.fireBullet` (`index.js:607-614`), named by
//!    [`AiBallistics`]. The source reads exactly one thing off the result:
//!    `impacts[0].point`.
//! 3. **Thrown bodies** — `phys.addRigidBody` / `removeRigidBody` and the
//!    body's live `position` (`index.js:685-716`), named by [`GrenadeBodies`].
//! 4. **The camera** — `ctx.camera`, arriving through
//!    [`AiCore::set_camera`] as the nine-plus-sixteen numbers the source reads
//!    off it. Same choice, and the same reason, as
//!    `crate::ui::system::UiCore::set_camera`.
//! 5. **`sky`, `player` and `world`** — three `ctx.peek` reads of *values*, not
//!    behaviour: the sun, the player's feet, and the level's bounds and spawn
//!    points. They arrive through [`AiCore::set_sky`],
//!    [`AiCore::set_player`] and [`AiCore::set_world`].
//!
//! ## What is NOT here, and why
//!
//! * **The scene graph.** `THREE.Group`, `SkinnedMesh`, `IcosahedronGeometry`
//!   and the `ai.root` subtree are render bookkeeping. The one number the
//!   source's scene graph feeds back into behaviour is `mesh.matrixWorld`, and
//!   that is recomputed here exactly as Three composes it (see
//!   [`actor_matrix_world`]).
//! * **`prewarmMaterials`'s second half.** `renderer.compileAsync`,
//!   `r.patcher.patch` and the throwaway `SkinnedMesh` need a live WebGL
//!   context. [`AiCore::prewarm_materials`] ports the deterministic half — the
//!   enumeration and de-duplication of every material every variant will ever
//!   ask for, which is the part that must run in `MATERIAL_SLOTS` order — and
//!   reports it as [`PrewarmReport`], whose `ok` is `false` for exactly the
//!   reason the source's is: no renderer.
//! * **`console.info` / `console.warn`.** Diagnostics; the numbers they print
//!   are all fields of [`AiStats`] or of the variant build.
//! * **`stats.navMs`.** Wall-clock, and therefore not a value a deterministic
//!   port may carry.
//!
//! ## Determinism
//!
//! The fork order is the contract and it is preserved exactly:
//!
//! | source | here |
//! |---|---|
//! | `ai/index.js:55` `this.rng = ctx.rng.fork()` | [`AiCore::new`]'s argument |
//! | `ai/index.js:61` `new SoldierMaterials(this.rng.fork(), …)` | [`AiCore::new`] |
//! | `ai/index.js:377` `buildSoldier(name, { rng: this.rng.fork() })` | [`AiCore::variant`] |
//! | `agent.js:97` `this.rng = ai.rng.fork()` | [`AiCore::spawn`], **before** `variant()` |
//! | `agent.js:136` the animator's `this.rng.fork()` | [`Agent::new`]'s second return value |
//! | `ai/index.js:541` `new Squad(this.rng.fork())` | [`AiCore::create_squad`] |
//!
//! `spawn`'s order is the subtle one: JavaScript evaluates
//! `new Agent(this, …)`'s body top-down, so the agent's own fork
//! (`agent.js:97`) is taken **before** `ai.variant(name)` (`agent.js:99`) can
//! fork for a variant this level has not built yet. Swapping those two reorders
//! every draw in the garrison.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use axiom_kernel::Seconds;

use crate::engine::Ctx;
use crate::events::SubscriptionId;
use crate::jsmath;
use crate::physics::bvh::Aabb;
use crate::physics::surfaces::mask;
use crate::registry::{Phase, Subsystem};
use crate::rng::Rng;
use crate::weapons::rig_math::{Q, V3};

use super::agent::{
    next_agent_id, Agent, AgentAnimator, AgentController, AgentCtx, AgentEvent, AgentState,
    AnimatorState, BodyPart, Clip, CoverSource, GroundHeight, HitRegion, HitboxSegment, Neighbor,
    PathSource, SquadPermissions,
};
use super::animator::{
    apply_matrix4, Animator, GroundProbe, Mat4, ProbeOut, StateUpdate, WeaponAnchors,
};
use super::clips::{ClipId, HitRegion as ClipHitRegion};
use super::geo::BoundingSphere;
use super::grounding::{FootSource, GroundShadows, Placement};
use super::nav::{
    line_of_sight, CoverBuildOpts, CoverMap, NavGrid, NavGridOpts, SquadMemberPos, WorldProbe,
};
use super::rig::RIG;
use super::soldier::{build_soldier, resolve_materials, MaterialRequest, SoldierBuild, MATERIAL_SLOTS, VARIANTS};
use super::squad::{MemberSnapshot, Squad};
use super::textures::{SoldierMaterials, SoldierOpts};

/// A world position. The source's `THREE.Vector3`, which is `f64` per
/// component — see the `ui/system.rs` note on storage width.
pub type Vec3 = [f64; 3];

/* ================================================================== */
/* Seams                                                              */
/* ================================================================== */

/// `phys.fireBullet({ … })` (`index.js:607-614`), narrowed to what
/// `onAgentFire` asks of it.
pub trait AiBallistics {
    /// Returns the impact list. The source reads only `impacts[0].point`, but
    /// the whole list is what `fireBullet` hands back and a caller wiring the
    /// real physics facade has no reason to throw the rest away.
    fn fire_bullet(&mut self, request: BulletRequest) -> Vec<BulletImpact>;
}

/// `fireBullet`'s argument object. `index.js:607-614`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BulletRequest {
    pub origin: Vec3,
    pub dir: Vec3,
    pub damage: f64,
    pub penetration: f64,
    pub max_dist: f64,
    pub mask: u16,
}

/// One entry of `fireBullet`'s result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BulletImpact {
    pub point: Vec3,
}

/// A live thrown grenade body. The source keeps the `RigidBody` object
/// (`index.js:685-697`) and reads `body.position` back every frame
/// (`index.js:706`); this port keeps the handle the implementor gave it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BodyId(pub u32);

/// `phys.addRigidBody`'s argument object for a grenade. `index.js:685-696`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GrenadeBody {
    pub radius: f64,
    pub mass: f64,
    pub position: Vec3,
    pub velocity: Vec3,
    pub restitution: f64,
    pub friction: f64,
    pub lifetime: f64,
}

/// `phys.addRigidBody` / `phys.removeRigidBody`, plus the one read-back
/// (`index.js:706`, `g.body?.position ?? g.mesh.position`).
pub trait GrenadeBodies {
    fn add(&mut self, body: GrenadeBody) -> Option<BodyId>;
    /// `None` is the source's `g.body?.position` being absent, which falls
    /// back to the mesh position — i.e. where it was thrown from.
    fn position(&self, id: BodyId) -> Option<Vec3>;
    fn remove(&mut self, id: BodyId);
}

/// `phys.createCharacter({ radius, height, position, stepHeight, slopeLimit })`
/// (`agent.js:146-153`), the one physics call the *agent constructor* makes
/// that is not a ray. Named here rather than in `agent.rs` because the agent
/// never creates its own controller — `ai/index.js` owns the spawn.
pub trait AiCharacters {
    fn create_character(
        &self,
        radius: f64,
        height: f64,
        position: Vec3,
    ) -> Box<dyn AgentController>;
}

/// `crate::physics::character::Character` is the ported swept controller;
/// this is the six members `_move`/`_drive` touch.
impl AgentController for crate::physics::character::Character {
    fn position(&self) -> Vec3 {
        self.position
    }
    fn grounded(&self) -> bool {
        self.grounded
    }
    fn last_move_blocked(&self) -> bool {
        self.last_move_blocked
    }
    fn set_height(&mut self, h: f64) {
        // `c.setHeight?.(h)` — one argument, so `force` is `undefined`.
        crate::physics::character::Character::set_height(self, h, false);
    }
    fn move_by(&mut self, dx: f64, dy: f64, dz: f64) {
        crate::physics::character::Character::move_by(self, dx, dy, dz);
    }
    fn teleport_to(&mut self, x: f64, y: f64, z: f64) {
        crate::physics::character::Character::teleport(self, x, y, z);
    }
}

impl AiCharacters for crate::physics::probe::PhysicsWorld {
    fn create_character(
        &self,
        radius: f64,
        height: f64,
        position: Vec3,
    ) -> Box<dyn AgentController> {
        let mut c = crate::physics::character::Character::new(
            self.world(),
            crate::physics::character::CharacterOpts {
                radius,
                height,
                step_height: 0.42,
                // NOTE the 48. `CharacterController` reads `slopeLimit` in
                // RADIANS (`character.js:40`, whose own default is
                // `50 * PI/180`), so `ai/index.js`'s `slopeLimit: 48` asks for
                // 48 radians and gets `cos(48) = -0.748` — every surface
                // counts as ground. Ported as written; see the notes file.
                slope_limit: 48.0,
                ..crate::physics::character::CharacterOpts::default()
            },
        );
        // `opts.position` is applied at the END of the source's constructor
        // (`character.js:72`), after every other field.
        c.set_position(position[0], position[1], position[2]);
        Box::new(c)
    }
}

/// `ctx.camera`, as the source reads it. `matrix_world` /
/// `matrix_world_inverse` / `projection_matrix` are column-major, the order
/// `THREE.Matrix4.elements` uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraState {
    pub position: Vec3,
    /// `cam.quaternion`, `[x, y, z, w]`.
    pub quaternion: [f64; 4],
    /// Degrees, as `PerspectiveCamera.fov` is.
    pub fov: f64,
    pub aspect: f64,
    pub projection_matrix: [f64; 16],
    pub matrix_world: [f64; 16],
    pub matrix_world_inverse: [f64; 16],
}

impl Default for CameraState {
    fn default() -> Self {
        const IDENTITY: [f64; 16] = [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ];
        CameraState {
            position: [0.0, 0.0, 0.0],
            quaternion: [0.0, 0.0, 0.0, 1.0],
            fov: 70.0,
            aspect: 1.0,
            projection_matrix: IDENTITY,
            matrix_world: IDENTITY,
            matrix_world_inverse: IDENTITY,
        }
    }
}

/// The two `ctx.peek('sky')` reads: `sunAltitude` (`index.js:553`) and
/// `sunDirection` (`index.js:798`). `None` is the subsystem being absent,
/// which the source spells `sky?.sunAltitude ?? 0.6`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SkyState {
    pub sun_altitude: Option<f64>,
    pub sun_direction: Option<Vec3>,
}

/// One `world.spawnPoints` entry. `world/index.js:144-148`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnPoint {
    pub position: Vec3,
    pub yaw: f64,
}

/// The three `ctx.peek('world')` reads: `bounds` (`index.js:413`),
/// `spawnPoints` (`index.js:485`) and `groundHeight` (`index.js:451`).
#[derive(Debug, Clone, Default)]
pub struct WorldInfo {
    /// `None` is the source's `?? new Box3(-70,-4,-70 .. 70,24,70)` fallback.
    pub bounds: Option<Aabb>,
    pub spawn_points: Vec<SpawnPoint>,
    /// A flat fallback height for `groundAt` when no ray hits. `None` is the
    /// source's `?? 0`.
    pub ground_height: Option<f64>,
}

/* ================================================================== */
/* Event arguments                                                    */
/* ================================================================== */

// THE EVENT-PAYLOAD VOCABULARY IS FORKED ACROSS THIS CRATE, and this module
// deliberately does not widen the fork. `crate::audio::system`,
// `crate::ui::system` and `crate::player::system` each declare their own
// listener-side type for `weapon:fire`, `bullet:impact`, `damage:dealt`,
// `explosion` and `player:footstep`; `EventBus` dispatches on `TypeId`, so only
// one of them ever sees a given emit. Unifying them is a whole-game decision
// and belongs in the integration pass.
//
// So the types below are NOT a fourth set of bus payloads. They are the
// arguments of `AiCore`'s handler methods — the shape `ai/index.js`'s handler
// bodies actually read — and `AiSystem::wire_events` adapts the EXISTING
// payload types into them (see that function for what each existing type can
// and cannot supply). Every emit this module makes goes into the
// [`AiEffect`] journal, which carries the source's full payload; the bus emit
// alongside it uses whichever existing type fits.

/// `on('weapon:fire')`'s payload as the handler reads it. `index.js:296-308`.
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponFireHeard {
    /// `e.weapon` — `Some("ai_rifle")` is the source's "ignore our own".
    pub weapon: Option<String>,
    /// `if (!e || !e.origin) return`.
    pub origin: Vec3,
    /// `if (e.dir)` — the suppression arm only runs when a direction is given.
    pub dir: Option<Vec3>,
}

/// `on('bullet:impact')`'s payload. `index.js:310-318`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BulletImpactHeard {
    pub point: Vec3,
}

/// `on('damage:dealt')`'s payload. `index.js:320-327`.
///
/// The source's guard is `e.target instanceof Agent` — an object-identity test
/// with no counterpart across an `Any` payload. The port names the agent by
/// id, which is the identity the rest of this module already keys on, and
/// treats "no such id" exactly as the source treats a non-`Agent` target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DamageDealtToAgent {
    pub target_agent: i32,
    pub amount: f64,
    pub headshot: bool,
    /// `e.part ?? 'torso'`.
    pub part: Option<BodyPart>,
    /// `e.point ?? a.position`.
    pub point: Option<Vec3>,
    pub incident: Option<Vec3>,
}

/// `on('explosion')`'s payload. `index.js:329-343`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExplosionHeard {
    pub position: Vec3,
    /// `e.radius ?? 5`.
    pub radius: Option<f64>,
    /// `e.damage ?? 100`.
    pub damage: Option<f64>,
}

/// `on('player:footstep')`'s payload. `index.js:345-349`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerFootstepHeard {
    pub position: Vec3,
    pub running: bool,
}

/* ================================================================== */
/* The effect journal                                                 */
/* ================================================================== */

/// One outward call the facade made, in the order it made it — the six
/// `ctx.events.emit` sites plus `player.onNearMiss`.
///
/// A facade *is* what it calls and when, so this is the port's primary
/// observable, exactly as `crate::ui::system::UiEffect` is for the HUD. It
/// also carries payload fields no existing bus type has a home for
/// (`weapon:fire`'s flash gains and seed, `weapon:shell`'s velocity), so
/// nothing the source emits is silently dropped while the vocabulary is
/// forked.
#[derive(Debug, Clone, PartialEq)]
pub enum AiEffect {
    /// `weapon:fire` — the enemy muzzle. `index.js:588-596`.
    WeaponFire {
        /// Always `"ai_rifle"`; the AI's own `weapon:fire` handler keys on it.
        weapon: &'static str,
        origin: Vec3,
        dir: Vec3,
        seed: u32,
        intensity: f64,
        light: f64,
        flash_scale: f64,
    },
    /// `weapon:shell`. `index.js:599-602`.
    WeaponShell { position: Vec3, velocity: Vec3 },
    /// `bullet:tracer`. `index.js:625`.
    BulletTracer { from: Vec3, to: Vec3, speed: f64 },
    /// `damage:dealt` — the enemy hitting the player. `index.js:646-654`.
    DamageDealt {
        amount: f64,
        headshot: bool,
        killed: bool,
        point: Vec3,
        from: Vec3,
        source_agent: i32,
    },
    /// `player.onNearMiss(miss)`. `index.js:638`.
    NearMiss { miss: f64 },
    /// `weapon:reload`. `index.js:658`.
    WeaponReload { weapon: &'static str, actor: i32 },
    /// `explosion` — a grenade cooking off. `index.js:707-712`.
    Explosion {
        position: Vec3,
        radius: f64,
        damage: f64,
        source_agent: i32,
    },
    /// `actor:death`, raised by [`Agent::die`] (`agent.js:876-881`).
    ActorDeath {
        actor: i32,
        point: Vec3,
        impulse: Vec3,
        headshot: bool,
    },
}

/* ================================================================== */
/* Three's frustum, sphere and matrix, as `_updateRelevance` uses them */
/* ================================================================== */

/// `THREE.Plane`, as `Frustum` stores it: a unit normal and a constant.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Plane {
    normal: Vec3,
    constant: f64,
}

impl Plane {
    /// `setComponents(x, y, z, w).normalize()`. The normalise is
    /// `1 / normal.length()` and a multiply, not three divides.
    fn from_components(x: f64, y: f64, z: f64, w: f64) -> Plane {
        let inv = 1.0 / (x * x + y * y + z * z).sqrt();
        Plane { normal: [x * inv, y * inv, z * inv], constant: w * inv }
    }

    /// `distanceToPoint(point)` — `normal.dot(point) + constant`.
    fn distance_to_point(&self, p: Vec3) -> f64 {
        self.normal[0] * p[0] + self.normal[1] * p[1] + self.normal[2] * p[2] + self.constant
    }
}

/// `THREE.Frustum`. Six planes in Three's own order: right, left, bottom, top,
/// far, near.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Frustum {
    planes: [Plane; 6],
}

impl Frustum {
    /// `setFromProjectionMatrix(m, WebGLCoordinateSystem)`.
    #[must_use]
    pub fn from_projection_matrix(me: &[f64; 16]) -> Frustum {
        let (me0, me1, me2, me3) = (me[0], me[1], me[2], me[3]);
        let (me4, me5, me6, me7) = (me[4], me[5], me[6], me[7]);
        let (me8, me9, me10, me11) = (me[8], me[9], me[10], me[11]);
        let (me12, me13, me14, me15) = (me[12], me[13], me[14], me[15]);
        Frustum {
            planes: [
                Plane::from_components(me3 - me0, me7 - me4, me11 - me8, me15 - me12),
                Plane::from_components(me3 + me0, me7 + me4, me11 + me8, me15 + me12),
                Plane::from_components(me3 + me1, me7 + me5, me11 + me9, me15 + me13),
                Plane::from_components(me3 - me1, me7 - me5, me11 - me9, me15 - me13),
                Plane::from_components(me3 - me2, me7 - me6, me11 - me10, me15 - me14),
                Plane::from_components(me3 + me2, me7 + me6, me11 + me10, me15 + me14),
            ],
        }
    }

    /// `intersectsSphere(sphere)`.
    #[must_use]
    pub fn intersects_sphere(&self, center: Vec3, radius: f64) -> bool {
        let neg_radius = -radius;
        self.planes.iter().all(|p| p.distance_to_point(center) >= neg_radius)
    }
}

/// `Matrix4.getMaxScaleOnAxis()`.
fn max_scale_on_axis(te: &[f64; 16]) -> f64 {
    let scale_x_sq = te[0] * te[0] + te[1] * te[1] + te[2] * te[2];
    let scale_y_sq = te[4] * te[4] + te[5] * te[5] + te[6] * te[6];
    let scale_z_sq = te[8] * te[8] + te[9] * te[9] + te[10] * te[10];
    scale_x_sq.max(scale_y_sq).max(scale_z_sq).sqrt()
}

/// `Sphere.copy(bs).applyMatrix4(m)`. `index.js:840`.
fn sphere_apply_matrix4(bs: BoundingSphere, m: &Mat4) -> (Vec3, f64) {
    let c = apply_matrix4(V3::new(bs.center[0], bs.center[1], bs.center[2]), m);
    ([c.x, c.y, c.z], bs.radius * max_scale_on_axis(&m.e))
}

/// The actor's `mesh.matrixWorld` (`agent.js:114-133`): a `THREE.Group` at
/// `position`, rotated `yaw` about Y, uniformly scaled, holding the mesh at
/// the identity. `ai.root` is never transformed, so the group's world matrix
/// **is** its local matrix, and multiplying the mesh's identity into it is
/// exact.
///
/// `group.rotation.y = yaw` goes through `Euler`'s default `'XYZ'` order —
/// the same Euler-order site [`Animator::set_actor`] documents.
#[must_use]
pub fn actor_matrix_world(position: Vec3, yaw: f64, scale: f64) -> Mat4 {
    Mat4::compose(
        V3::new(position[0], position[1], position[2]),
        Q::from_euler_xyz(0.0, yaw, 0.0),
        V3::new(scale, scale, scale),
    )
}

/* ================================================================== */
/* Actors                                                             */
/* ================================================================== */

/// `a.staged` — the tableau pin `debugStage` hangs on an agent
/// (`index.js:1016-1025`, `index.js:1074-1080`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Staged {
    pub crouch: bool,
    pub speed: f64,
    pub fire: bool,
    pub no_damage: bool,
    pub heading: Vec3,
    pub aim_weight: f64,
    pub reload_every: f64,
    pub suppression: f64,
}

/// One live enemy: the [`Agent`], the [`Animator`] the source builds inside
/// its constructor, and the per-actor bookkeeping `ai/index.js` keeps on the
/// side (`a.staged`, `mesh.userData.owNoShadow`, the squad it belongs to).
pub struct Actor {
    pub agent: Agent,
    pub animator: Animator,
    /// `phys.createCharacter(...)` (`agent.js:146-153`). `None` with no
    /// physics, and cleared by [`Agent::die`].
    pub controller: Option<Box<dyn AgentController>>,
    /// Index into [`AiCore::squads`]; `squad.add(a)` in `populate`.
    pub squad: Option<usize>,
    pub staged: Option<Staged>,
    /// `mesh.userData.owNoShadow` (`index.js:855`) — render honours it per
    /// frame.
    pub no_shadow: bool,
    /// `a.mesh.geometry.boundingSphere`, copied from the variant build.
    pub bounding_sphere: BoundingSphere,
    /// The last `syncHitboxes()` result (`index.js:768`). The source pushes
    /// these straight into physics colliders; there is no collider registry
    /// here, so they are published instead.
    pub hitboxes: Vec<HitboxSegment>,
    /// `animator.ejectWorld` as the tick opened — see [`AiCore::on_agent_fire`].
    eject_at_tick_start: Vec3,
    /// `Agent`'s own `_pendingDest` (`agent.js:238`), observed at the seam.
    ///
    /// [`Agent`] keeps that field private, and `agent.js:281-284`'s retry
    /// (`if (this.pathPending) this._goTo(this._pendingDest)`) needs it. The
    /// destination is the argument [`PathSource::request_path`] was refused
    /// with, so [`BudgetedPath`] records it here on exactly the deferral that
    /// sets `path_pending` — the same value, taken from the same call.
    pending_dest: Vec3,
}

/// `this.stats`. `index.js:79`, `:114`, `:858`.
///
/// `navMs` is deliberately absent: it is `performance.now()` arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AiStats {
    pub agents: usize,
    pub alive: usize,
    pub cover_pts: usize,
    pub walkable: usize,
    pub paths_deferred: usize,
    pub lod_irrelevant: usize,
}

/// `prewarmMaterials()`'s result object. `index.js:202`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PrewarmReport {
    /// The source sets this only after `compileAsync` succeeded, which needs a
    /// renderer. There is none here, so it is always `false` — the same value
    /// the source produces on `if (!renderer) return out`.
    pub ok: bool,
    /// `mats.length + 1` — the deduped material list plus the grenade's.
    pub materials: usize,
    /// `renderer.info.programs.length` delta; always 0 without a renderer.
    pub programs: usize,
}

/// A live thrown grenade. `index.js:697`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grenade {
    pub body: Option<BodyId>,
    /// Where it was thrown from — the source's `g.mesh.position` fallback.
    pub thrown_from: Vec3,
    pub fuse: f64,
    pub agent: i32,
}

/* ================================================================== */
/* Ground probe / ground height                                       */
/* ================================================================== */

/// `probeGround` handed to every animator as `opts.probe`
/// (`agent.js:138` -> `ai/index.js:433`).
struct AiGroundProbe {
    phys: Option<Rc<dyn WorldProbe>>,
}

impl GroundProbe for AiGroundProbe {
    fn probe(&self, x: f64, z: f64, from_y: f64, out: &mut ProbeOut) -> bool {
        let Some(phys) = self.phys.as_deref() else { return false };
        probe_ground_with(phys, x, z, from_y, out)
    }
}

/// `probeGround(x, z, fromY, out)`. `index.js:433-444`.
fn probe_ground_with(phys: &dyn WorldProbe, x: f64, z: f64, from_y: f64, out: &mut ProbeOut) -> bool {
    let Some(h) = phys.raycast([x, from_y, z], [0.0, -1.0, 0.0], 3.2, mask::WORLD) else {
        return false;
    };
    out.y = h.point[1];
    out.nx = h.normal[0];
    out.ny = h.normal[1];
    out.nz = h.normal[2];
    out.hit = true;
    true
}

/// `groundAt(x, z, fromY = 40)`. `index.js:446-452`.
fn ground_at_with(phys: Option<&dyn WorldProbe>, fallback: Option<f64>, x: f64, z: f64, from_y: f64) -> f64 {
    let Some(phys) = phys else { return 0.0 };
    match phys.raycast([x, from_y, z], [0.0, -1.0, 0.0], 80.0, mask::WORLD) {
        Some(h) => h.point[1],
        None => fallback.unwrap_or(0.0),
    }
}

/// [`GroundHeight`] for the frame loop — `this.ai.groundAt` as `agent.js:721`
/// calls it.
struct GroundQuery<'a> {
    phys: Option<&'a dyn WorldProbe>,
    fallback: Option<f64>,
}

impl GroundHeight for GroundQuery<'_> {
    fn ground_at(&self, x: f64, z: f64, from_y: f64) -> f64 {
        ground_at_with(self.phys, self.fallback, x, z, from_y)
    }
}

/// The `dyn`-safe reborrow [`NavGrid::build`] and [`CoverMap::build`] need:
/// both take `&impl WorldProbe`, and a bare `&dyn WorldProbe` is not `Sized`.
struct ProbeRef<'a>(&'a dyn WorldProbe);

impl WorldProbe for ProbeRef<'_> {
    fn raycast(
        &self,
        origin: Vec3,
        dir: Vec3,
        max_dist: f64,
        m: u16,
    ) -> Option<super::nav::RayHit> {
        self.0.raycast(origin, dir, max_dist, m)
    }
    fn raycast_any(&self, origin: Vec3, dir: Vec3, max_dist: f64, m: u16) -> bool {
        self.0.raycast_any(origin, dir, max_dist, m)
    }
}

/// `requestPath` as the agent sees it: the shared grid behind this frame's
/// budget. `index.js:785-793`.
struct BudgetedPath<'a> {
    grid: Option<&'a mut NavGrid>,
    budget: &'a mut i32,
    deferred: &'a mut usize,
    /// Where the last refused request wanted to go — see [`Actor::pending_dest`].
    pending_dest: &'a mut Vec3,
}

impl PathSource for BudgetedPath<'_> {
    fn request_path(&mut self, from: Vec3, to: Vec3) -> Option<Vec<Vec3>> {
        // `if (!this.grid) return 0` — no grid is "no route", not "deferred".
        let Some(grid) = self.grid.as_deref_mut() else { return Some(Vec::new()) };
        if *self.budget <= 0 {
            *self.deferred += 1;
            *self.pending_dest = to;
            return None;
        }
        *self.budget -= 1;
        Some(grid.find_path(from, to, super::nav::FindPathOpts::default()))
    }
}

/// A borrowed [`AgentController`], re-wrapped so it can enter
/// [`AgentCtx`].
///
/// [`Actor`] owns its controller as `Box<dyn AgentController>`, whose *object*
/// lifetime is `'static`; `AgentCtx<'a>::controller` is
/// `&'a mut (dyn AgentController + 'a)`. Shortening a trait object's lifetime
/// behind a `&mut` is invariant and therefore forbidden — but *unsizing a
/// sized type* is not, and picks the short lifetime freely. So the box is
/// reborrowed into this sized wrapper and the wrapper is unsized instead. The
/// same trick is why `cover.as_mut().map(|c| c as &mut dyn CoverSource)` a few
/// lines below needs no wrapper: `CoverMap` is already sized.
struct CtrlRef<'r>(&'r mut (dyn AgentController + 'static));

impl AgentController for CtrlRef<'_> {
    fn position(&self) -> Vec3 {
        self.0.position()
    }
    fn grounded(&self) -> bool {
        self.0.grounded()
    }
    fn last_move_blocked(&self) -> bool {
        self.0.last_move_blocked()
    }
    fn set_height(&mut self, h: f64) {
        self.0.set_height(h);
    }
    fn move_by(&mut self, dx: f64, dy: f64, dz: f64) {
        self.0.move_by(dx, dy, dz);
    }
    fn teleport_to(&mut self, x: f64, y: f64, z: f64) {
        self.0.teleport_to(x, y, z);
    }
}

/// One of `agent.js:281-286`'s five phases, named so the AI tier can run them
/// individually and interleave `ai/index.js`'s own work between them —
/// `onAgentFire` reads `animator.ejectWorld` before `_drive` re-poses the
/// skeleton, and `throwGrenade` calls `animator.fire(0.35)` from inside
/// `_think`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase5 {
    /// `if (this.pathPending) this._goTo(this._pendingDest)`.
    PendingPath(Vec3),
    Think,
    Move,
    Shoot,
    Drive,
}

/// The squad seat, rebuilt per agent because [`SquadPermissions`] needs both
/// the squad and the frame's member snapshots.
struct Seat<'a> {
    squad: &'a mut Squad,
    members: &'a [MemberSnapshot],
}

impl SquadPermissions for Seat<'_> {
    fn request_peek(&mut self, agent_id: i32, _dt: f64) -> bool {
        self.squad.request_peek(agent_id)
    }
    fn can_flank(&mut self, agent_id: i32) -> bool {
        self.squad.can_flank(agent_id, self.members)
    }
    fn claim_flank(&mut self, agent_id: i32) {
        self.squad.claim_flank(agent_id);
    }
    fn request_grenade(&mut self) -> bool {
        self.squad.request_grenade()
    }
}

/* ================================================================== */
/* The animator seam                                                  */
/* ================================================================== */

/// The eleven members `agent.js` touches on its animator, wired to the real
/// [`Animator`].
///
/// This impl lives here rather than in `animator.rs` because this is the seam
/// — the module that owns both halves — and because `animator.rs` has no
/// reason to know [`AgentAnimator`] exists. Its opposite number,
/// `impl FootSource for Animator`, is already in `animator.rs`
/// (`animator.rs:1376`) for the symmetric reason: `bonePos` is the animator's
/// own API and grounding only names it.
impl AgentAnimator for Animator {
    fn reloading(&self) -> bool {
        Animator::reloading(self)
    }
    fn vaulting(&self) -> bool {
        Animator::vaulting(self)
    }
    fn muzzle_world(&self) -> Vec3 {
        [self.muzzle_world.x, self.muzzle_world.y, self.muzzle_world.z]
    }
    fn muzzle_dir(&self) -> Vec3 {
        [self.muzzle_dir.x, self.muzzle_dir.y, self.muzzle_dir.z]
    }
    fn bone_pos(&self, bone: &str) -> Vec3 {
        let p = Animator::bone_pos(self, bone);
        [p.x, p.y, p.z]
    }
    fn fire(&mut self, strength: f64) {
        Animator::fire(self, strength);
    }
    fn hit(&mut self, region: HitRegion, side: f64, strength: f64) {
        Animator::hit(self, clip_region(region), side, strength);
    }
    fn reload(&mut self, duration: f64) {
        Animator::reload(self, duration);
    }
    fn vault(&mut self, duration: f64) {
        Animator::vault(self, duration);
    }
    fn turn(&mut self, dir: f64) {
        Animator::turn(self, dir);
    }
    fn set_state(&mut self, s: AnimatorState) {
        Animator::set_state(
            self,
            StateUpdate {
                clip: Some(clip_id(s.clip)),
                speed: Some(s.speed),
                crouch: Some(s.crouch),
                aim_target: Some(Some(V3::new(s.aim_target[0], s.aim_target[1], s.aim_target[2]))),
                look_target: Some(Some(V3::new(
                    s.look_target[0],
                    s.look_target[1],
                    s.look_target[2],
                ))),
                aim_weight: Some(s.aim_weight),
                suppress: Some(s.suppress),
                hurt: None,
            },
        );
    }
    fn update(&mut self, dt: f64, elapsed: f64) {
        Animator::update(self, dt, elapsed);
    }
    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

/// `agent.js`'s clip name -> the animator's clip id. Both enums are in source
/// order and the names match one for one.
fn clip_id(c: Clip) -> ClipId {
    match c {
        Clip::Idle => ClipId::Idle,
        Clip::Walk => ClipId::Walk,
        Clip::Run => ClipId::Run,
        Clip::CrouchWalk => ClipId::CrouchWalk,
        Clip::CrouchIdle => ClipId::CrouchIdle,
        Clip::HurtIdle => ClipId::HurtIdle,
    }
}

/// `agent.js`'s region name -> `clips.js`'s. `agent.js` cannot produce the
/// `Other` arm (its `applyDamage` only ever names the six).
fn clip_region(r: HitRegion) -> ClipHitRegion {
    match r {
        HitRegion::Head => ClipHitRegion::Head,
        HitRegion::Torso => ClipHitRegion::Torso,
        HitRegion::ArmR => ClipHitRegion::ArmR,
        HitRegion::ArmL => ClipHitRegion::ArmL,
        HitRegion::LegR => ClipHitRegion::LegR,
        HitRegion::LegL => ClipHitRegion::LegL,
    }
}

/* ================================================================== */
/* The core                                                           */
/* ================================================================== */

/// `class AiSystem`'s mutable guts — JavaScript's `this`, made explicit.
pub struct AiCore {
    /// `this.rng` (`index.js:55`), already forked from `ctx.rng`.
    pub rng: Rng,
    pub materials: SoldierMaterials,
    pub ground: GroundShadows,
    /// `this._variants`, in insertion order — the order the source's `Map`
    /// iterates, which `dispose` walks.
    variants: Vec<(String, SoldierBuild)>,
    pub actors: Vec<Actor>,
    pub squads: Vec<Squad>,
    pub grid: Option<NavGrid>,
    pub cover: Option<CoverMap>,
    pub inspect: bool,
    /// `this.forcePopulate` — dev: garrison even in a deterministic capture.
    pub force_populate: bool,
    nav_pending: bool,
    pub stats: AiStats,
    /// `this._pathBudget`.
    path_budget: i32,
    /// `this.pathsPerFrame` (`index.js:113`).
    pub paths_per_frame: i32,
    /// `this._sun` — kept between frames exactly as the source's scratch is.
    sun: Vec3,
    /// `_nextId` (`agent.js:90`), an explicit counter rather than a static.
    next_agent_id: i32,
    next_squad_id: u32,
    prewarmed: Option<PrewarmReport>,
    grenades: Vec<Grenade>,

    /* ---- the seams ---- */
    phys: Option<Rc<dyn WorldProbe>>,
    characters: Option<Rc<dyn AiCharacters>>,
    camera: CameraState,
    sky: SkyState,
    world: WorldInfo,
    /// `ctx.peek('player')?.position` — the player's FEET; `playerPosition`
    /// lifts by 1.35.
    player: Option<Vec3>,
    /// `ctx.time.frame`, read by the muzzle-flash seed (`index.js:595`).
    pub frame: u64,
    /// `ctx.time.elapsed`.
    pub elapsed: f64,
    /// `ctx.config.deterministic` (`index.js:164`, `:729`).
    pub deterministic: bool,

    /// Every outward call, in order.
    effects: Vec<AiEffect>,
}

impl AiCore {
    /// `init(ctx)`. `index.js:53-155`, minus the scene graph and the
    /// `console.info` diagnostics.
    ///
    /// `rng` is `ctx.rng.fork()` (`index.js:55`) — already forked, the same
    /// contract [`Agent::new`] and [`Squad::new`] take.
    #[must_use]
    pub fn new(mut rng: Rng, anisotropy: u32) -> Self {
        // `new SoldierMaterials(this.rng.fork(), { size: 512, anisotropy:
        // ctx.config.q.anisotropy ?? 8, camo: ['arid','woodland','urban'] })`
        let materials = SoldierMaterials::new(
            &mut rng.fork(),
            &SoldierOpts {
                size: 512,
                anisotropy,
                camo: vec!["arid".to_string(), "woodland".to_string(), "urban".to_string()],
            },
        );
        AiCore {
            rng,
            materials,
            // Contact occlusion under every actor. Without it the cast shadow
            // alone leaves them hovering: see grounding.js.
            ground: GroundShadows::new(16),
            variants: Vec::new(),
            actors: Vec::new(),
            squads: Vec::new(),
            grid: None,
            cover: None,
            inspect: false,
            force_populate: false,
            nav_pending: true,
            stats: AiStats::default(),
            path_budget: 0,
            paths_per_frame: 2,
            sun: [0.0, 1.0, 0.0],
            // `_nextId` starts at 1 (`agent.js:90`) and `this.id = _nextId++`
            // hands out 1, 2, 3, ...
            next_agent_id: 1,
            next_squad_id: 0,
            prewarmed: None,
            grenades: Vec::new(),
            phys: None,
            characters: None,
            camera: CameraState::default(),
            sky: SkyState::default(),
            world: WorldInfo::default(),
            player: None,
            frame: 0,
            elapsed: 0.0,
            deterministic: false,
            effects: Vec::new(),
        }
    }

    /* ---------------- seam setters ---------------- */

    /// `this.phys` (`index.js:399-401`) — one object in the source, two
    /// narrowly-named halves here: the rays (also handed to every animator as
    /// its foot-IK probe) and `createCharacter`. Both arrive together because
    /// the source's `ctx.peek('physics')` is one presence test: with no
    /// physics an agent gets neither a controller nor a hitbox
    /// (`agent.js:142-173`).
    pub fn set_physics(
        &mut self,
        phys: Option<Rc<dyn WorldProbe>>,
        characters: Option<Rc<dyn AiCharacters>>,
    ) {
        self.phys = phys;
        self.characters = characters;
    }

    pub fn set_camera(&mut self, camera: CameraState) {
        self.camera = camera;
    }

    pub fn set_sky(&mut self, sky: SkyState) {
        self.sky = sky;
    }

    /// The player's **feet**, the source's `p.position ?? p.capsulePosition`.
    pub fn set_player(&mut self, player: Option<Vec3>) {
        self.player = player;
    }

    pub fn set_world(&mut self, world: WorldInfo) {
        self.world = world;
    }

    /// `ctx.time`, read by the flash seed and handed to the animators.
    pub fn set_clock(&mut self, frame: u64, elapsed: f64) {
        self.frame = frame;
        self.elapsed = elapsed;
    }

    /// The ordered effect journal, and a way to drain it.
    #[must_use]
    pub fn effects(&self) -> &[AiEffect] {
        &self.effects
    }

    pub fn take_effects(&mut self) -> Vec<AiEffect> {
        std::mem::take(&mut self.effects)
    }

    /* ================================================================== */
    /* boot                                                               */
    /* ================================================================== */

    /// `_bootNav(ctx)`. `index.js:161-169`.
    ///
    /// The source's `try`/`catch` has nothing to catch here: `_buildNav`
    /// returns early rather than throwing on every condition the source's
    /// `catch` could see (no physics, no triangles), and `populate` returns 0.
    /// The `_navPending = true` that the catch sets is therefore reached the
    /// same way — by `_buildNav` leaving it set.
    pub fn boot_nav(&mut self) {
        self.build_nav();
        if !self.nav_pending && (!self.deterministic || self.force_populate) {
            self.populate(2, 3);
        }
    }

    /// `prewarmMaterials()`. `index.js:199-265` — the deterministic half.
    ///
    /// `resolveMaterials()` is a pure function of the variant name, so every
    /// material every variant will ever ask for can be created now. It draws
    /// no random numbers, so the RNG stream — and therefore the picture — is
    /// untouched. It MUST be handed `MATERIAL_SLOTS` in the builder's own
    /// order: three sorts opaque draws by the global `Material.id` counter, so
    /// creating them in any other order reorders those draws and flips the
    /// depth tie on coplanar surfaces.
    ///
    /// Idempotent, exactly as the source's `if (this._prewarmed) return` is.
    pub fn prewarm_materials(&mut self) -> PrewarmReport {
        if let Some(done) = self.prewarmed {
            return done;
        }
        let slots: Vec<String> = MATERIAL_SLOTS.iter().map(|s| (*s).to_string()).collect();
        let mut mats: Vec<MaterialRequest> = Vec::new();
        for (name, _) in VARIANTS {
            for m in resolve_materials(name, &slots) {
                // `if (m && !seen.has(m))` — the source dedups on the material
                // OBJECT, which `SoldierMaterials.get` caches by its option
                // key; two requests with the same key are the same object.
                // `MaterialRequest` is that key as a value, so equality here
                // is the same relation.
                if !mats.contains(&m) {
                    mats.push(m);
                }
            }
        }
        // the thrown grenade's mesh is built on the first throw, mid-firefight
        let out = PrewarmReport {
            ok: false,
            materials: mats.len() + 1,
            programs: 0,
        };
        self.prewarmed = Some(out);
        out
    }

    /* ================================================================== */
    /* events                                                             */
    /* ================================================================== */

    /// `on('weapon:fire')`. `index.js:296-308`.
    pub fn on_weapon_fire(&mut self, e: &WeaponFireHeard) {
        if e.weapon.as_deref() == Some("ai_rifle") {
            return; // ignore our own
        }
        // A gunshot is the loudest thing in the level: everybody hears it, and
        // anyone near the line of fire also feels suppressed by it.
        for a in &mut self.actors {
            if !a.agent.alive {
                continue;
            }
            a.agent.hear(e.origin, 90.0);
            if let Some(dir) = e.dir {
                let d = distance_to_ray(a.agent.position, e.origin, dir, a.agent.eye_height);
                if d < 2.6 {
                    a.agent.suppress(0.45 * (1.0 - d / 2.6) + 0.12);
                }
            }
        }
    }

    /// `on('bullet:impact')`. `index.js:310-318`.
    pub fn on_bullet_impact(&mut self, e: &BulletImpactHeard) {
        for a in &mut self.actors {
            if !a.agent.alive {
                continue;
            }
            let d = distance(a.agent.position, e.point);
            if d < 3.2 {
                a.agent.suppress(0.5 * (1.0 - d / 3.2));
            } else if d < 12.0 {
                a.agent.hear(e.point, 12.0);
            }
        }
    }

    /// `on('damage:dealt')`. `index.js:320-327`. Returns `e.killed` — the
    /// source writes it back onto the payload object, which a `&dyn Any`
    /// payload cannot carry, so it comes back as a value.
    pub fn on_damage_dealt(&mut self, e: &DamageDealtToAgent) -> bool {
        let Some(i) = self.actors.iter().position(|a| a.agent.id == e.target_agent) else {
            return false; // `!(e.target instanceof Agent)`
        };
        if !self.actors[i].agent.alive {
            return false;
        }
        // `e.amount * this._falloff(e.point)` — `_falloff(undefined)` returns
        // 1, so an absent point means NO falloff. It must not be defaulted to
        // the agent's position here; that default belongs to `applyDamage`'s
        // `point` argument on the next line, and only there.
        let amount = e.amount * self.falloff(e.point);
        let part = if e.headshot { BodyPart::Head } else { e.part.unwrap_or(BodyPart::Torso) };
        let point = e.point.unwrap_or(self.actors[i].agent.position);
        let mut events = Vec::new();
        {
            let AiCore { actors, cover, .. } = self;
            let actor = &mut actors[i];
            actor.agent.apply_damage(
                amount,
                part,
                point,
                e.incident,
                &mut actor.animator,
                cover.as_mut().map(|c| c as &mut dyn CoverSource),
                &mut events,
            );
        }
        self.drain_agent_events(i, &events);
        !self.actors[i].agent.alive
    }

    /// `on('explosion')`. `index.js:329-343`.
    pub fn on_explosion(&mut self, e: &ExplosionHeard) {
        let radius = e.radius.unwrap_or(5.0);
        let damage = e.damage.unwrap_or(100.0);
        let phys = self.phys.clone();
        for i in 0..self.actors.len() {
            if !self.actors[i].agent.alive {
                continue;
            }
            let pos = self.actors[i].agent.position;
            let eye = self.actors[i].agent.eye();
            let d = distance(pos, e.position) + 0.001;
            self.actors[i].agent.hear(e.position, 120.0);
            if d > radius {
                continue;
            }
            if let Some(p) = phys.as_deref() {
                if !line_of_sight(p, e.position, eye, mask::EXPLOSION) {
                    continue;
                }
            }
            let f = 1.0 - d / radius;
            // `this._v.copy(a.position).sub(e.position).normalize()`
            let mut v = [pos[0] - e.position[0], pos[1] - e.position[1], pos[2] - e.position[2]];
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            let inv = 1.0 / jsmath::or_one(l);
            v = [v[0] * inv, v[1] * inv, v[2] * inv];
            self.actors[i].agent.suppress(1.4 * f);
            let mut events = Vec::new();
            {
                let AiCore { actors, cover, .. } = self;
                let actor = &mut actors[i];
                actor.agent.apply_damage(
                    damage * f * f,
                    BodyPart::Torso,
                    eye,
                    Some(v),
                    &mut actor.animator,
                    cover.as_mut().map(|c| c as &mut dyn CoverSource),
                    &mut events,
                );
            }
            self.drain_agent_events(i, &events);
        }
    }

    /// `on('player:footstep')`. `index.js:345-349`.
    pub fn on_player_footstep(&mut self, e: &PlayerFootstepHeard) {
        let loud = if e.running { 24.0 } else { 11.0 };
        for a in &mut self.actors {
            if a.agent.alive {
                a.agent.hear(e.position, loud);
            }
        }
    }

    /// `_falloff(point)`. `index.js:352-359`.
    ///
    /// The source's second guard (`if (!p) return 1`) is dead:
    /// `playerPosition` always returns its `out`.
    #[must_use]
    pub fn falloff(&self, point: Option<Vec3>) -> f64 {
        let Some(point) = point else { return 1.0 };
        let p = self.player_position();
        let d = distance(p, point);
        // full damage inside 22 m, tapering to 45 % by 70 m
        if d < 22.0 {
            1.0
        } else {
            f64::max(0.45, 1.0 - (d - 22.0) * 0.0125)
        }
    }

    /* ================================================================== */
    /* assets                                                             */
    /* ================================================================== */

    /// `variant(name)`. `index.js:373-392`. Builds on first ask and caches.
    pub fn variant(&mut self, name: &str) -> &SoldierBuild {
        let found = self.variants.iter().position(|(k, _)| k == name);
        let i = match found {
            Some(i) => i,
            None => {
                let mut fork = self.rng.fork();
                let built = build_soldier(name, &mut fork);
                self.variants.push((name.to_string(), built));
                self.variants.len() - 1
            }
        };
        &self.variants[i].1
    }

    /// `this._variants`, in build order — the read-only half of
    /// [`AiCore::variant`].
    ///
    /// It exists because the *mutable* half cannot safely be called from render
    /// code. [`AiCore::variant`] takes `&mut self` because it builds a variant
    /// on first ask, and building forks [`AiCore::rng`] (`index.js:377`). A
    /// drawer asking for a name this level never garrisoned would therefore
    /// insert a fork into the middle of the frame stream and reshuffle every
    /// value drawn after it — the exact hazard the "fork once, in order"
    /// discipline exists to prevent. Anything that only wants to *read* a built
    /// body's geometry or materials asks here.
    #[must_use]
    pub fn built_variants(&self) -> &[(String, SoldierBuild)] {
        &self.variants
    }

    /// `rigIndex(name)`. `index.js:395-397`.
    #[must_use]
    pub fn rig_index(&self, name: &str) -> usize {
        RIG.index(name)
    }

    /* ================================================================== */
    /* navigation                                                         */
    /* ================================================================== */

    /// `_buildNav()`. `index.js:407-430`.
    pub fn build_nav(&mut self) {
        let Some(phys) = self.phys.clone() else { return };
        // `phys.staticWorld.dirty` / `rebuildStatic()` / `triangleCount <= 0`
        // are all the physics facade keeping its own BVH current; the probe
        // handed here is already built, so there is nothing to rebuild and
        // nothing to wait for. A caller with no level yet passes no probe,
        // which is the same "retry next frame" state.
        let bounds = self.world.bounds.unwrap_or(Aabb {
            minx: -70.0,
            miny: -4.0,
            minz: -70.0,
            maxx: 70.0,
            maxy: 24.0,
            maxz: 70.0,
        });
        // `bounds.expandByScalar(2)`
        let bounds = Aabb {
            minx: bounds.minx - 2.0,
            miny: bounds.miny - 2.0,
            minz: bounds.minz - 2.0,
            maxx: bounds.maxx + 2.0,
            maxy: bounds.maxy + 2.0,
            maxz: bounds.maxz + 2.0,
        };
        let probe = ProbeRef(phys.as_ref());
        let mut grid = NavGrid::new(
            bounds,
            NavGridOpts { cell: 0.8, radius: 0.36, height: 1.78, ..NavGridOpts::default() },
        );
        grid.build(&probe);
        let mut cover = CoverMap::new();
        cover.build(&grid, &probe, CoverBuildOpts { step: 1, reach: 1.3 });
        self.stats.cover_pts = cover.points.len();
        self.stats.walkable = grid.walkable_count;
        self.grid = Some(grid);
        self.cover = Some(cover);
        self.nav_pending = false;
    }

    /// `probeGround(x, z, fromY, out)`. `index.js:433-444`.
    pub fn probe_ground(&self, x: f64, z: f64, from_y: f64, out: &mut ProbeOut) -> bool {
        match self.phys.as_deref() {
            Some(p) => probe_ground_with(p, x, z, from_y, out),
            None => false,
        }
    }

    /// `groundAt(x, z, fromY = 40)`. `index.js:446-452`.
    #[must_use]
    pub fn ground_at(&self, x: f64, z: f64, from_y: f64) -> f64 {
        ground_at_with(self.phys.as_deref(), self.world.ground_height, x, z, from_y)
    }

    /// `playerPosition(out)`. `index.js:455-465` — the player's CHEST.
    #[must_use]
    pub fn player_position(&self) -> Vec3 {
        match self.player.filter(|p| p[0].is_finite()) {
            Some(p) => [p[0], p[1] + 1.35, p[2]],
            None => {
                // `out.setFromMatrixPosition(this.ctx.camera.matrixWorld); out.y -= 0.1`
                let m = &self.camera.matrix_world;
                [m[12], m[13] - 0.1, m[14]]
            }
        }
    }

    /* ================================================================== */
    /* spawning                                                           */
    /* ================================================================== */

    /// `spawn(variantName, position, yaw, opts)`. `index.js:471-475`.
    ///
    /// Returns the index into [`AiCore::actors`] — the source returns the
    /// `Agent` itself, which the callers use only to `squad.add(a)` and to
    /// write `a.staged`.
    pub fn spawn(
        &mut self,
        variant_name: &str,
        position: Vec3,
        yaw: f64,
        patrol: Option<Vec<Vec3>>,
    ) -> usize {
        // `agent.js:96-99`, in order: the id, the agent's own fork, then the
        // variant (which forks again if this is the level's first one).
        let id = next_agent_id(&mut self.next_agent_id);
        let agent_rng = self.rng.fork();
        let build = self.variant(variant_name);
        let scale = build.variant.scale;
        let weapon = WeaponAnchors {
            muzzle: build.weapon.muzzle,
            foregrip: build.weapon.foregrip,
            mag_bottom: build.weapon.mag_bottom,
            ejection: build.weapon.ejection,
        };
        let bounding_sphere = build.geometry.bounding_sphere;

        let has_physics = self.phys.is_some();
        let (mut a, animator_rng) = Agent::new(
            id,
            agent_rng,
            variant_name,
            scale,
            RIG.eye_height,
            position,
            yaw,
            has_physics,
        );
        a.patrol_points = patrol;

        let mut animator = Animator::new(
            &RIG,
            Some(weapon),
            Some(animator_rng),
            Some(Box::new(AiGroundProbe { phys: self.phys.clone() })),
            scale,
        );
        // `agent.js:126-133`:
        //     this.group.position.copy(this.position);
        //     this.group.rotation.y = this.yaw;
        //     this.group.updateMatrixWorld(true);
        // "The bones' world matrices are derived from the group's, so the
        // group has to be current before anything reads them — including the
        // very first animator pass and a same-frame ragdoll hand-off."
        // `Agent` has no group (it is scene-graph state), so the actor node
        // that stands in for it is written here, by the tier that owns the
        // scene. See `AiCore::place_actor_for_drive` for the per-frame half.
        animator.set_actor(V3::new(position[0], position[1], position[2]), yaw);

        // `this.controller = phys ? phys.createCharacter({...}) : null`
        let controller: Option<Box<dyn AgentController>> = self
            .characters
            .as_ref()
            .map(|c| c.create_character(a.radius, a.height, position));

        self.actors.push(Actor {
            agent: a,
            animator,
            controller,
            squad: None,
            staged: None,
            no_shadow: false,
            bounding_sphere,
            hitboxes: Vec::new(),
            eject_at_tick_start: [0.0, 0.0, 0.0],
            pending_dest: [0.0, 0.0, 0.0],
        });
        self.actors.len() - 1
    }

    /// `populate(opts)`. `index.js:483-538`.
    ///
    /// Garrison the level: two squads on patrol routes drawn from the world's
    /// own spawn points, far enough from the player to be found rather than
    /// spawned on top of.
    pub fn populate(&mut self, squads: usize, per: usize) -> usize {
        if self.world.spawn_points.is_empty() || self.grid.is_none() {
            return 0;
        }
        let player = self.player_position();
        // rank the spawn points by distance from the player, take the far half
        let mut ranked: Vec<(usize, SpawnPoint, f64)> = self
            .world
            .spawn_points
            .iter()
            .enumerate()
            .map(|(i, s)| (i, *s, distance(s.position, player)))
            .collect();
        // `.sort((a, b) => b.d - a.d)` — descending, and stable in both
        // languages, so ties keep spawn-point order.
        ranked.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        ranked.retain(|e| e.2 > 18.0);
        if ranked.is_empty() {
            return 0;
        }

        const VARIANT_NAMES: [&str; 3] = ["vanguard", "irregular", "breacher"];
        let mut made = 0usize;
        let mut q = 0usize;
        while q < squads && q < ranked.len() {
            let squad = self.create_squad();
            let anchor = ranked[q % ranked.len()].1;
            // patrol route: this spawn point and the two next-nearest ones
            let mut route = vec![anchor.position];
            let mut others: Vec<&(usize, SpawnPoint, f64)> = ranked
                .iter()
                .filter(|e| e.0 != ranked[q % ranked.len()].0)
                .collect();
            others.sort_by(|a, b| {
                let da = distance(a.1.position, anchor.position);
                let db = distance(b.1.position, anchor.position);
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            });
            for o in others.iter().take(2) {
                route.push(o.1.position);
            }

            for m in 0..per {
                let jitter_a = self.rng.range(0.0, std::f64::consts::PI * 2.0);
                let jitter_r = self.rng.range(0.8, 3.2);
                let mut p = [
                    anchor.position[0] + jitter_a.cos() * jitter_r,
                    anchor.position[1],
                    anchor.position[2] + jitter_a.sin() * jitter_r,
                ];
                let ci = self
                    .grid
                    .as_ref()
                    .expect("grid checked above")
                    .nearest(p[0], p[2], Some(anchor.position[1]), 6, 1.4);
                if ci >= 0 {
                    let grid = self.grid.as_ref().expect("grid checked above");
                    let nx = grid.nx as i64;
                    p = [
                        grid.world_x(ci % nx),
                        f64::from(grid.floor()[ci as usize]),
                        grid.world_z(ci / nx),
                    ];
                } else {
                    p[1] = self.ground_at(p[0], p[2], anchor.position[1] + 4.0);
                }
                // The yaw argument is evaluated BEFORE `spawn` is called, so
                // this `signed()` draw precedes the agent's own fork.
                let yaw = anchor.yaw + self.rng.signed() * 0.7;
                let name = VARIANT_NAMES[(q * per + m) % VARIANT_NAMES.len()];
                let idx = self.spawn(name, p, yaw, Some(route.clone()));
                let agent_id = self.actors[idx].agent.id;
                self.actors[idx].squad = Some(squad);
                self.squads[squad].add(agent_id);
                made += 1;
            }
            q += 1;
        }
        made
    }

    /// `createSquad()`. `index.js:540-544`. Returns the squad's index.
    pub fn create_squad(&mut self) -> usize {
        let id = self.next_squad_id;
        self.next_squad_id += 1;
        let s = Squad::new(id, self.rng.fork());
        self.squads.push(s);
        self.squads.len() - 1
    }

    /* ================================================================== */
    /* firing                                                             */
    /* ================================================================== */

    /// `_daylight()`. `index.js:551-555`. 0 at night, 1 in full daylight.
    #[must_use]
    pub fn daylight(&self) -> f64 {
        let alt = self.sky.sun_altitude.unwrap_or(0.6); // radians above the horizon
        f64::min(1.0, f64::max(0.0, f64::max(0.0, alt).sin() * 4.0))
    }

    /// `_flashGain()`. `index.js:562-564` — the SPRITE gain.
    #[must_use]
    pub fn flash_gain(&self) -> f64 {
        0.12 + 0.5 * (1.0 - self.daylight())
    }

    /// `_flashLight()`. `index.js:579-582` — the LIGHT gain, deliberately
    /// separate and two orders of magnitude smaller.
    #[must_use]
    pub fn flash_light(&self) -> f64 {
        let day = self.daylight();
        0.006 + 0.05 * (1.0 - day)
    }

    /// `_sunDirection()`. `index.js:796-803`.
    pub fn sun_direction(&mut self) -> Vec3 {
        match self.sky.sun_direction.filter(|d| d[0].is_finite()) {
            Some(d) => self.sun = d,
            None => self.sun = [0.3, 0.8, 0.4],
        }
        let len_sq = self.sun[0] * self.sun[0] + self.sun[1] * self.sun[1] + self.sun[2] * self.sun[2];
        if len_sq < 1e-8 {
            self.sun = [0.0, 1.0, 0.0];
        }
        let l = (self.sun[0] * self.sun[0] + self.sun[1] * self.sun[1] + self.sun[2] * self.sun[2]).sqrt();
        let inv = 1.0 / jsmath::or_one(l);
        self.sun = [self.sun[0] * inv, self.sun[1] * inv, self.sun[2] * inv];
        self.sun
    }

    /// `onAgentFire(agent, origin, dir)`. `index.js:584-626`.
    fn on_agent_fire(
        &mut self,
        actor_index: usize,
        origin: Vec3,
        dir: Vec3,
        ballistics: Option<&mut (dyn AiBallistics + '_)>,
    ) {
        let agent_id = self.actors[actor_index].agent.id;
        let weapon_damage = self.actors[actor_index].agent.weapon_damage;
        let ammo = self.actors[actor_index].agent.ammo;
        let staged_no_damage = self.actors[actor_index].staged.is_some_and(|s| s.no_damage);
        // `se.position.copy(agent.animator.ejectWorld)` — read at the moment
        // `_fireRound` runs, which is inside `_shoot` and therefore BEFORE
        // `_drive` re-poses the skeleton. `eject_world` is only ever written
        // by `Animator::update`, so the value here is the one the tick opened
        // with; `step_actor` snapshots it for exactly that reason.
        let eject = self.actors[actor_index].eject_at_tick_start;

        // muzzle flash, light and smoke come from fx via the canonical event
        // `(agent.id * 2654435761 + ctx.time.frame) >>> 0`
        let seed = (agent_id as u32)
            .wrapping_mul(2_654_435_761)
            .wrapping_add(self.frame as u32);
        self.effects.push(AiEffect::WeaponFire {
            weapon: "ai_rifle",
            origin,
            dir,
            seed,
            intensity: self.flash_gain(),
            light: self.flash_light(),
            flash_scale: 0.8,
        });

        // ejected case:
        // `se.velocity.set(dir.z, 0.55, -dir.x).multiplyScalar(2.1).addScaledVector(dir, -0.6)`
        let velocity = [
            dir[2] * 2.1 + dir[0] * -0.6,
            0.55 * 2.1 + dir[1] * -0.6,
            -dir[0] * 2.1 + dir[2] * -0.6,
        ];
        self.effects.push(AiEffect::WeaponShell { position: eject, velocity });

        // the round itself
        let mut end: Option<Vec3> = None;
        if let Some(b) = ballistics {
            let impacts = b.fire_bullet(BulletRequest {
                origin,
                dir,
                damage: weapon_damage,
                penetration: 0.9,
                max_dist: 200.0,
                mask: mask::BULLET,
            });
            if let Some(first) = impacts.first() {
                end = Some(first.point);
            }
        }
        // physics has no player collider, so test the player capsule ourselves.
        // Staged agents shoot for the camera, not for blood.
        if !staged_no_damage {
            self.test_player_hit(actor_index, origin, dir, end);
        }

        let from = origin;
        let to = end.unwrap_or([
            origin[0] + dir[0] * 120.0,
            origin[1] + dir[1] * 120.0,
            origin[2] + dir[2] * 120.0,
        ]);
        if (agent_id + ammo) % 3 == 0 {
            self.effects.push(AiEffect::BulletTracer { from, to, speed: 800.0 });
        }
    }

    /// `_testPlayerHit(agent, origin, dir, end)`. `index.js:628-655`.
    fn test_player_hit(&mut self, actor_index: usize, origin: Vec3, dir: Vec3, end: Option<Vec3>) {
        let p = self.player_position();
        let max_t = end.map_or(200.0, |e| distance(origin, e));
        let px = p[0] - origin[0];
        let py = p[1] - origin[1];
        let pz = p[2] - origin[2];
        let t = px * dir[0] + py * dir[1] + pz * dir[2];
        if t < 0.5 || t > max_t {
            return;
        }
        let miss = jsmath::hypot3(px - dir[0] * t, py - dir[1] * t, pz - dir[2] * t);
        if miss > 0.42 {
            if miss < 1.6 {
                self.effects.push(AiEffect::NearMiss { miss }); // whip-crack past the ear
            }
            return;
        }
        let amount =
            self.actors[actor_index].agent.weapon_damage * if miss < 0.16 { 1.25 } else { 1.0 };
        // Damage is applied *only* through the event below. `player` listens
        // for `damage:dealt` with itself as the target, so applying it here as
        // well wounded the player twice for every round that connected.
        self.effects.push(AiEffect::DamageDealt {
            amount,
            headshot: false,
            killed: false,
            point: p,
            from: origin,
            source_agent: self.actors[actor_index].agent.id,
        });
    }

    /// `emitReload(agent)`. `index.js:657-659`.
    fn emit_reload(&mut self, actor_index: usize) {
        let actor = self.actors[actor_index].agent.id;
        self.effects.push(AiEffect::WeaponReload { weapon: "ai_rifle", actor });
    }

    /// `throwGrenade(agent, from, target)`. `index.js:672-699`.
    pub fn throw_grenade(
        &mut self,
        actor_index: usize,
        from: Vec3,
        target: Vec3,
        gravity: f64,
        bodies: Option<&mut (dyn GrenadeBodies + '_)>,
    ) {
        let Some(bodies) = bodies else { return }; // `if (!phys) return`
        // lobbed ballistic solve
        let dx = target[0] - from[0];
        let dz = target[2] - from[2];
        let dist = f64::max(0.5, jsmath::hypot2(dx, dz));
        let g = gravity.abs();
        let speed = f64::min(18.0, f64::max(4.0, (dist * g) / 0.95).sqrt());
        let vy = speed * 0.62;
        let vh = f64::min(speed, dist / f64::max(0.35, (2.0 * vy) / g));
        let body = bodies.add(GrenadeBody {
            radius: 0.05,
            mass: 0.42,
            position: from,
            velocity: [(dx / dist) * vh, vy, (dz / dist) * vh],
            restitution: 0.28,
            friction: 0.7,
            lifetime: 9.0,
        });
        let agent = self.actors[actor_index].agent.id;
        self.grenades.push(Grenade { body, thrown_from: from, fuse: 2.35, agent });
        Animator::fire(&mut self.actors[actor_index].animator, 0.35);
    }

    /// `_updateGrenades(dt)`. `index.js:701-717`.
    fn update_grenades(&mut self, dt: f64, bodies: Option<&mut (dyn GrenadeBodies + '_)>) {
        let mut bodies = bodies;
        let mut i = self.grenades.len();
        while i > 0 {
            i -= 1;
            self.grenades[i].fuse -= dt;
            if self.grenades[i].fuse > 0.0 {
                continue;
            }
            let g = self.grenades[i];
            let p = g
                .body
                .and_then(|id| bodies.as_deref().and_then(|b| b.position(id)))
                .unwrap_or(g.thrown_from);
            self.effects.push(AiEffect::Explosion {
                position: p,
                radius: 6.5,
                damage: 120.0,
                source_agent: g.agent,
            });
            if let (Some(id), Some(b)) = (g.body, bodies.as_deref_mut()) {
                b.remove(id);
            }
            self.grenades.remove(i);
        }
    }

    /// The live grenades, in the order the source's `_grenades` array holds
    /// them.
    #[must_use]
    pub fn grenades(&self) -> &[Grenade] {
        &self.grenades
    }

    /* ================================================================== */
    /* frame                                                              */
    /* ================================================================== */

    /// `update(dt, ctx)`. `index.js:723-761`.
    pub fn update(
        &mut self,
        dt: f64,
        mut ballistics: Option<&mut (dyn AiBallistics + '_)>,
        mut bodies: Option<&mut (dyn GrenadeBodies + '_)>,
        gravity: f64,
    ) {
        if self.nav_pending {
            self.build_nav();
            // Populate the level for normal play. Capture runs stay empty
            // unless a shot asks for a tableau.
            if !self.nav_pending && (!self.deterministic || self.force_populate) {
                self.populate(2, 3);
            }
        }

        // Per-frame A* budget: see `request_path`.
        self.path_budget = self.paths_per_frame;
        self.update_relevance();

        // `for (const s of this.squads) s.update(dt)`
        let members = self.member_snapshots();
        for qi in 0..self.squads.len() {
            let out = self.squads[qi].update(dt, &members);
            for c in out.contacts {
                if let Some(a) = self.actors.iter_mut().find(|a| a.agent.id == c.member_id) {
                    a.agent.receive_squad_contact(c);
                }
            }
        }

        let mut alive = 0usize;
        for i in 0..self.actors.len() {
            if self.actors[i].agent.alive {
                // `animator.ejectWorld` as this tick opens — `onAgentFire`
                // reads it before `_drive` re-poses the skeleton.
                self.actors[i].eject_at_tick_start = {
                    let e = self.actors[i].animator.eject_world;
                    [e.x, e.y, e.z]
                };
                match self.actors[i].staged {
                    Some(_) => self.update_staged(
                        i,
                        dt,
                        ballistics.as_deref_mut(),
                        bodies.as_deref_mut(),
                        gravity,
                    ),
                    None => self.step_actor(
                        i,
                        dt,
                        ballistics.as_deref_mut(),
                        bodies.as_deref_mut(),
                        gravity,
                    ),
                }
                alive += 1;
            } else if let Some(t) = self.actors[i].agent.dead_time {
                self.actors[i].agent.dead_time = Some(t + dt);
            }
        }
        self.update_grenades(dt, bodies.as_deref_mut());
        self.stats.agents = self.actors.len();
        self.stats.alive = alive;
    }

    /// `lateUpdate()`. `index.js:763-773`.
    pub fn late_update(&mut self) {
        self.ground.begin();
        for i in 0..self.actors.len() {
            let segments = {
                let a = &self.actors[i];
                a.agent.sync_hitboxes(&a.animator)
            };
            self.actors[i].hitboxes = segments;
            // Dead men keep their contact: a ragdoll on the floor needs it most.
            let a = &self.actors[i];
            self.ground.add_actor(
                a.agent.position,
                a.agent.yaw,
                a.agent.scale,
                a.agent.crouch,
                Some(&a.animator as &dyn FootSource),
            );
        }
    }

    /// The contact-shadow placements collected by the last
    /// [`AiCore::late_update`]. `grounding.js`'s two instanced meshes.
    #[must_use]
    pub fn shadow_placements(&self) -> (&[Placement], &[Placement]) {
        self.ground.end()
    }

    /// One live agent's five phases, interleaved exactly as `agent.js:267-287`
    /// runs them.
    ///
    /// [`Agent::update`] returns its events in a batch at the end, which is one
    /// tick too late for two of them: `onAgentFire` reads
    /// `animator.ejectWorld` *before* `_drive` re-poses the skeleton
    /// (`index.js:600`), and `throwGrenade` calls `animator.fire(0.35)` from
    /// inside `_think` (`index.js:698`), so a deferred grenade would start its
    /// recoil a frame late and every muzzle transform after it would drift.
    /// So the phases are driven individually here — the same thing
    /// `_updateStaged` does in the source (`index.js:899-901`).
    fn step_actor(
        &mut self,
        i: usize,
        dt: f64,
        ballistics: Option<&mut (dyn AiBallistics + '_)>,
        bodies: Option<&mut (dyn GrenadeBodies + '_)>,
        gravity: f64,
    ) {
        // `agent.js:271-287`'s prologue.
        {
            let a = &mut self.actors[i].agent;
            a.state_time += dt;
            a.suppression = f64::max(0.0, a.suppression - dt * 0.55);
            a.fire_cooldown -= dt;
            a.burst_cooldown -= dt;
            a.grenade_cooldown -= dt;
            a.peek_timer -= dt;
            a.repath_timer -= dt;
            a.vault_cooldown -= dt;
            if a.last_known_age < 1e6 {
                a.last_known_age += dt;
            }
        }

        let mut ballistics = ballistics;
        let mut bodies = bodies;
        // a path the frame budget deferred: ask again before anything else does
        if self.actors[i].agent.path_pending {
            let dest = self.actors[i].pending_dest;
            self.run_phase(i, gravity, dt, Phase5::PendingPath(dest));
        }

        let player = self.player_position_opt();
        let phys = self.phys.clone();
        {
            let a = &mut self.actors[i].agent;
            a.sense(dt, player, phys.as_deref());
        }

        let events = self.run_phase(i, gravity, dt, Phase5::Think);
        self.drain_agent_events_full(
            i,
            &events,
            ballistics.as_deref_mut(),
            bodies.as_deref_mut(),
            gravity,
        );

        self.run_phase(i, gravity, dt, Phase5::Move);

        let events = self.run_phase(i, gravity, dt, Phase5::Shoot);
        self.drain_agent_events_full(
            i,
            &events,
            ballistics.as_deref_mut(),
            bodies.as_deref_mut(),
            gravity,
        );

        self.place_actor_for_drive(i, dt);
        self.run_phase(i, gravity, dt, Phase5::Drive);
    }

    /// `_updateStaged(a, dt)`. `index.js:869-902`.
    fn update_staged(
        &mut self,
        i: usize,
        dt: f64,
        ballistics: Option<&mut (dyn AiBallistics + '_)>,
        bodies: Option<&mut (dyn GrenadeBodies + '_)>,
        gravity: f64,
    ) {
        let s = self.actors[i].staged.expect("caller checked");
        let p = self.player_position();
        {
            let a = &mut self.actors[i].agent;
            a.state_time += dt;
            a.fire_cooldown -= dt;
            a.burst_cooldown -= dt;
            a.state = AgentState::Combat;
            a.has_target = true;
            a.target_visible = true;
            a.alertness = 1.0;
            a.last_known = p;
            a.last_known_age = 0.0;
            a.crouch = s.crouch;
            a.aim_weight = s.aim_weight;
            a.suppression = s.suppression;
            a.desired_speed = s.speed;
            a.want_fire = s.fire;
            if s.speed != 0.0 {
                a.has_move_target = true;
                if a.path.is_empty() {
                    a.path.push([0.0, 0.0, 0.0]);
                }
                a.path[0] = [
                    a.position[0] + s.heading[0] * 6.0,
                    a.position[1] + s.heading[1] * 6.0,
                    a.position[2] + s.heading[2] * 6.0,
                ];
                a.path_len = 1;
                a.path_index = 0;
            } else {
                a.has_move_target = false;
            }
        }
        if s.reload_every != 0.0
            && self.actors[i].agent.state_time > s.reload_every
            && !Animator::reloading(&self.actors[i].animator)
        {
            self.actors[i].agent.state_time = 0.0;
            Animator::reload(&mut self.actors[i].animator, 2.4);
        }

        let mut ballistics = ballistics;
        let mut bodies = bodies;
        self.run_phase(i, gravity, dt, Phase5::Move);
        let events = self.run_phase(i, gravity, dt, Phase5::Shoot);
        self.drain_agent_events_full(
            i,
            &events,
            ballistics.as_deref_mut(),
            bodies.as_deref_mut(),
            gravity,
        );
        self.place_actor_for_drive(i, dt);
        self.run_phase(i, gravity, dt, Phase5::Drive);
    }

    /// `agent.js:941-943` — the three group writes at the top of `_drive`,
    /// which [`Agent::drive`] leaves to this tier because they are scene-graph
    /// state ("minus the three `group` writes", `agent.rs`).
    ///
    /// They sit **after** `_drive`'s vault root motion and **before** its
    /// `an.setState`/`an.update`, so the animator must see the post-lerp
    /// position. `drive` still owns that mutation; the same lerp is evaluated
    /// here read-only so the actor node is current when the IK runs.
    fn place_actor_for_drive(&mut self, i: usize, dt: f64) {
        let actor = &mut self.actors[i];
        let a = &actor.agent;
        let mut position = a.position;
        if let (Some(vt), Some(from), Some(to)) = (a.vault_t, a.vault_from, a.vault_to) {
            if Animator::vaulting(&actor.animator) {
                let t = (vt + dt / 0.8).min(1.0);
                position = [
                    from[0] + (to[0] - from[0]) * t,
                    from[1] + (to[1] - from[1]) * t,
                    from[2] + (to[2] - from[2]) * t,
                ];
                position[1] += (t * std::f64::consts::PI).sin() * 0.42;
            }
        }
        let yaw = a.yaw;
        actor
            .animator
            .set_actor(V3::new(position[0], position[1], position[2]), yaw);
    }

    /// Build the per-frame [`AgentCtx`] and run one phase of `agent.js:281-286`
    /// against it.
    ///
    /// A closure would read better, but `AgentCtx<'a>` borrows eight things
    /// created inside this function, and `impl FnOnce(&mut AgentCtx<'_>)`
    /// either pins `'a` to `'static` (elided) or demands the callback work for
    /// every `'a` (higher-ranked). Naming the phases as data sidesteps both,
    /// and it makes the source's five-call sequence legible at the call site.
    fn run_phase(&mut self, i: usize, gravity: f64, dt: f64, phase: Phase5) -> Vec<AgentEvent> {
        let player = self.player_position_opt();
        let neighbors: Vec<Neighbor> = self.actors.iter().map(|a| a.agent.as_neighbor()).collect();
        let members = self.member_snapshots();
        let squad_positions: Option<Vec<SquadMemberPos>> = self.actors[i].squad.map(|qi| {
            self.squads[qi]
                .members
                .iter()
                .filter_map(|id| self.actors.iter().find(|a| a.agent.id == *id))
                .map(|a| SquadMemberPos {
                    id: a.agent.id,
                    alive: a.agent.alive,
                    x: a.agent.position[0],
                    z: a.agent.position[2],
                })
                .collect()
        });
        let elapsed = self.elapsed;
        let phys = self.phys.clone();
        let fallback = self.world.ground_height;
        let squad_index = self.actors[i].squad;

        let AiCore { actors, squads, grid, cover, path_budget, stats, .. } = self;
        let (left, right) = actors.split_at_mut(i);
        let _ = left;
        let (actor, _rest) = right.split_first_mut().expect("index in range");

        let ground = GroundQuery { phys: phys.as_deref(), fallback };
        let mut path = BudgetedPath {
            grid: grid.as_mut(),
            budget: path_budget,
            deferred: &mut stats.paths_deferred,
            pending_dest: &mut actor.pending_dest,
        };
        let mut seat = squad_index.map(|qi| Seat { squad: &mut squads[qi], members: &members });
        let mut controller = actor.controller.as_deref_mut().map(CtrlRef);

        let mut w = AgentCtx {
            player,
            phys: phys.as_deref(),
            gravity,
            elapsed,
            neighbors: &neighbors,
            animator: &mut actor.animator,
            controller: controller.as_mut().map(|c| c as &mut dyn AgentController),
            path: Some(&mut path),
            cover: cover.as_mut().map(|c| c as &mut dyn CoverSource),
            squad: seat.as_mut().map(|s| s as &mut dyn SquadPermissions),
            squad_positions: squad_positions.as_deref(),
            ground: &ground,
        };
        let a = &mut actor.agent;
        let mut events = Vec::new();
        match phase {
            Phase5::PendingPath(dest) => {
                a.go_to(&mut w, dest);
            }
            Phase5::Think => a.think(dt, &mut w, &mut events),
            Phase5::Move => {
                a.move_step(dt, &mut w);
            }
            Phase5::Shoot => a.shoot(dt, &mut w, &mut events),
            Phase5::Drive => a.drive(dt, &mut w),
        }
        events
    }

    /// Turn one agent's events into this system's outward calls.
    /// `agent.js` raises them through `this.ai.*`; the port returns them as
    /// data (see [`AgentEvent`]).
    fn drain_agent_events_full(
        &mut self,
        i: usize,
        events: &[AgentEvent],
        ballistics: Option<&mut (dyn AiBallistics + '_)>,
        bodies: Option<&mut (dyn GrenadeBodies + '_)>,
        gravity: f64,
    ) {
        let mut ballistics = ballistics;
        let mut bodies = bodies;
        for e in events {
            match *e {
                AgentEvent::Reload => self.emit_reload(i),
                AgentEvent::Fire { origin, dir } => {
                    self.on_agent_fire(i, origin, dir, ballistics.as_deref_mut());
                }
                AgentEvent::Grenade { from, target } => {
                    self.throw_grenade(i, from, target, gravity, bodies.as_deref_mut());
                }
                AgentEvent::Death { point, impulse, headshot } => {
                    let actor = self.actors[i].agent.id;
                    self.effects
                        .push(AiEffect::ActorDeath { actor, point, impulse, headshot });
                }
            }
        }
    }

    /// The death events an out-of-frame `applyDamage` produced.
    fn drain_agent_events(&mut self, i: usize, events: &[AgentEvent]) {
        for e in events {
            if let AgentEvent::Death { point, impulse, headshot } = *e {
                let actor = self.actors[i].agent.id;
                self.effects.push(AiEffect::ActorDeath { actor, point, impulse, headshot });
            }
        }
    }

    fn member_snapshots(&self) -> Vec<MemberSnapshot> {
        self.actors.iter().map(|a| a.agent.snapshot()).collect()
    }

    fn player_position_opt(&self) -> Option<Vec3> {
        Some(self.player_position())
    }

    /* ================================================================== */
    /* frame budgets and LOD                                              */
    /* ================================================================== */

    /// `requestPath(from, dest, out)`. `index.js:785-793`. `None` is the
    /// source's `-1` — this frame's budget is spent.
    pub fn request_path(&mut self, from: Vec3, dest: Vec3) -> Option<Vec<Vec3>> {
        let Some(grid) = self.grid.as_mut() else { return Some(Vec::new()) };
        if self.path_budget <= 0 {
            self.stats.paths_deferred += 1;
            return None;
        }
        self.path_budget -= 1;
        Some(grid.find_path(from, dest, super::nav::FindPathOpts::default()))
    }

    /// Reset the frame's A* budget without stepping. `update` does this itself;
    /// a caller driving `request_path` directly needs it too.
    pub fn reset_path_budget(&mut self) {
        self.path_budget = self.paths_per_frame;
    }

    /// `_updateRelevance(ctx)`. `index.js:825-859`.
    ///
    /// An actor is IRRELEVANT only when both hold: its (already 1.45x
    /// inflated) bounding sphere, grown by a further 4 m, misses the camera
    /// frustum; and the volume its sun shadow could darken — the sphere swept
    /// along `-sunDir` — misses it too.
    pub fn update_relevance(&mut self) {
        let mvp = Mat4::multiply_matrices(
            &Mat4 { e: self.camera.projection_matrix },
            &Mat4 { e: self.camera.matrix_world_inverse },
        );
        let frustum = Frustum::from_projection_matrix(&mvp.e);
        let sun = self.sun_direction();
        // how far a shadow ray can travel before it is under the level
        let floor_y = if self.grid.is_some() { -6.0 } else { -20.0 };
        let sun_y = f64::max(0.06, sun[1]);
        let mut irrelevant = 0usize;

        for a in &mut self.actors {
            let m = actor_matrix_world(a.agent.position, a.agent.yaw, a.agent.scale);
            let (center, radius) = sphere_apply_matrix4(a.bounding_sphere, &m);
            let radius = radius + 4.0;
            let mut visible = frustum.intersects_sphere(center, radius);
            if !visible {
                let t_max = f64::min(320.0, (center[1] - floor_y) / sun_y);
                let step = f64::max(2.0, radius * 0.9);
                let mut t = step;
                while t <= t_max {
                    let c = [
                        center[0] + sun[0] * -t,
                        center[1] + sun[1] * -t,
                        center[2] + sun[2] * -t,
                    ];
                    if frustum.intersects_sphere(c, radius) {
                        visible = true;
                        break;
                    }
                    t += step;
                }
            }
            a.agent.lod_irrelevant = !visible;
            if !visible {
                irrelevant += 1;
            }
            a.no_shadow = !visible;
        }
        self.stats.lod_irrelevant = irrelevant;
    }

    /* ================================================================== */
    /* staged tableau for the capture harness                             */
    /* ================================================================== */

    /// `_stageSlot(cam, ndcX, wantDepth, placed)`. `index.js:911-973`.
    pub fn stage_slot(&self, ndc_x: f64, want_depth: f64, placed: &[Vec3]) -> Vec3 {
        let cam = &self.camera;
        // `this._v.set(0, 0, -1).applyQuaternion(cam.quaternion)`
        let q = Q::new(cam.quaternion[0], cam.quaternion[1], cam.quaternion[2], cam.quaternion[3]);
        let mut f = V3::new(0.0, 0.0, -1.0).apply_quat(q);
        f.y = 0.0;
        let f = f.normalize();
        let (rx, rz) = (f.z, -f.x); // camera right, flattened
        let tan_h = ((cam.fov * std::f64::consts::PI) / 360.0).tan() * cam.aspect;
        let ideal = [
            cam.position[0] + f.x * want_depth + rx * (ndc_x * tan_h * want_depth),
            cam.position[1] + f.y * want_depth,
            cam.position[2] + f.z * want_depth + rz * (ndc_x * tan_h * want_depth),
        ];
        let y_ref = cam.position[1] - 1.7;
        let mut out = [ideal[0], y_ref, ideal[2]];
        let Some(g) = self.grid.as_ref() else {
            out[1] = self.ground_at(out[0], out[2], cam.position[1] + 3.0);
            return out;
        };
        let cx = g.cell_x(ideal[0]);
        let cz = g.cell_z(ideal[2]);
        let span = (7.0 / g.cell).ceil() as i64;
        let mut best: i64 = -1;
        let mut best_score = f64::INFINITY;
        let mut best_x = 0.0;
        let mut best_z = 0.0;
        for dz in -span..=span {
            for dx in -span..=span {
                let (ix, iz) = (cx + dx, cz + dz);
                if !g.walkable(ix, iz, false) {
                    continue;
                }
                let i = g.index(ix, iz);
                let fy = f64::from(g.floor()[i]);
                if (fy - y_ref).abs() > 1.0 {
                    continue;
                }
                let x = g.world_x(ix);
                let z = g.world_z(iz);
                // spacing from the men already placed
                if placed.iter().any(|q| jsmath::hypot2(q[0] - x, q[2] - z) < 2.4) {
                    continue;
                }
                // project
                let ex = x - cam.position[0];
                let ez = z - cam.position[2];
                let depth = ex * f.x + ez * f.z;
                if depth < 3.0 {
                    continue;
                }
                let lateral = ex * rx + ez * rz;
                let ndc = lateral / (depth * tan_h);
                // must be visible: chest and head
                if let Some(p) = self.phys.as_deref() {
                    if !line_of_sight(p, cam.position, [x, fy + 1.25, z], mask::SIGHT) {
                        continue;
                    }
                    if !line_of_sight(p, cam.position, [x, fy + 1.62, z], mask::SIGHT) {
                        continue;
                    }
                }
                let mut score = (ndc - ndc_x).abs() * 9.0 + (depth - want_depth).abs() * 0.5;
                // prefer standing next to something solid
                score -= f64::from(g.enclosure()[i]) * 0.35;
                if score < best_score {
                    best_score = score;
                    best = i as i64;
                    best_x = x;
                    best_z = z;
                }
            }
        }
        if best >= 0 {
            out = [best_x, f64::from(g.floor()[best as usize]), best_z];
        } else {
            out[1] = self.ground_at(out[0], out[2], cam.position[1] + 3.0);
        }
        out
    }

    /// `debugStage(name)`. `index.js:1107-1109` — the dispatcher.
    ///
    /// This is the AI's whole capture-facing API, and it is the reason the
    /// tableau below is reachable at all: every caller in the source passes a
    /// *name* (`dev/shots.js:81`, `core/prewarm.js:493` and `:654`,
    /// `tools/profile.mjs:68`, `ai/aicost.mjs:38`), and one of them
    /// (`prewarm.js:654`) passes `'none'` **specifically to hit the no-op
    /// path** — it is how the prewarm harness asks for a frame with no staged
    /// combat in it. Porting only the `'firefight'` body, as this file did,
    /// left that vocabulary inexpressible and left the module doc's promise of
    /// `ai.debugStage('firefight')` implemented by nothing.
    ///
    /// `None` is the source's `return this.stats`: nothing was staged, so there
    /// is no time-of-day to push. `Some(hour)` is the write into `sky` that
    /// [`AiCore::debug_stage_firefight`] hands back rather than makes — the
    /// caller applies it (`index.js:1116`'s
    /// `ctx.peek('sky')?.setTimeOfDay?.(17.9)`).
    pub fn debug_stage(&mut self, name: &str) -> Option<f64> {
        (name == "firefight").then(|| self.debug_stage_firefight())
    }

    /// `debugStage('firefight')`. `index.js:980-1054`.
    ///
    /// A staged firefight in front of the shot camera: one man up and firing
    /// from behind hard cover, one crouched and peeking, one moving between
    /// positions, one reloading further back.
    ///
    /// Returns the time-of-day the source pushes into `sky.setTimeOfDay(17.9)`
    /// — a write into another subsystem, handed back rather than made.
    pub fn debug_stage_firefight(&mut self) -> f64 {
        if self.inspect {
            return self.stage_inspect();
        }
        if self.nav_pending {
            self.build_nav();
        }
        let cam_position = self.camera.position;
        let q = Q::new(
            self.camera.quaternion[0],
            self.camera.quaternion[1],
            self.camera.quaternion[2],
            self.camera.quaternion[3],
        );
        let mut f = V3::new(0.0, 0.0, -1.0).apply_quat(q);
        f.y = 0.0;
        let f = f.normalize();
        let right = [f.z, 0.0, -f.x];
        let squad = self.create_squad();

        /// `[variant, ndcX, depth, crouch, speed, fire, reloadEvery]`
        const LAYOUT: [(&str, f64, f64, bool, f64, bool, f64); 5] = [
            // hero: up and firing, left of frame, close enough to read the kit
            ("vanguard", -0.44, 8.0, false, 0.0, true, 0.0),
            // second man crouched in cover, right of frame
            ("breacher", 0.30, 12.0, true, 0.0, true, 0.0),
            // one caught mid-stride between positions
            ("irregular", -0.14, 16.0, false, 4.1, false, 0.0),
            // one reloading behind cover on the far right
            ("vanguard", 0.60, 9.5, true, 0.0, true, 3.4),
            // depth: a fifth man well down the street
            ("irregular", -0.26, 22.0, false, 0.0, true, 0.0),
        ];

        let mut placed: Vec<Vec3> = Vec::new();
        for (variant, ndc_x, d, crouch, speed, fire, reload) in LAYOUT {
            let pos = self.stage_slot(ndc_x, d, &placed);
            let yaw = (cam_position[0] - pos[0]).atan2(cam_position[2] - pos[2]);
            let idx = self.spawn(variant, pos, yaw, None);
            let agent_id = self.actors[idx].agent.id;
            self.squads[squad].add(agent_id);
            self.actors[idx].squad = Some(squad);
            self.actors[idx].staged = Some(Staged {
                crouch,
                speed,
                fire,
                no_damage: true,
                heading: [right[0] * -1.0, right[1] * -1.0, right[2] * -1.0],
                aim_weight: 1.0,
                reload_every: reload,
                suppression: if crouch { 0.15 } else { 0.0 },
            });
            // stagger the burst timers so the frame catches muzzle flashes
            self.actors[idx].agent.burst_cooldown = self.rng.range(0.0, 0.3);
            self.actors[idx].agent.burst_left = self.rng.int(2, 6);
            self.actors[idx].agent.peeking = true;
            self.actors[idx].agent.aim_target = self.player_position();
            // `a.animator.update(0.016, 0)` — the actor node was written by
            // `spawn`, exactly as the source's constructor writes the group.
            Animator::update(&mut self.actors[idx].animator, 0.016, 0.0);
            placed.push(pos);
        }

        // One man already down, handed to the ragdoll solver with the round's
        // impulse — it dresses the tableau and it exercises the death path.
        let d_pos = self.stage_slot(-0.58, 9.4, &placed);
        let yaw = (cam_position[0] - d_pos[0]).atan2(cam_position[2] - d_pos[2]);
        let idx = self.spawn("breacher", d_pos, yaw, None);
        let agent_id = self.actors[idx].agent.id;
        self.squads[squad].add(agent_id);
        self.actors[idx].squad = Some(squad);
        Animator::update(&mut self.actors[idx].animator, 0.016, 0.0);
        let hit = [d_pos[0], d_pos[1] + 1.35, d_pos[2]];
        let mut inc = [
            hit[0] - cam_position[0],
            hit[1] - cam_position[1],
            hit[2] - cam_position[2],
        ];
        let l = (inc[0] * inc[0] + inc[1] * inc[1] + inc[2] * inc[2]).sqrt();
        let ivl = 1.0 / jsmath::or_one(l);
        inc = [inc[0] * ivl, inc[1] * ivl, inc[2] * ivl];
        let mut events = Vec::new();
        {
            let AiCore { actors, cover, .. } = self;
            let actor = &mut actors[idx];
            actor.agent.apply_damage(
                260.0,
                BodyPart::Torso,
                hit,
                Some(inc),
                &mut actor.animator,
                cover.as_mut().map(|c| c as &mut dyn CoverSource),
                &mut events,
            );
        }
        self.drain_agent_events(idx, &events);

        17.9
    }

    /// `_stageInspect()`. `index.js:1057-1083`. Model inspection line-up.
    fn stage_inspect(&mut self) -> f64 {
        let cam_position = self.camera.position;
        let q = Q::new(
            self.camera.quaternion[0],
            self.camera.quaternion[1],
            self.camera.quaternion[2],
            self.camera.quaternion[3],
        );
        let mut f = V3::new(0.0, 0.0, -1.0).apply_quat(q);
        f.y = 0.0;
        let f = f.normalize();
        let right = [f.z, 0.0, -f.x];
        const LAYOUT: [(&str, f64, f64, f64); 3] = [
            ("vanguard", 1.9, 0.35, 0.25),
            ("irregular", 2.7, -0.95, 3.0),
            ("breacher", 3.6, 1.15, -0.7),
        ];
        for (nm, d, s2, extra_yaw) in LAYOUT {
            let mut p = [
                cam_position[0] + f.x * d + right[0] * s2,
                cam_position[1] + f.y * d + right[1] * s2,
                cam_position[2] + f.z * d + right[2] * s2,
            ];
            p[1] = self.ground_at(p[0], p[2], cam_position[1] + 1.0);
            let to_cam = (cam_position[0] - p[0]).atan2(cam_position[2] - p[2]);
            let idx = self.spawn(nm, p, to_cam + extra_yaw, None);
            self.actors[idx].staged = Some(Staged {
                crouch: false,
                speed: 0.0,
                fire: false,
                no_damage: false,
                heading: [0.0, 0.0, 1.0],
                aim_weight: 1.0,
                reload_every: 0.0,
                suppression: 0.0,
            });
        }
        11.5
    }

    /* ================================================================== */

    /// `dispose()`. `index.js:1087-1104`, minus the GPU/scene teardown.
    pub fn dispose(&mut self) {
        for a in &mut self.actors {
            a.agent.dispose();
        }
        self.actors.clear();
        self.squads.clear();
        self.grenades.clear();
        self.variants.clear();
    }
}

/// `Math.hypot(dx, dy, dz)`-free distance: `THREE.Vector3.distanceTo` really
/// is the root of the sum of squares.
fn distance(a: Vec3, b: Vec3) -> f64 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    let dz = a[2] - b[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// `_distanceToRay(point, origin, dir, eyeH)`. `index.js:361-367` — note the
/// `Math.hypot`, which is NOT the plain root here.
#[must_use]
pub fn distance_to_ray(point: Vec3, origin: Vec3, dir: Vec3, eye_h: f64) -> f64 {
    let px = point[0] - origin[0];
    let py = point[1] + eye_h * 0.7 - origin[1];
    let pz = point[2] - origin[2];
    let t = f64::max(0.0, px * dir[0] + py * dir[1] + pz * dir[2]);
    jsmath::hypot3(px - dir[0] * t, py - dir[1] * t, pz - dir[2] * t)
}


/* ================================================================== */
/* The Subsystem wrapper                                              */
/* ================================================================== */

/// The registered subsystem. `static id = 'ai'`, `static deps = ['physics',
/// 'world']`.
pub struct AiSystem {
    core: Rc<RefCell<AiCore>>,
    offs: Vec<(&'static str, SubscriptionId)>,
}

impl AiSystem {
    #[must_use]
    pub fn new(core: AiCore) -> Self {
        AiSystem { core: Rc::new(RefCell::new(core)), offs: Vec::new() }
    }

    /// The shared guts, so the app can call the public API and read the
    /// journal. Same handle shape as `AudioSystem::core`.
    #[must_use]
    pub fn core(&self) -> Rc<RefCell<AiCore>> {
        Rc::clone(&self.core)
    }

    /// `_wireEvents(ctx)`. `index.js:292-350`.
    ///
    /// **Three of the five subscriptions cannot be complete today**, because
    /// the crate's event-payload vocabulary is forked (see the comment above
    /// the argument types). This function subscribes to the EXISTING payload
    /// type that comes closest, and the gaps are listed here rather than
    /// papered over with a fourth set of structs:
    ///
    /// | event | type downcast to | complete? |
    /// |---|---|---|
    /// | `weapon:fire` | [`crate::audio::system::WeaponFire`] | **no** — it has no `dir`, so the line-of-fire suppression arm (`index.js:303-306`) never runs |
    /// | `bullet:impact` | [`crate::audio::system::BulletImpact`] | yes |
    /// | `explosion` | [`crate::player::system::ExplosionEvent`] | yes — the only one of the three `explosion` types carrying `damage` |
    /// | `player:footstep` | [`crate::player::system::PlayerFootstepEvent`] | yes — and it is the emitter's own type |
    /// | `damage:dealt` | *nothing* | **no** — no existing type names WHICH agent was hit, nor `part`/`incident`/`amount` together |
    ///
    /// [`AiCore::on_weapon_fire`] and [`AiCore::on_damage_dealt`] are complete
    /// and are pinned by the golden; only the *bus adaptation* is short. A
    /// caller that needs the full behaviour today calls the methods directly,
    /// which is what an app translating between module contracts does anyway.
    pub fn wire_events(&mut self, ctx: &Ctx<'_>) {
        {
            let core = Rc::clone(&self.core);
            let id = ctx.events.on("weapon:fire", move |p: &dyn Any| {
                if let Some(p) = p.downcast_ref::<crate::audio::system::WeaponFire>() {
                    if let Some(origin) = p.origin {
                        core.borrow_mut().on_weapon_fire(&WeaponFireHeard {
                            weapon: p.weapon.clone(),
                            origin,
                            dir: None,
                        });
                    }
                }
                Ok(())
            });
            self.offs.push(("weapon:fire", id));
        }
        {
            let core = Rc::clone(&self.core);
            let id = ctx.events.on("bullet:impact", move |p: &dyn Any| {
                if let Some(p) = p.downcast_ref::<crate::audio::system::BulletImpact>() {
                    if let Some(point) = p.point {
                        core.borrow_mut().on_bullet_impact(&BulletImpactHeard { point });
                    }
                }
                Ok(())
            });
            self.offs.push(("bullet:impact", id));
        }
        {
            let core = Rc::clone(&self.core);
            let id = ctx.events.on("explosion", move |p: &dyn Any| {
                if let Some(p) = p.downcast_ref::<crate::player::system::ExplosionEvent>() {
                    core.borrow_mut().on_explosion(&ExplosionHeard {
                        position: p.position,
                        radius: p.radius,
                        damage: p.damage,
                    });
                }
                Ok(())
            });
            self.offs.push(("explosion", id));
        }
        {
            let core = Rc::clone(&self.core);
            let id = ctx.events.on("player:footstep", move |p: &dyn Any| {
                if let Some(p) = p.downcast_ref::<crate::player::system::PlayerFootstepEvent>() {
                    core.borrow_mut().on_player_footstep(&PlayerFootstepHeard {
                        position: p.position,
                        running: p.running,
                    });
                }
                Ok(())
            });
            self.offs.push(("player:footstep", id));
        }
    }
}

impl Subsystem for AiSystem {
    fn id(&self) -> &'static str {
        "ai"
    }

    fn deps(&self) -> &'static [&'static str] {
        &["physics", "world"]
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Update, Phase::LateUpdate]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn init(&mut self, ctx: &Ctx<'_>) -> Result<(), crate::error::CoreError> {
        self.wire_events(ctx);
        let mut core = self.core.borrow_mut();
        core.deterministic = ctx.config.deterministic;
        core.boot_nav();
        core.prewarm_materials();
        Ok(())
    }

    fn update(&mut self, dt: Seconds, ctx: &Ctx<'_>) {
        let mut core = self.core.borrow_mut();
        core.set_clock(ctx.time.frame, ctx.time.elapsed);
        // Gravity, ballistics and the grenade body sink all live behind the
        // physics facade, which `Ctx` does not carry — the app hands them in
        // through `AiCore::update` directly. Stepping without them is the
        // source's own `if (!phys)` path.
        core.update(f64::from(dt.get()), None, None, DEFAULT_GRAVITY);
    }

    fn late_update(&mut self, _dt: Seconds, _ctx: &Ctx<'_>) {
        self.core.borrow_mut().late_update();
    }

    fn dispose(&mut self) {
        self.offs.clear();
        self.core.borrow_mut().dispose();
    }
}

/// `UNITS.gravity` (`physics/index.js:194`) — the value `throwGrenade` reads
/// off the physics facade when one is wired.
pub const DEFAULT_GRAVITY: f64 = crate::config::UNITS.gravity;
