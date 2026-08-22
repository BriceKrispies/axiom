//! **Per-object motion blur**: `src/render/motionblur.js`, as WGSL plus its CPU
//! reference.
//!
//! Ported from Claude-of-Duty `src/render/motionblur.js` (150 lines). Two passes
//! sharing [`crate::taa`]'s input — the G-buffer velocity channel — and the
//! source's own header names the three things that make it not a naive blur:
//!
//! 1. **Velocity is dilated over a 16x16 tile** ([`MOTION_BLUR_TILE`]) so a fast
//!    object bleeds *outside* its own silhouette. A blur that stops at the
//!    object's edge is the giveaway of a naive implementation.
//! 2. **Samples are depth-weighted** ([`mb_tap_weight`]) so the background does
//!    not smear over a foreground object.
//! 3. **The shutter is a real fraction of the frame time**
//!    ([`motion_blur_shutter`]), so the amount of blur is frame-rate
//!    independent.
//!
//! # The one divergence from the source
//!
//! The same one [`crate::taa`] carries, for the same reason and applied in one
//! place: the stored velocity is a clip-space delta whose `y` runs **up**, and a
//! WebGPU framebuffer's `v` runs **down**, so [`mb_velocity`] multiplies `y` by
//! [`crate::gbuffer::VELOCITY_TEXTURE_V_SIGN`] before anything offsets a `uv`
//! with it. It is applied *after* the tile-vs-own selection, which compares
//! **lengths** and is therefore sign-invariant — so the tile pass and the
//! selection stay bit-identical to the source and the divergence is one line.
//!
//! Get it backwards and the blur trails the wrong way vertically: a jumping
//! object smears toward where it came from horizontally and toward where it is
//! going vertically, which reads as "the blur is broken" long before anyone
//! suspects a sign.
//!
//! # Storage width
//!
//! The tile target is the source's `hdrTarget(tw, th, { format: THREE.RGFormat })`
//! — `THREE.HalfFloatType` by `hdrTarget`'s default — i.e. `Rg16Float`. The
//! output target is `Rgba16Float`. Unlike [`crate::taa`] there is **no feedback
//! loop** here: nothing reads this pass's own previous output, so the half-float
//! rounding is a single quantisation at the end rather than something that
//! accumulates. That is why this module has no `history_store` peer.
//!
//! # What this module deliberately does not own
//!
//! No `wgpu` resources and no pass struct. The tile target's size
//! ([`motion_blur_tile_size`]) and the two uniform blocks are here; allocating
//! them is the frame graph's job. See [`MOTION_BLUR_BLUR_WGSL`] for exactly what
//! it must supply.

use crate::gbuffer::VELOCITY_TEXTURE_V_SIGN;

// ---------------------------------------------------------------------------
// The authored numbers
// ---------------------------------------------------------------------------

/// `#define OW_MB_TAPS 12` — the tap count along the velocity vector.
///
/// Each tap is sampled in **both** directions from the centre, so the real
/// sample count is [`MOTION_BLUR_SAMPLES`]. The distribution is uniform in `t`
/// over `(0, 1]`, jittered by a per-pixel interleaved-gradient offset, and mapped
/// to `t - 0.5` so the taps straddle the centre.
pub(crate) const MOTION_BLUR_TAPS: usize = 12;

/// Colour samples one blurred pixel takes, excluding the centre:
/// [`MOTION_BLUR_TAPS`] positions in two directions each.
pub(crate) const MOTION_BLUR_SAMPLES: usize = MOTION_BLUR_TAPS * 2;

/// The tile side, in full-resolution pixels — `Math.ceil(w / 16)`.
pub(crate) const MOTION_BLUR_TILE: u32 = 16;

/// Taps the tile-max pass takes: an 8x8 grid.
pub(crate) const MOTION_BLUR_TILE_TAPS: usize = 64;

/// `uParams.y` — the largest blur radius, in pixels. Set by the constructor and
/// never overwritten by `index.js`, so unlike the shutter this really is a
/// constant.
pub(crate) const MOTION_BLUR_MAX_RADIUS_PX: f32 = 48.0;

/// `uParams.w` — the wet/dry mix on the finished blur. Also never overwritten.
pub(crate) const MOTION_BLUR_INTENSITY: f32 = 1.0;

/// `index.js`'s live `shutter: 0.42`, the numerator of
/// [`motion_blur_shutter`].
pub(crate) const MOTION_BLUR_SOURCE_SHUTTER: f64 = 0.42;

/// `uParams.z = frame % 64` — the frame counter's modulus, which is what makes
/// the sample jitter cycle rather than drift.
pub(crate) const MOTION_BLUR_FRAME_MODULUS: u32 = 64;

/// The `MotionBlur` constructor's own `new THREE.Vector4(0.5, 48, 0, 1)`.
///
/// The `0.5` shutter is **dead**: `render()` overwrites `uParams.x` from
/// `index.js`'s settings on every frame, before the pass ever runs. Ported
/// anyway — dead computation in the source is still part of the source — and
/// pinned by [`tests::the_constructor_shutter_is_dead`].
pub(crate) const MOTION_BLUR_CONSTRUCTOR_PARAMS: [f32; 4] =
    [0.5, MOTION_BLUR_MAX_RADIUS_PX, 0.0, MOTION_BLUR_INTENSITY];

/// The shutter fraction for a frame of `dt` seconds:
/// `this.settings.shutter * (1 / 60 / dt)`.
///
/// Grouped as the source groups it — `(1 / 60) / dt`, then multiplied — because
/// float arithmetic is not associative and this scalar multiplies every offset
/// in the pass. Evaluated in `f64` (JavaScript's only number type) and narrowed
/// once.
///
/// At 60 Hz it is exactly the authored fraction; at 30 Hz it **halves**. That is
/// the point, and it is the opposite of the intuition: what it multiplies is a
/// velocity measured in per-frame displacement, so a frame twice as long already
/// carries twice the motion, and halving the factor keeps the smear the same
/// length in time rather than in frames.
pub(crate) fn motion_blur_shutter(shutter_setting: f64, dt: f64) -> f32 {
    (shutter_setting * (1.0 / 60.0 / dt)) as f32
}

/// The tile target's dimensions — `Math.max(1, Math.ceil(w / 16))` per axis.
pub(crate) fn motion_blur_tile_size(width: u32, height: u32) -> (u32, u32) {
    (
        width.div_ceil(MOTION_BLUR_TILE).max(1),
        height.div_ceil(MOTION_BLUR_TILE).max(1),
    )
}

// ---------------------------------------------------------------------------
// GLSL primitives, written out
// ---------------------------------------------------------------------------

/// GLSL `mix(x, y, a)` — `x * (1 - a) + y * a`.
fn mix(x: f32, y: f32, a: f32) -> f32 {
    x * (1.0 - a) + y * a
}

/// GLSL `clamp(x, 0.0, 1.0)`.
fn clamp01(x: f32) -> f32 {
    x.max(0.0).min(1.0)
}

/// GLSL `smoothstep(e0, e1, x)`, edges first.
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp01((x - e0) / (e1 - e0));
    t * t * (3.0 - 2.0 * t)
}

