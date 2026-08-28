//! Ported from Claude-of-Duty `src/fx/index.js:1-1316` — the `FxSystem`
//! facade, in full apart from the GPU object graph.
//!
//! ## History: this module used to claim much less, and the claim was wrong
//!
//! An earlier draft of this doc said `index.js` was "genuinely inseparable
//! from THREE's scene graph" and filed `_syncLighting`, `muzzleFlash`,
//! `onWeaponFire`, the whole `debugBurst`/`_findTarget` capture harness and
//! the per-frame `update`/`lateUpdate` as unportable. That was the same
//! mistake [`crate::sky`]'s module doc records against itself: a
//! justification that legitimately covers the *object lifetime* was applied
//! to the *arithmetic inside it*. A camera transform is a 4x4 matrix; a
//! matrix is not a GPU. Those methods are now ported, taking the matrices
//! they read as an explicit [`FxFrame`] argument instead of reaching into a
//! `ctx.camera` that does not exist here.
//!
//! ## What this module does not port, and why
//!
//! Exactly three things, all of them object lifetime with no value to read:
//!
//! * `init`'s `ctx.scene.add(...)` / `render.registerPass(...)` /
//!   `render.addLight(...)` and `dispose()`'s matching teardown.
//! * `prewarmMaterials()` (`index.js:289-336`) and `_viewmodelPresent()` —
//!   `renderer.compile` against a scratch `THREE.Scene`, whose entire purpose
//!   is populating a driver-side program cache. Its observable is
//!   `renderer.info.programs.length`. [`FxSystem::late_update`] keeps the
//!   self-scheduling counter that decides *when* it would run
//!   (`index.js:820`), because that is a state machine and it is testable.
//! * `_pushLighting(l)` (`index.js:874-881`), which copies six already-
//!   computed values into `ShaderMaterial` uniforms. The values are all
//!   published as fields here ([`FxSystem::amb_top`] and friends); only the
//!   copy is missing.
//!
//! ## `ambience.js` — ported, wired, and the RNG caveat retired
//!
//! `index.js:121-124` constructs `new Ambience(this, {...})` and `update`
//! drives it (`index.js:786-787`). Both are reproduced here at the same points,
//! and [`FxSystem::add_smoke_column`]/[`FxSystem::add_smoke_source`]/
//! [`FxSystem::remove_smoke_source`] forward as `index.js:622-633` does.
//!
//! This file used to carry a caveat naming the construction as "the one place
//! this port's RNG stream can diverge from the real game's", conditional on
//! whether `Ambience`'s constructor draws. It does not: `new Ambience(...)` is
//! pure state initialisation, golden-pinned as `rngBefore == rngAfter`. The
//! caveat is retired, not merely satisfied — a deferral is a claim, and this one
//! was checked against the code.

use crate::ai::animator::quat_from_axis_angle;
use crate::fx::ambience::{Ambience, AmbienceInit, AmbienceScene, ColumnOpts, SourceOpts};
use crate::fx::atlas::{bake_decal_atlas, bake_particle_atlas, DecalAtlas, ParticleAtlas};
use crate::fx::decals::{DecalAdd, DecalSystem};
use crate::fx::haze::HazeSystem;
use crate::fx::lights::LightPool;
use crate::fx::muzzle::{muzzle_flash, MuzzleFlashOpts, MuzzleProfile};
use crate::fx::particles::{ParticleLayer, ParticleMode, ParticleSpawn};
use crate::fx::shells::{ShellSpawnOpts, ShellSystem};
use crate::fx::world::FxWorld;
use crate::physics::surfaces::mask;
use crate::rng::Rng;
use crate::weapons::rig_math::{M4, V3};
use crate::world::palette::Surface;

fn clamp(v: f64, a: f64, b: f64) -> f64 {
    v.clamp(a, b)
}

fn clamp_i(v: f64, a: f64, b: f64) -> usize {
    clamp(v, a, b).round() as usize
}

/// `Vector3.transformDirection(m)` — the upper 3x3 of a **column-major**
/// `Matrix4.elements`, then `normalize()`.
///
/// Three's `normalize()` is `divideScalar(length() || 1)`, hence
/// [`crate::jsmath::or_one`]. Note that `_syncLighting` calls
/// `.transformDirection(...).normalize()` — a *second* normalize of an
/// already-unit vector. That is not a no-op in floating point (the length is
/// `1 ± eps`, and dividing by it moves the last bits), so this port keeps
/// both calls at those sites rather than tidying one away.
fn transform_direction(v: V3, m: M4) -> V3 {
    let e = &m.e;
    let (x, y, z) = (v.x, v.y, v.z);
    let out = V3::new(
        e[0] * x + e[4] * y + e[8] * z,
        e[1] * x + e[5] * y + e[9] * z,
        e[2] * x + e[6] * y + e[10] * z,
    );
    three_normalize(out)
}

/// `Vector3.normalize()` — `divideScalar(length() || 1)`.
fn three_normalize(v: V3) -> V3 {
    v.scale(1.0 / crate::jsmath::or_one(v.length()))
}

/// `stats`, `index.js:182`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FxStats {
    /// Incremented once per handled `bullet:impact` — **not** once per
    /// particle. `index.js:385` is the only `stats.spawned++` in the file.
    pub spawned: u64,
    pub decals: u64,
    /// `this.stats.live = this.add.spawned + this.lit.spawned`
    /// (`index.js:809`). Despite the name this is the two layers' *cumulative*
    /// emit counters, not a live count — `ParticleLayer.spawned` is never
    /// reset (`particles.js:265, 415`). Ported as the source has it.
    pub live: u64,
}

/// [`FxSystem::add_decal`]'s options — [`DecalAdd`] minus the fields the
/// facade itself resolves (`point`/`normal`/`now`/`world`/`mask`). See the
/// module doc on why `roll`/`flip` stay `Option`: the source draws an RNG
/// value for either one that is omitted (`index.js:512-533`), so collapsing
/// them to a fixed default here would silently desync the RNG stream from
/// the source's draw order.
#[derive(Debug, Clone, Default)]
pub struct DecalOpts {
    pub tile: usize,
    pub size: f64,
    pub roll: Option<f64>,
    pub life: Option<f64>,
    pub fade: Option<f64>,
    pub opacity: Option<f64>,
    pub max_angle: Option<f64>,
    pub depth: Option<f64>,
    pub flip: Option<bool>,
}

/* ==================================================================== */
/* Event payloads                                                       */
/* ==================================================================== */

// NOTE FOR THE INTEGRATION PASS. `EventBus` payloads cross as `&dyn Any` and
// a handler downcasts to ONE concrete type, so there must be exactly one
// payload type per event name across the whole game. This is the THIRD
// subsystem to declare payloads for the same names, after
// `crate::audio::system` and `crate::ui::system` (whose own note says the
// same thing). The fields FX needs and neither of the other two carries:
//
//   `bullet:impact`  — `normal` and `incident`. Audio's `BulletImpact` has
//                      point/surface/damage/exit only; without the normal
//                      there is no impact spray direction at all.
//   `weapon:fire`    — `origin`, `dir`, `intensity`, `light`, `flashScale`,
//                      and the `fx === false` suppression flag. Audio's
//                      carries `origin`/`suppressed`/`empty`; the HUD's
//                      carries `recoil`.
//   `weapon:shell`   — `velocity`. Audio's carries `position` only.
//   `explosion`      — `damage`. Both others carry position/radius only.
//   `actor:death`    — nothing extra (audio's `ActorDeath` would do).
//   `player:land`    — nothing extra (audio's `PlayerLand` would do).
//   `player:footstep`— nothing extra (audio's `PlayerFootstep` would do).
//
// Converging the three into one superset per event is a whole-game decision
// and belongs in the integration pass. The four types below are declared
// where they are *needed* rather than being invented in a shared module,
// which is the same call the two earlier subsystems made.

/// `bullet:impact`, consumed at `index.js:167`.
#[derive(Debug, Clone, Copy)]
pub struct BulletImpact {
    /// `if (!e || !e.point) return`.
    pub point: Option<(f64, f64, f64)>,
    /// `if (!e.normal) return`.
    pub normal: Option<(f64, f64, f64)>,
    /// `e.incident ?? -e.normal` (`index.js:377, 388-391`).
    pub incident: Option<(f64, f64, f64)>,
    pub surface: Surface,
    /// `e.damage ?? 25`.
    pub damage: Option<f64>,
    /// `e.exit === true` — the exit wound gets 0.75x the energy.
    pub exit: bool,
}

/// `weapon:fire`, consumed at `index.js:169`.
#[derive(Debug, Clone, Default)]
pub struct WeaponFire {
    /// `e.fx === false` suppresses FX entirely so the caller can drive them.
    pub fx: Option<bool>,
    pub origin: Option<(f64, f64, f64)>,
    pub dir: Option<(f64, f64, f64)>,
    pub weapon: Option<String>,
    pub intensity: Option<f64>,
    pub light: Option<f64>,
    /// `e.flashScale`.
    pub flash_scale: Option<f64>,
}

/// `weapon:shell`, consumed at `index.js:170`.
#[derive(Debug, Clone, Copy, Default)]
pub struct WeaponShell {
    pub position: Option<(f64, f64, f64)>,
    pub velocity: Option<(f64, f64, f64)>,
}

/// `player:footstep`, consumed at `index.js:174`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlayerFootstep {
    pub running: bool,
    pub position: Option<(f64, f64, f64)>,
}

/* ==================================================================== */
/* The muzzle-light table — dead code in the source, ported anyway      */
/* ==================================================================== */

/// `MUZZLE_LIGHT`, `index.js:1293-1302`. Peak candela per weapon class.
///
/// **Nothing in `index.js` reads this table or calls [`weapon_key`]** — they
/// are dead code in the source (`muzzle.js` has its own, differently-keyed
/// profile table). Ported per the port recipe's "dead computation in the
/// source is still part of the source": the judgement that it is dead can be
/// wrong, and preserving it costs nothing.
///
/// **Order is load-bearing.** [`weapon_key`] iterates with `for (const name
/// in MUZZLE_LIGHT)` and returns the FIRST key the lowercased input
/// *contains*, so JS object insertion order decides the answer for an input
/// matching more than one — e.g. `"suppressed smg"` returns `"smg"`, because
/// `smg` is declared third and `suppressed` eighth. An array preserves that;
/// a `HashMap` would silently randomise it.
pub const MUZZLE_LIGHT: [(&str, f64); 8] = [
    ("rifle", 90.0),
    ("carbine", 78.0),
    ("smg", 60.0),
    ("pistol", 44.0),
    ("shotgun", 150.0),
    ("sniper", 130.0),
    ("lmg", 105.0),
    ("suppressed", 16.0),
];

