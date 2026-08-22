//! The player subsystem facade — the integration layer.
//!
//! Ported from Claude-of-Duty `src/player/index.js:1-752` — the whole file.
//!
//! This is what turns the already-ported movement machine, camera rig, mantle
//! probe, springs and tuning into a running player: it owns the frame order,
//! the look consumption, the ADS blend, the one-shot event drain, the health
//! model, the AI-facing hitbox and the low-health screen treatment.
//!
//! ```text
//! PUBLIC API   const p = ctx.get('player')
//! TRANSFORM  position feet_position eye_position velocity forward yaw pitch
//!            speed horizontal_speed character height hitbox
//! STATE      state stance sprinting tactical_sprint sliding grounded airborne
//!            mantling lean_amount slide_progress
//! AIM        ads_requested ads_progress set_ads_progress
//! FEEL       add_recoil add_kick add_trauma view_kick camera_rig
//! HEALTH     health max_health health_fraction low_health dead suppression
//!            damage_indicators apply_damage heal add_suppression
//! CONTROL    set_control_enabled teleport respawn debug_state
//! ```
//!
//! Collision is *never* computed here — everything goes through the character
//! controller and [`PhysicsWorld`]'s capsule sweeps.
//!
//! ## Events
//!
//! Emitted: `player:state`, `player:land`, `player:footstep`, `player:jump`,
//! `player:mantle`, and (from [`crate::player::health`]) `damage:taken`,
//! `player:health`, `player:heartbeat`, `player:death`.
//! Consumed: `damage:dealt`, `explosion`, `bullet:impact`.
//!
//! ## The six seams this port had to name
//!
//! 1. **`ctx.camera`.** The source composes the rig onto a live
//!    `THREE.PerspectiveCamera` in `CameraRig.applyTo` and then reads
//!    `camera.position` and `camera.quaternion` back out —
//!    `_onExplosion`, `_onBulletImpact` and `Health.damage` all need them.
//!    `crate::player::camera` deliberately did not port `applyTo` (there was
//!    no camera type to write onto), so the facade owns [`PlayerCamera`], a
//!    value-typed stand-in, and [`PlayerCamera::apply_rig`] is that missing
//!    `camera.js:346-356`. It belongs in `camera.rs` the moment a render
//!    camera lands; it lives here because the facade is the first caller that
//!    genuinely needs it.
//! 2. **`ctx.input`.** [`crate::player::movement::PlayerInput`] already names
//!    the four calls `latchInput` makes; the facade also reads `input.look`
//!    and `input.stick`, so [`PlayerLook`] extends it with those two.
//! 3. **`ctx.get('physics')`.** [`PlayerPhysics`] names the four calls the
//!    player makes on the physics facade — `groundHeight`, `lineOfSight`,
//!    `createCharacter`, and the ledge/lean probe it inherits from
//!    [`WorldProbe`]. [`PhysicsWorld`] implements it directly, so the real
//!    collision world binds with no adapter.
//! 4. **`ctx.peek('world').spawn(i)`.** `src/world/index.js` is a separate
//!    slice, so the spawn table arrives through [`SpawnSource`] — one method,
//!    exactly the one the source calls.
//! 5. **`physics.addCollider`.** No collider registry is ported yet
//!    (`src/physics/index.js` is its own slice), so [`PlayerHitbox`] is a
//!    plain value the facade owns and keeps on the interpolated capsule. What
//!    ships it to a registry is the physics facade's business.
//! 6. **`this.ctx` inside an event handler.** JavaScript's `this` reaches
//!    `ctx.time.elapsed`, `ctx.config` and `ctx.events` from inside a handler.
//!    [`EventBus`] handlers are `Fn`, so [`PlayerCore`] caches `time`,
//!    `config` and the bus as fields, refreshed at the top of every
//!    `fixed_update`/`update` — which *is* what `this.ctx` is, spelled out.
//!    Shared mutable state uses the same `Rc<RefCell<Core>>` shape
//!    `crate::audio::system` established for the same reason.
//!
//! ## Not ported
//!
//! `console.info` at `index.js:192-196` — a one-shot spawn banner. Console
//! output is not how this workspace reports anything; [`PlayerCore::spawn`]
//! carries the same facts as a value.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use axiom_kernel::Seconds;

use crate::config::Config;
use crate::engine::{Ctx, Time};
use crate::error::CoreError;
use crate::events::{EventBus, SubscriptionId};
use crate::input::Input;
use crate::physics::character::{Character, CharacterOpts};
use crate::physics::probe::PhysicsWorld;
use crate::physics::surfaces::{layer, mask};
use crate::player::camera::{CameraRig, Euler, HealthView, ViewKick};
use crate::player::health::{DamageIndicator, DamageKind, DamageOpts, Health};
use crate::player::lowhealth::LowHealthPass;
use crate::player::mantle::{LedgeKind, WorldProbe};
use crate::player::movement::{CharacterController, Movement, MovementState, PlayerInput};
use crate::player::springs::{approach, clamp, clamp01, lerp, DEG};
use crate::player::tuning::{Stance, CAMERA, FOOTSTEP, HEALTH, JUMP_SPEED, MOVE, STAND};
use crate::player::Vec3;
use crate::registry::{Phase, Subsystem};
use crate::rng::Rng;
use crate::world::palette::Surface;

/* ==================================================================== */
/* Seams                                                                */
/* ==================================================================== */

/// The `ctx.input` facts `index.js` reads that
/// [`crate::player::movement::PlayerInput`] does not already name:
/// `input.look` (`index.js:236-237`) and `input.stick` (`index.js:240-245`).
pub trait PlayerLook: PlayerInput {
    /// `input.look.x`, `input.look.y` — this frame's pointer delta, already
    /// scaled by sensitivity.
    fn look(&self) -> (f64, f64);
    /// `input.stick.lookX`, `input.stick.lookY` — the right stick, already
    /// dead-zoned and curved by `Input`.
    fn stick_look(&self) -> (f64, f64);
    /// [`Movement::latch_input`] wants the supertrait object, and a
    /// `&dyn PlayerLook` does not coerce to `&dyn PlayerInput` without trait
    /// upcasting, so implementors — which are always `Sized` — hand it over.
    /// Every implementation is `{ self }`.
    fn as_player_input(&self) -> &dyn PlayerInput;
}

impl PlayerLook for Input {
    fn look(&self) -> (f64, f64) {
        (self.look.x, self.look.y)
    }

    fn stick_look(&self) -> (f64, f64) {
        (self.stick.look_x, self.stick.look_y)
    }

    fn as_player_input(&self) -> &dyn PlayerInput {
        self
    }
}

/// One entry of `world.spawnPoints`. `world/index.js:409-412`'s return shape.
/// The source's `sp.yaw ?? 0` default is folded into the implementation, which
/// is where the table lives.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Spawn {
    pub position: Vec3,
    pub yaw: f64,
}

/// `ctx.peek('world')?.spawn?.(i)`. `index.js:202` and `index.js:659`.
pub trait SpawnSource {
    fn spawn(&self, index: i64) -> Option<Spawn>;
}

/// `ctx.get('physics')`, narrowed to the four things the player subsystem
/// actually asks of it — the same "name exactly the methods you call" seam
/// shape `crate::player::mantle::WorldProbe` and
/// `crate::audio::spatial::WorldProbe` already use.
///
/// [`PhysicsWorld`] implements it, so the real collision world binds with no
/// adapter; a test binds an analytic stub instead and the facade cannot tell.
pub trait PlayerPhysics: WorldProbe {
    /// `physics.groundHeight(x, z, fromY)` (`physics/index.js:675-678`). The
    /// source returns `-Infinity` for "no floor"; `None` is what every
    /// caller's `Number.isFinite(gy)` guard actually tests.
    fn ground_height(&self, x: f64, z: f64, from_y: f64) -> Option<f64>;

