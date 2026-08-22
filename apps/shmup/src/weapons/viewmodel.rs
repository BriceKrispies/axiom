//! Ported from Claude-of-Duty `src/weapons/viewmodel.js:1-1088` — the
//! additive layer stack that drives the held weapon's transform every frame
//! (`class Viewmodel`, `viewmodel.js:100-1083`).
//!
//! **Scope of this slice.** `viewmodel.js` is two things: the *rig* — the
//! additive pose stack (`update`), the solved ADS translation, the per-shot
//! recoil impulse, the clip state machine, the moving-part drive, the
//! collimated-dot solve and the viewmodel FOV — and a *mesh/scene* layer
//! (`addWeapon`'s mesh construction, `shapeMasks`, `_fitSupportHand`,
//! `_bakeContactAOOnWeapon`, the reticle's `CircleGeometry`/`RingGeometry`
//! sprites, `dispose`) whose cosmetic vertex-mask baking calls into
//! `materials.js`'s `bakeMasks`, which is not ported. This port carries the
//! **whole rig**:
//!
//! - [`Viewmodel::update`] — the whole additive stack: base pose (hip/ADS/
//!   sprint/low-ready blend), sway (six layered [`Noise1::fbm`] fields plus a
//!   two-sine breathing cycle), stride bob, the spring-lag layer, recoil +
//!   settle springs, jump/land springs, the keyframed clip offset from
//!   [`crate::weapons::clips`] **and its event dispatch**, the composed rig
//!   transform, the hand solve, the moving-part drive, the reticle solve and
//!   the viewmodel FOV — in the source's order.
//! - [`Viewmodel::ads_pose`] — the solved (not authored) ADS translation:
//!   the sight node lands exactly on the camera axis at `eye_relief` for any
//!   weapon.
//! - [`Viewmodel::add_recoil`] — the physically-parameterised per-shot kick.
//! - [`Viewmodel::solve_hands`] — the per-frame two-bone IK solve for both
//!   arms, including the body-fixed-shoulder-into-rig-space rebasing the
//!   source's comment calls out (`viewmodel.js:930-935`).
//! - [`Viewmodel::update_parts`] / [`Viewmodel::mag_from_hand`] — the
//!   moving-part drive (`viewmodel.js:856-927`) as *numbers*: this port has no
//!   meshes to write, so the transforms the source assigns to
//!   `p.bolt.position`, `p.trigger.rotation.x`, `p.magazine.quaternion`, ...
//!   land in [`PartsState`] instead.
//! - [`Viewmodel::update_reticle`] — the collimator solve
//!   (`viewmodel.js:972-1034`) as numbers, in [`ReticleState`].
//!
//! ### What is deliberately *not* carried, and why
//!
//! - **`addWeapon`'s mesh half** (`viewmodel.js:315-404`) and the reticle's
//!   four sprite geometries (`viewmodel.js:206-233`): geometry construction
//!   against a renderer this port does not have, gated on `bakeMasks`, which
//!   is not ported. `addWeapon`'s *node* half — the `entry` object at
//!   `viewmodel.js:405-434` — **is** carried, as [`WeaponRig`].
//! - **`_fitSupportHand`/`_bakeContactAOOnWeapon`** (`viewmodel.js:460-515`):
//!   both bake vertex colours into meshes. Their one non-cosmetic output is
//!   the per-weapon *fitted* support-hand pose name (`clamp:<id>`);
//!   [`WeaponRig::lhand_pose`] therefore carries the authored `clamp`/`cup`
//!   instead. The golden pins this divergence by name rather than hiding it.
//!
//!   **This gap is now one function wide, and unblocked.** It was originally
//!   two: `weapons::hands` also did not port `Arm::fitToCylinder`. It does now
//!   (`Arm::fit_to_cylinder`, pinned over five cases against `models/rifle.js`'s
//!   real `gripL`/`handguard`, including the re-fit and the unknown-key
//!   fallback). So the only thing still missing is `_fitSupportHand` itself —
//!   the call site here. Porting it is a bounded follow-up, not a blocked one.
//! - **`_updateReticle`'s `lookAt`** (`viewmodel.js:1006`): the reticle
//!   sprite's billboard *orientation*. It is `Object3D.lookAt` on a mesh this
//!   port does not build, and reproducing it faithfully would mean modelling
//!   the anchor's world **position** (which cancels out of every other value
//!   in this file) plus `Matrix4.lookAt`/`Matrix4.extractRotation` — i.e.
//!   widening [`ViewCamera`] purely to orient absent geometry. Everything
//!   `_updateReticle` *decides* — visibility, the on-axis dot position, the
//!   angular size and all four opacities — is ported.
//! - **`muzzleWorld`/`ejectWorld`/`ejectVelocity`/`boreDir`**
//!   (`viewmodel.js:1041-1071`): each reads `w.group.matrixWorld`, and
//!   `Object3D.updateMatrixWorld` composes that against the **anchor's**
//!   world matrix, which the source refreshes from the *renderer's*
//!   scene-graph walk, not from `update`. Their value is therefore a function
//!   of render-loop ordering that does not exist here; porting them without
//!   that loop would pin an ordering this port does not have. They land with
//!   the renderer.
//! - **`trackCamera`/`rigOverride`/`debugFrozen`** (`viewmodel.js:299-303`):
//!   hooks for `weapons/preview.js` and `weapons/index.js`'s debug freeze, not
//!   the runtime rig. [`Viewmodel::update`] always tracks the camera.
//!
//! ## The camera boundary
//!
//! The source reads `ctx.camera.matrixWorld` every frame to copy the world
//! camera's position/orientation onto the viewmodel anchor
//! (`viewmodel.js:636-646`), then decomposes the anchor's *orientation* alone
//! for the lag layer's angular velocity (`viewmodel.js:649-664`). No
//! camera/render subsystem has landed in this port yet, so — following the
//! `WorldProbe` (`audio::spatial`)/`ScreenProjector` (`ui::markers`)
//! precedent — [`Viewmodel::update`] takes the anchor orientation through the
//! narrow [`ViewCamera`] trait rather than a concrete camera type. The
//! anchor *position* is not part of that contract because nothing this slice
//! computes reads it (see the `lookAt` note above). The rig's own output
//! (`rig_pos`/`rig_quat`) is the transform the source composes as a child of
//! that camera anchor (view-model space), not a world transform.
//!
//! ## Field privacy follows the source
//!
//! `viewmodel.js` is a plain JS class: every field is public except the ones
//! it marks `_`-prefixed by convention (`_angVel`, `_prevYaw`, `_handPosL`,
//! ...). This port keeps that split — the animation state the source names
//! without an underscore is `pub` here, the underscore-prefixed working state
//! is private behind accessors. `rng` is the one exception: it is public in
//! the source but handing out `&mut Rng` would let a caller desynchronise the
//! recoil stream, so it stays private (the port recipe's determinism rule).

use crate::rng::Rng;
use crate::weapons::clips::{make_sample_result, Clip, GripNode, SampleResult};
use crate::weapons::defs::WeaponDef;
use crate::weapons::hands::{Arm, ArmOpts, HandPoseName};
use crate::weapons::mathx::{
    clamp, clamp01, damp, lerp, smootherstep, wrap_pi, Noise1, Spring, Spring3,
    NOISE1_DEFAULT_SIZE, TAU,
};
use crate::weapons::models::rifle::RifleModel;
use crate::weapons::rig_math::{Q, V3};