/// `weaponKey(weapon)`, `index.js:1304-1310`.
///
/// The source accepts either a string or an object and falls back through
/// `w.class ?? w.kind ?? w.name ?? ''`; that collapsing is the caller's job
/// here, so this takes the already-resolved name. `None` is the source's
/// falsy-`weapon` early return.
pub fn weapon_key(weapon: Option<&str>) -> &'static str {
    let Some(key) = weapon else {
        return "rifle";
    };
    // `String(key).toLowerCase()`. JS `toLowerCase` is Unicode-aware and
    // Rust's `to_lowercase` is too; every key in the table is ASCII, so they
    // can only disagree on characters that cannot match anyway.
    let k = key.to_lowercase();
    MUZZLE_LIGHT
        .iter()
        .find(|(name, _)| k.contains(name))
        .map_or("rifle", |(name, _)| *name)
}

/// The candela peak for a weapon name — `MUZZLE_LIGHT[weaponKey(w)]`.
pub fn muzzle_light(weapon: Option<&str>) -> f64 {
    let key = weapon_key(weapon);
    MUZZLE_LIGHT.iter().find(|(name, _)| *name == key).map_or(90.0, |(_, v)| *v)
}

/* ==================================================================== */
/* The camera seam                                                      */
/* ==================================================================== */

/// One camera's transforms for this frame — what `ctx.camera` /
/// `ctx.viewCamera` supply to `_syncLighting`, `muzzleFlash`, `onWeaponFire`
/// and every `_stage*` helper.
///
/// The source calls `cam.updateMatrixWorld()` first at each of those sites;
/// here the caller has already done that, which is why both matrices are
/// given rather than one plus an inversion. Both are three's
/// `Matrix4.elements`: **column-major**, translation in `e[12..15]`.
#[derive(Debug, Clone, Copy)]
pub struct CameraFrame {
    pub matrix_world: M4,
    pub matrix_world_inverse: M4,
}

impl Default for CameraFrame {
    fn default() -> Self {
        CameraFrame {
            matrix_world: M4::IDENTITY,
            matrix_world_inverse: M4::IDENTITY,
        }
    }
}

impl CameraFrame {
    /// `camera.position` — `setFromMatrixPosition(matrixWorld)`.
    pub fn position(&self) -> V3 {
        V3::new(self.matrix_world.e[12], self.matrix_world.e[13], self.matrix_world.e[14])
    }
    /// `Object3D.worldToLocal(v)`.
    fn world_to_local(&self, v: V3) -> V3 {
        v.apply_matrix4(self.matrix_world_inverse)
    }
    /// `Object3D.localToWorld(v)`.
    fn local_to_world(&self, v: V3) -> V3 {
        v.apply_matrix4(self.matrix_world)
    }
}

/// Everything the frame context supplies that this module reads. `ctx.camera`
/// and `ctx.viewCamera`, plus the one `ctx.peek('weapons')` call
/// `_stageMuzzle` makes (`index.js:1045-1050`).
#[derive(Debug, Clone, Copy, Default)]
pub struct FxFrame<'a> {
    pub camera: CameraFrame,
    pub view_camera: CameraFrame,
    /// `wp.muzzleWorld(v)` — the weapon's own muzzle transform. `None` when
    /// there is no `weapons` subsystem; the source additionally treats a
    /// result with `lengthSq() <= 1e-6` as "not welded" and falls back to a
    /// fixed eye-relative offset, which [`FxSystem::stage_muzzle`] does too.
    pub muzzle_world: Option<V3>,
    /// `r?.sunDir` — the direction the renderer decided the sun is in.
    pub sun_dir: Option<V3>,
    /// `r?.activeSun` — `(color, intensity)`. `None` is the source's
    /// `sunI = 4.3` fallback.
    pub active_sun: Option<(V3, f64)>,
    /// `ctx.scene.fog` — `(color, density)`. The source resolves
    /// `fog.density ?? 1 / Math.max(1, fog.far ?? 400)` at the emitter.
    pub fog: Option<(V3, f64)>,
    /// `ctx.scene`, as the ambience needs it (`index.js:787`): the one question
    /// it asks is whether a followed prop is still attached, so the seam is that
    /// question and nothing else. `None` is a scene-less harness, which the
    /// source cannot express and which
    /// [`crate::fx::ambience::Ambience::update`] treats as "still attached".
    pub scene: Option<&'a dyn AmbienceScene>,
}

/// [`FxSystem::muzzle_flash`]'s `o` — the facade's option bag,
/// `index.js:415-443`. Distinct from [`crate::fx::muzzle::MuzzleFlashOpts`],
/// which is the *resolved* bag the recipe module reads: this one still has
/// `view`/`viewSpace` unmerged and its `position`/`direction` in whichever
/// space the caller chose.
#[derive(Debug, Clone, Default)]
pub struct FacadeMuzzleOpts {
    pub position: (f64, f64, f64),
    pub direction: (f64, f64, f64),
    pub weapon: Option<String>,
    /// Emit the sprites into the viewmodel scene, converting through the two
    /// cameras, so the flash composites over the weapon rather than under it.
    pub view: bool,
    /// The caller's point is ALREADY in viewmodel space (the usual case for a
    /// weapon whose muzzle is a bone in `viewScene`): map it back out for the
    /// light and leave the sprites where they are.
    pub view_space: bool,
    pub intensity: Option<f64>,
    pub light: Option<f64>,
    pub scale: Option<f64>,
}

/// `_toView(pos, dir)`, `index.js:478-488` — a world-space point+dir into
/// viewmodel-scene space.
fn to_view(frame: &FxFrame<'_>, pos: V3, dir: V3) -> (V3, V3) {
    let p = frame.view_camera.local_to_world(frame.camera.world_to_local(pos));
    // Two `transformDirection`s, each of which normalizes, then an explicit
    // third `normalize()`. All three are in the source; none is redundant in
    // floating point.
    let d = transform_direction(dir, frame.camera.matrix_world_inverse);
    let d = transform_direction(d, frame.view_camera.matrix_world);
    (p, three_normalize(d))
}

/// `_fromView(pos)`, `index.js:466-475` — a viewmodel-scene point into world
/// space, for the punctual light.
fn from_view(frame: &FxFrame<'_>, pos: V3) -> V3 {
    frame.camera.local_to_world(frame.view_camera.world_to_local(pos))
}

/// `addSmokeColumn`'s `o` parameter shape, `explosions.js:181-189` — kept as
/// a real type even though [`FxSystem::add_smoke_column`] is a stub (see the
/// module doc), so the call sites that build one stay faithful to the
/// source's argument shape.
#[derive(Debug, Clone, Copy, Default)]
pub struct SmokeColumnOpts {
    pub radius: f64,
    pub duration: f64,
    pub rate: f64,
    pub rise: f64,
    pub dark: f64,
    pub life: f64,
    pub growth: f64,
}

/* ==================================================================== */
/* Debug staging — index.js:883-1264                                    */
/* ==================================================================== */

/// One entry of `this._script`, `index.js:918`. In the source each entry is
/// `{ t, fn }` where `fn` is a closure; the closures fall into six shapes and
/// this enum is those six.
///
/// The split between [`StageAction::Impact`] and [`StageAction::ImpactRandom`]
/// is **not** cosmetic. `_stageWallHits` computes its `u`/`v` (and their RNG
/// wobble) at *staging* time, outside the closure (`index.js:996-997`), while
/// `debugBurst`'s `'combat'` arm draws them *inside* the closure, at fire time
/// (`index.js:953-955`). Collapsing the two would move three RNG draws per
/// loop from one moment to another and desync the whole stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StageAction {
    /// `boom(side)`, `index.js:925-931`.
    Boom { side: f64 },
    /// `_impactAt(target, u, v, surf)` with `u`/`v` fixed at staging time.
    Impact { u: f64, v: f64, surface: Option<Surface> },
    /// `_impactAt(target, rng.signed() * a, rng.range(lo, hi), surf)` — both
    /// draws happen when the entry fires.
    ImpactRandom {
        u_scale: f64,
        v_lo: f64,
        v_hi: f64,
        surface: Option<Surface>,
    },
    Muzzle,
    Shell,
    Tracer,
    Crossfire,
}

/// `{ t, fn }`, `index.js:918`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScriptEntry {
    pub t: f64,
    pub action: StageAction,
}

/// `debugBurst(kind)`'s argument, `index.js:896`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BurstKind {
    /// `'none'` / `'clear'` / `'off'` — stop a previously staged loop.
    None,
    Explosion,
    Muzzle,
    /// `'combat'` / `'firefight'`.
    Combat,
    /// The default arm: sustained fire walking across a wall.
    Impacts,
}

/// `debugBurst`'s return value, `index.js:905, 935, 947, 961, 984`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BurstReport {
    pub staged: BurstKind,
    /// `target.point.toArray()` — absent for the `'muzzle'` and `'none'` arms.
    pub at: Option<(f64, f64, f64)>,
    /// Only the default `'impacts'` arm reports it.
    pub surface: Option<Surface>,
}

/// `this._target`, `index.js:1096-1106` — the staged surface and its tangent
/// frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StageTarget {
    pub point: V3,
    pub normal: V3,
    pub tangent: V3,
    pub bitangent: V3,
    pub surface: Surface,
    /// `t.world` — `Some` when a real BVH hit backed it, `None` for the
    /// virtual plane (which also suppresses decals).
    pub has_world: bool,
    pub distance: f64,
    pub span_u: f64,
    pub span_v: f64,
}

impl Default for StageTarget {
    /// The literal at `index.js:1096-1106`.
    fn default() -> Self {
        StageTarget {
            point: V3::ZERO,
            normal: V3::ZERO,
            tangent: V3::ZERO,
            bitangent: V3::ZERO,
            surface: Surface::Concrete,
            has_world: false,
            distance: 0.0,
            span_u: 3.0,
            span_v: 1.2,
        }
    }
}

/// One probe hit recorded by `_findTarget`'s fan — `this._probes`' eight
/// floats per entry (`index.js:1142-1151`) plus the parallel `_probeSurf`
/// name. A struct rather than a flat stride because nothing depends on the
/// packing, only on the values and their order.
///
/// **`f32`, not `f64`, and that is load-bearing.** `this._probes` is a
/// `Float32Array` (`index.js:1129`), so every value here is rounded to single
/// precision on the way in and read back as the rounded value. The scoring,
/// the planarity test and the span measurement then all run in `f64` over
/// `f32` inputs. Storing these as `f64` moves the chosen hit point in the
/// eighth significant digit and the golden catches it — this is the
/// `Float32Array`-storage-width trap the port recipe names, in the one place
/// in this file where it applies.
#[derive(Debug, Clone, Copy)]
struct Probe {
    point: [f32; 3],
    normal: [f32; 3],
    distance: f32,
    /// `probes[b + 7]` — the framing-error score before the planarity bonus.
    cost: f32,
    surface: Surface,
}