    /// `physics.lineOfSight(from, to, mask)` (`physics/index.js:616-623`).
    fn line_of_sight(&self, from: Vec3, to: Vec3, mask: u16) -> bool;

    /// `physics.createCharacter({...})` with [`PLAYER_CHARACTER`]'s options
    /// (`movement.js:141-149`). The source builds the controller inside
    /// `Movement.init`; this port's `Movement::init` takes an already-built
    /// one, so the construction lands on the physics seam where it belongs.
    fn create_player_character(&self) -> Box<dyn CharacterController>;

    /// `Movement::step` wants the supertrait object; see [`PlayerLook::
    /// as_player_input`] for why implementors hand it over rather than the
    /// call site coercing. Every implementation is `{ self }`.
    fn as_world_probe(&self) -> &dyn WorldProbe;
}

impl PlayerPhysics for PhysicsWorld {
    fn ground_height(&self, x: f64, z: f64, from_y: f64) -> Option<f64> {
        PhysicsWorld::ground_height(self, x, z, from_y)
    }

    /// `physics/index.js:616-623`, transcribed. Duplicated here because the
    /// physics *facade* (`src/physics/index.js`) is a separate slice that has
    /// not landed; when it does, this body should move to it. Normalises with
    /// `sqrt`, matching `crate::physics::probe`'s convention — the source uses
    /// `Math.hypot`, which rounds differently in the last bit.
    fn line_of_sight(&self, from: Vec3, to: Vec3, mask: u16) -> bool {
        let dx = to[0] - from[0];
        let dy = to[1] - from[1];
        let dz = to[2] - from[2];
        let d = (dx * dx + dy * dy + dz * dz).sqrt();
        if d < 1e-6 {
            return true;
        }
        !self.raycast_any(from, [dx / d, dy / d, dz / d], d - 1e-3, mask)
    }

    fn create_player_character(&self) -> Box<dyn CharacterController> {
        Box::new(Character::new(self.world(), PLAYER_CHARACTER))
    }

    fn as_world_probe(&self) -> &dyn WorldProbe {
        self
    }
}

/* ==================================================================== */
/* ctx.camera                                                           */
/* ==================================================================== */

/// The engine camera the rig is composed onto — the facade's stand-in for
/// `ctx.camera`, a `THREE.PerspectiveCamera` whose `rotation.order` the engine
/// sets to `'YXZ'` (`core/engine.js:30`).
///
/// Only the four things `index.js` and `health.js` actually read off it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerCamera {
    pub position: Vec3,
    /// `camera.rotation`, order `'YXZ'`.
    pub rotation: Euler,
    /// `camera.quaternion`, `[x, y, z, w]`. Three keeps this in sync on every
    /// `rotation.set` through the Euler's change callback, not on
    /// `updateMatrixWorld`.
    pub quaternion: [f64; 4],
    /// Degrees, as the source authors FOV.
    pub fov: f64,
}

impl PlayerCamera {
    pub fn new(fov: f64) -> Self {
        PlayerCamera {
            position: [0.0, 0.0, 0.0],
            rotation: Euler::default(),
            quaternion: [0.0, 0.0, 0.0, 1.0],
            fov,
        }
    }

    /// `CameraRig.applyTo(camera)`. `camera.js:346-356`.
    ///
    /// Lives here rather than in [`CameraRig`] because `camera.rs` had no
    /// camera type to write onto — see the module doc comment. It also writes
    /// `rig.forward`, which is the *only* place the source ever computes it.
    pub fn apply_rig(&mut self, rig: &mut CameraRig) {
        self.position = rig.eye_position;
        self.rotation = rig.rotation;
        // The source guards the FOV write (and the projection-matrix rebuild
        // behind it) on a 1e-3 degree change; without a projection matrix here
        // the guard only decides whether `camera.fov` moves, so it is kept.
        if (self.fov - rig.fov).abs() > 1e-3 {
            self.fov = rig.fov;
        }
        self.quaternion = quat_from_euler_yxz(self.rotation);
        rig.forward = apply_quaternion([0.0, 0.0, -1.0], self.quaternion);
    }
}

/// `THREE.Quaternion.setFromEuler(e)` for order `'YXZ'`, transcribed from
/// three r180 (`three.core.js:3730-3735`).
///
/// **The Euler-order trap.** `'YXZ'` composes `qY * qX * qZ`;
/// `axiom_math::Quat::from_euler_xyz` composes the opposite way and would
/// produce a camera that banks on its own. This is written out rather than
/// delegated for exactly that reason.
pub fn quat_from_euler_yxz(e: Euler) -> [f64; 4] {
    let (x, y, z) = (e.pitch, e.yaw, e.roll);
    let c1 = (x / 2.0).cos();
    let c2 = (y / 2.0).cos();
    let c3 = (z / 2.0).cos();
    let s1 = (x / 2.0).sin();
    let s2 = (y / 2.0).sin();
    let s3 = (z / 2.0).sin();
    [
        s1 * c2 * c3 + c1 * s2 * s3,
        c1 * s2 * c3 - s1 * c2 * s3,
        c1 * c2 * s3 - s1 * s2 * c3,
        c1 * c2 * c3 + s1 * s2 * s3,
    ]
}

/// `THREE.Vector3.applyQuaternion(q)`, transcribed from three r180
/// (`three.core.js:4798-4817`). The grouping is the source's, left to right —
/// folding it would change the last bits.
pub fn apply_quaternion(v: Vec3, q: [f64; 4]) -> Vec3 {
    let (vx, vy, vz) = (v[0], v[1], v[2]);
    let (qx, qy, qz, qw) = (q[0], q[1], q[2], q[3]);
    // t = 2 * cross(q.xyz, v)
    let tx = 2.0 * (qy * vz - qz * vy);
    let ty = 2.0 * (qz * vx - qx * vz);
    let tz = 2.0 * (qx * vy - qy * vx);
    // v + q.w * t + cross(q.xyz, t)
    [
        vx + qw * tx + qy * tz - qz * ty,
        vy + qw * ty + qz * tx - qx * tz,
        vz + qw * tz + qx * ty - qy * tx,
    ]
}

/* ==================================================================== */
/* The AI-facing hitbox                                                 */
/* ==================================================================== */

/// The capsule collider the source registers on `LAYER.PLAYER` so `ai` has
/// something to shoot at. `index.js:169-176`, plus the fields
/// `Collider.setSegment` writes (`physics/index.js:135-140`).
///
/// `LAYER.PLAYER` is deliberately absent from `MASK.BULLET` and
/// `MASK.CHARACTER`, so this can never be hit by the player's own muzzle ray
/// and never blocks the player's own movement sweeps: an AI that wants to hit
/// us traces with `MASK.BULLET | LAYER.PLAYER`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerHitbox {
    pub layer: u16,
    pub surface: Surface,
    pub part: &'static str,
    pub radius: f64,
    pub ax: f64,
    pub ay: f64,
    pub az: f64,
    pub bx: f64,
    pub by: f64,
    pub bz: f64,
    pub enabled: bool,
}