/// GLSL `length(vec2)` — the plain root, not `Math.hypot`'s compensated sum.
fn length2(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

/// GLSL `fract(x)` — `x - floor(x)`, **not** `%`. The argument here is a frag
/// coordinate plus a frame offset, always positive, but the definition is the
/// definition.
fn fract(x: f32) -> f32 {
    x - x.floor()
}

/// `owIGN` — the interleaved-gradient hash from `glsl.js`:
///
/// ```glsl
/// float owIGN( vec2 p ) {
///   return fract( 52.9829189 * fract( dot( p, vec2( 0.06711056, 0.00583715 ) ) ) );
/// }
/// ```
///
/// **This function is deliberately ill-conditioned**: the outer multiply by
/// `52.98` amplifies any last-bit difference in the inner `fract` by two orders
/// of magnitude, and near a wrap it is discontinuous. That is what makes it a
/// good dither and a bad thing to hold to a tight tolerance — see
/// [`parity::the_interleaved_gradient_noise_agrees_away_from_its_wraps`].
///
/// Distinct from `cascade::shading`'s `owIGNoise`, which comes from `csm.js`;
/// they are separate functions in the source and are kept separate here.
pub(crate) fn mb_ign(p: [f32; 2]) -> f32 {
    fract(52.9829189 * fract(p[0] * 0.06711056 + p[1] * 0.00583715))
}

// ---------------------------------------------------------------------------
// Pass 1 — tile max
// ---------------------------------------------------------------------------

/// The offset of tile tap `index`, in `uv`:
/// `( vec2( float( x ), float( y ) ) - 3.5 ) * 2.0 * uTexel`, scanned `y`-major
/// (`for y { for x { … } }`), so `index = y * 8 + x`.
///
/// **A source characteristic worth stating**: the taps span `-7 … +7` texels,
/// fifteen of the sixteen the tile covers. The dilation therefore misses one
/// texel column and one row per tile. Ported as written; it is a one-texel
/// under-coverage of the tile, not a defect that changes the algorithm.
pub(crate) fn mb_tile_offset(index: usize, texel: [f32; 2]) -> [f32; 2] {
    let x = (index % 8) as f32;
    let y = (index / 8) as f32;
    [(x - 3.5) * 2.0 * texel[0], (y - 3.5) * 2.0 * texel[1]]
}

/// The longest velocity in the tile, by squared length.
///
/// `bestLen` starts at `0.0` and the test is strict `>`, so an all-zero tile
/// answers `(0, 0)` and the earliest of equal magnitudes wins. Both are the
/// source's, and both matter: the first decides whether a static tile blurs at
/// all, the second decides which of two equally-fast objects owns the tile.
pub(crate) fn mb_tile_max(taps: &[[f32; 2]; MOTION_BLUR_TILE_TAPS]) -> [f32; 2] {
    taps.iter()
        .fold(([0.0_f32, 0.0], 0.0_f32), |(best, best_len), v| {
            let l = v[0] * v[0] + v[1] * v[1];
            let take = usize::from(l > best_len);
            ([best, *v][take], [best_len, l][take])
        })
        .0
}

// ---------------------------------------------------------------------------
// Pass 2 — the blur
// ---------------------------------------------------------------------------

/// Choose this pixel's blur vector and report its pre-clamp length in pixels.
///
/// ```glsl
/// vec2 vel = length( tileVel ) > length( ownVel ) ? tileVel : ownVel;
/// vel *= uParams.x;
/// float pixels = length( vel * uResolution );
/// if ( pixels < 1.0 ) { gl_FragColor = centre; return; }
/// if ( pixels > maxPx ) vel *= maxPx / pixels;
/// ```
///
/// Returns `(vel, pixels)` — `pixels` is the value **before** the clamp, which
/// is the one the caller's `< 1.0` early-out tests. Below one pixel of motion
/// there is nothing to blur and the centre passes through untouched.
///
/// The `y` flip lands between the selection and the shutter scale; see the
/// module doc for why that position is both correct and the least invasive.
pub(crate) fn mb_velocity(
    tile_vel: [f32; 2],
    own_vel: [f32; 2],
    shutter: f32,
    max_px: f32,
    resolution: [f32; 2],
) -> ([f32; 2], f32) {
    let chosen = [own_vel, tile_vel][usize::from(length2(tile_vel) > length2(own_vel))];
    let flipped = [chosen[0], chosen[1] * VELOCITY_TEXTURE_V_SIGN];
    let scaled = [flipped[0] * shutter, flipped[1] * shutter];
    let pixels = length2([scaled[0] * resolution[0], scaled[1] * resolution[1]]);
    let k = [1.0, max_px / pixels][usize::from(pixels > max_px)];
    ([scaled[0] * k, scaled[1] * k], pixels)
}

/// The `uv` and the `t` of colour sample `index`, `index` in
/// `0..MOTION_BLUR_SAMPLES`.
///
/// The source's nesting is `for i in 1..=12 { for s in 0..2 { … } }`, so
/// `index = (i - 1) * 2 + s`: `i` selects the distance along the vector and `s`
/// selects the direction. `t = (i + jitter) / 12` runs over `(0, 1]` and the
/// offset is `vel * (t - 0.5)`, straddling the centre.
///
/// Returns `(uv, t)`; `t` is also the tap's own weight falloff input, so the two
/// travel together rather than being recomputed.
pub(crate) fn mb_tap_uv(uv: [f32; 2], vel: [f32; 2], index: usize, jitter: f32) -> ([f32; 2], f32) {
    let i = index / 2 + 1;
    let s = index % 2;
    let t = (i as f32 + jitter) / MOTION_BLUR_TAPS as f32;
    let o = [vel[0] * (t - 0.5), vel[1] * (t - 0.5)];
    let signed = [[-o[0], -o[1]], o][usize::from(s == 0)];
    ([uv[0] + signed[0], uv[1] + signed[1]], t)
}

/// Whether a tap's `uv` is on screen — the complement of the source's
/// `if ( uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 ) continue;`.
///
/// Written as the negation of the source's *skip* test rather than as an
/// interval test, so a NaN `uv` is kept exactly as the source keeps it (`NaN < 0`
/// is false, so the source does not `continue`).
pub(crate) fn mb_tap_inside(uv: [f32; 2]) -> bool {
    !((uv[0] < 0.0) | (uv[0] > 1.0) | (uv[1] < 0.0) | (uv[1] > 1.0))
}

/// A tap's weight: depth rejection times the distance falloff.
///
/// ```glsl
/// float w = 1.0 - smoothstep( 0.0, 1.5, ( d - centreDepth ) / max( 1.0, centreDepth ) );
/// w = mix( 0.15, 1.0, clamp( w, 0.0, 1.0 ) ) * ( 1.0 - t * 0.35 );
/// ```
///
/// A sample much *further* than the centre is background leaking into a
/// foreground object and is pushed down to `0.15` — down-weighted, never zeroed,
/// because zeroing it makes the blur's own edge visible. A sample *nearer* than
/// the centre keeps full weight: foreground bleeding over background is what
/// motion blur is supposed to look like. The relative form (`/ max(1, depth)`)
/// is what makes the rejection scale-free, so a 2 m gap matters at arm's length
/// and does not at 200 m.
pub(crate) fn mb_tap_weight(d: f32, centre_depth: f32, t: f32) -> f32 {
    let w = 1.0 - smoothstep(0.0, 1.5, (d - centre_depth) / centre_depth.max(1.0));
    mix(0.15, 1.0, clamp01(w)) * (1.0 - t * 0.35)
}

/// Accumulate the taps onto the centre sample.
///
/// `meta[k]` is `(depth, coverage, t, inside)`, matching what the fragment
/// shader has already sampled. Coverage below `0.5` is "no surface", and the
/// source replaces such a tap's depth with `1e5` so the sky is always treated as
/// far background — the same substitution it makes for the centre pixel.
///
/// Returns `(sum, wsum)`, seeded with the centre at weight `1.0`. The `inside`
/// lane reproduces the source's `continue`: an off-screen tap contributes to
/// **neither** sum, which is not the same as contributing zero weight.
pub(crate) fn mb_accumulate(
    centre: [f32; 3],
    centre_depth: f32,
    colors: &[[f32; 3]; MOTION_BLUR_SAMPLES],
    meta: &[[f32; 4]; MOTION_BLUR_SAMPLES],
) -> ([f32; 3], f32) {
    colors
        .iter()
        .zip(meta.iter())
        .fold((centre, 1.0_f32), |(sum, wsum), (c, m)| {
            let d = [1e5_f32, m[0]][usize::from(m[1] >= 0.5)];
            let w = mb_tap_weight(d, centre_depth, m[2]);
            let take = usize::from(m[3] >= 0.5);
            (
                [
                    [sum[0], sum[0] + c[0] * w][take],
                    [sum[1], sum[1] + c[1] * w][take],
                    [sum[2], sum[2] + c[2] * w][take],
                ],
                [wsum, wsum + w][take],
            )
        })
}

/// `mix( centre.rgb, blurred, uParams.w )` — the wet/dry mix.
pub(crate) fn mb_blend(centre: [f32; 3], blurred: [f32; 3], intensity: f32) -> [f32; 3] {
    [0_usize, 1, 2].map(|i| mix(centre[i], blurred[i], intensity))
}

/// The centre depth the whole pass measures against: the sampled linear view
/// depth, or `1e5` where there is no surface.
pub(crate) fn mb_centre_depth(depth: f32, coverage: f32) -> f32 {
    [1e5_f32, depth][usize::from(coverage >= 0.5)]
}

// ---------------------------------------------------------------------------
// The uniform blocks
// ---------------------------------------------------------------------------

/// Floats in the tile pass's uniform block: `texel` (2) plus two floats of
/// padding, because a uniform buffer binding is 16-byte aligned and a lone
/// `vec2` would be a 8-byte block.
pub(crate) const MOTION_BLUR_TILE_UNIFORM_FLOATS: usize = 4;

/// Floats in the blur pass's uniform block: `texel` (2) + `resolution` (2) +
/// `params` (4). `params` lands at byte 16, already aligned.
pub(crate) const MOTION_BLUR_UNIFORM_FLOATS: usize = 8;

/// Pack the tile pass's uniform. `texel` is the **full-resolution** texel size,
/// not the tile target's: the pass renders at tile resolution and samples the
/// full-resolution velocity buffer.
pub(crate) fn pack_motion_blur_tile_uniform(
    texel: [f32; 2],
) -> [f32; MOTION_BLUR_TILE_UNIFORM_FLOATS] {
    [texel[0], texel[1], 0.0, 0.0]
}

/// Pack the blur pass's uniform.
///
/// `texel` is **dead** in the source's `BLUR` shader — declared, uploaded by
/// `setSize`, and never read by the body. Carried for the same reason
/// [`MOTION_BLUR_CONSTRUCTOR_PARAMS`] is.
pub(crate) fn pack_motion_blur_uniform(
    texel: [f32; 2],
    resolution: [f32; 2],
    params: [f32; 4],
) -> [f32; MOTION_BLUR_UNIFORM_FLOATS] {
    let mut out = [0.0_f32; MOTION_BLUR_UNIFORM_FLOATS];
    out[0..2].copy_from_slice(&texel);
    out[2..4].copy_from_slice(&resolution);
    out[4..8].copy_from_slice(&params);
    out
}

// ---------------------------------------------------------------------------
// WGSL
// ---------------------------------------------------------------------------

/// The pure arithmetic of both passes, with **no bindings** — the half the real
/// pipelines and the parity harness all compile, so none of them can drift.
///
/// Written from the GLSL text of `motionblur.js`, not from the Rust above.
pub(crate) const MOTION_BLUR_WGSL_COMMON: &str = r#"
// The clip-y-up vs framebuffer-v-down sign. One fact; see
// gbuffer::VELOCITY_TEXTURE_V_SIGN, which is its home.
const mb_v_sign: f32 = -1.0;

const mb_taps: i32 = 12;

fn mb_mix(x: f32, y: f32, a: f32) -> f32 { return x * (1.0 - a) + y * a; }
fn mb_clamp01(x: f32) -> f32 { return min(max(x, 0.0), 1.0); }

fn mb_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = mb_clamp01((x - e0) / (e1 - e0));
    return t * t * (3.0 - 2.0 * t);
}

