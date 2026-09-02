//! The weapons subsystem facade — the firing machine.
//!
//! Ported from Claude-of-Duty `src/weapons/index.js:1-843` — the whole file.
//!
//! This is what turns the already-ported viewmodel rig, clips, ballistics,
//! recoil patterns and weapon models into a gun you can pull a trigger on: it
//! owns the ammunition model, the fire-mode state machine, the recoil-pattern
//! index, the spread cone, the reload/inspect/switch choreography, the shell
//! ejection queue, the dropped-magazine pool, and the four events the rest of
//! the game listens to.
//!
//! ```text
//! PUBLIC API   const wp = ctx.get('weapons')
//! WEAPON     current weapon_ids set_weapon next_weapon set_weapon_immediate
//! AMMO       ammo reload cycle_fire_mode fire_mode
//! FIRE       can_fire try_fire firing spread_degrees ads_progress
//! CLIPS      reloading inspecting switching inspect
//! RIG        viewmodel muzzle_world bore_dir eject_world eject_velocity
//! HUD        hud_state
//! DEBUG      debug_pose stats
//! ```
//!
//! ## Events
//!
//! Emitted: `weapon:fire`, `weapon:shell`, `weapon:reload`. `bullet:tracer`
//! comes out of [`crate::weapons::ballistics`] (physics owns penetration, so
//! `bullet:impact` is not this file's either).
//!
//! **The payload types are the ones `crate::audio::system` already declares**
//! — [`crate::audio::system::WeaponFire`], [`WeaponReload`][crate::audio::
//! system::WeaponReload] and [`WeaponShell`][crate::audio::system::WeaponShell].
//! [`crate::events::EventBus`] dispatches a `&dyn Any` payload that each
//! handler downcasts, so *two structs for one event name means only one
//! subsystem ever sees an emit*. `crate::ui::system` declares its own,
//! differently-shaped `WeaponFire`/`WeaponReload` and says so in a comment;
//! adding a third set here would fork the vocabulary again, so this facade
//! reuses the richest existing one instead. What that costs is recorded
//! precisely, because it is a real gap and not a rounding of the contract:
//!
//! | source payload field | nearest existing struct field | status |
//! |---|---|---|
//! | `weapon:fire.weapon` | `WeaponFire::weapon` (a name) | carried, as the id |
//! | `weapon:fire.origin` | `WeaponFire::origin` | carried |
//! | `weapon:fire.dir` | — | **missing**; read it off [`WeaponCore::fire_dir`] |
//! | `weapon:fire.seed` | — | **missing**; [`WeaponCore::fire_seed`] |
//! | `weapon:shell.position` | `WeaponShell::position` | carried |
//! | `weapon:shell.velocity` | — | **missing**; [`WeaponCore::shell_payload`] |
//! | `weapon:shell.caseLen/caseRadius/spin` | — | **missing**; same accessor |
//! | `weapon:reload.weapon/phase` | `WeaponReload::{weapon, phase}` | carried |
//!
//! Every missing field is still *computed*, in the source's order and with the
//! source's RNG draws, and exposed as a value on [`WeaponCore`]; nothing is
//! dropped from the simulation. What is missing is only the shared vocabulary
//! to carry it across the bus, and converging that is the integration pass's
//! job (`ui::system` names the same decision).
//!
//! ## The seams this port had to name
//!
//! 1. **`ctx.camera`.** `tryFire` reads the camera quaternion for the aim
//!    basis, and the viewmodel anchor tracks the camera's world transform.
//!    `crate::weapons::viewmodel::ViewCamera` already carries the orientation;
//!    [`FireCamera`] extends it with the position, which the world-space
//!    muzzle/eject queries need.
//! 2. **`viewmodel.muzzleWorld` / `ejectWorld` / `ejectVelocity` / `boreDir`.**
//!    `viewmodel.rs` deliberately did not port these four
//!    (`viewmodel.js:1041-1071`): each reads `w.group.matrixWorld`, whose
//!    value is a function of the render loop's scene-graph walk, and that
//!    loop did not exist when the rig landed. It exists now — as
//!    [`WeaponCore::sync_anchor`] — so the four live here, on the first
//!    caller that genuinely needs them, and should move down to `viewmodel.rs`
//!    the moment a camera lands there. See "the one-frame anchor lag" below.
//! 3. **`ctx.peek('player')`.** [`WeaponPlayer`] names exactly the ten members
//!    `index.js` reads or calls. Every one is optional in the source
//!    (`index.js:170`: "all optional: the viewmodel works standalone"), so the
//!    whole trait object is an `Option`.
//! 4. **`ctx.peek('physics')`.** [`WeaponPhysics`] is `spawnDebris` +
//!    `removeRigidBody` (`_dropMagazine`) on top of
//!    [`RaycastWorld`][crate::weapons::ballistics::RaycastWorld] (which
//!    `ballistics.rs` already defined for the same physics facade).
//!    `src/physics/index.js` is a different slice; when it lands it implements
//!    this trait and the binding is done.
//! 5. **`ctx.input`.** [`WeaponInput`] is the six members `update` reads;
//!    [`crate::input::Input`] implements it directly.
//! 6. **`ctx.scene` / `THREE.Object3D` magazine proxies.** `_magProxy`
//!    (`index.js:540-572`) builds two reusable scene-graph groups per weapon
//!    that share the viewmodel's magazine meshes. There is no scene graph
//!    here, so [`MagProxy`] is the same pool as *data* — a transform, a
//!    visibility flag, an expiry and the physics body handle. That is
//!    everything `_dropMagazine` and `lateUpdate`'s retirement pass actually
//!    read; the mesh sharing is a renderer concern.
//!
//! ## The one-frame anchor lag is real, and it is preserved
//!
//! `viewmodel.update` writes `anchor.position`/`anchor.quaternion` from the
//! camera but never composes them into `anchor.matrixWorld`; the **renderer**
//! does that, in its scene walk, after every `lateUpdate`. So `muzzleWorld`
//! reads the anchor pose that was baked at the end of the *previous* frame,
//! composed against whatever the rig pose is right now. A shot fired in
//! `update` therefore uses frame N-1's anchor and frame N-1's rig; the
//! `weapon:fire` payload assembled in `lateUpdate` uses frame N-1's anchor and
//! frame N's rig. [`WeaponCore::sync_anchor`] is that render walk, spelled
//! out, and the host calls it once per frame after `late_update`.
//!
//! ## Not ported
//!
//! * **`console.info`** (`index.js:181-184`) — a build banner. The facts it
//!   prints are [`WeaponStats::tris`] and the weapon count.
//! * **`resize()`** (`index.js:830`) — empty in the source.
//! * **`_onClipEvent` dispatch timing.** The source's `onClipEvent` callback
//!   fires *inside* `viewmodel.update`, between the clip sample and the pose
//!   compose. `viewmodel.rs` queues the beats instead and the caller drains
//!   them after `update` returns (its own module doc explains why a `&mut
//!   self` callback field would infect every signature). Same beats, same
//!   order, same frame — but the *effects* of a beat land after the compose
//!   rather than before it. That is invisible for `boltrelease` (it only
//!   reaches `_updateParts`, whose output is a part transform) and for
//!   `magout`/`magin`/`start`/`end` on a reload. It is visible for exactly one
//!   thing: the `end` beat of a `holster` clip swaps the active weapon, so the
//!   frame the swap lands on composes the *outgoing* weapon's pose here and
//!   the *incoming* weapon's pose in the source. One frame, on a weapon
//!   switch. `tests/weapons_system_port.rs` names it rather than hiding it.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;


use crate::audio::foley::ReloadPhase;
use crate::audio::system::{WeaponFire, WeaponReload, WeaponShell};
use crate::engine::{Ctx, Time};
use crate::error::CoreError;
use crate::events::{EventBus, SubscriptionId};
use crate::input::{Action, Input};
use crate::registry::{Phase, Subsystem};
use crate::rng::Rng;
use crate::weapons::ballistics::{
    ProjectileSim, RaycastWorld, SpawnParams, Vec3 as BVec3,
};
use crate::weapons::clips::{build_clips, AttachNodes, Clip, Clips, GripNode, PosNode};
use crate::weapons::defs::{build_recoil_pattern, WeaponDef, PISTOL, RIFLE, SMG};
use crate::weapons::hands::HandPoseName;
use crate::weapons::materials::ENV_OCCLUSION;
use crate::weapons::mathx::{lerp, DEG};
use crate::weapons::models::pistol::{build_pistol, PistolModel};
use crate::weapons::models::rifle::{build_rifle, RifleModel};
use crate::weapons::models::smg::{build_smg, SmgModel};
use crate::weapons::models::GripTarget;
use crate::weapons::rig_math::{M4, Q, V3};
use crate::weapons::viewmodel::{
    FrameInput, OpticNode, ViewCamera, Viewmodel, WeaponRig,
};

/* ==================================================================== */
/* Seams                                                                */
/* ==================================================================== */

/// `ctx.camera`, narrowed to the two facts this facade reads: the world
/// orientation (which the viewmodel anchor copies, and which `tryFire` builds
/// the aim basis from) and the world position (which the anchor also copies,
/// and which the world-space muzzle/eject queries need).
///
/// [`ViewCamera`] already carries the orientation for
/// [`Viewmodel::update`]; this extends it rather than declaring a second,
/// competing camera contract.
pub trait FireCamera: ViewCamera {
    /// `camera.getWorldPosition(...)` / `setFromMatrixPosition(cam.matrixWorld)`.
    fn position(&self) -> V3;

    /// `cam.quaternion`, which `tryFire` reads directly (`index.js:373`) to
    /// build the aim basis.
    ///
    /// **This is not [`ViewCamera::orientation`].** That one is the *anchor's*
    /// quaternion, and the source derives it as
    /// `anchor.quaternion.setFromRotationMatrix(cam.matrixWorld)`
    /// (`viewmodel.js:641`) — a round trip out through the world matrix and
    /// back through the trace method, which does not return the camera's own
    /// quaternion bit-for-bit. Conflating the two compiles and is wrong in the
    /// last bits of every shot direction, which then compounds through the
    /// projectile integrator. An implementation whose camera is authored as a
    /// quaternion in the first place may return the same value for both.
    fn aim_orientation(&self) -> Q;
    /// [`Viewmodel::update`] wants the supertrait object, and `&dyn
    /// FireCamera` does not coerce to `&dyn ViewCamera` without trait
    /// upcasting, so implementors — always `Sized` — hand it over. Every
    /// implementation is `{ self }`. Mirrors
    /// `crate::player::system::PlayerLook::as_player_input`.
    fn as_view_camera(&self) -> &dyn ViewCamera;
}