impl PlayerHitbox {
    /// `physics.addCollider({ shape: 'capsule', layer: LAYER.PLAYER, surface:
    /// 'flesh', owner: this, part: 'torso', radius: 0.3 })`. `owner` has no
    /// port equivalent (no registry holds it); the `isPlayer` flag it existed
    /// to expose is [`PlayerCore::IS_PLAYER`].
    pub fn new() -> Self {
        PlayerHitbox {
            layer: layer::PLAYER,
            surface: Surface::Flesh,
            part: "torso",
            radius: 0.3,
            ax: 0.0,
            ay: 0.0,
            az: 0.0,
            bx: 0.0,
            by: 0.0,
            bz: 0.0,
            enabled: true,
        }
    }

    /// `collider.setSegment(ax, ay, az, bx, by, bz, r)`.
    #[allow(clippy::too_many_arguments)]
    pub fn set_segment(&mut self, ax: f64, ay: f64, az: f64, bx: f64, by: f64, bz: f64, r: f64) {
        self.ax = ax;
        self.ay = ay;
        self.az = az;
        self.bx = bx;
        self.by = by;
        self.bz = bz;
        self.radius = r;
    }
}

impl Default for PlayerHitbox {
    fn default() -> Self {
        PlayerHitbox::new()
    }
}

/* ==================================================================== */
/* Event payloads                                                       */
/* ==================================================================== */

/// `player:state`'s `state` field. The ten [`MovementState`]s plus `lean`,
/// which only the facade produces (`index.js:379-380`) — the movement machine
/// itself never enters a lean state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerStateName {
    Stand,
    Crouch,
    Prone,
    Sprint,
    TacSprint,
    Slide,
    Jump,
    Fall,
    Mantle,
    Vault,
    Lean,
}

impl PlayerStateName {
    /// The string the source puts in the payload.
    pub fn as_str(self) -> &'static str {
        match self {
            PlayerStateName::Stand => "stand",
            PlayerStateName::Crouch => "crouch",
            PlayerStateName::Prone => "prone",
            PlayerStateName::Sprint => "sprint",
            PlayerStateName::TacSprint => "tacsprint",
            PlayerStateName::Slide => "slide",
            PlayerStateName::Jump => "jump",
            PlayerStateName::Fall => "fall",
            PlayerStateName::Mantle => "mantle",
            PlayerStateName::Vault => "vault",
            PlayerStateName::Lean => "lean",
        }
    }
}

impl From<MovementState> for PlayerStateName {
    fn from(s: MovementState) -> Self {
        match s {
            MovementState::Stand => PlayerStateName::Stand,
            MovementState::Crouch => PlayerStateName::Crouch,
            MovementState::Prone => PlayerStateName::Prone,
            MovementState::Sprint => PlayerStateName::Sprint,
            MovementState::TacSprint => PlayerStateName::TacSprint,
            MovementState::Slide => PlayerStateName::Slide,
            MovementState::Jump => PlayerStateName::Jump,
            MovementState::Fall => PlayerStateName::Fall,
            MovementState::Mantle => PlayerStateName::Mantle,
            MovementState::Vault => PlayerStateName::Vault,
        }
    }
}

/// `player:state`. `index.js:112-116` plus the three fields `_publishState`
/// adds (`tacticalSprint`, `adsProgress`, `healthFraction`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerStateEvent {
    pub stance: Stance,
    pub sprinting: bool,
    pub tactical_sprint: bool,
    pub sliding: bool,
    pub ads: bool,
    pub ads_progress: f64,
    pub state: PlayerStateName,
    pub grounded: bool,
    pub airborne: bool,
    pub mantling: bool,
    pub lean: f64,
    pub speed: f64,
    pub health: f64,
    pub health_fraction: f64,
    pub crouched: bool,
}

impl Default for PlayerStateEvent {
    fn default() -> Self {
        PlayerStateEvent {
            stance: Stance::Stand,
            sprinting: false,
            tactical_sprint: false,
            sliding: false,
            ads: false,
            ads_progress: 0.0,
            state: PlayerStateName::Stand,
            grounded: true,
            airborne: false,
            mantling: false,
            lean: 0.0,
            speed: 0.0,
            health: HEALTH.max,
            health_fraction: 1.0,
            crouched: false,
        }
    }
}

/// `player:land`. `index.js:117`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerLandEvent {
    pub velocity: f64,
    pub surface: Surface,
    pub position: Vec3,
}

/// `player:footstep`. `index.js:118-121`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerFootstepEvent {
    pub position: Vec3,
    pub surface: Surface,
    pub running: bool,
    pub left: bool,
    pub speed: f64,
    pub stance: Stance,
}

/// `player:jump`. `index.js:123`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerJumpEvent {
    pub position: Vec3,
}

/// `player:mantle`. `index.js:122`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerMantleEvent {
    pub kind: LedgeKind,
    pub height: f64,
}

/// `damage:dealt`, as this facade reads it (`index.js:415-424`).
///
/// The source's `t !== this && t !== 'player' && t?.isPlayer !== true` test is
/// a duck-typed identity check on a JavaScript object reference; the emitter
/// decides it here, exactly as `crate::audio::system::DamageDealt` does.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DamageDealtEvent {
    pub target_is_player: bool,
    /// `e.amount ?? 0`.
    pub amount: Option<f64>,
    /// `e.from` — the *shooter*.
    pub from: Option<Vec3>,
    /// `e.source?.position`.
    pub source_position: Option<Vec3>,
    /// `e.point` — where the round landed, i.e. the player. Last resort only:
    /// using it first pinned every direction arc to dead ahead.
    pub point: Option<Vec3>,
}

/// `explosion`, as this facade reads it (`index.js:426-440`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ExplosionEvent {
    pub position: Vec3,
    /// `e.radius ?? 5`.
    pub radius: Option<f64>,
    /// `e.damage ?? 90`.
    pub damage: Option<f64>,
}

/// `bullet:impact`, as this facade reads it (`index.js:442-455`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BulletImpactEvent {
    pub point: Vec3,
}

/// `getHudState()`'s preallocated snapshot. `index.js:125-129`. The shape is
/// fixed by the contract at the top of `src/ui/index.js`;
/// [`crate::ui::PlayerPull`] is the consumer's view of the same object, and
/// the app translates between them (`suppression`/`dead` are on this side
/// only, `armour` on that side only).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerHudState {
    pub health: f64,
    pub max_health: f64,
    pub regen: bool,
    pub dead: bool,
    /// 0..1 against tactical sprint — `ui` uses this as the reticle-bloom
    /// weight.
    pub move_amount: f64,
    pub sprint: bool,
    pub crouch: bool,
    pub ads: bool,
    pub airborne: bool,
    pub suppression: f64,
    pub position: Vec3,
}

/// `get stats()`. `index.js:725-738`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerStats {
    pub state: PlayerStateName,
    pub stance: Stance,
    pub speed: f64,
    pub vertical: f64,
    pub grounded: bool,
    pub lean: f64,
    pub fov: f64,
    pub health: f64,
    pub suppression: f64,
}

/// `debugState(name)`'s return. `index.js:718-721`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DebugStateReport {
    pub state: PlayerStateName,
    pub stance: Stance,
    pub speed: f64,
    pub health: f64,
    pub ads: f64,
}

/// `debugState(name)`'s argument. `index.js:673-716`. The source's `default:
/// break;` arm — an unrecognised name changes nothing and still reports — has
/// no counterpart: an unnamed state is not expressible as a variant, and the
/// only caller is a dev overlay picking from this list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugState {
    Sprint,
    TacSprint,
    Crouch,
    Prone,
    Slide,
    Air,
    Hurt,
    Critical,
    Reset,
}

