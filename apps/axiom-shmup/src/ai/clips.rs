//! Ported from Claude-of-Duty `src/ai/clips.js:1-355`.
//!
//! AI — animation content.
//!
//! Poses are authored as **local euler deltas in degrees** on top of the bind
//! pose. The rig is built so that every bone's local axes mean the same thing:
//!
//! ```text
//!   x  flexion   — positive bends the bone forward (knee extends, spine bows)
//!   y  twist     — roll about the bone's own length
//!   z  lateral   — positive tips the bone toward the character's right
//! ```
//!
//! That makes a walk cycle readable as anatomy rather than as quaternion soup,
//! and lets layers be blended by simple lerp of the delta arrays.
//!
//! Locomotion curves are hand-tuned against reference gait: the knee flexes
//! hardest just after toe-off, the pelvis drops through mid-stance and rolls
//! toward the stance leg, and the spine counter-rotates against the pelvis.
//!
//! ## Shape of the port
//!
//! Every clip function here takes `&mut Poser` where the source takes `P` —
//! the accumulator is defined in [`super::animator`] because that is the file
//! that defines it in the source (`animator.js:33-66`), and Rust is happy with
//! the resulting module cycle inside one crate.
//!
//! The euler *deltas* are the whole contract: nothing here touches a
//! quaternion, a matrix or a bone. The one thing that is not a plain degree
//! delta is `P.hip(...)`, the pelvis translation offset in metres.
//!
//! There is no `Float32Array` in `clips.js` — checked. The `f32` rounding in
//! this stack lives one level up, in [`super::animator::Poser`]'s accumulator.

use super::animator::Poser;

const TAU: f64 = std::f64::consts::PI * 2.0;

/// Smooth positive lobe used for knee/ankle curves. `clips.js:23-26`.
///
/// `s > 0` is a strict comparison in the source, so a zero (or negative)
/// `sin` contributes exactly nothing — this is a `sign`-like three-valued
/// gate, not a `max`, and `0.0f64.powf(k)` (which is `0`) would coincide but
/// `(-x).powf(1.5)` (which is `NaN`) would not.
///
/// The source's `k = 1.4` default is dropped: `gait` is the only caller and
/// it passes `1.5` and `2` explicitly, so the default is dead. (Kept as a
/// note rather than as an unreachable overload — see the port recipe on dead
/// source computation; a defaulted *parameter* with no defaulting call site
/// has no behaviour to preserve.)
fn lobe(x: f64, k: f64) -> f64 {
    let s = x.sin();
    if s > 0.0 {
        s.powf(k)
    } else {
        0.0
    }
}

/// The six looping locomotion clips `CLIPS` exposes (`clips.js:354`).
///
/// **Order matches the source's object literal.** `CLIPS[clip]` is a string
/// lookup in JS so the order carries no meaning there, but this enum is the
/// animator's clip identity and is kept in source order on principle (see the
/// port recipe's "an enum used as a table index is order-dependent" trap).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClipId {
    #[default]
    Idle,
    Walk,
    Run,
    CrouchWalk,
    CrouchIdle,
    HurtIdle,
}

impl ClipId {
    /// `C.CLIPS[clip] ?? C.idle` (`animator.js:243-244`) — the dispatch, with
    /// the source's fallback folded into the enum: an unknown clip name cannot
    /// be represented, so every variant resolves.
    pub fn eval(self, p: &mut Poser, ph: f64) {
        match self {
            ClipId::Idle => idle(p, ph),
            ClipId::Walk => walk(p, ph),
            ClipId::Run => run(p, ph),
            ClipId::CrouchWalk => crouch_walk(p, ph),
            ClipId::CrouchIdle => crouch_idle(p, ph),
            ClipId::HurtIdle => hurt_idle(p, ph),
        }
    }
}

/* ------------------------------------------------------------------ */
/* base stance                                                        */
/* ------------------------------------------------------------------ */