/// A camera pose held as plain values, for a test or for an app that already
/// has the numbers. Mirrors `crate::weapons::viewmodel::FixedOrientation`.
#[derive(Debug, Clone, Copy)]
pub struct FixedCamera {
    pub position: V3,
    pub orientation: Q,
}

impl ViewCamera for FixedCamera {
    fn orientation(&self) -> Q {
        self.orientation
    }
}

impl FireCamera for FixedCamera {
    fn position(&self) -> V3 {
        self.position
    }

    /// A camera authored as a single quaternion has nothing to re-derive, so
    /// the aim and anchor orientations coincide here. See
    /// [`FireCamera::aim_orientation`].
    fn aim_orientation(&self) -> Q {
        self.orientation
    }

    fn as_view_camera(&self) -> &dyn ViewCamera {
        self
    }
}

/// `ctx.peek('player')` — the ten members `index.js` reads. Every one is
/// optional in the source, and so is the whole object.
pub trait WeaponPlayer {
    /// `p.addRecoil(pitch, yaw, roll, punch)` (`index.js:407`). The source
    /// guards on `p?.addRecoil` being present; a player that does not model
    /// recoil implements this as a no-op.
    fn add_recoil(&mut self, pitch: f64, yaw: f64, roll: f64, punch: f64);
    /// `player.velocity` (`index.js:520`, `:699`). `None` is the source's
    /// falsy `pv`.
    fn velocity(&self) -> Option<V3>;
    /// `player?.adsRequested === true` (`index.js:601`).
    fn ads_requested(&self) -> bool;
    /// `player?.sprinting === true` (`index.js:602`).
    fn sprinting(&self) -> bool;
    /// `player?.horizontalSpeed ?? player?.speed ?? 0` (`index.js:603`),
    /// already collapsed by the implementor — the two names are the same
    /// quantity under two spellings.
    fn horizontal_speed(&self) -> f64;
    /// `player?.stance` (`index.js:604`, `:665`).
    fn stance(&self) -> Option<Stance>;
    /// `player?.airborne === true` (`index.js:605`).
    fn airborne(&self) -> bool;
    /// `player?.state === 'mantle'` (`index.js:606`).
    fn state_is_mantle(&self) -> bool;
    /// `player?.mantling === true` (`index.js:606`).
    fn mantling(&self) -> bool;
    /// `player?.setAdsProgress?.(t)` (`index.js:629`).
    fn set_ads_progress(&mut self, t: f64);
}

/// `player.stance`, the three values `index.js` compares against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stance {
    Stand,
    Crouch,
    Prone,
}

/// `phys.spawnDebris(...)`'s option object (`index.js:526-532`), minus
/// `object3D` — there is no scene graph here (see the module doc's seam 6).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DebrisOpts {
    pub size: f64,
    /// `'rubber'`, the surface the source names.
    pub surface: crate::world::palette::Surface,
    pub mass: f64,
    pub lifetime: f64,
    pub restitution: f64,
}

/// The handle `phys.spawnDebris` returns and `phys.removeRigidBody` takes.
/// The source stores whatever object physics hands back; the port keeps an
/// opaque id so the pool can hold one without naming physics' internals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DebrisHandle(pub u64);

/// `ctx.peek('physics')`, narrowed to what the weapons facade asks of it: the
/// two rigid-body calls `_dropMagazine`/`lateUpdate` make, on top of the
/// bullet-cast pair [`RaycastWorld`] already declares for the same facade.
pub trait WeaponPhysics: RaycastWorld {
    /// `phys.spawnDebris(position, velocity, opts)` (`index.js:525-532`).
    /// `None` is the source's `phys?.spawnDebris` being absent.
    fn spawn_debris(
        &mut self,
        position: V3,
        velocity: V3,
        opts: DebrisOpts,
    ) -> Option<DebrisHandle>;

    /// `phys.removeRigidBody(body)` (`index.js:569`, `:717`).
    fn remove_rigid_body(&mut self, body: DebrisHandle);

    /// [`ProjectileSim`] wants the supertrait object; see
    /// [`FireCamera::as_view_camera`] for why implementors hand it over.
    /// Every implementation is `{ self }`.
    fn as_raycast_world(&mut self) -> &mut dyn RaycastWorld;
}

/// `ctx.input`, narrowed to the six members `update` reads
/// (`index.js:600-622`). [`Input`] implements it directly.
pub trait WeaponInput {
    fn frozen(&self) -> bool;
    /// `input.enabled !== false` is the source's test, so the default is
    /// "enabled".
    fn enabled(&self) -> bool;
    fn ads(&self) -> bool;
    fn fire(&self) -> bool;
    fn fire_pressed(&self) -> bool;
    /// `input.wheel` — truthy scrolls to the next weapon.
    fn wheel(&self) -> f64;
    fn pressed(&self, code: &str) -> bool;
    fn action_pressed(&self, action: Action) -> bool;
}

impl WeaponInput for Input {
    fn frozen(&self) -> bool {
        self.frozen
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn ads(&self) -> bool {
        Input::ads(self)
    }

    fn fire(&self) -> bool {
        Input::fire(self)
    }

    fn fire_pressed(&self) -> bool {
        Input::fire_pressed(self)
    }

    fn wheel(&self) -> f64 {
        self.wheel
    }

    fn pressed(&self, code: &str) -> bool {
        Input::pressed(self, code)
    }

    fn action_pressed(&self, action: Action) -> bool {
        Input::action_pressed(self, action)
    }
}

/* ==================================================================== */
/* Value types                                                          */
/* ==================================================================== */

/// `wp.ammo` (`index.js:203-217`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ammo {
    /// Rounds in the magazine **plus** the chambered one.
    pub mag: u32,
    /// Rounds in the magazine alone.
    pub in_mag: u32,
    pub chambered: bool,
    pub reserve: u32,
    pub mag_size: u32,
    pub total: u32,
    pub empty: bool,
}

/// `wp.stats` (`index.js:180`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WeaponStats {
    /// Viewmodel triangles across all three weapons.
    pub tris: usize,
    pub draw_calls: usize,
    pub live: u32,
    pub fired: u32,
}

/// `this._state` (`index.js:111-120`) — the gathered per-frame facts handed to
/// [`Viewmodel::update`]. Carries all eight fields the source's object has,
/// even though [`FrameInput`] names only six: `crouch` and `empty` are read by
/// `_restSpread` and by the auto-reload test, and the source stores them on
/// the same object.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct WeaponFrameState {
    pub ads: bool,
    pub sprint: bool,
    pub low_ready: bool,
    pub speed: f64,
    pub crouch: bool,
    pub airborne: bool,
    pub trigger: bool,
    pub empty: bool,
}

impl WeaponFrameState {
    /// The six fields [`Viewmodel::update`] reads.
    pub fn as_frame_input(self) -> FrameInput {
        FrameInput {
            ads: self.ads,
            sprint: self.sprint,
            low_ready: self.low_ready,
            speed: self.speed,
            airborne: self.airborne,
            trigger: self.trigger,
        }
    }
}

/// `this._hudState` (`index.js:122-125`), the preallocated snapshot
/// `getHudState` mutates and `ui` polls.
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponHudState {
    pub name: String,
    pub mode: &'static str,
    pub ammo: u32,
    pub reserve: u32,
    pub mag_size: u32,
    pub reloading: bool,
    pub reload_progress: f64,
    pub ads: bool,
    pub spread: f64,
    pub firing: bool,
}

impl Default for WeaponHudState {
    fn default() -> Self {
        WeaponHudState {
            name: String::new(),
            mode: "auto",
            ammo: 0,
            reserve: 0,
            mag_size: 0,
            reloading: false,
            reload_progress: 0.0,
            ads: false,
            spread: 0.0,
            firing: false,
        }
    }
}

/// The full `weapon:fire` payload the source assembles (`index.js:89`), kept
/// as facade state because two of its four fields have no home in the shared
/// event vocabulary — see the module doc's table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FirePayload {
    pub weapon: Option<&'static str>,
    pub origin: V3,
    pub dir: V3,
    pub seed: u32,
}

impl Default for FirePayload {
    fn default() -> Self {
        FirePayload {
            weapon: None,
            origin: V3::ZERO,
            dir: V3::ZERO,
            seed: 0,
        }
    }
}

/// The full `weapon:shell` payload (`index.js:94-101`). The source's comment
/// is worth keeping: it carries the real case dimensions and a spin "so fx can
/// size and tumble the brass instead of guessing: a 9x19 case is less than
/// half the length of a 5.56x45 one".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShellPayload {
    pub position: V3,
    pub velocity: V3,
    pub weapon: Option<&'static str>,
    pub case_len: f64,
    pub case_radius: f64,
    pub spin: f64,
}

impl Default for ShellPayload {
    fn default() -> Self {
        ShellPayload {
            position: V3::ZERO,
            velocity: V3::ZERO,
            weapon: None,
            case_len: 0.0446,
            case_radius: 0.00495,
            spin: 0.0,
        }
    }
}

/// One slot of `this._shellQueue` (`index.js:107-109`). `t < 0` is free.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ShellSlot {
    pub t: f64,
}

/// The rig transform and moving-part poses **as the scene graph last saw
/// them**, which is what a clip beat dispatched from inside
/// `viewmodel.update` reads.
///
/// The source fires `magdrop` between the clip sample and the pose compose, so
/// `_dropMagazine`'s `mag.updateMatrixWorld()` sees `parts.magazine`'s local
/// transform from the PREVIOUS frame (`_updateParts` has not run yet) composed
/// against `group.matrixWorld` from the previous *render* walk (`rig.
/// updateMatrixWorld(true)` has not run yet either). Both are one frame stale,
/// and during a reload the magazine is travelling fast enough for that to be
/// 1.2 cm. [`WeaponCore::late_update`] snapshots them before stepping the rig
/// and hands them to the beat, which is that ordering spelled out.
#[derive(Debug, Clone, Copy)]
struct PreStepPose {
    rig_pos: V3,
    rig_quat: Q,
    parts: crate::weapons::viewmodel::PartsState,
}

/// One reusable dropped-magazine prop (`index.js:561`'s
/// `{ group, body, until }`), as data — see the module doc's seam 6.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MagProxy {
    pub position: V3,
    pub quaternion: Q,
    pub visible: bool,
    pub body: Option<DebrisHandle>,
    pub until: f64,
}

impl Default for MagProxy {
    fn default() -> Self {
        MagProxy {
            position: V3::ZERO,
            quaternion: Q::IDENTITY,
            visible: false,
            body: None,
            until: 0.0,
        }
    }
}

