//! **EV100 metering** — the physical exposure the reference drives every frame
//! from, transcribed from its GLSL.
//!
//! Ported from Claude-of-Duty `src/render/exposure.js` (the `LOGLUM`, `REDUCE`
//! and `ADAPT` passes and the `AutoExposure` chain that runs them), plus the
//! limits and key `src/render/index.js:213,342` sets on it.
//!
//! # The shape of the measurement, because it is not the usual one
//!
//! It is **not** a fixed key, and it is **not** a plain average. It is a
//! four-level GPU reduction of a *centre-weighted, sky-de-weighted, per-tap
//! clamped log luminance*, with an asymmetric temporal adaptation on top and a
//! separate key scale at the end. Every one of those five qualifiers changes the
//! number, so all five are ported:
//!
//! 1. **The chain.** `64x64 -> 16x16 -> 4x4 -> 1x1`, each step a 4x4 box of the
//!    previous (`exposure.js:175-186`). The first level takes four bilinear taps
//!    of the *scene* at `±1` texel, so a 1280x720 frame is folded through
//!    ~16k weighted samples with no readback and no stall.
//! 2. **The per-tap clamp** ([`TAP_CLAMP`], 40). The solar disc is authored at
//!    radiance 4000 and a specular hit off a rail can be worse; one such pixel
//!    inside a 4-tap box would drag the whole log average by stops
//!    (`exposure.js:32-38`).
//! 3. **Centre weighting.** `w = exp( -dot(d, d) * 1.1 )` over
//!    `d = (uv - 0.5) * 2`, so the middle of the frame — what the player aims at
//!    — dominates.
//! 4. **Sky de-weight, ramped by the sky's own luminance.** Where the depth
//!    channel is `0` (nothing was written) or past [`FAR_DISTANCE`], the weight
//!    is scaled toward [`SKY_WEIGHT`] — but only as the luminance crosses
//!    [`SKY_KNEE`]. The ramp is the point: a moonlit night sky is legitimate
//!    scene content and the only absolute anchor the meter has after dark, so
//!    de-weighting it unconditionally makes night adapt up into an overcast
//!    afternoon (`exposure.js:51-60`).
//! 5. **The key.** `exposure = key / (1.2 * 2^ev)`, with `key = 1.06`
//!    (`index.js:342`). `1.2` is `78 / (q * S)` at `q = 0.65`, `S = 100` — the
//!    saturation-based speed constant, spelled out in the source header.
//!
//! Adaptation is asymmetric ([`SPEED_UP`] 1.4, [`SPEED_DOWN`] 3.2) and clamped
//! into `[minEv, maxEv]`, and the *whole history* is a single 1x1 float texel
//! ping-ponged between two targets: `prevEv` is the `.g` lane of last frame's
//! result. There is no CPU-side accumulator, which is why [`adapt`] takes the
//! previous EV as an argument rather than owning state.
//!
//! # The metered EV is scene-relative, and that agrees with the sky
//!
//! `exposure.js:107` computes `log2( lum * 100 / 12.5 )` on a **framebuffer
//! radiance unit**, not on cd/m². `apps/shmup/src/sky/atmosphere.rs`'s
//! photometric contract fixes that unit: 1 framebuffer radiance unit =
//! [`SCENE_LUX`] (25 000) cd/m². So the shader's `ev100` is a true EV100 minus
//! `log2(25000) = 14.61` stops, and [`photometric_ev100`] is that conversion.
//!
//! This is not a discrepancy to correct — it is the same contract, expressed in
//! the engine's unit — and it is *checkable*, which is why
//! `tests::the_metered_ev_agrees_with_the_skys_photometric_contract` exists.
//! A sunlit stucco wall is ~0.32 radiance units by the sky module's own
//! reckoning; metered here that is EV 1.36, which converts to a true EV100 of
//! 15.97 — the textbook "sunny 16" reading. Had the stray `pi` the sky module
//! records as its 1.65-stop bug still been in place, this test would read 17.6
//! and fail. The renderer's own comment (`index.js:209-213`) is the other
//! anchor: daylight frames meter between -1 and -2.1 and a moonlit street at
//! -5.2, which is why the lower limit is [`SCENE_MIN_EV`] (-4.3) — a night lock,
//! not a clamp that ever binds in daylight.
//!
//! # What is in `AutoExposure` and is not here
//!
//! `this.manual = 1.0` and `this.enabled = true` (`exposure.js:151-152`) are
//! **dead**: nothing in the source reads either one. `autoExposure` is a
//! *renderer* setting and it works by passing `dt = 1e3` (`index.js:1497`),
//! which drives `k` to 1 and makes the adaptation a snap — not by consulting
//! `enabled`. Recorded rather than ported, because neither field carries any
//! arithmetic to transcribe: there is no class here, only the functions the
//! passes call. The rest of the class is target allocation, ping-pong
//! bookkeeping and `dispose`, all of which is wiring.
//!
//! # What must be linear HDR, and what is not today
//!
//! Both `owMeterTap` and the tone map downstream of it read **linear scene
//! radiance**. This crate's scene intermediate is still 8-bit sRGB
//! (`crate::surface_encode::scene_target_format` returns the surface format with
//! an sRGB suffix), so a fragment that emitted 4.0 was clamped to white before
//! any post pass could see it, and [`TAP_CLAMP`]'s 40 — an eight-times-over-white
//! ceiling whose entire job is to survive a sun disc at 4000 — could never once
//! bind. `axiom_host::RenderCapability::HdrTargets` has landed as a *declaration*
//! (`crate::hdr_target`), but nothing yet allocates the `Rgba16Float`
//! intermediate it licenses. Metering an already-clamped buffer measures the
//! clamp, so this module is inert until that target exists; see the report notes
//! for the exact wiring that is missing.

use axiom_math::{Vec2, Vec3, Vec4};

/// `owLum`, from the shared `COMMON` chunk (`glsl.js:31`).
///
/// One Rust definition for the whole crate — the source has one too. The two
/// WGSL strings each carry their own copy of the body because each has to
/// concatenate standalone in front of a different pass;
/// `tests::the_two_wgsl_luminance_bodies_are_the_same_expression` holds them
/// to being the same text.
use crate::agx::lum;

/// The metering weight a de-weighted sky keeps (`uMeter.x`, `exposure.js:130`).
pub(crate) const SKY_WEIGHT: f32 = 0.15;

/// Metres of linear view depth past which a fragment counts as aerial
/// perspective, i.e. mostly sky (`uMeter.y`, `exposure.js:130`).
pub(crate) const FAR_DISTANCE: f32 = 400.0;

/// The per-tap luminance ceiling applied **before** the log (`uMeter.z`,
/// `exposure.js:130`). See the module docs: the solar disc is authored at 4000.
pub(crate) const TAP_CLAMP: f32 = 40.0;

/// The luminance range over which the sky de-weight ramps in (`uSkyKnee`,
/// `exposure.js:131`).
pub(crate) const SKY_KNEE: Vec2 = Vec2::new(0.06, 0.3);

/// Adaptation rate when the image is getting **brighter** — i.e. when the
/// metered EV *falls* (`uParams.y`, `exposure.js:140`). The eye brightens up
/// slowly.
pub(crate) const SPEED_UP: f32 = 1.4;

/// Adaptation rate when the image is getting **darker** — i.e. when the metered
/// EV *rises* (`uParams.z`, `exposure.js:140`). The eye darkens down quickly.
pub(crate) const SPEED_DOWN: f32 = 3.2;

/// The largest timestep the adaptation will accept (`exposure.js:191`,
/// `Math.min(dt, 0.1)`). A hitch must not teleport the exposure.
pub(crate) const MAX_ADAPT_DT: f32 = 0.1;

/// `78 / (q * S)` with `q = 0.65`, `S = 100` — the saturation-based speed
/// constant that turns an EV100 into a maximum luminance (`exposure.js:117`).
pub(crate) const SPEED_CONSTANT: f32 = 1.2;

/// The key scale the renderer sets (`settings.exposureKey`, `index.js:342`).
/// Divides the saturation luminance, so above 1 is brighter.
pub(crate) const EXPOSURE_KEY: f32 = 1.06;

/// The EV floor the renderer sets (`index.js:213`, `setLimits(-4.3, 20)`).
///
/// This is the **night exposure lock**: a moonlit street meters at -5.2, and
/// letting the meter chase that turns night into an overcast afternoon. Daylight
/// meters between -1 and -2.1, so it only ever binds after dark.
pub(crate) const SCENE_MIN_EV: f32 = -4.3;

