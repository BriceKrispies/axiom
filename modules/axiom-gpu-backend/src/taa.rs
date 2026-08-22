//! **Temporal antialiasing**: `src/render/taa.js`, as WGSL plus its CPU
//! reference.
//!
//! Ported from Claude-of-Duty `src/render/taa.js` (272 lines), which is a
//! variance-clipped, YCoCg, Catmull-Rom-resampled temporal resolve — not a
//! naive "lerp last frame in". The four things the source's own header names as
//! the reasons browser TAA smears, and which this port preserves exactly:
//!
//! 1. **Velocity must not contain the jitter.** [`crate::gbuffer`] already builds
//!    slot 1 from the *unjittered* view-projection pair, and the jitter this
//!    module hands out is applied only to the rasterisation matrix.
//! 2. **History is resampled with a 5-tap Catmull-Rom filter**, not bilinear —
//!    bilinear throws away the high frequencies the accumulation exists to
//!    build up. [`catmull_rom_plan`] / [`catmull_rom_combine`].
//! 3. **Geometry no velocity buffer can describe** (skinned limbs, morph
//!    targets) is tagged by the prepass with [`crate::gbuffer::COVERAGE_DYNAMIC`]
//!    (`0.7`), and [`taa_dynamic`] turns that tag into a tighter variance clip
//!    and a capped history tail. This is the coverage lane's whole purpose.
//! 4. **Rejection is a variance *clip* in YCoCg**, the velocity is dilated to the
//!    closest-depth neighbour, and the feedback drops with screen-space speed.
//!
//! Blending happens in a "reinhard weighted" tonemapped space
//! ([`tonemap_w`] / [`tonemap_w_inv`]) so one bright sample cannot bleed a
//! firefly across eight frames.
//!
//! # The one divergence from the source: `taa_v_sign`
//!
//! The source runs on WebGL, whose framebuffer `v` runs **up**, so a clip-space
//! `y` delta *is* a texture-space `v` delta. A WebGPU framebuffer's `v` runs
//! **down**, so it is not. [`crate::gbuffer::VELOCITY_TEXTURE_V_SIGN`] (`-1.0`)
//! is that one fact, and it is applied in exactly three places, all of them the
//! same conversion:
//!
//! - reading the velocity texture ([`taa_velocity`]);
//! - turning a `uv` into an NDC point, and
//! - turning a reprojected NDC point back into a `uv`
//!   (both in [`taa_background_velocity`]).
//!
//! Getting it wrong does not read as "the sign is wrong": it reads as *TAA
//! smears upward and never settles*, because the history feedback loop
//! integrates the error. It is pinned by
//! [`tests::the_shader_and_the_gbuffer_agree_on_the_v_sign`].
//!
//! The **jitter** deliberately gets no such flip. The resolve never learns the
//! jitter — velocity comes from unjittered matrices — so all the accumulation
//! requires is that the whole frame rasterises at one common sub-pixel offset.
//! Mirroring the sequence in `y` would change which sub-pixel positions are
//! visited, in what order, and nothing else; the source's order is kept.
//!
//! # Storage width is part of the algorithm
//!
//! Both history targets are the source's `hdrTarget(...)`, i.e.
//! `THREE.HalfFloatType` → `Rgba16Float`. The resolve **reads its own previous
//! output**, so every frame's result is rounded to `f16` before the next frame
//! consumes it. That is a feedback loop: a last-bit difference does not stay a
//! last-bit difference, it accumulates. [`taa_history_store`] is that rounding,
//! and any whole-chain reference must apply it between frames.
//!
//! # What this module deliberately does not own
//!
//! No `wgpu` resources, no pass struct, no ping-pong bookkeeping. Those belong
//! to the frame graph (the port of `render/index.js`), and what it must supply
//! is spelled out under [`TAA_WGSL`].

use crate::gbuffer::VELOCITY_TEXTURE_V_SIGN;

// ---------------------------------------------------------------------------
// The jitter sequence
// ---------------------------------------------------------------------------

/// How many entries the source's Halton table holds — `for (i = 1; i <= 16;)`.
///
/// The sequence *is* the anti-aliasing: it decides which sub-pixel positions the
/// accumulation ever visits, so its length, its bases and its offset are part of
/// the algorithm and not tuning.
pub(crate) const TAA_JITTER_LENGTH: u32 = 16;

/// Digits of the radical inverse [`halton`] evaluates.
///
/// The source's `while (i > 0)` runs until the index is exhausted; a fixed count
/// is the branchless form of the same loop, and it is **exact**, not an
/// approximation: once `i` reaches zero every further term is `f * 0.0 == 0.0`
/// and `r + 0.0 == r`. Thirty-two covers every `u32`, so the two forms agree for
/// any index, not merely for the sixteen the table uses.
const HALTON_DIGITS: u32 = 32;

/// The radical inverse of `index` in `base` — the source's
/// `h = (i, b) => { let f = 1, r = 0; while (i > 0) { f /= b; r += f * (i % b); i = Math.floor(i / b); } return r; }`.
///
/// Evaluated in `f64` because the source is JavaScript and every number in that
/// table is a `f64` all the way to the projection matrix. Narrowing early would
/// move the sub-pixel offsets, which moves the sample positions, which is the
/// filter.
pub(crate) fn halton(index: u32, base: u32) -> f64 {
    let b = f64::from(base);
    (0..HALTON_DIGITS)
        .fold((1.0_f64, 0.0_f64, index), |(f, r, i), _| {
            let f = f / b;
            (f, r + f * f64::from(i % base), i / base)
        })
        .1
}

/// The sub-pixel offset for frame `index`, **in pixels**, centred on zero.
///
/// `HALTON[i] = [h(i, 2) - 0.5, h(i, 3) - 0.5]` for `i` in `1..=16`, indexed by
/// `this.index % HALTON.length`. Frame 0 therefore takes `h(1, ·)`, which is why
/// the `+ 1` is here and not folded away.
pub(crate) fn taa_jitter(index: u32) -> [f64; 2] {
    let i = index % TAA_JITTER_LENGTH + 1;
    [halton(i, 2) - 0.5, halton(i, 3) - 0.5]
}

/// `projection` with frame `index`'s jitter folded into it — the source's
/// `_applyJitter`.
///
/// ```js
/// const jx = (j.x * 2) / this.screenSize.width;
/// const jy = (j.y * 2) / this.screenSize.height;
/// camera.projectionMatrix.elements[8] += jx;
/// camera.projectionMatrix.elements[9] += jy;
/// ```
///
/// `elements[8]`/`[9]` are the column-major slots `m[2][0]` and `m[2][1]`, the
/// same two slots in Axiom's column-major convention. Three's matrix elements
/// are `f64` and are narrowed once on upload, so the add happens in `f64` here
/// too: doing it in `f32` would round twice, and a jitter that lands on a
/// slightly different sub-pixel position is a slightly different filter.
///
/// **World camera only.** The source jitters `camera` and never `viewCamera` —
/// the viewmodel has its own MSAA target and no temporal history, so a jitter
/// there is a permanent sub-pixel wobble with nothing to resolve it out.
pub(crate) fn taa_jitter_projection(
    projection: &[f32; 16],
    index: u32,
    width: f64,
    height: f64,
) -> [f32; 16] {
    let j = taa_jitter(index);
    let jx = (j[0] * 2.0) / width;
    let jy = (j[1] * 2.0) / height;
    let mut out = *projection;
    out[8] = (f64::from(projection[8]) + jx) as f32;
    out[9] = (f64::from(projection[9]) + jy) as f32;
    out
}

// ---------------------------------------------------------------------------
// GLSL primitives, written out
// ---------------------------------------------------------------------------

/// GLSL `mix(x, y, a)` — `x * (1 - a) + y * a`, which is **not** `x + (y - x) * a`.
fn mix(x: f32, y: f32, a: f32) -> f32 {
    x * (1.0 - a) + y * a
}

/// GLSL `clamp(x, 0.0, 1.0)` — `min(max(x, 0), 1)`.
fn clamp01(x: f32) -> f32 {
    x.max(0.0).min(1.0)
}

/// GLSL `step(edge, x)` — `0.0` when `x < edge`, `1.0` otherwise.
fn step(edge: f32, x: f32) -> f32 {
    f32::from(x >= edge)
}

/// GLSL `smoothstep(e0, e1, x)`. Argument order is GLSL's (edges first), not
/// `MathUtils.smoothstep`'s.
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp01((x - e0) / (e1 - e0));
    t * t * (3.0 - 2.0 * t)
}