/// `rot` in `teleport(eyeOrPos, rot)`. `index.js:643-648` — the source accepts
/// a bare yaw in radians, a `THREE.Euler`, or anything with a `.y`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TeleportRotation {
    /// `rot` absent or falsy: neither yaw nor pitch is touched.
    #[default]
    Keep,
    /// `typeof rot === 'number'`: yaw only, pitch untouched.
    Yaw(f64),
    /// An object: `yaw ?? this.movement.yaw`, and `clamp(pitch ?? 0, ±limit)`
    /// — note the pitch default is **0**, not "keep", so an object with no `x`
    /// levels the view.
    Euler { pitch: Option<f64>, yaw: Option<f64> },
}

/* ==================================================================== */
/* The core                                                             */
/* ==================================================================== */

/// `physics.createCharacter({...})`'s options at `movement.js:141-149`. The
/// source builds the controller inside `Movement.init`; this port's
/// `Movement::init` takes an already-built one (the physics seam's job), so
/// the option block lands here, at the facade that owns the physics handle.
pub const PLAYER_CHARACTER: CharacterOpts = CharacterOpts {
    radius: 0.32,
    height: STAND.height,
    step_height: STAND.step_height,
    slope_limit: 48.0 * (std::f64::consts::PI / 180.0),
    snap_distance: 0.34,
    mask: mask::CHARACTER,
    max_iterations: 5,
};

/// `this._prev`. `index.js:133-136` — the last emitted discrete state,
/// compared field-wise so no string is built.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PrevDiscrete {
    /// `''` in the source: no state has been emitted yet, so the next publish
    /// always fires.
    state: Option<PlayerStateName>,
    stance: Option<Stance>,
    sprinting: bool,
    tactical_sprint: bool,
    sliding: bool,
    grounded: bool,
    ads: bool,
    mantling: bool,
}

impl Default for PrevDiscrete {
    fn default() -> Self {
        PrevDiscrete {
            state: None,
            stance: None,
            sprinting: false,
            tactical_sprint: false,
            sliding: false,
            grounded: true,
            ads: false,
            mantling: false,
        }
    }
}

/// `class PlayerSystem`'s state. `index.js:89-751`.
pub struct PlayerCore {
    pub movement: Movement,
    pub rig: CameraRig,
    pub health: Health,
    pub low_health_pass: Option<LowHealthPass>,
    pub hitbox: Option<PlayerHitbox>,
    pub camera: PlayerCamera,

    pub control_enabled: bool,
    pub ads_amount: f64,
    ads_external: bool,
    ads_external_age: f64,
    pub ads_requested: bool,

    look_frame: Option<u64>,
    prev_yaw: f64,

    state_payload: PlayerStateEvent,
    prev: PrevDiscrete,

    /// Where the player was put at `init`. The facts `index.js:192-196`'s
    /// `console.info` banner printed, as a value.
    pub spawn: Spawn,

    /// `this.rng = ctx.rng.fork()` (`index.js:147`). **Never read anywhere in
    /// `index.js`** — but the fork draws one `u32` from the root stream, and
    /// dropping it would shift every value every later subsystem draws. Dead
    /// computation in the source is still part of the source.
    pub rng: Rng,

    physics: Option<Rc<dyn PlayerPhysics>>,
    spawns: Option<Rc<dyn SpawnSource>>,

    /// The cached `ctx` scalars — see the module doc comment's seam 5.
    events: EventBus,
    config: Config,
    time: Time,
}

impl PlayerCore {
    /// `this.isPlayer = true` (`index.js:94`) — lets `ai`/`physics` recognise
    /// the local player from an owner pointer.
    pub const IS_PLAYER: bool = true;

    /// `constructor()`. `index.js:93-138`.
    pub fn new(config: Config) -> Self {
        PlayerCore {
            movement: Movement::new(),
            rig: CameraRig::new(config.fov),
            health: Health::new(),
            low_health_pass: None,
            hitbox: None,
            camera: PlayerCamera::new(config.fov),

            control_enabled: true,
            ads_amount: 0.0,
            ads_external: false,
            ads_external_age: 0.0,
            ads_requested: false,

            look_frame: None,
            prev_yaw: 0.0,

            state_payload: PlayerStateEvent::default(),
            prev: PrevDiscrete::default(),

            spawn: Spawn::default(),
            rng: Rng::new(0),

            physics: None,
            spawns: None,

            events: EventBus::new(),
            config,
            time: Time::default(),
        }
    }

    /* ==================================================================== */
    /* init                                                                 */
    /* ==================================================================== */

    /// `init(ctx)`. `index.js:144-197`, minus the event wiring (which is
    /// [`PlayerSystem::wire_events`], the same split
    /// `crate::audio::system::AudioSystem` uses) and the `console.info`
    /// banner.
    pub fn init(
        &mut self,
        physics: Rc<dyn PlayerPhysics>,
        spawns: Option<Rc<dyn SpawnSource>>,
        rng: Rng,
        events: EventBus,
        config: Config,
        time: Time,
    ) {
        self.physics = Some(Rc::clone(&physics));
        self.spawns = spawns;
        self.rng = rng;
        self.events = events;
        self.config = config;
        self.time = time;

        // ---- spawn -----------------------------------------------------------
        let spawn = self.resolve_spawn();
        self.spawn = spawn;
        self.movement
            .init(physics.create_player_character(), Some(spawn.position));
        self.movement.yaw = spawn.yaw;
        self.movement.pitch = 0.0;
        self.prev_yaw = spawn.yaw;
        self.rig.reset(STAND.eye);
        let health_view = self.health_view();
        let (cfg, t) = (self.config, self.time);
        self.rig
            .update(1.0 / 60.0, &mut self.movement, health_view, &cfg, &t);
        self.camera.apply_rig(&mut self.rig);

        // ---- hitbox ----------------------------------------------------------
        self.hitbox = Some(PlayerHitbox::new());
        self.sync_hitbox();
    }

    /// `const render = ctx.peek('render'); if (render?.registerPass) { ... }`.
    /// `index.js:180-184`. The pass is created only when something can draw
    /// it; handing it to a render pipeline (and holding the source's
    /// `_unregisterPass`) is the registrar's business, not the player's.
    pub fn install_low_health_pass(&mut self) {
        self.low_health_pass = Some(LowHealthPass::new());
    }

    /// Refresh the cached `ctx` scalars.
    ///
    /// The source's `ctx.time`/`ctx.config` are **live objects**:
    /// `this.ctx.time.elapsed` read inside `teleport`, `debugState` or an
    /// event handler always sees the clock the engine has already advanced for
    /// this frame. These are copies, so the frame driver must hand them over
    /// at the top of each frame — *before* emitting into the bus or calling an
    /// external API like [`PlayerCore::teleport`] — and `fixed_update` /
    /// `update` refresh them again on the way in. Getting this wrong shifts
    /// `_lookFrame` and `lastDamageTime` by one frame.
    pub fn set_ctx(&mut self, time: Time, config: Config) {
        self.time = time;
        self.config = config;
    }

    /// `_resolveSpawn()`. `index.js:199-211`.
    fn resolve_spawn(&self) -> Spawn {
        let mut out = Spawn {
            position: [0.0, 0.2, 0.0],
            yaw: 0.0,
        };
        if let Some(sp) = self.spawns.as_ref().and_then(|w| w.spawn(0)) {
            out.position = sp.position;
            out.yaw = sp.yaw;
        }
        // Physics owns the exact floor; drop onto it so we never start embedded.
        // `Number.isFinite(gy)` is `Option::is_some` here — see
        // `PhysicsWorld::ground_height`'s doc comment.
        let gy = self.physics.as_ref().and_then(|p| {
            p.ground_height(out.position[0], out.position[2], out.position[1] + 6.0)
        });
        out.position[1] = match gy {
            Some(gy) => gy + 0.03,
            None => out.position[1] + 0.2,
        };
        out
    }