/// One weapon's mutable run-time state (`this.states`' value,
/// `index.js:157-165`).
#[derive(Debug, Clone)]
pub struct WeaponState {
    pub def: &'static WeaponDef,
    /// `def.cycleTime = 60 / def.rpm` (`index.js:153`). The source copies the
    /// def and adds this field; `WeaponDef` here is a `const` with no such
    /// field, so it lives beside the def where the copy would have put it.
    pub cycle_time: f64,
    /// `buildRecoilPattern(def, Rng)` — `[pitch, yaw]` per shot, `f32` because
    /// the source writes a `Float32Array`.
    pub pattern: Vec<[f32; 2]>,
    pub mag: u32,
    pub chambered: bool,
    pub reserve: u32,
    pub mode: &'static str,
    pub mode_index: usize,
}

/// Everything `addWeapon` returns that this facade keeps: the rig
/// [`Viewmodel::set_active`] takes, the clips [`Viewmodel::play`] takes, the
/// magazine length and the cartridge dimensions.
#[derive(Debug, Clone)]
pub struct WeaponEntry {
    pub id: &'static str,
    pub rig: WeaponRig,
    pub clips: Clips,
    /// `w.magLen` (`index.js:515`).
    pub mag_len: f64,
    /// `w.shell.caseLen` (`index.js:704`).
    pub case_len: f64,
    /// `w.shell.rimR` (`index.js:705`).
    pub rim_r: f64,
    /// The triangle count the source's `addWeapon` accumulates while building
    /// meshes. There are no meshes here, so it is summed off the same merged
    /// geometry buckets `Assembly::build` produces — which is exactly what the
    /// source counts (`triCount(geo)` per material bucket).
    pub tris: usize,
}

/// The five clip names `play` is called with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClipName {
    ReloadTac,
    ReloadEmpty,
    Inspect,
    Draw,
    Holster,
}

impl ClipName {
    fn pick(self, clips: &Clips) -> Clip {
        match self {
            ClipName::ReloadTac => clips.reload_tac.clone(),
            ClipName::ReloadEmpty => clips.reload_empty.clone(),
            ClipName::Inspect => clips.inspect.clone(),
            ClipName::Draw => clips.draw.clone(),
            ClipName::Holster => clips.holster.clone(),
        }
    }
}

/// `debugPose(kind)` (`index.js:734`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugPose {
    Idle,
    Ads,
    Fire,
}

/* ==================================================================== */
/* Model -> rig                                                         */
/* ==================================================================== */

fn v3f(a: [f32; 3]) -> V3 {
    V3::new(f64::from(a[0]), f64::from(a[1]), f64::from(a[2]))
}

fn grip_node(g: GripTarget) -> GripNode {
    GripNode {
        pos: [
            f64::from(g.pos[0]),
            f64::from(g.pos[1]),
            f64::from(g.pos[2]),
        ],
        finger: Some([
            f64::from(g.finger[0]),
            f64::from(g.finger[1]),
            f64::from(g.finger[2]),
        ]),
        back: Some([
            f64::from(g.back[0]),
            f64::from(g.back[1]),
            f64::from(g.back[2]),
        ]),
    }
}

/// `addWeapon`'s node half (`viewmodel.js:405-434`) for the **smg**.
///
/// `crate::weapons::viewmodel::WeaponRig::from_rifle` is the rifle's; its doc
/// says "the smg and pistol get their own converters when a consumer needs
/// them", and this facade is that consumer. They live here rather than in
/// `viewmodel.rs` only because this slice may not edit that file; they belong
/// next to `from_rifle` and should move there.
pub fn rig_from_smg(model: &SmgModel, def: &'static WeaponDef) -> WeaponRig {
    let n = &model.nodes;
    WeaponRig {
        id: model.id,
        def,
        sight: v3f(n.sight),
        iron_sight: v3f(n.iron_sight),
        muzzle: v3f(n.muzzle),
        eject: v3f(n.eject),
        eject_dir: v3f(n.eject_dir).normalize_or_zero(),
        optic: Some(OpticNode {
            center: [
                f64::from(n.optic_glass.center[0]),
                f64::from(n.optic_glass.center[1]),
                f64::from(n.optic_glass.center[2]),
            ],
            aperture_r: f64::from(n.optic_glass.aperture_r),
        }),
        mag_seat_pos: v3f(n.mag_seat.pos),
        mag_seat_quat: Q::from_euler_xyz(
            f64::from(n.mag_seat.rot[0]),
            f64::from(n.mag_seat.rot[1]),
            f64::from(n.mag_seat.rot[2]),
        ),
        grip_r: grip_node(n.grip_r),
        grip_l: grip_node(n.grip_l),
        charge_pull: v3f(n.charge_pull),
        bolt_travel: v3f(n.bolt_travel),
        // No slide; `nodes.slideTravel ?? [0,0,0]`.
        slide_travel: V3::ZERO,
        trigger_pull: f64::from(n.trigger_pull),
        mag_len: f64::from(model.mag_size.len),
        // `lhandPose: model.id === 'pistol' ? 'cup' : 'clamp'`
        // (`viewmodel.js:433`). `_fitSupportHand` refines this to
        // `clamp:<id>` for a weapon with a handguard the fingertips can reach;
        // the smg's does not fit (the capture records a bare `clamp`), so the
        // authored value is also the fitted one here.
        lhand_pose: HandPoseName::Clamp,
        bolt_rest: Some(v3f(n.bolt_rest.pos)),
        slide_rest: None,
        charge_rest: Some(v3f(n.charge_rest.pos)),
        has_trigger: true,
        has_selector: true,
        has_magazine: true,
    }
}

/// `addWeapon`'s node half for the **pistol**. See [`rig_from_smg`].
pub fn rig_from_pistol(model: &PistolModel, def: &'static WeaponDef) -> WeaponRig {
    let n = &model.nodes;
    WeaponRig {
        id: model.id,
        def,
        sight: v3f(n.sight),
        iron_sight: v3f(n.iron_sight),
        muzzle: v3f(n.muzzle),
        eject: v3f(n.eject),
        eject_dir: v3f(n.eject_dir).normalize_or_zero(),
        // The pistol has iron sights only; `model.nodes.opticGlass ?? null`.
        optic: None,
        mag_seat_pos: v3f(n.mag_seat.pos),
        mag_seat_quat: Q::from_euler_xyz(
            f64::from(n.mag_seat.rot[0]),
            f64::from(n.mag_seat.rot[1]),
            f64::from(n.mag_seat.rot[2]),
        ),
        grip_r: grip_node(n.grip_r),
        grip_l: grip_node(n.grip_l),
        // No charging handle and no separate bolt; both `?? [0,0,0]`.
        charge_pull: V3::ZERO,
        bolt_travel: V3::ZERO,
        slide_travel: v3f(n.slide_travel),
        trigger_pull: f64::from(n.trigger_pull),
        mag_len: f64::from(model.mag_size.len),
        lhand_pose: HandPoseName::Cup,
        bolt_rest: None,
        slide_rest: Some(v3f(n.slide_rest.pos)),
        charge_rest: None,
        has_trigger: true,
        // The pistol model has no selector switch.
        has_selector: false,
        has_magazine: true,
    }
}

/* ==================================================================== */
/* The core                                                             */
/* ==================================================================== */