/// GLSL `length(vec2)` — the plain root of the plain sum of squares. **Not**
/// `Math.hypot`: this is GLSL, and no compensation happens in a shader.
fn length2(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

/// `owLum` — `dot(c, vec3(0.2126, 0.7152, 0.0722))`.
pub(crate) fn lum(c: [f32; 3]) -> f32 {
    c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
}

/// `tonemapW` — `c / (1.0 + owLum(c))`. The space the blend happens in, so a
/// single firefly cannot bleed across the history tail.
pub(crate) fn tonemap_w(c: [f32; 3]) -> [f32; 3] {
    let d = 1.0 + lum(c);
    [c[0] / d, c[1] / d, c[2] / d]
}

/// `tonemapWInv` — `c / max(1e-4, 1.0 - owLum(c))`. A division, as the source
/// writes it; the `max` is the source's and is what keeps the inverse finite as
/// the tonemapped luminance approaches one.
pub(crate) fn tonemap_w_inv(c: [f32; 3]) -> [f32; 3] {
    let d = 1e-4_f32.max(1.0 - lum(c));
    [c[0] / d, c[1] / d, c[2] / d]
}

/// `owRgbToYCoCg`. Clipping in YCoCg is chroma-aware, which is why a coloured
/// edge rejects properly here and does not in RGB.
pub(crate) fn rgb_to_ycocg(c: [f32; 3]) -> [f32; 3] {
    [
        0.25 * c[0] + 0.5 * c[1] + 0.25 * c[2],
        0.5 * c[0] - 0.5 * c[2],
        -0.25 * c[0] + 0.5 * c[1] - 0.25 * c[2],
    ]
}

/// `owYCoCgToRgb`.
pub(crate) fn ycocg_to_rgb(c: [f32; 3]) -> [f32; 3] {
    let t = c[0] - c[2];
    [t + c[1], c[0] + c[2], t - c[1]]
}

// ---------------------------------------------------------------------------
// History resampling
// ---------------------------------------------------------------------------

/// The four Catmull-Rom weights for a fractional texel position, as the source
/// writes them:
///
/// ```glsl
/// vec2 w0 = f * ( -0.5 + f * ( 1.0 - 0.5 * f ) );
/// vec2 w1 = 1.0 + f * f * ( -2.5 + 1.5 * f );
/// vec2 w2 = f * ( 0.5 + f * ( 2.0 - 1.5 * f ) );
/// vec2 w3 = f * f * ( -0.5 + 0.5 * f );
/// ```
///
/// The nesting is Horner's, and it is the specification: expanding it into a
/// polynomial in powers of `f` is algebraically equal and numerically different.
pub(crate) fn catmull_rom_weights(f: f32) -> [f32; 4] {
    [
        f * (-0.5 + f * (1.0 - 0.5 * f)),
        1.0 + f * f * (-2.5 + 1.5 * f),
        f * (0.5 + f * (2.0 - 1.5 * f)),
        f * f * (-0.5 + 0.5 * f),
    ]
}

/// The five bilinear tap positions and their weights for `sampleCatmullRom`.
///
/// Nine Catmull-Rom taps collapse to five hardware bilinear fetches by riding
/// the `w1 + w2` pair on one fetch at a fractional offset. The order of the five
/// is the source's `a, b, c, d, e`, and it is load-bearing: the accumulation
/// sums in that order and float addition is not associative.
///
/// Returns `(uv, weight)`. The **sampler must be linear-filtered and
/// clamp-to-edge** — the source's `hdrTarget` is, and the fractional `offset12`
/// tap is meaningless without it.
pub(crate) fn catmull_rom_plan(uv: [f32; 2], resolution: [f32; 2]) -> ([[f32; 2]; 5], [f32; 5]) {
    let sample_pos = [uv[0] * resolution[0], uv[1] * resolution[1]];
    let tex_pos1 = [
        (sample_pos[0] - 0.5).floor() + 0.5,
        (sample_pos[1] - 0.5).floor() + 0.5,
    ];
    let f = [sample_pos[0] - tex_pos1[0], sample_pos[1] - tex_pos1[1]];
    let wx = catmull_rom_weights(f[0]);
    let wy = catmull_rom_weights(f[1]);
    let w12 = [wx[1] + wx[2], wy[1] + wy[2]];
    let offset12 = [wx[2] / w12[0].max(1e-5), wy[2] / w12[1].max(1e-5)];
    let tex_pos0 = [
        (tex_pos1[0] - 1.0) / resolution[0],
        (tex_pos1[1] - 1.0) / resolution[1],
    ];
    let tex_pos3 = [
        (tex_pos1[0] + 2.0) / resolution[0],
        (tex_pos1[1] + 2.0) / resolution[1],
    ];
    let tex_pos12 = [
        (tex_pos1[0] + offset12[0]) / resolution[0],
        (tex_pos1[1] + offset12[1]) / resolution[1],
    ];
    (
        [
            [tex_pos12[0], tex_pos0[1]],
            [tex_pos0[0], tex_pos12[1]],
            [tex_pos12[0], tex_pos12[1]],
            [tex_pos3[0], tex_pos12[1]],
            [tex_pos12[0], tex_pos3[1]],
        ],
        [
            w12[0] * wy[0],
            wx[0] * w12[1],
            w12[0] * w12[1],
            wx[3] * w12[1],
            w12[0] * wy[3],
        ],
    )
}

/// The weighted sum of the five taps, normalised by `max(wsum, 1e-5)`.
///
/// Summed in tap order, which is why this takes the plan's arrays rather than a
/// set: a re-ordered sum is a different number.
pub(crate) fn catmull_rom_combine(taps: &[[f32; 3]; 5], weight: &[f32; 5]) -> [f32; 3] {
    let (result, wsum) = taps.iter().zip(weight.iter()).fold(
        ([0.0_f32; 3], 0.0_f32),
        |(r, s), (c, w)| {
            (
                [r[0] + c[0] * w, r[1] + c[1] * w, r[2] + c[2] * w],
                s + w,
            )
        },
    );
    let d = wsum.max(1e-5);
    [result[0] / d, result[1] / d, result[2] / d]
}

/// One store-and-load round trip through the `Rgba16Float` history target.
///
/// The resolve samples its own previous output, so this rounding sits **inside**
/// the feedback loop and a reference that skips it drifts further from the GPU
/// every frame rather than staying one ULP away. Borrowed from
/// [`crate::bloom_pyramid::half_storage`], whose own doc says to lift it the
/// moment a second pass needs it — this is that second pass, and the lift into
/// [`crate::hdr_target`] is reported rather than done here, because it moves a
/// file this slice does not own.
pub(crate) fn taa_history_store(rgb: [f32; 3]) -> [f32; 3] {
    rgb.map(crate::bloom_pyramid::half_storage::quantize)
}

// ---------------------------------------------------------------------------
// Velocity: dilation, the coverage tag, reprojection
// ---------------------------------------------------------------------------

/// The "this pixel deforms inside its own transform" weight, from the coverage
/// lane of [`crate::gbuffer::GBufferChannel::Normal`]:
///
/// ```glsl
/// float dynamic = max(
///   step( 0.5, ca ) * ( 1.0 - smoothstep( 0.72, 0.92, ca ) ),
///   step( 0.5, cb ) * ( 1.0 - smoothstep( 0.72, 0.92, cb ) ) );
/// ```
///
/// `ca` is the centre pixel's coverage, `cb` the dilated (closest-depth)
/// neighbour's. At [`crate::gbuffer::COVERAGE_DYNAMIC`] (`0.7`) this is exactly
/// `1.0` — `0.7` is below the smoothstep's lower edge — and at
/// [`crate::gbuffer::COVERAGE_STATIC`] (`1.0`) exactly `0.0`. The `step` against
/// `0.5` keeps the sky out: uncovered pixels are coverage `0`, which is "no
/// surface", not "a deforming surface".
///
/// Its two consumers are the clip gamma (`mix(1.0, 0.38, dynamic)` — a much
/// tighter variance clip) and the feedback cap (`min(feedback,
/// mix(1.0, 0.55, dynamic))` — a much shorter tail). Slightly noisier limbs, and
/// no background dragged through a silhouette.
pub(crate) fn taa_dynamic(ca: f32, cb: f32) -> f32 {
    let a = step(0.5, ca) * (1.0 - smoothstep(0.72, 0.92, ca));
    let b = step(0.5, cb) * (1.0 - smoothstep(0.72, 0.92, cb));
    a.max(b)
}

/// Dilate the velocity fetch to the closest-depth neighbour of the 3x3.
///
/// Each tap is `(uv.x, uv.y, coverage, depth)`; uncovered taps read as `1e8`,
/// which is the source's sentinel and is below the `1e9` seed, so the first tap
/// always wins initially and `bestUv` is always one of the nine. Strict `<`
/// keeps the earliest of equal depths, and the scan order is
/// `i % 3 - 1`, `i / 3 - 1`.
///
/// Returns `(best_uv, best_depth)`. Silhouettes take their own motion vector
/// rather than the background's, which is what stops an edge from shearing.
pub(crate) fn taa_dilate(centre_uv: [f32; 2], taps: &[[f32; 4]; 9]) -> ([f32; 2], f32) {
    taps.iter()
        .fold((centre_uv, 1e9_f32), |(best_uv, best_depth), tap| {
            let d = [1e8_f32, tap[3]][usize::from(tap[2] > 0.5)];
            let take = usize::from(d < best_depth);
            ([best_uv, [tap[0], tap[1]]][take], [best_depth, d][take])
        })
}

/// `m * v` for a column-major `mat4x4<f32>`, matching WGSL's operator.
fn mat4_mul_vec4(m: &[f32; 16], v: [f32; 4]) -> [f32; 4] {
    [0_usize, 1, 2, 3].map(|r| m[r] * v[0] + m[4 + r] * v[1] + m[8 + r] * v[2] + m[12 + r] * v[3])
}

/// Background velocity: reproject the far plane with the previous camera.
///
/// ```glsl
/// vec4 h = uInvVP * vec4( vUv * 2.0 - 1.0, 1.0, 1.0 );
/// vec3 wpos = h.xyz / h.w;
/// vec4 pc = uPrevVP * vec4( wpos, 1.0 );
/// vec2 prevUv = ( pc.xy / pc.w ) * 0.5 + 0.5;
/// vel = vUv - prevUv;
/// ```
///
/// Two applications of [`VELOCITY_TEXTURE_V_SIGN`], both the same fact: the
/// `uv → NDC` step and the `NDC → uv` step each cross the y-up/v-down boundary
/// the source never had to. `z = 1.0` is the far plane in **both** conventions
/// (WebGPU's NDC depth is `0..1`, WebGL's `-1..1`, and far is `1` in each), so
/// that literal needs no change — only the matrices must be the WebGPU-convention
/// pair the frame graph builds.
///
/// The result is already a texture-space delta, so it takes no further flip.
pub(crate) fn taa_background_velocity(
    uv: [f32; 2],
    inv_vp: &[f32; 16],
    prev_vp: &[f32; 16],
) -> [f32; 2] {
    let ndc_raw = [uv[0] * 2.0 - 1.0, uv[1] * 2.0 - 1.0];
    let h = mat4_mul_vec4(
        inv_vp,
        [ndc_raw[0], ndc_raw[1] * VELOCITY_TEXTURE_V_SIGN, 1.0, 1.0],
    );
    let wpos = [h[0] / h[3], h[1] / h[3], h[2] / h[3]];
    let pc = mat4_mul_vec4(prev_vp, [wpos[0], wpos[1], wpos[2], 1.0]);
    let p = [pc[0] / pc[3], pc[1] / pc[3]];
    let prev_uv = [
        p[0] * 0.5 + 0.5,
        p[1] * VELOCITY_TEXTURE_V_SIGN * 0.5 + 0.5,
    ];
    [uv[0] - prev_uv[0], uv[1] - prev_uv[1]]
}

/// Pick the velocity for this pixel: the stored one where there is a surface,
/// the reprojected background otherwise.
///
/// **The one place the velocity texture is read**, and therefore the one place
/// [`VELOCITY_TEXTURE_V_SIGN`] applies to it. The stored delta is half an NDC
/// delta in a clip space whose `y` runs up; `huv = uv - vel` needs it in
/// framebuffer space, whose `v` runs down.
pub(crate) fn taa_velocity(coverage: f32, stored: [f32; 2], background: [f32; 2]) -> [f32; 2] {
    let surface = [stored[0], stored[1] * VELOCITY_TEXTURE_V_SIGN];
    [background, surface][usize::from(coverage > 0.5)]
}

// ---------------------------------------------------------------------------
// The resolve
// ---------------------------------------------------------------------------

/// The `uParams` lanes, in the source's order:
/// `x` feedback, `y` clipGamma, `z` first-frame, `w` motionScale.
///
/// **`w` is dead in the source** — declared, set to `1`, commented "motionScale",
/// and never read by the shader. Ported anyway: dead computation in the source
/// is still part of the source, and a reader diffing the two files will look for
/// it.
pub(crate) const TAA_SOURCE_PARAMS: [f32; 4] = [0.92, 1.25, 1.0, 1.0];

/// Everything one resolve consumes, already sampled.
///
/// The split is deliberate: this is the **arithmetic**, with the sampler out of
/// the loop, so a parity proof measures the transcription rather than the
/// texture unit's subtexel precision.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TaaResolveIn {
    /// The 3x3 of `tCurrent`, in the source's `i` order
    /// (`x = i % 3 - 1`, `y = i / 3 - 1`).
    pub(crate) neighbourhood: [[f32; 3]; 9],
    /// `texture2D( tCurrent, vUv ).rgb`. The same texel as `neighbourhood[4]`;
    /// the source samples it twice and so does this.
    pub(crate) current: [f32; 3],
    /// The Catmull-Rom resampled history at `huv`, before the `max(_, 0)`.
    pub(crate) history_rgb: [f32; 3],
    /// The chosen velocity, already **texture-space** (see [`taa_velocity`]).
    pub(crate) velocity: [f32; 2],
    /// `vUv - vel`. Passed rather than derived so the border test is exactly the
    /// source's, on exactly the value the history was sampled at.
    pub(crate) huv: [f32; 2],
    /// [`taa_dynamic`]'s answer.
    pub(crate) dynamic: f32,
    /// `uResolution`, in pixels.
    pub(crate) resolution: [f32; 2],
    /// [`TAA_SOURCE_PARAMS`]-shaped.
    pub(crate) params: [f32; 4],
}