    /* ==================================================================== */
    /* look                                                                 */
    /* ==================================================================== */

    /// `_consumeLook(dt)`. `index.js:223-260`.
    ///
    /// Mouse/stick look is consumed once per rendered frame. It happens in the
    /// first fixed step when there is one (so movement uses this frame's yaw
    /// with zero latency) and in `update()` otherwise — above 120 fps a frame
    /// can contain no fixed step at all and dropping the delta there would
    /// feel like a hitch.
    fn consume_look(&mut self, dt: f64, input: &dyn PlayerLook) {
        let frame = self.time.frame;
        if self.look_frame == Some(frame) {
            return;
        }
        self.look_frame = Some(frame);
        if !self.control_enabled {
            self.movement.yaw_rate = 0.0;
            return;
        }
        let sens = lerp(1.0, self.config.ads_sens_scale, clamp01(self.ads_amount));

        let (look_x, look_y) = input.look();
        let mut d_yaw = -look_x * sens;
        let mut d_pitch = -look_y * sens;

        // Gamepad: rate-based, already curved by Input. `if (stick.lookX ||
        // stick.lookY)` is JavaScript truthiness on two numbers — nonzero.
        let (stick_x, stick_y) = input.stick_look();
        if stick_x != 0.0 || stick_y != 0.0 {
            let rate = 3.1 * sens; // rad/s at full deflection
            d_yaw -= stick_x * rate * dt;
            d_pitch -= stick_y * rate * dt;
        }
        // Mantles are rooted: you keep your head, but the shoulders are committed.
        if self.movement.mantle_motion.active {
            d_yaw *= 0.55;
            d_pitch *= 0.55;
        }

        self.movement.yaw += d_yaw;
        self.movement.pitch = clamp(
            self.movement.pitch + d_pitch,
            -CAMERA.pitch_limit,
            CAMERA.pitch_limit,
        );
        // Keep yaw bounded so long sessions never lose float precision.
        if self.movement.yaw > std::f64::consts::PI {
            self.movement.yaw -= std::f64::consts::PI * 2.0;
        } else if self.movement.yaw < -std::f64::consts::PI {
            self.movement.yaw += std::f64::consts::PI * 2.0;
        }

        self.movement.yaw_rate = if dt > 1e-5 { d_yaw / dt } else { 0.0 };
        self.prev_yaw = self.movement.yaw;
    }

    /* ==================================================================== */
    /* frame                                                                */
    /* ==================================================================== */

    /// `fixedUpdate(h, ctx)`. `index.js:266-273`. The source's `if
    /// (!this.movement) return;` guard is structural in Rust: `Movement` is
    /// always constructed, and a `Movement` with no controller already
    /// no-ops in `step`.
    pub fn fixed_update(&mut self, time: &Time, config: &Config, input: &dyn PlayerLook) {
        self.set_ctx(*time, *config);
        let h = time.fixed;
        self.consume_look(if time.dt > 1e-5 { time.dt } else { h }, input);
        let t = self.time;
        self.movement.latch_input(&t, input.as_player_input());
        if !self.control_enabled {
            return;
        }
        self.movement.ads_amount = self.ads_amount;
        let world = self.physics.as_deref().map(|p| p.as_world_probe());
        self.movement.step(&t, world);
    }

    /// `update(dt, ctx)`. `index.js:275-291`.
    pub fn update(&mut self, dt: f64, time: &Time, config: &Config, input: &dyn PlayerLook) {
        self.set_ctx(*time, *config);
        self.consume_look(dt, input);
        let t = self.time;
        self.movement.latch_input(&t, input.as_player_input());

        self.update_ads(dt, input);
        self.drain_movement_events();
        let now = self.time.elapsed;
        self.health
            .update(dt, now, &mut self.rig, &self.events);

        let health_view = self.health_view();
        let cfg = self.config;
        self.rig
            .update(dt, &mut self.movement, health_view, &cfg, &t);
        if self.control_enabled {
            self.camera.apply_rig(&mut self.rig);
        } else {
            // The camera is being driven by somebody else (a cutscene, the
            // shot harness); read our forward back off it rather than writing.
            self.rig.forward = apply_quaternion([0.0, 0.0, -1.0], self.camera.quaternion);
        }

        if let Some(pass) = self.low_health_pass.as_mut() {
            pass.sync(&self.health);
        }
        self.sync_hitbox();
        self.publish_state();
    }

    /// `health` as `camera.js` reads it.
    fn health_view(&self) -> HealthView {
        HealthView {
            fraction: self.health.fraction(),
            suppression: self.health.suppression,
        }
    }

    /// `_syncHitbox()`. `index.js:293-302`. Keeps the AI-facing hitbox on the
    /// interpolated capsule.
    fn sync_hitbox(&mut self) {
        let p = self.movement.render_position;
        let r = 0.3;
        let h = self.movement.stance.def().height;
        let dead = self.health.dead;
        if let Some(hb) = self.hitbox.as_mut() {
            hb.set_segment(p[0], p[1] + r, p[2], p[0], p[1] + r.max(h - r), p[2], r);
            hb.enabled = !dead;
        }
    }

    /// `_updateAds(dt)`. `index.js:304-319`.
    fn update_ads(&mut self, dt: f64, input: &dyn PlayerLook) {
        self.ads_requested = self.control_enabled
            && input.ads()
            && !self.movement.mantle_motion.active
            && !self.movement.sliding
            && !self.health.dead;

        if self.ads_external {
            // `weapons` is driving the blend; stop trusting it if it goes quiet.
            self.ads_external_age += dt;
            if self.ads_external_age > 0.6 {
                self.ads_external = false;
            }
        }
        if !self.ads_external {
            self.ads_amount = approach(
                self.ads_amount,
                if self.ads_requested { 1.0 } else { 0.0 },
                0.075,
                dt,
            );
        }
        self.movement.ads_amount = self.ads_amount;
    }

