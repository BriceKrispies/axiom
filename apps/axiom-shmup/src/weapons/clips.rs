//! Ported from Claude-of-Duty `src/weapons/clips.js:1-318`.
//!
//! Procedural animation clips (reload, inspect, draw, holster).
//!
//! These are *authored keyframes*, not baked animation data: every key is
//! expressed relative to the weapon's own attachment nodes, so the same
//! timeline drives a carbine, an SMG and a pistol with correct hand
//! positions, and the timing scales with the weapon's reload speed.
//!
//! Channels (`clips.js:11-18`):
//!   - `weapon` : additive pose offset for the whole viewmodel (`pos`/`rot`)
//!   - `lhand`  : support-hand target in weapon space (`pos`/`finger`/`back`/`pose`)
//!   - `parts`  : moving-part drive (`mag`/`mag_visible`/`charge`/`bolt`/`slide`)
//!   - `events` : named beats the weapon system reacts to (`t`, `name`)
//!
//! **Source quirk in the doc comment itself**, preserved for the record: the
//! source's channel doc (`clips.js:15`) advertises a `parts` channel driving
//! `mag / magHand / charge / bolt / slide / trigger`, but `Clip.sample`'s
//! parts blend (`clips.js:87-95`) only ever reads/writes `mag`, `magVisible`,
//! `charge`, `bolt` and `slide` — `magHand` and `trigger` are named in prose
//! but never wired to any track or blend. No authored clip data sets them
//! either. There is nothing to port for those two names; they are dead
//! vocabulary in the source.
//!
//! **A second, load-bearing source quirk — preserved exactly, not fixed —
//! documented in full at [`build_clips`]:** several tracks' *final* keyframe
//! is authored with the literal `t: 1` instead of `t: 1 * scale` (the
//! per-clip time scale — `tac`/`emp`/`insp`), which makes the track's last
//! two keyframes go out of chronological order whenever the clip's duration
//! exceeds one second (true of every shipped weapon's `reloadTac`,
//! `reloadEmpty` and `inspect`). Because [`locate`] (`sampleTrack`,
//! `clips.js:28-42`) walks keys with a single forward scan and never
//! re-sorts, the track snaps straight to its final (neutral) pose the moment
//! elapsed time first reaches the second-to-last keyframe's *scaled* time,
//! instead of easing through the authored tail. See `weapon_channel_snaps_early_because_the_final_key_ignores_the_time_scale`
//! in `tests/weapons_clips_port.rs` for the pinned behaviour.
//!
//! A key may carry an `ease`; every struct below spells it out explicitly
//! (`clips.js:18`'s implicit "default: smooth" becomes [`Ease::Smooth`]
//! written at every site, matching this port's convention of full explicit
//! struct literals — see `defs.rs`).

use crate::weapons::defs::WeaponDef;
use crate::weapons::mathx::{clamp01, ease_out_back, ease_out_cubic, lerp, smootherstep};

// ============================================================================
//  easing
// ============================================================================

/// `EASE`, `clips.js:21-26`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ease {
    Linear,
    Smooth,
    Out,
    Back,
}

impl Ease {
    /// `EASE[name](t)`. `clips.js:22-25`. The `back` case's `k = 1.4` is the
    /// source's literal argument to `easeOutBack(t, 1.4)` — distinct from
    /// [`crate::weapons::mathx::EASE_OUT_BACK_DEFAULT_K`] (1.6), the
    /// *defaulted* `easeOutBack(t)` call used elsewhere in the source.
    pub fn apply(self, t: f64) -> f64 {
        match self {
            Ease::Linear => t,
            Ease::Smooth => smootherstep(0.0, 1.0, t),
            Ease::Out => ease_out_cubic(t),
            Ease::Back => ease_out_back(t, 1.4),
        }
    }
}

/// Support-hand grip shape while a track key holds it. `clips.js` writes this
/// as a plain string (`'wrap'`, `'pinch'`, `'open'`, `'clamp'`); the port uses
/// an enum since every value that appears in the authored clip data is one of
/// these four literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pose {
    Wrap,
    Pinch,
    Open,
    Clamp,
}

// ============================================================================
//  keyframes
// ============================================================================

/// One `weapon` channel keyframe. E.g. `clips.js:147-152`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeaponKey {
    pub t: f64,
    pub p: [f64; 3],
    pub r: [f64; 3],
    pub ease: Ease,
}

/// One `lhand` channel keyframe. E.g. `clips.js:155-165`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LHandKey {
    pub t: f64,
    pub p: [f64; 3],
    pub finger: [f64; 3],
    pub back: [f64; 3],
    pub pose: Pose,
    /// `a.weight ?? 1` (`clips.js:80`) — no authored key in `clips.js` ever
    /// sets `weight`, so every key here carries the literal default `1.0`
    /// explicitly rather than modelling an `Option` no call site would fill.
    pub weight: f64,
    pub ease: Ease,
}

/// One `parts` channel keyframe. E.g. `clips.js:168-177`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartsKey {
    pub t: f64,
    pub mag: f64,
    /// `magVisible` (`clips.js:90`) — kept as `f64` (not `bool`) on the key
    /// itself because the source stores `0`/`1` and the *sampled* result
    /// applies a threshold (`> 0.5`) combined with the blend weight; see
    /// [`Clip::sample`]'s parts branch.
    pub mag_visible: f64,
    pub bolt: f64,
    pub slide: f64,
    pub charge: f64,
    pub ease: Ease,
}