/// The narrow camera contract [`Viewmodel::update`] needs: this frame's
/// world-space camera orientation (Y-up quaternion), the one piece of
/// `ctx.camera`/`anchor` the rig reads. See the module doc's "camera
/// boundary" section.
pub trait ViewCamera {
    fn orientation(&self) -> Q;
}

/// A fixed orientation, for tests and for any caller that already has the
/// quaternion (an app's per-frame camera state) without wanting to define a
/// whole type for it. Mirrors `ui::markers::FixedCamera`.
#[derive(Debug, Clone, Copy)]
pub struct FixedOrientation(pub Q);

impl ViewCamera for FixedOrientation {
    fn orientation(&self) -> Q {
        self.0
    }
}

/// The subset of `s` (`viewmodel.js`'s per-frame input object,
/// `viewmodel.js:624-625`'s doc comment) that [`Viewmodel::update`] actually
/// reads. The doc comment additionally names `crouch`, `empty` and
/// `cycleTime`, but none of the three appears anywhere in `update`'s body —
/// `crouch`/`empty` are dead documented vocabulary (the same pattern
/// `clips.rs` found for `magHand`/`trigger`), and the `cycleTime` the source
/// reads inside `_updateParts` is `w.def.cycleTime`, not `s.cycleTime`. This
/// struct carries only the six fields the ported `update` touches.
///
/// (`_updateParts` also *takes* `s` and never reads it — `viewmodel.js:856`.
/// [`Viewmodel::update_parts`] therefore does not take it either; the dead
/// parameter is recorded here rather than reproduced.)
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameInput {
    pub ads: bool,
    pub sprint: bool,
    pub low_ready: bool,
    pub speed: f64,
    pub airborne: bool,
    pub trigger: bool,
}

/// `model.nodes.opticGlass` — the two fields `_updateReticle` reads off it
/// (`viewmodel.js:982`, `:996`). Built by `weapons::parts::optics`'
/// `build_optic`/`build_mini_reflex`; both always supply `apertureR`, so the
/// source's `optic.apertureR ?? 0.01` fallback (`viewmodel.js:996`) is dead
/// and is not modelled as an `Option`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpticNode {
    pub center: [f64; 3],
    pub aperture_r: f64,
}

/// One weapon's built `entry` (`addWeapon`, `viewmodel.js:405-434`) minus
/// every *mesh* field (`group`, `meshes`, `parts`, `tris`, `model`) — see the
/// module doc. `clips` is not carried either: [`Viewmodel::play`] takes a
/// built [`Clip`] directly, so the caller owns
/// [`crate::weapons::clips::build_clips`]'s output.
///
/// `f64` throughout, matching the rig (see `weapons::rig_math`'s module doc
/// for why the rig integrates in `f64` while `weapons::models` authors mesh
/// geometry in `f32`).
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponRig {
    /// `model.id` — read by `setActive`'s pistol fallback
    /// (`viewmodel.js:532`) and `_solveHands`' (`viewmodel.js:949`).
    pub id: &'static str,
    pub def: &'static WeaponDef,
    /// `nodes.sight`, weapon space. `viewmodel.js:415`.
    pub sight: V3,
    /// `nodes.ironSight ?? nodes.sight`. `viewmodel.js:416`. Nothing in the
    /// rig reads it — it is `addWeapon` output for the (not-yet-ported)
    /// iron-sight ADS path. Carried because dead computation in the source is
    /// still part of the source.
    pub iron_sight: V3,
    /// `nodes.muzzle`. `viewmodel.js:417`. Read only by `muzzleWorld`
    /// (out of scope — see the module doc).
    pub muzzle: V3,
    /// `nodes.eject`. `viewmodel.js:418`. Read only by `ejectWorld`.
    pub eject: V3,
    /// `nodes.ejectDir ?? [1, 0.4, 0.2]`, **normalised**
    /// (`viewmodel.js:419`). Read only by `ejectVelocity`.
    pub eject_dir: V3,
    /// `nodes.opticGlass ?? null`. `viewmodel.js:420`.
    pub optic: Option<OpticNode>,
    /// `nodes.magSeat.pos`. `viewmodel.js:421`.
    pub mag_seat_pos: V3,
    /// `new Quaternion().setFromEuler(new Euler().fromArray(nodes.magSeat.rot))`
    /// (`viewmodel.js:422-424`) — `THREE.Euler`'s default order is `'XYZ'`.
    pub mag_seat_quat: Q,
    pub grip_r: GripNode,
    pub grip_l: GripNode,
    /// `nodes.chargePull ?? [0, 0, 0]`. `viewmodel.js:427`.
    pub charge_pull: V3,
    /// `nodes.boltTravel ?? [0, 0, 0]`. `viewmodel.js:428`.
    pub bolt_travel: V3,
    /// `nodes.slideTravel ?? [0, 0, 0]`. `viewmodel.js:429`.
    pub slide_travel: V3,
    /// `nodes.triggerPull ?? -0.3`. `viewmodel.js:430`.
    pub trigger_pull: f64,
    /// `model.magSize?.len ?? 0.2`. `viewmodel.js:431`.
    pub mag_len: f64,
    /// `w.lhandPose` (`viewmodel.js:433`, overwritten by `_fitSupportHand` at
    /// `viewmodel.js:469`). The source's per-weapon *fitted* override
    /// (`clamp:<id>`) is out of scope — see the module doc — so this is
    /// always the authored pose (`cup` for the pistol, `clamp` otherwise).
    pub lhand_pose: HandPoseName,

    /* -- moving-part seats. `None` == the model has no such assembly, which
          is exactly what `_updateParts`' `if (p.bolt)` / `if (p.slide)` /
          `if (p.charging)` guards test (`viewmodel.js:870-898`). -- */
    /// `nodes.boltRest.pos`, seated at `viewmodel.js:400`.
    pub bolt_rest: Option<V3>,
    /// `nodes.slideRest.pos`, seated at `viewmodel.js:401`.
    pub slide_rest: Option<V3>,
    /// `nodes.chargeRest.pos`, seated at `viewmodel.js:399`.
    pub charge_rest: Option<V3>,
    /// `parts.trigger` exists (`viewmodel.js:893`).
    pub has_trigger: bool,
    /// `parts.selector` exists (`viewmodel.js:896`).
    pub has_selector: bool,
    /// `parts.magazine` exists (`viewmodel.js:901`).
    pub has_magazine: bool,
}

impl WeaponRig {
    /// `addWeapon(model, def)`'s node half (`viewmodel.js:405-434`) for the
    /// rifle. The smg and pistol get their own converters when a consumer
    /// needs them — the three models return three differently-shaped `nodes`
    /// objects (`RifleNodes`/`SmgNodes`/`PistolNodes`) with no common Rust
    /// trait, exactly as the source's untyped `model.nodes` has no shared
    /// shape either.
    ///
    /// Every `[f32; 3]` node widens to `f64` here: `weapons::models` authors
    /// geometry in `f32` (see its module doc) while the rig integrates in
    /// `f64` like the source. The widening is the seam, and it is here rather
    /// than smeared through the rig.
    pub fn from_rifle(model: &RifleModel, def: &'static WeaponDef) -> WeaponRig {
        let n = &model.nodes;
        WeaponRig {
            id: model.id,
            def,
            sight: v3f(n.sight),
            iron_sight: v3f(n.iron_sight),
            muzzle: v3f(n.muzzle),
            eject: v3f(n.eject),
            eject_dir: v3f(n.eject_dir).normalize(),
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
            // The rifle has no slide; `nodes.slideTravel ?? [0,0,0]`.
            slide_travel: V3::ZERO,
            trigger_pull: f64::from(n.trigger_pull),
            mag_len: f64::from(model.mag_size.len),
            lhand_pose: HandPoseName::Clamp,
            bolt_rest: Some(v3f(n.bolt_rest.pos)),
            slide_rest: None,
            charge_rest: Some(v3f(n.charge_rest.pos)),
            has_trigger: true,
            has_selector: true,
            has_magazine: true,
        }
    }
}