    /// `_drainMovementEvents()`. `index.js:321-374`. Turns the movement
    /// machine's one-shot flags into events + camera impulses.
    fn drain_movement_events(&mut self) {
        if self.movement.land_event.pending {
            self.movement.land_event.pending = false;
            let speed = self.movement.land_event.speed;
            let mag = self.rig.on_land(speed);
            let payload = PlayerLandEvent {
                velocity: speed,
                surface: self.movement.land_event.surface,
                position: self.movement.position,
            };
            self.events.emit("player:land", &payload);
            // Fall damage — CoD only hurts you past a real drop.
            let l = &CAMERA.land;
            if speed > l.damage_speed {
                let amount = (speed - l.damage_speed) * l.damage_per_speed;
                let now = self.time.elapsed;
                let camera_position = self.camera.position;
                let camera_yaw = self.camera.rotation.yaw;
                self.health.damage(
                    amount,
                    None,
                    DamageOpts {
                        yaw: None,
                        kind: DamageKind::Fall,
                    },
                    &mut self.rig,
                    camera_position,
                    camera_yaw,
                    now,
                    &self.events,
                );
            }
            if mag > 0.35 {
                self.movement.set_foot_hold(FOOTSTEP.land_hold);
            }
        }

        if self.movement.step_event.pending {
            self.movement.step_event.pending = false;
            let e = PlayerFootstepEvent {
                position: [
                    self.movement.step_event.x,
                    self.movement.step_event.y,
                    self.movement.step_event.z,
                ],
                surface: self.movement.step_event.surface,
                running: self.movement.step_event.running,
                left: self.movement.step_event.left,
                speed: self.movement.horizontal_speed,
                stance: self.movement.stance,
            };
            self.rig.on_footstep(e.running, e.stance);
            self.events.emit("player:footstep", &e);
        }

        if self.movement.jumped {
            self.movement.jumped = false;
            self.rig.add_recoil(-0.35 * DEG, 0.0, 0.0, 0.004);
            let payload = PlayerJumpEvent {
                position: self.movement.position,
            };
            self.events.emit("player:jump", &payload);
        }

        if self.movement.slide_started {
            self.movement.slide_started = false;
            self.rig.on_slide_start(self.movement.slide_side);
        }
        if self.movement.slide_ended {
            self.movement.slide_ended = false;
        }

        if self.movement.mantle_event.pending {
            self.movement.mantle_event.pending = false;
            let payload = PlayerMantleEvent {
                kind: self.movement.mantle_event.kind,
                height: self.movement.mantle_event.height,
            };
            self.rig.add_trauma(if payload.kind == LedgeKind::Vault {
                0.08
            } else {
                0.14
            });
            self.events.emit("player:mantle", &payload);
        }
    }

    /// `_publishState()`. `index.js:376-409`.
    fn publish_state(&mut self) {
        let m = &self.movement;
        let leaning = m.lean_amount.abs() > 0.35;
        let state = if leaning
            && (m.state == MovementState::Stand || m.state == MovementState::Crouch)
        {
            PlayerStateName::Lean
        } else {
            PlayerStateName::from(m.state)
        };
        let s = PlayerStateEvent {
            state,
            stance: m.stance,
            crouched: m.stance != Stance::Stand,
            sprinting: m.sprinting,
            tactical_sprint: m.tactical_sprint,
            sliding: m.sliding,
            ads: self.ads_amount > 0.5,
            ads_progress: self.ads_amount,
            grounded: m.grounded,
            airborne: !m.grounded,
            mantling: m.mantle_motion.active,
            lean: m.lean_amount,
            speed: m.horizontal_speed,
            health: self.health.value,
            health_fraction: self.health.fraction(),
        };
        self.state_payload = s;
        // Emit only when something discrete actually changed. Field-wise
        // compare, because building a key string every frame would be a
        // per-frame allocation.
        let q = self.prev;
        if q.state != Some(s.state)
            || q.stance != Some(s.stance)
            || q.sprinting != s.sprinting
            || q.tactical_sprint != s.tactical_sprint
            || q.sliding != s.sliding
            || q.grounded != s.grounded
            || q.ads != s.ads
            || q.mantling != s.mantling
        {
            self.prev = PrevDiscrete {
                state: Some(s.state),
                stance: Some(s.stance),
                sprinting: s.sprinting,
                tactical_sprint: s.tactical_sprint,
                sliding: s.sliding,
                grounded: s.grounded,
                ads: s.ads,
                mantling: s.mantling,
            };
            self.events.emit("player:state", &s);
        }
    }

    /* ==================================================================== */
    /* incoming damage                                                      */
    /* ==================================================================== */

    /// `_onDamageDealt(e)`. `index.js:415-424`.
    pub fn on_damage_dealt(&mut self, e: &DamageDealtEvent) {
        if !e.target_is_player {
            return;
        }
        // Direction indicators need the *shooter*, not the impact point: `ai`
        // sets `point` to where the round landed (which is the player), and
        // `from` to the muzzle. Using `point` pinned every arc to dead ahead.
        let from = e.from.or(e.source_position).or(e.point);
        self.apply_damage(
            e.amount.unwrap_or(0.0),
            from,
            DamageOpts {
                yaw: None,
                kind: DamageKind::Bullet,
            },
        );
    }

    /// `_onExplosion(e)`. `index.js:426-440`.
    pub fn on_explosion(&mut self, e: &ExplosionEvent) {
        let eye = self.camera.position;
        let r = e.radius.unwrap_or(5.0);
        // `Vector3.distanceTo` — sqrt of the squared distance, not `hypot`.
        let dx = e.position[0] - eye[0];
        let dy = e.position[1] - eye[1];
        let dz = e.position[2] - eye[2];
        let d = (dx * dx + dy * dy + dz * dz).sqrt();
        if d > r * 1.6 {
            return;
        }
        // Occluded blasts still shake you, they just do not wound you.
        let clear = self
            .physics
            .as_ref()
            .is_some_and(|p| p.line_of_sight(e.position, eye, mask::EXPLOSION));
        let falloff = clamp01(1.0 - d / r).powf(1.6);
        self.rig.add_trauma(clamp01(falloff * 1.4));
        self.health
            .add_suppression(HEALTH.suppression.per_explosion * falloff);
        if clear && falloff > 0.02 {
            self.apply_damage(
                e.damage.unwrap_or(90.0) * falloff,
                Some(e.position),
                DamageOpts {
                    yaw: None,
                    kind: DamageKind::Explosion,
                },
            );
        }
    }

    /// `_onBulletImpact(e)`. `index.js:442-455`.
    pub fn on_bullet_impact(&mut self, e: &BulletImpactEvent) {
        if self.health.dead {
            return;
        }
        let eye = self.camera.position;
        let dx = e.point[0] - eye[0];
        let dy = e.point[1] - eye[1];
        let dz = e.point[2] - eye[2];
        let d2 = dx * dx + dy * dy + dz * dz;
        let big_r = HEALTH.suppression.radius;
        if d2 > big_r * big_r {
            return;
        }
        // Heuristic: rounds we fired land where we are looking. Anything
        // cracking in beside or behind us is somebody shooting at us.
        //
        // `Math.sqrt(d2) || 1e-4` is JavaScript truthiness: zero (and NaN)
        // fall through to the epsilon.
        let s = d2.sqrt();
        let d = if s == 0.0 || s.is_nan() { 1e-4 } else { s };
        let f = self.rig.forward;
        if (dx * f[0] + dy * f[1] + dz * f[2]) / d > 0.55 {
            return;
        }
        self.health
            .add_suppression(HEALTH.suppression.per_near_miss * (1.0 - d / big_r));
    }

    /* ==================================================================== */
    /* public API                                                           */
    /* ==================================================================== */

    /// `getHudState()`. `index.js:465-483`. Polled by `ui` every
    /// `late_update`.
    pub fn hud_state(&self) -> PlayerHudState {
        let m = &self.movement;
        let hp = &self.health;
        PlayerHudState {
            health: hp.value,
            max_health: hp.max,
            regen: hp.regenerating,
            dead: hp.dead,
            suppression: hp.suppression,
            move_amount: 1.0f64.min(m.horizontal_speed / MOVE.tac_sprint_speed),
            sprint: m.sprinting || m.tactical_sprint,
            crouch: m.stance == Stance::Crouch || m.stance == Stance::Prone,
            ads: self.ads_amount > 0.5,
            airborne: !m.grounded,
            position: self.position(),
        }
    }