/// One named beat the weapon system reacts to. `clips.js:16`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipEvent {
    pub t: f64,
    pub name: &'static str,
}

/// Shared accessor so [`locate`] can walk any of the three track kinds with
/// one implementation. The source has no equivalent trait — `sampleTrack`
/// (`clips.js:28-42`) is untyped JS reading `.t`/`.ease` off whatever object
/// it is handed — this is the Rust shape that gets the same generality.
trait Keyframe {
    fn t(&self) -> f64;
    fn ease(&self) -> Ease;
}

impl Keyframe for WeaponKey {
    fn t(&self) -> f64 {
        self.t
    }
    fn ease(&self) -> Ease {
        self.ease
    }
}

impl Keyframe for LHandKey {
    fn t(&self) -> f64 {
        self.t
    }
    fn ease(&self) -> Ease {
        self.ease
    }
}

impl Keyframe for PartsKey {
    fn t(&self) -> f64 {
        self.t
    }
    fn ease(&self) -> Ease {
        self.ease
    }
}

/// Find the bracketing keyframe pair and the eased blend weight between them.
/// `sampleTrack`, `clips.js:28-42`, minus the `blend` callback: the source
/// passes a closure that reads `a`/`b`/`w` and writes into `out` in the same
/// breath, but each of the three channels writes different fields, so this
/// port returns the `(index_a, index_b, weight)` triple and each caller in
/// [`Clip::sample`] does its own field-by-field lerp — the direct Rust
/// analogue of the source's per-channel callback body.
///
/// Returns `None` for an empty/absent track, matching `sampleTrack`'s
/// `if (!keys || !keys.length) return false;` (`clips.js:29`) — the boolean
/// return is unused by every call site in the source, so nothing here needs
/// to thread it further than the `Option`.
fn locate<K: Keyframe>(keys: &[K], t: f64) -> Option<(usize, usize, f64)> {
    if keys.is_empty() {
        return None;
    }
    let mut i = 0usize;
    while i < keys.len() - 1 && keys[i + 1].t() <= t {
        i += 1;
    }
    let a = i;
    let b = (keys.len() - 1).min(i + 1);
    let w = if b != a {
        let span = keys[b].t() - keys[a].t();
        let raw = if span > 1e-6 {
            clamp01((t - keys[a].t()) / span)
        } else {
            1.0
        };
        keys[b].ease().apply(raw)
    } else {
        0.0
    };
    Some((a, b, w))
}

// ============================================================================
//  sample result
// ============================================================================

/// `makeSampleResult()`'s `lhand` sub-object. `clips.js:105`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LHandSample {
    pub pos: [f64; 3],
    pub finger: [f64; 3],
    pub back: [f64; 3],
    pub pose: Pose,
    pub weight: f64,
}

/// `makeSampleResult()`'s `parts` sub-object. `clips.js:106`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PartsSample {
    pub mag: f64,
    pub mag_visible: bool,
    pub charge: f64,
    pub bolt: f64,
    pub slide: f64,
}

/// The preallocated result buffer `Clip.sample` writes into.
/// `makeSampleResult()`, `clips.js:100-108`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SampleResult {
    pub active: bool,
    pub pos: [f64; 3],
    pub rot: [f64; 3],
    pub lhand: LHandSample,
    pub parts: PartsSample,
}

impl Default for SampleResult {
    /// `makeSampleResult()`, `clips.js:101-107`.
    fn default() -> Self {
        SampleResult {
            active: false,
            pos: [0.0, 0.0, 0.0],
            rot: [0.0, 0.0, 0.0],
            lhand: LHandSample {
                pos: [0.0, 0.0, 0.0],
                finger: [0.0, 0.0, 0.0],
                back: [0.0, 0.0, 0.0],
                pose: Pose::Wrap,
                weight: 0.0,
            },
            parts: PartsSample {
                mag: 0.0,
                mag_visible: true,
                charge: 0.0,
                bolt: 0.0,
                slide: 0.0,
            },
        }
    }
}

/// `makeSampleResult()`. `clips.js:100-108`.
pub fn make_sample_result() -> SampleResult {
    SampleResult::default()
}

// ============================================================================
//  clip
// ============================================================================

/// One authored animation clip. `class Clip`, `clips.js:44-98`.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    pub name: &'static str,
    pub duration: f64,
    /// `channels.weapon ?? null`. `clips.js:48`.
    pub weapon: Option<Vec<WeaponKey>>,
    /// `channels.lhand ?? null`. `clips.js:49`.
    pub lhand: Option<Vec<LHandKey>>,
    /// `channels.parts ?? null`. `clips.js:51`.
    pub parts: Option<Vec<PartsKey>>,
    /// `channels.events ?? []`. `clips.js:52`.
    pub events: Vec<ClipEvent>,
}