/// The resolve, as `taa.js`'s `main()` computes it once every input is sampled.
///
/// The first-frame lane (`uParams.z > 0.5`) is applied here as a select rather
/// than as the source's early `return`; the shader keeps the early return as
/// well, because it also skips the sampling, and the two agree by construction —
/// both answer `current`.
pub(crate) fn taa_resolve(input: &TaaResolveIn) -> [f32; 3] {
    let (m1, m2, nmin, nmax) = input.neighbourhood.iter().fold(
        ([0.0_f32; 3], [0.0_f32; 3], [1e9_f32; 3], [-1e9_f32; 3]),
        |(m1, m2, nmin, nmax), tap| {
            let c = rgb_to_ycocg(tonemap_w(*tap));
            (
                [m1[0] + c[0], m1[1] + c[1], m1[2] + c[2]],
                [m2[0] + c[0] * c[0], m2[1] + c[1] * c[1], m2[2] + c[2] * c[2]],
                [nmin[0].min(c[0]), nmin[1].min(c[1]), nmin[2].min(c[2])],
                [nmax[0].max(c[0]), nmax[1].max(c[1]), nmax[2].max(c[2])],
            )
        },
    );
    let mean = [m1[0] / 9.0, m1[1] / 9.0, m1[2] / 9.0];
    let sigma = [0_usize, 1, 2].map(|i| (m2[i] / 9.0 - mean[i] * mean[i]).max(0.0).sqrt());
    let gamma = input.params[1] * mix(1.0, 0.38, input.dynamic);
    let lo = [0_usize, 1, 2].map(|i| (mean[i] - gamma * sigma[i]).max(nmin[i]));
    let hi = [0_usize, 1, 2].map(|i| (mean[i] + gamma * sigma[i]).min(nmax[i]));

    let history = rgb_to_ycocg(tonemap_w(input.history_rgb.map(|c| c.max(0.0))));
    let centre = [0_usize, 1, 2].map(|i| 0.5 * (lo[i] + hi[i]));
    let extent = [0_usize, 1, 2].map(|i| 0.5 * (hi[i] - lo[i]) + 1e-5);
    let dir = [0_usize, 1, 2].map(|i| history[i] - centre[i]);
    let ts = [0_usize, 1, 2].map(|i| (extent[i] / dir[i].abs().max(1e-5)).abs());
    let clip_t = clamp01(ts[0].min(ts[1].min(ts[2])));
    let clipped = [0_usize, 1, 2].map(|i| centre[i] + dir[i] * clip_t);

    let outside = (input.huv[0] < 0.0)
        | (input.huv[0] > 1.0)
        | (input.huv[1] < 0.0)
        | (input.huv[1] > 1.0);
    let feedback = [input.params[0], 0.0][usize::from(outside)];
    let speed = length2([
        input.velocity[0] * input.resolution[0],
        input.velocity[1] * input.resolution[1],
    ]);
    let feedback = feedback * mix(1.0, 0.72, clamp01(speed / 24.0));
    let feedback = feedback * mix(0.82, 1.0, clip_t);
    let feedback = feedback.min(mix(1.0, 0.55, input.dynamic));

    let cur_y = rgb_to_ycocg(tonemap_w(input.current));
    let wc = 1.0 / (1.0 + cur_y[0]);
    let wh = 1.0 / (1.0 + clipped[0]);
    let sum = mix(wc, wh, feedback);
    let denom = sum.max(1e-5);
    let out_y = [0_usize, 1, 2]
        .map(|i| (cur_y[i] * wc * (1.0 - feedback) + clipped[i] * wh * feedback) / denom);
    let result = tonemap_w_inv(ycocg_to_rgb(out_y)).map(|c| c.max(0.0));
    [result, input.current][usize::from(input.params[2] > 0.5)]
}

// ---------------------------------------------------------------------------
// The uniform block
// ---------------------------------------------------------------------------

/// Floats in the resolve's uniform block: `inv_vp` (16) + `prev_vp` (16) +
/// `texel` (2) + `resolution` (2) + `params` (4).
///
/// No padding is needed: `texel` and `resolution` are `vec2` at byte offsets 128
/// and 136, and `params` lands at 144, already 16-byte aligned.
pub(crate) const TAA_UNIFORM_FLOATS: usize = 40;

/// Pack the resolve's uniform block. Both matrices are column-major, the
/// convention every matrix crossing this backend uses, and both are the
/// **unjittered** WebGPU-convention pair.
pub(crate) fn pack_taa_uniform(
    inv_vp: &[f32; 16],
    prev_vp: &[f32; 16],
    texel: [f32; 2],
    resolution: [f32; 2],
    params: [f32; 4],
) -> [f32; TAA_UNIFORM_FLOATS] {
    let mut out = [0.0_f32; TAA_UNIFORM_FLOATS];
    out[0..16].copy_from_slice(inv_vp);
    out[16..32].copy_from_slice(prev_vp);
    out[32..34].copy_from_slice(&texel);
    out[34..36].copy_from_slice(&resolution);
    out[36..40].copy_from_slice(&params);
    out
}

// ---------------------------------------------------------------------------
// WGSL
// ---------------------------------------------------------------------------

/// The pure arithmetic of the resolve, with **no bindings**: the half both the
/// real pass and the parity harness compile, so neither can drift from the
/// other.
///
/// Written from the GLSL text of `taa.js`'s `RESOLVE`, not from the Rust above.
/// `mix`, `clamp`, `step`, `smoothstep` and `length` are written out rather than
/// taken from WGSL's builtins, which are permitted to factor differently.
pub(crate) const TAA_WGSL_COMMON: &str = r#"
// The clip-y-up vs framebuffer-v-down sign. One fact; see
// gbuffer::VELOCITY_TEXTURE_V_SIGN, which is its home.
const taa_v_sign: f32 = -1.0;

fn taa_mix(x: f32, y: f32, a: f32) -> f32 { return x * (1.0 - a) + y * a; }
fn taa_clamp01(x: f32) -> f32 { return min(max(x, 0.0), 1.0); }
fn taa_step(edge: f32, x: f32) -> f32 { return select(0.0, 1.0, x >= edge); }

fn taa_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = taa_clamp01((x - e0) / (e1 - e0));
    return t * t * (3.0 - 2.0 * t);
}

fn taa_length2(v: vec2<f32>) -> f32 { return sqrt(v.x * v.x + v.y * v.y); }

fn taa_lum(c: vec3<f32>) -> f32 { return c.r * 0.2126 + c.g * 0.7152 + c.b * 0.0722; }
fn taa_tonemap_w(c: vec3<f32>) -> vec3<f32> { return c / (1.0 + taa_lum(c)); }
fn taa_tonemap_w_inv(c: vec3<f32>) -> vec3<f32> { return c / max(1e-4, 1.0 - taa_lum(c)); }

fn taa_rgb_to_ycocg(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
         0.25 * c.r + 0.5 * c.g + 0.25 * c.b,
         0.5  * c.r             - 0.5  * c.b,
        -0.25 * c.r + 0.5 * c.g - 0.25 * c.b);
}

fn taa_ycocg_to_rgb(c: vec3<f32>) -> vec3<f32> {
    let t = c.x - c.z;
    return vec3<f32>(t + c.y, c.x + c.z, t - c.y);
}

fn taa_catmull_rom_weights(f: f32) -> vec4<f32> {
    return vec4<f32>(
        f * (-0.5 + f * (1.0 - 0.5 * f)),
        1.0 + f * f * (-2.5 + 1.5 * f),
        f * (0.5 + f * (2.0 - 1.5 * f)),
        f * f * (-0.5 + 0.5 * f));
}

struct TaaCatmullPlan {
    uv: array<vec2<f32>, 5>,
    weight: array<f32, 5>,
};

fn taa_catmull_rom_plan(uv: vec2<f32>, resolution: vec2<f32>) -> TaaCatmullPlan {
    let sample_pos = uv * resolution;
    let tex_pos1 = floor(sample_pos - 0.5) + 0.5;
    let f = sample_pos - tex_pos1;

    let wx = taa_catmull_rom_weights(f.x);
    let wy = taa_catmull_rom_weights(f.y);
    let w12 = vec2<f32>(wx.y + wx.z, wy.y + wy.z);
    let offset12 = vec2<f32>(wx.z, wy.z) / max(w12, vec2<f32>(1e-5));

    let tex_pos0 = (tex_pos1 - 1.0) / resolution;
    let tex_pos3 = (tex_pos1 + 2.0) / resolution;
    let tex_pos12 = (tex_pos1 + offset12) / resolution;

    var plan: TaaCatmullPlan;
    plan.uv[0] = vec2<f32>(tex_pos12.x, tex_pos0.y);
    plan.uv[1] = vec2<f32>(tex_pos0.x, tex_pos12.y);
    plan.uv[2] = vec2<f32>(tex_pos12.x, tex_pos12.y);
    plan.uv[3] = vec2<f32>(tex_pos3.x, tex_pos12.y);
    plan.uv[4] = vec2<f32>(tex_pos12.x, tex_pos3.y);
    plan.weight[0] = w12.x * wy.x;
    plan.weight[1] = wx.x * w12.y;
    plan.weight[2] = w12.x * w12.y;
    plan.weight[3] = wx.w * w12.y;
    plan.weight[4] = w12.x * wy.w;
    return plan;
}

// A value-typed array parameter cannot be indexed by a runtime value in WGSL,
// so each of these copies into a function-scope `var` first. Same arithmetic,
// same order; the copy is what makes the loop legal.
fn taa_catmull_rom_combine(taps: array<vec3<f32>, 5>, weight: array<f32, 5>) -> vec3<f32> {
    var t = taps;
    var w = weight;
    var result = vec3<f32>(0.0);
    var wsum = 0.0;
    for (var i = 0; i < 5; i = i + 1) {
        result = result + t[i] * w[i];
        wsum = wsum + w[i];
    }
    return result / max(wsum, 1e-5);
}

fn taa_dynamic(ca: f32, cb: f32) -> f32 {
    return max(
        taa_step(0.5, ca) * (1.0 - taa_smoothstep(0.72, 0.92, ca)),
        taa_step(0.5, cb) * (1.0 - taa_smoothstep(0.72, 0.92, cb)));
}

// taps[i] = (uv.x, uv.y, coverage, depth). Returns (best_uv, best_depth).
fn taa_dilate(centre_uv: vec2<f32>, taps: array<vec4<f32>, 9>) -> vec3<f32> {
    var t = taps;
    var best_uv = centre_uv;
    var best_depth = 1e9;
    for (var i = 0; i < 9; i = i + 1) {
        var d = 1e8;
        if (t[i].z > 0.5) { d = t[i].w; }
        if (d < best_depth) { best_depth = d; best_uv = t[i].xy; }
    }
    return vec3<f32>(best_uv, best_depth);
}

fn taa_background_velocity(uv: vec2<f32>, inv_vp: mat4x4<f32>, prev_vp: mat4x4<f32>) -> vec2<f32> {
    let ndc_raw = uv * 2.0 - 1.0;
    let h = inv_vp * vec4<f32>(ndc_raw.x, ndc_raw.y * taa_v_sign, 1.0, 1.0);
    let wpos = h.xyz / h.w;
    let pc = prev_vp * vec4<f32>(wpos, 1.0);
    let p = pc.xy / pc.w;
    let prev_uv = vec2<f32>(p.x * 0.5 + 0.5, p.y * taa_v_sign * 0.5 + 0.5);
    return uv - prev_uv;
}

fn taa_velocity(coverage: f32, stored: vec2<f32>, background: vec2<f32>) -> vec2<f32> {
    return select(background, vec2<f32>(stored.x, stored.y * taa_v_sign), coverage > 0.5);
}