/// Weight on the left leg, knees soft, weapon at low ready. `clips.js:33-61`.
///
/// The source's third parameter (`p = {}`) is never read anywhere in the file
/// and no caller passes it; it is omitted rather than carried as an
/// always-empty argument.
pub fn idle(p: &mut Poser, ph: f64) {
    let t = ph * TAU;
    let breath = (t * 0.55).sin();
    let sway = (t * 0.31 + 1.1).sin();
    let micro = (t * 1.7 + 0.4).sin() * 0.35 + (t * 2.9).sin() * 0.2;

    p.hip(0.012 * sway, -0.008 + 0.004 * breath, 0.0);
    p.d("Hips", -1.5, 2.2 * sway, 1.6);
    p.d("Spine", 1.6 + 0.7 * breath, -1.4 * sway, -0.8);
    p.d("Spine1", 1.2 + 0.9 * breath, -1.0 * sway, -0.6);
    p.d("Spine2", -0.6 + 1.1 * breath, 1.6 * sway, 0.4);
    p.d("Neck", 1.0 - 0.5 * breath, 1.2 * sway + micro, 0.0);
    p.d("Head", -1.2, 1.0 * micro, 0.6 * sway);

    // stance: right leg carries, left slightly forward
    p.d("UpLegR", -2.0, 1.5, -1.5);
    p.d("LegR", -5.5, 0.0, 0.0);
    p.d("FootR", 4.5, -1.5, 0.0);
    p.d("UpLegL", 5.0, -4.5, 2.5);
    p.d("LegL", -9.0, 0.0, 0.0);
    p.d("FootL", 5.5, 3.0, 0.0);

    // shoulders settle, weapon rides the breath
    p.d("ClavicleR", -1.5 + 0.8 * breath, 0.0, 1.2);
    p.d("ClavicleL", -1.0 + 0.6 * breath, 0.0, -1.0);
    p.d("UpperArmR", -3.0, 0.0, 2.0);
    p.d("UpperArmL", 2.0, 0.0, -2.0);
    p.d("ForearmR", 2.0, 0.0, 0.0);
}

/// Stock in the shoulder, head over the sights, weight forward on bent knees.
/// Additive over any base. `clips.js:68-87`.
pub fn aim_add(p: &mut Poser, w: f64) {
    // fighting stance: knees soft, hips dropped, feet staggered
    p.hip(0.0, -0.035 * w, 0.012 * w);
    p.d("Hips", 4.0 * w, 3.0 * w, 0.0);
    p.d("UpLegR", 8.0 * w, 4.0 * w, -3.0 * w);
    p.d("LegR", -17.0 * w, 0.0, 0.0);
    p.d("FootR", 9.0 * w, -2.0 * w, 0.0);
    p.d("UpLegL", 3.0 * w, -6.0 * w, 4.0 * w);
    p.d("LegL", -13.0 * w, 0.0, 0.0);
    p.d("FootL", 8.0 * w, 3.0 * w, 0.0);
    p.d("Spine1", 2.5 * w, 0.0, 0.0);
    p.d("Spine2", 3.0 * w, -5.0 * w, 0.0);
    p.d("Neck", 5.0 * w, 3.0 * w, 0.0);
    p.d("Head", -3.5 * w, 2.0 * w, -1.5 * w);
    p.d("ClavicleR", -6.0 * w, -2.0 * w, 5.0 * w);
    p.d("ClavicleL", -3.0 * w, 4.0 * w, -3.0 * w);
    p.d("UpperArmR", 10.0 * w, 0.0, 14.0 * w);
    p.d("ForearmR", -12.0 * w, 0.0, 0.0);
    p.d("UpperArmL", 8.0 * w, 0.0, -6.0 * w);
}

/* ------------------------------------------------------------------ */
/* locomotion                                                         */
/* ------------------------------------------------------------------ */

/// The per-side bone names `gait`/`turnStep` build with a template literal
/// (`` P.d(`UpLeg${s}`, ...) ``). A table rather than a `format!` so the port
/// allocates nothing where the source allocates a string.
struct SideNames {
    upleg: &'static str,
    leg: &'static str,
    foot: &'static str,
    toe: &'static str,
}

static SIDE_R: SideNames = SideNames { upleg: "UpLegR", leg: "LegR", foot: "FootR", toe: "ToeR" };
static SIDE_L: SideNames = SideNames { upleg: "UpLegL", leg: "LegL", foot: "FootL", toe: "ToeL" };

/// The hand-tuned gait constants. `clips.js:124-149`.
struct Gait {
    thigh: f64,
    thigh_bias: f64,
    thigh_twist: f64,
    splay: f64,
    knee_base: f64,
    knee: f64,
    knee_stance: f64,
    ankle: f64,
    ankle_bias: f64,
    toe: f64,
    sway: f64,
    bob: f64,
    bob_bias: f64,
    pelvis_tilt: f64,
    pelvis_yaw: f64,
    pelvis_roll: f64,
    lean: f64,
    spine_yaw: f64,
    arm_swing: f64,
}