/// `class WeaponSystem`'s state (`index.js:61-843`).
pub struct WeaponCore {
    pub viewmodel: Viewmodel,
    pub sim: ProjectileSim,
    /// `this.states` — insertion-ordered, because `weaponIds` is
    /// `[...this.states.keys()]` and `nextWeapon` indexes into it.
    states: Vec<(&'static str, WeaponState)>,
    entries: Vec<(&'static str, WeaponEntry)>,
    pub active_id: &'static str,
    pub debug_mode: Option<DebugPose>,

    fire_timer: f64,
    burst_left: u32,
    burst_cooldown: f64,
    /// `this._semiLatch` (`index.js:76`) — **assigned in the constructor and
    /// never read anywhere in `index.js`**. Dead state in the source is still
    /// part of the source.
    semi_latch: bool,
    spread: f64,
    shot_index: usize,
    since_shot: f64,
    switch_timer: f64,
    switch_to: Option<&'static str>,
    /// `this._reloadPhase` (`index.js:81`) — likewise assigned once and never
    /// read.
    reload_phase: Option<ReloadPhase>,

    fire_payload: FirePayload,
    shell_payload: ShellPayload,
    pending_shots: u32,
    pending_first: bool,
    fire_seed: u32,
    pending_reload_empty: bool,

    shell_queue: [ShellSlot; 8],
    /// `this._magPools` (`index.js:541`), keyed by weapon id — two proxies
    /// each, created lazily on the first drop.
    mag_pools: Vec<(&'static str, [usize; 2])>,
    /// `this._droppedMags` (`index.js:110`) — every proxy ever created, in
    /// creation order. The pools index into this.
    dropped_mags: Vec<MagProxy>,

    state: WeaponFrameState,
    hud_state: WeaponHudState,
    pub stats: WeaponStats,

    /// `this.rng = ctx.rng.fork()` (`index.js:134`).
    rng: Rng,

    /// The viewmodel anchor's composed pose, as of the last render walk. See
    /// the module doc's "one-frame anchor lag".
    anchor_pos: V3,
    anchor_quat: Q,

    /// `debugPose`'s scripted fire frames (`index.js:801`) and its frame
    /// counter (`index.js:817`).
    script_frames: Option<Vec<i64>>,
    debug_frame: i64,

    /// The cached `ctx` scalars, refreshed at the top of every step — the
    /// same shape `crate::player::system::PlayerCore` uses, for the same
    /// reason (`this.ctx` inside a method is not available to a `Fn` handler).
    events: EventBus,
    time: Time,
}

impl WeaponCore {
    /// `constructor()`. `index.js:65-126`.
    pub fn new(rng: &mut Rng) -> Self {
        // `this.rng = ctx.rng.fork()` FIRST (`index.js:134`), then
        // `new Viewmodel(ctx, mats)` forks `ctx.rng` again
        // (`viewmodel.js:104`) — two forks off the ROOT stream, in that order.
        // Forking the viewmodel off `this.rng` instead would give it a
        // different sequence and shift every `addRecoil` jitter.
        let own = rng.fork();
        let viewmodel = Viewmodel::new(rng);
        WeaponCore {
            viewmodel,
            sim: ProjectileSim::new(),
            states: Vec::new(),
            entries: Vec::new(),
            active_id: "rifle",
            debug_mode: None,

            fire_timer: 0.0,
            burst_left: 0,
            burst_cooldown: 0.0,
            semi_latch: false,
            spread: 0.0,
            shot_index: 0,
            since_shot: 10.0,
            switch_timer: 0.0,
            switch_to: None,
            reload_phase: None,

            fire_payload: FirePayload::default(),
            shell_payload: ShellPayload::default(),
            pending_shots: 0,
            pending_first: false,
            fire_seed: 0,
            pending_reload_empty: false,

            shell_queue: [ShellSlot { t: -1.0 }; 8],
            mag_pools: Vec::new(),
            dropped_mags: Vec::new(),

            state: WeaponFrameState::default(),
            hud_state: WeaponHudState::default(),
            stats: WeaponStats::default(),

            rng: own,

            anchor_pos: V3::ZERO,
            anchor_quat: Q::IDENTITY,

            script_frames: None,
            debug_frame: 0,

            events: EventBus::new(),
            time: Time::default(),
        }
    }

    /// `init(ctx)`. `index.js:132-185`, minus the `console.info` banner and
    /// the two event subscriptions (which are [`WeaponSystem::wire_events`] —
    /// the split `crate::audio::system` established).
    ///
    /// `ctx.viewScene.environmentIntensity = ENV_OCCLUSION` (`index.js:145`)
    /// has no scene to write to here; the value is returned so a renderer can
    /// apply it, and [`WeaponCore::env_intensity`] reads it back.
    pub fn init(&mut self, events: EventBus, time: Time) {
        self.events = events;
        self.time = time;

        let mut tris = 0usize;
        // `for (const id of ['rifle', 'smg', 'pistol'])` (`index.js:151`).
        let rifle = build_rifle();
        let smg = build_smg();
        let pistol = build_pistol();

        let add = |id: &'static str,
                       def: &'static WeaponDef,
                       rig: WeaponRig,
                       nodes: AttachNodes,
                       mag_len: f64,
                       case_len: f32,
                       rim_r: f32,
                       model_tris: usize,
                       states: &mut Vec<(&'static str, WeaponState)>,
                       entries: &mut Vec<(&'static str, WeaponEntry)>| {
            let clips = build_clips(&nodes, def);
            entries.push((
                id,
                WeaponEntry {
                    id,
                    rig,
                    clips,
                    mag_len,
                    case_len: f64::from(case_len),
                    rim_r: f64::from(rim_r),
                    tris: model_tris,
                },
            ));
            states.push((
                id,
                WeaponState {
                    def,
                    // `def.cycleTime = 60 / def.rpm` (`index.js:153`).
                    cycle_time: 60.0 / def.rpm,
                    pattern: build_recoil_pattern(&def.recoil),
                    mag: def.mag_size,
                    chambered: true,
                    reserve: def.reserve,
                    mode: def.modes[0],
                    mode_index: 0,
                },
            ));
        };

        let mut rifle_model = rifle;
        let rifle_tris = assembly_tris(&mut rifle_model);
        tris += rifle_tris;
        add(
            "rifle",
            &RIFLE,
            WeaponRig::from_rifle(&rifle_model, &RIFLE),
            AttachNodes {
                grip_l: grip_node(rifle_model.nodes.grip_l),
                mag_seat: PosNode {
                    pos: [
                        f64::from(rifle_model.nodes.mag_seat.pos[0]),
                        f64::from(rifle_model.nodes.mag_seat.pos[1]),
                        f64::from(rifle_model.nodes.mag_seat.pos[2]),
                    ],
                },
                charge_rest: Some(PosNode {
                    pos: [
                        f64::from(rifle_model.nodes.charge_rest.pos[0]),
                        f64::from(rifle_model.nodes.charge_rest.pos[1]),
                        f64::from(rifle_model.nodes.charge_rest.pos[2]),
                    ],
                }),
            },
            f64::from(rifle_model.mag_size.len),
            rifle_model.shell.case_len,
            rifle_model.shell.rim_r,
            rifle_tris,
            &mut self.states,
            &mut self.entries,
        );

        let mut smg_model = smg;
        let smg_tris = assembly_tris_smg(&mut smg_model);
        tris += smg_tris;
        add(
            "smg",
            &SMG,
            rig_from_smg(&smg_model, &SMG),
            AttachNodes {
                grip_l: grip_node(smg_model.nodes.grip_l),
                mag_seat: PosNode {
                    pos: [
                        f64::from(smg_model.nodes.mag_seat.pos[0]),
                        f64::from(smg_model.nodes.mag_seat.pos[1]),
                        f64::from(smg_model.nodes.mag_seat.pos[2]),
                    ],
                },
                charge_rest: Some(PosNode {
                    pos: [
                        f64::from(smg_model.nodes.charge_rest.pos[0]),
                        f64::from(smg_model.nodes.charge_rest.pos[1]),
                        f64::from(smg_model.nodes.charge_rest.pos[2]),
                    ],
                }),
            },
            f64::from(smg_model.mag_size.len),
            smg_model.shell.case_len,
            smg_model.shell.rim_r,
            smg_tris,
            &mut self.states,
            &mut self.entries,
        );

        let mut pistol_model = pistol;
        let pistol_tris = assembly_tris_pistol(&mut pistol_model);
        tris += pistol_tris;
        add(
            "pistol",
            &PISTOL,
            rig_from_pistol(&pistol_model, &PISTOL),
            AttachNodes {
                grip_l: grip_node(pistol_model.nodes.grip_l),
                mag_seat: PosNode {
                    pos: [
                        f64::from(pistol_model.nodes.mag_seat.pos[0]),
                        f64::from(pistol_model.nodes.mag_seat.pos[1]),
                        f64::from(pistol_model.nodes.mag_seat.pos[2]),
                    ],
                },
                // The pistol has no charging handle: `nodes.chargeRest` is
                // absent, which is the branch `build_clips` takes for the
                // slide-rack reload (`clips.js:209-216`).
                charge_rest: None,
            },
            f64::from(pistol_model.mag_size.len),
            pistol_model.shell.case_len,
            pistol_model.shell.rim_r,
            pistol_tris,
            &mut self.states,
            &mut self.entries,
        );

        let active = self.active_id;
        let rig = self.entry(active).rig.clone();
        self.viewmodel.set_active(rig);
        self.play(ClipName::Draw);

        self.stats = WeaponStats {
            tris,
            draw_calls: 0,
            live: 0,
            fired: 0,
        };
    }

    /// `ctx.viewScene.environmentIntensity` (`index.js:145`) — see [`init`].
    ///
    /// [`init`]: WeaponCore::init
    pub fn env_intensity(&self) -> f64 {
        ENV_OCCLUSION
    }

    /* ================================================================ */
    /* public getters (`index.js:191-283`)                              */
    /* ================================================================ */

    fn state_of(&self, id: &str) -> &WeaponState {
        &self
            .states
            .iter()
            .find(|(k, _)| *k == id)
            .expect("every registered weapon has a state")
            .1
    }

    fn state_mut(&mut self) -> &mut WeaponState {
        let id = self.active_id;
        &mut self
            .states
            .iter_mut()
            .find(|(k, _)| *k == id)
            .expect("the active weapon has a state")
            .1
    }

    fn entry(&self, id: &str) -> &WeaponEntry {
        &self
            .entries
            .iter()
            .find(|(k, _)| *k == id)
            .expect("every registered weapon has an entry")
            .1
    }

    /// `get state()` (`index.js:191-193`).
    pub fn weapon_state(&self) -> &WeaponState {
        self.state_of(self.active_id)
    }

    /// `get current()` (`index.js:195-197`).
    pub fn current(&self) -> &'static WeaponDef {
        self.weapon_state().def
    }

    /// `get weaponIds()` (`index.js:199-201`).
    pub fn weapon_ids(&self) -> Vec<&'static str> {
        self.states.iter().map(|(k, _)| *k).collect()
    }

    /// `get ammo()` (`index.js:203-217`).
    pub fn ammo(&self) -> Ammo {
        let s = self.weapon_state();
        let mag = s.mag;
        let ch = u32::from(s.chambered);
        Ammo {
            mag: mag + ch,
            in_mag: mag,
            chambered: s.chambered,
            reserve: s.reserve,
            mag_size: s.def.mag_size,
            total: mag + ch + s.reserve,
            empty: mag + ch == 0,
        }
    }

    /// `get fireMode()` (`index.js:219-221`).
    pub fn fire_mode(&self) -> &'static str {
        self.weapon_state().mode
    }

    /// `get adsProgress()` (`index.js:223-225`).
    pub fn ads_progress(&self) -> f64 {
        self.viewmodel.ads_t
    }

    /// `get reloading()` (`index.js:227-230`).
    pub fn reloading(&self) -> bool {
        matches!(self.viewmodel.clip_name(), Some("reloadTac" | "reloadEmpty"))
    }

    /// `get inspecting()` (`index.js:232-234`).
    pub fn inspecting(&self) -> bool {
        self.viewmodel.clip_name() == Some("inspect")
    }

    /// `get switching()` (`index.js:236-238`).
    pub fn switching(&self) -> bool {
        self.switch_to.is_some()
    }

    /// `get firing()` (`index.js:240-242`).
    pub fn firing(&self) -> bool {
        self.since_shot < 0.12
    }

    /// `get spreadDegrees()` (`index.js:244-247`) — the live cone half-angle
    /// in degrees; the crosshair gap is driven off it.
    pub fn spread_degrees(&self) -> f64 {
        self.spread
    }

    /// The duration of the clip `viewmodel` is playing. `vm.clip.duration` in
    /// the source; `Viewmodel::clip` is private here (the field is public in
    /// the source only because JS has no private fields), and the clip playing
    /// is always one of the active weapon's five, so it is looked up by name.
    fn active_clip_duration(&self) -> Option<f64> {
        let name = self.viewmodel.clip_name()?;
        let c = &self.entry(self.active_id).clips;
        match name {
            "reloadTac" => Some(c.reload_tac.duration),
            "reloadEmpty" => Some(c.reload_empty.duration),
            "inspect" => Some(c.inspect.duration),
            "draw" => Some(c.draw.duration),
            "holster" => Some(c.holster.duration),
            _ => None,
        }
    }

    /// `getHudState()` (`index.js:258-283`). The source mutates and returns a
    /// preallocated object; this does the same and hands back a borrow.
    pub fn hud_state(&mut self) -> &WeaponHudState {
        let a = self.ammo();
        let reloading = self.reloading();
        let firing = self.firing();
        let ads_t = self.viewmodel.ads_t;
        let clip_duration = self.active_clip_duration();
        let clip_t = self.viewmodel.clip_t;
        let spread = self.spread;
        let s = self.weapon_state();
        let name = s.def.label.to_string();
        let mode = s.mode;
        let h = &mut self.hud_state;
        h.name = name;
        h.mode = mode;
        // `a.mag` counts the chambered round, so a topped-off rifle is 31; the
        // HUD draws one pip per round against magSize, so clamp the DISPLAY to
        // the magazine capacity rather than overflowing the pip strip.
        h.ammo = a.mag.min(a.mag_size);
        h.reserve = a.reserve;
        h.mag_size = a.mag_size;
        h.reloading = reloading;
        // 0..1 through the active reload clip; the bar is meaningless otherwise.
        h.reload_progress = match (reloading, clip_duration) {
            (true, Some(d)) if d != 0.0 => (clip_t / d).min(1.0),
            _ => 0.0,
        };
        h.ads = ads_t > 0.5;
        // `ui` maps this to reticle bloom as `4 + spread * 40` px, so hand it a
        // normalised 0..1 rather than raw degrees.
        h.spread = (spread / 6.0).clamp(0.0, 1.0);
        h.firing = firing;
        h
    }

    /* ================================================================ */
    /* weapon management (`index.js:289-326`)                           */
    /* ================================================================ */

    fn play(&mut self, name: ClipName) -> f64 {
        let clip = name.pick(&self.entry(self.active_id).clips);
        self.viewmodel.play(clip)
    }

    /// `setWeapon(id)` (`index.js:289-294`).
    pub fn set_weapon(&mut self, id: &str) -> bool {
        let known = self.states.iter().find(|(k, _)| *k == id).map(|(k, _)| *k);
        let Some(id) = known else { return false };
        if id == self.active_id || self.switch_to.is_some() {
            return false;
        }
        self.switch_to = Some(id);
        self.switch_timer = self.play(ClipName::Holster);
        true
    }

    /// `nextWeapon()` (`index.js:296-300`).
    pub fn next_weapon(&mut self) -> bool {
        let ids = self.weapon_ids();
        // `indexOf` is -1 for a missing id, and `(-1 + 1) % n` is 0 — the
        // source's own arithmetic, preserved.
        let i = ids
            .iter()
            .position(|k| *k == self.active_id)
            .map_or(-1i64, |p| p as i64);
        let next = ids[(((i + 1) % ids.len() as i64) + ids.len() as i64) as usize % ids.len()];
        self.set_weapon(next)
    }

    /// `cycleFireMode()` (`index.js:302-309`). Returns the (possibly
    /// unchanged) mode.
    pub fn cycle_fire_mode(&mut self) -> &'static str {
        if self.weapon_state().def.modes.len() < 2 {
            return self.weapon_state().mode;
        }
        let s = self.state_mut();
        s.mode_index = (s.mode_index + 1) % s.def.modes.len();
        s.mode = s.def.modes[s.mode_index];
        let mode = s.mode;
        self.burst_left = 0;
        mode
    }