/// The EV ceiling the renderer sets (`index.js:213`) — headroom for a
/// physically-scaled sky.
pub(crate) const SCENE_MAX_EV: f32 = 20.0;

/// The four sizes of the reduction chain (`exposure.js:145-148`).
pub(crate) const CHAIN_SIZES: [u32; 4] = [64, 16, 4, 1];

/// Lux per framebuffer radiance unit — the sky module's photometric contract
/// (`src/sky/atmosphere.js:53`, `SCENE_LUX`). See the module docs.
pub(crate) const SCENE_LUX: f32 = 25000.0;

/// A metered EV (this module's, in scene radiance units) as a **true EV100** in
/// cd/m².
///
/// `EV100 = log2(L * 100 / K)` with `L` in cd/m², and `L = scene * SCENE_LUX`,
/// so the two differ by exactly `log2(SCENE_LUX)`. Written as the log of the
/// constant rather than a baked 14.61 so the two numbers cannot drift apart.
pub(crate) fn photometric_ev100(metered_ev: f32) -> f32 {
    metered_ev + SCENE_LUX.log2()
}

/// The metering chain as WGSL: the same functions as `LOGLUM`, `REDUCE` and
/// `ADAPT`, taking their fetched values as arguments instead of sampling.
///
/// Split this way for the same reason `material_shader`'s layers are: the
/// texture fetches are wiring and the arithmetic is the algorithm, and only the
/// arithmetic can be held against a CPU reference. [`EXPOSURE_PASS_WGSL`] is the
/// wiring, and it calls exactly these.
///
/// `clamp`, `mix`, `dot` and `smoothstep` are written out; WGSL's builtins may
/// factor differently from GLSL's.
pub(crate) const EXPOSURE_WGSL: &str = r#"
// EV100 metering, from Claude-of-Duty `src/render/exposure.js`.

fn axiom_meter_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    return min(max(x, lo), hi);
}

fn axiom_meter_mix(a: f32, b: f32, t: f32) -> f32 {
    return a * (1.0 - t) + b * t;
}

fn axiom_meter_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = axiom_meter_clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// `owLum` (`glsl.js:31`).
fn axiom_meter_lum(c: vec3<f32>) -> f32 {
    return c.x * 0.2126 + c.y * 0.7152 + c.z * 0.0722;
}

// `owMeterTap` (`exposure.js:35-38`), with the fetch hoisted out. The clamp is
// applied to the LUMINANCE, after the negative floor and before the log.
fn axiom_meter_tap(c: vec3<f32>, tap_clamp: f32) -> f32 {
    let m = vec3<f32>(max(c.x, 0.0), max(c.y, 0.0), max(c.z, 0.0));
    return min(axiom_meter_lum(m), tap_clamp);
}

// The centre weight (`exposure.js:48-49`).
fn axiom_meter_centre_weight(uv: vec2<f32>) -> f32 {
    let d = (uv - 0.5) * 2.0;
    return exp(-(d.x * d.x + d.y * d.y) * 1.1);
}

// The sky de-weight (`exposure.js:61-66`), written with the source's two nested
// conditions rather than as arithmetic: WGSL is not held to the Branchless Law,
// and the CPU reference reaches the same value by multiplier, which is a
// stronger cross-check than two copies of the same shape.
fn axiom_meter_sky_deweight(
    w: f32,
    lum: f32,
    meter: vec4<f32>,
    sky_knee: vec2<f32>,
    depth: f32,
) -> f32 {
    var ow = w;
    if ( meter.w > 0.5 ) {
        if ( depth <= 0.0 || depth > meter.y ) {
            ow = ow * axiom_meter_mix(
                1.0, meter.x, axiom_meter_smoothstep(sky_knee.x, sky_knee.y, lum));
        }
    }
    return ow;
}

// The whole `LOGLUM` fragment body (`exposure.js:40-69`): four taps in the
// source's order, their mean floored at 1e-5, the centre weight, the sky
// de-weight, and the (weighted log, weight) pair the reduction sums.
fn axiom_meter_loglum(
    t0: vec3<f32>,
    t1: vec3<f32>,
    t2: vec3<f32>,
    t3: vec3<f32>,
    uv: vec2<f32>,
    meter: vec4<f32>,
    sky_knee: vec2<f32>,
    depth: f32,
) -> vec2<f32> {
    var lum = axiom_meter_tap(t0, meter.z);
    lum = lum + axiom_meter_tap(t1, meter.z);
    lum = lum + axiom_meter_tap(t2, meter.z);
    lum = lum + axiom_meter_tap(t3, meter.z);
    lum = max(lum * 0.25, 1e-5);

    let w = axiom_meter_sky_deweight(
        axiom_meter_centre_weight(uv), lum, meter, sky_knee, depth);
    return vec2<f32>(log2(lum) * w, w);
}

// `REDUCE`'s tap offset (`exposure.js:81`): a 4x4 box centred on the texel.
fn axiom_meter_reduce_offset(x: i32, y: i32, texel: vec2<f32>) -> vec2<f32> {
    return (vec2<f32>(f32(x), f32(y)) - 1.5) * texel;
}

// `REDUCE`'s normalisation (`exposure.js:85`).
fn axiom_meter_reduce(s: vec2<f32>) -> vec2<f32> {
    return s / 16.0;
}

// The whole `ADAPT` fragment body (`exposure.js:97-121`). Returns
// (exposure, ev), which is what the 1x1 target's .r/.g lanes carry.
//
//   params: x dt, y speedUp, z speedDown, w manual EV bias
//   limits: x minEV, y maxEV, z reset, w keyScale
fn axiom_meter_adapt(
    s: vec2<f32>,
    prev: f32,
    params: vec4<f32>,
    limits: vec4<f32>,
) -> vec2<f32> {
    let avg_log_lum = s.x / max(s.y, 1e-4);
    let lum = max(exp2(avg_log_lum), 1e-5);

    // The bias is ADDED: a higher EV100 means a smaller exposure, so + is
    // darker. `exposure.js:102-107` is emphatic about this; the file's own
    // history is that the subtraction contradicted the header for a long time.
    var ev100 = log2(lum * 100.0 / 12.5) + params.w;
    ev100 = axiom_meter_clamp(ev100, limits.x, limits.y);

    var prev_ev = prev;
    if ( limits.z > 0.5 ) { prev_ev = ev100; }

    var speed = params.y;
    if ( ev100 > prev_ev ) { speed = params.z; }
    let k = 1.0 - exp(-params.x * speed);
    let ev = axiom_meter_mix(prev_ev, ev100, axiom_meter_clamp(k, 0.0, 1.0));

    let max_lum = 1.2 * exp2(ev);   // 78/(q*S) with q=0.65, S=100 -> 1.2
    let exposure = limits.w / max_lum;
    return vec2<f32>(exposure, ev);
}
"#;

/// The three metering passes, as the pipeline shape a caller has to bind.
///
/// Prescriptive on purpose — one bind group serving all three, in the order
/// `AutoExposure.update` runs them (`exposure.js:169-199`) — so wiring this is a
/// pipeline and four render passes, not a re-derivation. Concatenate
/// [`EXPOSURE_WGSL`] in front of it.
///
/// The depth and previous-EV textures are bound for every pass even though only
/// two of the three read them, which is the same trade
/// `crate::post_chain` makes with its reinterpreted `Params`: one layout, no
/// per-pass bind group churn.
///
/// Not compiled by anything in this crate yet — see
/// `tests::nothing_in_the_present_path_compiles_this_yet`.
pub(crate) const EXPOSURE_PASS_WGSL: &str = r#"
struct MeterVsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct MeterParams {
    // xy = 1 / source size, in texels. zw unused.
    texel: vec4<f32>,
    // x skyWeight, y farDistance(m), z tapClamp, w hasDepth
    meter: vec4<f32>,
    // xy = the luminance range the sky de-weight ramps over. zw unused.
    sky_knee: vec4<f32>,
    // x dt, y speedUp, z speedDown, w manual EV bias
    adapt: vec4<f32>,
    // x minEV, y maxEV, z reset, w keyScale
    limits: vec4<f32>,
};

@group(0) @binding(0) var meter_src: texture_2d<f32>;
@group(0) @binding(1) var meter_sampler: sampler;
@group(0) @binding(2) var<uniform> meter_params: MeterParams;
@group(0) @binding(3) var meter_depth: texture_2d<f32>;
@group(0) @binding(4) var meter_prev: texture_2d<f32>;