/// `gait(P, ph, k)`. `clips.js:93-122`.
fn gait(p: &mut Poser, ph: f64, k: &Gait) {
    let t = ph * TAU;
    for side in [1.0f64, -1.0f64] {
        let s = if side > 0.0 { &SIDE_R } else { &SIDE_L };
        let o = if side > 0.0 { 0.0 } else { std::f64::consts::PI }; // legs half a cycle apart
        let a = t + o;
        // thigh: swings forward through the air, back through stance
        let thigh = k.thigh * a.sin() + k.thigh_bias;
        // knee: heavy flexion just after toe-off, small at heel strike
        let knee = -(k.knee_base
            + k.knee * lobe(a - 0.55, 1.5)
            + k.knee_stance * lobe(a + std::f64::consts::PI + 0.4, 2.0));
        // ankle: toe-off push then dorsiflexion to clear the ground
        let ankle = k.ankle * (a - 1.9).sin() + k.ankle_bias;
        p.d(s.upleg, thigh, side * k.thigh_twist, side * k.splay);
        p.d(s.leg, knee, 0.0, 0.0);
        p.d(s.foot, ankle, -side * 1.5, 0.0);
        p.d(s.toe, (0.0f64).max(-k.toe * (a - 2.6).sin()), 0.0, 0.0);
    }
    // pelvis: two bobs per stride, roll toward the stance leg, counter-yaw
    p.hip(k.sway * t.sin(), k.bob_bias + k.bob * (2.0 * t).cos(), 0.0);
    p.d("Hips", k.pelvis_tilt, k.pelvis_yaw * t.sin(), k.pelvis_roll * (t + 1.2).sin());
    p.d(
        "Spine",
        k.lean * 0.35,
        -k.spine_yaw * 0.45 * t.sin(),
        -k.pelvis_roll * 0.35 * (t + 1.2).sin(),
    );
    p.d("Spine1", k.lean * 0.35, -k.spine_yaw * 0.75 * t.sin(), 0.0);
    p.d("Spine2", k.lean * 0.3, -k.spine_yaw * t.sin(), 0.0);
    p.d("Neck", -k.lean * 0.5, k.spine_yaw * 0.6 * t.sin(), 0.0);
    // the rifle rides on the shoulders, so they take the bounce
    p.d("ClavicleR", -k.arm_swing * t.sin() - 1.0, 0.0, 1.5);
    p.d("ClavicleL", k.arm_swing * t.sin() - 1.0, 0.0, -1.5);
    p.d("UpperArmR", -k.arm_swing * 0.6 * t.sin(), 0.0, 2.0);
    p.d("UpperArmL", k.arm_swing * 0.8 * t.sin(), 0.0, -2.0);
}

const WALK: Gait = Gait {
    thigh: 21.0,
    thigh_bias: -2.0,
    thigh_twist: 1.5,
    splay: 1.5,
    knee_base: 7.0,
    knee: 46.0,
    knee_stance: 8.0,
    ankle: 12.0,
    ankle_bias: 2.0,
    toe: 16.0,
    sway: 0.014,
    bob: 0.014,
    bob_bias: -0.014,
    pelvis_tilt: -1.0,
    pelvis_yaw: 4.5,
    pelvis_roll: 3.2,
    lean: 4.0,
    spine_yaw: 3.4,
    arm_swing: 3.5,
};

const RUN: Gait = Gait {
    thigh: 34.0,
    thigh_bias: 2.0,
    thigh_twist: 2.0,
    splay: 2.0,
    knee_base: 14.0,
    knee: 86.0,
    knee_stance: 22.0,
    ankle: 20.0,
    ankle_bias: 4.0,
    toe: 26.0,
    sway: 0.02,
    bob: 0.03,
    bob_bias: -0.03,
    pelvis_tilt: -3.0,
    pelvis_yaw: 7.0,
    pelvis_roll: 5.0,
    lean: 13.0,
    spine_yaw: 6.0,
    arm_swing: 7.0,
};

const CROUCH: Gait = Gait {
    thigh: 13.0,
    thigh_bias: 38.0,
    thigh_twist: 2.0,
    splay: 4.0,
    knee_base: 74.0,
    knee: 26.0,
    knee_stance: 6.0,
    ankle: 8.0,
    ankle_bias: 26.0,
    toe: 8.0,
    sway: 0.01,
    bob: 0.008,
    bob_bias: -0.008,
    pelvis_tilt: 6.0,
    pelvis_yaw: 3.0,
    pelvis_roll: 2.0,
    lean: 16.0,
    spine_yaw: 2.4,
    arm_swing: 2.0,
};