impl Probe {
    fn point_v3(self) -> V3 {
        V3::new(
            f64::from(self.point[0]),
            f64::from(self.point[1]),
            f64::from(self.point[2]),
        )
    }
    fn normal_v3(self) -> V3 {
        V3::new(
            f64::from(self.normal[0]),
            f64::from(self.normal[1]),
            f64::from(self.normal[2]),
        )
    }
}

/// `class FxSystem`'s CPU-testable state. See the module doc for what is
/// dropped (every `THREE.*` object, the renderer/camera handles).
pub struct FxSystem {
    pub rng: Rng,
    pub now: f64,
    pub gravity: f64,
    /// `this.pScale = clamp( budget / 12000, 0.4, 1.25 )`, `index.js:121`.
    ///
    /// A particle **count** multiplier, and only that: every use in the source
    /// has the shape `Math.round( N * fx.pScale )` (`impacts.js:158, 334`,
    /// `muzzle.js:287, 326, 389`, `explosions.js:22`, `index.js:713, 768`) and
    /// none of them scales `size0`/`size1`. There is no `uPScale` uniform in
    /// `particles.js`. The four presets give 0.4 / 0.5 / 1.0 / 1.25
    /// (`config.js:34, 49, 64, 79`).
    pub pscale: f64,

    pub lit: ParticleLayer,
    pub add: ParticleLayer,
    pub motes: ParticleLayer,
    pub view_add: ParticleLayer,
    pub view_lit: ParticleLayer,
    view_attached: bool,

    pub decals: DecalSystem,
    pub shells: ShellSystem,
    pub haze_sys: HazeSystem,
    pub lights: LightPool,
    pub view_lights: Option<LightPool>,

    pub atlas: ParticleAtlas,
    pub decal_atlas: DecalAtlas,

    pub stats: FxStats,

    /// The physics seam — see [`crate::fx::world`]'s module doc. `None`
    /// reproduces every `if (physics) { ... }`/`physics?.` guard in the
    /// source.
    pub world: Option<Box<dyn FxWorld>>,

    /// The right/up rows of `ctx.camera.matrixWorldInverse` — the only thing
    /// `screenAngle` reads off the camera (`muzzle.js:43-51`, `m[0], m[4],
    /// m[8]` and `m[1], m[5], m[9]`).
    ///
    /// Held on the system rather than passed per call because the source holds
    /// it the same way: `screenAngle` reaches for `fx.ctx.camera`, a live
    /// reference the FX system has owned since construction, and its callers
    /// (`impacts.js`'s per-surface recipes) take no camera argument. Refreshed
    /// once per [`FxSystem::update`]; `None` is the source's
    /// `if (!cam) return 0`.
    pub camera_basis: Option<([f64; 3], [f64; 3])>,

    /// `this.ambience` (`index.js:121`) — the drifting motes, shimmer and the
    /// smoke-column/source emitters.
    ///
    /// Held as an `Option` for one reason: the source calls
    /// `this.ambience.update( this, … )`, handing the ambience a mutable
    /// reference to the very object that owns it. Rust will not alias that, and
    /// the alias is not incidental — [`crate::fx::ambience::Ambience::update`]
    /// draws off `fx.rng`, so it MUST see the system's own stream to keep the
    /// draw order the source has. Vacating the slot for the duration of its own
    /// update is the honest encoding of what the source does: while the ambience
    /// is running, it is not reachable through the system.
    ambience: Option<Ambience>,

    /// [`FxSystem::sun_world`]'s backing field — see the module doc.
    sun_world: (f64, f64, f64),

    suppress_decals: bool,

    // ---- lighting inputs, `index.js:126-133` ------------------------
    /// `this._ambTop` — upper-hemisphere ambient the lit-particle shader reads.
    pub amb_top: V3,
    /// `this._ambBot`.
    pub amb_bot: V3,
    /// `this._sunCol` — already multiplied by the sun's intensity.
    pub sun_col: V3,
    /// `this._sunView` — the sun direction in *view* space.
    pub sun_view: V3,
    /// `this._upView`.
    pub up_view: V3,
    /// `this._fog` — `(r, g, b, density)`.
    pub fog: (f64, f64, f64, f64),
    /// `this._ambientOverride` — set by [`FxSystem::set_ambient`]; once true,
    /// `_syncLighting` stops deriving the ambient from the sun.
    ambient_override: bool,
    /// `this._sunFactor` — handed to `ambience` each frame (`index.js:786`).
    pub sun_factor: f64,
    /// The viewmodel-space sun direction, `index.js:864-868`. Only meaningful
    /// once the view layers are attached.
    pub sun_view_vm: V3,
    /// `viewLit.uniforms.uUpView` in viewmodel space, `index.js:870`.
    pub up_view_vm: V3,

    // ---- dev burst script, `index.js:177-180` -----------------------
    script: Vec<ScriptEntry>,
    script_time: f64,
    script_period: f64,
    /// `this._target` — retained between bursts exactly as the source's
    /// `(this._target ??= {...})` does.
    target: StageTarget,
    /// `this._targetSupport` — the winning probe's co-planar neighbour count.
    /// Logged by the source; kept because it is the only window onto the
    /// planarity scoring.
    pub target_support: i64,

    // ---- pre-warm scheduling, `index.js:202-203, 820` ---------------
    warm_ticks: u32,
    warmed: bool,
}

impl FxSystem {
    /// `async init(ctx)`, `index.js:38-206` — the budget dimensioning, pool
    /// construction and atlas baking. `particle_budget`/`decal_budget` are
    /// `config.q.particleBudget`/`decalBudget` (`crate::config::
    /// QualityPreset`). `seed` is the root FX RNG seed — the source forks
    /// this from `ctx.rng` (`this.rng = ctx.rng.fork()`, `index.js:40`); a
    /// caller wiring this into the full engine should pass
    /// `ctx.rng.fork()`'s output here.
    pub fn new(seed: u32, particle_budget: u32, decal_budget: u32, gravity: f64) -> Self {
        let mut rng = Rng::new(seed);

        let budget = f64::from(particle_budget);
        let big = particle_budget >= 10_000;
        let atlas_size = if big { 1024 } else { 512 };
        // `buildParticleAtlas(this.rng.fork(), atlasSize)`, then
        // `buildDecalAtlas(this.rng.fork(), atlasSize)` — in that order.
        let atlas = bake_particle_atlas(&mut rng.fork(), atlas_size);
        let decal_atlas = bake_decal_atlas(&mut rng.fork(), atlas_size);

        let mote = clamp_i(budget * 0.06, 96.0, 600.0);
        let haze_cap = clamp_i(budget * 0.04, 48.0, 320.0);
        let view_add_cap = clamp_i(budget * 0.03, 48.0, 400.0);
        let view_lit_cap = clamp_i(budget * 0.02, 32.0, 256.0);
        let rest = (budget - mote as f64 - haze_cap as f64 - view_add_cap as f64 - view_lit_cap as f64).max(256.0);
        let lit_cap = (rest * 0.55).round() as usize;
        let add_cap = rest as usize - lit_cap;

        let pscale = clamp(budget / 12000.0, 0.4, 1.25);

        let lit = ParticleLayer::new(lit_cap, ParticleMode::Lit);
        let add = ParticleLayer::new(add_cap, ParticleMode::Additive);
        let motes = ParticleLayer::new(mote, ParticleMode::Additive);
        let view_add = ParticleLayer::new(view_add_cap, ParticleMode::Additive);
        let view_lit = ParticleLayer::new(view_lit_cap, ParticleMode::Lit);

        let decals = DecalSystem::new(decal_budget as usize, decal_atlas.cols);
        let haze_sys = HazeSystem::new(haze_cap);
        let lights = LightPool::new(4);
        // `new ShellSystem(this)`, `index.js:127` — forks `this.rng` for the
        // brass texture bake (`shells.js:58`); see `ShellSystem::new`.
        let shells = ShellSystem::new(&mut rng);
        // `new Ambience( this, { motes: mote, shimmer: budget >= 4000 } )`
        // (`index.js:121-124`), constructed here so it lands in the source's
        // position. Verified to spend ZERO rng: `rngBefore == rngAfter` is
        // pinned by `ambience`'s own tests, which is why the fork below it is
        // unaffected and why this module doc's old "the one place this port's
        // stream can diverge" caveat was wrong and has been removed.
        let ambience = Ambience::new(&AmbienceInit {
            motes: Some(mote as f64),
            box_size: None,
            shimmer: Some(budget >= 4000.0),
        });

        FxSystem {
            rng,
            now: 0.0,
            gravity,
            pscale,
            lit,
            add,
            motes,
            view_add,
            view_lit,
            view_attached: false,
            decals,
            shells,
            haze_sys,
            lights,
            view_lights: None,
            ambience: Some(ambience),
            atlas,
            decal_atlas,
            stats: FxStats::default(),
            world: None,
            camera_basis: None,
            sun_world: (0.0, 1.0, 0.0),
            suppress_decals: false,
            // `index.js:127-133`.
            amb_top: V3::new(0.42, 0.5, 0.66),
            amb_bot: V3::new(0.2, 0.17, 0.14),
            sun_col: V3::new(1.0, 0.93, 0.82),
            sun_view: V3::new(0.0, 1.0, 0.0),
            up_view: V3::new(0.0, 1.0, 0.0),
            fog: (0.62, 0.66, 0.72, 0.0),
            ambient_override: false,
            sun_factor: 1.0,
            sun_view_vm: V3::new(0.0, 1.0, 0.0),
            up_view_vm: V3::new(0.0, 1.0, 0.0),
            script: Vec::new(),
            script_time: 0.0,
            script_period: 0.0,
            target: StageTarget::default(),
            target_support: -1,
            warm_ticks: 0,
            warmed: false,
        }
    }

    /// A minimal instance for unit tests across the `fx` module — a small
    /// budget, no physics world bound. Not a port of anything in the
    /// source; the source has no equivalent because JS tests do not need a
    /// typed constructor to skip optional state.
    #[cfg(test)]
    pub fn test_instance(seed: u32) -> Self {
        FxSystem::new(seed, 2000, 64, -19.62)
    }

    // ===================================================================
    // emit helpers — `index.js:212-226`.
    // ===================================================================

    // These do NOT touch `stats.spawned`: the source's five emit helpers are
    // one-liners (`index.js:216-226`) and `stats.spawned++` appears exactly
    // once in the whole file, in `onImpact` (`index.js:385`). An earlier draft
    // of this port incremented here, which made `stats.spawned` count
    // particles instead of impacts — two orders of magnitude out.

    pub fn emit_add(&mut self, s: &ParticleSpawn) -> usize {
        self.add.emit(s, self.now)
    }

    pub fn emit_lit(&mut self, s: &ParticleSpawn) -> usize {
        self.lit.emit(s, self.now)
    }

    pub fn emit_mote(&mut self, s: &ParticleSpawn) -> usize {
        self.motes.emit(s, self.now)
    }

    pub fn emit_view_add(&mut self, s: &ParticleSpawn) -> usize {
        self.attach_view();
        self.view_add.emit(s, self.now)
    }