fn taa_resolve(
    neighbourhood: array<vec3<f32>, 9>,
    current: vec3<f32>,
    history_rgb: vec3<f32>,
    vel: vec2<f32>,
    huv: vec2<f32>,
    dynamic: f32,
    resolution: vec2<f32>,
    params: vec4<f32>,
) -> vec3<f32> {
    var nb = neighbourhood;
    var m1 = vec3<f32>(0.0);
    var m2 = vec3<f32>(0.0);
    var nmin = vec3<f32>(1e9);
    var nmax = vec3<f32>(-1e9);
    for (var i = 0; i < 9; i = i + 1) {
        let c = taa_rgb_to_ycocg(taa_tonemap_w(nb[i]));
        m1 = m1 + c;
        m2 = m2 + c * c;
        nmin = min(nmin, c);
        nmax = max(nmax, c);
    }
    let mean = m1 / 9.0;
    let sigma = sqrt(max(m2 / 9.0 - mean * mean, vec3<f32>(0.0)));
    let gamma = params.y * taa_mix(1.0, 0.38, dynamic);
    let lo = max(mean - gamma * sigma, nmin);
    let hi = min(mean + gamma * sigma, nmax);

    let hist = taa_rgb_to_ycocg(taa_tonemap_w(max(history_rgb, vec3<f32>(0.0))));

    // clip toward the neighbourhood centre rather than clamping per channel:
    // clamping kills sub-pixel detail, clipping keeps it.
    let centre = 0.5 * (lo + hi);
    let extent = 0.5 * (hi - lo) + 1e-5;
    let dir = hist - centre;
    let ts = abs(extent / max(abs(dir), vec3<f32>(1e-5)));
    let clip_t = taa_clamp01(min(ts.x, min(ts.y, ts.z)));
    let clipped = centre + dir * clip_t;

    var feedback = params.x;
    if (huv.x < 0.0 || huv.x > 1.0 || huv.y < 0.0 || huv.y > 1.0) { feedback = 0.0; }
    // fast motion -> trust the history less
    let speed = taa_length2(vel * resolution);
    feedback = feedback * taa_mix(1.0, 0.72, taa_clamp01(speed / 24.0));
    // heavy clipping means we were rejecting: shorten the tail
    feedback = feedback * taa_mix(0.82, 1.0, clip_t);
    // deforming geometry: cap the tail outright, no velocity describes it
    feedback = min(feedback, taa_mix(1.0, 0.55, dynamic));

    let cur_y = taa_rgb_to_ycocg(taa_tonemap_w(current));

    // luminance weighting (Karis) — suppresses the shimmer a plain lerp leaves
    // on specular highlights
    let wc = 1.0 / (1.0 + cur_y.x);
    let wh = 1.0 / (1.0 + clipped.x);
    let sum = taa_mix(wc, wh, feedback);
    let out_y = (cur_y * wc * (1.0 - feedback) + clipped * wh * feedback) / max(sum, 1e-5);

    let result = taa_tonemap_w_inv(taa_ycocg_to_rgb(out_y));
    return select(max(result, vec3<f32>(0.0)), current, params.z > 0.5);
}
"#;

/// The resolve pass itself: bindings, the full-screen triangle, and the fragment
/// entry point that samples and calls [`TAA_WGSL_COMMON`]'s `taa_resolve`.
///
/// # What the frame graph must supply
///
/// Concatenate with [`TAA_WGSL_COMMON`] (see [`taa_shader_source`]) and drive
/// with:
///
/// | binding | resource |
/// |---|---|
/// | 0 | uniform, [`pack_taa_uniform`] |
/// | 1 | `sampler` — **linear filter, clamp-to-edge**; the Catmull-Rom taps are meaningless otherwise |
/// | 2 | `tCurrent` — this frame's HDR colour |
/// | 3 | `tHistory` — the *other* history target |
/// | 4 | `tVelocity` — [`crate::gbuffer::GBufferChannel::Velocity`] |
/// | 5 | `tNormal` — [`crate::gbuffer::GBufferChannel::Normal`] (coverage in `z`) |
/// | 6 | `tDepth` — [`crate::gbuffer::GBufferChannel::Depth`] |
///
/// and, beyond the bindings:
///
/// - **the jittered projection** for the world camera's rasterisation, from
///   [`taa_jitter_projection`] with a frame counter that advances once per
///   resolved frame (the source's `this.index++` lives in `nextJitter`);
/// - **the unjittered `invVP` and the previous frame's `prevVP`**, which are
///   the same pair [`crate::gbuffer::pack_gbuffer_uniform`] already takes;
/// - **two `Rgba16Float` history targets, ping-ponged** — the pass reads one and
///   writes the other, and the written one becomes the resolved colour. The
///   *unwritten* one is the source's `previousTexture`, the correct SSR source;
/// - `params.z = 1.0` on the first frame after a resize or a camera cut, `0.0`
///   otherwise (the source's `_needsReset`).
///
/// Entry points: `taa_vs` and `taa_resolve_fs`. Target format `Rgba16Float`.
pub(crate) const TAA_WGSL: &str = r#"
struct TaaU {
    inv_vp: mat4x4<f32>,
    prev_vp: mat4x4<f32>,
    texel: vec2<f32>,
    resolution: vec2<f32>,
    // x feedback, y clipGamma, z first-frame, w motionScale (DEAD in the source)
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> taa_u: TaaU;
@group(0) @binding(1) var taa_samp: sampler;
@group(0) @binding(2) var taa_current: texture_2d<f32>;
@group(0) @binding(3) var taa_history: texture_2d<f32>;
@group(0) @binding(4) var taa_velocity_tex: texture_2d<f32>;
@group(0) @binding(5) var taa_normal: texture_2d<f32>;
@group(0) @binding(6) var taa_depth: texture_2d<f32>;

struct TaaVsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// The source's full-screen triangle: positions (-1,-1), (3,-1), (-1,3).
// Its `vUv = position.xy * 0.5 + 0.5` assumes WebGL's v-up framebuffer; here v
// runs down, so the v lane is mirrored. Same triangle, same texels.
@vertex
fn taa_vs(@builtin(vertex_index) index: u32) -> TaaVsOut {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let p = corners[index];
    var out: TaaVsOut;
    out.position = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return out;
}

// Every fetch is textureSampleLevel, never textureSample: the taps below sit
// inside non-uniform control flow, where WGSL forbids an implicit-derivative
// sample. The targets carry no mips, so level 0 is what texture2D read anyway.
@fragment
fn taa_resolve_fs(in: TaaVsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let current = textureSampleLevel(taa_current, taa_samp, uv, 0.0).rgb;

    if (taa_u.params.z > 0.5) { return vec4<f32>(current, 1.0); }

    // --- velocity, dilated to the closest-depth neighbour -------------------
    var taps: array<vec4<f32>, 9>;
    for (var i = 0; i < 9; i = i + 1) {
        let o = vec2<f32>(f32(i % 3) - 1.0, f32(i / 3) - 1.0) * taa_u.texel;
        let suv = uv + o;
        let n = textureSampleLevel(taa_normal, taa_samp, suv, 0.0);
        let d = textureSampleLevel(taa_depth, taa_samp, suv, 0.0).r;
        taps[i] = vec4<f32>(suv, n.z, d);
    }
    let dilated = taa_dilate(uv, taps);
    let best_uv = dilated.xy;

    let ca = textureSampleLevel(taa_normal, taa_samp, uv, 0.0).z;
    let nrm = textureSampleLevel(taa_normal, taa_samp, best_uv, 0.0);
    let dynamic = taa_dynamic(ca, nrm.z);

    let stored = textureSampleLevel(taa_velocity_tex, taa_samp, best_uv, 0.0).rg;
    let vel = taa_velocity(nrm.z, stored, taa_background_velocity(uv, taa_u.inv_vp, taa_u.prev_vp));
    let huv = uv - vel;

    // --- the 3x3 of tCurrent ------------------------------------------------
    var neighbourhood: array<vec3<f32>, 9>;
    for (var i = 0; i < 9; i = i + 1) {
        let o = vec2<f32>(f32(i % 3) - 1.0, f32(i / 3) - 1.0) * taa_u.texel;
        neighbourhood[i] = textureSampleLevel(taa_current, taa_samp, uv + o, 0.0).rgb;
    }

    // --- history, resampled with a 5-tap Catmull-Rom ------------------------
    var plan = taa_catmull_rom_plan(huv, taa_u.resolution);
    var history_taps: array<vec3<f32>, 5>;
    for (var i = 0; i < 5; i = i + 1) {
        history_taps[i] = textureSampleLevel(taa_history, taa_samp, plan.uv[i], 0.0).rgb;
    }
    let history_rgb = taa_catmull_rom_combine(history_taps, plan.weight);

    let resolved = taa_resolve(
        neighbourhood, current, history_rgb, vel, huv,
        dynamic, taa_u.resolution, taa_u.params);
    return vec4<f32>(resolved, 1.0);
}
"#;

/// The resolve's complete shader text — the common arithmetic followed by the
/// pass. The one source both the real pipeline and the parity harness build on.
pub(crate) fn taa_shader_source() -> String {
    [TAA_WGSL_COMMON, TAA_WGSL].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `-0.5`-centred Halton pair the source builds, spelled out for the
    /// first four frames from the definition rather than from [`halton`].
    #[test]
    fn the_jitter_sequence_is_halton_two_three_offset_by_a_half() {
        // h(1,2) = 1/2, h(2,2) = 1/4, h(3,2) = 3/4, h(4,2) = 1/8
        // h(1,3) = 1/3, h(2,3) = 2/3, h(3,3) = 1/9, h(4,3) = 4/9
        let expected = [
            [0.5 - 0.5, 1.0 / 3.0 - 0.5],
            [0.25 - 0.5, 2.0 / 3.0 - 0.5],
            [0.75 - 0.5, 1.0 / 9.0 - 0.5],
            [0.125 - 0.5, 4.0 / 9.0 - 0.5],
        ];
        let worst = (0..4)
            .map(|frame| {
                let j = taa_jitter(frame);
                let e = expected[frame as usize];
                (j[0] - e[0]).abs().max((j[1] - e[1]).abs())
            })
            .fold(0.0_f64, f64::max);
        assert!(
            worst < 1e-15,
            "the first four jitter offsets must be the Halton(2,3) pair minus a half; worst delta {worst:e}"
        );
    }

    #[test]
    fn the_jitter_sequence_wraps_at_sixteen_and_stays_inside_a_pixel() {
        assert_eq!(taa_jitter(0), taa_jitter(TAA_JITTER_LENGTH));
        assert_eq!(taa_jitter(7), taa_jitter(TAA_JITTER_LENGTH + 7));
        let worst = (0..TAA_JITTER_LENGTH)
            .flat_map(|i| taa_jitter(i))
            .fold(0.0_f64, |acc, v| acc.max(v.abs()));
        assert!(
            worst < 0.5,
            "every jitter offset must be strictly inside a half pixel; worst |offset| {worst}"
        );
    }

    #[test]
    fn the_fixed_digit_fold_agrees_with_the_source_while_loop() {
        // The source's loop, transcribed directly, for comparison.
        fn source_halton(mut i: u32, base: u32) -> f64 {
            let b = f64::from(base);
            let mut f = 1.0_f64;
            let mut r = 0.0_f64;
            let mut guard = 0;
            while i > 0 && guard < 64 {
                f /= b;
                r += f * f64::from(i % base);
                i /= base;
                guard += 1;
            }
            r
        }
        let worst = (0_u32..64)
            .flat_map(|i| [(i, 2_u32), (i, 3), (i, 5)])
            .map(|(i, b)| (halton(i, b) - source_halton(i, b)).abs())
            .fold(0.0_f64, f64::max);
        assert_eq!(
            worst, 0.0,
            "the branchless fold must be bit-equal to the source's while loop; worst delta {worst:e}"
        );
    }

    #[test]
    fn the_jitter_lands_only_on_the_two_projection_slots_the_source_touches() {
        let projection = [0_usize; 16].map(|_| 0.0_f32);
        let jittered = taa_jitter_projection(&projection, 2, 1920.0, 1080.0);
        let touched: Vec<usize> = (0..16).filter(|i| jittered[*i] != 0.0).collect();
        assert_eq!(
            touched,
            vec![8, 9],
            "only elements[8] and elements[9] carry the jitter"
        );
        let j = taa_jitter(2);
        assert_eq!(jittered[8], ((j[0] * 2.0) / 1920.0) as f32);
        assert_eq!(jittered[9], ((j[1] * 2.0) / 1080.0) as f32);
    }

    #[test]
    fn the_jitter_add_happens_in_f64_and_narrows_once() {
        // A projection slot large enough that an f32 add would swallow the
        // offset differently from an f64 one.
        let mut projection = [0.0_f32; 16];
        projection[8] = 1.0;
        let jittered = taa_jitter_projection(&projection, 1, 1920.0, 1080.0);
        let j = taa_jitter(1);
        let expected = (1.0_f64 + (j[0] * 2.0) / 1920.0) as f32;
        assert_eq!(
            jittered[8], expected,
            "the add must happen in f64 and narrow once"
        );
    }

    #[test]
    fn the_glsl_primitives_are_the_glsl_definitions() {
        assert_eq!(mix(2.0, 6.0, 0.25), 2.0 * 0.75 + 6.0 * 0.25);
        assert_eq!(clamp01(-3.0), 0.0);
        assert_eq!(clamp01(3.0), 1.0);
        assert_eq!(clamp01(0.4), 0.4);
        // GLSL step is >=, so a value exactly on the edge is 1.0.
        assert_eq!(step(0.5, 0.5), 1.0);
        assert_eq!(step(0.5, 0.499), 0.0);
        assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);
        assert_eq!(smoothstep(0.72, 0.92, 0.7), 0.0);
        assert_eq!(smoothstep(0.72, 0.92, 1.0), 1.0);
        assert_eq!(length2([3.0, 4.0]), 5.0);
    }