/// `[f32; 3]` node -> `f64` [`V3`].
fn v3f(a: [f32; 3]) -> V3 {
    V3::new(f64::from(a[0]), f64::from(a[1]), f64::from(a[2]))
}

/// `nodes.gripR`/`nodes.gripL` widened to the rig's `f64`. `finger`/`back`
/// become `Some` because every shipped model authors them; the `??` defaults
/// at `viewmodel.js:940`/`:947-948` stay modelled as `None` for a model that
/// does not.
fn grip_node(g: crate::weapons::models::GripTarget) -> GripNode {
    GripNode {
        pos: [f64::from(g.pos[0]), f64::from(g.pos[1]), f64::from(g.pos[2])],
        finger: Some([
            f64::from(g.finger[0]),
            f64::from(g.finger[1]),
            f64::from(g.finger[2]),
        ]),
        back: Some([f64::from(g.back[0]), f64::from(g.back[1]), f64::from(g.back[2])]),
    }
}

/// One `onClipEvent(name, clipName)` dispatch (`viewmodel.js:804`). The
/// source hands the beat to a caller-installed callback; a `&mut self`
/// callback field would infect every method's signature in Rust, so
/// [`Viewmodel::update`] queues the frame's beats instead and the caller
/// drains [`Viewmodel::clip_events`] after stepping. Same beats, same order,
/// same frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FiredClipEvent {
    /// `ev.name`.
    pub name: &'static str,
    /// `c.name` — the clip the beat belongs to.
    pub clip: &'static str,
}

/// `_updateParts`' output (`viewmodel.js:856-913`) as numbers: the transforms
/// the source assigns to the moving-part `Object3D`s this port does not
/// build. A `None` field is a part the active weapon does not have.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartsState {
    /// `boltOff` (`viewmodel.js:868`) — the shared 0..1 bolt/slide travel.
    pub bolt_off: f64,
    /// `p.bolt.position`.
    pub bolt_pos: Option<V3>,
    /// `p.slide.position`.
    pub slide_pos: Option<V3>,
    /// `p.charging.position`.
    pub charge_pos: Option<V3>,
    /// `p.trigger.rotation.x`.
    pub trigger_rot_x: Option<f64>,
    /// `p.selector.rotation.x`.
    pub selector_rot_x: Option<f64>,
    /// `p.magazine.visible` (== `this.magVisible`).
    pub mag_visible: bool,
    /// `p.magazine.position`.
    pub mag_pos: Option<V3>,
    /// `p.magazine.quaternion`.
    pub mag_quat: Option<Q>,
}

impl Default for PartsState {
    fn default() -> Self {
        PartsState {
            bolt_off: 0.0,
            bolt_pos: None,
            slide_pos: None,
            charge_pos: None,
            trigger_rot_x: None,
            selector_rot_x: None,
            // `this.magVisible = true` (`viewmodel.js:278`).
            mag_visible: true,
            mag_pos: None,
            mag_quat: None,
        }
    }
}

/// `_updateReticle`'s output (`viewmodel.js:972-1034`) as numbers.
///
/// The source's three early returns set `this.reticle.visible = false` and
/// leave the position/scale/opacities at *last frame's* values; this struct
/// reproduces that exactly — only [`ReticleState::visible`] is written on an
/// early return.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReticleState {
    pub visible: bool,
    /// `this.reticle.position` — the collimated dot in camera/anchor space.
    pub position: V3,
    /// `coreR` (`viewmodel.js:1023`) — the uniform scale applied to all four
    /// reticle elements, which are authored at unit radius.
    pub core_scale: f64,
    /// `dotCore.material.opacity` = `alpha`.
    pub core_opacity: f64,
    /// `dotRim.material.opacity` = `alpha * 0.8`.
    pub rim_opacity: f64,
    /// `dotRing.material.opacity` = `alpha`.
    pub ring_opacity: f64,
    /// `dotHalo.material.opacity` = `alpha * 0.06`.
    pub halo_opacity: f64,
}

impl Default for ReticleState {
    fn default() -> Self {
        ReticleState {
            // `THREE.Object3D.visible` defaults to `true`; the first
            // `_updateReticle` call overwrites it either way.
            visible: true,
            position: V3::ZERO,
            // The four sprites are authored at unit radius and start
            // unscaled (`Object3D.scale` defaults to 1).
            core_scale: 1.0,
            // The four sprite materials' authored opacities, before the first
            // `_updateReticle` writes over them.
            //
            // READ THE FACTORY, DO NOT INFER IT FROM THE CALL SITE. The three
            // additive elements are built by `mats.reticle(color, intensity)`
            // (`viewmodel.js:217-220`), whose second argument looks like an
            // opacity and is not: `materials.js:1154-1163` multiplies the
            // *colour* by `intensity` and sets `opacity: 1` flat. Only
            // `mats.reticleOutline(0.85)` takes an opacity
            // (`materials.js:1135-1146`). This block first shipped with 0.95 /
            // 0.34 / 0.475 read off the call site, and the golden caught all
            // three at frame 0 — these values are not decoration, they are
            // what every frame before the reticle is first visible reports,
            // because `_updateReticle`'s early returns leave them untouched.
            core_opacity: 1.0,
            rim_opacity: 0.85,
            ring_opacity: 1.0,
            halo_opacity: 1.0,
        }
    }
}

/// `NOISE_RATES` — `noiseRates`, `viewmodel.js:259`.
const NOISE_RATES: [f64; 6] = [0.13, 0.19, 0.271, 0.083, 0.117, 0.163];

/// `const fovBase = 60` (`viewmodel.js:846`) and the viewmodel camera's
/// authored starting FOV (`core/engine.js:35`:
/// `new THREE.PerspectiveCamera(60, 1, 0.005, 12)`) — the same number, so
/// [`Viewmodel::view_fov`] starts on it and the `> 1e-3` dead-zone at
/// `viewmodel.js:848` behaves as it does in the game from frame one.
const FOV_BASE: f64 = 60.0;