@vertex
fn meter_vs(@builtin(vertex_index) vi: u32) -> MeterVsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var uv = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );
    var out: MeterVsOut;
    out.clip = vec4<f32>(pos[vi], 0.0, 1.0);
    out.uv = uv[vi];
    return out;
}

// `LOGLUM` (`exposure.js:22-70`): scene -> 64x64.
@fragment
fn meter_loglum_fs(in: MeterVsOut) -> @location(0) vec4<f32> {
    let texel = meter_params.texel.xy;
    let t0 = textureSample(meter_src, meter_sampler, in.uv + vec2<f32>(-1.0, -1.0) * texel).rgb;
    let t1 = textureSample(meter_src, meter_sampler, in.uv + vec2<f32>( 1.0, -1.0) * texel).rgb;
    let t2 = textureSample(meter_src, meter_sampler, in.uv + vec2<f32>(-1.0,  1.0) * texel).rgb;
    let t3 = textureSample(meter_src, meter_sampler, in.uv + vec2<f32>( 1.0,  1.0) * texel).rgb;
    let depth = textureSample(meter_depth, meter_sampler, in.uv).r;
    let out = axiom_meter_loglum(
        t0, t1, t2, t3, in.uv,
        meter_params.meter, meter_params.sky_knee.xy, depth);
    return vec4<f32>(out.x, out.y, 0.0, 1.0);
}

// `REDUCE` (`exposure.js:72-87`): 64 -> 16 -> 4 -> 1, the same pass three times.
// The accumulation order is the source's: y outer, x inner.
@fragment
fn meter_reduce_fs(in: MeterVsOut) -> @location(0) vec4<f32> {
    let texel = meter_params.texel.xy;
    var s = vec2<f32>(0.0, 0.0);
    for ( var y: i32 = 0; y < 4; y = y + 1 ) {
        for ( var x: i32 = 0; x < 4; x = x + 1 ) {
            let o = axiom_meter_reduce_offset(x, y, texel);
            s = s + textureSample(meter_src, meter_sampler, in.uv + o).rg;
        }
    }
    let out = axiom_meter_reduce(s);
    return vec4<f32>(out.x, out.y, 0.0, 1.0);
}

// `ADAPT` (`exposure.js:89-122`): the 1x1 pair plus last frame's EV, into
// (exposure, ev). Ping-ponged between two 1x1 float targets.
@fragment
fn meter_adapt_fs(in: MeterVsOut) -> @location(0) vec4<f32> {
    let s = textureSample(meter_src, meter_sampler, vec2<f32>(0.5, 0.5)).rg;
    let prev = textureSample(meter_prev, meter_sampler, vec2<f32>(0.5, 0.5)).g;
    let out = axiom_meter_adapt(s, prev, meter_params.adapt, meter_params.limits);
    return vec4<f32>(out.x, out.y, 0.0, 1.0);
}
"#;

/// GLSL `clamp( x, lo, hi )` — `min( max( x, lo ), hi )`.
fn glsl_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    f32::min(f32::max(x, lo), hi)
}

/// GLSL `mix( a, b, t )` — `a * (1 - t) + b * t`, which is the specification and
/// is *not* interchangeable with a fused form.
fn glsl_mix(a: f32, b: f32, t: f32) -> f32 {
    a * (1.0 - t) + b * t
}

/// GLSL `smoothstep( e0, e1, x )`.
fn glsl_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = glsl_clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `owMeterTap` (`exposure.js:35-38`) with the texture fetch hoisted out: the
/// per-tap luminance, floored at zero per channel and ceilinged at `tap_clamp`.
pub(crate) fn meter_tap(c: Vec3, tap_clamp: f32) -> f32 {
    let m = Vec3::new(
        f32::max(c.x, 0.0),
        f32::max(c.y, 0.0),
        f32::max(c.z, 0.0),
    );
    f32::min(lum(m), tap_clamp)
}

/// The centre weight (`exposure.js:48-49`).
pub(crate) fn centre_weight(uv: Vec2) -> f32 {
    let d = Vec2::new((uv.x - 0.5) * 2.0, (uv.y - 0.5) * 2.0);
    (-(d.x * d.x + d.y * d.y) * 1.1).exp()
}

/// The sky de-weight (`exposure.js:61-66`).
///
/// The source's two nested conditions become one multiplier, because the spine
/// is branchless — and because the arms are provably exact either way: when the
/// condition is false the factor is literally `1.0 * 1.0 + f * 0.0`, and `w *
/// 1.0` is `w`. `meter` is `(skyWeight, farDistance, tapClamp, hasDepth)`.
pub(crate) fn sky_deweight(w: f32, luminance: f32, meter: Vec4, sky_knee: Vec2, depth: f32) -> f32 {
    let applies = f32::from(u8::from(
        (meter.w > 0.5) & ((depth <= 0.0) | (depth > meter.y)),
    ));
    let factor = glsl_mix(
        1.0,
        meter.x,
        glsl_smoothstep(sky_knee.x, sky_knee.y, luminance),
    );
    w * glsl_mix(1.0, factor, applies)
}

/// The `LOGLUM` pass's whole fragment (`exposure.js:40-69`), with its four scene
/// taps supplied in the source's order: `(-1,-1)`, `(1,-1)`, `(-1,1)`, `(1,1)`.
///
/// Returns `(log2(lum) * w, w)` — the numerator and denominator of the weighted
/// log average, which is what the reduction chain sums.
pub(crate) fn loglum(
    taps: [Vec3; 4],
    uv: Vec2,
    meter: Vec4,
    sky_knee: Vec2,
    depth: f32,
) -> Vec2 {
    let luminance = taps
        .iter()
        .fold(0.0_f32, |sum, tap| sum + meter_tap(*tap, meter.z));
    let luminance = f32::max(luminance * 0.25, 1e-5);
    let w = sky_deweight(centre_weight(uv), luminance, meter, sky_knee, depth);
    Vec2::new(luminance.log2() * w, w)
}

/// `REDUCE`'s tap offset (`exposure.js:81`).
pub(crate) fn reduce_offset(x: i32, y: i32, texel: Vec2) -> Vec2 {
    Vec2::new(
        (x as f32 - 1.5) * texel.x,
        (y as f32 - 1.5) * texel.y,
    )
}

/// `REDUCE` (`exposure.js:77-86`): a 4x4 box of `(weighted log, weight)` pairs,
/// accumulated **y outer, x inner** and divided by sixteen.
///
/// The accumulation order is part of the algorithm — float addition is not
/// associative — so `samples` is indexed `y * 4 + x`, matching the source's loop
/// nesting and `EXPOSURE_PASS_WGSL`'s.
pub(crate) fn reduce(samples: &[Vec2; 16]) -> Vec2 {
    let sum = samples
        .iter()
        .fold(Vec2::new(0.0, 0.0), |s, v| Vec2::new(s.x + v.x, s.y + v.y));
    Vec2::new(sum.x / 16.0, sum.y / 16.0)
}