    #[test]
    fn the_tonemap_round_trips_and_ycocg_round_trips() {
        let colours = [[0.2_f32, 0.5, 0.9], [1.6, 0.1, 0.0], [0.0, 0.0, 0.0]];
        let worst = colours
            .iter()
            .map(|c| {
                let back = tonemap_w_inv(tonemap_w(*c));
                (0..3).map(|i| (back[i] - c[i]).abs()).fold(0.0_f32, f32::max)
            })
            .fold(0.0_f32, f32::max);
        assert!(
            worst < 1e-6,
            "tonemapWInv must invert tonemapW; worst delta {worst:e}"
        );
        let worst_ycocg = colours
            .iter()
            .map(|c| {
                let back = ycocg_to_rgb(rgb_to_ycocg(*c));
                (0..3).map(|i| (back[i] - c[i]).abs()).fold(0.0_f32, f32::max)
            })
            .fold(0.0_f32, f32::max);
        assert!(
            worst_ycocg < 1e-6,
            "owYCoCgToRgb must invert owRgbToYCoCg; worst delta {worst_ycocg:e}"
        );
        assert_eq!(lum([1.0, 0.0, 0.0]), 0.2126);
    }

    #[test]
    fn the_catmull_rom_weights_are_a_partition_of_unity() {
        let worst = (0..=20)
            .map(|i| {
                let f = i as f32 / 20.0;
                let w = catmull_rom_weights(f);
                (w[0] + w[1] + w[2] + w[3] - 1.0).abs()
            })
            .fold(0.0_f32, f32::max);
        assert!(
            worst < 2e-7,
            "the four Catmull-Rom weights must sum to one; worst deviation {worst:e}"
        );
        // At f = 0 the filter degenerates to the single centre tap.
        assert_eq!(catmull_rom_weights(0.0), [0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn the_catmull_rom_plan_is_five_taps_that_reproduce_a_flat_field() {
        let (uv, weight) = catmull_rom_plan([0.3125, 0.6875], [64.0, 64.0]);
        // **The five weights do NOT sum to one, and must not.** The collapse from
        // nine Catmull-Rom taps to five bilinear fetches keeps the centre row,
        // the centre column and their intersection — and drops the four corners.
        // The dropped mass is exactly `(wx0 + wx3) * (wy0 + wy3)`, which is why
        // `catmull_rom_combine` divides by the weight sum rather than trusting
        // it. Asserting the identity, not the constant, is what makes this a test
        // of the collapse rather than of a number.
        let frac = |uv: f32, res: f32| {
            let sample_pos = uv * res;
            sample_pos - ((sample_pos - 0.5).floor() + 0.5)
        };
        let wx = catmull_rom_weights(frac(0.3125, 64.0));
        let wy = catmull_rom_weights(frac(0.6875, 64.0));
        let corners = (wx[0] + wx[3]) * (wy[0] + wy[3]);
        assert!(
            (weight.iter().sum::<f32>() - (1.0 - corners)).abs() < 2e-6,
            "the five collapsed weights must sum to 1 - the dropped corners              ({}); sum {}",
            1.0 - corners,
            weight.iter().sum::<f32>()
        );
        // A constant history must come back unchanged.
        let flat = [[0.5_f32, 0.25, 0.125]; 5];
        let out = catmull_rom_combine(&flat, &weight);
        let worst = (0..3).map(|i| (out[i] - flat[0][i]).abs()).fold(0.0, f32::max);
        assert!(
            worst < 1e-6,
            "a flat history must resample to itself; worst delta {worst:e}"
        );
        // Every tap lands inside the texture for a mid-texture uv.
        let inside = uv.iter().all(|p| p[0] > 0.0 && p[0] < 1.0 && p[1] > 0.0 && p[1] < 1.0);
        assert!(inside, "the five taps must land inside the texture: {uv:?}");
    }

    #[test]
    fn the_catmull_rom_combine_falls_back_to_the_one_e_minus_five_floor() {
        let out = catmull_rom_combine(&[[1.0_f32, 1.0, 1.0]; 5], &[0.0; 5]);
        assert_eq!(
            out,
            [0.0, 0.0, 0.0],
            "an all-zero weight set divides by the 1e-5 floor, not by zero"
        );
    }

    #[test]
    fn the_history_store_rounds_to_half_precision() {
        // 1/3 is not representable in f16; the store must move it.
        let stored = taa_history_store([1.0 / 3.0, 1.0, 0.0]);
        assert_ne!(stored[0], 1.0 / 3.0, "f16 storage must round 1/3");
        assert!(
            (stored[0] - 1.0 / 3.0).abs() < 1e-3,
            "and must round to the nearest f16, not something else; got {}",
            stored[0]
        );
        assert_eq!(stored[1], 1.0);
        assert_eq!(stored[2], 0.0);
    }

    #[test]
    fn the_coverage_lane_selects_the_dynamic_rejection() {
        use crate::gbuffer::{COVERAGE_DYNAMIC, COVERAGE_STATIC};
        assert_eq!(
            taa_dynamic(COVERAGE_DYNAMIC, COVERAGE_DYNAMIC),
            1.0,
            "coverage 0.7 is fully dynamic — that is what the lane is for"
        );
        assert_eq!(
            taa_dynamic(COVERAGE_STATIC, COVERAGE_STATIC),
            0.0,
            "rigid geometry rejects nothing extra"
        );
        // The sky (coverage 0) is "no surface", not "a deforming surface".
        assert_eq!(taa_dynamic(0.0, 0.0), 0.0);
        // Either lane alone raises it: a dynamic neighbour is enough.
        assert_eq!(taa_dynamic(COVERAGE_STATIC, COVERAGE_DYNAMIC), 1.0);
        assert_eq!(taa_dynamic(COVERAGE_DYNAMIC, COVERAGE_STATIC), 1.0);
        // And the smoothstep really does ramp between the two edges.
        let mid = taa_dynamic(0.82, 0.0);
        assert!(
            mid > 0.0 && mid < 1.0,
            "coverage inside 0.72..0.92 must ramp; got {mid}"
        );
    }

    #[test]
    fn the_dilation_takes_the_closest_covered_neighbour_and_keeps_the_earliest_tie() {
        let mut taps = [[0.0_f32, 0.0, 1.0, 50.0]; 9];
        taps.iter_mut().enumerate().for_each(|(i, t)| {
            t[0] = i as f32;
            t[1] = 0.0;
        });
        taps[6][3] = 3.0;
        let (uv, depth) = taa_dilate([99.0, 99.0], &taps);
        assert_eq!((uv, depth), ([6.0, 0.0], 3.0));

        // An uncovered tap reads as the 1e8 sentinel, so it never wins.
        let mut only_far = [[0.0_f32, 0.0, 0.0, 1.0]; 9];
        only_far[4] = [4.0, 0.0, 1.0, 7.0];
        let (uv, depth) = taa_dilate([99.0, 99.0], &only_far);
        assert_eq!((uv, depth), ([4.0, 0.0], 7.0));

        // All uncovered: every tap is 1e8, the first one wins, and the centre
        // seed of 1e9 is never the answer.
        let all_far = [[5.0_f32, 6.0, 0.0, 1.0]; 9];
        let (uv, depth) = taa_dilate([99.0, 99.0], &all_far);
        assert_eq!((uv, depth), ([5.0, 6.0], 1e8));
    }

    #[test]
    fn the_velocity_flips_v_for_a_surface_and_leaves_the_background_alone() {
        let stored = [0.01_f32, 0.02];
        let background = [-0.03_f32, 0.04];
        assert_eq!(taa_velocity(1.0, stored, background), [0.01, -0.02]);
        assert_eq!(taa_velocity(0.7, stored, background), [0.01, -0.02]);
        assert_eq!(taa_velocity(0.0, stored, background), background);
    }

    /// A camera that has not moved must produce zero background velocity, and a
    /// camera that has must produce one whose `v` points the way the framebuffer
    /// does — the whole reason [`VELOCITY_TEXTURE_V_SIGN`] exists.
    #[test]
    fn the_background_reprojection_is_zero_for_a_static_camera() {
        // A WebGPU-convention perspective-ish VP and its inverse: use identity,
        // which is its own inverse, so wpos == ndc and prev_uv == uv.
        let identity = {
            let mut m = [0.0_f32; 16];
            [0_usize, 5, 10, 15].iter().for_each(|i| m[*i] = 1.0);
            m
        };
        let worst = [[0.1_f32, 0.2], [0.5, 0.5], [0.9, 0.75]]
            .iter()
            .map(|uv| {
                let v = taa_background_velocity(*uv, &identity, &identity);
                v[0].abs().max(v[1].abs())
            })
            .fold(0.0_f32, f32::max);
        assert!(
            worst < 1e-6,
            "a static camera reprojects onto itself; worst |velocity| {worst:e}"
        );
    }

    #[test]
    fn the_background_reprojection_carries_the_v_down_convention() {
        let identity = {
            let mut m = [0.0_f32; 16];
            [0_usize, 5, 10, 15].iter().for_each(|i| m[*i] = 1.0);
            m
        };
        // The previous VP translates NDC y by -0.2, so last frame this point sat
        // at NDC y = -0.2 — which, with v running DOWN, is v = 0.6, below the
        // centre. `huv = uv - vel` must therefore land at 0.6, so vel.y < 0.
        //
        // This is the assertion that fails if VELOCITY_TEXTURE_V_SIGN is dropped
        // from the reprojection: without it huv goes to 0.4 and the history is
        // fetched from the wrong side, which is what "TAA smears upward" is.
        let mut prev = identity;
        prev[13] = -0.2;
        let v = taa_background_velocity([0.5, 0.5], &identity, &prev);
        assert!(
            (v[1] + 0.1).abs() < 1e-6,
            "the v delta must be -0.1, so huv = 0.6 samples BELOW the centre; got {v:?}"
        );
        assert!(v[0].abs() < 1e-6, "and must not move u; got {v:?}");
    }

    #[test]
    fn the_matrix_product_is_column_major() {
        // A matrix whose fourth column is a translation: column-major puts it at
        // elements 12..16.
        let mut m = [0.0_f32; 16];
        [0_usize, 5, 10, 15].iter().for_each(|i| m[*i] = 1.0);
        m[12] = 3.0;
        m[13] = 4.0;
        m[14] = 5.0;
        assert_eq!(mat4_mul_vec4(&m, [1.0, 1.0, 1.0, 1.0]), [4.0, 5.0, 6.0, 1.0]);
    }

    /// A resolve input that exercises every lane: a textured neighbourhood, a
    /// history inside the clip box, real motion.
    fn probe(index: usize) -> TaaResolveIn {
        let base = index as f32 * 0.037;
        let neighbourhood = [0_usize, 1, 2, 3, 4, 5, 6, 7, 8].map(|i| {
            let t = base + i as f32 * 0.11;
            [0.20 + t * 0.5, 0.44 + t * 0.25, 0.66 + t * 0.125]
        });
        TaaResolveIn {
            neighbourhood,
            current: neighbourhood[4],
            history_rgb: [0.31 + base, 0.52 + base * 0.5, 0.71 + base * 0.25],
            velocity: [0.004 * (index as f32 + 1.0), -0.002 * index as f32],
            huv: [0.5 - 0.004 * (index as f32 + 1.0), 0.5],
            dynamic: (index % 3) as f32 * 0.5,
            resolution: [1920.0, 1080.0],
            params: TAA_SOURCE_PARAMS_RESOLVING,
        }
    }

    /// [`TAA_SOURCE_PARAMS`] with the first-frame lane cleared — every frame but
    /// the first.
    const TAA_SOURCE_PARAMS_RESOLVING: [f32; 4] = [0.92, 1.25, 0.0, 1.0];

    #[test]
    fn the_first_frame_lane_answers_the_current_colour_untouched() {
        let mut input = probe(3);
        input.params = TAA_SOURCE_PARAMS;
        assert_eq!(
            taa_resolve(&input),
            input.current,
            "uParams.z > 0.5 must short-circuit to the current colour"
        );
        assert_eq!(TAA_SOURCE_PARAMS[2], 1.0, "the source resets on frame one");
    }

    #[test]
    fn a_still_frame_with_matching_history_resolves_to_that_colour() {
        let flat = [0.4_f32, 0.6, 0.8];
        let input = TaaResolveIn {
            neighbourhood: [flat; 9],
            current: flat,
            history_rgb: flat,
            velocity: [0.0, 0.0],
            huv: [0.5, 0.5],
            dynamic: 0.0,
            resolution: [1920.0, 1080.0],
            params: TAA_SOURCE_PARAMS_RESOLVING,
        };
        let out = taa_resolve(&input);
        let worst = (0..3).map(|i| (out[i] - flat[i]).abs()).fold(0.0, f32::max);
        assert!(
            worst < 1e-5,
            "a converged still frame must be a fixed point; worst delta {worst:e}"
        );
    }

    #[test]
    fn history_outside_the_frame_is_dropped_entirely() {
        let mut input = probe(1);
        input.history_rgb = [8.0, 0.0, 0.0];
        let inside = taa_resolve(&input);
        input.huv = [-0.01, 0.5];
        let outside = taa_resolve(&input);
        let delta = (0..3).map(|i| (inside[i] - outside[i]).abs()).fold(0.0, f32::max);
        assert!(
            delta > 1e-4,
            "an off-screen huv must zero the feedback; delta was only {delta:e}"
        );
        let to_current = (0..3)
            .map(|i| (outside[i] - input.current[i]).abs())
            .fold(0.0, f32::max);
        assert!(
            to_current < 1e-5,
            "with zero feedback the answer is the current colour; delta {to_current:e}"
        );
    }

    #[test]
    fn a_dynamic_pixel_keeps_less_history_than_a_rigid_one() {
        let mut rigid = probe(2);
        rigid.dynamic = 0.0;
        rigid.history_rgb = [1.4, 0.2, 0.2];
        let mut dynamic = rigid;
        dynamic.dynamic = 1.0;
        let rigid_out = taa_resolve(&rigid);
        let dynamic_out = taa_resolve(&dynamic);
        let rigid_pull = (rigid_out[0] - rigid.current[0]).abs();
        let dynamic_pull = (dynamic_out[0] - dynamic.current[0]).abs();
        assert!(
            dynamic_pull < rigid_pull,
            "coverage-tagged geometry must keep a shorter tail; rigid {rigid_pull:e} vs dynamic {dynamic_pull:e}"
        );
    }

    #[test]
    fn fast_motion_shortens_the_tail() {
        // Both speeds are set here rather than taken from the fixture. `probe(4)`
        // already moves at 39 px/frame, which is **past** the 24 px saturation —
        // so comparing it against anything faster compares two clamped values and
        // proves nothing. That is what this test used to do.
        let mut slow = probe(4);
        // 1920 * 0.004 = 7.7 px/frame, a third of the way up the ramp.
        slow.velocity = [0.004, 0.0];
        let mut fast = slow;
        // 1920 * 0.05 = 96 px/frame — saturated, the other end of the ramp.
        fast.velocity = [0.05, 0.0];
        let slow_out = taa_resolve(&slow);
        let fast_out = taa_resolve(&fast);
        let slow_pull = (slow_out[0] - slow.current[0]).abs();
        let fast_pull = (fast_out[0] - fast.current[0]).abs();
        assert!(
            fast_pull < slow_pull,
            "the feedback must drop with screen-space speed; slow {slow_pull:e} vs fast {fast_pull:e}"
        );
    }

    #[test]
    fn an_out_of_gamut_history_is_clipped_into_the_neighbourhood_box() {
        let mut input = probe(5);
        input.history_rgb = [40.0, 0.0, 0.0];
        let out = taa_resolve(&input);
        assert!(
            out.iter().all(|c| *c >= 0.0 && *c < 4.0),
            "a wild history must be clipped toward the neighbourhood centre; got {out:?}"
        );
    }

    #[test]
    fn the_resolve_never_returns_a_negative_channel() {
        let mut input = probe(6);
        input.history_rgb = [-9.0, -9.0, -9.0];
        let out = taa_resolve(&input);
        assert!(
            out.iter().all(|c| *c >= 0.0),
            "the source's final max(_, 0) must hold; got {out:?}"
        );
    }

    #[test]
    fn the_resolve_input_is_debuggable_and_copyable() {
        let input = probe(0);
        let copy = input;
        assert_eq!(copy.dynamic, input.dynamic);
        assert_eq!(copy.resolution, input.resolution);
        assert!(format!("{input:?}").contains("TaaResolveIn"));
    }

    #[test]
    fn the_uniform_block_packs_in_the_order_the_wgsl_struct_declares() {
        let inv = [0_usize; 16].map(|_| 1.0_f32);
        let prev = [0_usize; 16].map(|_| 2.0_f32);
        let packed = pack_taa_uniform(&inv, &prev, [0.5, 0.25], [1920.0, 1080.0], TAA_SOURCE_PARAMS);
        assert_eq!(packed.len(), TAA_UNIFORM_FLOATS);
        assert_eq!(&packed[0..16], &inv);
        assert_eq!(&packed[16..32], &prev);
        assert_eq!(&packed[32..36], &[0.5, 0.25, 1920.0, 1080.0]);
        assert_eq!(&packed[36..40], &TAA_SOURCE_PARAMS);
    }

    #[test]
    fn the_shader_and_the_gbuffer_agree_on_the_v_sign() {
        assert_eq!(
            VELOCITY_TEXTURE_V_SIGN, -1.0,
            "the gbuffer's stated sign is the one this shader hard-codes"
        );
        assert!(
            TAA_WGSL_COMMON.contains("const taa_v_sign: f32 = -1.0;"),
            "the WGSL must declare the same sign the Rust applies"
        );
        // Three applications, and no more: the velocity read and the two NDC
        // conversions. A fourth would be double-flipping something.
        assert_eq!(
            TAA_WGSL_COMMON.matches("taa_v_sign").count(),
            4,
            "one declaration plus exactly three uses"
        );
    }

    #[test]
    fn the_shader_source_is_the_common_arithmetic_followed_by_the_pass() {
        let source = taa_shader_source();
        assert!(source.starts_with(TAA_WGSL_COMMON));
        assert!(source.ends_with(TAA_WGSL));
        assert!(
            source.contains("fn taa_resolve_fs"),
            "the pass entry point must be present"
        );
        assert!(
            source.contains("fn taa_vs"),
            "the full-screen triangle must be present"
        );
        // Never textureSample: the taps sit in non-uniform control flow.
        assert_eq!(
            source.matches("textureSample(").count(),
            0,
            "every fetch must be textureSampleLevel"
        );
    }

    #[test]
    fn the_dead_motion_scale_lane_is_carried_and_never_read() {
        assert_eq!(TAA_SOURCE_PARAMS[3], 1.0);
        assert!(
            TAA_WGSL.contains("motionScale (DEAD in the source)"),
            "the dead lane is named where it is declared"
        );
        // Nothing in the arithmetic reads params.w.
        assert_eq!(
            TAA_WGSL_COMMON.matches("params.w").count(),
            0,
            "uParams.w is declared and never read, exactly as in taa.js"
        );
    }
}

/// **CPU↔GPU parity for the temporal resolve**, on the crate's one shared
/// adapter.
///
/// The pattern is `surface_program::parity`'s and `bloom_pyramid::parity`'s:
/// every sampled input arrives through a uniform, so the texture unit is out of
/// the loop and what is measured is the *transcription*. Eight entry points, one
/// per piece of the resolve, because folding them into a single number would let
/// the loosest hide the rest.
///
/// **These tolerances are expectations, not measurements** — this slice was
/// written in a wave that forbids building, so nothing here has run. Each is
/// stated with the reasoning that produced it and must be replaced by a measured
/// figure on the first green run;
/// [`the_tolerances_are_within_ten_times_the_measured_delta`] prints the real
/// numbers in its assertion message.
#[cfg(all(test, feature = "offscreen"))]
mod parity {
    use super::*;

    /// Columns in one render; also the number of probe samples.
    const SAMPLES: usize = 16;

    /// `vec4` lanes of uniform per sample. Must match `HARNESS`'s unpack.
    const LANES: usize = 31;

    /// `copy_texture_to_buffer` wants each row aligned to this many bytes.
    const ROW_ALIGN: u32 = 256;

    /// **Measured: `2.38e-7`** on a native adapter — two `f32` ULP at unit
    /// magnitude, so the ~40-multiply-add chain does not compound at all and the
    /// `4e-6` estimate was 17x too generous.
    ///
    /// The resolve is ~40 multiply-adds, two reciprocals, a `sqrt` and a divide,
    /// on values of order 1.
    const RESOLVE_TOLERANCE: f32 = 5.0e-7;

    /// **Measured: `6.79e-6`** on a native adapter — the five-tap Catmull-Rom
    /// history resample, which used to share [`RESOLVE_TOLERANCE`] and measures
    /// **thirty times** looser than it. One budget could not serve both.
    ///
    /// The reason it is looser is [`PLAN_TOLERANCE`]: the resample multiplies
    /// each tap by a weight the plan produced, so it inherits the plan's
    /// divide-shaped error and then sums five of them before normalising.
    const RESAMPLE_TOLERANCE: f32 = 1.4e-5;

    /// **Expected, unverified.** Four Horner polynomials in one variable —
    /// shorter than the resolve by an order of magnitude, so a tighter budget.
    /// Outputs are order 1, where two `f32` ULP is `2.4e-7`.
    const WEIGHTS_TOLERANCE: f32 = 5.0e-7;

    /// **Measured: `3.81e-6`** on a native adapter — the divide really is where
    /// the adapter takes its freedom, and by more than the `1e-6` estimate.
    ///
    /// The plan divides three times by `resolution` and once by `w12`, and a
    /// division is where a driver is most likely to substitute a
    /// reciprocal-multiply. Outputs are `uv`s of order 1, so `3.81e-6` is ~32
    /// ULP — large for a `uv`, and it is a real property of the reciprocal, not
    /// a transcription error: the *positions* it produces still land on the same
    /// texel.
    const PLAN_TOLERANCE: f32 = 8.0e-6;

    /// **Measured: `0`** on a native adapter — bit-exact, and the estimate that
    /// stood here (`1e-5`, "the loosest tier") was the furthest wrong of the five.
    ///
    /// Two `mat4` products and two perspective divides. Matrix multiply *is* the
    /// one place a driver is most free to re-order `dot` against an `fma` chain —
    /// this adapter simply does not. Pinned at zero, like [`EXACT_TOLERANCE`]: if
    /// another adapter re-orders, that is worth failing on and reading, not worth
    /// pre-absorbing into a budget an order of magnitude wide.
    const VELOCITY_TOLERANCE: f32 = 0.0;

    /// **Expected, unverified.** Selection and comparison only — no arithmetic
    /// beyond what the caller already did.
    const EXACT_TOLERANCE: f32 = 0.0;

    /// One fragment entry point per thing being compared, each evaluating the
    /// sample its pixel column names.
    const HARNESS: &str = r#"
struct TaaParitySamples { items: array<vec4<f32>, 496> };
@group(0) @binding(0) var<uniform> taa_parity: TaaParitySamples;

struct TaaParityMatrices { inv_vp: mat4x4<f32>, prev_vp: mat4x4<f32> };
@group(0) @binding(1) var<uniform> taa_parity_m: TaaParityMatrices;

@vertex
fn taa_parity_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

fn taa_parity_base(sample: u32) -> u32 { return sample * 31u; }

fn taa_parity_neighbourhood(sample: u32) -> array<vec3<f32>, 9> {
    let base = taa_parity_base(sample);
    return array<vec3<f32>, 9>(
        taa_parity.items[base + 0u].xyz,
        taa_parity.items[base + 1u].xyz,
        taa_parity.items[base + 2u].xyz,
        taa_parity.items[base + 3u].xyz,
        taa_parity.items[base + 4u].xyz,
        taa_parity.items[base + 5u].xyz,
        taa_parity.items[base + 6u].xyz,
        taa_parity.items[base + 7u].xyz,
        taa_parity.items[base + 8u].xyz,
    );
}

fn taa_parity_dilation(sample: u32) -> array<vec4<f32>, 9> {
    let base = taa_parity_base(sample) + 16u;
    return array<vec4<f32>, 9>(
        taa_parity.items[base + 0u],
        taa_parity.items[base + 1u],
        taa_parity.items[base + 2u],
        taa_parity.items[base + 3u],
        taa_parity.items[base + 4u],
        taa_parity.items[base + 5u],
        taa_parity.items[base + 6u],
        taa_parity.items[base + 7u],
        taa_parity.items[base + 8u],
    );
}

@fragment
fn taa_parity_resolve_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let base = taa_parity_base(sample);
    let motion = taa_parity.items[base + 11u];
    let frame = taa_parity.items[base + 12u];
    return vec4<f32>(
        taa_resolve(
            taa_parity_neighbourhood(sample),
            taa_parity.items[base + 9u].xyz,
            taa_parity.items[base + 10u].xyz,
            motion.xy,
            motion.zw,
            frame.x,
            frame.yz,
            taa_parity.items[base + 13u]),
        0.0);
}

@fragment
fn taa_parity_dynamic_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let cov = taa_parity.items[taa_parity_base(sample) + 14u];
    return vec4<f32>(
        taa_dynamic(cov.x, cov.y),
        taa_step(0.5, cov.x),
        taa_smoothstep(0.72, 0.92, cov.x),
        taa_mix(1.0, 0.38, cov.z));
}

@fragment
fn taa_parity_weights_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    return taa_catmull_rom_weights(taa_parity.items[taa_parity_base(sample) + 12u].w);
}

// Column c carries tap (c % 5) of sample (c / 5): the whole plan, uv and weight
// together, so a transposed tap cannot survive.
@fragment
fn taa_parity_plan_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let column = u32(position.x);
    let sample = column / 5u;
    let tap = column % 5u;
    let lane = taa_parity.items[taa_parity_base(sample) + 25u];
    var plan = taa_catmull_rom_plan(lane.xy, lane.zw);
    return vec4<f32>(plan.uv[tap], plan.weight[tap], 0.0);
}