    pub fn emit_view_lit(&mut self, s: &ParticleSpawn) -> usize {
        self.attach_view();
        self.view_lit.emit(s, self.now)
    }

    /// `view ? fx.emitViewAdd : fx.emitAdd`, the dispatch every recipe module
    /// (`muzzle.js`) reads once and calls repeatedly (`muzzle.js:120-121`).
    pub fn emit_add_view(&mut self, view: bool, s: &ParticleSpawn) -> usize {
        if view {
            self.emit_view_add(s)
        } else {
            self.emit_add(s)
        }
    }

    pub fn emit_lit_view(&mut self, view: bool, s: &ParticleSpawn) -> usize {
        if view {
            self.emit_view_lit(s)
        } else {
            self.emit_lit(s)
        }
    }

    /// `_attachView()`, `index.js:229-243`. Idempotent; a real viewmodel
    /// scene attachment has no meaning here (see the module doc), so this
    /// only tracks the `view_lights` pool the source lazily builds
    /// alongside it.
    fn attach_view(&mut self) {
        if self.view_attached {
            return;
        }
        self.view_attached = true;
        if self.view_lights.is_none() {
            self.view_lights = Some(LightPool::new(2));
        }
    }

    // ===================================================================
    // public API — `index.js:369-661`.
    // ===================================================================

    /// `onImpact(e)`, `index.js:373-378`. `e.exit` and `e.damage` are the
    /// caller-supplied event payload fields; `incident` is
    /// `e.incident ?? this._defaultIncident(e)` (`index.js:377`,
    /// `-e.normal`) resolved by the caller since there is no event-bus
    /// payload type in this port yet.
    pub fn on_impact(
        &mut self,
        point: (f64, f64, f64),
        normal: (f64, f64, f64),
        incident: Option<(f64, f64, f64)>,
        surface: crate::world::palette::Surface,
        damage: Option<f64>,
        exit: bool,
    ) {
        let mut energy = clamp(0.7 + damage.unwrap_or(25.0) / 55.0, 0.7, 1.7);
        if exit {
            energy *= 0.75;
        }
        let inc = incident.unwrap_or((-normal.0, -normal.1, -normal.2));
        crate::fx::impacts::spawn_impact(self, point, normal, inc, surface, energy);
        // `this.stats.spawned++`, `index.js:385` — once per impact.
        self.stats.spawned += 1;
    }

    /// The `on('bullet:impact', ...)` handler, `index.js:167`. The two early
    /// returns (`!e.point`, `!e.normal`) are the source's, and `now` is
    /// `ctx.time.elapsed` read at `index.js:380`.
    pub fn handle_impact(&mut self, now: f64, e: &BulletImpact) {
        let Some(point) = e.point else {
            return;
        };
        self.now = now;
        let Some(normal) = e.normal else {
            return;
        };
        self.on_impact(point, normal, e.incident, e.surface, e.damage, e.exit);
    }

    /// `onWeaponFire(e)`, `index.js:394-408`. `e.fx === false` suppresses.
    ///
    /// The first-person test is `camPos.distanceToSquared(e.origin) < 2.25` —
    /// squared distance against 1.5 m squared, with `camPos` read off
    /// `ctx.camera.matrixWorld`.
    pub fn on_weapon_fire(&mut self, now: f64, frame: &FxFrame<'_>, e: &WeaponFire) -> Option<MuzzleProfile> {
        if e.fx == Some(false) {
            return None;
        }
        let (origin, dir) = (e.origin?, e.dir?);
        self.now = now;
        let cam_pos = frame.camera.position();
        let first_person = cam_pos.distance_to_squared(V3::new(origin.0, origin.1, origin.2)) < 2.25;
        Some(self.muzzle_flash(
            now,
            frame,
            &FacadeMuzzleOpts {
                position: origin,
                direction: dir,
                weapon: e.weapon.clone(),
                view: first_person,
                intensity: e.intensity,
                light: e.light,
                scale: e.flash_scale,
                view_space: false,
            },
        ))
    }

    /// The `on('weapon:shell', ...)` handler, `index.js:170`.
    pub fn handle_weapon_shell(&mut self, now: f64, e: &WeaponShell) {
        let Some(position) = e.position else {
            return;
        };
        self.now = now;
        self.spawn_shell(position, e.velocity, ShellSpawnOpts::default());
    }

    /// The `on('player:footstep', ...)` handler, `index.js:174`.
    pub fn handle_footstep(&mut self, now: f64, e: &PlayerFootstep) {
        let Some(position) = e.position else {
            return;
        };
        self.on_footstep(now, e.running, position);
    }

    /// `tracer(from, to, speed)`, `index.js:491-495`. `now` is the
    /// `this.now = ctx.time.elapsed` the source does before spawning; the
    /// `!from || !to` guard is the caller's, since a `(f64, f64, f64)` cannot
    /// be absent here.
    pub fn tracer_at(&mut self, now: f64, from: (f64, f64, f64), to: (f64, f64, f64), speed: f64) {
        self.now = now;
        self.tracer(from, to, speed);
    }

    /// [`FxSystem::tracer_at`] against the already-set [`FxSystem::now`].
    pub fn tracer(&mut self, from: (f64, f64, f64), to: (f64, f64, f64), speed: f64) {
        crate::fx::tracers::spawn_tracer(self, from, to, speed, 1.0);
    }

    /// `explosion(e)`, `index.js:498-501`, with the `this.now` write.
    pub fn explosion_at(&mut self, now: f64, opts: &crate::fx::explosions::ExplosionOpts) {
        self.now = now;
        self.explosion(opts);
    }

    /// [`FxSystem::explosion_at`] against the already-set [`FxSystem::now`].
    pub fn explosion(&mut self, opts: &crate::fx::explosions::ExplosionOpts) {
        crate::fx::explosions::explode(self, opts);
    }

    /// `spawnShell(position, velocity, opts)`, `index.js:505-509`.
    pub fn spawn_shell(&mut self, position: (f64, f64, f64), velocity: Option<(f64, f64, f64)>, opts: ShellSpawnOpts) {
        self.shells.spawn(&mut self.rng, position, velocity, opts);
    }

    /// `addDecal(point, normal, opts)`, `index.js:511-535`. See the module
    /// doc's [`DecalOpts`] note on why `roll`/`flip` are resolved here, in
    /// this exact order, rather than defaulted inside [`DecalSystem::add`].
    pub fn add_decal(&mut self, point: (f64, f64, f64), normal: (f64, f64, f64), opts: DecalOpts) -> bool {
        if self.suppress_decals {
            return false;
        }
        let roll = opts.roll.unwrap_or_else(|| self.rng.float() * std::f64::consts::PI * 2.0);
        let flip = opts.flip.unwrap_or_else(|| self.rng.float() < 0.5);
        let now = self.now;
        let world = self.world.as_deref();
        let add = DecalAdd {
            point: [point.0, point.1, point.2],
            normal: [normal.0, normal.1, normal.2],
            size: opts.size,
            tile: opts.tile,
            roll: Some(roll),
            life: opts.life.or(Some(60.0)),
            fade: opts.fade.or(Some(0.72)),
            opacity: opts.opacity.or(Some(1.0)),
            max_angle: opts.max_angle.or(Some(62.0)),
            // `o.depth = opts.depth ?? Math.max( 0.04, o.size * 0.32 )`
            // (`index.js:578`), against the *resolved* size. This was the one
            // decal option without its `??`, so an unset `depth` fell through
            // to `DecalSystem::add`'s own default — `max( 0.045, size * 0.35 )`
            // (`decals.js:191`), the arm the source only reaches when a caller
            // bypasses this facade. That made the projector box 9.4% thicker
            // (0.35 / 0.32) on every decal type that leaves `depth` unset.
            depth: opts.depth.or(Some(0.04f64.max(opts.size * 0.32))),
            flip,
            // `o.mask = ph?.MASK?.WORLD ?? 0xffff` (`index.js:532`). An
            // earlier draft hardcoded `0xffff` unconditionally, which is the
            // no-physics fallback — with a world bound the source narrows the
            // decal projection to STATIC|PROP (= 3).
            mask: world.map_or(0xffff, |_| mask::WORLD),
            now,
            world: world.map(|w| w as &dyn crate::fx::decals::DecalWorld),
        };
        let ok = self.decals.add(&add);
        if ok {
            self.stats.decals += 1;
        }
        ok
    }

    /// `scorch(x, y, z, radius)`, `index.js:538-565`.
    pub fn scorch(&mut self, x: f64, y: f64, z: f64, radius: f64) {
        let mut px = x;
        let mut py = y;
        let mut pz = z;
        let mut n = (0.0, 1.0, 0.0);
        if let Some(world) = self.world.as_deref() {
            if let Some(hit) = world.raycast((x, y + 0.4, z), (0.0, -1.0, 0.0), radius * 1.5 + 1.0, mask::WORLD) {
                px = hit.point.0;
                py = hit.point.1;
                pz = hit.point.2;
                n = hit.normal;
            }
        }
        self.add_decal(
            (px, py, pz),
            n,
            DecalOpts {
                tile: crate::fx::atlas::d::SCORCH,
                size: radius * 1.05,
                life: Some(120.0),
                fade: Some(0.55),
                opacity: Some(0.9),
                max_angle: Some(80.0),
                depth: Some(radius * 0.35),
                roll: None,
                flip: None,
            },
        );
    }

    /// `bloodSpatterBehind(point, incident)`, `index.js:568-580`.
    pub fn blood_spatter_behind(&mut self, point: (f64, f64, f64), incident: (f64, f64, f64)) {
        let Some(world) = self.world.as_deref() else {
            return;
        };
        let Some(hit) = world.raycast(point, incident, 2.6, mask::WORLD) else {
            return;
        };
        let tile = if self.rng.float() < 0.5 {
            crate::fx::atlas::d::BLOOD_A
        } else {
            crate::fx::atlas::d::BLOOD_B
        };
        let size = self.rng.range(0.32, 0.62);
        let opacity = self.rng.range(0.7, 1.0);
        self.add_decal(
            hit.point,
            hit.normal,
            DecalOpts {
                tile,
                size,
                life: Some(90.0),
                fade: Some(0.8),
                opacity: Some(opacity),
                max_angle: Some(70.0),
                roll: None,
                flip: None,
                depth: None,
            },
        );
    }

    /* =================================================================== */
    /*  muzzle flash — index.js:410-488                                    */
    /* =================================================================== */

    /// `viewFlash(x, y, z, r, g, b, strength)`, `index.js:348-366`.
    ///
    /// `key` is the source's `render?.viewSun?.intensity ?? 2.5`; there is no
    /// renderer to read it from, so it is a parameter with the source's own
    /// fallback as the sensible default.
    #[allow(clippy::too_many_arguments)]
    pub fn view_flash_with_key(&mut self, key: f64, x: f64, y: f64, z: f64, r: f64, g: f64, b: f64, strength: f64) {
        self.attach_view();
        // 0.72 cd per unit of key; 4 cm of lift off the bore axis.
        let peak = (key * 0.72 * clamp(strength, 0.05, 2.2)).max(0.04);
        if let Some(pool) = self.view_lights.as_mut() {
            pool.flash(x, y + 0.04, z, r, g, b, peak, 0.09, 8.0, 1.6, 2.0);
        }
    }