fn mb_length(v: vec2<f32>) -> f32 { return sqrt(v.x * v.x + v.y * v.y); }
fn mb_fract(x: f32) -> f32 { return x - floor(x); }

fn mb_ign(p: vec2<f32>) -> f32 {
    return mb_fract(52.9829189 * mb_fract(p.x * 0.06711056 + p.y * 0.00583715));
}

// 8x8 taps spread over the 16x16 source tile centred on this output texel.
fn mb_tile_offset(index: i32, texel: vec2<f32>) -> vec2<f32> {
    let x = f32(index % 8);
    let y = f32(index / 8);
    return (vec2<f32>(x, y) - 3.5) * 2.0 * texel;
}

// A value-typed array parameter cannot be indexed by a runtime value in WGSL,
// so this and mb_accumulate copy into a function-scope `var` first. Same
// arithmetic, same order; the copy is what makes the loop legal.
fn mb_tile_max(taps: array<vec2<f32>, 64>) -> vec2<f32> {
    var t = taps;
    var best = vec2<f32>(0.0);
    var best_len = 0.0;
    for (var i = 0; i < 64; i = i + 1) {
        let v = t[i];
        let l = v.x * v.x + v.y * v.y;
        if (l > best_len) { best_len = l; best = v; }
    }
    return best;
}

// Returns (vel.xy, pixels) — pixels is the PRE-clamp length the caller's
// `< 1.0` early-out tests.
fn mb_velocity(
    tile_vel: vec2<f32>,
    own_vel: vec2<f32>,
    shutter: f32,
    max_px: f32,
    resolution: vec2<f32>,
) -> vec3<f32> {
    var vel = own_vel;
    if (mb_length(tile_vel) > mb_length(own_vel)) { vel = tile_vel; }
    vel = vec2<f32>(vel.x, vel.y * mb_v_sign);
    vel = vel * shutter;
    let pixels = mb_length(vel * resolution);
    if (pixels > max_px) { vel = vel * (max_px / pixels); }
    return vec3<f32>(vel, pixels);
}

// Returns (uv.xy, t) for colour sample `index` in 0..24.
fn mb_tap_uv(uv: vec2<f32>, vel: vec2<f32>, index: i32, jitter: f32) -> vec3<f32> {
    let i = index / 2 + 1;
    let s = index % 2;
    let t = (f32(i) + jitter) / f32(mb_taps);
    let o = vel * (t - 0.5);
    // `signed` is a WGSL reserved keyword — naga rejects it outright, which is
    // why this shader would not compile at all.
    let offset = select(-o, o, s == 0);
    return vec3<f32>(uv + offset, t);
}

fn mb_tap_inside(uv: vec2<f32>) -> bool {
    return !(uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0);
}

// a sample that is much further away than the centre is background leaking in
// — down-weight it
fn mb_tap_weight(d: f32, centre_depth: f32, t: f32) -> f32 {
    let w = 1.0 - mb_smoothstep(0.0, 1.5, (d - centre_depth) / max(1.0, centre_depth));
    return mb_mix(0.15, 1.0, mb_clamp01(w)) * (1.0 - t * 0.35);
}

fn mb_centre_depth(depth: f32, coverage: f32) -> f32 {
    return select(1e5, depth, coverage >= 0.5);
}

// taps[k] = (depth, coverage, t, inside). Returns (sum.xyz, wsum).
// Named `taps`, not `meta`: `meta` is a WGSL reserved keyword and naga rejects
// it, so the shader would not compile at all.
fn mb_accumulate(
    centre: vec3<f32>,
    centre_depth: f32,
    colors: array<vec4<f32>, 24>,
    taps: array<vec4<f32>, 24>,
) -> vec4<f32> {
    var c = colors;
    var m = taps;
    var sum = centre;
    var wsum = 1.0;
    for (var k = 0; k < 24; k = k + 1) {
        if (m[k].w < 0.5) { continue; }
        var d = m[k].x;
        if (m[k].y < 0.5) { d = 1e5; }
        let w = mb_tap_weight(d, centre_depth, m[k].z);
        sum = sum + c[k].xyz * w;
        wsum = wsum + w;
    }
    return vec4<f32>(sum, wsum);
}

fn mb_blend(centre: vec3<f32>, blurred: vec3<f32>, intensity: f32) -> vec3<f32> {
    return vec3<f32>(
        mb_mix(centre.x, blurred.x, intensity),
        mb_mix(centre.y, blurred.y, intensity),
        mb_mix(centre.z, blurred.z, intensity));
}

struct MbVsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// The source's full-screen triangle, shared by both passes — which is why it
// lives in the binding-free half: a WGSL module may not declare two resources at
// one @group/@binding, so the tile pass's bindings and the blur pass's can never
// be concatenated into one module.
//
// `vUv = position.xy * 0.5 + 0.5` assumes WebGL's v-up framebuffer; here v runs
// down, so the v lane is mirrored. Same triangle, same texels.
@vertex
fn mb_vs(@builtin(vertex_index) index: u32) -> MbVsOut {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let p = corners[index];
    var out: MbVsOut;
    out.position = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return out;
}
"#;