@fragment
fn taa_parity_history_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let base = taa_parity_base(sample);
    let lane = taa_parity.items[base + 25u];
    let plan = taa_catmull_rom_plan(lane.xy, lane.zw);
    let taps = array<vec3<f32>, 5>(
        taa_parity.items[base + 26u].xyz,
        taa_parity.items[base + 27u].xyz,
        taa_parity.items[base + 28u].xyz,
        taa_parity.items[base + 29u].xyz,
        taa_parity.items[base + 30u].xyz,
    );
    return vec4<f32>(taa_catmull_rom_combine(taps, plan.weight), 0.0);
}

@fragment
fn taa_parity_dilate_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let centre = taa_parity.items[taa_parity_base(sample) + 15u].zw;
    return vec4<f32>(taa_dilate(centre, taa_parity_dilation(sample)), 0.0);
}

@fragment
fn taa_parity_velocity_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let base = taa_parity_base(sample);
    let lane = taa_parity.items[base + 15u];
    let cov = taa_parity.items[base + 14u];
    let background = taa_background_velocity(lane.zw, taa_parity_m.inv_vp, taa_parity_m.prev_vp);
    return vec4<f32>(taa_velocity(cov.z, lane.xy, background), background);
}

// Declared before its caller: WGSL resolves in source order.
fn taa_parity_ycocg(c: vec3<f32>) -> vec3<f32> {
    return taa_tonemap_w_inv(taa_ycocg_to_rgb(taa_rgb_to_ycocg(taa_tonemap_w(c))));
}