/// `ADAPT` (`exposure.js:97-121`) — **the semantic definition** of the exposure
/// this engine would drive a frame with.
///
/// * `s` — the 1x1 chain result, `(sum of weighted logs, sum of weights)`.
/// * `prev` — last frame's EV, the `.g` lane of the other ping-pong target.
/// * `params` — `(dt, speedUp, speedDown, evBias)`.
/// * `limits` — `(minEv, maxEv, reset, keyScale)`.
///
/// Returns `(exposure, ev)`: the scalar every downstream pass multiplies by, and
/// the EV to hand back as `prev` next frame.
pub(crate) fn adapt(s: Vec2, prev: f32, params: Vec4, limits: Vec4) -> Vec2 {
    let avg_log_lum = s.x / f32::max(s.y, 1e-4);
    let luminance = f32::max(avg_log_lum.exp2(), 1e-5);

    // ADDED, not subtracted: a higher EV100 is a smaller exposure, so + is
    // darker (`exposure.js:102-106`).
    let ev100 = (luminance * 100.0 / 12.5).log2() + params.w;
    let ev100 = glsl_clamp(ev100, limits.x, limits.y);

    // A reset replaces the history outright rather than blending toward it, so
    // the first frame after a scene change is already correct. Table index, not
    // a blend: `prev` may be whatever an uninitialised target held.
    let prev_ev = [prev, ev100][usize::from(limits.z > 0.5)];
    let speed = [params.y, params.z][usize::from(ev100 > prev_ev)];
    let k = 1.0 - ((-params.x) * speed).exp();
    let ev = glsl_mix(prev_ev, ev100, glsl_clamp(k, 0.0, 1.0));

    let max_lum = SPEED_CONSTANT * ev.exp2();
    Vec2::new(limits.w / max_lum, ev)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The renderer's own metering configuration, so a test reads the numbers a
    /// frame would (`exposure.js:130-141`, `index.js:213,342`).
    fn scene_meter(has_depth: bool) -> Vec4 {
        Vec4::new(
            SKY_WEIGHT,
            FAR_DISTANCE,
            TAP_CLAMP,
            f32::from(u8::from(has_depth)),
        )
    }

    fn scene_limits(reset: bool) -> Vec4 {
        Vec4::new(
            SCENE_MIN_EV,
            SCENE_MAX_EV,
            f32::from(u8::from(reset)),
            EXPOSURE_KEY,
        )
    }

    fn scene_params(dt: f32, bias: f32) -> Vec4 {
        Vec4::new(f32::min(dt, MAX_ADAPT_DT), SPEED_UP, SPEED_DOWN, bias)
    }

    /// The whole chain over an idealised frame: `level` radiance everywhere and
    /// the centre weight taken at the middle, so every cell of the 64x64 level
    /// carries the same pair and each reduction is the identity on it.
    ///
    /// Idealised in exactly one respect — a real frame's centre weight varies
    /// across the 64x64 grid — and that is the point: it isolates whether the
    /// chain *recovers the luminance it was given*, with the weighting held
    /// constant. The weighting itself is covered by its own tests above.
    fn meter_uniform_frame(level: f32) -> Vec2 {
        let cell = loglum(
            [Vec3::new(level, level, level); 4],
            Vec2::new(0.5, 0.5),
            scene_meter(false),
            SKY_KNEE,
            0.0,
        );
        // 64 -> 16 -> 4 -> 1: three reductions, one per step of CHAIN_SIZES.
        (0..CHAIN_SIZES.len() - 1).fold(cell, |c, _| reduce(&[c; 16]))
    }

    #[test]
    fn the_constants_are_the_sources() {
        assert_eq!(SKY_WEIGHT, 0.15, "exposure.js:130");
        assert_eq!(FAR_DISTANCE, 400.0, "exposure.js:130");
        assert_eq!(TAP_CLAMP, 40.0, "exposure.js:130");
        assert_eq!((SKY_KNEE.x, SKY_KNEE.y), (0.06, 0.3), "exposure.js:131");
        assert_eq!(SPEED_UP, 1.4, "exposure.js:140");
        assert_eq!(SPEED_DOWN, 3.2, "exposure.js:140");
        assert_eq!(MAX_ADAPT_DT, 0.1, "exposure.js:191");
        assert_eq!(EXPOSURE_KEY, 1.06, "index.js:342");
        assert_eq!((SCENE_MIN_EV, SCENE_MAX_EV), (-4.3, 20.0), "index.js:213");
        assert_eq!(CHAIN_SIZES, [64, 16, 4, 1], "exposure.js:145-148");
        // 78 / (0.65 * 100) — the saturation-based speed constant.
        assert!(
            (SPEED_CONSTANT - 78.0 / (0.65 * 100.0)).abs() < 1.0e-6,
            "1.2 is 78/(q*S) at q=0.65, S=100"
        );
        // Adaptation is ASYMMETRIC, and in this direction: darkening is faster.
        assert!(SPEED_DOWN > SPEED_UP);
    }

    /// The per-tap clamp is the reason a sun disc cannot drag the average, and
    /// it applies to the luminance, not per channel.
    #[test]
    fn a_tap_is_floored_at_zero_and_ceilinged_at_the_clamp() {
        assert_eq!(meter_tap(Vec3::new(4000.0, 4000.0, 4000.0), TAP_CLAMP), 40.0);
        // A negative channel — a filtered tap or a bloom subtract can produce
        // one — is floored per channel BEFORE the luminance, so it cannot cancel
        // a positive one.
        let floored = meter_tap(Vec3::new(-10.0, 1.0, 0.0), TAP_CLAMP);
        assert!(
            (floored - 0.7152).abs() < 1.0e-6,
            "the negative red must be floored, not subtracted; got {floored}"
        );
        // Under the clamp it is exactly the Rec.709 luminance.
        assert_eq!(
            meter_tap(Vec3::new(0.5, 0.25, 0.125), TAP_CLAMP),
            lum(Vec3::new(0.5, 0.25, 0.125))
        );
    }

    /// Centre weighting: unity dead centre, and down by `e^-2.2` at a corner
    /// where `dot(d, d)` is 2.
    #[test]
    fn the_centre_weight_peaks_at_the_middle_and_falls_to_the_corners() {
        assert_eq!(centre_weight(Vec2::new(0.5, 0.5)), 1.0);
        let corner = centre_weight(Vec2::new(0.0, 0.0));
        assert!(
            (corner - (-2.2_f32).exp()).abs() < 1.0e-7,
            "a corner is exp(-2.2); got {corner}"
        );
        assert!(centre_weight(Vec2::new(0.5, 0.0)) > corner);
        // Symmetric about both axes — to within one ULP, which is all the
        // symmetry an `f32` has: `0.9 - 0.5` is `0.40000004` while `0.1 - 0.5` is
        // exactly `-0.4`, so the two weights differ in the last bit. Asserting
        // exact equality here was this test's first draft and it failed.
        let left = centre_weight(Vec2::new(0.1, 0.5));
        let right = centre_weight(Vec2::new(0.9, 0.5));
        assert!((left - right).abs() < 1.0e-7, "{left} vs {right}");
    }

    /// The sky de-weight, all four of its regimes.
    #[test]
    fn the_sky_deweight_needs_depth_a_sky_pixel_and_luminance() {
        let w = 0.8;
        let bright = 1.0;
        // No depth channel at all: the de-weight is an exact identity.
        assert_eq!(
            sky_deweight(w, bright, scene_meter(false), SKY_KNEE, 0.0),
            w,
            "with hasDepth 0 the weight must be untouched, bit for bit"
        );
        // Depth present, but the fragment is near geometry: also identity.
        assert_eq!(
            sky_deweight(w, bright, scene_meter(true), SKY_KNEE, 12.0),
            w
        );
        // Depth 0 means nothing was written — sky — and a bright sky is cut to
        // SKY_WEIGHT because the smoothstep has saturated.
        assert!(
            (sky_deweight(w, bright, scene_meter(true), SKY_KNEE, 0.0) - w * SKY_WEIGHT).abs()
                < 1.0e-7
        );
        // Past the far distance is also sky.
        assert!(
            (sky_deweight(w, bright, scene_meter(true), SKY_KNEE, 900.0) - w * SKY_WEIGHT).abs()
                < 1.0e-7
        );
        // A DARK sky is left alone, which is the whole reason the ramp exists:
        // a moonlit sky is the only absolute anchor the meter has at night.
        assert_eq!(
            sky_deweight(w, 0.01, scene_meter(true), SKY_KNEE, 0.0),
            w,
            "below the knee the night sky keeps its full weight"
        );
        // ...and the ramp between is monotone.
        let ramp: Vec<f32> = (0..=20)
            .map(|i| sky_deweight(w, 0.05 + i as f32 * 0.015, scene_meter(true), SKY_KNEE, 0.0))
            .collect();
        ramp.windows(2)
            .for_each(|p| assert!(p[1] <= p[0], "the de-weight must not rise: {p:?}"));
    }

    /// The `LOGLUM` cell: the four taps are averaged, the average is floored, and
    /// the pair is `(log2 * w, w)`.
    #[test]
    fn a_loglum_cell_is_the_weighted_log_of_the_four_tap_mean() {
        let taps = [
            Vec3::new(0.2, 0.2, 0.2),
            Vec3::new(0.4, 0.4, 0.4),
            Vec3::new(0.1, 0.1, 0.1),
            Vec3::new(0.5, 0.5, 0.5),
        ];
        let cell = loglum(taps, Vec2::new(0.5, 0.5), scene_meter(false), SKY_KNEE, 0.0);
        let mean: f32 = (0.2 + 0.4 + 0.1 + 0.5) * 0.25;
        assert_eq!(cell.y, 1.0, "dead centre, no sky: the weight is one");
        assert!((cell.x - mean.log2()).abs() < 1.0e-6, "got {}", cell.x);
    }

    /// A black frame cannot produce `-inf`: the mean is floored at `1e-5`.
    #[test]
    fn a_black_frame_is_floored_rather_than_producing_a_negative_infinity() {
        let cell = loglum(
            [Vec3::new(0.0, 0.0, 0.0); 4],
            Vec2::new(0.5, 0.5),
            scene_meter(false),
            SKY_KNEE,
            0.0,
        );
        assert!(cell.x.is_finite());
        assert!((cell.x - 1e-5_f32.log2()).abs() < 1.0e-4, "got {}", cell.x);
    }

    /// The reduction is a mean, and its accumulation order is the source's.
    #[test]
    fn the_reduction_averages_sixteen_pairs_in_the_sources_order() {
        let mut block = [Vec2::new(0.0, 0.0); 16];
        (0..16).for_each(|i| block[i] = Vec2::new(i as f32, 1.0));
        let out = reduce(&block);
        assert_eq!(out.x, 120.0 / 16.0);
        assert_eq!(out.y, 1.0);
        // Order matters in float, so the `y * 4 + x` index mapping — and the
        // pass's loop nesting that matches it — is part of the algorithm, not
        // formatting. Fifteen values far below the sixteenth demonstrate it: read
        // large-first each addend vanishes into the rounding, read small-first
        // they accumulate into something the large value can see.
        let mut small_last = [Vec2::new(1.0e-8, 0.0); 16];
        small_last[15] = Vec2::new(1.0, 0.0);
        let mut small_first = small_last;
        small_first.reverse();
        assert_ne!(
            reduce(&small_last).x,
            reduce(&small_first).x,
            "float addition is not associative, and this test must be able to see it"
        );
    }

    /// The 4x4 box offsets are centred: `(x, y) - 1.5` runs -1.5..1.5.
    #[test]
    fn the_reduce_offsets_are_a_centred_four_by_four_box() {
        let texel = Vec2::new(1.0 / 64.0, 1.0 / 64.0);
        assert_eq!(reduce_offset(0, 0, texel).x, -1.5 / 64.0);
        assert_eq!(reduce_offset(3, 3, texel).y, 1.5 / 64.0);
        // Symmetric, so the box has no drift.
        assert_eq!(reduce_offset(0, 1, texel).x, -reduce_offset(3, 2, texel).x);
    }

    /// The reset arm: with `reset` set, the first frame lands on the metered EV
    /// rather than easing toward whatever the target happened to hold.
    #[test]
    fn a_reset_adopts_the_metered_ev_immediately() {
        // A weighted log-average of exactly 0 => lum 1.0 => ev100 = log2(8) = 3.
        let s = Vec2::new(0.0, 1.0);
        let out = adapt(s, -999.0, scene_params(0.016, 0.0), scene_limits(true));
        assert!(
            (out.y - 3.0).abs() < 1.0e-5,
            "a reset must ignore the stale history entirely; got {}",
            out.y
        );
        assert!(
            (out.x - EXPOSURE_KEY / (SPEED_CONSTANT * 8.0)).abs() < 1.0e-6,
            "exposure = key / (1.2 * 2^ev); got {}",
            out.x
        );
    }

    /// Without a reset the EV eases from the history, and asymmetrically: the
    /// same one-stop step covers more ground going darker than going brighter.
    #[test]
    fn adaptation_is_asymmetric_and_eases_rather_than_snapping() {
        let s = Vec2::new(0.0, 1.0); // ev100 = 3
        let dt = 0.1;
        let darkening = adapt(s, 2.0, scene_params(dt, 0.0), scene_limits(false)).y;
        let brightening = adapt(s, 4.0, scene_params(dt, 0.0), scene_limits(false)).y;
        assert!(
            (2.0..3.0).contains(&darkening),
            "the EV must ease up from 2 toward 3; got {darkening}"
        );
        assert!(
            (3.0..4.0).contains(&brightening),
            "the EV must ease down from 4 toward 3; got {brightening}"
        );
        // Fractions of the one-stop gap covered in this step, so the two are
        // comparable — and each must be exactly `1 - exp(-dt * speed)`.
        let darkening_fraction = darkening - 2.0;
        let brightening_fraction = 4.0 - brightening;
        assert!(
            darkening_fraction > brightening_fraction,
            "the eye darkens down quickly ({darkening_fraction}) and brightens up \
             slowly ({brightening_fraction})"
        );
        assert!(
            (darkening_fraction - (1.0 - (-dt * SPEED_DOWN).exp())).abs() < 1.0e-6,
            "a rising EV must use SPEED_DOWN; got {darkening_fraction}"
        );
        assert!(
            (brightening_fraction - (1.0 - (-dt * SPEED_UP).exp())).abs() < 1.0e-6,
            "a falling EV must use SPEED_UP; got {brightening_fraction}"
        );
        // And a bigger dt covers more ground, monotonically.
        let slow = adapt(s, 2.0, scene_params(0.008, 0.0), scene_limits(false)).y;
        assert!(slow < darkening);
    }

    /// The bias is ADDED to the EV, so positive is darker — the sign the
    /// source's header, `setExposureBias` and the sky's `evBias` all agree on
    /// and the shader's arithmetic once contradicted.
    #[test]
    fn a_positive_bias_darkens_the_image() {
        let s = Vec2::new(0.0, 1.0);
        let neutral = adapt(s, 0.0, scene_params(1.0e3, 0.0), scene_limits(true));
        let biased = adapt(s, 0.0, scene_params(1.0e3, 1.0), scene_limits(true));
        assert!(
            (biased.y - neutral.y - 1.0).abs() < 1.0e-5,
            "+1 EV of bias is one stop; got {} vs {}",
            biased.y,
            neutral.y
        );
        assert!(
            biased.x < neutral.x,
            "a higher EV is a SMALLER exposure: {} vs {}",
            biased.x,
            neutral.x
        );
        assert!(
            (biased.x * 2.0 - neutral.x).abs() < 1.0e-6,
            "one stop is exactly half the exposure"
        );
    }

    /// The limits bind, and the floor is the night lock the renderer documents.
    #[test]
    fn the_ev_is_clamped_into_the_renderers_window() {
        // A moonlit street: the source says it meters at -5.2, below the floor.
        let dark = Vec2::new((-5.2_f32 - f32::log2(100.0 / 12.5)).exp2().log2(), 1.0);
        let out = adapt(dark, 0.0, scene_params(1.0e3, 0.0), scene_limits(true));
        assert_eq!(
            out.y, SCENE_MIN_EV,
            "the night lock must bind before the meter chases a moonlit street"
        );
        // ...and the ceiling.
        let blazing = Vec2::new(40.0, 1.0);
        let hot = adapt(blazing, 0.0, scene_params(1.0e3, 0.0), scene_limits(true));
        assert_eq!(hot.y, SCENE_MAX_EV);
    }

    /// A zero total weight — every cell de-weighted to nothing — divides by the
    /// `1e-4` floor instead of by zero, and the result is still finite.
    #[test]
    fn a_zero_weight_meter_does_not_divide_by_zero() {
        let out = adapt(
            Vec2::new(0.0, 0.0),
            0.0,
            scene_params(0.016, 0.0),
            scene_limits(true),
        );
        assert!(out.x.is_finite() & out.y.is_finite());
        assert!(out.x > 0.0);
    }

    /// **The photometric contract.** The metered EV is scene-relative; converted
    /// to a true EV100 it must land on the photographic reading for the same
    /// scene, which is what pins this module to `apps/shmup/src/sky/atmosphere.rs`.
    #[test]
    fn the_metered_ev_agrees_with_the_skys_photometric_contract() {
        // atmosphere.js's own worked example: sunlit stucco, albedo 0.4 at 45
        // degrees, is ~0.32 framebuffer radiance units.
        let stucco = 0.32_f32;
        let metered = (stucco * 100.0 / 12.5).log2();
        let true_ev = photometric_ev100(metered);
        assert!(
            (true_ev - 16.0).abs() < 0.5,
            "a sunlit surface must read near EV 16 (the 'sunny 16' rule); got {true_ev}"
        );
        // The offset is exactly log2(SCENE_LUX), and SCENE_LUX is the sky's.
        assert_eq!(SCENE_LUX, 25000.0, "atmosphere.js:53");
        assert!((photometric_ev100(0.0) - 14.6096).abs() < 1.0e-3);
        // Had the 1.65-stop `pi` bug the sky module records still been present,
        // the same surface would meter here 1.65 stops hotter and this bound
        // would fail — which is the point of asserting it from this side too.
        let with_pi_bug = photometric_ev100((stucco * std::f32::consts::PI * 100.0 / 12.5).log2());
        let pi_bug_stops = with_pi_bug - true_ev;
        assert!(
            (pi_bug_stops - 1.651).abs() < 0.01,
            "the recorded bug is 1.65 stops; got {pi_bug_stops}"
        );
        assert!((with_pi_bug - 16.0).abs() > 0.5, "...and it would miss the mark");
    }

    /// End to end on a uniform frame: the chain reproduces the input's own log
    /// luminance, and the renderer's daylight reading lands in the window its
    /// comment claims (`index.js:209-213`: daylight meters -1 to -2.1).
    #[test]
    fn the_whole_chain_recovers_a_uniform_frames_luminance() {
        let level = 0.25_f32;
        let one = meter_uniform_frame(level);
        let out = adapt(one, 0.0, scene_params(1.0e3, 0.0), scene_limits(true));
        let expected = (level * 100.0 / 12.5).log2();
        assert!(
            (out.y - expected).abs() < 1.0e-4,
            "the chain must recover the frame's own EV; got {} want {expected}",
            out.y
        );
        // `index.js:211` says daylight frames meter between -1 and -2.1. Inverted
        // through `ev = log2(lum * 8)`, that is a weighted mean of 0.029..0.063
        // framebuffer radiance units — a useful number to have written down,
        // because it is what a daylight frame's *average* has to be for the
        // ported exposure to sit where the reference's does.
        let daylight = adapt(
            meter_uniform_frame(0.05),
            0.0,
            scene_params(1.0e3, 0.0),
            scene_limits(true),
        )
        .y;
        assert!(
            (-2.1..=-1.0).contains(&daylight),
            "a 0.05-unit mean must meter inside the documented daylight window; \
             got {daylight}"
        );
        // Both ends of that window, so the inversion above is pinned and not a
        // comment that can rot.
        [(0.0625_f32, -1.0_f32), (0.029_f32, -2.1_f32)]
            .iter()
            .for_each(|(level, ev)| {
                let got = adapt(
                    meter_uniform_frame(*level),
                    0.0,
                    scene_params(1.0e3, 0.0),
                    scene_limits(true),
                )
                .y;
                assert!(
                    (got - ev).abs() < 0.02,
                    "{level} radiance units must meter at EV {ev}; got {got}"
                );
            });
    }

    /// Every WGSL function the CPU reference above mirrors is declared.
    #[test]
    fn the_wgsl_declares_every_function_this_module_mirrors() {
        [
            "fn axiom_meter_clamp(",
            "fn axiom_meter_mix(",
            "fn axiom_meter_smoothstep(",
            "fn axiom_meter_lum(",
            "fn axiom_meter_tap(",
            "fn axiom_meter_centre_weight(",
            "fn axiom_meter_sky_deweight(",
            "fn axiom_meter_loglum(",
            "fn axiom_meter_reduce_offset(",
            "fn axiom_meter_reduce(",
            "fn axiom_meter_adapt(",
        ]
        .iter()
        .for_each(|needle| {
            assert!(
                EXPOSURE_WGSL.contains(needle),
                "EXPOSURE_WGSL is missing {needle}"
            );
        });
        ["fn meter_vs(", "fn meter_loglum_fs(", "fn meter_reduce_fs(", "fn meter_adapt_fs("]
            .iter()
            .for_each(|needle| {
                assert!(
                    EXPOSURE_PASS_WGSL.contains(needle),
                    "EXPOSURE_PASS_WGSL is missing {needle}"
                );
            });
    }

    /// The pass text loops **y outer, x inner**, which is the accumulation order
    /// [`reduce`] is written to. Float addition is not associative, so this is a
    /// correctness property, not formatting.
    #[test]
    fn the_reduce_pass_nests_its_loops_in_the_sources_order() {
        let y_at = EXPOSURE_PASS_WGSL
            .find("var y: i32 = 0")
            .expect("the reduce pass loops over y");
        let x_at = EXPOSURE_PASS_WGSL
            .find("var x: i32 = 0")
            .expect("the reduce pass loops over x");
        assert!(y_at < x_at, "y must be the OUTER loop, as in exposure.js:79-80");
    }

    /// The WGSL must not reach for a builtin whose factoring the specification
    /// leaves open. `min`/`max` are exact; `exp`/`exp2`/`log2` are approximated
    /// on both sides anyway.
    #[test]
    fn the_wgsl_calls_no_unspecified_builtin() {
        // The written-out names are removed first, so `step(` inside
        // `smoothstep(` cannot produce a false positive.
        let stripped = EXPOSURE_WGSL
            .replace("axiom_meter_smoothstep(", "")
            .replace("axiom_meter_clamp(", "")
            .replace("axiom_meter_mix(", "");
        ["clamp(", "mix(", "dot(", "smoothstep(", "step("]
            .iter()
            .for_each(|needle| {
                assert!(
                    !stripped.contains(needle),
                    "EXPOSURE_WGSL calls the {needle} builtin"
                );
            });
        assert!(EXPOSURE_WGSL.contains("axiom_meter_clamp("));
        assert!(EXPOSURE_WGSL.contains("axiom_meter_smoothstep("));
    }

    /// `owLum` has one body. The AgX chunk and the metering chunk each carry a
    /// copy so each concatenates standalone; they must be the same expression.
    #[test]
    fn the_two_wgsl_luminance_bodies_are_the_same_expression() {
        let body = "return c.x * 0.2126 + c.y * 0.7152 + c.z * 0.0722;";
        assert!(EXPOSURE_WGSL.contains(body));
        assert!(crate::agx::AGX_WGSL.contains(body));
    }

    /// **The opt-in proof.** Nothing in the crate's present path compiles either
    /// metering string, so no app in the repo silently changes exposure by this
    /// module existing. See `agx.rs` for the same scan and why it is a scan.
    #[test]
    fn nothing_in_the_present_path_compiles_this_yet() {
        [
            ("post_chain.rs", include_str!("post_chain.rs")),
            ("upscale.rs", include_str!("upscale.rs")),
            ("scene_renderer.rs", include_str!("scene_renderer.rs")),
        ]
        .iter()
        .for_each(|(name, source)| {
            assert!(
                !source.contains("EXPOSURE_WGSL")
                    & !source.contains("EXPOSURE_PASS_WGSL")
                    & !source.contains("exposure::"),
                "{name} now references the meter; exposure is no longer opt-in, and \
                 this test must be replaced by one proving the OFF path is unchanged"
            );
        });
    }
}