    /// `get position()` — FEET (bottom of the capsule), interpolated.
    pub fn position(&self) -> Vec3 {
        self.movement.render_position
    }
    /// `get feetPosition()` — the un-interpolated simulation position.
    pub fn feet_position(&self) -> Vec3 {
        self.movement.position
    }
    /// `get eyePosition()`.
    pub fn eye_position(&self) -> Vec3 {
        self.rig.eye_position
    }
    pub fn velocity(&self) -> Vec3 {
        self.movement.velocity
    }
    /// `get forward()` — unit view forward.
    pub fn forward(&self) -> Vec3 {
        self.rig.forward
    }
    pub fn yaw(&self) -> f64 {
        self.movement.yaw
    }
    pub fn pitch(&self) -> f64 {
        self.movement.pitch
    }
    pub fn speed(&self) -> f64 {
        self.movement.speed
    }
    pub fn horizontal_speed(&self) -> f64 {
        self.movement.horizontal_speed
    }
    /// `get state()` — the *published* state, so it carries `lean`.
    pub fn state(&self) -> PlayerStateName {
        self.state_payload.state
    }
    pub fn stance(&self) -> Stance {
        self.movement.stance
    }
    pub fn sprinting(&self) -> bool {
        self.movement.sprinting
    }
    pub fn tactical_sprint(&self) -> bool {
        self.movement.tactical_sprint
    }
    pub fn sliding(&self) -> bool {
        self.movement.sliding
    }
    pub fn slide_progress(&self) -> f64 {
        self.movement.slide_progress()
    }
    pub fn grounded(&self) -> bool {
        self.movement.grounded
    }
    pub fn airborne(&self) -> bool {
        !self.movement.grounded
    }
    pub fn mantling(&self) -> bool {
        self.movement.mantle_motion.active
    }
    pub fn lean_amount(&self) -> f64 {
        self.movement.lean_amount
    }
    pub fn eye_height(&self) -> f64 {
        self.rig.eye
    }
    pub fn ads_progress(&self) -> f64 {
        self.ads_amount
    }
    pub fn view_kick(&self) -> ViewKick {
        self.rig.view_kick
    }
    pub fn camera_rig(&self) -> &CameraRig {
        &self.rig
    }
    /// `get height()` — capsule height of the current stance.
    pub fn height(&self) -> f64 {
        self.movement.stance.def().height
    }
    pub fn max_health(&self) -> f64 {
        self.health.max
    }
    pub fn health_fraction(&self) -> f64 {
        self.health.fraction()
    }
    pub fn low_health(&self) -> bool {
        self.health.low()
    }
    pub fn dead(&self) -> bool {
        self.health.dead
    }
    pub fn suppression(&self) -> f64 {
        self.health.suppression
    }
    pub fn damage_indicators(&self) -> &[DamageIndicator] {
        &self.health.indicators
    }
    pub fn heartbeat_pulse(&self) -> f64 {
        self.health.pulse
    }
    pub fn bob_phase(&self) -> f64 {
        self.rig.bob_phase()
    }

    /// `setAdsProgress(v)`. `index.js:586-591`. `weapons` owns the ADS curve;
    /// hand it over and everything else follows.
    pub fn set_ads_progress(&mut self, v: f64) {
        self.ads_amount = clamp01(v);
        self.ads_external = true;
        self.ads_external_age = 0.0;
        self.movement.ads_amount = self.ads_amount;
    }

    /// `addRecoil(pitch, yaw, roll, punch)`. `index.js:593-595`.
    pub fn add_recoil(&mut self, pitch: f64, yaw: f64, roll: f64, punch: f64) {
        self.rig.add_recoil(pitch, yaw, roll, punch);
    }
    /// `addKick(pitch, yaw, roll)`. `index.js:596-598`.
    pub fn add_kick(&mut self, pitch: f64, yaw: f64, roll: f64) {
        self.rig.add_kick(pitch, yaw, roll);
    }
    /// `addTrauma(a)`. `index.js:599-601`.
    pub fn add_trauma(&mut self, a: f64) {
        self.rig.add_trauma(a);
    }
    /// `addCameraShake(a)`. `index.js:603-605` — an alias some subsystems
    /// reach for.
    pub fn add_camera_shake(&mut self, a: f64) {
        self.rig.add_trauma(a);
    }

    /// `applyDamage(amount, from, opts)`. `index.js:607-609`.
    ///
    /// The source spreads `opts` *after* `{ yaw: this.movement.yaw }`, so an
    /// explicit `opts.yaw` wins and everything else falls back to the movement
    /// yaw — which is what `opts.yaw.or(...)` says here.
    pub fn apply_damage(&mut self, amount: f64, from: Option<Vec3>, opts: DamageOpts) -> f64 {
        let opts = DamageOpts {
            yaw: opts.yaw.or(Some(self.movement.yaw)),
            kind: opts.kind,
        };
        let now = self.time.elapsed;
        let camera_position = self.camera.position;
        let camera_yaw = self.camera.rotation.yaw;
        self.health.damage(
            amount,
            from,
            opts,
            &mut self.rig,
            camera_position,
            camera_yaw,
            now,
            &self.events,
        )
    }

    /// `heal(a)`. `index.js:610-612`.
    pub fn heal(&mut self, a: f64) {
        self.health.heal(a);
    }
    /// `addSuppression(a)`. `index.js:613-615`.
    pub fn add_suppression(&mut self, a: f64) {
        self.health.add_suppression(a);
    }

    /// `setControlEnabled(on)`. `index.js:617-632`. For the shot harness and
    /// cutscenes.
    pub fn set_control_enabled(&mut self, on: bool) {
        self.control_enabled = on;
        self.movement.control_enabled = on;
        if !on {
            // `latchInput(-2)` — a frame number no real frame equals, so the
            // held keys are flushed and the next real latch always runs.
            self.movement.flush_latched_input();
            self.movement.velocity = [0.0, 0.0, 0.0];
            self.movement.sprinting = false;
            self.movement.tactical_sprint = false;
            self.movement.sliding = false;
            self.movement.cancel_mantle();
            self.ads_amount = 0.0;
            self.ads_external = false;
        } else {
            self.movement.invalidate_cmd_frame();
        }
    }

    /// `teleport(eyeOrPos, rot)`. `index.js:639-655`.
    ///
    /// `eye` is the **EYE** position — that is what the shot harness hands us,
    /// since it passes the camera transform.
    pub fn teleport(&mut self, eye: Vec3, rot: TeleportRotation) {
        let eye_h = STAND.eye;
        let feet_y = eye[1] - eye_h;
        match rot {
            TeleportRotation::Keep => {}
            TeleportRotation::Yaw(yaw) => self.movement.yaw = yaw,
            TeleportRotation::Euler { pitch, yaw } => {
                self.movement.yaw = yaw.unwrap_or(self.movement.yaw);
                self.movement.pitch = clamp(
                    pitch.unwrap_or(0.0),
                    -CAMERA.pitch_limit,
                    CAMERA.pitch_limit,
                );
            }
        }
        self.movement.teleport(eye[0], feet_y, eye[2]);
        self.rig.reset(eye_h);
        self.rig.eye_position = eye;
        self.rig.fov = self.config.fov;
        self.look_frame = Some(self.time.frame);
        self.prev.state = None;
    }

    /// `respawn(index = 0)`. `index.js:657-668`.
    pub fn respawn(&mut self, index: i64) {
        let sp = self.spawns.as_ref().and_then(|w| w.spawn(index));
        self.health.reset(true);
        let Some(sp) = sp else {
            return;
        };
        let gy = self
            .physics
            .as_ref()
            .and_then(|p| p.ground_height(sp.position[0], sp.position[2], sp.position[1] + 6.0));
        let feet_y = match gy {
            Some(gy) => gy + 0.03,
            None => sp.position[1],
        };
        self.movement.yaw = sp.yaw;
        self.movement.pitch = 0.0;
        self.movement.teleport(sp.position[0], feet_y, sp.position[2]);
        self.rig.reset(STAND.eye);
    }