@fragment
fn taa_parity_tonemap_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let sample = u32(position.x);
    let c = taa_parity.items[taa_parity_base(sample) + 9u].xyz;
    return vec4<f32>(taa_parity_ycocg(c), taa_lum(c));
}
"#;

    /// The crate's one instance + adapter + device, renamed into this module's
    /// vocabulary. Never a `wgpu::Instance` of its own; see [`crate::test_gpu`].
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
                        label: Some("axiom-taa-parity-shader"),
                        source: wgpu::ShaderSource::Wgsl([TAA_WGSL_COMMON, HARNESS].concat().into()),
                    })
            });
            assert!(
                failure.is_none(),
                "the TAA WGSL must compile: {}",
                failure.map_or(String::new(), |error| error.to_string())
            );
            module
        }

        /// Render `entry_point` over a `SAMPLES x 1` `Rgba32Float` target.
        fn render(
            &self,
            module: &wgpu::ShaderModule,
            entry_point: &str,
            samples: &[u8],
            matrices: &[u8],
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
                    label: Some("axiom-taa-parity-bgl"),
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
            let samples_buffer = make("axiom-taa-parity-samples", samples);
            let matrices_buffer = make("axiom-taa-parity-matrices", matrices);
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-taa-parity-bg"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: samples_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: matrices_buffer.as_entire_binding(),
                    },
                ],
            });
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("axiom-taa-parity-pl"),
                        bind_group_layouts: &[&layout],
                        push_constant_ranges: &[],
                    });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("axiom-taa-parity-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("taa_parity_vs"),
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
                label: Some("axiom-taa-parity-target"),
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
                label: Some("axiom-taa-parity-readback"),
                size: u64::from(row_bytes),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("axiom-taa-parity-pass"),
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

    /// The sixteen probes, and the matrices they reproject with.
    struct Probes {
        resolve: Vec<super::TaaResolveIn>,
        coverage: Vec<[f32; 4]>,
        catmull_f: Vec<f32>,
        catmull_uv: Vec<[f32; 4]>,
        catmull_taps: Vec<[[f32; 3]; 5]>,
        dilation: Vec<[[f32; 4]; 9]>,
        velocity_lane: Vec<[f32; 4]>,
        inv_vp: [f32; 16],
        prev_vp: [f32; 16],
    }

    fn probes() -> Probes {
        let coverages = [0.0_f32, 0.7, 1.0, 0.82];
        let resolve = (0..SAMPLES)
            .map(|s| {
                let base = s as f32 * 0.0371;
                let neighbourhood = [0_usize, 1, 2, 3, 4, 5, 6, 7, 8].map(|i| {
                    let t = base + i as f32 * 0.0917;
                    [0.17 + t * 0.61, 0.39 + t * 0.29, 0.58 + t * 0.13]
                });
                super::TaaResolveIn {
                    neighbourhood,
                    current: neighbourhood[4],
                    history_rgb: [0.29 + base, 0.47 + base * 0.5, 0.63 + base * 0.25],
                    velocity: [0.0031 * (s as f32 + 1.0), -0.0017 * s as f32],
                    huv: [0.5 - 0.0031 * (s as f32 + 1.0), 0.5 + 0.0017 * s as f32],
                    dynamic: (s % 4) as f32 / 3.0,
                    resolution: [1920.0, 1080.0],
                    params: [0.92, 1.25, 0.0, 1.0],
                }
            })
            .collect();
        let coverage = (0..SAMPLES)
            .map(|s| {
                [
                    coverages[s % 4],
                    coverages[(s + 1) % 4],
                    coverages[(s + 2) % 4],
                    0.0,
                ]
            })
            .collect();
        let catmull_f = (0..SAMPLES).map(|s| s as f32 / (SAMPLES as f32 - 1.0)).collect();
        let catmull_uv = (0..SAMPLES)
            .map(|s| [0.13 + s as f32 * 0.047, 0.29 + s as f32 * 0.031, 1920.0, 1080.0])
            .collect();
        let catmull_taps = (0..SAMPLES)
            .map(|s| {
                [0_usize, 1, 2, 3, 4].map(|i| {
                    let t = s as f32 * 0.07 + i as f32 * 0.23;
                    [0.2 + t, 0.5 + t * 0.5, 0.8 + t * 0.25]
                })
            })
            .collect();
        let dilation = (0..SAMPLES)
            .map(|s| {
                [0_usize, 1, 2, 3, 4, 5, 6, 7, 8].map(|i| {
                    [
                        0.1 + i as f32 * 0.01,
                        0.2 + i as f32 * 0.02,
                        coverages[(s + i) % 4],
                        3.0 + ((s * 9 + i) % 7) as f32,
                    ]
                })
            })
            .collect();
        let velocity_lane = (0..SAMPLES)
            .map(|s| {
                [
                    0.004 * (s as f32 - 8.0),
                    0.003 * (s as f32 - 4.0),
                    0.05 + s as f32 * 0.055,
                    0.07 + s as f32 * 0.051,
                ]
            })
            .collect();
        // A pair of plausible view-projections: the current one identity-ish,
        // the previous one translated and slightly scaled.
        let mut inv_vp = [0.0_f32; 16];
        [0_usize, 5, 10, 15].iter().for_each(|i| inv_vp[*i] = 1.0);
        inv_vp[0] = 1.6;
        inv_vp[5] = 0.9;
        inv_vp[12] = 0.3;
        inv_vp[13] = -0.2;
        let mut prev_vp = [0.0_f32; 16];
        [0_usize, 5, 10, 15].iter().for_each(|i| prev_vp[*i] = 1.0);
        prev_vp[0] = 0.62;
        prev_vp[5] = 1.11;
        prev_vp[12] = -0.17;
        prev_vp[13] = 0.09;
        prev_vp[14] = 0.02;
        Probes {
            resolve,
            coverage,
            catmull_f,
            catmull_uv,
            catmull_taps,
            dilation,
            velocity_lane,
            inv_vp,
            prev_vp,
        }
    }

    /// Lay the probes out in the `LANES`-per-sample block the harness unpacks.
    fn uniform(p: &Probes) -> Vec<u8> {
        let mut lanes = vec![[0.0_f32; 4]; SAMPLES * LANES];
        (0..SAMPLES).for_each(|s| {
            let base = s * LANES;
            let r = &p.resolve[s];
            (0..9).for_each(|i| {
                lanes[base + i] = [
                    r.neighbourhood[i][0],
                    r.neighbourhood[i][1],
                    r.neighbourhood[i][2],
                    0.0,
                ];
            });
            lanes[base + 9] = [r.current[0], r.current[1], r.current[2], 0.0];
            lanes[base + 10] = [r.history_rgb[0], r.history_rgb[1], r.history_rgb[2], 0.0];
            lanes[base + 11] = [r.velocity[0], r.velocity[1], r.huv[0], r.huv[1]];
            lanes[base + 12] = [r.dynamic, r.resolution[0], r.resolution[1], p.catmull_f[s]];
            lanes[base + 13] = r.params;
            lanes[base + 14] = p.coverage[s];
            lanes[base + 15] = p.velocity_lane[s];
            (0..9).for_each(|i| lanes[base + 16 + i] = p.dilation[s][i]);
            lanes[base + 25] = p.catmull_uv[s];
            (0..5).for_each(|i| {
                lanes[base + 26 + i] = [
                    p.catmull_taps[s][i][0],
                    p.catmull_taps[s][i][1],
                    p.catmull_taps[s][i][2],
                    0.0,
                ];
            });
        });
        lanes
            .iter()
            .flat_map(|lane| lane.iter().flat_map(|v| v.to_le_bytes()))
            .collect()
    }

    fn matrices(p: &Probes) -> Vec<u8> {
        p.inv_vp
            .iter()
            .chain(p.prev_vp.iter())
            .flat_map(|v| v.to_le_bytes())
            .collect()
    }

    /// The worst absolute disagreement between `gpu[i][lane]` and `cpu(i)[lane]`.
    fn worst(gpu: &[[f32; 4]], cpu: impl Fn(usize) -> [f32; 4], lanes: usize) -> f32 {
        gpu.iter()
            .enumerate()
            .flat_map(|(i, got)| {
                let want = cpu(i);
                (0..lanes)
                    .map(|l| (got[l] - want[l]).abs())
                    .collect::<Vec<f32>>()
            })
            .fold(0.0_f32, f32::max)
    }

    /// A real adapter, or a loud failure. `Noop` proves nothing.
    fn require_real(gpu: &Gpu) {
        assert_ne!(
            gpu.backend,
            wgpu::Backend::Noop,
            "a parity proof needs a real adapter"
        );
    }

    #[test]
    fn the_resolve_agrees_with_the_cpu_reference() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let got = gpu.render(&module, "taa_parity_resolve_fs", &uniform(&p), &matrices(&p));
        let delta = worst(
            &got,
            |i| {
                let c = super::taa_resolve(&p.resolve[i]);
                [c[0], c[1], c[2], 0.0]
            },
            3,
        );
        assert!(
            delta <= RESOLVE_TOLERANCE,
            "the resolve must agree with its CPU reference; worst delta {delta:e} vs budget {RESOLVE_TOLERANCE:e}"
        );
    }

    #[test]
    fn the_coverage_rejection_agrees_with_the_cpu_reference() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let got = gpu.render(&module, "taa_parity_dynamic_fs", &uniform(&p), &matrices(&p));
        let delta = worst(
            &got,
            |i| {
                let cov = p.coverage[i];
                [
                    super::taa_dynamic(cov[0], cov[1]),
                    super::step(0.5, cov[0]),
                    super::smoothstep(0.72, 0.92, cov[0]),
                    super::mix(1.0, 0.38, cov[2]),
                ]
            },
            4,
        );
        assert!(
            delta <= WEIGHTS_TOLERANCE,
            "the coverage rejection must agree with its CPU reference; worst delta {delta:e} vs budget {WEIGHTS_TOLERANCE:e}"
        );
    }

    #[test]
    fn the_catmull_rom_weights_agree_with_the_cpu_reference() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let got = gpu.render(&module, "taa_parity_weights_fs", &uniform(&p), &matrices(&p));
        let delta = worst(&got, |i| super::catmull_rom_weights(p.catmull_f[i]), 4);
        assert!(
            delta <= WEIGHTS_TOLERANCE,
            "the Catmull-Rom weights must agree with their CPU reference; worst delta {delta:e} vs budget {WEIGHTS_TOLERANCE:e}"
        );
    }

    #[test]
    fn the_catmull_rom_plan_agrees_tap_for_tap() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let got = gpu.render(&module, "taa_parity_plan_fs", &uniform(&p), &matrices(&p));
        let delta = worst(
            &got,
            |column| {
                let lane = p.catmull_uv[column / 5];
                let (uv, weight) = super::catmull_rom_plan([lane[0], lane[1]], [lane[2], lane[3]]);
                let tap = column % 5;
                [uv[tap][0], uv[tap][1], weight[tap], 0.0]
            },
            4,
        );
        assert!(
            delta <= PLAN_TOLERANCE,
            "every Catmull-Rom tap position and weight must agree; worst delta {delta:e} vs budget {PLAN_TOLERANCE:e}"
        );
    }

    #[test]
    fn the_history_resample_agrees_with_the_cpu_reference() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let got = gpu.render(&module, "taa_parity_history_fs", &uniform(&p), &matrices(&p));
        let delta = worst(
            &got,
            |i| {
                let lane = p.catmull_uv[i];
                let (_, weight) = super::catmull_rom_plan([lane[0], lane[1]], [lane[2], lane[3]]);
                let c = super::catmull_rom_combine(&p.catmull_taps[i], &weight);
                [c[0], c[1], c[2], 0.0]
            },
            3,
        );
        assert!(
            delta <= RESAMPLE_TOLERANCE,
            "the five-tap history resample must agree; worst delta {delta:e} vs budget {RESAMPLE_TOLERANCE:e}"
        );
    }

    #[test]
    fn the_velocity_dilation_is_bit_exact() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let got = gpu.render(&module, "taa_parity_dilate_fs", &uniform(&p), &matrices(&p));
        let delta = worst(
            &got,
            |i| {
                let lane = p.velocity_lane[i];
                let (uv, depth) = super::taa_dilate([lane[2], lane[3]], &p.dilation[i]);
                [uv[0], uv[1], depth, 0.0]
            },
            3,
        );
        assert_eq!(
            delta, EXACT_TOLERANCE,
            "dilation only selects, so it must be bit-exact; worst delta {delta:e}"
        );
    }

    #[test]
    fn the_velocity_selection_and_background_reprojection_agree() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let got = gpu.render(&module, "taa_parity_velocity_fs", &uniform(&p), &matrices(&p));
        let delta = worst(
            &got,
            |i| {
                let lane = p.velocity_lane[i];
                let background =
                    super::taa_background_velocity([lane[2], lane[3]], &p.inv_vp, &p.prev_vp);
                let vel = super::taa_velocity(p.coverage[i][2], [lane[0], lane[1]], background);
                [vel[0], vel[1], background[0], background[1]]
            },
            4,
        );
        assert!(
            delta <= VELOCITY_TOLERANCE,
            "the velocity selection and reprojection must agree; worst delta {delta:e} vs budget {VELOCITY_TOLERANCE:e}"
        );
    }

    #[test]
    fn the_tonemap_and_ycocg_round_trip_agrees() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let got = gpu.render(&module, "taa_parity_tonemap_fs", &uniform(&p), &matrices(&p));
        let delta = worst(
            &got,
            |i| {
                let c = p.resolve[i].current;
                let r = super::tonemap_w_inv(super::ycocg_to_rgb(super::rgb_to_ycocg(
                    super::tonemap_w(c),
                )));
                [r[0], r[1], r[2], super::lum(c)]
            },
            4,
        );
        assert!(
            delta <= RESOLVE_TOLERANCE,
            "the tonemap/YCoCg round trip must agree; worst delta {delta:e} vs budget {RESOLVE_TOLERANCE:e}"
        );
    }

    /// **The calibration test.** Re-measures every tier and fails if a budget has
    /// drifted more than 10x looser than the hardware needs — the brief's rule,
    /// asserted so the justification cannot rot.
    ///
    /// It is also how the expectations above become measurements: on the first
    /// green run its message carries the real numbers.
    #[test]
    fn the_tolerances_are_within_ten_times_the_measured_delta() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let p = probes();
        let module = gpu.compile();
        let samples = uniform(&p);
        let m = matrices(&p);

        let resolve = worst(
            &gpu.render(&module, "taa_parity_resolve_fs", &samples, &m),
            |i| {
                let c = super::taa_resolve(&p.resolve[i]);
                [c[0], c[1], c[2], 0.0]
            },
            3,
        );
        let weights = worst(
            &gpu.render(&module, "taa_parity_weights_fs", &samples, &m),
            |i| super::catmull_rom_weights(p.catmull_f[i]),
            4,
        );
        let plan = worst(
            &gpu.render(&module, "taa_parity_plan_fs", &samples, &m),
            |column| {
                let lane = p.catmull_uv[column / 5];
                let (uv, weight) = super::catmull_rom_plan([lane[0], lane[1]], [lane[2], lane[3]]);
                let tap = column % 5;
                [uv[tap][0], uv[tap][1], weight[tap], 0.0]
            },
            4,
        );
        let velocity = worst(
            &gpu.render(&module, "taa_parity_velocity_fs", &samples, &m),
            |i| {
                let lane = p.velocity_lane[i];
                let background =
                    super::taa_background_velocity([lane[2], lane[3]], &p.inv_vp, &p.prev_vp);
                let vel = super::taa_velocity(p.coverage[i][2], [lane[0], lane[1]], background);
                [vel[0], vel[1], background[0], background[1]]
            },
            4,
        );

        let slack = |budget: f32, measured: f32| budget / measured.max(f32::MIN_POSITIVE);
        assert!(
            slack(RESOLVE_TOLERANCE, resolve) <= 10.0,
            "resolve budget {RESOLVE_TOLERANCE:e} is more than 10x the measured {resolve:e}"
        );
        assert!(
            slack(WEIGHTS_TOLERANCE, weights) <= 10.0,
            "weights budget {WEIGHTS_TOLERANCE:e} is more than 10x the measured {weights:e}"
        );
        assert!(
            slack(PLAN_TOLERANCE, plan) <= 10.0,
            "plan budget {PLAN_TOLERANCE:e} is more than 10x the measured {plan:e}"
        );
        assert!(
            slack(VELOCITY_TOLERANCE, velocity) <= 10.0,
            "velocity budget {VELOCITY_TOLERANCE:e} is more than 10x the measured {velocity:e}"
        );
    }

    /// The pass shader — bindings and all — must compile, not merely the
    /// arithmetic the harness exercises.
    #[test]
    fn the_resolve_pass_shader_compiles() {
        let gpu = Gpu::shared();
        require_real(&gpu);
        let (_, failure) = crate::test_gpu::validating(&gpu.device, || {
            gpu.device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("axiom-taa-pass-shader"),
                    source: wgpu::ShaderSource::Wgsl(super::taa_shader_source().into()),
                })
        });
        assert!(
            failure.is_none(),
            "the TAA pass shader must compile: {}",
            failure.map_or(String::new(), |error| error.to_string())
        );
    }
}