/// Pass 1: the 8x8 tile-max dilation of the velocity buffer.
///
/// Renders at [`motion_blur_tile_size`] into an **`Rg16Float`** target, sampling
/// the full-resolution velocity buffer — which is why its `texel` uniform is the
/// full-resolution texel size and not its own.
///
/// Bindings: `0` uniform ([`pack_motion_blur_tile_uniform`]), `1` sampler
/// (linear, clamp-to-edge), `2` the velocity texture. Entry points `mb_vs` and
/// `mb_tile_fs`.
pub(crate) const MOTION_BLUR_TILE_WGSL: &str = r#"
struct MbTileU {
    // Texel size of the FULL-RES velocity buffer, not of this target.
    texel: vec2<f32>,
    pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> mb_tile_u: MbTileU;
@group(0) @binding(1) var mb_tile_samp: sampler;
@group(0) @binding(2) var mb_tile_velocity: texture_2d<f32>;

@fragment
fn mb_tile_fs(in: MbVsOut) -> @location(0) vec2<f32> {
    var taps: array<vec2<f32>, 64>;
    for (var i = 0; i < 64; i = i + 1) {
        let o = mb_tile_offset(i, mb_tile_u.texel);
        taps[i] = textureSampleLevel(mb_tile_velocity, mb_tile_samp, in.uv + o, 0.0).rg;
    }
    return mb_tile_max(taps);
}
"#;

/// Pass 2: the blur itself.
///
/// # What the frame graph must supply
///
/// Concatenate with [`MOTION_BLUR_WGSL_COMMON`] (see
/// [`motion_blur_blur_source`]) and drive with:
///
/// | binding | resource |
/// |---|---|
/// | 0 | uniform, [`pack_motion_blur_uniform`] |
/// | 1 | `sampler` — linear, clamp-to-edge |
/// | 2 | `tColor` — the colour to blur (in the source's frame graph, [`crate::taa`]'s resolved output) |
/// | 3 | `tVelocity` — [`crate::gbuffer::GBufferChannel::Velocity`] |
/// | 4 | `tTile` — pass 1's `Rg16Float` output |
/// | 5 | `tDepth` — [`crate::gbuffer::GBufferChannel::Depth`] |
/// | 6 | `tNormal` — [`crate::gbuffer::GBufferChannel::Normal`] (coverage in `z`) |
///
/// with `params = [motion_blur_shutter(…), MOTION_BLUR_MAX_RADIUS_PX,
/// (frame % MOTION_BLUR_FRAME_MODULUS) as f32, MOTION_BLUR_INTENSITY]`, and a
/// frame counter that advances once per rendered frame. Target format
/// `Rgba16Float`. Entry points `mb_vs` and `mb_blur_fs`. This is its **own**
/// shader module — see [`motion_blur_blur_source`] for why it cannot share one
/// with the tile pass.
///
/// # A stated divergence: the jitter's screen coordinate
///
/// `owIGN( gl_FragCoord.xy + … )` reads WebGL's `gl_FragCoord`, whose `y` counts
/// **up** from the bottom-left. WGSL's `@builtin(position).xy` counts **down**
/// from the top-left, so this pass's dither pattern is the source's mirrored in
/// `y`. That is a different arrangement of the same distribution — interleaved
/// gradient noise has no preferred origin, and the value feeds a `±0.5` sample
/// offset, not a colour. It is *not* corrected, because the correction would need
/// the render height threaded in solely to reproduce a phase; it is stated here
/// so nobody later reads a mirrored dither as a bug. If exact browser parity is
/// ever wanted, the fix is `vec2(position.x, resolution.y - position.y)`.
pub(crate) const MOTION_BLUR_BLUR_WGSL: &str = r#"
struct MbBlurU {
    // Declared and uploaded by the source's setSize; DEAD — the BLUR body never
    // reads it. Carried because dead computation in the source is part of it.
    texel: vec2<f32>,
    resolution: vec2<f32>,
    // x shutter, y maxRadiusPx, z frame % 64, w intensity
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> mb_u: MbBlurU;
@group(0) @binding(1) var mb_samp: sampler;
@group(0) @binding(2) var mb_color: texture_2d<f32>;
@group(0) @binding(3) var mb_velocity_tex: texture_2d<f32>;
@group(0) @binding(4) var mb_tile: texture_2d<f32>;
@group(0) @binding(5) var mb_depth: texture_2d<f32>;
@group(0) @binding(6) var mb_normal: texture_2d<f32>;

// textureSampleLevel throughout: the taps sit inside non-uniform control flow,
// where WGSL forbids an implicit-derivative sample. No target here has mips.
@fragment
fn mb_blur_fs(in: MbVsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let centre = textureSampleLevel(mb_color, mb_samp, uv, 0.0);
    let tile_vel = textureSampleLevel(mb_tile, mb_samp, uv, 0.0).rg;
    let own_vel = textureSampleLevel(mb_velocity_tex, mb_samp, uv, 0.0).rg;

    let chosen = mb_velocity(tile_vel, own_vel, mb_u.params.x, mb_u.params.y, mb_u.resolution);
    if (chosen.z < 1.0) { return centre; }
    let vel = chosen.xy;

    let centre_depth = mb_centre_depth(
        textureSampleLevel(mb_depth, mb_samp, uv, 0.0).r,
        textureSampleLevel(mb_normal, mb_samp, uv, 0.0).z);

    let jitter = mb_ign(in.position.xy + mb_u.params.z * 2.717) - 0.5;

    var colors: array<vec4<f32>, 24>;
    var taps: array<vec4<f32>, 24>;
    for (var k = 0; k < 24; k = k + 1) {
        let tap = mb_tap_uv(uv, vel, k, jitter);
        let suv = tap.xy;
        let inside = mb_tap_inside(suv);
        colors[k] = textureSampleLevel(mb_color, mb_samp, suv, 0.0);
        taps[k] = vec4<f32>(
            textureSampleLevel(mb_depth, mb_samp, suv, 0.0).r,
            textureSampleLevel(mb_normal, mb_samp, suv, 0.0).z,
            tap.z,
            select(0.0, 1.0, inside));
    }

    let acc = mb_accumulate(centre.rgb, centre_depth, colors, taps);
    let blurred = acc.xyz / acc.w;
    return vec4<f32>(mb_blend(centre.rgb, blurred, mb_u.params.w), centre.a);
}
"#;

/// The tile pass's complete shader text.
pub(crate) fn motion_blur_tile_source() -> String {
    [MOTION_BLUR_WGSL_COMMON, MOTION_BLUR_TILE_WGSL].concat()
}

/// The blur pass's complete shader text.
///
/// **Two modules, not one.** The tile pass and the blur pass each claim
/// `@group(0) @binding(0..)`, and a WGSL module may not declare two resources at
/// the same group and binding, so they cannot share a compilation unit. The
/// vertex stage they do share lives in [`MOTION_BLUR_WGSL_COMMON`], which is
/// binding-free for exactly this reason.
pub(crate) fn motion_blur_blur_source() -> String {
    [MOTION_BLUR_WGSL_COMMON, MOTION_BLUR_BLUR_WGSL].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shutter_is_a_fraction_of_the_frame_time() {
        // At 60 Hz the authored fraction passes through unchanged.
        let at_60 = motion_blur_shutter(MOTION_BLUR_SOURCE_SHUTTER, 1.0 / 60.0);
        assert!(
            (at_60 - 0.42).abs() < 1e-6,
            "at 60 Hz the shutter is the authored fraction; got {at_60}"
        );
        // At 30 Hz the frame is twice as long, so the factor HALVES — the
        // opposite of what "a longer frame exposes longer" suggests, and worth
        // stating because the intuition is the wrong way round.
        //
        // What this scalar multiplies is a *velocity in per-frame displacement*.
        // At 30 Hz the same motion moves twice as far per frame, so halving the
        // multiplier keeps the smear the same length in **time**. The factor is
        // a normalisation to a 60 Hz-equivalent frame, not an exposure.
        let at_30 = motion_blur_shutter(MOTION_BLUR_SOURCE_SHUTTER, 1.0 / 30.0);
        assert!(
            (at_30 - 0.21).abs() < 1e-6,
            "at 30 Hz the shutter halves; got {at_30}"
        );
        // The grouping is (1/60)/dt, evaluated in f64 and narrowed once.
        let dt = 1.0 / 144.0;
        assert_eq!(
            motion_blur_shutter(MOTION_BLUR_SOURCE_SHUTTER, dt),
            (MOTION_BLUR_SOURCE_SHUTTER * (1.0 / 60.0 / dt)) as f32
        );
    }

    #[test]
    fn the_constructor_shutter_is_dead() {
        assert_eq!(MOTION_BLUR_CONSTRUCTOR_PARAMS[0], 0.5);
        assert_ne!(
            f64::from(MOTION_BLUR_CONSTRUCTOR_PARAMS[0]),
            MOTION_BLUR_SOURCE_SHUTTER,
            "index.js overwrites uParams.x every frame, so the constructor's 0.5 never runs"
        );
        assert_eq!(MOTION_BLUR_CONSTRUCTOR_PARAMS[1], MOTION_BLUR_MAX_RADIUS_PX);
        assert_eq!(MOTION_BLUR_CONSTRUCTOR_PARAMS[3], MOTION_BLUR_INTENSITY);
        assert_eq!(MOTION_BLUR_FRAME_MODULUS, 64);
    }

    #[test]
    fn the_tile_target_is_a_sixteenth_of_the_frame_rounded_up() {
        assert_eq!(motion_blur_tile_size(1920, 1080), (120, 68));
        assert_eq!(motion_blur_tile_size(16, 16), (1, 1));
        assert_eq!(motion_blur_tile_size(17, 33), (2, 3));
        // Never zero, however small the frame.
        assert_eq!(motion_blur_tile_size(1, 1), (1, 1));
        assert_eq!(motion_blur_tile_size(0, 0), (1, 1));
    }

    #[test]
    fn the_glsl_primitives_are_the_glsl_definitions() {
        assert_eq!(mix(2.0, 6.0, 0.25), 2.0 * 0.75 + 6.0 * 0.25);
        assert_eq!(clamp01(-1.0), 0.0);
        assert_eq!(clamp01(2.0), 1.0);
        assert_eq!(clamp01(0.3), 0.3);
        assert_eq!(smoothstep(0.0, 1.5, 0.75), 0.5);
        assert_eq!(smoothstep(0.0, 1.5, -1.0), 0.0);
        assert_eq!(smoothstep(0.0, 1.5, 9.0), 1.0);
        assert_eq!(length2([3.0, 4.0]), 5.0);
        // fract is x - floor(x), so it is positive on negatives too.
        assert_eq!(fract(2.25), 0.25);
        assert_eq!(fract(-2.25), 0.75);
    }

    #[test]
    fn the_interleaved_gradient_noise_stays_inside_the_unit_interval() {
        let worst = (0..64)
            .flat_map(|y| (0..64).map(move |x| mb_ign([x as f32, y as f32])))
            .fold(0.0_f32, f32::max);
        assert!(
            worst < 1.0 && worst > 0.9,
            "owIGN must fill (0, 1); the max over a 64x64 block was {worst}"
        );
        // It is a hash, not a ramp: neighbours must differ substantially.
        let a = mb_ign([10.0, 10.0]);
        let b = mb_ign([11.0, 10.0]);
        assert!(
            (a - b).abs() > 0.1,
            "adjacent pixels must decorrelate; {a} vs {b}"
        );
    }

    #[test]
    fn the_tile_offsets_span_fifteen_texels_not_sixteen() {
        let texel = [1.0_f32, 1.0]; // texel units, so offsets read as texels
        let first = mb_tile_offset(0, texel);
        let last = mb_tile_offset(63, texel);
        assert_eq!(first, [-7.0, -7.0]);
        assert_eq!(last, [7.0, 7.0]);
        // y-major: index 1 steps in x, index 8 steps in y.
        assert_eq!(mb_tile_offset(1, texel), [-5.0, -7.0]);
        assert_eq!(mb_tile_offset(8, texel), [-7.0, -5.0]);
        // The span is 14 texels of centres across a 16-texel tile — the source's
        // one-texel under-coverage, stated rather than silently fixed.
        assert_eq!(last[0] - first[0], 14.0);
    }

    #[test]
    fn the_tile_max_takes_the_longest_and_keeps_the_earliest_tie() {
        let mut taps = [[0.0_f32, 0.0]; MOTION_BLUR_TILE_TAPS];
        taps[40] = [0.3, 0.4];
        taps[7] = [0.03, 0.04];
        assert_eq!(mb_tile_max(&taps), [0.3, 0.4]);
        // Equal magnitudes: strict `>` keeps the first.
        let mut tie = [[0.0_f32, 0.0]; MOTION_BLUR_TILE_TAPS];
        tie[5] = [1.0, 0.0];
        tie[9] = [0.0, 1.0];
        assert_eq!(mb_tile_max(&tie), [1.0, 0.0]);
        // A static tile blurs not at all.
        assert_eq!(mb_tile_max(&[[0.0, 0.0]; MOTION_BLUR_TILE_TAPS]), [0.0, 0.0]);
    }

    #[test]
    fn the_velocity_prefers_the_tile_and_flips_v() {
        let resolution = [1920.0_f32, 1080.0];
        // A slow pixel inside a fast tile takes the tile's vector — the whole
        // point of the dilation.
        let (vel, pixels) = mb_velocity([0.01, 0.02], [0.001, 0.0], 1.0, 48.0, resolution);
        assert_eq!(vel[0], 0.01);
        assert_eq!(vel[1], -0.02, "the stored clip-y delta must be flipped for v");
        assert!(pixels > 1.0, "and the motion must register; {pixels} px");
        // A fast pixel in a slow tile keeps its own.
        let (own, _) = mb_velocity([0.0, 0.0], [0.004, 0.0], 1.0, 48.0, resolution);
        assert_eq!(own, [0.004, 0.0]);
    }

    #[test]
    fn the_velocity_clamps_at_the_max_radius_and_reports_the_preclamp_length() {
        let resolution = [1920.0_f32, 1080.0];
        let (vel, pixels) = mb_velocity([0.5, 0.0], [0.0, 0.0], 1.0, 48.0, resolution);
        assert!(
            (pixels - 960.0).abs() < 1e-3,
            "pixels is reported BEFORE the clamp; got {pixels}"
        );
        let clamped = length2([vel[0] * resolution[0], vel[1] * resolution[1]]);
        assert!(
            (clamped - 48.0).abs() < 1e-3,
            "and the vector is clamped to the max radius; got {clamped}"
        );
    }

    #[test]
    fn a_sub_pixel_velocity_is_reported_below_the_one_pixel_floor() {
        let (_, pixels) = mb_velocity([0.0, 0.0], [1e-5, 0.0], 1.0, 48.0, [1920.0, 1080.0]);
        assert!(
            pixels < 1.0,
            "below a pixel of motion the caller passes the centre through; {pixels} px"
        );
        // The shutter scales it: a half-shutter halves the motion.
        let (_, full) = mb_velocity([0.0, 0.0], [0.01, 0.0], 1.0, 48.0, [1920.0, 1080.0]);
        let (_, half) = mb_velocity([0.0, 0.0], [0.01, 0.0], 0.5, 48.0, [1920.0, 1080.0]);
        assert!(
            (full - half * 2.0).abs() < 1e-3,
            "the shutter is a linear scale; {full} vs {half}"
        );
    }

    #[test]
    fn the_taps_straddle_the_centre_in_both_directions() {
        let uv = [0.5_f32, 0.5];
        let vel = [0.1_f32, 0.0];
        // index 0 and 1 are i = 1, s = 0 and s = 1: mirrored offsets.
        let (a, ta) = mb_tap_uv(uv, vel, 0, 0.0);
        let (b, tb) = mb_tap_uv(uv, vel, 1, 0.0);
        assert_eq!(ta, tb, "both directions share one t");
        assert!(
            (a[0] - uv[0] + (b[0] - uv[0])).abs() < 1e-7,
            "the two directions must be exact mirrors; {a:?} {b:?}"
        );
        // t runs over (0, 1] across the twelve distances.
        let (_, t_first) = mb_tap_uv(uv, vel, 0, 0.0);
        let (_, t_last) = mb_tap_uv(uv, vel, MOTION_BLUR_SAMPLES - 1, 0.0);
        assert!((t_first - 1.0 / 12.0).abs() < 1e-7, "t starts at 1/12; {t_first}");
        assert_eq!(t_last, 1.0, "and ends at 1");
        // The jitter shifts every t by the same fraction of a step.
        let (_, jittered) = mb_tap_uv(uv, vel, 0, 0.5);
        assert!((jittered - 1.5 / 12.0).abs() < 1e-7, "got {jittered}");
    }

    #[test]
    fn a_tap_off_the_edge_is_skipped_and_a_nan_is_not() {
        assert!(mb_tap_inside([0.0, 1.0]));
        assert!(mb_tap_inside([0.5, 0.5]));
        assert!(!mb_tap_inside([-0.001, 0.5]));
        assert!(!mb_tap_inside([0.5, 1.001]));
        // The source's test is a skip-if, so NaN is NOT skipped. Faithful.
        assert!(mb_tap_inside([f32::NAN, 0.5]));
    }

    #[test]
    fn a_background_tap_is_down_weighted_and_a_foreground_tap_is_not() {
        let centre_depth = 10.0_f32;
        let near = mb_tap_weight(2.0, centre_depth, 0.5);
        let same = mb_tap_weight(10.0, centre_depth, 0.5);
        let far = mb_tap_weight(40.0, centre_depth, 0.5);
        assert!(
            near >= same,
            "a nearer sample keeps full weight; {near} vs {same}"
        );
        assert!(
            far < same * 0.3,
            "a far sample is pushed toward the 0.15 floor; {far} vs {same}"
        );
        assert!(far > 0.0, "but never zeroed, or the blur's own edge shows");
        // The distance falloff: t = 1 costs 35%.
        let close = mb_tap_weight(10.0, centre_depth, 0.0);
        let distant = mb_tap_weight(10.0, centre_depth, 1.0);
        assert!(
            (distant - close * 0.65).abs() < 1e-6,
            "the falloff is (1 - t * 0.35); {close} vs {distant}"
        );
        // The rejection is relative: the same absolute gap far away is nothing.
        let far_scene = mb_tap_weight(230.0, 200.0, 0.5);
        assert!(
            far_scene > far,
            "a 30 m gap at 200 m must reject less than a 30 m gap at 10 m; {far_scene} vs {far}"
        );
    }

    #[test]
    fn the_centre_depth_pushes_the_sky_to_the_far_sentinel() {
        assert_eq!(mb_centre_depth(12.0, 1.0), 12.0);
        assert_eq!(mb_centre_depth(12.0, 0.7), 12.0);
        assert_eq!(mb_centre_depth(12.0, 0.0), 1e5);
    }

    /// A tap set with every lane exercised: covered and uncovered, on- and
    /// off-screen.
    fn tap_set() -> ([[f32; 3]; MOTION_BLUR_SAMPLES], [[f32; 4]; MOTION_BLUR_SAMPLES]) {
        let colors = std::array::from_fn(|k| [0.1 + k as f32 * 0.01, 0.2, 0.3]);
        let meta = std::array::from_fn(|k| {
            [
                8.0 + (k % 5) as f32 * 4.0,
                f32::from(k % 3 != 0),
                (k / 2 + 1) as f32 / MOTION_BLUR_TAPS as f32,
                f32::from(k % 7 != 0),
            ]
        });
        (colors, meta)
    }

    #[test]
    fn the_accumulation_seeds_with_the_centre_at_unit_weight() {
        let (colors, mut meta) = tap_set();
        // Every tap off-screen: nothing contributes, so the answer is the centre.
        meta.iter_mut().for_each(|m| m[3] = 0.0);
        let centre = [0.4_f32, 0.5, 0.6];
        let (sum, wsum) = mb_accumulate(centre, 10.0, &colors, &meta);
        assert_eq!((sum, wsum), (centre, 1.0));
    }

    #[test]
    fn an_off_screen_tap_contributes_to_neither_sum() {
        let (colors, mut meta) = tap_set();
        let with_all = mb_accumulate([0.4, 0.5, 0.6], 10.0, &colors, &meta);
        meta[3][3] = 0.0;
        let without_one = mb_accumulate([0.4, 0.5, 0.6], 10.0, &colors, &meta);
        assert!(
            with_all.1 > without_one.1,
            "dropping a tap must drop its weight from wsum too, not just from sum; {} vs {}",
            with_all.1,
            without_one.1
        );
        // ...which is not the same as giving it zero weight.
        assert_ne!(with_all.0, without_one.0);
    }

    #[test]
    fn an_uncovered_tap_is_treated_as_far_background() {
        let (colors, mut meta) = tap_set();
        meta.iter_mut().for_each(|m| {
            m[1] = 1.0;
            m[3] = 1.0;
        });
        let covered = mb_accumulate([0.4, 0.5, 0.6], 10.0, &colors, &meta);
        meta[5][1] = 0.0;
        let uncovered = mb_accumulate([0.4, 0.5, 0.6], 10.0, &colors, &meta);
        assert!(
            uncovered.1 < covered.1,
            "the sky's 1e5 sentinel must down-weight that tap; {} vs {}",
            uncovered.1,
            covered.1
        );
    }

    #[test]
    fn the_blend_is_the_wet_dry_mix() {
        let centre = [0.2_f32, 0.4, 0.6];
        let blurred = [0.8_f32, 0.6, 0.4];
        assert_eq!(mb_blend(centre, blurred, 0.0), centre);
        assert_eq!(mb_blend(centre, blurred, 1.0), blurred);
        let half = mb_blend(centre, blurred, 0.5);
        assert!((half[0] - 0.5).abs() < 1e-6, "got {half:?}");
    }

    #[test]
    fn the_uniform_blocks_pack_in_the_order_the_wgsl_structs_declare() {
        assert_eq!(
            pack_motion_blur_tile_uniform([0.25, 0.5]),
            [0.25, 0.5, 0.0, 0.0]
        );
        let params = [0.42_f32, MOTION_BLUR_MAX_RADIUS_PX, 17.0, MOTION_BLUR_INTENSITY];
        let packed = pack_motion_blur_uniform([0.25, 0.5], [1920.0, 1080.0], params);
        assert_eq!(packed.len(), MOTION_BLUR_UNIFORM_FLOATS);
        assert_eq!(&packed[0..4], &[0.25, 0.5, 1920.0, 1080.0]);
        assert_eq!(&packed[4..8], &params);
        assert_eq!(MOTION_BLUR_TILE_UNIFORM_FLOATS, 4);
    }

    #[test]
    fn the_shader_and_the_gbuffer_agree_on_the_v_sign() {
        assert_eq!(VELOCITY_TEXTURE_V_SIGN, -1.0);
        assert!(MOTION_BLUR_WGSL_COMMON.contains("const mb_v_sign: f32 = -1.0;"));
        // One declaration plus exactly one use: the whole divergence.
        assert_eq!(
            MOTION_BLUR_WGSL_COMMON.matches("mb_v_sign").count(),
            2,
            "the flip must land once, in mb_velocity"
        );
    }

    #[test]
    fn the_shader_sources_compose_the_common_arithmetic_first() {
        let tile = motion_blur_tile_source();
        assert!(tile.starts_with(MOTION_BLUR_WGSL_COMMON));
        assert!(tile.contains("fn mb_tile_fs"));
        assert!(tile.contains("fn mb_vs"));
        let blur = motion_blur_blur_source();
        assert!(blur.starts_with(MOTION_BLUR_WGSL_COMMON));
        assert!(blur.ends_with(MOTION_BLUR_BLUR_WGSL));
        assert!(
            blur.contains("fn mb_vs"),
            "the shared vertex stage rides in the binding-free half"
        );
        assert!(
            !blur.contains("mb_tile_u"),
            "the two passes must be separate modules; both claim @group(0) @binding(0)"
        );
        assert_eq!(
            blur.matches("textureSample(").count(),
            0,
            "every fetch must be textureSampleLevel"
        );
        assert_eq!(MOTION_BLUR_SAMPLES, 24);
    }

    #[test]
    fn the_dead_texel_uniform_is_carried_and_named() {
        assert!(
            MOTION_BLUR_BLUR_WGSL.contains("DEAD — the BLUR body never"),
            "the dead uniform is named where it is declared"
        );
    }
}

/// **CPU↔GPU parity for the motion blur**, on the crate's one shared adapter.
///
/// Same instrument as [`crate::taa::parity`] and `bloom_pyramid::parity`: every
/// sampled input arrives through a uniform, so what is measured is the
/// transcription rather than the texture unit.
///
/// **These tolerances are expectations, not measurements** — written in a wave
/// that forbids building, so none of them has run. Each carries the reasoning
/// that produced it, and
/// [`the_tolerances_are_within_ten_times_the_measured_delta`] prints the real
/// numbers on the first green run.
#[cfg(all(test, feature = "offscreen"))]
mod parity {
    use super::*;