    /// `debugState(name)`. `index.js:671-722`. Named states for dev overlays
    /// and future shots.
    pub fn debug_state(&mut self, name: DebugState) -> DebugStateReport {
        match name {
            DebugState::Sprint => {
                self.movement.stance_want = Stance::Stand;
                self.movement.sprinting = true;
                let yaw = self.movement.yaw;
                self.movement.velocity = [
                    -yaw.sin() * MOVE.sprint_speed,
                    0.0,
                    -yaw.cos() * MOVE.sprint_speed,
                ];
            }
            DebugState::TacSprint => {
                self.movement.sprinting = true;
                self.movement.tactical_sprint = true;
            }
            DebugState::Crouch => self.movement.stance_want = Stance::Crouch,
            DebugState::Prone => self.movement.stance_want = Stance::Prone,
            DebugState::Slide => {
                self.movement.sprinting = true;
                let yaw = self.movement.yaw;
                self.movement.velocity = [
                    -yaw.sin() * MOVE.sprint_speed,
                    0.0,
                    -yaw.cos() * MOVE.sprint_speed,
                ];
                self.movement.debug_begin_slide(
                    [-yaw.sin(), 0.0, -yaw.cos()],
                    1.0,
                    MOVE.sprint_speed,
                );
                self.movement.slide_started = false;
                self.rig.on_slide_start(1.0);
            }
            DebugState::Air => {
                self.movement.velocity[1] = *JUMP_SPEED;
                self.movement.grounded = false;
            }
            DebugState::Hurt => {
                self.health.value = self.health.max * 0.28;
                self.health.last_damage_time = self.time.elapsed;
                self.health.effect =
                    clamp01((HEALTH.low_threshold - 0.28) / HEALTH.low_threshold);
            }
            DebugState::Critical => {
                self.health.value = self.health.max * 0.11;
                self.health.last_damage_time = self.time.elapsed;
                self.health.effect = 1.0;
                self.health.hit_flash = 0.6;
            }
            DebugState::Reset => {
                self.health.reset(true);
                self.health.effect = 0.0;
            }
        }
        DebugStateReport {
            state: self.state(),
            stance: self.movement.stance,
            speed: self.movement.horizontal_speed,
            health: self.health.value,
            ads: self.ads_amount,
        }
    }

    /// `get stats()`. `index.js:725-738` — a snapshot for the dev HUD.
    pub fn stats(&self) -> PlayerStats {
        PlayerStats {
            state: self.state(),
            stance: self.movement.stance,
            speed: self.movement.horizontal_speed,
            vertical: self.movement.velocity[1],
            grounded: self.movement.grounded,
            lean: self.movement.lean_amount,
            fov: self.rig.fov,
            health: self.health.value,
            suppression: self.health.suppression,
        }
    }

    /// `dispose()`. `index.js:740-751`. Unsubscribing is
    /// [`PlayerSystem::dispose`]'s job (it owns the subscription ids);
    /// removing the collider and unregistering the pass have no registry to
    /// talk to yet, so both are dropped here.
    pub fn dispose(&mut self) {
        self.hitbox = None;
        if let Some(pass) = self.low_health_pass.as_mut() {
            pass.dispose();
        }
        self.low_health_pass = None;
        self.movement.dispose();
    }
}

/* ==================================================================== */
/* The Subsystem wrapper                                                */
/* ==================================================================== */

/// The registered subsystem. `static id = 'player'`,
/// `static deps = ['physics', 'world', 'render']`.
///
/// The core lives behind an `Rc<RefCell<…>>` because the three incoming-damage
/// handlers mutate it while the frame loop is already stepping systems — the
/// same shape, for the same reason, as `crate::audio::system::AudioSystem`.
pub struct PlayerSystem {
    core: Rc<RefCell<PlayerCore>>,
    input: Rc<RefCell<Input>>,
    physics: Rc<dyn PlayerPhysics>,
    spawns: Option<Rc<dyn SpawnSource>>,
    offs: Vec<(&'static str, SubscriptionId)>,
}

impl PlayerSystem {
    /// The three seams `Ctx` cannot supply: the collision world, the spawn
    /// table, and the input snapshot. Everything else comes from `ctx` in
    /// [`Subsystem::init`].
    pub fn new(
        config: Config,
        physics: Rc<dyn PlayerPhysics>,
        spawns: Option<Rc<dyn SpawnSource>>,
        input: Rc<RefCell<Input>>,
    ) -> Self {
        PlayerSystem {
            core: Rc::new(RefCell::new(PlayerCore::new(config))),
            input,
            physics,
            spawns,
            offs: Vec::new(),
        }
    }

    /// The shared guts, for a caller that needs the player by concrete type.
    pub fn core(&self) -> Rc<RefCell<PlayerCore>> {
        Rc::clone(&self.core)
    }

    /// `index.js:186-190`'s three subscriptions.
    pub fn wire_events(&mut self, ctx: &Ctx<'_>) {
        macro_rules! on {
            ($name:literal, $payload:ty, $method:ident) => {{
                let core = Rc::clone(&self.core);
                let id = ctx.events.on($name, move |p: &dyn Any| {
                    if let Some(p) = p.downcast_ref::<$payload>() {
                        core.borrow_mut().$method(p);
                    }
                    Ok(())
                });
                self.offs.push(($name, id));
            }};
        }
        on!("damage:dealt", DamageDealtEvent, on_damage_dealt);
        on!("explosion", ExplosionEvent, on_explosion);
        on!("bullet:impact", BulletImpactEvent, on_bullet_impact);
    }
}

impl Subsystem for PlayerSystem {
    fn id(&self) -> &'static str {
        "player"
    }

    fn deps(&self) -> &'static [&'static str] {
        &["physics", "world", "render"]
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::FixedUpdate, Phase::Update]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn init(&mut self, ctx: &Ctx<'_>) -> Result<(), CoreError> {
        let rng = ctx.rng.borrow_mut().fork();
        self.core.borrow_mut().init(
            Rc::clone(&self.physics),
            self.spawns.clone(),
            rng,
            ctx.events.clone(),
            *ctx.config,
            *ctx.time,
        );
        // `const render = ctx.peek('render'); if (render?.registerPass) { … }`
        // — the pass exists only when something can draw it.
        if ctx.peek("render").is_some() {
            self.core.borrow_mut().install_low_health_pass();
        }
        self.wire_events(ctx);
        Ok(())
    }

    fn fixed_update(&mut self, _h: Seconds, ctx: &Ctx<'_>) {
        let input = self.input.borrow();
        self.core
            .borrow_mut()
            .fixed_update(ctx.time, ctx.config, &*input);
    }

    /// The `dt` handed on is `ctx.time.dt`, **not** the `Seconds` argument:
    /// `Time::dt_seconds` narrows the clock to `f32`, and the source's
    /// `update(dt, ctx)` is given the `f64`. A narrowed `dt` changes every
    /// spring's integration and every `approach` in the rig.
    fn update(&mut self, _dt: Seconds, ctx: &Ctx<'_>) {
        let input = self.input.borrow();
        self.core
            .borrow_mut()
            .update(ctx.time.dt, ctx.time, ctx.config, &*input);
    }

    fn dispose(&mut self) {
        self.offs.clear();
        self.core.borrow_mut().dispose();
    }
}