    /// `muzzleFlash(o)`, `index.js:415-445` — the facade's own marshalling
    /// around [`crate::fx::muzzle::muzzle_flash`].
    ///
    /// The light always lives in the world, even when the sprites are drawn in
    /// viewmodel space, because it is the world it has to illuminate.
    pub fn muzzle_flash(&mut self, now: f64, frame: &FxFrame<'_>, o: &FacadeMuzzleOpts) -> MuzzleProfile {
        self.now = now;
        let (pos, dir, light_pos, view) =
            self.resolve_muzzle_frame(frame, o.position, o.direction, o.view, o.view_space);
        let pos = V3::new(pos.0, pos.1, pos.2);
        let dir = V3::new(dir.0, dir.1, dir.2);
        // `screenAngle`'s basis: the right/up rows of the relevant camera's
        // `matrixWorldInverse` (`muzzle.js:43-51`).
        let cam = if view { frame.view_camera } else { frame.camera };
        let m = &cam.matrix_world_inverse.e;
        let opts = MuzzleFlashOpts {
            position: (pos.x, pos.y, pos.z),
            direction: (dir.x, dir.y, dir.z),
            weapon: o.weapon.as_deref(),
            intensity: o.intensity,
            scale: o.scale,
            light: o.light,
            view,
            light_pos: Some(light_pos),
            camera_basis: Some(([m[0], m[4], m[8]], [m[1], m[5], m[9]])),
        };
        muzzle_flash(self, &opts)
    }

    /// The space-resolution half of `muzzleFlash` (`index.js:419-443`) —
    /// `(position, direction, lightPos, view)`, i.e. exactly the four
    /// `_flashArg` fields the source computes before delegating.
    ///
    /// Split out because it is the whole of `_toView`/`_fromView`'s
    /// observable behaviour and it is worth pinning on its own, away from the
    /// several dozen RNG draws `muzzle.js` makes afterwards.
    pub fn resolve_muzzle_frame(
        &self,
        frame: &FxFrame<'_>,
        position: (f64, f64, f64),
        direction: (f64, f64, f64),
        view: bool,
        view_space: bool,
    ) -> ((f64, f64, f64), (f64, f64, f64), (f64, f64, f64), bool) {
        let mut pos = V3::new(position.0, position.1, position.2);
        let mut dir = V3::new(direction.0, direction.1, direction.2);
        let light_pos = if view_space {
            // Caller handed us a point already in viewmodel space: map it back
            // out for the light and leave the sprites where they are.
            from_view(frame, pos)
        } else {
            let lp = pos;
            if view {
                let (p, d) = to_view(frame, pos, dir);
                pos = p;
                dir = d;
            }
            lp
        };
        (
            (pos.x, pos.y, pos.z),
            (dir.x, dir.y, dir.z),
            (light_pos.x, light_pos.y, light_pos.z),
            // `o.view === true || o.viewSpace === true`.
            view || view_space,
        )
    }

    /// `this._viewAttached`.
    pub fn view_attached(&self) -> bool {
        self.view_attached
    }

    /// `this._target` — the surface the last [`FxSystem::debug_burst`] staged
    /// its timeline against.
    pub fn target(&self) -> StageTarget {
        self.target
    }

    /* =================================================================== */
    /*  lighting — index.js:635-642, 823-881                               */
    /* =================================================================== */

    /// `setAmbient(topColor, bottomColor, sunColor)`, `index.js:636-642`. Let
    /// `sky` drive the values smoke and dust are lit with. Any argument may be
    /// `None`; passing all three `None` still latches the override flag,
    /// exactly as the source does.
    pub fn set_ambient(&mut self, top: Option<V3>, bottom: Option<V3>, sun: Option<V3>) {
        if let Some(v) = top {
            self.amb_top = v;
        }
        if let Some(v) = bottom {
            self.amb_bot = v;
        }
        if let Some(v) = sun {
            self.sun_col = v;
        }
        self.ambient_override = true;
    }

    /// `_syncLighting(ctx)`, `index.js:823-872`, minus `_pushLighting`'s
    /// uniform copies (see the module doc).
    fn sync_lighting(&mut self, frame: &FxFrame<'_>) {
        // Sun direction and colour come from whatever light the renderer
        // decided is the sun, so smoke is lit by the same key as the world.
        if let Some(sun_dir) = frame.sun_dir {
            // `.transformDirection(...).normalize()` — both, deliberately.
            self.sun_view = three_normalize(transform_direction(sun_dir, frame.camera.matrix_world_inverse));
            // `sunWorld()` (`index.js:601-610`) reads the SAME `render.sunDir`
            // and is sticky when it is absent, so latch it here rather than
            // making callers push it separately.
            self.sun_world = (sun_dir.x, sun_dir.y, sun_dir.z);
        }
        let mut sun_i = 4.3;
        if let Some((color, intensity)) = frame.active_sun {
            sun_i = intensity;
            if !self.ambient_override {
                self.sun_col = V3::new(color.x * sun_i, color.y * sun_i, color.z * sun_i);
            }
        }
        self.sun_factor = clamp(sun_i / 4.3, 0.0, 1.6);

        if !self.ambient_override {
            // Clear-sky irradiance is roughly a fifth of direct sun, blue
            // above and bounced-warm below.
            let a = clamp(sun_i * 0.22, 0.02, 3.0);
            self.amb_top = V3::new(a * 0.78, a * 0.92, a * 1.25);
            self.amb_bot = V3::new(a * 0.5, a * 0.44, a * 0.38);
        }
        self.up_view = three_normalize(transform_direction(
            V3::new(0.0, 1.0, 0.0),
            frame.camera.matrix_world_inverse,
        ));

        match frame.fog {
            // The source resolves `fog.density ?? 1 / Math.max(1, fog.far ??
            // 400)` at this site; the caller has already done that here.
            Some((color, density)) => self.fog = (color.x, color.y, color.z, density),
            // Note: only `w` is cleared. The rgb keeps whatever it last held.
            None => self.fog.3 = 0.0,
        }

        if self.view_attached {
            // The viewmodel camera has its own basis; recompute against it.
            if let Some(sun_dir) = frame.sun_dir {
                self.sun_view_vm =
                    three_normalize(transform_direction(sun_dir, frame.view_camera.matrix_world_inverse));
            }
            self.up_view_vm = three_normalize(transform_direction(
                V3::new(0.0, 1.0, 0.0),
                frame.view_camera.matrix_world_inverse,
            ));
        }
    }

    /* =================================================================== */
    /*  frame — index.js:776-821                                           */
    /* =================================================================== */

    /// `update(dt, ctx)`, `index.js:780-788`.
    ///
    /// `ambience.update` is absent — see the module doc's `ambience.js` gap.
    /// [`FxSystem::sun_factor`] is still computed and stored, because that is
    /// the one value the source hands it.
    pub fn update(&mut self, dt: f64, now: f64, frame: &FxFrame<'_>) {
        self.now = now;
        // `fx.ctx.camera` is live for the whole frame in the source; this is
        // the one place that fact enters the port. See `camera_basis`.
        let m = &frame.camera.matrix_world_inverse.e;
        self.camera_basis = Some(([m[0], m[4], m[8]], [m[1], m[5], m[9]]));
        self.sync_lighting(frame);
        self.lights.update(dt);
        if let Some(pool) = self.view_lights.as_mut() {
            pool.update(dt);
        }
        self.run_script(dt, frame);
        // `this.ambience.sunFactor = this._sunFactor;`
        // `this.ambience.update( dt, this.now, ctx.camera, ctx.scene );`
        // (`index.js:786-787`) — last in the frame, after `_runScript`, because
        // the script can spawn emitters this same tick.
        //
        // Taken out and put back: see the `ambience` field's doc. `update` needs
        // `&mut FxSystem` for the shared rng stream, which is the whole reason
        // the draw order matches the source.
        let now = self.now;
        self.ambience.take().map(|mut ambience| {
            ambience.sun_factor = self.sun_factor;
            ambience.update(self, dt, now, &frame.camera, frame.scene);
            self.ambience = Some(ambience);
        });
    }

    /// `lateUpdate(dt, ctx)`, `index.js:790-821`. Returns whether this is the
    /// frame the source would have called `prewarmMaterials()` on — the
    /// second one (`if (!this._warmed && ++this._warmTicks > 1)`).
    pub fn late_update(&mut self, dt: f64, now: f64) -> bool {
        self.now = now;
        self.shells.update(dt, self.gravity);
        self.lit.flush(now);
        self.add.flush(now);
        self.motes.flush(now);
        if self.view_attached {
            self.view_add.flush(now);
            self.view_lit.flush(now);
        }
        self.decals.flush(now);
        // `hazeSys.update(now, depth, camera)` — the depth texture and camera
        // are pass state; the CPU half is the layer flush (`haze.js:212-219`).
        self.haze_sys.layer.flush(now);
        self.stats.live = self.add.spawned() + self.lit.spawned();

        // Self-scheduled pre-warm, on the second frame. It cannot run earlier
        // and be useful: the program cache key carries the number of *visible*
        // lights, and the renderer only settles that inside its first rendered
        // frame.
        if !self.warmed {
            self.warm_ticks += 1;
            if self.warm_ticks > 1 {
                self.warmed = true;
                return true;
            }
        }
        false
    }

    /* =================================================================== */
    /*  debug staging — index.js:883-1264                                  */
    /* =================================================================== */

    /// `_runScript(dt)`, `index.js:1241-1256`. Public because it is the whole
    /// of the debug timeline and the only way to drive a staged burst without
    /// also running the (unported) `ambience` half of `update`.
    pub fn run_script(&mut self, dt: f64, frame: &FxFrame<'_>) {
        if self.script.is_empty() {
            return;
        }
        let period = self.script_period;
        let prev = self.script_time;
        let mut now = prev + dt;
        if now < period {
            self.fire(prev, now, frame);
        } else {
            // Fire the tail of this loop and the head of the next one, so a
            // wrap never silently swallows the events that straddle it.
            self.fire(prev, period, frame);
            now -= period;
            self.fire(-1e-6, now, frame);
        }
        self.script_time = now;
    }

    /// `_fire(from, to)`, `index.js:1258-1264`. Half-open on the left:
    /// `e.t > from && e.t <= to`.
    fn fire(&mut self, from: f64, to: f64, frame: &FxFrame<'_>) {
        // The source iterates `this._script` by index while the handlers run.
        // No handler mutates the list, so a snapshot of the actions is
        // equivalent and satisfies the borrow checker.
        let due: Vec<StageAction> = self
            .script
            .iter()
            .filter(|e| e.t > from && e.t <= to)
            .map(|e| e.action)
            .collect();
        for action in due {
            self.run_action(action, frame);
        }
    }