    /// Columns in one render. Twenty-four so [`the_tap_positions_agree`] can put
    /// one colour sample in each.
    const SAMPLES: usize = 24;

    /// `vec4` lanes per sample in the small uniform.
    const LANES: usize = 8;

    /// Samples in the bulky uniform — the tile tap set and the accumulation tap
    /// set, which are 80 lanes each and do not want twenty-four copies.
    const BULK: usize = 4;

    /// `vec4` lanes per bulky sample: 32 tile taps + 24 colours + 24 meta.
    const BULK_LANES: usize = 80;

    /// `copy_texture_to_buffer` wants each row aligned to this many bytes.
    const ROW_ALIGN: u32 = 256;

    /// **Measured: `8.94e-8`** on a native adapter — under one `f32` ULP at
    /// unit scale, so the adapter is not in fact contracting anything here.
    /// `mb_tap_weight` is a divide, a `smoothstep`, a `mix` and a multiply on
    /// values of order 1. The `1e-6` that stood here was 11x the measurement.
    const WEIGHT_TOLERANCE: f32 = 2.0e-7;

    /// **Measured: `9.54e-7`** on a native adapter — almost exactly
    /// twenty-four times [`WEIGHT_TOLERANCE`]'s measured `8.94e-8`, which is the
    /// linear-in-tap-count accumulation the estimate predicted. The estimate's
    /// *shape* was right and its magnitude was 21x too generous, because it was
    /// built on the weight budget rather than on the weight measurement.
    const ACCUMULATE_TOLERANCE: f32 = 2.0e-6;