impl Clip {
    /// Sample into a preallocated result object. `sample(t, out)`,
    /// `clips.js:56-97`.
    ///
    /// The source returns `out` at the end (`clips.js:96`) purely for call-site
    /// chaining convenience; every caller already holds `out` by reference, so
    /// this returns nothing and callers keep using the `&mut SampleResult`
    /// they passed in.
    pub fn sample(&self, t: f64, out: &mut SampleResult) {
        out.active = true;

        // ---- weapon additive pose ----
        match &self.weapon {
            Some(keys) => {
                if let Some((a, b, w)) = locate(keys, t) {
                    let (ka, kb) = (&keys[a], &keys[b]);
                    (0..3).for_each(|k| {
                        out.pos[k] = lerp(ka.p[k], kb.p[k], w);
                        out.rot[k] = lerp(ka.r[k], kb.r[k], w);
                    });
                }
            }
            None => {
                out.pos = [0.0, 0.0, 0.0];
                out.rot = [0.0, 0.0, 0.0];
            }
        }

        // ---- support hand ----
        match &self.lhand {
            Some(keys) => {
                if let Some((a, b, w)) = locate(keys, t) {
                    let (ka, kb) = (&keys[a], &keys[b]);
                    (0..3).for_each(|k| {
                        out.lhand.pos[k] = lerp(ka.p[k], kb.p[k], w);
                        out.lhand.finger[k] = lerp(ka.finger[k], kb.finger[k], w);
                        out.lhand.back[k] = lerp(ka.back[k], kb.back[k], w);
                    });
                    // `w < 0.5 ? a.pose ?? 'wrap' : b.pose ?? 'wrap'`
                    // (`clips.js:79`) — every authored key sets `pose`, so the
                    // `?? 'wrap'` fallback never triggers in this port; `Pose`
                    // has no "unset" representation to model it either way.
                    out.lhand.pose = if w < 0.5 { ka.pose } else { kb.pose };
                    out.lhand.weight = lerp(ka.weight, kb.weight, w);
                }
            }
            None => {
                out.lhand.weight = 0.0;
            }
        }

        // ---- moving parts ----
        if let Some(keys) = &self.parts {
            if let Some((a, b, w)) = locate(keys, t) {
                let (ka, kb) = (&keys[a], &keys[b]);
                out.parts.mag = lerp(ka.mag, kb.mag, w);
                // `(b.magVisible ?? a.magVisible ?? 1) > 0.5 || w < 0.5`
                // (`clips.js:90`) — every authored `PartsKey` in `build_clips`
                // sets `mag_visible` explicitly, so `b`'s value is always
                // present and the `?? a.magVisible ?? 1` fallback chain never
                // triggers; the port reads `kb.mag_visible` directly.
                out.parts.mag_visible = kb.mag_visible > 0.5 || w < 0.5;
                out.parts.charge = lerp(ka.charge, kb.charge, w);
                out.parts.bolt = lerp(ka.bolt, kb.bolt, w);
                out.parts.slide = lerp(ka.slide, kb.slide, w);
            }
        }
    }
}

// ============================================================================
//  clip construction
// ============================================================================

/// `const v3 = (x, y, z) => [x, y, z];`. `clips.js:114`.
const fn v3(x: f64, y: f64, z: f64) -> [f64; 3] {
    [x, y, z]
}

/// The support-hand grip attachment. `nodes.gripL`, e.g.
/// `models/rifle.js:435-439`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GripNode {
    pub pos: [f64; 3],
    /// `grip.finger` (`clips.js:125`) — `None` stands in for the source's
    /// possibly-missing property (`grip.finger ?? v3(0.82, 0.5, -0.28)`).
    pub finger: Option<[f64; 3]>,
    /// `grip.back` (`clips.js:126`).
    pub back: Option<[f64; 3]>,
}

/// A weapon-space attachment point that `build_clips` reads only `.pos` from
/// — `nodes.magSeat` and `nodes.chargeRest` in the source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PosNode {
    pub pos: [f64; 3],
}

/// The subset of a weapon model's rig `nodes` that `buildClips` reads.
/// `clips.js` takes the whole `model.nodes` object
/// (`viewmodel.js:413`: `buildClips(model.nodes, def)`); the full rig
/// (`viewmodel.js`, `models/*.js`) is a separate, not-yet-ported piece of the
/// weapon system, so this carries exactly the three fields `build_clips`
/// touches — `gripL`, `magSeat.pos`, and the optional `chargeRest.pos`
/// (`clips.js:121-139`) — rather than a placeholder for the whole rig.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttachNodes {
    pub grip_l: GripNode,
    pub mag_seat: PosNode,
    /// `nodes.chargeRest` (`clips.js:137-139`) — `None` for a weapon with no
    /// charging handle; `build_clips` takes the pistol's slide-rack branch in
    /// that case (`clips.js:209-216`).
    pub charge_rest: Option<PosNode>,
}

/// Every clip one weapon owns. The source's return value is a plain object
/// literal (`clips.js:317`: `{ reloadTac, reloadEmpty, inspect, draw, holster }`).
#[derive(Debug, Clone, PartialEq)]
pub struct Clips {
    pub reload_tac: Clip,
    pub reload_empty: Clip,
    pub inspect: Clip,
    pub draw: Clip,
    pub holster: Clip,
}