    fn run_action(&mut self, action: StageAction, frame: &FxFrame<'_>) {
        match action {
            StageAction::Boom { side } => {
                // `_tmpA = target.point + normal*1.1 + tangent*(side*1.6)`.
                let t = self.target;
                let p = t
                    .point
                    .add_scaled(t.normal, 1.1)
                    .add_scaled(t.tangent, side * 1.6);
                // `{ position, radius: 3.6, damage: 120 }` — `explode` never
                // reads `damage` (`explosions.js:20-209`), so it has no field
                // on `ExplosionOpts` and is dropped here rather than carried
                // as a value nothing consumes.
                let opts = crate::fx::explosions::ExplosionOpts {
                    position: (p.x, p.y, p.z),
                    radius: 3.6,
                    ..Default::default()
                };
                self.explosion(&opts);
            }
            StageAction::Impact { u, v, surface } => self.impact_at(u, v, surface),
            StageAction::ImpactRandom {
                u_scale,
                v_lo,
                v_hi,
                surface,
            } => {
                // Draw order: `rng.signed()` then `rng.range()`, both at fire
                // time (`index.js:953-955`).
                let u = self.rng.signed() * u_scale;
                let v = self.rng.range(v_lo, v_hi);
                self.impact_at(u, v, surface);
            }
            StageAction::Muzzle => self.stage_muzzle(frame),
            StageAction::Shell => self.stage_shell(frame),
            StageAction::Tracer => self.stage_tracer(frame),
            StageAction::Crossfire => self.stage_crossfire(frame),
        }
    }

    /// `debugBurst(kind)`, `index.js:896-985`. Stages a photogenic looping
    /// timeline for the screenshot harness.
    pub fn debug_burst(&mut self, now: f64, frame: &FxFrame<'_>, kind: BurstKind) -> BurstReport {
        // `'none'`/`'clear'`/`'off'` stops a previously staged loop. The
        // capture harness applies shots back to back in one session, so a
        // burst staged for `impacts` would otherwise still be walking rounds
        // across a wall during every later shot.
        if kind == BurstKind::None {
            self.script.clear();
            self.script_time = 0.0;
            self.script_period = 0.0;
            return BurstReport {
                staged: BurstKind::None,
                at: None,
                surface: None,
            };
        }
        self.now = now;
        let target = self.find_target(frame);
        self.target = target;
        self.script.clear();
        self.script_time = 0.0;
        self.script_period = 1.56;

        let at = |script: &mut Vec<ScriptEntry>, t: f64, action: StageAction| {
            script.push(ScriptEntry { t, action });
        };

        match kind {
            BurstKind::None => unreachable!("handled above"),
            BurstKind::Explosion => {
                // Two detonations per loop, half a period apart.
                self.script_period = 1.1;
                at(&mut self.script, 0.02, StageAction::Boom { side: -1.0 });
                at(&mut self.script, 0.56, StageAction::Boom { side: 1.0 });
                self.stage_wall_hits(6, 0.1, 0.95);
                BurstReport {
                    staged: BurstKind::Explosion,
                    at: Some((target.point.x, target.point.y, target.point.z)),
                    surface: None,
                }
            }
            BurstKind::Muzzle => {
                // Cyclic rate shorter than the flash lifetime.
                self.script_period = 0.44;
                for i in 0..8 {
                    let t = f64::from(i) * 0.055;
                    at(&mut self.script, t, StageAction::Muzzle);
                    if i % 3 == 0 {
                        at(&mut self.script, t + 0.01, StageAction::Shell);
                    }
                    if i % 2 == 0 {
                        at(&mut self.script, t + 0.004, StageAction::Tracer);
                    }
                }
                BurstReport {
                    staged: BurstKind::Muzzle,
                    at: None,
                    surface: None,
                }
            }
            BurstKind::Combat => {
                self.script_period = 1.6;
                self.stage_wall_hits(9, 0.04, 1.2);
                at(
                    &mut self.script,
                    1.3,
                    StageAction::ImpactRandom {
                        u_scale: 0.7,
                        v_lo: -0.2,
                        v_hi: 0.5,
                        surface: Some(Surface::Metal),
                    },
                );
                at(
                    &mut self.script,
                    1.42,
                    StageAction::ImpactRandom {
                        u_scale: 0.6,
                        v_lo: -0.2,
                        v_hi: 0.5,
                        surface: None,
                    },
                );
                at(
                    &mut self.script,
                    1.5,
                    StageAction::ImpactRandom {
                        u_scale: 0.5,
                        v_lo: -0.1,
                        v_hi: 0.6,
                        surface: Some(Surface::Metal),
                    },
                );
                at(&mut self.script, 1.36, StageAction::Tracer);
                at(&mut self.script, 1.44, StageAction::Crossfire);
                at(&mut self.script, 1.12, StageAction::Shell);
                at(&mut self.script, 1.34, StageAction::Shell);
                at(&mut self.script, 1.52, StageAction::Muzzle);
                BurstReport {
                    staged: BurstKind::Combat,
                    at: None,
                    surface: None,
                }
            }
            BurstKind::Impacts => {
                // The cadence matters more than the choreography: rounds land
                // every 50 ms on a 0.9 s loop, strictly shorter than the 75 ms
                // flash lifetime.
                self.script_period = 0.9;
                self.stage_wall_hits(18, 0.0, 0.85);
                at(&mut self.script, 0.30, StageAction::Crossfire);
                at(&mut self.script, 0.74, StageAction::Crossfire);
                at(&mut self.script, 0.18, StageAction::Shell);
                at(&mut self.script, 0.52, StageAction::Shell);
                BurstReport {
                    staged: BurstKind::Impacts,
                    at: Some((target.point.x, target.point.y, target.point.z)),
                    surface: Some(target.surface),
                }
            }
        }
    }

    /// `_stageWallHits(at, target, count, t0, t1)`, `index.js:987-1001`.
    ///
    /// Every RNG draw here happens at *staging* time — the closure the source
    /// pushes captures already-computed `u`/`v`. See [`StageAction`].
    fn stage_wall_hits(&mut self, count: i32, t0: f64, t1: f64) {
        let target = self.target;
        for i in 0..count {
            let f = f64::from(i) / f64::from(count.max(1) - 1).max(1.0);
            let t = t0 + (t1 - t0) * f;
            // A gunner walking rounds across the wall: a broad sweep with a
            // wobble, so the group reads as aimed fire not a scatter plot.
            let su = (target.span_u * 0.88).min(1.35);
            let sv = (target.span_v * 0.7).min(0.36);
            let u = (f * 3.9 + 0.4).sin() * su + self.rng.signed() * su * 0.13;
            let v = (f * 2.4).cos() * sv + self.rng.signed() * sv * 0.35;
            let surface = if i % 3 == 2 { Some(Surface::Metal) } else { None };
            self.script.push(ScriptEntry {
                t,
                action: StageAction::Impact { u, v, surface },
            });
        }
    }

    /// `_impactAt(target, u, v, surfaceOverride)`, `index.js:1004-1027`.
    fn impact_at(&mut self, u: f64, v: f64, surface_override: Option<Surface>) {
        let target = self.target;
        let mut p = target.point.add_scaled(target.tangent, u).add_scaled(target.bitangent, v);
        let mut n = target.normal;
        if target.has_world {
            if let Some(world) = self.world.as_deref() {
                // Re-trace so the hit sits on the real surface (and picks up
                // its normal and material) rather than on the plane through
                // the first hit.
                let origin = p.add_scaled(n, 1.2);
                let d = n.scale(-1.0);
                if let Some(hit) = world.raycast((origin.x, origin.y, origin.z), (d.x, d.y, d.z), 2.6, mask::WORLD) {
                    p = V3::new(hit.point.0, hit.point.1, hit.point.2);
                    n = V3::new(hit.normal.0, hit.normal.1, hit.normal.2);
                }
            }
        }
        let mut d = n.scale(-1.0);
        // Give the incoming round a believable oblique angle. Draw order:
        // x then z (`index.js:1020-1022`).
        d.x += self.rng.signed() * 0.35;
        d.y -= 0.18;
        d.z += self.rng.signed() * 0.35;
        let d = three_normalize(d);
        self.suppress_decals = !target.has_world;
        crate::fx::impacts::spawn_impact(
            self,
            (p.x, p.y, p.z),
            (n.x, n.y, n.z),
            (d.x, d.y, d.z),
            surface_override.unwrap_or(target.surface),
            1.15,
        );
        self.suppress_decals = false;
    }

    /// `_stageMuzzle()`, `index.js:1042-1059`.
    ///
    /// `view: true`, because that is the path a real trigger pull takes and it
    /// is the only path that reaches `viewFlash` — i.e. the only one that
    /// lights the weapon.
    fn stage_muzzle(&mut self, frame: &FxFrame<'_>) {
        let welded = frame.muzzle_world.filter(|m| m.length_sq() > 1e-6);
        let pos = welded.unwrap_or_else(|| V3::new(0.16, -0.13, -0.72).apply_matrix4(frame.camera.matrix_world));
        let dir = transform_direction(V3::new(0.0, 0.0, -1.0), frame.camera.matrix_world);
        let now = self.now;
        self.muzzle_flash(
            now,
            frame,
            &FacadeMuzzleOpts {
                position: (pos.x, pos.y, pos.z),
                direction: (dir.x, dir.y, dir.z),
                weapon: Some("rifle".to_string()),
                view: true,
                ..Default::default()
            },
        );
    }

    /// `_stageShell()`, `index.js:1061-1068`.
    fn stage_shell(&mut self, frame: &FxFrame<'_>) {
        let cam = frame.camera;
        let pos = V3::new(0.2, -0.1, -0.45).apply_matrix4(cam.matrix_world);
        // Draw order: x, y, z.
        let vx = self.rng.range(1.3, 2.1);
        let vy = self.rng.range(1.2, 2.0);
        let vz = self.rng.range(-0.4, 0.4);
        // `applyMatrix4(cam.matrixWorld).sub(cam.position)` — a point
        // transform followed by subtracting the origin, NOT `transformDirection`
        // (no renormalisation, and translation cancels rather than being
        // dropped).
        let vel = V3::new(vx, vy, vz).apply_matrix4(cam.matrix_world).sub(cam.position());
        // `spawnShell` re-reads `ctx.time.elapsed` into `this.now`; here the
        // script runner has already set it for this frame.
        self.spawn_shell((pos.x, pos.y, pos.z), Some((vel.x, vel.y, vel.z)), ShellSpawnOpts::default());
    }