// The CPU reference above is the semantic definition; this holds it up against a
// real GPU running `EXPOSURE_WGSL`, and compile-checks `EXPOSURE_PASS_WGSL`.
// Compiled only with `--features offscreen`, and it ASSERTS an adapter was
// acquired rather than skipping. Harness shape per `material_shader::cloth`.
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod parity {
    use super::*;

    /// How many contexts one run compares, and the target's width.
    const SAMPLES: usize = 24;

    /// Sixteen-byte lanes per context in the uniform block: four taps, one
    /// `(uv, depth)`, the meter vector, one `(knee, texel)`, the sixteen
    /// reduction inputs, then params / limits / `(s, prev)`.
    ///
    /// Must match `HARNESS_WGSL`'s stride exactly. It did not at first — the
    /// count was 24 against 26 packed — and `uniform_bytes`' trailing `resize`
    /// silently *truncated* the buffer rather than failing, so the GPU read
    /// every context but the first from the wrong offset. The disagreement it
    /// produced was 36%, not a ULP, which is the useful property: a packing
    /// error and a precision error do not look alike.
    const LANES: usize = 26;

    /// `copy_texture_to_buffer` requires each row aligned to this many bytes.
    const ROW_ALIGN: u32 = 256;

    /// The agreement budget, **relative above unit magnitude**.
    ///
    /// Relative rather than purely absolute because two of the lanes leave the
    /// unit band by a lot: the metered EV spans `[-4.3, 20]` and the `log2`
    /// output reaches -17. The `max(_, 1)` floor keeps it absolute for the
    /// weights and the exposure scalar, which live in `0..=1.1`.
    ///
    /// Where the deviation comes from, per stage, as this sweep measured it on a
    /// Vulkan adapter:
    ///
    /// | entry point | worst scaled |
    /// |---|---|
    /// | `meter_reduce_parity_fs` | **`0`** — bit for bit |
    /// | `meter_loglum_parity_fs` | `2.18e-7` |
    /// | `meter_adapt_parity_fs` | `2.73e-7` |
    ///
    /// The reduction being *exactly* equal is the useful one: it is sixteen
    /// additions in a fixed order and a divide by a power of two, so there is
    /// nothing for a contraction to reorder and nothing for a reciprocal to
    /// approximate. It also means the harness really is feeding both sides the
    /// same bytes, which after the `LANES` bug above is worth having proven.
    ///
    /// The other two are two ULP each and are transcendental: `exp` in the centre
    /// weight and `log2` in the cell; `exp2` twice and `exp` once in the adapt.
    /// `exp2(ev)` is the amplifier — a relative error `e` in `ev` becomes
    /// `e · ev · ln2` relative in the exposure, and `ev` reaches 20 — which is why
    /// the worst lane is the exposure scalar rather than the EV that produced it.
    ///
    /// **Measured, not fitted**: [`MEASURED_WORST`] is what this machine reports
    /// and it is asserted, so the justification cannot rot. The budget is 3.7x
    /// it, which leaves room for one more contracted multiply-add and no more.
    const TOLERANCE: f32 = 1.0e-6;

    /// The worst scaled deviation this module has actually been measured at
    /// (Vulkan, `meter_adapt_parity_fs` sample 6: GPU `1.308409` vs CPU
    /// `1.3084093`). Only ever raised from a real run on a worse adapter.
    const MEASURED_WORST: f32 = 2.8e-7;

    /// One context: everything the three entry points read.
    struct Context {
        taps: [Vec3; 4],
        uv: Vec2,
        meter: Vec4,
        sky_knee: Vec2,
        depth: f32,
        block: [Vec2; 16],
        texel: Vec2,
        s: Vec2,
        prev: f32,
        params: Vec4,
        limits: Vec4,
    }

    /// The contexts, chosen to cross every regime: taps under and over the
    /// per-tap clamp and one negative channel; UVs from dead centre to a corner;
    /// depth absent, near, and past the far distance; sky luminance below, on
    /// and above the knee; both adaptation directions with and without a reset;
    /// an EV that saturates each limit; and a zero total weight.
    fn contexts() -> Vec<Context> {
        (0..SAMPLES)
            .map(|index| {
                let t = index as f32;
                let s = t * 0.1307;
                // 2^-9 .. 2^14, so the clamp binds on the top third.
                let level = (t - 9.0).exp2();
                Context {
                    taps: [
                        Vec3::new(level * 0.9, level * 1.1, level * 0.4),
                        Vec3::new(level * 0.2, level * 0.7, level * 1.3),
                        Vec3::new(level * -0.3, level * 0.5, level * 0.8),
                        Vec3::new(level * 1.6, level * 0.05, level * 0.6),
                    ],
                    uv: Vec2::new(s * 0.5, 1.0 - s * 0.42),
                    meter: Vec4::new(
                        SKY_WEIGHT,
                        FAR_DISTANCE,
                        TAP_CLAMP,
                        // Two thirds of the contexts have a depth channel.
                        f32::from(u8::from(index % 3 != 0)),
                    ),
                    sky_knee: SKY_KNEE,
                    // 0 (sky), near geometry, and well past the far distance.
                    depth: [0.0, 37.5, 900.0, 399.9][index % 4],
                    // Values spanning several magnitudes, so the accumulation
                    // order is visible in the last bits.
                    block: {
                        let mut b = [Vec2::new(0.0, 0.0); 16];
                        (0..16).for_each(|i| {
                            b[i] = Vec2::new(
                                (i as f32 - 8.0 + s) * (1.0 + t * 0.31),
                                0.02 + i as f32 * 0.061,
                            );
                        });
                        b
                    },
                    texel: Vec2::new(1.0 / 64.0, 1.0 / 16.0),
                    // A weight sum of exactly zero on every fourth context, the
                    // `max(s.y, 1e-4)` floor's only chance to bind — paired with
                    // a zero numerator, because `8.0 / 1e-4` fed to `exp2`
                    // overflows to infinity and the two sides' infinity handling
                    // is not something this test is trying to compare.
                    s: [
                        Vec2::new(0.0, 0.0),
                        Vec2::new(t * 0.7 - 8.0, 0.4),
                        Vec2::new(t * 2.1 - 14.0, 3.1),
                        Vec2::new(t * 6.0 - 30.0, 12.0),
                    ][index % 4],
                    prev: t * 0.9 - 6.0,
                    params: Vec4::new(
                        [0.016, 0.1, 0.0041, 1.0e3][index % 4],
                        SPEED_UP,
                        SPEED_DOWN,
                        [0.0, 1.35, -0.6, 0.55][index % 4],
                    ),
                    limits: Vec4::new(
                        SCENE_MIN_EV,
                        SCENE_MAX_EV,
                        f32::from(u8::from(index % 5 == 0)),
                        EXPOSURE_KEY,
                    ),
                }
            })
            .collect()
    }

    /// The harness: a fullscreen triangle whose fragment stage evaluates the
    /// entry point at the context its pixel column names.
    const HARNESS_WGSL: &str = r#"
struct MeterContexts { items: array<vec4<f32>, 624> };
@group(0) @binding(0) var<uniform> ctx: MeterContexts;

fn lane(index: u32, slot: u32) -> vec4<f32> { return ctx.items[index * 26u + slot]; }

@vertex
fn meter_parity_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

// Slots 0..3 taps, 4 = (uv, depth, -), 5 = meter, 6 = (knee, texel).
@fragment
fn meter_loglum_parity_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let a = lane(i, 4u);
    let out = axiom_meter_loglum(
        lane(i, 0u).xyz, lane(i, 1u).xyz, lane(i, 2u).xyz, lane(i, 3u).xyz,
        a.xy, lane(i, 5u), lane(i, 6u).xy, a.z,
    );
    return vec4<f32>(
        out.x,
        out.y,
        axiom_meter_tap(lane(i, 0u).xyz, lane(i, 5u).z),
        axiom_meter_centre_weight(a.xy),
    );
}