/// `clips.js:151-153`.
pub fn walk(p: &mut Poser, ph: f64) {
    gait(p, ph, &WALK);
}

/// `clips.js:155-159`.
pub fn run(p: &mut Poser, ph: f64) {
    gait(p, ph, &RUN);
    // head stabilises against the bigger bounce
    p.d("Head", -3.0, 0.0, 0.0);
}

/// `clips.js:161-165`.
pub fn crouch_walk(p: &mut Poser, ph: f64) {
    gait(p, ph, &CROUCH);
    p.hip(0.0, -0.30, -0.02);
    p.d("Spine2", 4.0, 0.0, 0.0);
}

/// Static crouch — knees loaded, torso upright behind the weapon.
/// `clips.js:168-185`.
pub fn crouch_idle(p: &mut Poser, ph: f64) {
    let t = ph * TAU;
    let breath = (t * 0.6).sin();
    p.hip(0.004 * (t * 0.4).sin(), -0.315 + 0.004 * breath, -0.02);
    p.d("Hips", 7.0, 1.5, 1.0);
    p.d("UpLegR", 44.0, 3.0, -6.0);
    p.d("LegR", -78.0, 0.0, 0.0);
    p.d("FootR", 30.0, -2.0, 0.0);
    p.d("UpLegL", 36.0, -6.0, 7.0);
    p.d("LegL", -86.0, 0.0, 0.0);
    p.d("FootL", 32.0, 4.0, 0.0);
    p.d("Spine", 6.0 + 0.6 * breath, 0.0, 0.0);
    p.d("Spine1", 5.0 + 0.8 * breath, 0.0, 0.0);
    p.d("Spine2", 3.0 + 1.0 * breath, 0.0, 0.0);
    p.d("Neck", 2.0, 0.0, 0.0);
    p.d("ClavicleR", -2.0, 0.0, 1.5);
    p.d("ClavicleL", -1.5, 0.0, -1.5);
}

/// Prone-ish crawl is out of scope; a wounded low stance stands in for it.
/// `clips.js:188-202`.
pub fn hurt_idle(p: &mut Poser, ph: f64) {
    let t = ph * TAU;
    p.hip(0.0, -0.10, -0.03);
    p.d("Hips", 10.0, 0.0, 4.0);
    p.d("Spine", 12.0, 0.0, -3.0);
    p.d("Spine1", 9.0, 0.0, -2.0);
    p.d("Spine2", 5.0 + (t * 1.6).sin(), 0.0, 0.0);
    p.d("Neck", 6.0, 0.0, 0.0);
    p.d("UpLegR", 16.0, 0.0, -3.0);
    p.d("LegR", -28.0, 0.0, 0.0);
    p.d("FootR", 12.0, 0.0, 0.0);
    p.d("UpLegL", 10.0, 0.0, 4.0);
    p.d("LegL", -20.0, 0.0, 0.0);
    p.d("FootL", 9.0, 0.0, 0.0);
}

/* ------------------------------------------------------------------ */
/* one-shots (t is 0..1 over the clip's duration)                     */
/* ------------------------------------------------------------------ */

/// Pivot on the balls of the feet: the trailing foot lifts and re-plants.
/// `clips.js:209-220`. `dir` is `+1`/`-1`; the source tests `dir > 0`.
pub fn turn_step(p: &mut Poser, t: f64, dir: f64) {
    let e = (std::f64::consts::PI * (1.0f64).min(t)).sin(); // 0..1..0
    let s = if dir > 0.0 { &SIDE_R } else { &SIDE_L };
    let o = if dir > 0.0 { &SIDE_L } else { &SIDE_R };
    p.d(s.upleg, 12.0 * e, dir * 16.0 * e, 0.0);
    p.d(s.leg, -34.0 * e, 0.0, 0.0);
    p.d(s.foot, 16.0 * e, 0.0, 0.0);
    p.d(o.upleg, -4.0 * e, -dir * 4.0 * e, 0.0);
    p.d(o.leg, -10.0 * e, 0.0, 0.0);
    p.d("Hips", 0.0, dir * 6.0 * e, dir * -2.0 * e);
    p.hip(0.0, -0.012 * e, 0.0);
}