/// Build every clip for one weapon from its attachment nodes. `scale`
/// (`tac`/`emp`/`insp`/`drawT`/`holT` below) compresses or stretches the
/// whole timeline to the weapon's reload speed. `buildClips(nodes, def)`,
/// `clips.js:120-318`.
///
/// **Source quirk, preserved exactly — not fixed:** in `reloadTac`,
/// `reloadEmpty` and `inspect`, the *final* keyframe of the `weapon`, `lhand`
/// and `parts` tracks is authored at the literal `t: 1` (see e.g.
/// `clips.js:152`: `{ t: 1, p: v3(0, 0, 0), r: v3(0, 0, 0) }`) instead of
/// `t: 1 * scale`, unlike every other keyframe in the same track. For every
/// weapon shipped in `defs.rs` the relevant scale (`reload_tac`,
/// `reload_empty`, `inspect_time`) is always greater than one second, so that
/// final key's `t` is *smaller* than the second-to-last key's scaled `t` —
/// the track's own times are out of order. `locate` (`sampleTrack`) never
/// re-sorts; it walks forward with `while keys[i+1].t <= t`, so once elapsed
/// time first reaches the second-to-last key's scaled time, the scan happens
/// to satisfy `keys[last].t (== 1) <= t` too and jumps straight to the final
/// key, freezing the channel at its rest pose *before* the authored tail
/// (e.g. the `back`-eased overshoot key) is ever interpolated through. `draw`
/// and `holster` do not have this bug: their scales (`draw_time`,
/// `holster_time`) are always under one second, so the literal `1` is
/// legitimately their largest keyframe time. Pinned in
/// `tests/weapons_clips_port.rs`.
pub fn build_clips(nodes: &AttachNodes, def: &WeaponDef) -> Clips {
    let grip = nodes.grip_l;
    let seat = nodes.mag_seat.pos;
    // `def.magLen ?? 0.2` (`clips.js:123`) — `WeaponDef::mag_len` is a
    // required field in this port (every real weapon supplies it), so the
    // fallback is unreachable here; kept only as this comment, matching how
    // `defs.rs` documents the same kind of dead JS default.
    let mag_len = def.mag_len;
    // Support-hand orientation while it is holding the weapon vs. a magazine.
    let wrap_finger = grip.finger.unwrap_or(v3(0.82, 0.5, -0.28));
    let wrap_back = grip.back.unwrap_or(v3(-0.5, 0.32, -0.8));
    let mag_finger = v3(0.1, 0.72, -0.68);
    let mag_back = v3(-0.86, 0.34, -0.38);
    let hg_p = grip.pos;

    // Points the support hand visits, all in weapon space.
    let at_mag = v3(seat[0] + 0.012, seat[1] - mag_len * 0.62, seat[2] + 0.012);
    let below_gun = v3(seat[0] + 0.05, seat[1] - mag_len * 1.5, seat[2] + 0.09);
    let off_frame = v3(seat[0] + 0.11, seat[1] - mag_len * 2.0, seat[2] + 0.16);
    let mag_high = v3(seat[0] + 0.006, seat[1] - mag_len * 0.78, seat[2] + 0.008);
    let seated = v3(seat[0], seat[1] - mag_len * 0.62, seat[2]);
    let charge = nodes
        .charge_rest
        .map(|c| v3(c.pos[0] - 0.02, c.pos[1] + 0.008, c.pos[2] + 0.03));

    let tac = def.reload_tac;
    let emp = def.reload_empty;

    // ------------------------------------------------------------ tactical
    let reload_tac = {
        let weapon = vec![
            WeaponKey { t: 0.0, p: v3(0.0, 0.0, 0.0), r: v3(0.0, 0.0, 0.0), ease: Ease::Smooth },
            WeaponKey {
                t: 0.12 * tac,
                p: v3(0.014, -0.026, 0.03),
                r: v3(-0.14, 0.3, 0.42),
                ease: Ease::Smooth,
            },
            WeaponKey {
                t: 0.5 * tac,
                p: v3(0.016, -0.03, 0.026),
                r: v3(-0.1, 0.34, 0.5),
                ease: Ease::Smooth,
            },
            WeaponKey {
                t: 0.72 * tac,
                p: v3(0.012, -0.022, 0.022),
                r: v3(-0.12, 0.26, 0.44),
                ease: Ease::Smooth,
            },
            WeaponKey {
                t: 0.78 * tac,
                p: v3(0.008, -0.008, 0.014),
                r: v3(-0.05, 0.18, 0.3),
                ease: Ease::Back,
            },
            // Source quirk: literal `t: 1`, not `1 * tac` — see build_clips's doc.
            WeaponKey { t: 1.0, p: v3(0.0, 0.0, 0.0), r: v3(0.0, 0.0, 0.0), ease: Ease::Smooth },
        ];
        let lhand = vec![
            LHandKey {
                t: 0.0,
                p: hg_p,
                finger: wrap_finger,
                back: wrap_back,
                pose: Pose::Wrap,
                weight: 1.0,
                ease: Ease::Smooth,
            },
            LHandKey {
                t: 0.1 * tac,
                p: at_mag,
                finger: mag_finger,
                back: mag_back,
                pose: Pose::Pinch,
                weight: 1.0,
                ease: Ease::Smooth,
            },
            LHandKey {
                t: 0.2 * tac,
                p: at_mag,
                finger: mag_finger,
                back: mag_back,
                pose: Pose::Pinch,
                weight: 1.0,
                ease: Ease::Smooth,
            },
            LHandKey {
                t: 0.3 * tac,
                p: below_gun,
                finger: mag_finger,
                back: mag_back,
                pose: Pose::Pinch,
                weight: 1.0,
                ease: Ease::Out,
            },
            LHandKey {
                t: 0.42 * tac,
                p: off_frame,
                finger: mag_finger,
                back: mag_back,
                pose: Pose::Open,
                weight: 1.0,
                ease: Ease::Smooth,
            },
            LHandKey {
                t: 0.56 * tac,
                p: off_frame,
                finger: mag_finger,
                back: mag_back,
                pose: Pose::Pinch,
                weight: 1.0,
                ease: Ease::Smooth,
            },
            LHandKey {
                t: 0.68 * tac,
                p: below_gun,
                finger: mag_finger,
                back: mag_back,
                pose: Pose::Pinch,
                weight: 1.0,
                ease: Ease::Smooth,
            },
            LHandKey {
                t: 0.76 * tac,
                p: mag_high,
                finger: mag_finger,
                back: mag_back,
                pose: Pose::Pinch,
                weight: 1.0,
                ease: Ease::Out,
            },
            LHandKey {
                t: 0.8 * tac,
                p: seated,
                finger: mag_finger,
                back: mag_back,
                pose: Pose::Pinch,
                weight: 1.0,
                ease: Ease::Smooth,
            },
            LHandKey {
                t: 0.86 * tac,
                p: v3(seated[0], seated[1] - 0.012, seated[2]),
                finger: mag_finger,
                back: mag_back,
                pose: Pose::Open,
                weight: 1.0,
                ease: Ease::Smooth,
            },
            // Source quirk: literal `t: 1`, not `1 * tac` — see build_clips's doc.
            LHandKey {
                t: 1.0,
                p: hg_p,
                finger: wrap_finger,
                back: wrap_back,
                pose: Pose::Wrap,
                weight: 1.0,
                ease: Ease::Out,
            },
        ];
        let parts = vec![
            PartsKey { t: 0.0, mag: 0.0, mag_visible: 1.0, bolt: 0.0, slide: 0.0, charge: 0.0, ease: Ease::Smooth },
            PartsKey { t: 0.16 * tac, mag: 0.0, mag_visible: 1.0, bolt: 0.0, slide: 0.0, charge: 0.0, ease: Ease::Smooth },
            PartsKey { t: 0.2 * tac, mag: 1.0, mag_visible: 1.0, bolt: 0.0, slide: 0.0, charge: 0.0, ease: Ease::Smooth },
            PartsKey { t: 0.3 * tac, mag: 1.0, mag_visible: 1.0, bolt: 0.0, slide: 0.0, charge: 0.0, ease: Ease::Linear },
            PartsKey { t: 0.34 * tac, mag: 1.0, mag_visible: 0.0, bolt: 0.0, slide: 0.0, charge: 0.0, ease: Ease::Smooth },
            PartsKey { t: 0.66 * tac, mag: 1.0, mag_visible: 0.0, bolt: 0.0, slide: 0.0, charge: 0.0, ease: Ease::Smooth },
            PartsKey { t: 0.68 * tac, mag: 1.0, mag_visible: 1.0, bolt: 0.0, slide: 0.0, charge: 0.0, ease: Ease::Smooth },
            PartsKey { t: 0.79 * tac, mag: 1.0, mag_visible: 1.0, bolt: 0.0, slide: 0.0, charge: 0.0, ease: Ease::Out },
            PartsKey { t: 0.81 * tac, mag: 0.0, mag_visible: 1.0, bolt: 0.0, slide: 0.0, charge: 0.0, ease: Ease::Smooth },
            // Source quirk: literal `t: 1`, not `1 * tac` — see build_clips's doc.
            PartsKey { t: 1.0, mag: 0.0, mag_visible: 1.0, bolt: 0.0, slide: 0.0, charge: 0.0, ease: Ease::Smooth },
        ];
        let events = vec![
            ClipEvent { t: 0.02 * tac, name: "start" },
            ClipEvent { t: 0.2 * tac, name: "magout" },
            ClipEvent { t: 0.34 * tac, name: "magdrop" },
            ClipEvent { t: 0.81 * tac, name: "magin" },
            ClipEvent { t: 0.88 * tac, name: "slap" },
            ClipEvent { t: 0.995 * tac, name: "end" },
        ];
        Clip {
            name: "reloadTac",
            duration: tac,
            weapon: Some(weapon),
            lhand: Some(lhand),
            parts: Some(parts),
            events,
        }
    };

    // -------------------------------------------------------------- empty
    let mut empty_lhand = vec![
        LHandKey {
            t: 0.0,
            p: hg_p,
            finger: wrap_finger,
            back: wrap_back,
            pose: Pose::Wrap,
            weight: 1.0,
            ease: Ease::Smooth,
        },
        LHandKey {
            t: 0.08 * emp,
            p: at_mag,
            finger: mag_finger,
            back: mag_back,
            pose: Pose::Pinch,
            weight: 1.0,
            ease: Ease::Smooth,
        },
        LHandKey {
            t: 0.16 * emp,
            p: at_mag,
            finger: mag_finger,
            back: mag_back,
            pose: Pose::Pinch,
            weight: 1.0,
            ease: Ease::Smooth,
        },
        LHandKey {
            t: 0.26 * emp,
            p: below_gun,
            finger: mag_finger,
            back: mag_back,
            pose: Pose::Open,
            weight: 1.0,
            ease: Ease::Out,
        },
        LHandKey {
            t: 0.36 * emp,
            p: off_frame,
            finger: mag_finger,
            back: mag_back,
            pose: Pose::Open,
            weight: 1.0,
            ease: Ease::Smooth,
        },
        LHandKey {
            t: 0.48 * emp,
            p: off_frame,
            finger: mag_finger,
            back: mag_back,
            pose: Pose::Pinch,
            weight: 1.0,
            ease: Ease::Smooth,
        },
        LHandKey {
            t: 0.58 * emp,
            p: below_gun,
            finger: mag_finger,
            back: mag_back,
            pose: Pose::Pinch,
            weight: 1.0,
            ease: Ease::Smooth,
        },
        LHandKey {
            t: 0.66 * emp,
            p: mag_high,
            finger: mag_finger,
            back: mag_back,
            pose: Pose::Pinch,
            weight: 1.0,
            ease: Ease::Out,
        },
        LHandKey {
            t: 0.7 * emp,
            p: seated,
            finger: mag_finger,
            back: mag_back,
            pose: Pose::Pinch,
            weight: 1.0,
            ease: Ease::Smooth,
        },
        LHandKey {
            t: 0.75 * emp,
            p: v3(seated[0], seated[1] - 0.01, seated[2]),
            finger: mag_finger,
            back: mag_back,
            pose: Pose::Open,
            weight: 1.0,
            ease: Ease::Smooth,
        },
    ];
    match charge {
        Some(charge) => {
            let charge_finger = v3(0.55, 0.2, 0.81);
            let charge_back = v3(-0.2, 0.94, -0.27);
            empty_lhand.push(LHandKey {
                t: 0.82 * emp,
                p: v3(charge[0], charge[1], charge[2] - 0.01),
                finger: charge_finger,
                back: charge_back,
                pose: Pose::Pinch,
                weight: 1.0,
                ease: Ease::Out,
            });
            empty_lhand.push(LHandKey {
                t: 0.87 * emp,
                p: charge,
                finger: charge_finger,
                back: charge_back,
                pose: Pose::Pinch,
                weight: 1.0,
                ease: Ease::Smooth,
            });
            empty_lhand.push(LHandKey {
                t: 0.9 * emp,
                p: v3(charge[0], charge[1], charge[2] + 0.07),
                finger: charge_finger,
                back: charge_back,
                pose: Pose::Pinch,
                weight: 1.0,
                ease: Ease::Linear,
            });
            empty_lhand.push(LHandKey {
                t: 0.93 * emp,
                p: v3(charge[0], charge[1], charge[2] + 0.02),
                finger: charge_finger,
                back: charge_back,
                pose: Pose::Open,
                weight: 1.0,
                ease: Ease::Out,
            });
        }
        // Pistol: the support hand racks the slide from above.
        None => {
            let slide_finger = v3(0.7, -0.3, 0.65);
            let slide_back = v3(0.1, 0.94, 0.32);
            empty_lhand.push(LHandKey {
                t: 0.82 * emp,
                p: v3(-0.02, seat[1] + 0.06, seat[2] - 0.05),
                finger: slide_finger,
                back: slide_back,
                pose: Pose::Pinch,
                weight: 1.0,
                ease: Ease::Out,
            });
            empty_lhand.push(LHandKey {
                t: 0.88 * emp,
                p: v3(-0.02, seat[1] + 0.06, seat[2] - 0.02),
                finger: slide_finger,
                back: slide_back,
                pose: Pose::Pinch,
                weight: 1.0,
                ease: Ease::Linear,
            });
            empty_lhand.push(LHandKey {
                t: 0.92 * emp,
                p: v3(-0.02, seat[1] + 0.06, seat[2] - 0.06),
                finger: slide_finger,
                back: slide_back,
                pose: Pose::Open,
                weight: 1.0,
                ease: Ease::Out,
            });
        }
    }
    // Source quirk: literal `t: 1`, not `1 * emp` — see build_clips's doc.
    empty_lhand.push(LHandKey {
        t: 1.0,
        p: hg_p,
        finger: wrap_finger,
        back: wrap_back,
        pose: Pose::Wrap,
        weight: 1.0,
        ease: Ease::Out,
    });

    let empty_parts = vec![
        PartsKey { t: 0.0, mag: 0.0, mag_visible: 1.0, bolt: 1.0, slide: 1.0, charge: 0.0, ease: Ease::Smooth },
        PartsKey { t: 0.12 * emp, mag: 0.0, mag_visible: 1.0, bolt: 1.0, slide: 1.0, charge: 0.0, ease: Ease::Smooth },
        PartsKey { t: 0.16 * emp, mag: 1.0, mag_visible: 1.0, bolt: 1.0, slide: 1.0, charge: 0.0, ease: Ease::Smooth },
        PartsKey { t: 0.26 * emp, mag: 1.0, mag_visible: 1.0, bolt: 1.0, slide: 1.0, charge: 0.0, ease: Ease::Linear },
        PartsKey { t: 0.3 * emp, mag: 1.0, mag_visible: 0.0, bolt: 1.0, slide: 1.0, charge: 0.0, ease: Ease::Smooth },
        PartsKey { t: 0.56 * emp, mag: 1.0, mag_visible: 0.0, bolt: 1.0, slide: 1.0, charge: 0.0, ease: Ease::Smooth },
        PartsKey { t: 0.58 * emp, mag: 1.0, mag_visible: 1.0, bolt: 1.0, slide: 1.0, charge: 0.0, ease: Ease::Smooth },
        PartsKey { t: 0.69 * emp, mag: 1.0, mag_visible: 1.0, bolt: 1.0, slide: 1.0, charge: 0.0, ease: Ease::Out },
        PartsKey { t: 0.71 * emp, mag: 0.0, mag_visible: 1.0, bolt: 1.0, slide: 1.0, charge: 0.0, ease: Ease::Smooth },
        PartsKey { t: 0.86 * emp, mag: 0.0, mag_visible: 1.0, bolt: 1.0, slide: 1.0, charge: 0.0, ease: Ease::Smooth },
        PartsKey { t: 0.9 * emp, mag: 0.0, mag_visible: 1.0, bolt: 1.0, slide: 1.0, charge: 1.0, ease: Ease::Linear },
        PartsKey { t: 0.915 * emp, mag: 0.0, mag_visible: 1.0, bolt: 0.0, slide: 0.0, charge: 0.0, ease: Ease::Back },
        PartsKey { t: 1.0, mag: 0.0, mag_visible: 1.0, bolt: 0.0, slide: 0.0, charge: 0.0, ease: Ease::Smooth },
    ];

    let reload_empty = {
        let weapon = vec![
            WeaponKey { t: 0.0, p: v3(0.0, 0.0, 0.0), r: v3(0.0, 0.0, 0.0), ease: Ease::Smooth },
            WeaponKey {
                t: 0.1 * emp,
                p: v3(0.016, -0.03, 0.032),
                r: v3(-0.16, 0.34, 0.46),
                ease: Ease::Smooth,
            },
            WeaponKey {
                t: 0.44 * emp,
                p: v3(0.018, -0.034, 0.028),
                r: v3(-0.12, 0.38, 0.54),
                ease: Ease::Smooth,
            },
            WeaponKey {
                t: 0.7 * emp,
                p: v3(0.014, -0.026, 0.024),
                r: v3(-0.14, 0.3, 0.48),
                ease: Ease::Smooth,
            },
            WeaponKey {
                t: 0.72 * emp,
                p: v3(0.01, -0.014, 0.018),
                r: v3(-0.06, 0.24, 0.38),
                ease: Ease::Back,
            },
            WeaponKey {
                t: 0.86 * emp,
                p: v3(0.006, -0.012, 0.016),
                r: v3(-0.02, 0.42, 0.22),
                ease: Ease::Smooth,
            },
            WeaponKey {
                t: 0.92 * emp,
                p: v3(0.004, -0.006, 0.022),
                r: v3(0.02, 0.44, 0.18),
                ease: Ease::Linear,
            },
            // Source quirk: literal `t: 1`, not `1 * emp` — see build_clips's doc.
            WeaponKey { t: 1.0, p: v3(0.0, 0.0, 0.0), r: v3(0.0, 0.0, 0.0), ease: Ease::Out },
        ];
        let events = vec![
            ClipEvent { t: 0.02 * emp, name: "start" },
            ClipEvent { t: 0.16 * emp, name: "magout" },
            ClipEvent { t: 0.3 * emp, name: "magdrop" },
            ClipEvent { t: 0.71 * emp, name: "magin" },
            ClipEvent { t: 0.9 * emp, name: "charge" },
            ClipEvent { t: 0.917 * emp, name: "boltrelease" },
            ClipEvent { t: 0.995 * emp, name: "end" },
        ];
        Clip {
            name: "reloadEmpty",
            duration: emp,
            weapon: Some(weapon),
            lhand: Some(empty_lhand),
            parts: Some(empty_parts),
            events,
        }
    };

    // ------------------------------------------------------------- inspect
    let insp = def.inspect_time;
    let inspect = Clip {
        name: "inspect",
        duration: insp,
        weapon: Some(vec![
            WeaponKey { t: 0.0, p: v3(0.0, 0.0, 0.0), r: v3(0.0, 0.0, 0.0), ease: Ease::Smooth },
            WeaponKey {
                t: 0.16 * insp,
                p: v3(-0.03, -0.012, 0.075),
                r: v3(0.1, -0.62, -0.34),
                ease: Ease::Smooth,
            },
            WeaponKey {
                t: 0.34 * insp,
                p: v3(-0.026, -0.006, 0.085),
                r: v3(-0.05, -0.78, -0.5),
                ease: Ease::Smooth,
            },
            WeaponKey {
                t: 0.52 * insp,
                p: v3(0.01, -0.02, 0.07),
                r: v3(0.22, 0.5, 0.9),
                ease: Ease::Smooth,
            },
            WeaponKey {
                t: 0.7 * insp,
                p: v3(0.012, -0.024, 0.055),
                r: v3(0.3, 0.62, 1.15),
                ease: Ease::Smooth,
            },
            WeaponKey {
                t: 0.86 * insp,
                p: v3(-0.006, -0.01, 0.03),
                r: v3(0.06, -0.18, 0.2),
                ease: Ease::Smooth,
            },
            // Source quirk: literal `t: 1`, not `1 * insp` — see build_clips's doc.
            WeaponKey { t: 1.0, p: v3(0.0, 0.0, 0.0), r: v3(0.0, 0.0, 0.0), ease: Ease::Out },
        ]),
        lhand: Some(vec![
            LHandKey {
                t: 0.0,
                p: hg_p,
                finger: wrap_finger,
                back: wrap_back,
                pose: Pose::Wrap,
                weight: 1.0,
                ease: Ease::Smooth,
            },
            LHandKey {
                t: 0.3 * insp,
                p: v3(hg_p[0] - 0.01, hg_p[1] - 0.01, hg_p[2] + 0.03),
                finger: wrap_finger,
                back: wrap_back,
                pose: Pose::Clamp,
                weight: 1.0,
                ease: Ease::Smooth,
            },
            LHandKey {
                t: 0.55 * insp,
                p: v3(hg_p[0] + 0.01, hg_p[1] - 0.02, hg_p[2] - 0.02),
                finger: wrap_finger,
                back: wrap_back,
                pose: Pose::Wrap,
                weight: 1.0,
                ease: Ease::Smooth,
            },
            // Source quirk: literal `t: 1`, not `1 * insp` — see build_clips's doc.
            LHandKey {
                t: 1.0,
                p: hg_p,
                finger: wrap_finger,
                back: wrap_back,
                pose: Pose::Wrap,
                weight: 1.0,
                ease: Ease::Out,
            },
        ]),
        parts: Some(vec![
            PartsKey { t: 0.0, mag: 0.0, mag_visible: 1.0, bolt: 0.0, slide: 0.0, charge: 0.0, ease: Ease::Smooth },
            PartsKey { t: 1.0, mag: 0.0, mag_visible: 1.0, bolt: 0.0, slide: 0.0, charge: 0.0, ease: Ease::Smooth },
        ]),
        events: vec![ClipEvent { t: 0.995 * insp, name: "end" }],
    };

    // --------------------------------------------------------- draw/holster
    let draw_t = def.draw_time;
    let draw = Clip {
        name: "draw",
        duration: draw_t,
        weapon: Some(vec![
            WeaponKey {
                t: 0.0,
                p: v3(0.05, -0.3, 0.14),
                r: v3(-0.85, 0.5, 0.55),
                ease: Ease::Smooth,
            },
            WeaponKey {
                t: 0.55 * draw_t,
                p: v3(0.01, -0.03, 0.02),
                r: v3(-0.1, 0.06, 0.06),
                ease: Ease::Out,
            },
            WeaponKey {
                t: 0.78 * draw_t,
                p: v3(-0.004, 0.008, -0.006),
                r: v3(0.04, -0.02, -0.02),
                ease: Ease::Smooth,
            },
            WeaponKey { t: 1.0, p: v3(0.0, 0.0, 0.0), r: v3(0.0, 0.0, 0.0), ease: Ease::Out },
        ]),
        lhand: Some(vec![
            LHandKey {
                t: 0.0,
                p: v3(hg_p[0] - 0.02, hg_p[1] - 0.09, hg_p[2] + 0.06),
                finger: wrap_finger,
                back: wrap_back,
                pose: Pose::Open,
                weight: 1.0,
                ease: Ease::Smooth,
            },
            LHandKey {
                t: 0.6 * draw_t,
                p: hg_p,
                finger: wrap_finger,
                back: wrap_back,
                pose: Pose::Wrap,
                weight: 1.0,
                ease: Ease::Out,
            },
            LHandKey {
                t: 1.0,
                p: hg_p,
                finger: wrap_finger,
                back: wrap_back,
                pose: Pose::Wrap,
                weight: 1.0,
                ease: Ease::Smooth,
            },
        ]),
        parts: Some(vec![PartsKey {
            t: 0.0,
            mag: 0.0,
            mag_visible: 1.0,
            bolt: 0.0,
            slide: 0.0,
            charge: 0.0,
            ease: Ease::Smooth,
        }]),
        events: vec![ClipEvent { t: 0.995 * draw_t, name: "end" }],
    };

    let hol_t = def.holster_time;
    let holster = Clip {
        name: "holster",
        duration: hol_t,
        weapon: Some(vec![
            WeaponKey { t: 0.0, p: v3(0.0, 0.0, 0.0), r: v3(0.0, 0.0, 0.0), ease: Ease::Smooth },
            WeaponKey {
                t: 0.25 * hol_t,
                p: v3(0.004, 0.014, -0.01),
                r: v3(0.08, -0.04, -0.05),
                ease: Ease::Smooth,
            },
            WeaponKey {
                t: 1.0,
                p: v3(0.05, -0.32, 0.15),
                r: v3(-0.9, 0.55, 0.6),
                ease: Ease::Out,
            },
        ]),
        lhand: Some(vec![
            LHandKey {
                t: 0.0,
                p: hg_p,
                finger: wrap_finger,
                back: wrap_back,
                pose: Pose::Wrap,
                weight: 1.0,
                ease: Ease::Smooth,
            },
            LHandKey {
                t: 1.0,
                p: v3(hg_p[0] - 0.02, hg_p[1] - 0.1, hg_p[2] + 0.07),
                finger: wrap_finger,
                back: wrap_back,
                pose: Pose::Open,
                weight: 1.0,
                ease: Ease::Out,
            },
        ]),
        parts: Some(vec![PartsKey {
            t: 0.0,
            mag: 0.0,
            mag_visible: 1.0,
            bolt: 0.0,
            slide: 0.0,
            charge: 0.0,
            ease: Ease::Smooth,
        }]),
        events: vec![ClipEvent { t: 0.995 * hol_t, name: "end" }],
    };

    Clips {
        reload_tac,
        reload_empty,
        inspect,
        draw,
        holster,
    }
}