    /// `_stageTracer(target)`, `index.js:1070-1079`.
    fn stage_tracer(&mut self, frame: &FxFrame<'_>) {
        let m = frame.camera.matrix_world;
        let from = V3::new(0.18, -0.12, -0.7).apply_matrix4(m);
        // Fire past the staged surface: a tracer that only travels three
        // metres is over in a sixtieth of a second.
        let tx = self.rng.range(-3.0, 3.0);
        let ty = self.rng.range(-0.6, 1.4);
        let to = V3::new(tx, ty, -46.0).apply_matrix4(m);
        self.tracer((from.x, from.y, from.z), (to.x, to.y, to.z), 250.0);
    }

    /// `_stageCrossfire()`, `index.js:1082-1088` — an incoming round crossing
    /// the frame, so it reads as a firefight rather than a range.
    fn stage_crossfire(&mut self, frame: &FxFrame<'_>) {
        let m = frame.camera.matrix_world;
        let ax = self.rng.range(-14.0, -9.0);
        let ay = self.rng.range(-1.2, 1.4);
        let az = self.rng.range(-16.0, -8.0);
        let from = V3::new(ax, ay, az).apply_matrix4(m);
        let bx = self.rng.range(9.0, 15.0);
        let by = self.rng.range(-1.4, 1.2);
        let bz = self.rng.range(-18.0, -9.0);
        let to = V3::new(bx, by, bz).apply_matrix4(m);
        self.tracer((from.x, from.y, from.z), (to.x, to.y, to.z), 280.0);
    }

    /// `_findTarget()`, `index.js:1094-1239`.
    ///
    /// Two passes over a 9x7 fan of probes: the first records every hit, the
    /// second scores each by how many of the OTHERS lie on the same plane.
    /// Distance-and-centredness alone is not enough — as soon as the level
    /// gains a 12 cm pillar between the camera and the wall, the fan scores
    /// the pillar highest and the whole burst gets walked across a sliver of
    /// geometry 20 px wide.
    pub fn find_target(&mut self, frame: &FxFrame<'_>) -> StageTarget {
        let mut t = self.target;
        let cam_pos = frame.camera.position();
        let mut best: Option<(V3, V3, Surface, f64)> = None;
        let mut best_dist = f64::INFINITY;
        let mut probes: Vec<Probe> = Vec::new();

        let has_geometry = self.world.as_deref().map_or(false, |w| w.tri_count() > 0);
        if has_geometry {
            let world = self.world.as_deref().expect("checked above");
            let axis_x = V3::new(1.0, 0.0, 0.0);
            let axis_y = V3::new(0.0, 1.0, 0.0);
            for i in 0..63i32 {
                let yaw = f64::from(i % 9 - 4) * 0.075;
                let pitch = f64::from(i / 9 - 3) * 0.08;
                // `.applyAxisAngle(_axisX, pitch).applyAxisAngle(_axisY, yaw)`
                // then `transformDirection(cam.matrixWorld)`.
                let d = V3::new(0.0, 0.0, -1.0)
                    .apply_quat(quat_from_axis_angle(axis_x, pitch))
                    .apply_quat(quat_from_axis_angle(axis_y, yaw));
                let d = transform_direction(d, frame.camera.matrix_world);
                let Some(hit) = world.raycast((cam_pos.x, cam_pos.y, cam_pos.z), (d.x, d.y, d.z), 40.0, mask::WORLD)
                else {
                    continue;
                };
                let dist = hit.distance;
                // A grazing hit on a thin prop makes a poor showcase.
                let face = -(d.x * hit.normal.0 + d.y * hit.normal.1 + d.z * hit.normal.2);
                if dist < 1.2 || face < 0.3 {
                    continue;
                }
                // Every field goes through `f32` — `this._probes` is a
                // `Float32Array`. See [`Probe`].
                probes.push(Probe {
                    point: [hit.point.0 as f32, hit.point.1 as f32, hit.point.2 as f32],
                    normal: [hit.normal.0 as f32, hit.normal.1 as f32, hit.normal.2 as f32],
                    distance: dist as f32,
                    cost: ((dist - 5.0).abs() + (yaw.abs() + pitch.abs()) * 7.0 + (1.0 - face) * 3.0) as f32,
                    surface: hit.surface,
                });
            }
            for i in 0..probes.len() {
                let a = probes[i];
                let (a_point, a_normal) = (a.point_v3(), a.normal_v3());
                let pd = a_point.dot(a_normal);
                let mut support = 0i64;
                for (j, c) in probes.iter().enumerate() {
                    if j == i {
                        continue;
                    }
                    if c.normal_v3().dot(a_normal) < 0.96 {
                        continue;
                    }
                    let off = c.point_v3().dot(a_normal) - pd;
                    if off > -0.12 && off < 0.12 {
                        support += 1;
                    }
                }
                // Each co-planar neighbour is worth a metre of framing error:
                // 6+ of them beats a perfectly centred sliver every time.
                let score = f64::from(a.cost) - (support.min(12) as f64) * 1.0;
                if score < best_dist {
                    best_dist = score;
                    self.target_support = support;
                    best = Some((a_point, a_normal, a.surface, f64::from(a.distance)));
                }
            }
        }

        // Real geometry always beats the virtual plane: decals at 20 m still
        // read as bullet holes, a burst with no decals at all does not.
        match best.filter(|(_, _, _, dist)| *dist < 22.0) {
            Some((point, normal, surface, distance)) => {
                t.point = point;
                t.normal = normal;
                t.surface = surface;
                t.has_world = true;
                t.distance = distance;
            }
            None => {
                // Nothing close enough to shoot: stage the burst on a virtual
                // plane in front of the camera and skip decals.
                let fwd = transform_direction(V3::new(0.0, 0.0, -1.0), frame.camera.matrix_world);
                t.point = cam_pos.add_scaled(fwd, 3.2);
                t.normal = fwd.scale(-1.0);
                t.surface = Surface::Concrete;
                t.has_world = false;
                t.distance = 3.2;
            }
        }

        // Tangent frame on the surface, biased so 'up' on the wall is world up.
        let up = if t.normal.y.abs() > 0.9 {
            V3::new(1.0, 0.0, 0.0)
        } else {
            V3::new(0.0, 1.0, 0.0)
        };
        t.bitangent = three_normalize(up.add_scaled(t.normal, -t.normal.dot(up)));
        t.tangent = three_normalize(t.bitangent.cross(t.normal));

        // How big is the thing we picked? Sweeping 2.7 m over a 0.4 m pilaster
        // threw fifteen of eighteen rounds off it and onto whatever was behind.
        t.span_u = 0.35;
        t.span_v = 0.25;
        if !probes.is_empty() && t.has_world {
            let pd = t.point.dot(t.normal);
            let (mut u_min, mut u_max, mut v_min, mut v_max) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
            for p in &probes {
                if p.normal_v3().dot(t.normal) < 0.96 {
                    continue;
                }
                let off = p.point_v3().dot(t.normal) - pd;
                // The source is `if (off < -0.12 || off > 0.12) continue`, i.e.
                // an inclusive band — not the exclusive `> -0.12 && < 0.12`
                // used in the scoring loop twenty lines above it. The two
                // differ on an exactly-0.12 offset; both are transcribed as
                // written.
                if !(-0.12..=0.12).contains(&off) {
                    continue;
                }
                let rel = p.point_v3().sub(t.point);
                let u = rel.dot(t.tangent);
                let v = rel.dot(t.bitangent);
                u_min = u_min.min(u);
                u_max = u_max.max(u);
                v_min = v_min.min(v);
                v_max = v_max.max(v);
            }
            t.span_u = u_max.min(-u_min).max(0.35);
            t.span_v = v_max.min(-v_min).max(0.25);
        }
        self.target = t;
        t
    }

    /// The staged script, for tests and for the capture harness.
    pub fn script(&self) -> &[ScriptEntry] {
        &self.script
    }
    /// `this._scriptPeriod`.
    pub fn script_period(&self) -> f64 {
        self.script_period
    }
    /// `this._scriptTime`.
    pub fn script_time(&self) -> f64 {
        self.script_time
    }

    /// `sunWorld()`, `index.js:601-610`.
    ///
    /// The source reads `render?.sunDir` live and falls back to straight up.
    /// There is no renderer object here, so the direction is pushed in by
    /// [`FxSystem::set_sun_world`] — which is what a `sky` subsystem would
    /// drive, alongside [`FxSystem::set_ambient`]. The default is the
    /// source's own fallback.
    pub fn sun_world(&self) -> (f64, f64, f64) {
        self.sun_world
    }

    /// The write side of [`FxSystem::sun_world`] — not a method in the source,
    /// which reads the renderer directly.
    pub fn set_sun_world(&mut self, dir: (f64, f64, f64)) {
        self.sun_world = dir;
    }

    /// `haze(x, y, z, radius, grow, life, strength, tile)`, `index.js:
    /// 612-614`.
    #[allow(clippy::too_many_arguments)]
    pub fn haze(&mut self, x: f64, y: f64, z: f64, radius: f64, grow: f64, life: f64, strength: f64, tile: usize) {
        let now = self.now;
        let seed = self.rng.float();
        self.haze_sys.emit(now, x, y, z, radius, grow, life, strength, tile, seed);
    }

    /// `hazeRing(x, y, z, radius, grow, life, strength)`, `index.js:617-619`.
    pub fn haze_ring(&mut self, x: f64, y: f64, z: f64, radius: f64, grow: f64, life: f64, strength: f64) {
        let now = self.now;
        let seed = self.rng.float();
        self.haze_sys
            .emit(now, x, y, z, radius, grow, life, strength, crate::fx::atlas::p::RING, seed);
    }

    /// `addSmokeColumn(x, y, z, o)`, `index.js:622-624`.
    ///
    /// No longer a stub: `fx/ambience.js` is ported, so this forwards as the
    /// source does and returns the emitter tag.
    pub fn add_smoke_column(&mut self, x: f64, y: f64, z: f64, opts: SmokeColumnOpts) -> u64 {
        let o = ColumnOpts::from(opts);
        self.ambience
            .as_mut()
            .map_or(0, |a| a.add_column(x, y, z, &o))
    }

    /// `addSmokeSource(position, o)`, `index.js:627-629` — a persistent source;
    /// pass `object` to have it follow a prop.
    pub fn add_smoke_source(&mut self, position: (f64, f64, f64), opts: &SourceOpts) -> u64 {
        self.ambience
            .as_mut()
            .map_or(0, |a| a.add_source(position, opts))
    }

    /// `removeSmokeSource(tag)`, `index.js:631-633`.
    pub fn remove_smoke_source(&mut self, tag: u64) {
        self.ambience.as_mut().map(|a| a.remove(tag));
    }

    /// `viewFlash(x, y, z, r, g, b, strength)` with the source's own
    /// `render?.viewSun?.intensity ?? 2.5` fallback for the key. See
    /// [`FxSystem::view_flash_with_key`].
    #[allow(clippy::too_many_arguments)]
    pub fn view_flash(&mut self, x: f64, y: f64, z: f64, r: f64, g: f64, b: f64, strength: f64) {
        self.view_flash_with_key(2.5, x, y, z, r, g, b, strength);
    }