/// Vault: plant the support hand, tuck the knees over the obstacle, land.
/// Root motion (the actual translation) is driven by the agent.
/// `clips.js:226-248`.
pub fn vault(p: &mut Poser, t: f64) {
    let rise = (std::f64::consts::PI * (1.0f64).min(t * 1.05)).sin();
    let tuck = (std::f64::consts::PI * (1.0f64).min((0.0f64).max((t - 0.12) * 1.3))).sin();
    let land = (0.0f64).max((t - 0.7) / 0.3);
    p.hip(0.0, 0.10 * rise, 0.02 * rise);
    p.d("Hips", 26.0 * rise - 16.0 * land, 0.0, 0.0);
    p.d("Spine", 20.0 * rise, 0.0, -4.0 * rise);
    p.d("Spine1", 14.0 * rise, 0.0, -3.0 * rise);
    p.d("Spine2", 8.0 * rise, -14.0 * rise, 0.0);
    p.d("Neck", -8.0 * rise, 6.0 * rise, 0.0);
    p.d("UpLegR", 86.0 * tuck + 30.0 * land, 0.0, -10.0 * tuck);
    p.d("LegR", -104.0 * tuck - 20.0 * land, 0.0, 0.0);
    p.d("FootR", 24.0 * tuck, 0.0, 0.0);
    p.d("UpLegL", 68.0 * tuck + 12.0 * land, 0.0, 12.0 * tuck);
    p.d("LegL", -92.0 * tuck - 30.0 * land, 0.0, 0.0);
    p.d("FootL", 20.0 * tuck, 0.0, 0.0);
    // support arm swings out of the weapon grip
    p.d("ClavicleL", -18.0 * rise, 12.0 * rise, -14.0 * rise);
    p.d("UpperArmL", -46.0 * rise, 0.0, -28.0 * rise);
    p.d("ForearmL", -30.0 * rise, 0.0, 0.0);
    p.d("ClavicleR", -6.0 * rise, 0.0, 4.0 * rise);
    p.d("UpperArmR", -14.0 * rise, 0.0, 10.0 * rise);
}

/// Firing impulse. `t` is seconds since the shot; the shape is a fast spike
/// and a springy settle. `clips.js:255-269`.
pub fn recoil_add(p: &mut Poser, t: f64, strength: f64) {
    if t > 0.26 {
        return;
    }
    let e = (-t * 16.0).exp();
    let osc = (t * 92.0).sin();
    let k = strength * e;
    p.d("ClavicleR", -7.0 * k, 0.0, 3.0 * k);
    p.d("UpperArmR", -9.0 * k + 2.0 * osc * k, 0.0, 5.0 * k);
    p.d("ForearmR", 7.0 * k, 0.0, 0.0);
    p.d("ClavicleL", -3.0 * k, 0.0, -2.0 * k);
    p.d("UpperArmL", -6.0 * k, 0.0, -3.0 * k);
    p.d("Spine2", -3.5 * k, 1.5 * k * osc, 0.0);
    p.d("Spine1", -2.0 * k, 0.0, 0.0);
    p.d("Neck", -2.5 * k, 0.0, 0.0);
    p.d("Head", 1.5 * k, 0.8 * k * osc, 0.0);
}

/// The `region` argument of `hitAdd` (`clips.js:277-319`).
///
/// **Order matches the source's `switch` arms**, `default` last: the source
/// dispatches on a string, so nothing indexes this, but the variant order is
/// kept in source order on principle. `Other` is the `default:` arm — every
/// region string `agent.js` does not name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HitRegion {
    Head,
    #[default]
    Torso,
    ArmR,
    ArmL,
    LegR,
    LegL,
    Other,
}