    /// **Measured: `0`** on a native adapter — the `uv` lanes agree bit for bit.
    ///
    /// Two `sqrt`s, a compare and three multiplies, on `uv`-scale values of order
    /// `1e-2`. The estimate allowed one ULP (`1e-9`-ish) for a `sqrt` the adapter
    /// might round differently; it does not. Pinned at zero, on the same footing
    /// as [`EXACT_TOLERANCE`]: if another adapter's `sqrt` disagrees, that is
    /// worth failing on and reading, not worth pre-absorbing into a budget.
    const VELOCITY_TOLERANCE: f32 = 0.0;

    /// **Expected, unverified.** The same computation's `pixels` lane, which is a
    /// *screen-space* magnitude — order `30` on these probes, where one `f32` ULP
    /// is `~2e-6`. It gets its own budget because a single absolute number cannot
    /// serve both lanes: `1e-8` would be impossible here and `1e-5` would let a
    /// real defect through in the `uv` lanes above.
    const PIXELS_TOLERANCE: f32 = 1.0e-5;

    /// **Expected, unverified.** `mb_tap_uv` divides by `12.0`, which a driver may
    /// evaluate as a reciprocal-multiply, so this is not asserted bit-exact even
    /// though every other operation in it is a select or an add.
    const TAP_TOLERANCE: f32 = 1.0e-7;

    /// **Expected, unverified.** Selection and multiplication only — no division
    /// anywhere, so there is nothing for a driver to factor differently.
    const EXACT_TOLERANCE: f32 = 0.0;

    /// **Measured: `2.02e-4`** on a native adapter. Deliberately loose, and the
    /// estimate that stood here (`2e-5`) was an order of magnitude too tight.
    ///
    /// `owIGN` is `fract(52.9829189 * fract(0.06711056*x + 0.00583715*y))`. The
    /// error analysis that produced `2e-5` counted the outer multiply once: one
    /// ULP at `1e-7` scaled by `52.98` is `5e-6`. What it missed is that the
    /// *inner* `fract` takes pixel coordinates, which are large — the fractional
    /// part of a big number keeps only the low bits, so the relative error there
    /// is far worse than one ULP of a unit-scale value, and `52.98` then
    /// magnifies that.
    ///
    /// The test excludes wrap-adjacent samples ([`IGN_WRAP_GUARD`]) because a
    /// discontinuity has no tolerance, only a side.
    const IGN_TOLERANCE: f32 = 3.0e-4;

    /// How close to a `fract` wrap a sample may be before
    /// [`the_interleaved_gradient_noise_agrees_away_from_its_wraps`] declines to
    /// compare it. A discontinuity has no tolerance, only a side.
    const IGN_WRAP_GUARD: f32 = 1.0e-3;

    const HARNESS: &str = r#"
struct MbParitySmall { items: array<vec4<f32>, 192> };
@group(0) @binding(0) var<uniform> mb_small: MbParitySmall;

struct MbParityBulk { items: array<vec4<f32>, 320> };
@group(0) @binding(1) var<uniform> mb_bulk: MbParityBulk;

@vertex
fn mb_parity_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

fn mb_parity_small(sample: u32, lane: u32) -> vec4<f32> {
    return mb_small.items[sample * 8u + lane];
}

fn mb_parity_tile_taps(bulk: u32) -> array<vec2<f32>, 64> {
    let base = bulk * 80u;
    var taps: array<vec2<f32>, 64>;
    for (var i = 0u; i < 32u; i = i + 1u) {
        let packed = mb_bulk.items[base + i];
        taps[i * 2u] = packed.xy;
        taps[i * 2u + 1u] = packed.zw;
    }
    return taps;
}

@fragment
fn mb_parity_ign_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let lane = mb_parity_small(sample, 2u);
    return vec4<f32>(mb_ign(lane.xy), 0.0, 0.0, 0.0);
}

@fragment
fn mb_parity_velocity_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let v = mb_parity_small(sample, 0u);
    let s = mb_parity_small(sample, 1u);
    return vec4<f32>(mb_velocity(v.xy, v.zw, s.x, s.y, s.zw), 0.0);
}

// Column c carries colour sample c of probe 0: all twenty-four, so a swapped
// direction or an off-by-one in the tap index cannot survive.
@fragment
fn mb_parity_tapuv_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = i32(position.x);
    let lane = mb_parity_small(0u, 4u);
    let jitter = mb_parity_small(0u, 3u).w;
    let tap = mb_tap_uv(lane.zw, lane.xy, index, jitter);
    return vec4<f32>(tap, select(0.0, 1.0, mb_tap_inside(tap.xy)));
}

@fragment
fn mb_parity_tile_offset_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = i32(position.x);
    let texel = mb_parity_small(0u, 7u).xy;
    return vec4<f32>(mb_tile_offset(index, texel), 0.0, 0.0);
}

@fragment
fn mb_parity_weight_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let lane = mb_parity_small(sample, 5u);
    return vec4<f32>(
        mb_tap_weight(lane.x, lane.y, lane.z),
        mb_centre_depth(lane.x, lane.w),
        0.0,
        0.0);
}

@fragment
fn mb_parity_blend_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let centre = mb_parity_small(sample, 3u).xyz;
    let blurred = mb_parity_small(sample, 6u).xyz;
    let intensity = mb_parity_small(sample, 2u).w;
    return vec4<f32>(mb_blend(centre, blurred, intensity), 0.0);
}