/// `Math.hypot(x, y)` — **not** `(x * x + y * y).sqrt()`.
///
/// The port recipe names this trap: `Math.hypot` scales by the largest
/// magnitude first and sums with Kahan compensation, so it rounds
/// differently. Transcribed from V8's `MathHypot` and **verified** against
/// Node 24's `Math.hypot` over 200 000 random pairs in the same magnitude
/// range as the reticle's use: zero mismatches for this algorithm, 38%
/// mismatches for the naive form. The check lives in
/// `tests/weapons_viewmodel/capture.mjs` (`hypotCheck`) so it re-runs
/// whenever the golden is regenerated.
fn js_hypot2(x: f64, y: f64) -> f64 {
    let ax = x.abs();
    let ay = y.abs();
    // `for (arg of args) if (abs > max) max = abs`, starting from 0.
    let max = if ay > ax { ay } else { ax };
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    let mut compensation = 0.0f64;
    for a in [ax, ay] {
        let n = a / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    max * sum.sqrt()
}

/// `handBasis(out, finger, back)`. `viewmodel.js:88-98`. Right-handed hand
/// basis from a finger direction and a back-of-hand direction — the weapon
/// grip nodes' `finger`/`back` triples feed straight into this.
fn hand_basis(finger: V3, back: V3) -> Q {
    let bz = finger.scale(-1.0).normalize(); // hand +Z
    let mut by = back.sub(bz.scale(back.dot(bz)));
    if by.length_sq() < 1e-8 {
        by = V3::new(0.0, 1.0, 0.0).sub(bz.scale(bz.y));
    }
    by = by.normalize(); // hand +Y
    let bx = by.cross(bz).normalize(); // hand +X
    Q::from_basis(bx, by, bz)
}

/// `class Viewmodel` (rig subset — see module doc). `viewmodel.js:100-1083`.
#[derive(Debug, Clone)]
pub struct Viewmodel {
    /// `ctx.rng.fork()` (`viewmodel.js:104`). Private — see the module doc's
    /// "field privacy" note.
    rng: Rng,

    pub arm_r: Arm,
    pub arm_l: Arm,
    /// Body-fixed shoulders, in camera/anchor space. `viewmodel.js:165-166`.
    pub shoulder_r: V3,
    pub shoulder_l: V3,

    active: Option<WeaponRig>,

    pub ads_t: f64,
    pub ads_target: f64,
    pub sprint_t: f64,
    pub low_ready_t: f64,
    pub bob_phase: f64,
    /// `this.stepT` (`viewmodel.js:244`) — initialised and never read or
    /// written again anywhere in the source. Dead state, carried because
    /// dead computation in the source is still part of the source.
    pub step_t: f64,
    pub noise_t: f64,
    pub trigger_t: f64,
    pub trigger_target: f64,

    pub lag: Spring3,
    pub lag_rot: Spring3,
    pub rec_pos: Spring3,
    pub rec_rot: Spring3,
    pub jump_spring: Spring,
    pub land_spring: Spring,
    pub settle: Spring3,

    pub noise: [Noise1; 6],

    ang_vel_yaw: f64,
    ang_vel_pitch: f64,
    prev_yaw: f64,
    prev_pitch: f64,
    has_prev: bool,

    clip: Option<Clip>,
    pub clip_t: f64,
    pub clip_prev_t: f64,
    pub clip_result: SampleResult,
    /// The beats `update` dispatched this frame — the port's stand-in for
    /// `this.onClipEvent` (see [`FiredClipEvent`]). Cleared at the top of
    /// every [`Viewmodel::update`].
    clip_events: Vec<FiredClipEvent>,

    /// `boltCycle` (`viewmodel.js:274`) — 0..1, set to 1 by
    /// [`Viewmodel::add_recoil`] and decayed by [`Viewmodel::update_parts`].
    pub bolt_cycle: f64,
    /// `boltHold` (`viewmodel.js:275`) — "1 = locked back (empty)". Written
    /// only by the constructor and `setActive`, both to `0`; nothing in the
    /// source ever sets it to 1, so `boltOff`'s `Math.max(stroke, boltHold,
    /// clipBolt * boltHold)` (`viewmodel.js:868`) always reduces to `stroke`
    /// and the clip's authored `parts.bolt` track is multiplied away. Ported
    /// as written — this is a live source defect, pinned by name in
    /// `tests/weapons_viewmodel_port.rs`.
    pub bolt_hold: f64,
    /// `magInHand` (`viewmodel.js:276`) — initialised and reset by
    /// `setActive`, never otherwise written; `_updateParts` uses a *local*
    /// `inHand` instead (`viewmodel.js:902`). Dead state, carried.
    pub mag_in_hand: f64,
    /// `magVisible` (`viewmodel.js:277`) — this one *is* live, written every
    /// frame by `_updateParts` (`viewmodel.js:903`).
    pub mag_visible: bool,

    /// `this.anchor.quaternion` (`viewmodel.js:641`) — the camera orientation
    /// this frame, kept because `_updateReticle` reads it back.
    anchor_quat: Q,

    /// `_handPos`/`_handQuat` (`viewmodel.js:285-286`) — the shooting hand's
    /// weapon-space target, written by `_solveHands`.
    hand_pos: V3,
    hand_quat: Q,
    /// `_handPosL`/`_handQuatL` (`viewmodel.js:288-289`) — the support hand's
    /// target. `_magFromHand` reads these back, which is what makes the
    /// magazine follow the hand during a reload.
    hand_pos_l: V3,
    hand_quat_l: Q,
    /// The pose `_solveHands` selected for the support hand this frame
    /// (`viewmodel.js:949-955`).
    lhand_pose: HandPoseName,

    /// The rig's own transform this frame — a child of the camera anchor, so
    /// this is view-model space, not world space. `this.rig.position`/
    /// `.quaternion`, written at the end of [`Viewmodel::update`]
    /// (`viewmodel.js:819-826`).
    rig_pos: V3,
    rig_quat: Q,

    parts: PartsState,
    reticle: ReticleState,
    /// `ctx.viewCamera.fov` (`viewmodel.js:848-851`).
    view_fov: f64,
}

impl Viewmodel {
    /// `constructor(ctx, mats)` (rig subset). `viewmodel.js:101-305`.
    /// `rng` is forked from the caller's, exactly as `ctx.rng.fork()`
    /// (`viewmodel.js:104`) — every `Noise1` table draw below consumes from
    /// that forked stream, in the source's order, so the RNG contract in the
    /// port recipe (fork order + draw order are part of determinism) holds.
    ///
    /// **One determinism caveat, stated rather than hidden:** in the running
    /// game `mats.lib.bakeMasks` exists, so `addWeapon` and
    /// `Arm.bakeSurfaceMasks` draw from this same forked stream before any
    /// shot is fired (`viewmodel.js:158-162`, `:344`). `bakeMasks` is not
    /// ported (it is mesh vertex-colour work — see the module doc), so
    /// neither this port nor the golden capture makes those draws, and both
    /// therefore agree with each other and with a `mats.lib`-less original.
    /// Wiring `bakeMasks` up later will shift `add_recoil`'s jitter stream and
    /// must be done together with re-capturing this slice's golden.
    pub fn new(rng: &mut Rng) -> Self {
        let mut rng = rng.fork();
        // `for (i=0;i<6;i++) this.noise.push(new Noise1(this.rng, 512));`
        // `viewmodel.js:257-258` — six draws from the forked stream, in order.
        let noise: [Noise1; 6] =
            std::array::from_fn(|_| Noise1::new(&mut rng, NOISE1_DEFAULT_SIZE));

        // `viewmodel.js:130-148`: shoulder placement + starting pose per arm,
        // carried verbatim including the long comment there about why the
        // shoulders stay behind the eye and the reach is bought by the bone
        // cheat rather than blading the shoulder forward.
        let arm_r = Arm::new(
            1.0,
            ArmOpts {
                scale: 1.0,
                shoulder_x: 0.205,
                shoulder_y: -0.2,
                shoulder_z: 0.06,
                pose: HandPoseName::Grip,
                ..ArmOpts::default()
            },
        );
        let arm_l = Arm::new(
            -1.0,
            ArmOpts {
                scale: 0.97,
                shoulder_x: 0.2,
                shoulder_y: -0.22,
                shoulder_z: 0.02,
                pose: HandPoseName::Clamp,
                ..ArmOpts::default()
            },
        );

        Viewmodel {
            rng,
            arm_r,
            arm_l,
            shoulder_r: V3::new(0.205, -0.2, 0.06),
            shoulder_l: V3::new(-0.2, -0.22, 0.02),
            active: None,
            ads_t: 0.0,
            ads_target: 0.0,
            sprint_t: 0.0,
            low_ready_t: 0.0,
            bob_phase: 0.0,
            step_t: 0.0,
            noise_t: 0.0,
            trigger_t: 0.0,
            trigger_target: 0.0,
            // `viewmodel.js:249-255`.
            lag: Spring3::new(5.4, 0.46),
            lag_rot: Spring3::new(6.2, 0.42),
            rec_pos: Spring3::new(9.0, 0.42),
            rec_rot: Spring3::new(9.0, 0.42),
            jump_spring: Spring::new(5.5, 0.5, 0.0),
            land_spring: Spring::new(7.5, 0.55, 0.0),
            settle: Spring3::new(2.2, 0.7),
            noise,
            ang_vel_yaw: 0.0,
            ang_vel_pitch: 0.0,
            prev_yaw: 0.0,
            prev_pitch: 0.0,
            has_prev: false,
            clip: None,
            clip_t: 0.0,
            clip_prev_t: 0.0,
            clip_result: make_sample_result(),
            clip_events: Vec::new(),
            bolt_cycle: 0.0,
            bolt_hold: 0.0,
            mag_in_hand: 0.0,
            mag_visible: true,
            anchor_quat: Q::IDENTITY,
            hand_pos: V3::ZERO,
            hand_quat: Q::IDENTITY,
            hand_pos_l: V3::ZERO,
            hand_quat_l: Q::IDENTITY,
            lhand_pose: HandPoseName::Clamp,
            rig_pos: V3::ZERO,
            rig_quat: Q::IDENTITY,
            parts: PartsState::default(),
            reticle: ReticleState::default(),
            view_fov: FOV_BASE,
        }
    }

    /// `setActive(id)` (rig subset — no `w.group.visible` toggle to make).
    /// `viewmodel.js:517-534`.
    ///
    /// Re-selecting the weapon already in hand is a **no-op**, exactly as the
    /// source's `if (!w || w === this.active) return this.active`
    /// (`viewmodel.js:519`) makes it — the recoil springs must not reset
    /// mid-burst. The source's return value is the (possibly unchanged)
    /// active weapon, which every caller already holds; nothing is returned
    /// here, and [`Viewmodel::active`] reads it back.
    pub fn set_active(&mut self, weapon: WeaponRig) {
        if self.active.as_ref().is_some_and(|a| a.id == weapon.id) {
            return;
        }
        self.rec_pos.reset();
        self.rec_rot.reset();
        self.settle.reset();
        self.bolt_cycle = 0.0;
        self.bolt_hold = 0.0;
        self.mag_in_hand = 0.0;
        self.mag_visible = true;
        self.arm_r.set_pose(HandPoseName::Grip);
        // The FITTED clamp for this weapon in the source; the authored one
        // here (see `WeaponRig::lhand_pose`).
        self.arm_l.set_pose(weapon.lhand_pose);
        self.lhand_pose = weapon.lhand_pose;
        self.active = Some(weapon);
    }

    pub fn active(&self) -> Option<&WeaponRig> {
        self.active.as_ref()
    }

    /// The rig's transform this frame (view-model space — see module doc).
    pub fn rig_pose(&self) -> (V3, Q) {
        (self.rig_pos, self.rig_quat)
    }

    /// The lag layer's low-passed, clamped angular velocity — `this._angVel`
    /// (`viewmodel.js:261`), exposed so the clamp behaviour is directly
    /// assertable rather than only inferable from the composed pose.
    pub fn ang_vel(&self) -> (f64, f64) {
        (self.ang_vel_yaw, self.ang_vel_pitch)
    }

    pub fn ads_t(&self) -> f64 {
        self.ads_t
    }

    /// `_updateParts`' output this frame.
    pub fn parts(&self) -> &PartsState {
        &self.parts
    }

    /// `_updateReticle`'s output this frame.
    pub fn reticle(&self) -> &ReticleState {
        &self.reticle
    }

    /// `ctx.viewCamera.fov` after this frame's write (`viewmodel.js:846-851`).
    pub fn view_fov(&self) -> f64 {
        self.view_fov
    }

    /// The clip beats dispatched during the most recent
    /// [`Viewmodel::update`], in the order the source's `for (const ev of
    /// c.events)` loop would have called `onClipEvent`.
    pub fn clip_events(&self) -> &[FiredClipEvent] {
        &self.clip_events
    }

    /// The shooting hand's weapon-space target this frame (`_handPos`,
    /// `_handQuat`).
    pub fn hand_target_r(&self) -> (V3, Q) {
        (self.hand_pos, self.hand_quat)
    }

    /// The support hand's weapon-space target this frame (`_handPosL`,
    /// `_handQuatL`) — the value `_magFromHand` reads back.
    pub fn hand_target_l(&self) -> (V3, Q) {
        (self.hand_pos_l, self.hand_quat_l)
    }

    /// The support-hand pose `_solveHands` selected this frame.
    pub fn lhand_pose(&self) -> HandPoseName {
        self.lhand_pose
    }

    /// `play(name)`. `viewmodel.js:540-549`. Returns the clip's duration, as
    /// the source does (`0` when there is no active weapon — modelled here as
    /// "no weapon, no play", since the caller supplies the clip).
    pub fn play(&mut self, clip: Clip) -> f64 {
        if self.active.is_none() {
            return 0.0;
        }
        let duration = clip.duration;
        self.clip_t = 0.0;
        self.clip_prev_t = -1.0;
        self.clip = Some(clip);
        duration
    }

    /// `stopClip()`. `viewmodel.js:551-555`.
    pub fn stop_clip(&mut self) {
        self.clip = None;
        self.clip_result.active = false;
        self.clip_result.lhand.weight = 0.0;
    }

    /// `get clipPlaying()`. `viewmodel.js:557-559`.
    pub fn clip_playing(&self) -> bool {
        self.clip.is_some()
    }

    /// `get clipName()`. `viewmodel.js:561-563`.
    pub fn clip_name(&self) -> Option<&'static str> {
        self.clip.as_ref().map(|c| c.name)
    }

    /// The solved (not authored) ADS pose: the translation that puts
    /// `sight` exactly on the camera axis at `eye_relief`, and the cant
    /// orientation. `viewmodel.js:709-718`'s `if (ads > 1e-4)` body,
    /// factored out to a pure function so it is testable in isolation of the
    /// rest of the additive stack. `adsPos = (0,0,-eyeRelief) -
    /// (sight · adsQuat)`.
    pub fn ads_pose(sight: V3, eye_relief: f64, ads_cant: [f64; 3]) -> (V3, Q) {
        let ads_quat = Q::from_euler_xyz(ads_cant[0], ads_cant[1], ads_cant[2]);
        let sight_local = sight.apply_quat(ads_quat);
        let ads_pos = V3::new(0.0, 0.0, -eye_relief).sub(sight_local);
        (ads_pos, ads_quat)
    }

    /// Per-shot viewmodel kick. `addRecoil(pitch, yaw, first)`.
    /// `viewmodel.js:574-608`.
    pub fn add_recoil(&mut self, pitch: f64, yaw: f64, first: bool) {
        let Some(weapon) = self.active.clone() else { return };
        let r = weapon.def.recoil;
        let ads = self.ads_t;
        let scale = lerp(1.0, 0.54, ads) * if first { 1.18 } else { 1.0 };
        let jitter = 0.86 + self.rng.float() * 0.3;
        self.rec_pos.set_f(r.freq);
        self.rec_pos.set_z(r.damping);
        self.rec_rot.set_f(r.freq * 0.92);
        self.rec_rot.set_z(r.damping);
        // A velocity impulse of v0 on a spring of angular frequency w peaks
        // at roughly v0/w, so the kick amplitudes below are real
        // metres/radians. `viewmodel.js:588-599`.
        let wp = TAU * self.rec_pos.f();
        let wr = TAU * self.rec_rot.f();
        self.rec_pos.kick(
            self.rng.signed() * r.kick_back * 0.2 * scale * wp,
            r.kick_up * scale * jitter * wp,
            r.kick_back * scale * jitter * wp,
        );
        self.rec_rot.kick(
            (pitch * 5.5 + r.pitch * 1.4) * scale * jitter * wr,
            (-yaw * 4.5 - self.rng.signed() * r.yaw * 0.8) * scale * wr,
            (self.rng.signed() * 0.4 + 0.6) * r.roll * scale * wr,
        );
        let ws = TAU * self.settle.f();
        self.settle.kick(
            self.rng.signed() * 0.0012 * scale * ws,
            0.0018 * scale * ws,
            self.rng.signed() * 0.003 * scale * ws,
        );
        self.bolt_cycle = 1.0;
    }

    /// `jump()`. `viewmodel.js:610-612`.
    pub fn jump(&mut self) {
        self.jump_spring.kick(-1.2);
    }

    /// `land(speed = 3)`. `viewmodel.js:614-616`.
    pub fn land(&mut self, speed: f64) {
        self.land_spring.kick(clamp(speed * 0.45, 0.4, 3.4));
    }

    /// `update(dt, s)`. `viewmodel.js:627-852`.
    pub fn update(&mut self, dt: f64, s: &FrameInput, camera: &dyn ViewCamera) {
        self.clip_events.clear();
        let Some(weapon) = self.active.clone() else { return };
        let def = weapon.def;
        // `dt = dt>0 ? (dt<0.1?dt:0.1) : 0`. `viewmodel.js:633`.
        let dt = clamp(dt, 0.0, 0.1);

        /* -------- camera-relative anchor -------------------------------- */
        // `this.anchor.quaternion.setFromRotationMatrix(cam.matrixWorld)`
        // (`viewmodel.js:641`). The anchor *position* copy on the line above
        // is not modelled — see the module doc's camera-boundary note.
        self.anchor_quat = camera.orientation();

        /* -------- angular velocity for the lag layer -------------------- */
        let e = self.anchor_quat.to_euler_yxz();
        let yaw = e.y;
        let pitch = e.x;
        if self.has_prev && dt > 1e-5 {
            let dy = wrap_pi(yaw - self.prev_yaw) / dt;
            let dp = wrap_pi(pitch - self.prev_pitch) / dt;
            // Low-pass, then clamp: a teleport must not throw the gun off
            // screen. `viewmodel.js:655-657`.
            self.ang_vel_yaw = damp(self.ang_vel_yaw, clamp(dy, -9.0, 9.0), 18.0, dt);
            self.ang_vel_pitch = damp(self.ang_vel_pitch, clamp(dp, -9.0, 9.0), 18.0, dt);
        } else {
            self.ang_vel_yaw = 0.0;
            self.ang_vel_pitch = 0.0;
        }
        self.prev_yaw = yaw;
        self.prev_pitch = pitch;
        self.has_prev = true;

        /* -------- blends -------------------------------------------------- */
        let ads_rate = 1.0 / def.ads_time.max(0.05);
        // `this.clip && this.clip.name !== 'draw' ? 0 : s.ads ? 1 : 0`.
        // `viewmodel.js:668`.
        let want_ads = match &self.clip {
            Some(c) if c.name != "draw" => 0.0,
            _ => {
                if s.ads {
                    1.0
                } else {
                    0.0
                }
            }
        };
        self.ads_target = want_ads;
        self.ads_t = clamp01(
            self.ads_t + (if want_ads > 0.0 { ads_rate } else { -ads_rate * 1.25 }) * dt,
        );
        let ads = smootherstep(0.0, 1.0, self.ads_t);

        let sprint_target = if s.sprint && self.clip.is_none() { 1.0 } else { 0.0 };
        self.sprint_t = damp(self.sprint_t, sprint_target, 9.0, dt);
        self.low_ready_t = damp(self.low_ready_t, if s.low_ready { 1.0 } else { 0.0 }, 8.0, dt);

        self.trigger_target = if s.trigger { 1.0 } else { 0.0 };
        self.trigger_t = damp(self.trigger_t, self.trigger_target, 26.0, dt);

        /* -------- base pose ------------------------------------------------ */
        let mut base_pos = V3::from_array(def.hip_pos);
        let mut base_quat = Q::from_euler_xyz(def.hip_rot[0], def.hip_rot[1], def.hip_rot[2]);

        if self.sprint_t > 1e-3 {
            let p = V3::from_array(def.sprint_pos);
            let q = Q::from_euler_xyz(def.sprint_rot[0], def.sprint_rot[1], def.sprint_rot[2]);
            base_pos = base_pos.lerp(p, self.sprint_t);
            base_quat = base_quat.slerp(q, self.sprint_t);
        }
        if self.low_ready_t > 1e-3 {
            let p = V3::from_array(def.low_ready_pos);
            let q =
                Q::from_euler_xyz(def.low_ready_rot[0], def.low_ready_rot[1], def.low_ready_rot[2]);
            base_pos = base_pos.lerp(p, self.low_ready_t);
            base_quat = base_quat.slerp(q, self.low_ready_t);
        }

        /* -------- ADS pose: solved, not authored ---------------------------- */
        if ads > 1e-4 {
            let (ads_pos, ads_quat) = Self::ads_pose(weapon.sight, def.eye_relief, def.ads_cant);
            base_pos = base_pos.lerp(ads_pos, ads);
            base_quat = base_quat.slerp(ads_quat, ads);
        }

        /* -------- additive layers -------------------------------------------- */
        let sway_scale = def.sway_scale * lerp(1.0, 0.22, ads) * lerp(1.0, 1.5, self.sprint_t);
        self.noise_t += dt;
        let n = &self.noise;
        let nr = NOISE_RATES;
        let t = self.noise_t;
        let sway_x = n[0].fbm(t * nr[0], 3, 0.5) * 0.55 + n[3].fbm(t * nr[3] * 2.3, 2, 0.5) * 0.45;
        let sway_y = n[1].fbm(t * nr[1], 3, 0.5) * 0.55 + n[4].fbm(t * nr[4] * 2.1, 2, 0.5) * 0.45;
        let sway_z = n[2].fbm(t * nr[2], 2, 0.5) * 0.6 + n[5].fbm(t * nr[5] * 1.7, 2, 0.5) * 0.4;
        let breath = (t * 1.38).sin() * 0.5 + (t * 0.61 + 1.1).sin() * 0.25;

        let mut px = sway_x * 0.0075 * sway_scale;
        let mut py = (sway_y * 0.006 + breath * 0.0022) * sway_scale;
        let mut pz = sway_z * 0.004 * sway_scale;
        let mut rx = (sway_y * 0.021 + breath * 0.006) * sway_scale;
        let mut ry = sway_x * 0.028 * sway_scale;
        let mut rz = sway_z * 0.017 * sway_scale;

        /* -------- movement bob ------------------------------------------------ */
        let speed = s.speed;
        let bob_amt = def.bob_scale
            * clamp01(speed / 4.2)
            * lerp(1.0, 0.28, ads)
            * if s.airborne { 0.25 } else { 1.0 };
        if speed > 0.05 {
            self.bob_phase += dt * (3.1 + speed * 0.72) * if s.sprint { 1.05 } else { 1.0 };
            if self.bob_phase > TAU * 64.0 {
                self.bob_phase -= TAU * 64.0;
            }
        }
        let bp = self.bob_phase;
        px += bp.sin() * 0.0165 * bob_amt;
        py += (bp.cos().abs() - 0.6) * 0.0125 * bob_amt;
        pz += (bp * 2.0).sin() * 0.0055 * bob_amt;
        rz += bp.sin() * 0.031 * bob_amt;
        rx += (bp * 2.0).cos() * 0.014 * bob_amt;
        ry += (bp + 0.6).sin() * 0.019 * bob_amt;

        /* -------- weapon lag ---------------------------------------------- */
        let lag_scale = lerp(1.0, 0.42, ads);
        let (av_yaw, av_pitch) = (self.ang_vel_yaw, self.ang_vel_pitch);
        self.lag.step(
            dt,
            clamp(-av_yaw * 0.019, -0.05, 0.05) * lag_scale,
            clamp(av_pitch * 0.014, -0.04, 0.04) * lag_scale,
            clamp(-av_yaw.abs() * 0.006, -0.03, 0.03) * lag_scale,
        );
        self.lag_rot.step(
            dt,
            clamp(-av_pitch * 0.075, -0.24, 0.24) * lag_scale,
            clamp(av_yaw * 0.085, -0.3, 0.3) * lag_scale,
            clamp(-av_yaw * 0.055, -0.2, 0.2) * lag_scale,
        );
        px += self.lag.x();
        py += self.lag.y();
        pz += self.lag.z();
        rx += self.lag_rot.x();
        ry += self.lag_rot.y();
        rz += self.lag_rot.z();

        /* -------- recoil + settle ----------------------------------------- */
        self.rec_pos.step(dt, 0.0, 0.0, 0.0);
        self.rec_rot.step(dt, 0.0, 0.0, 0.0);
        self.settle.step(dt, 0.0, 0.0, 0.0);
        px += self.rec_pos.x();
        py += self.rec_pos.y();
        pz += self.rec_pos.z();
        // Note the deliberate axis swap on the settle drift: `rx` takes
        // `settle.y` and `ry` takes `settle.x` (`viewmodel.js:786-787`).
        rx += self.rec_rot.x() + self.settle.y();
        ry += self.rec_rot.y() + self.settle.x();
        rz += self.rec_rot.z() + self.settle.z();

        /* -------- jump / land --------------------------------------------- */
        self.jump_spring.step_to_target(dt);
        self.land_spring.step_to_target(dt);
        py -= self.land_spring.x * 0.014 + self.jump_spring.x * 0.006;
        rx -= self.land_spring.x * 0.05;

        /* -------- clip (reload / inspect / draw) --------------------------- */
        // `take`/put-back rather than a borrow: the loop below needs
        // `clip.events` while writing `self.clip_events`, and the tail may
        // clear `self.clip` outright.
        if let Some(clip) = self.clip.take() {
            self.clip_t += dt;
            let tt = clamp(self.clip_t, 0.0, clip.duration);
            clip.sample(tt, &mut self.clip_result);
            for ev in &clip.events {
                if ev.t > self.clip_prev_t && ev.t <= tt {
                    self.clip_events.push(FiredClipEvent {
                        name: ev.name,
                        clip: clip.name,
                    });
                }
            }
            self.clip_prev_t = tt;
            px += self.clip_result.pos[0];
            py += self.clip_result.pos[1];
            pz += self.clip_result.pos[2];
            rx += self.clip_result.rot[0];
            ry += self.clip_result.rot[1];
            rz += self.clip_result.rot[2];
            let done = self.clip_t >= clip.duration;
            self.clip = Some(clip);
            if done {
                self.stop_clip();
            }
        }

        /* -------- compose --------------------------------------------------- */
        self.rig_pos = V3::new(base_pos.x + px, base_pos.y + py, base_pos.z + pz);
        let add_quat = Q::from_euler_xyz(rx, ry, rz);
        self.rig_quat = base_quat.multiply(add_quat);

        /* -------- hands (first: the magazine can be held by one) ---------- */
        self.solve_hands(&weapon);

        /* -------- moving parts -------------------------------------------- */
        self.update_parts(&weapon, dt);

        /* -------- reticle -------------------------------------------------- */
        self.update_reticle(&weapon, ads);

        /* -------- viewmodel FOV ------------------------------------------- */
        let target_fov = FOV_BASE * lerp(1.0, def.view_fov, ads);
        if (self.view_fov - target_fov).abs() > 1e-3 {
            self.view_fov = target_fov;
        }
    }

    /// `_updateParts(w, dt, s, res)` (`viewmodel.js:856-913`), writing
    /// [`PartsState`] instead of mesh transforms. `s` is not taken: the
    /// source's parameter is never read (see [`FrameInput`]).
    fn update_parts(&mut self, w: &WeaponRig, dt: f64) {
        // Bolt / slide cycle: a fast rearward stroke and a slightly slower
        // return. `w.def.cycleTime ?? 60 / w.def.rpm` — `weapons/index.js:153`
        // assigns `def.cycleTime = 60 / def.rpm` at load, so the two arms of
        // that `??` are the same number and `WeaponDef` carries no
        // `cycle_time` field.
        if self.bolt_cycle > 0.0 {
            let cycle = ((60.0 / w.def.rpm) * 0.62).max(0.045);
            self.bolt_cycle = (self.bolt_cycle - dt / cycle).max(0.0);
        }
        let cyc = self.bolt_cycle;
        // 1 -> 0 over the cycle: out fast, back with a small bounce.
        let stroke = if cyc > 0.55 { (1.0 - cyc) / 0.45 } else { cyc / 0.55 };
        let clip_bolt = if self.clip_result.active {
            self.clip_result.parts.bolt
        } else {
            0.0
        };
        // See `bolt_hold`'s field doc: `boltHold` is never set to 1 anywhere
        // in the source, so this reduces to `stroke` and the clip's authored
        // `parts.bolt` track never reaches the mesh. Ported as written.
        let bolt_off = stroke.max(self.bolt_hold).max(clip_bolt * self.bolt_hold);
        self.parts.bolt_off = bolt_off;

        self.parts.bolt_pos = w.bolt_rest.map(|rest| {
            V3::new(
                rest.x + w.bolt_travel.x * bolt_off,
                rest.y + w.bolt_travel.y * bolt_off,
                rest.z + w.bolt_travel.z * bolt_off,
            )
        });
        self.parts.slide_pos = w.slide_rest.map(|rest| {
            V3::new(
                rest.x + w.slide_travel.x * bolt_off,
                rest.y + w.slide_travel.y * bolt_off,
                rest.z + w.slide_travel.z * bolt_off,
            )
        });
        let pull = if self.clip_result.active {
            self.clip_result.parts.charge
        } else {
            0.0
        };
        self.parts.charge_pos = w.charge_rest.map(|rest| {
            V3::new(
                rest.x + w.charge_pull.x * pull,
                rest.y + w.charge_pull.y * pull,
                rest.z + w.charge_pull.z * pull,
            )
        });
        let trigger_rot_x = w.trigger_pull * self.trigger_t;
        self.parts.trigger_rot_x = w.has_trigger.then_some(trigger_rot_x);
        // `lerp(-0.95, 0, clamp01(this.selectorLive ?? 1))`
        // (`viewmodel.js:897`). `selectorLive` is declared nowhere in the
        // whole source tree, so the `??` always takes the fallback below and
        // the lerp is a constant `0` — the selector never moves. Ported as
        // the expression the source evaluates rather than folded to a literal.
        const SELECTOR_LIVE_FALLBACK: f64 = 1.0;
        let selector_rot_x = lerp(-0.95, 0.0, clamp01(SELECTOR_LIVE_FALLBACK));
        self.parts.selector_rot_x = w.has_selector.then_some(selector_rot_x);

        // Magazine: seated, in the support hand, or hidden.
        if w.has_magazine {
            let in_hand = if self.clip_result.active {
                self.clip_result.parts.mag
            } else {
                0.0
            };
            self.mag_visible = if self.clip_result.active {
                self.clip_result.parts.mag_visible
            } else {
                true
            };
            self.parts.mag_visible = self.mag_visible;
            if in_hand > 1e-4 {
                // Follow the support hand: the magazine is gripped by its spine.
                let (pos, quat) = self.mag_from_hand(w, in_hand);
                self.parts.mag_pos = Some(pos);
                self.parts.mag_quat = Some(quat);
            } else {
                self.parts.mag_pos = Some(w.mag_seat_pos);
                self.parts.mag_quat = Some(w.mag_seat_quat);
            }
        }
    }

    /// `_magFromHand(w, magGroup, weight)`. `viewmodel.js:915-927`.
    ///
    /// The hand target is a WRIST in weapon space, so the magazine has to be
    /// offset into the palm (about 62 mm along the hand's -Z, the metacarpal
    /// axis) before the along-the-magazine offset — otherwise the mag is
    /// gripped by thin air behind the hand.
    fn mag_from_hand(&self, w: &WeaponRig, weight: f64) -> (V3, Q) {
        let q = self.hand_quat_l;
        let v = self.hand_pos_l;
        let v2 = V3::new(0.0, w.mag_len * 0.62, -0.062).apply_quat(q);
        let v = v.add(v2);
        // `lerpVectors(magSeatPos, v, weight)`.
        let pos = w.mag_seat_pos.lerp(v, weight);
        let quat = w.mag_seat_quat.slerp(q, weight);
        (pos, quat)
    }

    /// `_solveHands(w, res)`. `viewmodel.js:929-960`.
    fn solve_hands(&mut self, weapon: &WeaponRig) {
        // Shoulders are body-fixed: express the camera-space anchor in rig
        // space. `viewmodel.js:930-935`.
        let q_inv = self.rig_quat.invert();
        self.arm_r.shoulder = self.shoulder_r.sub(self.rig_pos).apply_quat(q_inv);
        self.arm_l.shoulder = self.shoulder_l.sub(self.rig_pos).apply_quat(q_inv);

        // ---- shooting hand: welded to the grip ----
        let g_r = weapon.grip_r;
        self.hand_pos = V3::from_array(g_r.pos);
        // `gR.finger ?? [0, -0.35, -0.94]`, `gR.back ?? [0.95, 0.25, 0.18]`.
        let finger_r = V3::from_array(g_r.finger.unwrap_or([0.0, -0.35, -0.94]));
        let back_r = V3::from_array(g_r.back.unwrap_or([0.95, 0.25, 0.18]));
        self.hand_quat = hand_basis(finger_r, back_r);
        self.arm_r.solve(self.hand_pos, self.hand_quat);
        self.arm_r.set_trigger(self.trigger_t);

        // ---- support hand: grip, or wherever the clip puts it ----
        let g_l = weapon.grip_l;
        let mut pos = g_l.pos;
        // `gL.finger ?? [0.82, 0.5, -0.28]`, `gL.back ?? [-0.5, 0.32, -0.8]`.
        let mut finger = g_l.finger.unwrap_or([0.82, 0.5, -0.28]);
        let mut back = g_l.back.unwrap_or([-0.5, 0.32, -0.8]);
        // `w.lhandPose ?? (w.id === 'pistol' ? 'cup' : 'clamp')`
        // (`viewmodel.js:949`). `addWeapon` always sets `lhandPose`, so the
        // `??` tail is unreachable; [`WeaponRig::lhand_pose`] is therefore a
        // plain field rather than an `Option`, and the fallback has no Rust
        // form to keep. It is recorded here rather than silently dropped.
        let mut pose = weapon.lhand_pose;
        if self.clip_result.active && self.clip_result.lhand.weight > 0.5 {
            pos = self.clip_result.lhand.pos;
            finger = self.clip_result.lhand.finger;
            back = self.clip_result.lhand.back;
            pose = HandPoseName::from(self.clip_result.lhand.pose);
        }
        self.hand_pos_l = V3::from_array(pos);
        self.hand_quat_l = hand_basis(V3::from_array(finger), V3::from_array(back));
        if pose != self.arm_l.pose_name {
            self.arm_l.set_pose(pose);
        }
        self.lhand_pose = pose;
        self.arm_l.solve(self.hand_pos_l, self.hand_quat_l);
    }

    /// The collimated dot. `_updateReticle(w, ads)`, `viewmodel.js:972-1034`.
    ///
    /// A red dot sight is a collimator: the reticle sits at optical infinity
    /// along the tube axis, so its apparent direction from the eye is the
    /// tube axis — independent of where the eye is. Reproducing that exactly
    /// (rather than gluing a sprite to the glass) is why the dot stays on
    /// target while the weapon sways, and why it vignettes out when you look
    /// through the tube from an angle.
    ///
    /// The `lookAt` billboard at `viewmodel.js:1006` is not carried — see the
    /// module doc.
    fn update_reticle(&mut self, w: &WeaponRig, ads: f64) {
        let Some(optic) = w.optic else {
            self.reticle.visible = false;
            return;
        };
        // Optic axis and lens centre, both in camera space. The weapon group
        // is a child of the rig which is a child of the anchor, so camera
        // space is just the rig transform applied to the weapon-local values.
        let v = V3::from_array(optic.center)
            .apply_quat(self.rig_quat)
            .add(self.rig_pos);
        let v3 = V3::new(0.0, 0.0, -1.0).apply_quat(self.rig_quat).normalize();

        // Where the axis ray from the eye crosses the lens plane.
        let s = v.dot(v3);
        if s <= 0.02 {
            self.reticle.visible = false;
            return;
        }
        let v2 = v3.scale(s); // dot position in camera space
        // Vignette: how far off the lens centre the apparent dot lands.
        let off_x = v2.x - v.x;
        let off_y = v2.y - v.y;
        let off = js_hypot2(off_x, off_y);
        let aperture_r = optic.aperture_r;
        let mut alpha = 1.0 - smootherstep(aperture_r * 0.5, aperture_r * 1.05, off);
        alpha *= lerp(0.55, 1.0, ads); // brighter once the eye is behind the glass

        if alpha <= 0.01 {
            self.reticle.visible = false;
            return;
        }
        self.reticle.visible = true;
        self.reticle.position = v2;
        // SIZE. Angular, so it is FOV-independent within a stance — but not
        // constant across stances, because the requirement is a fixed number
        // of PIXELS. hipfire 0.00385 rad -> 4.0 px radius; ADS 0.00655 rad ->
        // 7.9 px radius, with the halo at 1.6x and the segmented ring at
        // 3.2x, both scaled off the same number so the reticle never changes
        // shape.
        let core_r = s * lerp(0.00385, 0.00655, ads);
        self.reticle.core_scale = core_r;
        self.reticle.core_opacity = alpha;
        self.reticle.rim_opacity = alpha * 0.8;
        self.reticle.ring_opacity = alpha;
        // The halo is a bloom seed, not a glow: 6% at 1.6x the core radius
        // adds ~1 px of soft falloff and nothing else.
        self.reticle.halo_opacity = alpha * 0.06;
    }
}