/// Region-specific hit reaction; `t` seconds since impact, 0.45 s long.
/// `clips.js:272-320`.
///
/// The doc comment says 0.45 s and the guard says 0.5 s — a source quirk,
/// carried as written.
pub fn hit_add(p: &mut Poser, region: HitRegion, t: f64, dir_side: f64, strength: f64) {
    if t > 0.5 {
        return;
    }
    let e = (-t * 7.5).exp() * (1.0f64).min(t * 22.0);
    let k = strength * e;
    let side: f64 = if dir_side >= 0.0 { 1.0 } else { -1.0 };
    match region {
        HitRegion::Head => {
            p.d("Neck", -16.0 * k, 10.0 * k * side, 6.0 * k * side);
            p.d("Head", -20.0 * k, 14.0 * k * side, 8.0 * k * side);
            p.d("Spine2", -7.0 * k, 4.0 * k * side, 0.0);
            p.d("Spine1", -4.0 * k, 0.0, 0.0);
        }
        HitRegion::Torso => {
            p.d("Spine", -6.0 * k, 3.0 * k * side, 2.0 * k * side);
            p.d("Spine1", -9.0 * k, 5.0 * k * side, 3.0 * k * side);
            p.d("Spine2", -11.0 * k, 6.0 * k * side, 4.0 * k * side);
            p.d("Neck", 6.0 * k, -3.0 * k * side, 0.0);
            p.d("Hips", 4.0 * k, 0.0, 0.0);
            p.hip(-0.02 * k * side, -0.02 * k, -0.03 * k);
        }
        HitRegion::ArmR => {
            p.d("ClavicleR", -14.0 * k, 6.0 * k, 10.0 * k);
            p.d("UpperArmR", -22.0 * k, 0.0, 14.0 * k);
            p.d("ForearmR", 16.0 * k, 0.0, 0.0);
            p.d("Spine2", -5.0 * k, 6.0 * k, 0.0);
        }
        HitRegion::ArmL => {
            p.d("ClavicleL", -14.0 * k, -6.0 * k, -10.0 * k);
            p.d("UpperArmL", -24.0 * k, 0.0, -16.0 * k);
            p.d("ForearmL", 18.0 * k, 0.0, 0.0);
            p.d("Spine2", -5.0 * k, -6.0 * k, 0.0);
        }
        HitRegion::LegR => {
            p.d("UpLegR", 14.0 * k, 0.0, -8.0 * k);
            p.d("LegR", -30.0 * k, 0.0, 0.0);
            p.d("Hips", 8.0 * k, 0.0, -6.0 * k);
            p.hip(0.0, -0.05 * k, 0.0);
        }
        HitRegion::LegL => {
            p.d("UpLegL", 14.0 * k, 0.0, 8.0 * k);
            p.d("LegL", -30.0 * k, 0.0, 0.0);
            p.d("Hips", 8.0 * k, 0.0, 6.0 * k);
            p.hip(0.0, -0.05 * k, 0.0);
        }
        HitRegion::Other => {
            p.d("Spine1", -6.0 * k, 0.0, 0.0);
            p.d("Spine2", -6.0 * k, 0.0, 0.0);
        }
    }
}

/// Flinch/duck when rounds crack past. `clips.js:323-336`.
pub fn suppress_add(p: &mut Poser, w: f64) {
    if w <= 0.0 {
        return;
    }
    p.d("Hips", 7.0 * w, 0.0, 0.0);
    p.d("Spine", 9.0 * w, 0.0, 0.0);
    p.d("Spine1", 8.0 * w, 0.0, 0.0);
    p.d("Spine2", 6.0 * w, 0.0, 0.0);
    p.d("Neck", -6.0 * w, 0.0, 0.0);
    p.d("Head", -8.0 * w, 0.0, 0.0);
    p.d("UpLegR", 16.0 * w, 0.0, 0.0);
    p.d("LegR", -26.0 * w, 0.0, 0.0);
    p.d("UpLegL", 14.0 * w, 0.0, 0.0);
    p.d("LegL", -24.0 * w, 0.0, 0.0);
    p.hip(0.0, -0.10 * w, 0.0);
}

/// Reload: the support hand leaves the handguard, drops the magazine, fetches
/// a fresh one from the chest and slaps it home. The hand path itself is
/// driven by the animator's IK target; this is the body language around it.
/// `clips.js:343-352`.
pub fn reload_add(p: &mut Poser, t: f64) {
    let w = (1.0f64).min((0.0f64).max((t * 6.0).min((1.0 - t) * 6.0)));
    p.d("Spine2", 4.0 * w, -16.0 * w, -3.0 * w);
    p.d("Spine1", 3.0 * w, -6.0 * w, 0.0);
    p.d("Neck", 6.0 * w, 8.0 * w, 0.0);
    p.d("Head", -4.0 * w, 6.0 * w, 3.0 * w);
    p.d("ClavicleR", -4.0 * w, -4.0 * w, 4.0 * w);
    p.d("UpperArmR", 6.0 * w, 0.0, 10.0 * w);
    p.d("ForearmR", -6.0 * w, 0.0, 0.0);
}