    /// `reload()` (`index.js:311-320`). No-op if the magazine is full or the
    /// reserve is dry.
    pub fn reload(&mut self) -> bool {
        if self.reloading() || self.switching() {
            return false;
        }
        let s = self.weapon_state();
        if s.mag >= s.def.mag_size || s.reserve == 0 {
            return false;
        }
        let empty = s.mag == 0 && !s.chambered;
        self.viewmodel.stop_clip();
        self.play(if empty {
            ClipName::ReloadEmpty
        } else {
            ClipName::ReloadTac
        });
        self.pending_reload_empty = empty;
        true
    }

    /// `inspect()` (`index.js:322-326`).
    pub fn inspect(&mut self) -> bool {
        if self.reloading() || self.switching() || self.inspecting() {
            return false;
        }
        self.play(ClipName::Inspect);
        true
    }

    /* ================================================================ */
    /* firing (`index.js:332-430`)                                      */
    /* ================================================================ */

    /// `canFire()` (`index.js:332-338`).
    pub fn can_fire(&self) -> bool {
        if self.reloading() || self.switching() {
            return false;
        }
        if self.fire_timer > 0.0 {
            return false;
        }
        self.weapon_state().chambered
    }

    /// `tryFire()` (`index.js:341-420`). One round leaves the barrel; `false`
    /// if the trigger clicked dry.
    pub fn try_fire(
        &mut self,
        camera: &dyn FireCamera,
        player: Option<&mut (dyn WeaponPlayer + '_)>,
        physics: Option<&mut (dyn WeaponPhysics + '_)>,
    ) -> bool {
        if self.reloading() || self.switching() || self.fire_timer > 0.0 {
            return false;
        }
        if !self.weapon_state().chambered {
            // Dry: lock the bolt back and let the player know by feel.
            self.viewmodel.bolt_hold = 1.0;
            self.fire_timer = 0.25;
            return false;
        }
        if self.inspecting() {
            self.viewmodel.stop_clip();
        }

        let def = self.weapon_state().def;
        let first = self.since_shot > 0.35;

        // ---- feed the next round ----
        {
            let s = self.state_mut();
            s.chambered = false;
            if s.mag > 0 {
                s.mag -= 1;
                s.chambered = true;
            } else {
                self.viewmodel.bolt_hold = 1.0;
            }
        }

        // ---- deterministic recoil pattern ----
        let idx = self.shot_index.min(def.recoil.pattern_length - 1);
        let pair = self.weapon_state().pattern[idx];
        // The source indexes a flat `Float32Array` (`s.pattern[idx * 2]`), so
        // both values arrive already narrowed to `f32` and widened back.
        let pitch = f64::from(pair[0]);
        let yaw = f64::from(pair[1]);
        self.shot_index += 1;

        // ---- aim: camera forward + a spread cone ----
        let cam_quat = camera.aim_orientation();
        let cam_dir = cam_quat.rotate(V3::new(0.0, 0.0, -1.0)).normalize_or_zero();
        let mut dir = cam_dir;
        let spread_rad = self.spread * DEG;
        if spread_rad > 1e-5 {
            let (dx, dy) = self.rng.disc();
            let right = cam_quat.rotate(V3::new(1.0, 0.0, 0.0));
            let up = cam_quat.rotate(V3::new(0.0, 1.0, 0.0));
            dir = dir
                .add_scaled(right, spread_rad.tan() * dx)
                .add_scaled(up, spread_rad.tan() * dy)
                .normalize_or_zero();
        }

        // ---- projectile ----
        let muzzle = self.muzzle_world();
        let seed = self.rng.u32();
        let tracer = self.stats.fired % def.tracer_every == 0;
        let events = self.events.clone();
        let mut raycast = physics.map(|p| p.as_raycast_world());
        self.sim.spawn(
            SpawnParams {
                origin: BVec3::new(muzzle.x, muzzle.y, muzzle.z),
                dir: BVec3::new(dir.x, dir.y, dir.z),
                speed: def.muzzle_velocity,
                damage: def.damage,
                penetration: def.penetration,
                drag_k: def.drag_k,
                dropoff: def.dropoff,
                max_range: def.max_range,
                weapon: Some(def.id),
                mask: None,
                tracer,
            },
            raycast.as_deref_mut(),
            Some(&events),
        );

        // ---- feedback ----
        self.viewmodel.add_recoil(pitch, yaw, first);
        if let Some(p) = player {
            // The camera climb is the learnable part; the viewmodel kick is
            // the feel.
            p.add_recoil(pitch, yaw, def.recoil.roll * 0.35, def.recoil.punch);
        }
        self.spread = def.spread_max.min(self.spread + def.spread_per_shot);
        self.fire_timer = 60.0 / def.rpm;
        self.since_shot = 0.0;
        self.stats.fired += 1;
        self.pending_shots += 1;
        self.pending_first = self.pending_first || first;
        self.fire_seed = seed;

        // Shell leaves the port shortly after the shot, once the bolt is back.
        self.queue_shell(0.05f64.min(self.fire_timer * 0.45));
        true
    }

    /// `_queueShell(delay)` (`index.js:422-430`).
    fn queue_shell(&mut self, delay: f64) -> Option<usize> {
        for i in 0..self.shell_queue.len() {
            if self.shell_queue[i].t < 0.0 {
                self.shell_queue[i].t = delay;
                return Some(i);
            }
        }
        None
    }

    /* ================================================================ */
    /* reload / clip callbacks (`index.js:436-572`)                     */
    /* ================================================================ */

    /// `_onClipEvent(name, clipName)` (`index.js:436-475`).
    fn on_clip_event(
        &mut self,
        name: &str,
        clip_name: &str,
        physics: Option<&mut (dyn WeaponPhysics + '_)>,
        player_velocity: Option<V3>,
        pre: PreStepPose,
    ) {
        let is_reload = clip_name == "reloadTac" || clip_name == "reloadEmpty";
        match name {
            "start" => {
                if is_reload {
                    self.emit_reload(ReloadPhase::Start);
                }
            }
            "magout" => {
                if is_reload {
                    self.emit_reload(ReloadPhase::MagOut);
                }
            }
            "magdrop" => {
                if is_reload {
                    self.drop_magazine(physics, player_velocity, pre);
                }
            }
            "magin" => {
                if is_reload {
                    self.emit_reload(ReloadPhase::MagIn);
                    self.complete_reload(clip_name == "reloadEmpty");
                }
            }
            "boltrelease" => {
                self.viewmodel.bolt_hold = 0.0;
            }
            "end" => {
                if is_reload {
                    self.emit_reload(ReloadPhase::End);
                    self.viewmodel.bolt_hold = 0.0;
                }
                if clip_name == "holster" {
                    if let Some(to) = self.switch_to.take() {
                        self.active_id = to;
                        let rig = self.entry(to).rig.clone();
                        self.viewmodel.set_active(rig);
                        self.play(ClipName::Draw);
                        self.shot_index = 0;
                        self.spread = 0.0;
                    }
                }
            }
            _ => {}
        }
    }

    /// `_completeReload(empty)` (`index.js:482-494`).
    ///
    /// The chambered-round model: a tactical reload keeps the round in the
    /// chamber and gives you `magSize + 1`; an empty reload has to feed one
    /// out of the fresh magazine, so you end up with exactly `magSize`.
    fn complete_reload(&mut self, empty: bool) {
        let s = self.state_mut();
        let want = s.def.mag_size - s.mag;
        let take = want.min(s.reserve);
        s.reserve -= take;
        s.mag += take;
        if empty && !s.chambered && s.mag > 0 {
            s.mag -= 1;
            s.chambered = true;
        }
        self.shot_index = 0;
    }

    /// `_emitReload(phase)` (`index.js:496-500`).
    fn emit_reload(&mut self, phase: ReloadPhase) {
        let payload = WeaponReload {
            weapon: Some(self.current().id.to_string()),
            phase: Some(phase),
            position: None,
        };
        self.events.emit("weapon:reload", &payload);
    }

    /// `_dropMagazine()` (`index.js:503-537`). Spawn the discarded magazine as
    /// a real rigid body in the world.
    fn drop_magazine(
        &mut self,
        mut physics: Option<&mut (dyn WeaponPhysics + '_)>,
        player_velocity: Option<V3>,
        pre: PreStepPose,
    ) {
        let Some(w) = self.viewmodel.active().cloned() else {
            return;
        };
        let mag_len = self.entry(w.id).mag_len;
        // The magazine's world transform: the part's own animated local pose
        // (`_updateParts` writes it every frame) composed onto the rig, then
        // onto the anchor. `mag.updateMatrixWorld()` in the source.
        let local_pos = pre.parts.mag_pos.unwrap_or(w.mag_seat_pos);
        let local_quat = pre.parts.mag_quat.unwrap_or(w.mag_seat_quat);
        let mag_world = M4::multiply(
            self.compose_group(pre.rig_pos, pre.rig_quat),
            M4::compose(local_pos, local_quat, V3::new(1.0, 1.0, 1.0)),
        );
        // `Vector3.setFromMatrixPosition` is the translation column verbatim.
        let e = mag_world.e;
        let mut position = V3::new(e[12], e[13], e[14]);
        // `Quaternion.setFromRotationMatrix` over the same matrix's three
        // basis columns.
        let quaternion = Q::from_basis(
            V3::new(e[0], e[1], e[2]),
            V3::new(e[4], e[5], e[6]),
            V3::new(e[8], e[9], e[10]),
        );

        let Some(index) = self.mag_proxy(w.id, physics.as_deref_mut()) else {
            return;
        };

        // Magazine geometry hangs below its origin, so bias the body centre
        // down. `const half = w.magLen * 0.45; position.y -= half * 0.4;`
        let half = mag_len * 0.45;
        position.y -= half * 0.4;

        let mut vel = V3::new(0.0, -0.7, 0.0);
        if let Some(pv) = player_velocity {
            vel = vel.add(pv);
        }
        vel.x += self.rng.signed() * 0.25;
        vel.z += self.rng.signed() * 0.25;

        self.dropped_mags[index].position = position;
        self.dropped_mags[index].quaternion = quaternion;
        self.dropped_mags[index].visible = true;

        match physics {
            Some(phys) => {
                let body = phys.spawn_debris(
                    position,
                    vel,
                    DebrisOpts {
                        size: 0.02f64.max(mag_len * 0.28),
                        surface: crate::world::palette::Surface::Rubber,
                        mass: 0.38,
                        lifetime: 22.0,
                        restitution: 0.18,
                    },
                );
                self.dropped_mags[index].body = body;
                self.dropped_mags[index].until = self.time.elapsed + 22.0;
            }
            None => {
                self.dropped_mags[index].until = self.time.elapsed + 2.0;
            }
        }
    }

    /// `_magProxy(w)` (`index.js:540-572`) — two reusable world-space
    /// magazine props per weapon, reusing the oldest.
    fn mag_proxy(&mut self, id: &'static str, physics: Option<&mut (dyn WeaponPhysics + '_)>) -> Option<usize> {
        let pool = match self.mag_pools.iter().find(|(k, _)| *k == id) {
            Some((_, pool)) => *pool,
            None => {
                let base = self.dropped_mags.len();
                self.dropped_mags.push(MagProxy::default());
                self.dropped_mags.push(MagProxy::default());
                let pool = [base, base + 1];
                self.mag_pools.push((id, pool));
                pool
            }
        };
        // Reuse the oldest. `let best = pool[0]; for (p of pool) if (p.until <
        // best.until) best = p;` — strictly less, so a tie keeps `pool[0]`.
        let mut best = pool[0];
        for &i in &pool {
            if self.dropped_mags[i].until < self.dropped_mags[best].until {
                best = i;
            }
        }
        if let (Some(body), Some(phys)) = (self.dropped_mags[best].body, physics) {
            phys.remove_rigid_body(body);
        }
        self.dropped_mags[best].body = None;
        Some(best)
    }

    /* ================================================================ */
    /* frame (`index.js:578-723`)                                       */
    /* ================================================================ */

    /// `fixedUpdate(h)` (`index.js:578-580`).
    pub fn fixed_update(&mut self, h: f64, physics: Option<&mut (dyn WeaponPhysics + '_)>) {
        let mut raycast = physics.map(|p| p.as_raycast_world());
        self.sim.fixed_update(h, raycast.as_deref_mut());
    }

    /// `update(dt, ctx)` (`index.js:582-633`).
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        dt: f64,
        time: Time,
        input: &dyn WeaponInput,
        camera: &dyn FireCamera,
        mut player: Option<&mut (dyn WeaponPlayer + '_)>,
        mut physics: Option<&mut (dyn WeaponPhysics + '_)>,
    ) {
        self.time = time;
        let def = self.weapon_state().def;

        self.since_shot += dt;
        if self.fire_timer > 0.0 {
            self.fire_timer -= dt;
        }
        if self.burst_cooldown > 0.0 {
            self.burst_cooldown -= dt;
        }

        // ---- spread recovery ------------------------------------------
        let rest = self.rest_spread(def, player.as_deref());
        self.spread = rest.max(self.spread - def.spread_decay * dt * (1.0 + self.ads_progress()));
        if self.since_shot > 0.6 {
            self.shot_index = 0;
        }

        // ---- gather state ---------------------------------------------
        let live = !input.frozen() && input.enabled() && self.debug_mode.is_none();
        let empty = {
            let s = self.weapon_state();
            s.mag == 0 && !s.chambered
        };
        {
            let p = player.as_deref();
            self.state.ads = if live {
                input.ads() || p.is_some_and(|p| p.ads_requested())
            } else {
                self.debug_mode == Some(DebugPose::Ads)
            };
            self.state.sprint = live
                && p.is_some_and(WeaponPlayer::sprinting)
                && self.since_shot > 0.3;
            self.state.speed = p.map_or(0.0, WeaponPlayer::horizontal_speed);
            self.state.crouch = p.is_some_and(|p| p.stance() == Some(Stance::Crouch));
            self.state.airborne = p.is_some_and(WeaponPlayer::airborne);
            self.state.low_ready =
                p.is_some_and(|p| p.state_is_mantle() || p.mantling());
            self.state.empty = empty;
        }

        // ---- input -----------------------------------------------------
        if live {
            if input.action_pressed(Action::Reload) {
                self.reload();
            }
            if input.pressed("KeyB") {
                self.cycle_fire_mode();
            }
            if input.pressed("KeyI") {
                self.inspect();
            }
            if input.pressed("Digit1") {
                self.set_weapon("rifle");
            }
            if input.pressed("Digit2") {
                self.set_weapon("smg");
            }
            if input.pressed("Digit3") {
                self.set_weapon("pistol");
            }
            if input.pressed("Tab") {
                self.next_weapon();
            }
            if input.wheel() != 0.0 {
                self.next_weapon();
            }
            // `def` is captured before the mode may change above; the source
            // does the same — `_runTrigger(dt, ..., def, s)` is handed the def
            // read at the top of `update`.
            self.run_trigger(input, def, camera, player.as_deref_mut(), physics.as_deref_mut());
            self.state.trigger = input.fire() && self.can_fire();
            // Auto-reload on a dry trigger pull, like every modern shooter.
            if input.fire_pressed() && self.state.empty {
                self.reload();
            }
        } else if self.debug_mode.is_some() {
            self.run_debug(camera, player.as_deref_mut(), physics.as_deref_mut());
            self.state.trigger = self.since_shot < 0.09;
        }

        // Push the ADS curve to the player so camera FOV / move speed follow it.
        let ads_t = self.viewmodel.ads_t;
        if let Some(p) = player.as_deref_mut() {
            p.set_ads_progress(ads_t);
        }

        self.stats.live = self.sim.stats.live;
        self.stats.fired = self.sim.stats.fired;
    }

    /// `_runTrigger(dt, held, pressed, def, s)` (`index.js:636-659`) — the
    /// fire-mode state machine. `dt` is a parameter in the source and is never
    /// read, so it is not taken here.
    fn run_trigger(
        &mut self,
        input: &dyn WeaponInput,
        def: &'static WeaponDef,
        camera: &dyn FireCamera,
        mut player: Option<&mut (dyn WeaponPlayer + '_)>,
        mut physics: Option<&mut (dyn WeaponPhysics + '_)>,
    ) {
        let held = input.fire();
        let pressed = input.fire_pressed();
        match self.weapon_state().mode {
            "auto" => {
                if held {
                    self.try_fire(camera, player.as_deref_mut(), physics.as_deref_mut());
                }
            }
            "burst" => {
                if pressed && self.burst_left == 0 && self.burst_cooldown <= 0.0 {
                    self.burst_left = def.burst_count;
                }
                if self.burst_left > 0 && self.fire_timer <= 0.0 {
                    if self.try_fire(camera, player.as_deref_mut(), physics.as_deref_mut()) {
                        self.burst_left -= 1;
                        self.fire_timer = 60.0 / def.burst_rpm;
                        if self.burst_left == 0 {
                            self.burst_cooldown = def.burst_delay;
                        }
                    } else {
                        self.burst_left = 0;
                    }
                }
            }
            // semi
            _ => {
                if pressed {
                    self.try_fire(camera, player.as_deref_mut(), physics.as_deref_mut());
                }
            }
        }
    }

    /// `_restSpread(def, player, st)` (`index.js:661-670`).
    fn rest_spread(&self, def: &'static WeaponDef, player: Option<&dyn WeaponPlayer>) -> f64 {
        use crate::weapons::defs::Stance as SpreadStance;
        let mut base = lerp(def.spread_hip, def.spread_ads, self.ads_progress());
        // `st` is last frame's gathered state at this point in the source:
        // `_restSpread` runs at the TOP of `update`, before the block that
        // refreshes `this._state`. Preserved exactly.
        if self.state.crouch {
            base *= SpreadStance::Crouch.spread_mod();
        }
        if player.is_some_and(|p| p.stance() == Some(Stance::Prone)) {
            base *= SpreadStance::Prone.spread_mod();
        }
        if self.state.speed < 0.4 {
            base *= SpreadStance::Still.spread_mod();
        } else if self.state.speed > 3.2 {
            base *= SpreadStance::Walking.spread_mod();
        }
        if self.state.sprint {
            base *= SpreadStance::Sprinting.spread_mod();
        }
        if self.state.airborne {
            base *= SpreadStance::Airborne.spread_mod();
        }
        base
    }

    /// `lateUpdate(dt, ctx)` (`index.js:672-723`).
    pub fn late_update(
        &mut self,
        dt: f64,
        time: Time,
        camera: &dyn FireCamera,
        player: Option<&mut (dyn WeaponPlayer + '_)>,
        mut physics: Option<&mut (dyn WeaponPhysics + '_)>,
    ) {
        self.time = time;
        let player_velocity = player.as_deref().and_then(|p| p.velocity());
        // Snapshot the pose a clip beat would see — see [`PreStepPose`].
        let (pre_rig_pos, pre_rig_quat) = self.viewmodel.rig_pose();
        let pre = PreStepPose {
            rig_pos: pre_rig_pos,
            rig_quat: pre_rig_quat,
            parts: *self.viewmodel.parts(),
        };
        self.viewmodel
            .update(dt, &self.state.as_frame_input(), camera.as_view_camera());

        // The clip beats this frame. The source dispatches them from inside
        // `viewmodel.update`; see the module doc's "Not ported" note on the
        // one visible consequence.
        let beats: Vec<(&'static str, &'static str)> = self
            .viewmodel
            .clip_events()
            .iter()
            .map(|e| (e.name, e.clip))
            .collect();
        for (name, clip) in beats {
            self.on_clip_event(name, clip, physics.as_deref_mut(), player_velocity, pre);
        }

        // ---- muzzle flash / audio, now that the pose is final ----------
        if self.pending_shots > 0 {
            let weapon = self.current().id;
            self.fire_payload.origin = self.muzzle_world();
            self.fire_payload.dir = self.bore_dir();
            self.fire_payload.weapon = Some(weapon);
            self.fire_payload.seed = self.fire_seed;
            let payload = WeaponFire {
                weapon: Some(weapon.to_string()),
                suppressed: false,
                empty: false,
                origin: Some([
                    self.fire_payload.origin.x,
                    self.fire_payload.origin.y,
                    self.fire_payload.origin.z,
                ]),
                first_person: None,
            };
            for _ in 0..self.pending_shots {
                self.events.emit("weapon:fire", &payload);
            }
            self.pending_shots = 0;
            self.pending_first = false;
        }

        // ---- deferred shell ejection -----------------------------------
        for i in 0..self.shell_queue.len() {
            if self.shell_queue[i].t < 0.0 {
                continue;
            }
            self.shell_queue[i].t -= dt;
            if self.shell_queue[i].t > 0.0 {
                continue;
            }
            self.shell_queue[i].t = -1.0;
            self.shell_payload.position = self.eject_world();
            let speed = 2.3 + self.rng.float() * 1.2;
            self.shell_payload.velocity = self.eject_velocity(speed);
            if let Some(pv) = player_velocity {
                self.shell_payload.velocity = self.shell_payload.velocity.add(pv);
            }
            self.shell_payload.velocity.y += 1.1;
            let weapon = self.current().id;
            self.shell_payload.weapon = Some(weapon);
            let active = self.viewmodel.active().map(|w| w.id);
            let (case_len, rim_r) = match active {
                Some(id) => {
                    let e = self.entry(id);
                    (e.case_len, e.rim_r)
                }
                // `vm.active?.shell` absent — the source's `?? 0.0446` /
                // `?? 0.00495`.
                None => (0.0446, 0.00495),
            };
            self.shell_payload.case_len = case_len;
            self.shell_payload.case_radius = rim_r;
            self.shell_payload.spin = 28.0 + self.rng.float() * 34.0;
            let payload = WeaponShell {
                position: Some([
                    self.shell_payload.position.x,
                    self.shell_payload.position.y,
                    self.shell_payload.position.z,
                ]),
            };
            self.events.emit("weapon:shell", &payload);
        }

        // ---- retire dropped magazines -----------------------------------
        if !self.dropped_mags.is_empty() {
            let now = self.time.elapsed;
            for i in 0..self.dropped_mags.len() {
                let p = self.dropped_mags[i];
                if p.visible && p.until != 0.0 && now > p.until {
                    self.dropped_mags[i].visible = false;
                    if let (Some(body), Some(phys)) = (p.body, physics.as_deref_mut()) {
                        phys.remove_rigid_body(body);
                        self.dropped_mags[i].body = None;
                    }
                }
            }
        }
    }

    /// The renderer's scene-graph walk over `ctx.viewScene`, which is the only
    /// thing that composes the viewmodel anchor's world matrix. Call it once
    /// per frame, after [`WeaponCore::late_update`]. See the module doc.
    pub fn sync_anchor(&mut self, camera: &dyn FireCamera) {
        self.anchor_pos = camera.position();
        self.anchor_quat = camera.orientation();
    }

    /* ================================================================ */
    /* world-space queries (`viewmodel.js:1040-1071`, see seam 2)       */
    /* ================================================================ */

    /// `w.group.matrixWorld` — the anchor pose (as of the last
    /// [`sync_anchor`][WeaponCore::sync_anchor]) composed onto the current rig
    /// pose. The weapon group's own local transform is the identity
    /// (`viewmodel.js:315-319`).
    fn group_matrix(&self) -> M4 {
        let (rig_pos, rig_quat) = self.viewmodel.rig_pose();
        self.compose_group(rig_pos, rig_quat)
    }

    /// [`WeaponCore::group_matrix`] against an explicit rig pose — what a clip
    /// beat needs, because it runs before this frame's compose. See
    /// [`PreStepPose`].
    fn compose_group(&self, rig_pos: V3, rig_quat: Q) -> M4 {
        let anchor = M4::compose(self.anchor_pos, self.anchor_quat, V3::new(1.0, 1.0, 1.0));
        let rig = M4::compose(rig_pos, rig_quat, V3::new(1.0, 1.0, 1.0));
        M4::multiply(anchor, rig)
    }

    /// `muzzleWorld(out)` (`viewmodel.js:1041-1048`).
    pub fn muzzle_world(&self) -> V3 {
        match self.viewmodel.active() {
            None => V3::ZERO,
            Some(w) => self.group_matrix().transform_point(w.muzzle),
        }
    }

    /// `ejectWorld(out)` (`viewmodel.js:1050-1056`).
    pub fn eject_world(&self) -> V3 {
        match self.viewmodel.active() {
            None => V3::ZERO,
            Some(w) => self.group_matrix().transform_point(w.eject),
        }
    }

    /// `ejectVelocity(out, speed)` (`viewmodel.js:1058-1063`).
    /// `Vector3.transformDirection` applies the upper 3x3 and **normalises**,
    /// so the speed multiplies a unit vector.
    pub fn eject_velocity(&self, speed: f64) -> V3 {
        match self.viewmodel.active() {
            None => V3::ZERO,
            Some(w) => transform_direction(w.eject_dir, self.group_matrix()).mul_scalar(speed),
        }
    }

    /// `boreDir(out)` (`viewmodel.js:1066-1070`). The source normalises twice
    /// — `transformDirection` already does, and then `.normalize_or_zero()` again;
    /// transcribed as written because the second pass is not a no-op in the
    /// last bit.
    pub fn bore_dir(&self) -> V3 {
        match self.viewmodel.active() {
            None => V3::new(0.0, 0.0, -1.0),
            Some(_) => transform_direction(V3::new(0.0, 0.0, -1.0), self.group_matrix()).normalize_or_zero(),
        }
    }

    /* ================================================================ */
    /* capture harness (`index.js:729-826`)                             */
    /* ================================================================ */

    /// `debugPose(kind, opts)` (`index.js:734-805`) — freeze the viewmodel in
    /// a photogenic state for the screenshot harness.
    ///
    /// **Three lines of the source are not reachable here.** `vm._angVel.yaw =
    /// 0`, `vm._angVel.pitch = 0` and `vm._hasPrev = false`
    /// (`index.js:749-751`) reach `Viewmodel`'s private working state, which
    /// `viewmodel.rs` deliberately keeps private (see its "field privacy
    /// follows the source" note — those three are `_`-prefixed in the source
    /// too). On a freshly-constructed system they are already `0`/`false`, so
    /// the omission is a no-op on the path the harness actually uses; on a
    /// system that has been stepped it leaves one frame of angular velocity in
    /// the lag layer. `vm.debugFrozen = true` (`index.js:753`) is likewise
    /// absent, and is never read anywhere in `viewmodel.js`.
    pub fn debug_pose(&mut self, kind: DebugPose, grab_frame: f64) -> DebugPose {
        self.debug_mode = Some(kind);
        self.set_weapon_immediate("rifle");
        self.viewmodel.stop_clip();
        self.viewmodel.rec_pos.reset();
        self.viewmodel.rec_rot.reset();
        self.viewmodel.settle.reset();
        self.viewmodel.lag.reset();
        self.viewmodel.lag_rot.reset();
        self.viewmodel.bolt_hold = 0.0;
        self.viewmodel.bolt_cycle = 0.0;
        self.viewmodel.sprint_t = 0.0;
        self.viewmodel.low_ready_t = 0.0;
        self.viewmodel.bob_phase = 0.0;
        // A fixed, non-zero noise phase: a settled but not artificially
        // symmetric pose.
        self.viewmodel.noise_t = 12.37;
        self.spread = if kind == DebugPose::Ads { 0.24 } else { 2.05 };
        self.since_shot = 10.0;
        self.debug_frame = 0;

        {
            let s = self.state_mut();
            s.mag = if kind == DebugPose::Fire {
                22
            } else {
                s.def.mag_size
            };
            s.chambered = true;
            s.reserve = s.def.reserve;
        }

        if kind == DebugPose::Ads {
            self.viewmodel.ads_t = 1.0;
            self.state.ads = true;
        } else {
            self.viewmodel.ads_t = 0.0;
            self.state.ads = false;
        }
        self.state.sprint = false;
        self.state.speed = 0.0;
        self.state.trigger = false;

        // Frames (at the harness's fixed 60 Hz) on which to fire for the
        // 'fire' shot. A flash core lives 52 ms — about three frames at 60 Hz
        // — while the exact frame the shutter lands on is only known to within
        // a handful of frames, so: three spaced rounds early to fill the frame
        // with drifting smoke, brass in flight and a tracer, then a sustained
        // tail on a 2-frame cadence so a flash is lit continuously across the
        // whole uncertainty window. The cadence was 3 frames — the flash
        // core's own lifetime rounded UP — and frame 90 landed in the trough
        // between two cores. Two frames guarantees overlap.
        self.script_frames = if kind == DebugPose::Fire {
            let grab = crate::jsmath::round(grab_frame) as i64;
            let mut frames = vec![grab - 26, grab - 19, grab - 12];
            let mut f = grab - 6;
            while f <= grab + 18 {
                frames.push(f);
                f += 2;
            }
            Some(frames.into_iter().filter(|f| *f >= 2).collect())
        } else {
            None
        };
        kind
    }

    /// `setWeaponImmediate(id)` (`index.js:808-814`) — swap without the draw
    /// animation (harness + debug only).
    pub fn set_weapon_immediate(&mut self, id: &str) -> bool {
        let Some(id) = self.states.iter().find(|(k, _)| *k == id).map(|(k, _)| *k) else {
            return false;
        };
        self.switch_to = None;
        self.active_id = id;
        let rig = self.entry(id).rig.clone();
        self.viewmodel.set_active(rig);
        true
    }

    /// `_runDebug(ctx)` (`index.js:816-826`).
    fn run_debug(
        &mut self,
        camera: &dyn FireCamera,
        mut player: Option<&mut (dyn WeaponPlayer + '_)>,
        mut physics: Option<&mut (dyn WeaponPhysics + '_)>,
    ) {
        self.debug_frame += 1;
        let Some(frames) = self.script_frames.clone() else {
            return;
        };
        for f in frames {
            if f == self.debug_frame {
                self.fire_timer = 0.0;
                self.try_fire(camera, player.as_deref_mut(), physics.as_deref_mut());
            }
        }
    }

    /* ================================================================ */
    /* accessors for the state the shared event vocabulary cannot carry */
    /* ================================================================ */

    /// The `dir` field of the source's `weapon:fire` payload. See the module
    /// doc's table.
    pub fn fire_dir(&self) -> V3 {
        self.fire_payload.dir
    }

    /// The `seed` field of the source's `weapon:fire` payload.
    pub fn fire_seed(&self) -> u32 {
        self.fire_payload.seed
    }

    /// The whole `weapon:fire` payload the source assembles.
    pub fn fire_payload(&self) -> FirePayload {
        self.fire_payload
    }

    /// The whole `weapon:shell` payload the source assembles — the four fields
    /// (`velocity`, `caseLen`, `caseRadius`, `spin`) the shared vocabulary
    /// does not name.
    pub fn shell_payload(&self) -> ShellPayload {
        self.shell_payload
    }

    /// `this._shellQueue`'s timers.
    pub fn shell_queue(&self) -> &[ShellSlot; 8] {
        &self.shell_queue
    }

    /// `this._droppedMags`.
    pub fn dropped_mags(&self) -> &[MagProxy] {
        &self.dropped_mags
    }

    /// `this._state`.
    pub fn frame_state(&self) -> WeaponFrameState {
        self.state
    }

    /// `this._fireTimer`.
    pub fn fire_timer(&self) -> f64 {
        self.fire_timer
    }

    /// `this._burstLeft`.
    pub fn burst_left(&self) -> u32 {
        self.burst_left
    }

    /// `this._burstCooldown`.
    pub fn burst_cooldown(&self) -> f64 {
        self.burst_cooldown
    }

    /// `this._shotIndex`.
    pub fn shot_index(&self) -> usize {
        self.shot_index
    }

    /// `this._sinceShot`.
    pub fn since_shot(&self) -> f64 {
        self.since_shot
    }

    /// `this._switchTimer`.
    pub fn switch_timer(&self) -> f64 {
        self.switch_timer
    }

    /// `this._switchTo`.
    pub fn switch_to(&self) -> Option<&'static str> {
        self.switch_to
    }

    /// `this._pendingShots`.
    pub fn pending_shots(&self) -> u32 {
        self.pending_shots
    }

    /// `this._pendingFirst`.
    pub fn pending_first(&self) -> bool {
        self.pending_first
    }

    /// `this._pendingReloadEmpty`.
    pub fn pending_reload_empty(&self) -> bool {
        self.pending_reload_empty
    }

    /// `this._semiLatch` / `this._reloadPhase` — the two fields the source
    /// assigns and never reads. Exposed so the port's own dead state is at
    /// least visible rather than silently unused.
    pub fn dead_state(&self) -> (bool, Option<ReloadPhase>) {
        (self.semi_latch, self.reload_phase)
    }

    /// `this._scriptFrames`.
    pub fn script_frames(&self) -> Option<&[i64]> {
        self.script_frames.as_deref()
    }

    /// `this._debugFrame`.
    pub fn debug_frame(&self) -> i64 {
        self.debug_frame
    }

    /// One weapon's run-time state, by id.
    pub fn state_for(&self, id: &str) -> Option<&WeaponState> {
        self.states.iter().find(|(k, _)| *k == id).map(|(_, s)| s)
    }

    /// One weapon's build-time entry, by id.
    pub fn entry_for(&self, id: &str) -> Option<&WeaponEntry> {
        self.entries.iter().find(|(k, _)| *k == id).map(|(_, e)| e)
    }

    /// `dispose()` (`index.js:832-842`), minus the scene-graph teardown and
    /// the material/mesh disposal — neither exists here.
    pub fn dispose(&mut self, mut physics: Option<&mut (dyn WeaponPhysics + '_)>) {
        self.sim.clear();
        for i in 0..self.dropped_mags.len() {
            if let (Some(body), Some(phys)) = (self.dropped_mags[i].body, physics.as_deref_mut()) {
                phys.remove_rigid_body(body);
            }
        }
        self.dropped_mags.clear();
        self.mag_pools.clear();
    }
}

/// `Vector3.transformDirection(m)`: the upper 3x3 applied, then normalised.
fn transform_direction(v: V3, m: M4) -> V3 {
    let e = m.e;
    // Column-major, exactly as `Matrix4.elements` is laid out.
    let x = e[0] * v.x + e[4] * v.y + e[8] * v.z;
    let y = e[1] * v.x + e[5] * v.y + e[9] * v.z;
    let z = e[2] * v.x + e[6] * v.y + e[10] * v.z;
    V3::new(x, y, z).normalize_or_zero()
}

/// Triangles in a built weapon assembly — `addWeapon`'s `tris` accumulator
/// (`viewmodel.js:381`), which sums `triCount(geo)` over the merged
/// per-material buckets of the body and every moving sub-assembly.
fn assembly_tris(model: &mut RifleModel) -> usize {
    let mut n = geo_tris(&mut model.body);
    n += geo_tris(&mut model.moving.bolt);
    n += geo_tris(&mut model.moving.charging);
    n += geo_tris(&mut model.moving.trigger);
    n += geo_tris(&mut model.moving.selector);
    n += geo_tris(&mut model.moving.magazine);
    n
}

fn assembly_tris_smg(model: &mut SmgModel) -> usize {
    let mut n = geo_tris(&mut model.body);
    n += geo_tris(&mut model.moving.bolt);
    n += geo_tris(&mut model.moving.charging);
    n += geo_tris(&mut model.moving.trigger);
    n += geo_tris(&mut model.moving.selector);
    n += geo_tris(&mut model.moving.magazine);
    n
}

fn assembly_tris_pistol(model: &mut PistolModel) -> usize {
    let mut n = geo_tris(&mut model.body);
    n += geo_tris(&mut model.moving.slide);
    n += geo_tris(&mut model.moving.trigger);
    n += geo_tris(&mut model.moving.magazine);
    n
}

fn geo_tris(asm: &mut crate::weapons::geometry::Assembly) -> usize {
    // `triCount(geo)` (`geometry.js:441-444`) is
    // `(idx ? idx.count : position.count) / 3` — a bucket that never got an
    // index counts its vertices instead. Reading only `index` would score
    // every non-indexed bucket as zero.
    asm.build()
        .values()
        .map(|g| {
            let n = if g.index.is_empty() {
                g.pos.len() / 3
            } else {
                g.index.len()
            };
            n / 3
        })
        .sum::<usize>()
}

/* ==================================================================== */
/* The Subsystem wrapper                                                */
/* ==================================================================== */

/// The registered subsystem. `static id = 'weapons'`,
/// `static deps = ['materials', 'physics']`.
pub struct WeaponSystem {
    core: Rc<RefCell<WeaponCore>>,
    offs: Vec<(&'static str, SubscriptionId)>,
}

impl WeaponSystem {
    pub fn new(rng: &mut Rng) -> Self {
        WeaponSystem {
            core: Rc::new(RefCell::new(WeaponCore::new(rng))),
            offs: Vec::new(),
        }
    }

    /// The shared guts, for a host that drives the camera/player/physics seams
    /// by hand (as the app does — see the module doc's seams).
    pub fn core(&self) -> Rc<RefCell<WeaponCore>> {
        Rc::clone(&self.core)
    }

    /// `index.js:174-179`'s two subscriptions.
    pub fn wire_events(&mut self, ctx: &Ctx<'_>) {
        {
            let core = Rc::clone(&self.core);
            let id = ctx.events.on("player:land", move |p: &dyn Any| {
                if let Some(p) = p.downcast_ref::<crate::player::system::PlayerLandEvent>() {
                    // `Math.abs(e?.velocity ?? 3)`.
                    core.borrow_mut().viewmodel.land(p.velocity.abs());
                }
                Ok(())
            });
            self.offs.push(("player:land", id));
        }
        {
            let core = Rc::clone(&self.core);
            let id = ctx.events.on("player:jump", move |_p: &dyn Any| {
                core.borrow_mut().viewmodel.jump();
                Ok(())
            });
            self.offs.push(("player:jump", id));
        }
    }
}

impl Subsystem for WeaponSystem {
    fn id(&self) -> &'static str {
        "weapons"
    }

    fn deps(&self) -> &'static [&'static str] {
        &["materials", "physics"]
    }

    /// The source implements `fixedUpdate`, `update` and `lateUpdate`. All
    /// three need the camera/player/physics seams, which `Ctx` cannot supply,
    /// so the host drives [`WeaponCore`] directly and this wrapper only
    /// carries identity, the event wiring and teardown.
    fn phases(&self) -> &'static [Phase] {
        &[]
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn init(&mut self, ctx: &Ctx<'_>) -> Result<(), CoreError> {
        self.core
            .borrow_mut()
            .init(ctx.events.clone(), *ctx.time);
        self.wire_events(ctx);
        Ok(())
    }

    fn dispose(&mut self) {
        self.offs.clear();
        self.core.borrow_mut().dispose(None);
    }
}