    /// `onActorDeath(e)`, `index.js:656-700`. `now` is written after the
    /// `!e?.point` guard (`index.js:657-658`), which the caller applies by
    /// only calling this with a point.
    pub fn on_actor_death(&mut self, now: f64, point: (f64, f64, f64)) {
        self.now = now;
        let count = (8.0 * self.pscale).round() as i32 + 3;
        for i in 0..count {
            let (vx, vy, vz) = crate::fx::util::cone(&mut self.rng, 0.0, 1.0, 0.0, 1.4, 0.7);
            let mut s = crate::fx::particles::reset_spawn();
            s.x = point.0;
            s.y = point.1;
            s.z = point.2;
            s.vx = vx * self.rng.range(0.6, 2.4);
            s.vy = vy * self.rng.range(0.4, 1.6);
            s.vz = vz * self.rng.range(0.6, 2.4);
            s.tile = (if i % 2 == 1 { crate::fx::atlas::p::MIST } else { crate::fx::atlas::p::SMOKE_A }) as f64;
            s.size0 = self.rng.range(0.05, 0.1);
            s.size1 = self.rng.range(0.2, 0.4);
            s.size_curve = 0.5;
            s.life = self.rng.range(0.4, 0.8);
            s.drag = 5.0;
            s.gravity = -3.5;
            s.rot = self.rng.float() * 6.283;
            s.r0 = 0.3;
            s.g0 = 0.03;
            s.b0 = 0.026;
            s.r1 = 0.15;
            s.g1 = 0.015;
            s.b1 = 0.013;
            s.alpha = self.rng.range(0.4, 0.75);
            s.alpha_curve = 1.5;
            s.soft = 0.2;
            s.seed = self.rng.float();
            self.emit_lit(&s);
        }
        if let Some(gy) = self.world.as_deref().and_then(|w| w.ground_height(point.0, point.2, point.1 + 1.0)) {
            let tile = if self.rng.float() < 0.5 { crate::fx::atlas::d::BLOOD_A } else { crate::fx::atlas::d::BLOOD_B };
            let size = self.rng.range(0.5, 0.9);
            self.add_decal(
                (point.0, gy, point.2),
                (0.0, 1.0, 0.0),
                DecalOpts {
                    tile,
                    size,
                    life: Some(120.0),
                    fade: Some(0.85),
                    max_angle: Some(80.0),
                    roll: None,
                    flip: None,
                    opacity: None,
                    depth: None,
                },
            );
        }
    }

    /// `onLand(e)`, `index.js:678-712`. `camera_pos`/`ground_from` stand in
    /// for `ctx.camera.position`/the eye-relative ground probe origin the
    /// source computes from it — passed in since there is no camera in this
    /// port yet.
    pub fn on_land(
        &mut self,
        now: f64,
        velocity: f64,
        camera_pos: (f64, f64, f64),
        player_height: f64,
        eye_offset: f64,
    ) {
        // `this.now` is written AFTER the speed gate (`index.js:703-705`), so a
        // soft landing leaves it alone.
        let v = velocity.abs();
        if v < 3.2 {
            return;
        }
        self.now = now;
        let x = camera_pos.0;
        let z = camera_pos.2;
        let mut y = camera_pos.1 - player_height + eye_offset;
        if let Some(gy) = self.world.as_deref().and_then(|w| w.ground_height(x, z, camera_pos.1 + 1.0)) {
            y = gy;
        }
        let strength = clamp((v - 3.0) / 7.0, 0.2, 1.3);
        let count = (7.0 * self.pscale * strength).round() as i32 + 2;
        for _ in 0..count {
            let a = self.rng.float() * 6.283;
            let sp = self.rng.range(0.7, 2.4) * strength;
            let mut s = crate::fx::particles::reset_spawn();
            s.x = x + a.cos() * 0.22;
            s.y = y + 0.03;
            s.z = z + a.sin() * 0.22;
            s.vx = a.cos() * sp;
            s.vy = self.rng.range(0.1, 0.6);
            s.vz = a.sin() * sp;
            s.tile = crate::fx::atlas::p::DUST as f64;
            s.size0 = self.rng.range(0.05, 0.1);
            s.size1 = self.rng.range(0.3, 0.55);
            s.size_curve = 0.45;
            s.life = self.rng.range(0.5, 1.0);
            s.drag = 3.2;
            s.gravity = -0.6;
            s.rot = self.rng.float() * 6.283;
            s.spin = self.rng.signed() * 1.2;
            s.r0 = 0.48;
            s.g0 = 0.44;
            s.b0 = 0.39;
            s.r1 = 0.4;
            s.g1 = 0.37;
            s.b1 = 0.33;
            s.alpha = self.rng.range(0.25, 0.5) * strength;
            s.alpha_curve = 1.6;
            s.soft = 0.3;
            s.turb = 0.05;
            s.turb_freq = 2.0;
            s.seed = self.rng.float();
            self.emit_lit(&s);
        }
    }

    /// `onFootstep(e)`, `index.js:747-774`.
    ///
    /// Note the order: the `rng.float() > 0.55` coin flip happens BEFORE
    /// `this.now = ctx.time.elapsed`, so a suppressed footstep still spends a
    /// draw and still leaves `now` untouched. Both halves of that are
    /// observable and both are pinned.
    pub fn on_footstep(&mut self, now: f64, running: bool, position: (f64, f64, f64)) {
        if !running {
            return;
        }
        if self.rng.float() > 0.55 {
            return;
        }
        self.now = now;
        let mut s = crate::fx::particles::reset_spawn();
        s.x = position.0 + self.rng.signed() * 0.08;
        s.y = position.1 + 0.02;
        s.z = position.2 + self.rng.signed() * 0.08;
        s.vy = self.rng.range(0.1, 0.35);
        s.vx = self.rng.signed() * 0.3;
        s.vz = self.rng.signed() * 0.3;
        s.tile = crate::fx::atlas::p::DUST as f64;
        s.size0 = 0.04;
        s.size1 = self.rng.range(0.18, 0.32);
        s.size_curve = 0.45;
        s.life = self.rng.range(0.4, 0.75);
        s.drag = 3.4;
        s.gravity = -0.5;
        s.rot = self.rng.float() * 6.283;
        s.r0 = 0.46;
        s.g0 = 0.42;
        s.b0 = 0.37;
        s.r1 = 0.4;
        s.g1 = 0.36;
        s.b1 = 0.32;
        s.alpha = self.rng.range(0.1, 0.22);
        s.alpha_curve = 1.7;
        s.soft = 0.25;
        s.seed = self.rng.float();
        self.emit_lit(&s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_never_exceed_the_particle_budget() {
        for budget in [2000u32, 4000, 12000, 24000] {
            let fx = FxSystem::new(1, budget, 128, -19.62);
            let total = fx.lit.capacity + fx.add.capacity + fx.motes.capacity + fx.view_add.capacity + fx.view_lit.capacity;
            assert!(total as f64 <= f64::from(budget) * 1.05, "budget {budget} produced {total}");
        }
    }

    #[test]
    fn decal_capacity_matches_the_configured_budget() {
        for budget in [64usize, 128, 256, 512] {
            let fx = FxSystem::new(1, 4000, budget as u32, -19.62);
            assert_eq!(fx.decals.capacity, budget);
        }
    }

    #[test]
    fn pscale_is_clamped() {
        let low = FxSystem::new(1, 100, 64, -19.62);
        assert!((low.pscale - 0.4).abs() < 1e-9);
        let high = FxSystem::new(1, 100_000, 64, -19.62);
        assert!((high.pscale - 1.25).abs() < 1e-9);
    }

    #[test]
    fn on_impact_dispatches_by_surface() {
        let mut fx = FxSystem::test_instance(1);
        let before = fx.stats.spawned;
        fx.on_impact(
            (0.0, 1.0, 0.0),
            (0.0, 1.0, 0.0),
            None,
            crate::world::palette::Surface::Concrete,
            Some(25.0),
            false,
        );
        assert!(fx.stats.spawned > before);
    }

    #[test]
    fn on_footstep_ignores_a_stationary_step() {
        let mut fx = FxSystem::test_instance(2);
        let before = fx.stats.spawned;
        fx.on_footstep(0.0, false, (0.0, 0.0, 0.0));
        assert_eq!(fx.stats.spawned, before);
    }

    #[test]
    fn add_decal_respects_the_suppress_flag() {
        let mut fx = FxSystem::test_instance(3);
        fx.suppress_decals = true;
        let ok = fx.add_decal(
            (0.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
            DecalOpts {
                tile: 0,
                size: 0.1,
                ..Default::default()
            },
        );
        assert!(!ok);
    }

    /// `o.depth = opts.depth ?? Math.max( 0.04, o.size * 0.32 )`
    /// (`index.js:578`). `depth` was the one decal option this facade forwarded
    /// without its `??`, so an unset depth fell through to `DecalSystem::add`'s
    /// own `Math.max( 0.045, size * 0.35 )` (`decals.js:191`) — the arm the
    /// source only reaches when a caller bypasses the facade — and every
    /// projector came out 9.4% thicker.
    #[test]
    fn an_unset_decal_depth_takes_the_facades_default_not_the_systems() {
        for size in [0.05, 0.15, 0.5, 1.2] {
            let mut fx = FxSystem::test_instance(11);
            assert!(fx.add_decal(
                (0.0, 0.0, 0.0),
                (0.0, 1.0, 0.0),
                DecalOpts { tile: 0, size, ..Default::default() },
            ));
            let placed = fx.decals.placements().iter().find(|p| p.occupied).unwrap();
            let facade = 0.04f64.max(size * 0.32);
            let bypassed = 0.045f64.max(size * 0.35);
            assert!(
                (placed.half_depth - facade).abs() < 1e-12,
                "size {size}: {} != {facade}",
                placed.half_depth
            );
            // And the two really do differ at every size in play, so this test
            // would have caught the original defect.
            assert!((facade - bypassed).abs() > 1e-12, "size {size}");
        }
    }

    /// An explicit depth still wins, exactly as `opts.depth ?? ...` does.
    #[test]
    fn an_explicit_decal_depth_is_forwarded_untouched() {
        let mut fx = FxSystem::test_instance(12);
        assert!(fx.add_decal(
            (0.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
            DecalOpts { tile: 0, size: 0.15, depth: Some(0.9), ..Default::default() },
        ));
        let placed = fx.decals.placements().iter().find(|p| p.occupied).unwrap();
        assert!((placed.half_depth - 0.9).abs() < 1e-12);
    }

    #[test]
    fn spawn_shell_advances_the_ring() {
        let mut fx = FxSystem::test_instance(4);
        fx.spawn_shell((0.0, 1.0, 0.0), None, ShellSpawnOpts::default());
        assert!(fx.shells.alive_count() > 0);
    }
}