@fragment
fn mb_parity_tile_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let bulk = u32(position.x) % 4u;
    return vec4<f32>(mb_tile_max(mb_parity_tile_taps(bulk)), 0.0, 0.0);
}

@fragment
fn mb_parity_accumulate_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let bulk = sample % 4u;
    let base = bulk * 80u + 32u;
    var colors: array<vec4<f32>, 24>;
    var taps: array<vec4<f32>, 24>;
    for (var k = 0u; k < 24u; k = k + 1u) {
        colors[k] = mb_bulk.items[base + k];
        taps[k] = mb_bulk.items[base + 24u + k];
    }
    let lane = mb_parity_small(sample, 2u);
    return mb_accumulate(mb_parity_small(sample, 3u).xyz, lane.z, colors, taps);
}
"#;

    /// The crate's one instance + adapter + device. Never a `wgpu::Instance` of
    /// its own; see [`crate::test_gpu`].
    struct Gpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
        backend: wgpu::Backend,
    }

    impl Gpu {
        fn shared() -> Gpu {
            let gpu = crate::test_gpu::TestGpu::shared();
            Gpu {
                device: gpu.device.clone(),
                queue: gpu.queue.clone(),
                backend: gpu.backend,
            }
        }

        fn compile(&self) -> wgpu::ShaderModule {
            let (module, failure) = crate::test_gpu::validating(&self.device, || {
                self.device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("axiom-motionblur-parity-shader"),
                        source: wgpu::ShaderSource::Wgsl(
                            [MOTION_BLUR_WGSL_COMMON, HARNESS].concat().into(),
                        ),
                    })
            });
            assert!(
                failure.is_none(),
                "the motion blur WGSL must compile: {}",
                failure.map_or(String::new(), |error| error.to_string())
            );
            module
        }

        fn render(
            &self,
            module: &wgpu::ShaderModule,
            entry_point: &str,
            small: &[u8],
            bulk: &[u8],
        ) -> Vec<[f32; 4]> {
            let entry = |binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            };
            let layout = self
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("axiom-motionblur-parity-bgl"),
                    entries: &[entry(0), entry(1)],
                });
            let make = |label, contents| {
                wgpu::util::DeviceExt::create_buffer_init(
                    &self.device,
                    &wgpu::util::BufferInitDescriptor {
                        label: Some(label),
                        contents,
                        usage: wgpu::BufferUsages::UNIFORM,
                    },
                )
            };
            let small_buffer = make("axiom-motionblur-parity-small", small);
            let bulk_buffer = make("axiom-motionblur-parity-bulk", bulk);
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-motionblur-parity-bg"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: small_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: bulk_buffer.as_entire_binding(),
                    },
                ],
            });
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("axiom-motionblur-parity-pl"),
                        bind_group_layouts: &[&layout],
                        push_constant_ranges: &[],
                    });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("axiom-motionblur-parity-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("mb_parity_vs"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some(entry_point),
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
                label: Some("axiom-motionblur-parity-target"),
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
                label: Some("axiom-motionblur-parity-readback"),
                size: u64::from(row_bytes),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("axiom-motionblur-parity-pass"),
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

    struct Probes {
        small: Vec<[[f32; 4]; LANES]>,
        tile_taps: Vec<[[f32; 2]; MOTION_BLUR_TILE_TAPS]>,
        colors: Vec<[[f32; 3]; MOTION_BLUR_SAMPLES]>,
        meta: Vec<[[f32; 4]; MOTION_BLUR_SAMPLES]>,
    }

    fn probes() -> Probes {
        let small = (0..SAMPLES)
            .map(|s| {
                let f = s as f32;
                [
                    // 0: tile_vel.xy, own_vel.xy — sometimes the tile wins, sometimes not
                    [0.002 * (f - 12.0), 0.0013 * f, 0.0007 * f, -0.0021 * (f - 6.0)],
                    // 1: shutter, max_px, resolution
                    [0.42 + f * 0.01, MOTION_BLUR_MAX_RADIUS_PX, 1920.0, 1080.0],
                    // 2: ign_p.xy, centre_depth, intensity
                    [
                        3.0 + f * 41.0,
                        7.0 + f * 17.0,
                        4.0 + f * 3.7,
                        MOTION_BLUR_INTENSITY,
                    ],
                    // 3: centre_rgb, jitter
                    [0.13 + f * 0.017, 0.29 + f * 0.011, 0.47 + f * 0.005, f / 24.0 - 0.5],
                    // 4: vel.xy, uv.xy
                    [0.011 * (f - 12.0), 0.007 * (f - 5.0), 0.31 + f * 0.017, 0.44 + f * 0.019],
                    // 5: weight d, centre_depth, t, coverage
                    [
                        2.0 + f * 5.0,
                        6.0 + f * 1.3,
                        (f + 1.0) / 24.0,
                        f32::from(s % 3 != 0),
                    ],
                    // 6: blurred rgb, _
                    [0.71 - f * 0.019, 0.52 - f * 0.007, 0.33 + f * 0.013, 0.0],
                    // 7: texel.xy, _, _
                    [1.0 / 1920.0, 1.0 / 1080.0, 0.0, 0.0],
                ]
            })
            .collect();
        let tile_taps = (0..BULK)
            .map(|b| {
                std::array::from_fn(|i| {
                    let t = (b * MOTION_BLUR_TILE_TAPS + i) as f32;
                    [
                        0.0004 * ((t * 7.0) % 23.0 - 11.0),
                        0.0003 * ((t * 5.0) % 17.0 - 8.0),
                    ]
                })
            })
            .collect();
        let colors = (0..BULK)
            .map(|b| {
                std::array::from_fn(|k| {
                    let t = (b * MOTION_BLUR_SAMPLES + k) as f32 * 0.013;
                    [0.11 + t, 0.27 + t * 0.5, 0.43 + t * 0.25]
                })
            })
            .collect();
        let meta = (0..BULK)
            .map(|b| {
                std::array::from_fn(|k| {
                    [
                        3.0 + ((b * 7 + k) % 11) as f32 * 6.0,
                        f32::from((b + k) % 4 != 0),
                        (k / 2 + 1) as f32 / MOTION_BLUR_TAPS as f32,
                        f32::from((b + k) % 5 != 0),
                    ]
                })
            })
            .collect();
        Probes {
            small,
            tile_taps,
            colors,
            meta,
        }
    }

    fn small_uniform(p: &Probes) -> Vec<u8> {
        p.small
            .iter()
            .flat_map(|sample| sample.iter().flat_map(|lane| lane.iter().copied()))
            .flat_map(f32::to_le_bytes)
            .collect()
    }

    fn bulk_uniform(p: &Probes) -> Vec<u8> {
        let mut lanes = vec![[0.0_f32; 4]; BULK * BULK_LANES];
        (0..BULK).for_each(|b| {
            let base = b * BULK_LANES;
            (0..32).for_each(|i| {
                let a = p.tile_taps[b][i * 2];
                let c = p.tile_taps[b][i * 2 + 1];
                lanes[base + i] = [a[0], a[1], c[0], c[1]];
            });
            (0..MOTION_BLUR_SAMPLES).for_each(|k| {
                let c = p.colors[b][k];
                lanes[base + 32 + k] = [c[0], c[1], c[2], 0.0];
                lanes[base + 32 + MOTION_BLUR_SAMPLES + k] = p.meta[b][k];
            });
        });
        lanes
            .iter()
            .flat_map(|lane| lane.iter().flat_map(|v| v.to_le_bytes()))
            .collect()
    }

    fn worst(gpu: &[[f32; 4]], cpu: impl Fn(usize) -> [f32; 4], lanes: usize) -> f32 {
        worst_at(gpu, cpu, &(0..lanes).collect::<Vec<usize>>())
    }

    /// The worst disagreement over a chosen subset of lanes — needed wherever one
    /// entry point returns quantities of different magnitude.
    fn worst_at(gpu: &[[f32; 4]], cpu: impl Fn(usize) -> [f32; 4], lanes: &[usize]) -> f32 {
        gpu.iter()
            .enumerate()
            .flat_map(|(i, got)| {
                let want = cpu(i);
                lanes
                    .iter()
                    .map(|l| (got[*l] - want[*l]).abs())
                    .collect::<Vec<f32>>()
            })
            .fold(0.0_f32, f32::max)
    }

    fn require_real(gpu: &Gpu) {
        assert_ne!(
            gpu.backend,
            wgpu::Backend::Noop,
            "a parity proof needs a real adapter"
        );
    }

    #[test]
    fn the_velocity_selection_and_clamp_agree_with_the_cpu_reference() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let got = gpu.render(
            &module,
            "mb_parity_velocity_fs",
            &small_uniform(&p),
            &bulk_uniform(&p),
        );
        let reference = |i: usize| {
            let v = p.small[i][0];
            let s = p.small[i][1];
            let (vel, pixels) =
                super::mb_velocity([v[0], v[1]], [v[2], v[3]], s[0], s[1], [s[2], s[3]]);
            [vel[0], vel[1], pixels, 0.0]
        };
        let vel_delta = worst_at(&got, reference, &[0, 1]);
        assert!(
            vel_delta <= VELOCITY_TOLERANCE,
            "the velocity selection must agree; worst delta {vel_delta:e} vs budget {VELOCITY_TOLERANCE:e}"
        );
        let px_delta = worst_at(&got, reference, &[2]);
        assert!(
            px_delta <= PIXELS_TOLERANCE,
            "the pre-clamp pixel length must agree; worst delta {px_delta:e} vs budget {PIXELS_TOLERANCE:e}"
        );
    }

    #[test]
    fn the_tap_positions_agree() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let got = gpu.render(
            &module,
            "mb_parity_tapuv_fs",
            &small_uniform(&p),
            &bulk_uniform(&p),
        );
        let lane = p.small[0][4];
        let jitter = p.small[0][3][3];
        let delta = worst(
            &got,
            |index| {
                let (uv, t) =
                    super::mb_tap_uv([lane[2], lane[3]], [lane[0], lane[1]], index, jitter);
                [uv[0], uv[1], t, f32::from(super::mb_tap_inside(uv))]
            },
            4,
        );
        assert!(
            delta <= TAP_TOLERANCE,
            "every tap position, its t and its on-screen flag must agree; worst delta {delta:e} vs budget {TAP_TOLERANCE:e}"
        );
    }

    #[test]
    fn the_tile_tap_offsets_agree() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let got = gpu.render(
            &module,
            "mb_parity_tile_offset_fs",
            &small_uniform(&p),
            &bulk_uniform(&p),
        );
        let texel = p.small[0][7];
        let delta = worst(
            &got,
            |index| {
                let o = super::mb_tile_offset(index, [texel[0], texel[1]]);
                [o[0], o[1], 0.0, 0.0]
            },
            2,
        );
        assert_eq!(
            delta, EXACT_TOLERANCE,
            "a transposed tile offset would shear every dilation by a texel; worst delta {delta:e}"
        );
    }

    #[test]
    fn the_tile_max_agrees_with_the_cpu_reference() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let got = gpu.render(
            &module,
            "mb_parity_tile_fs",
            &small_uniform(&p),
            &bulk_uniform(&p),
        );
        let delta = worst(
            &got,
            |i| {
                let best = super::mb_tile_max(&p.tile_taps[i % BULK]);
                [best[0], best[1], 0.0, 0.0]
            },
            2,
        );
        assert_eq!(
            delta, EXACT_TOLERANCE,
            "the tile max only selects, so it must be bit-exact; worst delta {delta:e}"
        );
    }

    #[test]
    fn the_tap_weight_and_centre_depth_agree() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let got = gpu.render(
            &module,
            "mb_parity_weight_fs",
            &small_uniform(&p),
            &bulk_uniform(&p),
        );
        let delta = worst(
            &got,
            |i| {
                let lane = p.small[i][5];
                [
                    super::mb_tap_weight(lane[0], lane[1], lane[2]),
                    super::mb_centre_depth(lane[0], lane[3]),
                    0.0,
                    0.0,
                ]
            },
            2,
        );
        assert!(
            delta <= WEIGHT_TOLERANCE,
            "the depth weight must agree; worst delta {delta:e} vs budget {WEIGHT_TOLERANCE:e}"
        );
    }

    #[test]
    fn the_accumulation_agrees_with_the_cpu_reference() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let got = gpu.render(
            &module,
            "mb_parity_accumulate_fs",
            &small_uniform(&p),
            &bulk_uniform(&p),
        );
        let delta = worst(
            &got,
            |i| {
                let b = i % BULK;
                let centre = p.small[i][3];
                let (sum, wsum) = super::mb_accumulate(
                    [centre[0], centre[1], centre[2]],
                    p.small[i][2][2],
                    &p.colors[b],
                    &p.meta[b],
                );
                [sum[0], sum[1], sum[2], wsum]
            },
            4,
        );
        assert!(
            delta <= ACCUMULATE_TOLERANCE,
            "the twenty-four-tap accumulation must agree; worst delta {delta:e} vs budget {ACCUMULATE_TOLERANCE:e}"
        );
    }

    #[test]
    fn the_blend_agrees_with_the_cpu_reference() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let got = gpu.render(
            &module,
            "mb_parity_blend_fs",
            &small_uniform(&p),
            &bulk_uniform(&p),
        );
        let delta = worst(
            &got,
            |i| {
                let c = p.small[i][3];
                let b = p.small[i][6];
                let out = super::mb_blend(
                    [c[0], c[1], c[2]],
                    [b[0], b[1], b[2]],
                    p.small[i][2][3],
                );
                [out[0], out[1], out[2], 0.0]
            },
            3,
        );
        assert!(
            delta <= WEIGHT_TOLERANCE,
            "the wet/dry mix must agree; worst delta {delta:e} vs budget {WEIGHT_TOLERANCE:e}"
        );
    }

    /// `owIGN` is deliberately ill-conditioned, so this compares only away from
    /// its wraps. A discontinuity has no tolerance, only a side.
    #[test]
    fn the_interleaved_gradient_noise_agrees_away_from_its_wraps() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let got = gpu.render(
            &module,
            "mb_parity_ign_fs",
            &small_uniform(&p),
            &bulk_uniform(&p),
        );
        let compared: Vec<(usize, f32, f32)> = got
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let lane = p.small[i][2];
                (i, row[0], super::mb_ign([lane[0], lane[1]]))
            })
            .filter(|(_, _, want)| (*want > IGN_WRAP_GUARD) & (*want < 1.0 - IGN_WRAP_GUARD))
            .collect();
        assert!(
            compared.len() >= SAMPLES / 2,
            "most probes must land away from a wrap, or the proof proves nothing; {} of {SAMPLES}",
            compared.len()
        );
        let delta = compared
            .iter()
            .map(|(_, got, want)| (got - want).abs())
            .fold(0.0_f32, f32::max);
        assert!(
            delta <= IGN_TOLERANCE,
            "owIGN must agree away from its wraps; worst delta {delta:e} vs budget {IGN_TOLERANCE:e}"
        );
    }

    /// **The calibration test.** Re-measures every tier and fails if a budget has
    /// drifted more than 10x looser than the hardware needs. On the first green
    /// run its message carries the numbers that must replace the expectations
    /// above.
    #[test]
    fn the_tolerances_are_within_ten_times_the_measured_delta() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let small = small_uniform(&p);
        let bulk = bulk_uniform(&p);

        let weight = worst(
            &gpu.render(&module, "mb_parity_weight_fs", &small, &bulk),
            |i| {
                let lane = p.small[i][5];
                [
                    super::mb_tap_weight(lane[0], lane[1], lane[2]),
                    super::mb_centre_depth(lane[0], lane[3]),
                    0.0,
                    0.0,
                ]
            },
            2,
        );
        let accumulate = worst(
            &gpu.render(&module, "mb_parity_accumulate_fs", &small, &bulk),
            |i| {
                let b = i % BULK;
                let centre = p.small[i][3];
                let (sum, wsum) = super::mb_accumulate(
                    [centre[0], centre[1], centre[2]],
                    p.small[i][2][2],
                    &p.colors[b],
                    &p.meta[b],
                );
                [sum[0], sum[1], sum[2], wsum]
            },
            4,
        );
        let velocity = worst_at(
            &gpu.render(&module, "mb_parity_velocity_fs", &small, &bulk),
            |i| {
                let v = p.small[i][0];
                let s = p.small[i][1];
                let (vel, pixels) =
                    super::mb_velocity([v[0], v[1]], [v[2], v[3]], s[0], s[1], [s[2], s[3]]);
                [vel[0], vel[1], pixels, 0.0]
            },
            &[0, 1],
        );
        let pixels = worst_at(
            &gpu.render(&module, "mb_parity_velocity_fs", &small, &bulk),
            |i| {
                let v = p.small[i][0];
                let s = p.small[i][1];
                let (_, px) =
                    super::mb_velocity([v[0], v[1]], [v[2], v[3]], s[0], s[1], [s[2], s[3]]);
                [0.0, 0.0, px, 0.0]
            },
            &[2],
        );

        let slack = |budget: f32, measured: f32| budget / measured.max(f32::MIN_POSITIVE);
        assert!(
            slack(WEIGHT_TOLERANCE, weight) <= 10.0,
            "weight budget {WEIGHT_TOLERANCE:e} is more than 10x the measured {weight:e}"
        );
        assert!(
            slack(ACCUMULATE_TOLERANCE, accumulate) <= 10.0,
            "accumulate budget {ACCUMULATE_TOLERANCE:e} is more than 10x the measured {accumulate:e}"
        );
        assert!(
            slack(VELOCITY_TOLERANCE, velocity) <= 10.0,
            "velocity budget {VELOCITY_TOLERANCE:e} is more than 10x the measured {velocity:e}"
        );
        assert!(
            slack(PIXELS_TOLERANCE, pixels) <= 10.0,
            "pixels budget {PIXELS_TOLERANCE:e} is more than 10x the measured {pixels:e}"
        );
    }

    /// Both pass shaders — bindings and all — must compile, not merely the
    /// arithmetic the harness exercises.
    #[test]
    fn both_pass_shaders_compile() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        [
            ("axiom-motionblur-tile", super::motion_blur_tile_source()),
            ("axiom-motionblur-blur", super::motion_blur_blur_source()),
        ]
        .iter()
        .for_each(|(label, source)| {
            let (_, failure) = crate::test_gpu::validating(&gpu.device, || {
                gpu.device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some(label),
                        source: wgpu::ShaderSource::Wgsl(source.clone().into()),
                    })
            });
            assert!(
                failure.is_none(),
                "{label} must compile: {}",
                failure
                    .as_ref()
                    .map_or(String::new(), std::string::ToString::to_string)
            );
        });
    }
}