// Slots 7..22 = the 16 reduction inputs, in `y * 4 + x` order. The loop nesting
// here is the pass's, so what is compared is the ORDER as well as the values.
@fragment
fn meter_reduce_parity_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    var s = vec2<f32>(0.0, 0.0);
    for ( var y: i32 = 0; y < 4; y = y + 1 ) {
        for ( var x: i32 = 0; x < 4; x = x + 1 ) {
            s = s + lane(i, u32(7 + y * 4 + x)).xy;
        }
    }
    let out = axiom_meter_reduce(s);
    let off = axiom_meter_reduce_offset(1, 3, lane(i, 6u).zw);
    return vec4<f32>(out.x, out.y, off.x, off.y);
}

// Slots 23 = params, 24 = limits, 25 = (s.x, s.y, prev, -).
@fragment
fn meter_adapt_parity_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let a = lane(i, 25u);
    let out = axiom_meter_adapt(a.xy, a.z, lane(i, 23u), lane(i, 24u));
    return vec4<f32>(out.x, out.y, 0.0, 0.0);
}
"#;

    struct Gpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
        backend: wgpu::Backend,
    }

    impl Gpu {
        fn acquire() -> Gpu {
            // The crate's ONE instance + adapter + device (see `crate::test_gpu`):
            // ~50 tests each opening their own is what crashes the driver.
            let gpu = crate::test_gpu::TestGpu::shared();
            let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
            Gpu {
                device,
                queue,
                backend: gpu.backend,
            }
        }

        fn render(&self, module: &wgpu::ShaderModule, entry: &str, uniform: &[u8]) -> Vec<[f32; 4]> {
            let layout = self
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("axiom-exposure-parity-bgl"),
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    }],
                });
            let buffer = wgpu::util::DeviceExt::create_buffer_init(
                &self.device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("axiom-exposure-parity-uniform"),
                    contents: uniform,
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-exposure-parity-bg"),
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("axiom-exposure-parity-pl"),
                        bind_group_layouts: &[&layout],
                        push_constant_ranges: &[],
                    });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("axiom-exposure-parity-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("meter_parity_vs"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some(entry),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba32Float,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    multiview: None,
                    cache: None,
                });
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-exposure-parity-target"),
                size: wgpu::Extent3d {
                    width: SAMPLES as u32,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let row_bytes = (SAMPLES as u32 * 16).div_ceil(ROW_ALIGN) * ROW_ALIGN;
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("axiom-exposure-parity-readback"),
                size: u64::from(row_bytes),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("axiom-exposure-parity-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(row_bytes),
                        rows_per_image: Some(1),
                    },
                },
                wgpu::Extent3d {
                    width: SAMPLES as u32,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit(Some(encoder.finish()));
            let slice = readback.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            self.device
                .poll(wgpu::PollType::Wait)
                .expect("the readback must complete");
            let mapped = slice.get_mapped_range();
            (0..SAMPLES)
                .map(|sample| {
                    [0_usize, 1, 2, 3].map(|lane| {
                        let at = sample * 16 + lane * 4;
                        f32::from_le_bytes([
                            mapped[at],
                            mapped[at + 1],
                            mapped[at + 2],
                            mapped[at + 3],
                        ])
                    })
                })
                .collect()
        }
    }

    /// The uniform block: 24 `vec4` per context, in the order `lane()` unpacks.
    fn uniform_bytes(contexts: &[Context]) -> Vec<u8> {
        let bytes: Vec<u8> = contexts
            .iter()
            .flat_map(|c| {
                let head = [
                    [c.taps[0].x, c.taps[0].y, c.taps[0].z, 0.0],
                    [c.taps[1].x, c.taps[1].y, c.taps[1].z, 0.0],
                    [c.taps[2].x, c.taps[2].y, c.taps[2].z, 0.0],
                    [c.taps[3].x, c.taps[3].y, c.taps[3].z, 0.0],
                    [c.uv.x, c.uv.y, c.depth, 0.0],
                    [c.meter.x, c.meter.y, c.meter.z, c.meter.w],
                    [c.sky_knee.x, c.sky_knee.y, c.texel.x, c.texel.y],
                ];
                let block: Vec<[f32; 4]> =
                    c.block.iter().map(|v| [v.x, v.y, 0.0, 0.0]).collect();
                let tail = [
                    [c.params.x, c.params.y, c.params.z, c.params.w],
                    [c.limits.x, c.limits.y, c.limits.z, c.limits.w],
                    [c.s.x, c.s.y, c.prev, 0.0],
                ];
                head.into_iter()
                    .chain(block)
                    .chain(tail)
                    .collect::<Vec<[f32; 4]>>()
            })
            .flatten()
            .flat_map(f32::to_le_bytes)
            .collect();
        // An equality, never a `resize`. A `resize` to a SMALLER length is a
        // silent truncation, and that is exactly how the 26-vs-24 lane-count
        // error above got as far as a wrong number instead of a failure.
        assert_eq!(
            bytes.len(),
            SAMPLES * LANES * 16,
            "LANES must match what this function packs and what HARNESS_WGSL strides by"
        );
        bytes
    }

    /// Compare one entry point's four lanes against the CPU reference, and
    /// return the worst scaled deviation over the whole sweep together with the
    /// lane it came from.
    ///
    /// One assertion at the end rather than one per lane, so a run reports the
    /// *worst* disagreement rather than the first — which is what a budget has
    /// to be set from.
    fn compare(
        gpu: &Gpu,
        module: &wgpu::ShaderModule,
        entry: &str,
        expected: &[[f32; 4]],
    ) -> (f32, String) {
        let actual = gpu.render(module, entry, &uniform_bytes(&contexts()));
        actual
            .iter()
            .zip(expected)
            .enumerate()
            .flat_map(|(sample, (got, want))| {
                got.iter()
                    .zip(want)
                    .enumerate()
                    .map(move |(lane, (g, w))| (sample, lane, *g, *w))
            })
            .map(|(sample, lane, got, want)| {
                let scaled = (got - want).abs() / f32::max(want.abs(), 1.0);
                (
                    scaled,
                    format!("{entry} sample {sample} lane {lane}: GPU {got} vs CPU {want}"),
                )
            })
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .expect("the sweep compares at least one lane")
    }

    #[test]
    fn exposure_wgsl_agrees_with_the_cpu_reference_on_a_real_adapter() {
        let gpu = Gpu::acquire();
        // The error scope is the SHARED device's, so it is entered exclusively;
        // see `crate::test_gpu::validating`.
        let (module, failure) = crate::test_gpu::validating(&gpu.device, || {
            gpu
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("axiom-exposure-parity-shader"),
                    source: wgpu::ShaderSource::Wgsl(
                        format!("{EXPOSURE_WGSL}\n{HARNESS_WGSL}").into(),
                    ),
                })
        });
        assert!(
            failure.is_none(),
            "EXPOSURE_WGSL must compile"
        );

        let ctx = contexts();
        let loglum_expected: Vec<[f32; 4]> = ctx
            .iter()
            .map(|c| {
                let out = loglum(c.taps, c.uv, c.meter, c.sky_knee, c.depth);
                [
                    out.x,
                    out.y,
                    meter_tap(c.taps[0], c.meter.z),
                    centre_weight(c.uv),
                ]
            })
            .collect();
        let reduce_expected: Vec<[f32; 4]> = ctx
            .iter()
            .map(|c| {
                let out = reduce(&c.block);
                let off = reduce_offset(1, 3, c.texel);
                [out.x, out.y, off.x, off.y]
            })
            .collect();
        let adapt_expected: Vec<[f32; 4]> = ctx
            .iter()
            .map(|c| {
                let out = adapt(c.s, c.prev, c.params, c.limits);
                [out.x, out.y, 0.0, 0.0]
            })
            .collect();

        let per_entry = [
            ("meter_loglum_parity_fs", loglum_expected),
            ("meter_reduce_parity_fs", reduce_expected),
            ("meter_adapt_parity_fs", adapt_expected),
        ]
        .iter()
        .map(|(entry, expected)| compare(&gpu, &module, entry, expected))
        .collect::<Vec<(f32, String)>>();
        // Every entry point's worst, not just the overall one: the budget below
        // has to be ATTRIBUTABLE, and "which stage costs what" is only visible
        // if the failure message carries all of them.
        let summary = per_entry
            .iter()
            .map(|(w, at)| format!("{w:e} at {at}"))
            .collect::<Vec<String>>()
            .join(" | ");
        let (worst, at) = per_entry
            .iter()
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .cloned()
            .expect("at least one entry point is compared");

        assert!(
            worst <= TOLERANCE,
            "exposure parity on {:?}: worst scaled delta {worst:e} exceeds \
             the budget {TOLERANCE:e}, at {at}",
            gpu.backend
        );
        assert!(
            worst <= MEASURED_WORST,
            "exposure parity on {:?}: this adapter deviates by {worst:e} (at {at}), more \
             than the recorded {MEASURED_WORST:e}; redo the error account, do not raise it.              Per entry point: {summary}",
            gpu.backend
        );
    }

    /// The pass shaders are wiring, not arithmetic, so they cannot be held to a
    /// CPU reference — but they can be held to compiling against the real
    /// validator with the real function library in front of them, which is what
    /// catches a binding, a swizzle or an entry-point signature that is wrong.
    #[test]
    fn the_three_metering_passes_compile_on_a_real_adapter() {
        let gpu = Gpu::acquire();
        // The error scope is the SHARED device's, so it is entered exclusively;
        // see `crate::test_gpu::validating`.
        let (module, failure) = crate::test_gpu::validating(&gpu.device, || {
            gpu
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("axiom-exposure-pass-shader"),
                    source: wgpu::ShaderSource::Wgsl(
                        format!("{EXPOSURE_WGSL}\n{EXPOSURE_PASS_WGSL}").into(),
                    ),
                })
        });
        assert!(
            failure.is_none(),
            "EXPOSURE_PASS_WGSL must compile"
        );
        // ...and the three fragment stages must be usable as pipelines, which is
        // a stricter check than module validation: it resolves every binding.
        let layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("axiom-exposure-pass-bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 4,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            });
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("axiom-exposure-pass-pl"),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            });
        // The error scope is the SHARED device's, so it is entered exclusively;
        // see `crate::test_gpu::validating`.
        let ((), failure) = crate::test_gpu::validating(&gpu.device, || {
            ["meter_loglum_fs", "meter_reduce_fs", "meter_adapt_fs"]
                .iter()
                .for_each(|entry| {
                    gpu.device
                        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                            label: Some("axiom-exposure-pass-pipeline"),
                            layout: Some(&pipeline_layout),
                            vertex: wgpu::VertexState {
                                module: &module,
                                entry_point: Some("meter_vs"),
                                buffers: &[],
                                compilation_options: wgpu::PipelineCompilationOptions::default(),
                            },
                            fragment: Some(wgpu::FragmentState {
                                module: &module,
                                entry_point: Some(entry),
                                // The metering chain is float end to end: the source
                                // allocates every level as a FloatType RGBA target.
                                targets: &[Some(wgpu::ColorTargetState {
                                    format: wgpu::TextureFormat::Rgba32Float,
                                    blend: None,
                                    write_mask: wgpu::ColorWrites::ALL,
                                })],
                                compilation_options: wgpu::PipelineCompilationOptions::default(),
                            }),
                            primitive: wgpu::PrimitiveState::default(),
                            depth_stencil: None,
                            multisample: wgpu::MultisampleState::default(),
                            multiview: None,
                            cache: None,
                        });
                });
        });
        assert!(
            failure.is_none(),
            "every metering pass must build a pipeline on {:?}",
            gpu.backend
        );
    }
}
