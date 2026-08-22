//! **Screen-space reflections**: a bounded ray march in view space against the
//! G-buffer's linear depth, binary-refined, then reprojected into the *previous*
//! resolved frame through the velocity buffer.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/render/ssr.js` (197 lines) — both of
//! its shaders (the march and the separable blur that follows it) — plus the two
//! lines of `src/render/materialpatch.js` that *consume* the result, because the
//! roughness cutoff is part of this pass's contract and not of the material's.
//!
//! # What the pass is
//!
//! For each pixel that the G-buffer says is covered, reflect the view vector
//! about the view normal and march that ray. The step is **geometric**, not
//! linear: 28 steps whose scale is chosen so the last one lands exactly on
//! `maxDistance`, which spends resolution where a reflection is sharp (near the
//! surface) instead of spreading it evenly over 24 metres. On the first step
//! whose depth difference falls inside the thickness window, five bisections
//! refine the crossing, and the refined position is projected back to a UV.
//!
//! That UV is then **reprojected through the velocity buffer into last frame's
//! resolved colour**, so the reflected colour is already tone-stable and
//! antialiased — the TAA history is the source — and camera motion costs nothing.
//! Only lighting changes lag.
//!
//! The result is blended into the material's IBL *specular* term
//! ([`ssr_resolve`]) rather than added on top of the frame, so a wet road
//! **replaces** its cubemap reflection instead of doubling it.
//!
//! # The four exits, and why each is a black pixel
//!
//! The source returns `vec4( 0.0 )` — colour *and* confidence zero — from three
//! places, and the material's `mix` therefore leaves the cubemap alone. All
//! three are transcribed:
//!
//! 1. **No coverage** (`nrm.z < 0.5`): the G-buffer says nothing was drawn here.
//! 2. **The ray comes back at the camera** (`facing > 0.94`): the reflection of a
//!    near-normal-incidence view is behind the geometry that produced it and can
//!    never be resolved on screen.
//! 3. **The march found nothing**: it ran out of steps, left the screen, crossed
//!    the near plane, or exceeded `maxDistance`.
//!
//! The fourth exit is not an exit at all but a *fade*: a hit near the screen
//! border, at a grazing angle, at long range, or deep inside the thickness window
//! keeps its colour and loses its alpha ([`ssr_confidence`]). That is what stops
//! a reflection popping as geometry leaves the frustum.
//!
//! # Storage width is part of the algorithm
//!
//! - The march writes into a **half-resolution `Rgba16Float`** target
//!   (`hdrTarget(w >> 1, h >> 1)`); reflections are low frequency and this is the
//!   single most expensive marching pass in the frame. `gl_FragCoord` in the
//!   dither is therefore in **half-res pixels**, and the blur's `uDirection` is
//!   the **half-res texel** — see [`SSR_RESOLUTION_SHIFT`].
//! - It *samples* the full-resolution G-buffer. `vUv` is normalised, so the
//!   march point-samples one full-res texel in four. That is the source's
//!   behaviour, not an approximation of it.
//! - The blur reads and writes the same `Rgba16Float`, so a chain that runs
//!   march → blur-x → blur-y rounds to `f16` **twice** in between. The CPU
//!   reference models that by holding every [`ScreenImage`] at whatever precision
//!   its caller quantised it to; the parity harness quantises with
//!   [`crate::bloom_pyramid::half_storage`].
//!
//! # Two adaptations to WebGPU, both stated here and both exact
//!
//! Neither changes a value. Both are sign flips, and a float negation is exact,
//! so the source's grouping survives them intact.
//!
//! 1. **[`NDC_V_SIGN`]** — WebGL's framebuffer `v` runs *up* and coincides with
//!    NDC `y`; a WebGPU texture's `v` runs *down*. Every UV in this pass is a
//!    WebGPU texture coordinate, so the two places that cross between UV and NDC
//!    (`owViewPos` and the forward projection) negate `y`. The pair round-trips —
//!    [`tests::reconstruct_then_project_round_trips_the_uv`] is the proof, and it
//!    is a proof the CPU↔GPU parity tier *cannot* give, because both sides share
//!    the flip.
//! 2. **`gl_FragCoord`** — WebGL counts `y` up from the bottom of the
//!    framebuffer, `@builtin(position)` counts it down from the top. The
//!    interleaved-gradient dither is a function of that coordinate, so the pass
//!    is handed its target size and reconstructs the source's value as
//!    `size.y - position.y`. Without it the dither pattern is mirrored — visually
//!    irrelevant, and exactly the kind of silent divergence this port exists to
//!    not accumulate.
//!
//! A third difference is *not* an adaptation: `vUv` is computed from
//! `@builtin(position)` rather than taken from an interpolated varying. Same
//! value for a full-screen triangle, one interpolator removed from the parity
//! measurement.
//!
//! # Transcription notes (read before changing an expression)
//!
//! - **`uTexel` is dead.** `ssr.js` declares and writes it and its fragment
//!   shader never reads it. Dead source is still source: the lane is kept, named,
//!   and documented as unread. The pass instead divides by the WGSL
//!   `SsrUniform`'s `size` lane — `frag / size`, a division, **never**
//!   `frag * texel`, which is the reciprocal-multiply this port has already
//!   found five times.
//! - **`materialpatch.js` *does* multiply by a reciprocal**
//!   (`gl_FragCoord.xy * owScreenTexel`) and [`ssr_resolve`]'s note says so. Two
//!   different sites, two different faithful transcriptions.
//! - `stepScale = pow( maxDist / 0.06, 1.0 / float( OW_SSR_STEPS ) )` — the
//!   division is inside the `pow`, and the exponent is `1.0 / 28.0`, not a
//!   pre-multiplied constant. This is the one transcendental in the march and it
//!   compounds over 28 geometric steps; see the parity module's tolerance note.
//! - `t` starts at `0.06 + jitter * 0.06` but `stepScale` is derived from `0.06`
//!   alone, so a jittered ray overshoots `maxDistance` slightly early and the
//!   `if ( t > maxDist ) break` at the tail of the loop catches it. That is the
//!   source's arithmetic, not a bug to correct.
//! - The thickness window **grows with distance**: `diff < uParams.y + t * 0.06`.
//!   The confidence fade, however, uses the *un*-grown `uParams.y`.
//! - The confidence uses the **hit iteration's `t`**, not the refined `hi`; the
//!   edge fade uses the **refined** UV. Both are the source's.
//! - `lo` is written by the refine loop and never read after it. Kept.
//! - The refine loop samples at an **unclamped** UV that can leave the screen;
//!   clamp-to-edge addressing is what makes that well-defined, so the samplers
//!   are `AddressMode::ClampToEdge` and [`ScreenImage`] clamps identically.
//! - `max( color, vec3( 0.0 ) )` is a component-wise `max`, applied to the
//!   colour only; the alpha is `clamp( conf, 0, 1 ) * intensity`.
//!
//! # Where the shared vocabulary should eventually live
//!
//! [`ign`], [`view_pos`] and [`project_uv`] are `glsl.js`'s `COMMON`, which the
//! source textually inlines into every screen-space pass. They are transcribed
//! **independently** here and in [`crate::contact`] on purpose: one author
//! writing one copy is precisely how ten `sky/` defects survived a review. Once a
//! third consumer lands (`gtao.js`, `taa.js`, `motionblur.js` all include
//! `COMMON`), lift them into [`crate::gbuffer`] beside
//! [`crate::gbuffer::decode_normal`] — that module already argues the case for
//! `owDecodeNormal`, and the argument is the same. The file that must change is
//! `modules/axiom-gpu-backend/src/gbuffer.rs`.
//!
//! [`ScreenImage`] is different: it is harness scaffolding, not transcribed
//! source, so [`crate::contact`] shares this one rather than risking two
//! samplers that disagree about clamp-to-edge.

use crate::gbuffer::{decode_normal, VELOCITY_TEXTURE_V_SIGN};

/// `#define OW_SSR_STEPS 28` — the march's step count. **This is the algorithm.**
/// One more or one fewer changes `stepScale`, changes where every ray lands, and
/// changes every silhouette in the reflection.
pub(crate) const SSR_STEPS: i32 = 28;

/// `#define OW_SSR_REFINE 5` — bisections between `prevT` and `t` once a
/// crossing is bracketed. Five halvings resolve the bracket to 1/32 of the step
/// that found it.
pub(crate) const SSR_REFINE: i32 = 5;

/// The first step's distance in metres, and the base `stepScale` is derived
/// from. The jitter adds up to another `0.06`.
pub(crate) const SSR_START_T: f32 = 0.06;

/// `uParams` defaults, from `new THREE.Vector4( 24, 0.6, 0, 1 )`: max march
/// distance in metres, thickness in metres, frame (set per frame), intensity.
pub(crate) const SSR_MAX_DISTANCE: f32 = 24.0;
/// See [`SSR_MAX_DISTANCE`].
pub(crate) const SSR_THICKNESS: f32 = 0.6;
/// See [`SSR_MAX_DISTANCE`].
pub(crate) const SSR_INTENSITY: f32 = 1.0;

/// `uParams.value.z = frame % 64` — the dither's temporal cycle length.
pub(crate) const SSR_FRAME_CYCLE: u32 = 64;

/// The dither's per-frame offset: `owIGN( gl_FragCoord.xy + uParams.z * 7.331 )`.
pub(crate) const SSR_JITTER_FRAME_SCALE: f32 = 7.331;

/// `dot( -V, R )` above this cannot be resolved on screen at all, so the pass
/// returns black rather than marching.
pub(crate) const SSR_FACING_CUTOFF: f32 = 0.94;

/// The screen-border fade's width, in UV. `smoothstep( 0.0, 0.12, uv )` at each
/// edge.
pub(crate) const SSR_EDGE_FADE: f32 = 0.12;

/// `hdrTarget( w >> 1, h >> 1 )` — the march and its blur run at **half**
/// resolution in each axis.
pub(crate) const SSR_RESOLUTION_SHIFT: u32 = 1;

/// `material.roughness < 0.62` — `materialpatch.js`'s cutoff, above which the
/// reflection is not fetched at all.
pub(crate) const SSR_ROUGHNESS_CUTOFF: f32 = 0.62;

/// The far edge of `smoothstep( 0.62, 0.14, material.roughness )`. Note the
/// **reversed** edges: this is a ramp that reaches full weight at a *low*
/// roughness.
pub(crate) const SSR_ROUGHNESS_FULL: f32 = 0.14;

/// The sign that turns a WebGPU texture `v` into a clip-space `y`.
///
/// WebGL's framebuffer `v` runs up and coincides with NDC `y`, so the source
/// never needed this. A WebGPU texture's `v` runs down, so the two conversions
/// between UV and NDC — [`view_pos`] and [`project_uv`] — negate `y`. Negation
/// is exact in `f32`, so the source's grouping is untouched. The peer of
/// [`crate::gbuffer::VELOCITY_TEXTURE_V_SIGN`], and for the same reason.
pub(crate) const NDC_V_SIGN: f32 = -1.0;

/// A screen-space texture the CPU reference samples: the semantic definition of
/// what a `wgpu` sampler with `AddressMode::ClampToEdge` does to this pass's
/// inputs.
///
/// One type for all four inputs (depth, normal, velocity, colour) because the
/// **addressing** is the thing being modelled and it is identical for all of
/// them; only which lanes carry meaning differs. Unused lanes are whatever the
/// caller put there, exactly as an unwritten channel on a GPU texture is.
///
/// This is scaffolding, not transcription — see the module header for why
/// [`crate::contact`] shares it rather than transcribing a second one.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ScreenImage {
    width: u32,
    height: u32,
    texels: Vec<[f32; 4]>,
}

impl ScreenImage {
    /// A `width x height` image whose texel `(x, y)` is `texel(x, y)`, row-major
    /// in **memory order** — the order a texture upload uses, and the order in
    /// which row `0` is the row a UV near `v = 0` samples (WebGPU's `v` runs
    /// down, so row `0` is the top row).
    ///
    /// A generator rather than a buffer, so the length cannot disagree with the
    /// dimensions and there is no shape to validate.
    pub(crate) fn from_fn(
        width: u32,
        height: u32,
        texel: impl Fn(u32, u32) -> [f32; 4],
    ) -> ScreenImage {
        ScreenImage {
            width,
            height,
            texels: (0..height)
                .flat_map(|y| (0..width).map(move |x| (x, y)))
                .map(|(x, y)| texel(x, y))
                .collect(),
        }
    }

    /// The image's width in texels.
    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    /// The image's height in texels.
    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    /// Every texel, row-major in memory order.
    pub(crate) fn texels(&self) -> &[[f32; 4]] {
        &self.texels
    }

    /// One texel with clamp-to-edge addressing.
    ///
    /// `as i32` saturates in Rust where a GPU's coordinate arithmetic wraps, but
    /// the clamp that follows makes both land on the edge texel, which is what
    /// `ClampToEdge` means. A NaN coordinate becomes `0` and then the first
    /// texel; a GPU is free to return any texel for a NaN coordinate, so nothing
    /// in this pass may depend on that value — and nothing does, because a NaN
    /// coordinate only arises on a pixel whose result is discarded by the
    /// coverage test.
    fn texel(&self, x: i32, y: i32) -> [f32; 4] {
        let cx = x.clamp(0, self.width as i32 - 1) as usize;
        let cy = y.clamp(0, self.height as i32 - 1) as usize;
        self.texels[cy * self.width as usize + cx]
    }

    /// A **nearest**, clamp-to-edge sample: `floor( uv * dim )`, the texel whose
    /// footprint contains the coordinate. What `FilterMode::Nearest` does, and
    /// what the G-buffer's three attachments are sampled with — `prepass.js`
    /// sets `NearestFilter` on all three.
    pub(crate) fn nearest(&self, uv: [f32; 2]) -> [f32; 4] {
        self.texel(
            (uv[0] * self.width as f32).floor() as i32,
            (uv[1] * self.height as f32).floor() as i32,
        )
    }

    /// A **bilinear**, clamp-to-edge sample: `p = uv * dim - 0.5`, two lerps
    /// across `x` then one across `y`, each written `a + (b - a) * t` so both
    /// sides agree on which factoring a `mix` builtin is permitted to pick.
    ///
    /// This is what `pass.js`'s `hdrTarget` default (`LinearFilter`) does, and
    /// therefore how the previous resolved frame and both blur inputs are read.
    pub(crate) fn bilinear(&self, uv: [f32; 2]) -> [f32; 4] {
        let px = uv[0] * self.width as f32 - 0.5;
        let py = uv[1] * self.height as f32 - 0.5;
        let fx = px - px.floor();
        let fy = py - py.floor();
        let ix = px.floor() as i32;
        let iy = py.floor() as i32;
        let lower = lerp4(self.texel(ix, iy), self.texel(ix + 1, iy), fx);
        let upper = lerp4(self.texel(ix, iy + 1), self.texel(ix + 1, iy + 1), fx);
        lerp4(lower, upper, fy)
    }
}

/// `a + (b - a) * t`, written out. Not a `mix` builtin, whose factoring is
/// unspecified.
fn lerp4(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [0, 1, 2, 3].map(|lane| a[lane] + (b[lane] - a[lane]) * t)
}

/// GLSL `clamp( x, lo, hi )` = `min( max( x, lo ), hi )`, written out.
pub(crate) fn glsl_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    x.max(lo).min(hi)
}

/// GLSL `smoothstep( e0, e1, x )`, written out:
/// `t = clamp( ( x - e0 ) / ( e1 - e0 ), 0, 1 ); return t * t * ( 3 - 2 * t );`
///
/// The division is a division. `e0 > e1` is legal and produces a *descending*
/// ramp, which is exactly what the roughness weight uses.
pub(crate) fn glsl_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = glsl_clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `owIGN` from `glsl.js` — Jimenez's interleaved gradient noise, the right
/// dither for a rotating sample kernel a temporal filter then resolves:
///
/// ```glsl
/// fract( 52.9829189 * fract( dot( p, vec2( 0.06711056, 0.00583715 ) ) ) )
/// ```
///
/// `fract` is `x - floor(x)`, not a remainder, and `p` is a pixel coordinate
/// plus a frame offset so it is always positive here — but the definition is
/// written the GLSL way regardless, because a `%` would be wrong the moment it
/// is not.
///
/// The `dot` is written out: a builtin may factor its two products however it
/// likes.
pub(crate) fn ign(p: [f32; 2]) -> f32 {
    let d = p[0] * 0.06711056 + p[1] * 0.00583715;
    let inner = d - d.floor();
    let outer = 52.9829189 * inner;
    outer - outer.floor()
}

/// `owViewPos` from `glsl.js` — a view-space position from a UV and a
/// **positive** linear view depth in metres (the G-buffer's slot 2):
///
/// ```glsl
/// vec4 h = projInv * vec4( uv * 2.0 - 1.0, 1.0, 1.0 );
/// vec3 dir = h.xyz / h.w;
/// dir /= max( 1e-6, -dir.z );
/// return dir * depth;
/// ```
///
/// Two things a reader should not have to infer:
///
/// - The `y` of the NDC coordinate is negated ([`NDC_V_SIGN`]). See the module
///   header.
/// - The `1.0` handed in as NDC `z` is a *far plane* in both a GL-style
///   (`z ∈ [-1, 1]`) and a WebGPU-style (`z ∈ [0, 1]`) projection, and the
///   `dir /= max( 1e-6, -dir.z )` that follows normalises the ray to the `z = -1`
///   plane. The reconstructed direction is therefore independent of which
///   convention the caller's matrix uses, which is why this pass does not need
///   to know.
///
/// `proj_inv` is column-major, the convention every matrix crossing this backend
/// uses.
pub(crate) fn view_pos(uv: [f32; 2], depth: f32, proj_inv: &[f32; 16]) -> [f32; 3] {
    let ndc_x = uv[0] * 2.0 - 1.0;
    let ndc_y = (uv[1] * 2.0 - 1.0) * NDC_V_SIGN;
    let h = mat4_mul_vec4(proj_inv, [ndc_x, ndc_y, 1.0, 1.0]);
    let dir = [h[0] / h[3], h[1] / h[3], h[2] / h[3]];
    let scale = (-dir[2]).max(1e-6);
    let unit = [dir[0] / scale, dir[1] / scale, dir[2] / scale];
    [unit[0] * depth, unit[1] * depth, unit[2] * depth]
}

/// `clip.xy / clip.w * 0.5 + 0.5` for a view-space point — the forward half of
/// [`view_pos`], with the same [`NDC_V_SIGN`] flip so the two round-trip.
///
/// Only `x`, `y` and `w` of the clip position are read. The source builds the
/// whole `vec4` and reads three of its lanes; the `z` it discards is genuinely
/// unobservable, so it is not computed.
pub(crate) fn project_uv(p: [f32; 3], proj: &[f32; 16]) -> [f32; 2] {
    let clip = mat4_mul_vec4(proj, [p[0], p[1], p[2], 1.0]);
    [
        clip[0] / clip[3] * 0.5 + 0.5,
        clip[1] * NDC_V_SIGN / clip[3] * 0.5 + 0.5,
    ]
}

/// A column-major `mat4 * vec4`, accumulated **column by column** left to right,
/// which is the order GLSL and WGSL both specify and the order a re-association
/// would break.
fn mat4_mul_vec4(m: &[f32; 16], v: [f32; 4]) -> [f32; 4] {
    [0, 1, 2, 3].map(|row| m[row] * v[0] + m[4 + row] * v[1] + m[8 + row] * v[2] + m[12 + row] * v[3])
}

/// GLSL `dot` for a `vec3`, written out in the order the source's components
/// appear.
fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// GLSL `normalize( v )` = `v / length( v )`, `length` = `sqrt( dot( v, v ) )`.
/// The same factoring [`crate::gbuffer::decode_normal`] uses, so the two agree
/// where they overlap.
fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = dot3(v, v).sqrt();
    [v[0] / len, v[1] / len, v[2] / len]
}

/// GLSL `reflect( I, N )` = `I - 2.0 * dot( N, I ) * N`. Note the argument order
/// inside the `dot`: the *normal* first.
fn reflect3(i: [f32; 3], n: [f32; 3]) -> [f32; 3] {
    let k = 2.0 * dot3(n, i);
    [i[0] - k * n[0], i[1] - k * n[1], i[2] - k * n[2]]
}

/// `uParams` for the march: `x maxDistance  y thickness  z frame  w intensity`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SsrParams {
    /// `uParams.x`, metres.
    pub(crate) max_distance: f32,
    /// `uParams.y`, metres.
    pub(crate) thickness: f32,
    /// `uParams.z`, `frame % 64` as a float.
    pub(crate) frame: f32,
    /// `uParams.w`, the alpha the confidence is scaled by.
    pub(crate) intensity: f32,
}

impl SsrParams {
    /// The source's constructor defaults, with `frame` supplied.
    ///
    /// `frame` is reduced modulo [`SSR_FRAME_CYCLE`] here rather than by the
    /// caller, because `uParams.value.z = frame % 64` is the source's line and a
    /// caller that forgot it would silently lose the dither's temporal cycle
    /// after 64 frames of float precision drift.
    pub(crate) fn at_frame(frame: u32) -> SsrParams {
        SsrParams {
            max_distance: SSR_MAX_DISTANCE,
            thickness: SSR_THICKNESS,
            frame: (frame % SSR_FRAME_CYCLE) as f32,
            intensity: SSR_INTENSITY,
        }
    }
}

/// The four textures the march reads, plus the camera pair it marches with.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SsrInputs<'a> {
    /// G-buffer slot 2: linear view depth in metres, positive, `R32Float`.
    /// Nearest-sampled.
    pub(crate) depth: &'a ScreenImage,
    /// G-buffer slot 0: oct **view** normal in `xy`, coverage in `z`,
    /// `Rgba16Float`. Nearest-sampled.
    pub(crate) normal: &'a ScreenImage,
    /// G-buffer slot 1: screen-space velocity, `Rg16Float`. Nearest-sampled.
    /// Its `y` is negated on read — see [`crate::gbuffer::VELOCITY_TEXTURE_V_SIGN`].
    pub(crate) velocity: &'a ScreenImage,
    /// The **previous** resolved frame, HDR, `Rgba16Float`. Bilinear-sampled.
    pub(crate) color: &'a ScreenImage,
    /// `uProj`, column-major.
    pub(crate) proj: &'a [f32; 16],
    /// `uProjInv`, column-major.
    pub(crate) proj_inv: &'a [f32; 16],
}

/// What one marched ray found: the refined UV, the *unrefined* depth difference
/// that triggered the hit, and the `t` of the iteration that found it.
///
/// The three lanes are used by three different fades and they are deliberately
/// not the same quantity: `uv` is refined, `diff` and `t` are not.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SsrHit {
    uv: [f32; 2],
    diff: f32,
    t: f32,
}

/// One step of the march's carried state: the current distance, and the distance
/// of the previous step, which is the near end of the refine bracket.
#[derive(Debug, Clone, Copy)]
struct MarchState {
    t: f32,
    prev_t: f32,
}

/// The binary refine: `OW_SSR_REFINE` bisections of `[lo, hi]`, keeping the half
/// the crossing is in.
///
/// ```glsl
/// float mid = ( lo + hi ) * 0.5;
/// vec3 mp = start + R * mid;
/// float md = texture2D( tDepth, muv ).r;
/// if ( -mp.z - md > 0.0 ) hi = mid; else lo = mid;
/// ```
///
/// `muv` is **not** clamped and can leave the screen; clamp-to-edge addressing is
/// what makes that defined. Only `hi` is read afterwards, but `lo` genuinely
/// participates in the bisection.
fn refine(
    start: [f32; 3],
    ray: [f32; 3],
    lo: f32,
    hi: f32,
    proj: &[f32; 16],
    depth: &ScreenImage,
) -> f32 {
    (0..SSR_REFINE)
        .fold((lo, hi), |(lo, hi), _| {
            let mid = (lo + hi) * 0.5;
            let mp = [
                start[0] + ray[0] * mid,
                start[1] + ray[1] * mid,
                start[2] + ray[2] * mid,
            ];
            let muv = project_uv(mp, proj);
            let md = depth.nearest(muv)[0];
            let crossed = -mp[2] - md > 0.0;
            [(mid, hi), (lo, mid)][usize::from(crossed)]
        })
        .1
}

/// The march itself: `OW_SSR_STEPS` geometric steps, the first crossing refined.
///
/// Written as a `try_fold` whose `Err` carries the outcome, because the Rust
/// spine is branchless and the GLSL `break` is a control-flow exit. The three
/// exits keep the source's **order**: the near-plane test, then the screen test,
/// then the hit test — a step that leaves the screen cannot report a hit even if
/// the clamped sample it took happens to satisfy the thickness window.
///
/// One difference from the shader, and it is invisible: the reference evaluates
/// [`refine`] on **every** step rather than only on the step that hits, because
/// selecting a value is cheaper to write branchlessly than selecting whether to
/// compute it. Every sample it takes is clamp-addressed and therefore safe, and
/// the result is discarded unless the step actually hit.
fn march(
    inputs: &SsrInputs<'_>,
    params: SsrParams,
    start: [f32; 3],
    ray: [f32; 3],
    t0: f32,
    step_scale: f32,
) -> Option<SsrHit> {
    (0..SSR_STEPS)
        .try_fold(
            MarchState {
                t: t0,
                prev_t: t0,
            },
            |state, _| {
                let sp = [
                    start[0] + ray[0] * state.t,
                    start[1] + ray[1] * state.t,
                    start[2] + ray[2] * state.t,
                ];
                // `if ( sp.z > -0.05 ) break;`
                let past_near_plane = sp[2] > -0.05;
                let suv = project_uv(sp, inputs.proj);
                // `if ( suv.x <= 0.0 || suv.x >= 1.0 || suv.y <= 0.0 || suv.y >= 1.0 ) break;`
                let off_screen = (suv[0] <= 0.0)
                    | (suv[0] >= 1.0)
                    | (suv[1] <= 0.0)
                    | (suv[1] >= 1.0);

                let scene_depth = inputs.depth.nearest(suv)[0];
                let cov = inputs.normal.nearest(suv)[2];
                let diff = -sp[2] - scene_depth;
                // `cov > 0.5 && diff > 0.0 && diff < uParams.y + t * 0.06`
                let inside_window =
                    (cov > 0.5) & (diff > 0.0) & (diff < params.thickness + state.t * 0.06);
                let hit = inside_window & !past_near_plane & !off_screen;

                let hi = refine(start, ray, state.prev_t, state.t, inputs.proj, inputs.depth);
                let fp = [
                    start[0] + ray[0] * hi,
                    start[1] + ray[1] * hi,
                    start[2] + ray[2] * hi,
                ];
                let found = SsrHit {
                    uv: project_uv(fp, inputs.proj),
                    diff,
                    t: state.t,
                };

                // `prevT = t; t *= stepScale; if ( t > maxDist ) break;`
                let advanced = MarchState {
                    t: state.t * step_scale,
                    prev_t: state.t,
                };
                let exhausted = advanced.t > params.max_distance;
                let stop = past_near_plane | off_screen | hit | exhausted;
                [Ok(advanced), Err(hit.then_some(found))][usize::from(stop)]
            },
        )
        .err()
        .flatten()
}

/// The confidence, transcribed exactly:
///
/// ```glsl
/// vec2 edge = smoothstep( vec2( 0.0 ), vec2( 0.12 ), hitUv ) *
///             smoothstep( vec2( 0.0 ), vec2( 0.12 ), 1.0 - hitUv );
/// float conf = edge.x * edge.y;
/// conf *= 1.0 - smoothstep( 0.7, 0.94, facing );
/// conf *= 1.0 - smoothstep( maxDist * 0.55, maxDist, t );
/// conf *= 1.0 - smoothstep( uParams.y * 0.5, uParams.y, hitDiff );
/// ```
///
/// The four multiplications are **sequential**, left to right, and folding them
/// into one product would re-associate a chain the source specifies.
///
/// Note the last fade uses the *un*-distance-grown thickness, while the hit test
/// that produced `hit_diff` used `thickness + t * 0.06`. A hit found late in the
/// march can therefore sit outside this ramp entirely and fade to zero — which is
/// the intent: a thick late hit is a guess.
pub(crate) fn ssr_confidence(
    hit_uv: [f32; 2],
    facing: f32,
    t: f32,
    hit_diff: f32,
    max_distance: f32,
    thickness: f32,
) -> f32 {
    let edge_x = glsl_smoothstep(0.0, SSR_EDGE_FADE, hit_uv[0])
        * glsl_smoothstep(0.0, SSR_EDGE_FADE, 1.0 - hit_uv[0]);
    let edge_y = glsl_smoothstep(0.0, SSR_EDGE_FADE, hit_uv[1])
        * glsl_smoothstep(0.0, SSR_EDGE_FADE, 1.0 - hit_uv[1]);
    let conf = edge_x * edge_y;
    let conf = conf * (1.0 - glsl_smoothstep(0.7, SSR_FACING_CUTOFF, facing));
    let conf = conf * (1.0 - glsl_smoothstep(max_distance * 0.55, max_distance, t));
    conf * (1.0 - glsl_smoothstep(thickness * 0.5, thickness, hit_diff))
}

/// **The pass, for one pixel** — the semantic definition the WGSL is checked
/// against.
///
/// `frag_coord` is `@builtin(position).xy`, i.e. `(x + 0.5, y + 0.5)` with `y`
/// counting **down**; `target_size` is the *half-resolution* target's size in
/// pixels. Both are needed because the source's dither is a function of WebGL's
/// bottom-up `gl_FragCoord`, which is reconstructed as `size.y - position.y`.
///
/// Returns the `Rgba16Float` texel the pass writes: reflected colour in `rgb`,
/// confidence times intensity in `a`. The caller quantises; storing is the
/// caller's arithmetic, not this function's.
pub(crate) fn ssr_pixel(
    inputs: &SsrInputs<'_>,
    params: SsrParams,
    frag_coord: [f32; 2],
    target_size: [f32; 2],
) -> [f32; 4] {
    let v_uv = [frag_coord[0] / target_size[0], frag_coord[1] / target_size[1]];

    let nrm = inputs.normal.nearest(v_uv);
    // `if ( nrm.z < 0.5 ) { gl_FragColor = vec4( 0.0 ); return; }`
    let covered = nrm[2] >= 0.5;

    let depth = inputs.depth.nearest(v_uv)[0];
    let p = view_pos(v_uv, depth, inputs.proj_inv);
    let n = decode_normal([nrm[0], nrm[1]]);
    let v = normalize3(p);
    let r = reflect3(v, n);

    let facing = glsl_clamp(dot3([-v[0], -v[1], -v[2]], r), 0.0, 1.0);
    // `if ( facing > 0.94 ) { gl_FragColor = vec4( 0.0 ); return; }`
    let resolvable = facing <= SSR_FACING_CUTOFF;

    // WebGL counts gl_FragCoord.y up from the bottom; @builtin(position) counts
    // it down from the top.
    let gl_frag_coord = [frag_coord[0], target_size[1] - frag_coord[1]];
    let offset = params.frame * SSR_JITTER_FRAME_SCALE;
    let jitter = ign([gl_frag_coord[0] + offset, gl_frag_coord[1] + offset]);

    let bias = 0.02 + depth * 0.002;
    let start = [
        p[0] + n[0] * bias,
        p[1] + n[1] * bias,
        p[2] + n[2] * bias,
    ];
    let t0 = SSR_START_T + jitter * 0.06;
    let step_scale = (params.max_distance / SSR_START_T).powf(1.0 / SSR_STEPS as f32);

    let marched = march(inputs, params, start, r, t0, step_scale).map_or([0.0; 4], |hit| {
        let vel_raw = inputs.velocity.nearest(hit.uv);
        let vel = [vel_raw[0], vel_raw[1] * VELOCITY_TEXTURE_V_SIGN];
        let src_uv = [
            glsl_clamp(hit.uv[0] - vel[0], 0.001, 0.999),
            glsl_clamp(hit.uv[1] - vel[1], 0.001, 0.999),
        ];
        let color = inputs.color.bilinear(src_uv);
        let conf = ssr_confidence(
            hit.uv,
            facing,
            hit.t,
            hit.diff,
            params.max_distance,
            params.thickness,
        );
        [
            color[0].max(0.0),
            color[1].max(0.0),
            color[2].max(0.0),
            glsl_clamp(conf, 0.0, 1.0) * params.intensity,
        ]
    });

    [[0.0; 4], marched][usize::from(covered & resolvable)]
}

/// **The separable blur**, for one pixel:
///
/// ```glsl
/// vec4 sum = texture2D( tSrc, vUv ) * 0.4;
/// float w = 0.4;
/// for ( int i = 1; i <= 2; i ++ ) {
///   float wi = 0.3 / float( i );
///   sum += texture2D( tSrc, vUv + uDirection * float( i ) ) * wi;
///   sum += texture2D( tSrc, vUv - uDirection * float( i ) ) * wi;
///   w += wi * 2.0;
/// }
/// gl_FragColor = sum / w;
/// ```
///
/// Five taps at weights `0.4, 0.3, 0.3, 0.15, 0.15`, normalised by their sum
/// (`1.3`) at the end rather than pre-normalised — `sum / w`, a division. `wi` is
/// `0.3 / float( i )`, also a division, and the `+` tap is accumulated **before**
/// the `-` tap.
///
/// `direction` is one **half-resolution** texel in one axis; the pass runs
/// horizontally into a second target and then vertically back.
pub(crate) fn ssr_blur_pixel(
    src: &ScreenImage,
    uv: [f32; 2],
    direction: [f32; 2],
) -> [f32; 4] {
    let centre = src.bilinear(uv);
    let (sum, w) = (1..=2_i32).fold(
        ([0, 1, 2, 3].map(|lane| centre[lane] * 0.4), 0.4_f32),
        |(sum, w), i| {
            let wi = 0.3 / i as f32;
            let step = [direction[0] * i as f32, direction[1] * i as f32];
            let plus = src.bilinear([uv[0] + step[0], uv[1] + step[1]]);
            let minus = src.bilinear([uv[0] - step[0], uv[1] - step[1]]);
            let with_plus = [0, 1, 2, 3].map(|lane| sum[lane] + plus[lane] * wi);
            let with_minus = [0, 1, 2, 3].map(|lane| with_plus[lane] + minus[lane] * wi);
            (with_minus, w + wi * 2.0)
        },
    );
    [0, 1, 2, 3].map(|lane| sum[lane] / w)
}

/// `owW` from `materialpatch.js`: `owSsr.a * smoothstep( 0.62, 0.14, roughness )`.
///
/// The edges are **reversed** — full weight at a mirror roughness, zero at
/// `0.62` — which is why the source's `if ( material.roughness < 0.62 )` guard
/// is redundant to the arithmetic: at and above the cutoff the ramp is already
/// exactly zero. [`tests::the_roughness_gate_is_redundant_to_the_ramp`] pins
/// that, which is what lets the Rust be branchless while the WGSL keeps the
/// source's `if`.
pub(crate) fn ssr_resolve_weight(alpha: f32, roughness: f32) -> f32 {
    alpha * glsl_smoothstep(SSR_ROUGHNESS_CUTOFF, SSR_ROUGHNESS_FULL, roughness)
}

/// **How the material consumes this pass**, from `materialpatch.js`:
///
/// ```glsl
/// if ( owFeat.z > 0.5 && material.roughness < 0.62 ) {
///   vec4 owSsr = texture2D( owSsrTex, gl_FragCoord.xy * owScreenTexel );
///   float owW = owSsr.a * smoothstep( 0.62, 0.14, material.roughness );
///   radiance = mix( radiance, owSsr.rgb, clamp( owW, 0.0, 1.0 ) );
/// }
/// ```
///
/// GLSL `mix( x, y, a )` is `x * ( 1 - a ) + y * a` — **not** `x + (y - x) * a` —
/// and it is written that way here.
///
/// It lives in this module and not in the material's, because the roughness
/// cutoff, the ramp and the fact that the reflection **replaces** rather than
/// adds to the IBL specular are all properties of *this pass's* contract. The
/// port of `materialpatch.js` should call this rather than re-derive it. Two
/// things stay there, deliberately: the `owFeat.z` feature bit (a pass-enable,
/// not arithmetic) and the fetch itself — whose UV is
/// `gl_FragCoord.xy * owScreenTexel`, a genuine **reciprocal-multiply in the
/// source**, and not to be tidied into a division.
pub(crate) fn ssr_resolve(radiance: [f32; 3], reflection: [f32; 4], roughness: f32) -> [f32; 3] {
    let a = glsl_clamp(ssr_resolve_weight(reflection[3], roughness), 0.0, 1.0);
    [0, 1, 2].map(|lane| radiance[lane] * (1.0 - a) + reflection[lane] * a)
}

/// Floats in the march's `SsrUniform` block. `proj` (16) + `proj_inv` (16) +
/// `params` (4) + `texel` (2) + `size` (2) = 40, which is 160 bytes and already
/// a multiple of 16, so the block needs no tail padding.
pub(crate) const SSR_UNIFORM_FLOATS: usize = 40;

/// Pack the march's uniform block.
///
/// `texel` is `1 / half_width, 1 / half_height` — the source's `uTexel`, which
/// its **fragment shader never reads**. It is carried because dead source is
/// still source and because the blur's `uDirection` is exactly this value; the
/// shader derives its UV from `size` by division instead.
pub(crate) fn pack_ssr_uniform(
    proj: &[f32; 16],
    proj_inv: &[f32; 16],
    params: SsrParams,
    texel: [f32; 2],
    size: [f32; 2],
) -> [f32; SSR_UNIFORM_FLOATS] {
    let mut out = [0.0_f32; SSR_UNIFORM_FLOATS];
    out[0..16].copy_from_slice(proj);
    out[16..32].copy_from_slice(proj_inv);
    out[32] = params.max_distance;
    out[33] = params.thickness;
    out[34] = params.frame;
    out[35] = params.intensity;
    out[36] = texel[0];
    out[37] = texel[1];
    out[38] = size[0];
    out[39] = size[1];
    out
}

/// Floats in the blur's uniform block: `direction` (2) + `size` (2).
pub(crate) const SSR_BLUR_UNIFORM_FLOATS: usize = 4;

/// Pack the blur's uniform block. `direction` is `( texel.x, 0 )` on the
/// horizontal pass and `( 0, texel.y )` on the vertical one, in that order, and
/// `size` is the half-resolution target's.
pub(crate) fn pack_ssr_blur_uniform(
    direction: [f32; 2],
    size: [f32; 2],
) -> [f32; SSR_BLUR_UNIFORM_FLOATS] {
    [direction[0], direction[1], size[0], size[1]]
}

/// The half-resolution the march and its blur run at: `Math.max( 1, w >> 1 )`.
///
/// The `max(1, …)` is the source's and matters at the last mip-sized viewport a
/// phone can hand us; a zero-width target is not a render target.
pub(crate) fn ssr_target_size(width: u32, height: u32) -> (u32, u32) {
    (
        (width >> SSR_RESOLUTION_SHIFT).max(1),
        (height >> SSR_RESOLUTION_SHIFT).max(1),
    )
}

/// **The pure half of the pass's WGSL** — `glsl.js`'s `COMMON` helpers and the
/// arithmetic this pass adds, with no bindings and no entry points.
///
/// Split from [`SSR_PASS_WGSL`] for the reason `bloom_pyramid::wgsl` is split:
/// the parity harness compiles *these* functions with its own entry points, so
/// the tight tolerance measures the transcription and not a texture unit's
/// subtexel precision.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) const SSR_COMMON_WGSL: &str = r#"
// ---------------------------------------------------------------------------
// glsl.js COMMON, transcribed. The source textually inlines this into every
// screen-space pass via `${COMMON}`; so does this port.
// ---------------------------------------------------------------------------

// The sign that turns a WebGPU texture `v` into a clip-space `y`. The source
// has no such constant: WebGL's framebuffer `v` runs up and coincides with NDC
// `y`. A negation is exact, so the source's grouping survives it.
const SSR_NDC_V_SIGN: f32 = -1.0;

// The sign a consumer applies to the velocity buffer's `y` to turn it into a
// TEXTURE-space delta. The peer of `crate::gbuffer::VELOCITY_TEXTURE_V_SIGN`,
// and a different fact from the one above: that flip is about this pass's own
// projection, this one is about what the G-buffer stored.
const SSR_VELOCITY_V_SIGN: f32 = -1.0;

// GLSL `clamp( x, lo, hi )` = `min( max( x, lo ), hi )`.
fn ssr_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    return min(max(x, lo), hi);
}

// GLSL `smoothstep( e0, e1, x )`. Written out: WGSL's builtin is permitted to
// factor differently, and `e0 > e1` (the roughness ramp) must still descend.
fn ssr_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ssr_clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// GLSL `dot` for a vec3, written out: a builtin may factor its three products
// however it likes.
fn ssr_dot3(a: vec3<f32>, b: vec3<f32>) -> f32 {
    return a.x * b.x + a.y * b.y + a.z * b.z;
}

// GLSL `normalize( v )` = `v / length( v )`, `length` = `sqrt( dot( v, v ) )`.
fn ssr_normalize3(v: vec3<f32>) -> vec3<f32> {
    return v / sqrt(ssr_dot3(v, v));
}

// GLSL `reflect( I, N )` = `I - 2.0 * dot( N, I ) * N`. The NORMAL is the dot's
// first argument.
fn ssr_reflect(i: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    return i - 2.0 * ssr_dot3(n, i) * n;
}

// `owIGN`: fract( 52.9829189 * fract( dot( p, vec2( 0.06711056, 0.00583715 ) ) ) )
fn ssr_ign(p: vec2<f32>) -> f32 {
    let d = p.x * 0.06711056 + p.y * 0.00583715;
    return fract(52.9829189 * fract(d));
}

// `owDecodeNormal`. The CPU peer is `crate::gbuffer::decode_normal`, and the
// parity harness compares this against THAT rather than a second copy.
fn ssr_decode_normal(f: vec2<f32>) -> vec3<f32> {
    let nz = 1.0 - abs(f.x) - abs(f.y);
    let t = max(-nz, 0.0);
    let nx = f.x + select(t, -t, f.x >= 0.0);
    let ny = f.y + select(t, -t, f.y >= 0.0);
    return ssr_normalize3(vec3<f32>(nx, ny, nz));
}

// `owViewPos( uv, depth, projInv )`. The NDC `y` is negated; see the module
// header. The `1.0` NDC `z` is a far plane under both depth conventions and the
// `/ max( 1e-6, -dir.z )` normalises the ray either way.
fn ssr_view_pos(uv: vec2<f32>, depth: f32, proj_inv: mat4x4<f32>) -> vec3<f32> {
    let ndc = uv * 2.0 - 1.0;
    let h = proj_inv * vec4<f32>(ndc.x, ndc.y * SSR_NDC_V_SIGN, 1.0, 1.0);
    var dir = h.xyz / h.w;
    dir = dir / max(1e-6, -dir.z);
    return dir * depth;
}

// `clip.xy / clip.w * 0.5 + 0.5`, with the same `y` flip so the two round-trip.
fn ssr_project(p: vec3<f32>, proj: mat4x4<f32>) -> vec2<f32> {
    let clip = proj * vec4<f32>(p, 1.0);
    return vec2<f32>(clip.x, clip.y * SSR_NDC_V_SIGN) / clip.w * 0.5 + 0.5;
}

// ---------------------------------------------------------------------------
// ssr.js
// ---------------------------------------------------------------------------

const OW_SSR_STEPS: i32 = 28;
const OW_SSR_REFINE: i32 = 5;

// The four confidence fades, in the source's order, multiplied sequentially.
fn ssr_confidence(
    hit_uv: vec2<f32>,
    facing: f32,
    t: f32,
    hit_diff: f32,
    max_dist: f32,
    thickness: f32,
) -> f32 {
    let edge_x = ssr_smoothstep(0.0, 0.12, hit_uv.x) * ssr_smoothstep(0.0, 0.12, 1.0 - hit_uv.x);
    let edge_y = ssr_smoothstep(0.0, 0.12, hit_uv.y) * ssr_smoothstep(0.0, 0.12, 1.0 - hit_uv.y);
    var conf = edge_x * edge_y;
    conf = conf * (1.0 - ssr_smoothstep(0.7, 0.94, facing));
    conf = conf * (1.0 - ssr_smoothstep(max_dist * 0.55, max_dist, t));
    conf = conf * (1.0 - ssr_smoothstep(thickness * 0.5, thickness, hit_diff));
    return conf;
}

// ---------------------------------------------------------------------------
// materialpatch.js -- how the material consumes the reflection.
// ---------------------------------------------------------------------------

fn ssr_resolve_weight(alpha: f32, roughness: f32) -> f32 {
    return alpha * ssr_smoothstep(0.62, 0.14, roughness);
}

// The source's `if ( material.roughness < 0.62 )` is kept: WGSL is exempt from
// the Branchless Law and a shader should say what the GLSL says. The Rust peer
// is branchless because the ramp is already zero at and above the cutoff, which
// `tests::the_roughness_gate_is_redundant_to_the_ramp` pins.
fn ssr_resolve(radiance: vec3<f32>, reflection: vec4<f32>, roughness: f32) -> vec3<f32> {
    if ( roughness < 0.62 ) {
        let a = ssr_clamp(ssr_resolve_weight(reflection.a, roughness), 0.0, 1.0);
        // GLSL `mix( x, y, a )` = `x * ( 1 - a ) + y * a`.
        return radiance * (1.0 - a) + reflection.rgb * a;
    }
    return radiance;
}
"#;

/// **The march pass**: bindings, the full-screen triangle, and the fragment
/// stage. Concatenate after [`SSR_COMMON_WGSL`].
///
/// Every fetch is `textureSampleLevel(…, 0.0)`, never `textureSample`: the march
/// samples inside a loop with `break`s, which is non-uniform control flow, where
/// an implicit-derivative sample is invalid. An explicit LOD of zero is also
/// exactly right — none of these textures has a mip chain.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) const SSR_PASS_WGSL: &str = r#"
struct SsrUniform {
    proj: mat4x4<f32>,
    proj_inv: mat4x4<f32>,
    // x maxDistance  y thickness  z frame  w intensity
    params: vec4<f32>,
    // The source's `uTexel`, which its fragment shader NEVER READS. Carried
    // because dead source is still source, and because it is the blur's
    // `uDirection`.
    texel: vec2<f32>,
    // NOT in the source: the target's size in pixels. WebGL's gl_FragCoord.y
    // counts up from the bottom and @builtin(position).y counts down from the
    // top, so reproducing the source's dither needs the height. It also gives
    // the UV by DIVISION rather than by multiplying by `texel`.
    size: vec2<f32>,
};

@group(0) @binding(0) var<uniform> ssr_u: SsrUniform;
// Nearest + clamp-to-edge: `prepass.js` sets NearestFilter on all three
// G-buffer attachments, and slot 2 is R32Float, which is not filterable.
@group(0) @binding(1) var ssr_point: sampler;
// Linear + clamp-to-edge: `pass.js`'s hdrTarget default, which is how the
// previous resolved frame is read.
@group(0) @binding(2) var ssr_linear: sampler;
@group(0) @binding(3) var ssr_depth_tex: texture_2d<f32>;
@group(0) @binding(4) var ssr_normal_tex: texture_2d<f32>;
@group(0) @binding(5) var ssr_velocity_tex: texture_2d<f32>;
@group(0) @binding(6) var ssr_color_tex: texture_2d<f32>;

// `FS_VERT`'s full-screen TRIANGLE, in the source's corner order: one
// primitive, no diagonal seam, better quad utilisation on a tiled GPU.
@vertex
fn ssr_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

@fragment
fn ssr_fs(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    // `vUv`, computed rather than interpolated: the same value for a full-screen
    // triangle, one interpolator out of the parity measurement.
    let v_uv = frag_coord.xy / ssr_u.size;

    let nrm = textureSampleLevel(ssr_normal_tex, ssr_point, v_uv, 0.0);
    if ( nrm.z < 0.5 ) { return vec4<f32>(0.0, 0.0, 0.0, 0.0); }

    let depth = textureSampleLevel(ssr_depth_tex, ssr_point, v_uv, 0.0).r;
    let P = ssr_view_pos(v_uv, depth, ssr_u.proj_inv);
    let N = ssr_decode_normal(nrm.xy);
    let V = ssr_normalize3(P);
    let R = ssr_reflect(V, N);

    // Rays coming back at the camera cannot be resolved on screen.
    let facing = ssr_clamp(ssr_dot3(-V, R), 0.0, 1.0);
    if ( facing > 0.94 ) { return vec4<f32>(0.0, 0.0, 0.0, 0.0); }

    let maxDist = ssr_u.params.x;
    // WebGL's bottom-up gl_FragCoord, reconstructed.
    let gl_frag_coord = vec2<f32>(frag_coord.x, ssr_u.size.y - frag_coord.y);
    let jitter = ssr_ign(gl_frag_coord + ssr_u.params.z * 7.331);

    let start = P + N * ( 0.02 + depth * 0.002 );
    var t = 0.06 + jitter * 0.06;
    var prevT = t;
    let stepScale = pow( maxDist / 0.06, 1.0 / f32( OW_SSR_STEPS ) );

    var hit = false;
    var hitUv = vec2<f32>(0.0, 0.0);
    var hitDiff = 0.0;

    for ( var i = 0; i < OW_SSR_STEPS; i = i + 1 ) {
        let sp = start + R * t;
        if ( sp.z > -0.05 ) { break; }
        let suv = ssr_project(sp, ssr_u.proj);
        if ( suv.x <= 0.0 || suv.x >= 1.0 || suv.y <= 0.0 || suv.y >= 1.0 ) { break; }

        let sceneDepth = textureSampleLevel(ssr_depth_tex, ssr_point, suv, 0.0).r;
        let cov = textureSampleLevel(ssr_normal_tex, ssr_point, suv, 0.0).z;
        let diff = -sp.z - sceneDepth;

        if ( cov > 0.5 && diff > 0.0 && diff < ssr_u.params.y + t * 0.06 ) {
            // binary refine between prevT and t
            var lo = prevT;
            var hi = t;
            for ( var k = 0; k < OW_SSR_REFINE; k = k + 1 ) {
                let mid = ( lo + hi ) * 0.5;
                let mp = start + R * mid;
                let muv = ssr_project(mp, ssr_u.proj);
                // Deliberately UNCLAMPED, as the source is: clamp-to-edge
                // addressing is what makes an off-screen refine tap defined.
                let md = textureSampleLevel(ssr_depth_tex, ssr_point, muv, 0.0).r;
                if ( -mp.z - md > 0.0 ) { hi = mid; } else { lo = mid; }
            }
            let fp = start + R * hi;
            hitUv = ssr_project(fp, ssr_u.proj);
            hitDiff = diff;
            hit = true;
            break;
        }
        prevT = t;
        t = t * stepScale;
        if ( t > maxDist ) { break; }
    }

    if ( !hit ) { return vec4<f32>(0.0, 0.0, 0.0, 0.0); }

    // Reproject the hit into the previous frame so the colour lines up. The
    // velocity buffer's `y` is negated because it stores an NDC delta (y up)
    // and this is a texture delta (v down) -- see gbuffer::VELOCITY_TEXTURE_V_SIGN.
    let vel_raw = textureSampleLevel(ssr_velocity_tex, ssr_point, hitUv, 0.0).rg;
    let vel = vec2<f32>(vel_raw.x, vel_raw.y * SSR_VELOCITY_V_SIGN);
    let srcUv = vec2<f32>(
        ssr_clamp(hitUv.x - vel.x, 0.001, 0.999),
        ssr_clamp(hitUv.y - vel.y, 0.001, 0.999),
    );
    let color = textureSampleLevel(ssr_color_tex, ssr_linear, srcUv, 0.0).rgb;

    let conf = ssr_confidence(hitUv, facing, t, hitDiff, maxDist, ssr_u.params.y);
    return vec4<f32>(
        max(color, vec3<f32>(0.0, 0.0, 0.0)),
        ssr_clamp(conf, 0.0, 1.0) * ssr_u.params.w,
    );
}
"#;

/// **The separable blur pass**, `SSR_BLUR` from `ssr.js`. Its own bindings, so it
/// compiles as its own module; concatenate after [`SSR_COMMON_WGSL`] only if a
/// caller wants both in one module (it needs nothing from it).
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) const SSR_BLUR_WGSL: &str = r#"
struct SsrBlurUniform {
    // One half-resolution texel along one axis.
    direction: vec2<f32>,
    // The target size, for the same reason the march carries one.
    size: vec2<f32>,
};

@group(0) @binding(0) var<uniform> ssr_blur_u: SsrBlurUniform;
@group(0) @binding(1) var ssr_blur_linear: sampler;
@group(0) @binding(2) var ssr_blur_src: texture_2d<f32>;

@vertex
fn ssr_blur_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

@fragment
fn ssr_blur_fs(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec4<f32> {
    let v_uv = frag_coord.xy / ssr_blur_u.size;
    var sum = textureSampleLevel(ssr_blur_src, ssr_blur_linear, v_uv, 0.0) * 0.4;
    var w = 0.4;
    for ( var i = 1; i <= 2; i = i + 1 ) {
        let wi = 0.3 / f32( i );
        let step_uv = ssr_blur_u.direction * f32( i );
        sum = sum + textureSampleLevel(ssr_blur_src, ssr_blur_linear, v_uv + step_uv, 0.0) * wi;
        sum = sum + textureSampleLevel(ssr_blur_src, ssr_blur_linear, v_uv - step_uv, 0.0) * wi;
        w = w + wi * 2.0;
    }
    return sum / w;
}
"#;

#[cfg(all(test, feature = "offscreen"))]
mod parity;

// `pub(crate)` so the sibling parity modules can reach the synthetic scenes and
// the camera matrices; the same shape `bloom_pyramid::reference` uses.
#[cfg(test)]
pub(crate) mod tests {
    use super::{
        ign, glsl_clamp, glsl_smoothstep, pack_ssr_blur_uniform, pack_ssr_uniform, project_uv,
        ssr_blur_pixel, ssr_confidence, ssr_pixel, ssr_resolve, ssr_resolve_weight,
        ssr_target_size, view_pos, MarchState, ScreenImage, SsrHit, SsrInputs, SsrParams,
        NDC_V_SIGN,
        SSR_BLUR_UNIFORM_FLOATS, SSR_EDGE_FADE, SSR_FACING_CUTOFF, SSR_FRAME_CYCLE,
        SSR_JITTER_FRAME_SCALE, SSR_MAX_DISTANCE, SSR_REFINE, SSR_RESOLUTION_SHIFT,
        SSR_ROUGHNESS_CUTOFF, SSR_ROUGHNESS_FULL, SSR_START_T, SSR_STEPS, SSR_THICKNESS,
        SSR_UNIFORM_FLOATS,
    };

    /// A perspective projection, column-major, matching what a WebGPU-style
    /// camera hands this backend: 60 degrees vertical, 16:9, near 0.1, far 1000,
    /// depth `0..1`.
    ///
    /// Written out rather than built, so the test's geometry cannot move when a
    /// matrix helper elsewhere changes convention.
    pub(crate) fn projection() -> [f32; 16] {
        let f = 1.0_f32 / (60.0_f32.to_radians() * 0.5).tan();
        let aspect = 16.0 / 9.0;
        let near = 0.1_f32;
        let far = 1000.0_f32;
        [
            f / aspect, 0.0, 0.0, 0.0, //
            0.0, f, 0.0, 0.0, //
            0.0, 0.0, far / (near - far), -1.0, //
            0.0, 0.0, near * far / (near - far), 0.0,
        ]
    }

    /// The analytic inverse of [`projection`], also column-major.
    pub(crate) fn projection_inverse() -> [f32; 16] {
        let f = 1.0_f32 / (60.0_f32.to_radians() * 0.5).tan();
        let aspect = 16.0 / 9.0;
        let near = 0.1_f32;
        let far = 1000.0_f32;
        let c = far / (near - far);
        let d = near * far / (near - far);
        [
            aspect / f, 0.0, 0.0, 0.0, //
            0.0, 1.0 / f, 0.0, 0.0, //
            0.0, 0.0, 0.0, 1.0 / d, //
            0.0, 0.0, -1.0, c / d,
        ]
    }

    /// A synthetic G-buffer: a flat wall at `z = -6` filling the frame, its
    /// normal facing the camera, fully covered.
    ///
    /// Deliberately not a "scene": a marching pass is easiest to reason about
    /// against a surface whose depth is known everywhere, and the interesting
    /// cases are then produced by moving one block of it.
    fn wall(width: u32, height: u32, depth: f32) -> ScreenImage {
        ScreenImage::from_fn(width, height, |_, _| [depth, 0.0, 0.0, 0.0])
    }

    /// The normal attachment for [`wall`]: an oct-encoded `+z` view normal
    /// (`owEncodeNormal([0,0,1])` is `[0, 0]`) with full coverage.
    fn wall_normals(width: u32, height: u32) -> ScreenImage {
        ScreenImage::from_fn(width, height, |_, _| [0.0, 0.0, 1.0, 0.0])
    }

    #[test]
    fn the_march_constants_are_the_sources() {
        assert_eq!(SSR_STEPS, 28, "OW_SSR_STEPS");
        assert_eq!(SSR_REFINE, 5, "OW_SSR_REFINE");
        assert_eq!(SSR_FRAME_CYCLE, 64, "frame % 64");
        assert_eq!(SSR_RESOLUTION_SHIFT, 1, "w >> 1");
        assert!(
            (SSR_START_T - 0.06).abs() < f32::EPSILON,
            "the first step is 0.06 m, got {SSR_START_T}"
        );
        assert!(
            (SSR_MAX_DISTANCE - 24.0).abs() < f32::EPSILON
                && ((SSR_THICKNESS - 0.6).abs() < f32::EPSILON),
            "uParams defaults are (24, 0.6, 0, 1), got ({SSR_MAX_DISTANCE}, {SSR_THICKNESS})"
        );
        assert!(
            (SSR_FACING_CUTOFF - 0.94).abs() < f32::EPSILON
                && ((SSR_EDGE_FADE - 0.12).abs() < f32::EPSILON)
                && ((SSR_JITTER_FRAME_SCALE - 7.331).abs() < f32::EPSILON),
            "facing/edge/jitter constants drifted"
        );
        assert!(
            (SSR_ROUGHNESS_CUTOFF - 0.62).abs() < f32::EPSILON
                && ((SSR_ROUGHNESS_FULL - 0.14).abs() < f32::EPSILON),
            "the roughness ramp is smoothstep(0.62, 0.14, r)"
        );
        assert_eq!(NDC_V_SIGN, -1.0, "the WebGPU v flip");
    }

    /// **The last step lands on `maxDistance`.** That is the whole reason the
    /// distribution is geometric rather than linear, and it is the property an
    /// off-by-one in the step count destroys.
    #[test]
    fn the_geometric_step_scale_reaches_max_distance_in_exactly_the_step_count() {
        let scale = (SSR_MAX_DISTANCE / SSR_START_T).powf(1.0 / SSR_STEPS as f32);
        let reached = (0..SSR_STEPS).fold(SSR_START_T, |t, _| t * scale);
        assert!(
            (reached - SSR_MAX_DISTANCE).abs() < 1.0e-3,
            "28 geometric steps from 0.06 must reach 24.0, reached {reached} (scale {scale})"
        );
        assert!(
            (scale - 1.2386).abs() < 1.0e-3,
            "the step scale is pow(400, 1/28) ~= 1.2386, got {scale}"
        );
    }

    /// **The v-flip round-trips.** The CPU↔GPU parity tier cannot prove this —
    /// both sides carry the same flip — so it is proved algebraically instead:
    /// reconstruct a view position from a UV and project it back.
    #[test]
    fn reconstruct_then_project_round_trips_the_uv() {
        let proj = projection();
        let inv = projection_inverse();
        [[0.5, 0.5], [0.1, 0.9], [0.77, 0.23], [0.02, 0.02]]
            .iter()
            .for_each(|uv| {
                let p = view_pos(*uv, 7.5, &inv);
                let back = project_uv(p, &proj);
                assert!(
                    (back[0] - uv[0]).abs() < 1.0e-5 && (back[1] - uv[1]).abs() < 1.0e-5,
                    "uv {uv:?} reconstructed to {p:?} and projected back to {back:?}"
                );
            });
    }

    /// The reconstructed depth is the depth that went in: `owViewPos` returns a
    /// point at exactly `-z = depth`, which is what makes the march's `-sp.z`
    /// comparable to a depth texel.
    #[test]
    fn the_reconstructed_point_carries_the_supplied_view_depth() {
        let inv = projection_inverse();
        [1.0_f32, 6.0, 40.0].iter().for_each(|depth| {
            let p = view_pos([0.3, 0.7], *depth, &inv);
            assert!(
                (-p[2] - depth).abs() < 1.0e-4,
                "depth {depth} reconstructed to z {}",
                p[2]
            );
        });
    }

    #[test]
    fn the_glsl_primitives_match_their_definitions() {
        assert_eq!(glsl_clamp(2.0, 0.0, 1.0), 1.0);
        assert_eq!(glsl_clamp(-2.0, 0.0, 1.0), 0.0);
        assert_eq!(glsl_clamp(0.25, 0.0, 1.0), 0.25);
        assert_eq!(glsl_smoothstep(0.0, 1.0, -1.0), 0.0);
        assert_eq!(glsl_smoothstep(0.0, 1.0, 2.0), 1.0);
        assert_eq!(glsl_smoothstep(0.0, 1.0, 0.5), 0.5);
        // A descending ramp: e0 > e1 is legal and is what the roughness uses.
        assert_eq!(glsl_smoothstep(0.62, 0.14, 0.62), 0.0);
        assert_eq!(glsl_smoothstep(0.62, 0.14, 0.14), 1.0);
    }

    /// `owIGN` is `fract(x) = x - floor(x)`, so it is in `[0, 1)` everywhere —
    /// including at negative arguments, where a `%` would go negative.
    #[test]
    fn the_interleaved_gradient_noise_stays_in_the_unit_interval() {
        let extremes = (0..64)
            .map(|i| ign([i as f32 * 3.5 - 100.0, i as f32 * -7.25 + 40.0]))
            .fold((1.0_f32, 0.0_f32), |(lo, hi), v| (lo.min(v), hi.max(v)));
        assert!(
            extremes.0 >= 0.0 && extremes.1 < 1.0,
            "owIGN left [0, 1): {extremes:?}"
        );
        // A hand-evaluated point, so the two magic constants cannot both drift.
        let d = 10.0_f32 * 0.06711056 + 20.0 * 0.00583715;
        let expected = {
            let inner = d - d.floor();
            let outer = 52.9829189 * inner;
            outer - outer.floor()
        };
        assert_eq!(ign([10.0, 20.0]), expected);
    }

    #[test]
    fn the_image_samplers_clamp_to_the_edge() {
        let image = ScreenImage::from_fn(4, 2, |x, y| [x as f32, y as f32, 0.0, 0.0]);
        assert_eq!(image.width(), 4);
        assert_eq!(image.height(), 2);
        assert_eq!(image.texels().len(), 8);
        // Nearest: floor(uv * dim).
        assert_eq!(image.nearest([0.6, 0.75]), [2.0, 1.0, 0.0, 0.0]);
        // Off both ends, both axes.
        assert_eq!(image.nearest([-5.0, -5.0]), [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(image.nearest([5.0, 5.0]), [3.0, 1.0, 0.0, 0.0]);
        // Bilinear at a texel centre is that texel; halfway is the average.
        assert_eq!(image.bilinear([0.125, 0.25]), [0.0, 0.0, 0.0, 0.0]);
        let mid = image.bilinear([0.25, 0.25]);
        assert!(
            (mid[0] - 0.5).abs() < 1.0e-6,
            "halfway between texels 0 and 1 is 0.5, got {}",
            mid[0]
        );
        // Bilinear off the edge clamps too.
        assert_eq!(image.bilinear([-1.0, -1.0]), [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(image.bilinear([2.0, 2.0]), [3.0, 1.0, 0.0, 0.0]);
    }

    /// An uncovered pixel is a black pixel, colour **and** alpha, so the
    /// material's `mix` leaves the cubemap alone.
    #[test]
    fn an_uncovered_pixel_returns_black() {
        let depth = wall(8, 8, 6.0);
        let normal = ScreenImage::from_fn(8, 8, |_, _| [0.0, 0.0, 0.0, 0.0]);
        let velocity = ScreenImage::from_fn(8, 8, |_, _| [0.0; 4]);
        let color = ScreenImage::from_fn(8, 8, |_, _| [1.0, 1.0, 1.0, 1.0]);
        let proj = projection();
        let inv = projection_inverse();
        let inputs = SsrInputs {
            depth: &depth,
            normal: &normal,
            velocity: &velocity,
            color: &color,
            proj: &proj,
            proj_inv: &inv,
        };
        assert_eq!(
            ssr_pixel(&inputs, SsrParams::at_frame(0), [4.5, 4.5], [8.0, 8.0]),
            [0.0; 4]
        );
    }

    /// A wall facing the camera reflects straight back at it, so `dot(-V, R)`
    /// exceeds the cutoff and the pass refuses to march. This is exit 2, and it
    /// is the exit that fires over most of a head-on frame.
    #[test]
    fn a_head_on_surface_is_rejected_by_the_facing_cutoff() {
        let depth = wall(8, 8, 6.0);
        let normal = wall_normals(8, 8);
        let velocity = ScreenImage::from_fn(8, 8, |_, _| [0.0; 4]);
        let color = ScreenImage::from_fn(8, 8, |_, _| [1.0, 0.5, 0.25, 1.0]);
        let proj = projection();
        let inv = projection_inverse();
        let inputs = SsrInputs {
            depth: &depth,
            normal: &normal,
            velocity: &velocity,
            color: &color,
            proj: &proj,
            proj_inv: &inv,
        };
        // uv (0.5625, 0.5625): near enough to the axis that V is within a few
        // degrees of (0, 0, -1), so R is within a few degrees of (0, 0, 1) and
        // `dot(-V, R)` is ~0.958 — over the 0.94 cutoff.
        assert_eq!(
            ssr_pixel(&inputs, SsrParams::at_frame(3), [4.5, 4.5], [8.0, 8.0]),
            [0.0; 4]
        );
    }

    /// A mirror **floor** in front of a **back wall**, both as real planes.
    ///
    /// The depth buffer is built by intersecting each pixel's view ray with the
    /// two planes and keeping the nearer, so the scene is *geometrically
    /// consistent*: a ray reflected off the floor climbs away from it and never
    /// re-crosses it, and the only surface it can strike is the wall. A
    /// hand-painted depth ramp does not have that property — its gradient
    /// disagrees with its own normals and the march "hits" the floor it just
    /// left, which would make every assertion below pass for the wrong reason.
    ///
    /// - floor: the view-space plane `y = -1.5`, normal `(0, 1, 0)`
    /// - wall: the view-space plane `z = -12`, normal `(0, 0, 1)`
    ///
    /// `owEncodeNormal([0,1,0])` is `[0, 1]`; `owEncodeNormal([0,0,1])` is
    /// `[0, 0]`.
    pub(super) fn floor_scene() -> (ScreenImage, ScreenImage, ScreenImage, ScreenImage) {
        const SIZE: u32 = 32;
        const FLOOR_Y: f32 = -1.5;
        const WALL_DEPTH: f32 = 12.0;
        let inv = projection_inverse();
        // The depth of the nearer plane at pixel (x, y), and whether it is the
        // floor. `view_pos(uv, 1.0, inv)` is the unit ray through the pixel,
        // normalised to `-z = 1`, so a parameter along it *is* a view depth.
        let surface = move |x: u32, y: u32| -> (f32, bool) {
            let uv = [
                (x as f32 + 0.5) / SIZE as f32,
                (y as f32 + 0.5) / SIZE as f32,
            ];
            let ray = view_pos(uv, 1.0, &inv);
            let floor_depth = FLOOR_Y / ray[1];
            let hits_floor = ray[1] < 0.0 && floor_depth < WALL_DEPTH;
            match hits_floor {
                true => (floor_depth, true),
                false => (WALL_DEPTH, false),
            }
        };
        let depth = ScreenImage::from_fn(SIZE, SIZE, |x, y| [surface(x, y).0, 0.0, 0.0, 0.0]);
        let normal = ScreenImage::from_fn(SIZE, SIZE, |x, y| {
            let oct_y = match surface(x, y).1 {
                true => 1.0,
                false => 0.0,
            };
            [0.0, oct_y, 1.0, 0.0]
        });
        let velocity = ScreenImage::from_fn(SIZE, SIZE, |_, _| [0.0; 4]);
        let color = ScreenImage::from_fn(SIZE, SIZE, |x, y| {
            [x as f32 * 0.03, y as f32 * 0.02, 0.5, 1.0]
        });
        (depth, normal, velocity, color)
    }

    /// The scene is what it claims: the lower rows are floor, the upper rows are
    /// wall, and the floor recedes with height. If this drifts, every march
    /// assertion below is testing a different picture than its name says.
    #[test]
    fn the_floor_scene_is_a_floor_in_front_of_a_wall() {
        let (depth, normal, _, _) = floor_scene();
        let row_depth = |y: u32| depth.nearest([0.5, (y as f32 + 0.5) / 32.0])[0];
        let is_floor = |y: u32| normal.nearest([0.5, (y as f32 + 0.5) / 32.0])[1] > 0.5;
        assert!(is_floor(31), "the bottom row must be floor");
        assert!(!is_floor(0), "the top row must be wall");
        assert!(
            row_depth(31) < row_depth(24) && row_depth(24) < 12.0,
            "the floor must recede upward and stay in front of the wall: {} then {}",
            row_depth(31),
            row_depth(24)
        );
        assert_eq!(row_depth(0), 12.0, "the wall is at 12 m");
    }

    /// The march is *bounded*: whatever it finds or fails to find, the alpha it
    /// returns is in `[0, intensity]` and the colour is non-negative. Driven over
    /// a whole synthetic frame so every exit is taken somewhere.
    #[test]
    fn the_marched_frame_is_bounded_everywhere() {
        let (depth, normal, velocity, color) = floor_scene();
        let proj = projection();
        let inv = projection_inverse();
        let inputs = SsrInputs {
            depth: &depth,
            normal: &normal,
            velocity: &velocity,
            color: &color,
            proj: &proj,
            proj_inv: &inv,
        };
        let params = SsrParams::at_frame(11);
        let worst = (0..32)
            .flat_map(|y| (0..32).map(move |x| (x, y)))
            .map(|(x, y)| {
                ssr_pixel(
                    &inputs,
                    params,
                    [x as f32 + 0.5, y as f32 + 0.5],
                    [32.0, 32.0],
                )
            })
            .fold((0.0_f32, 0.0_f32), |(max_alpha, min_channel), texel| {
                (
                    max_alpha.max(texel[3]),
                    min_channel.min(texel[0].min(texel[1]).min(texel[2])),
                )
            });
        assert!(
            worst.0 <= params.intensity + 1.0e-6,
            "alpha exceeded the intensity: {}",
            worst.0
        );
        assert!(
            worst.1 >= 0.0,
            "max(color, 0) let a negative channel through: {}",
            worst.1
        );
    }

    /// **The mirror floor actually reflects.** If nothing on a wet-road-shaped
    /// scene ever returns alpha, the pass is a no-op and every other assertion
    /// here is vacuous — this is the test that says the march resolves a hit.
    #[test]
    fn the_mirror_floor_finds_the_wall() {
        let (depth, normal, velocity, color) = floor_scene();
        let proj = projection();
        let inv = projection_inverse();
        let inputs = SsrInputs {
            depth: &depth,
            normal: &normal,
            velocity: &velocity,
            color: &color,
            proj: &proj,
            proj_inv: &inv,
        };
        let hits = (16..32)
            .flat_map(|y| (0..32).map(move |x| (x, y)))
            .filter(|(x, y)| {
                ssr_pixel(
                    &inputs,
                    SsrParams::at_frame(0),
                    [*x as f32 + 0.5, *y as f32 + 0.5],
                    [32.0, 32.0],
                )[3] > 0.0
            })
            .count();
        assert!(
            hits > 0,
            "the mirror floor resolved no reflection at all; the march never hits"
        );
    }

    /// The dither moves with the frame, which is what a temporal filter resolves.
    /// If it did not, the jitter would be a fixed pattern burnt into the frame.
    #[test]
    fn the_dither_depends_on_the_frame() {
        let a = SsrParams::at_frame(0);
        let b = SsrParams::at_frame(1);
        assert_ne!(a.frame, b.frame);
        // `frame % 64`.
        assert_eq!(SsrParams::at_frame(64).frame, 0.0);
        assert_eq!(SsrParams::at_frame(65).frame, 1.0);
        let ja = ign([12.5 + a.frame * SSR_JITTER_FRAME_SCALE, 8.5 + a.frame * SSR_JITTER_FRAME_SCALE]);
        let jb = ign([12.5 + b.frame * SSR_JITTER_FRAME_SCALE, 8.5 + b.frame * SSR_JITTER_FRAME_SCALE]);
        assert_ne!(ja, jb, "the jitter is identical on two frames");
    }

    #[test]
    fn the_confidence_fades_at_every_edge_it_should() {
        let full = ssr_confidence([0.5, 0.5], 0.0, 1.0, 0.0, 24.0, 0.6);
        assert!((full - 1.0).abs() < 1.0e-6, "a central, near, thin, grazing hit is fully confident, got {full}");
        // Screen border.
        assert_eq!(ssr_confidence([0.0, 0.5], 0.0, 1.0, 0.0, 24.0, 0.6), 0.0);
        assert_eq!(ssr_confidence([0.5, 1.0], 0.0, 1.0, 0.0, 24.0, 0.6), 0.0);
        // Facing.
        assert_eq!(ssr_confidence([0.5, 0.5], 0.94, 1.0, 0.0, 24.0, 0.6), 0.0);
        // Distance.
        assert_eq!(ssr_confidence([0.5, 0.5], 0.0, 24.0, 0.0, 24.0, 0.6), 0.0);
        // Thickness.
        assert_eq!(ssr_confidence([0.5, 0.5], 0.0, 1.0, 0.6, 24.0, 0.6), 0.0);
        // And it is monotone between: half the border distance is a partial fade.
        let partial = ssr_confidence([0.06, 0.5], 0.0, 1.0, 0.0, 24.0, 0.6);
        assert!(
            partial > 0.0 && partial < 1.0,
            "the border fade is a ramp, not a step: {partial}"
        );
    }

    /// The blur's weights sum to `1.3` and it normalises by that, so a flat input
    /// comes out unchanged — the property a mis-transcribed `wi` breaks.
    #[test]
    fn the_blur_preserves_a_flat_image() {
        let src = ScreenImage::from_fn(16, 16, |_, _| [0.25, 0.5, 0.75, 1.0]);
        let out = ssr_blur_pixel(&src, [0.5, 0.5], [1.0 / 16.0, 0.0]);
        [0, 1, 2, 3].iter().for_each(|lane| {
            assert!(
                (out[*lane] - src.texels()[0][*lane]).abs() < 1.0e-6,
                "lane {lane} changed on a flat image: {out:?}"
            );
        });
    }

    /// A single bright texel spreads, and the centre keeps the largest share
    /// (`0.4 / 1.3`). Written as a ratio so it pins the weight *table*, not just
    /// "something blurred".
    #[test]
    fn the_blur_spreads_an_impulse_at_the_sources_weights() {
        let src = ScreenImage::from_fn(16, 1, |x, _| {
            [[0.0_f32, 1.0][usize::from(x == 8)], 0.0, 0.0, 0.0]
        });
        let texel = 1.0 / 16.0;
        let at = |x: u32| ssr_blur_pixel(&src, [(x as f32 + 0.5) * texel, 0.5], [texel, 0.0])[0];
        let centre = at(8);
        assert!(
            (centre - 0.4 / 1.3).abs() < 1.0e-5,
            "the centre tap is 0.4/1.3, got {centre}"
        );
        assert!(
            (at(9) - 0.3 / 1.3).abs() < 1.0e-5,
            "the +-1 tap is 0.3/1.3, got {}",
            at(9)
        );
        assert!(
            (at(10) - 0.15 / 1.3).abs() < 1.0e-5,
            "the +-2 tap is 0.15/1.3, got {}",
            at(10)
        );
        assert_eq!(at(7), at(9), "the blur is symmetric");
    }

    /// **The roughness gate is redundant to the ramp.** This is what licenses the
    /// branchless Rust: at and above `0.62` the smoothstep is exactly zero, so
    /// the source's `if` cannot change a value — only skip a fetch.
    #[test]
    fn the_roughness_gate_is_redundant_to_the_ramp() {
        (0..64)
            .map(|i| SSR_ROUGHNESS_CUTOFF + i as f32 * 0.01)
            .for_each(|roughness| {
                assert_eq!(
                    ssr_resolve_weight(1.0, roughness),
                    0.0,
                    "the ramp must be exactly zero at roughness {roughness}"
                );
            });
    }

    #[test]
    fn the_resolve_replaces_the_ibl_specular_rather_than_adding_to_it() {
        let radiance = [1.0, 1.0, 1.0];
        // A mirror with full confidence: the reflection wins outright.
        let mirror = ssr_resolve(radiance, [0.0, 0.25, 0.5, 1.0], 0.0);
        assert_eq!(mirror, [0.0, 0.25, 0.5]);
        // A rough surface: unchanged, and NOT brightened.
        assert_eq!(ssr_resolve(radiance, [9.0, 9.0, 9.0, 1.0], 0.8), radiance);
        // Halfway up the ramp: a genuine blend, never a sum.
        let blended = ssr_resolve(radiance, [0.0, 0.0, 0.0, 1.0], 0.38);
        assert!(
            blended[0] > 0.0 && blended[0] < 1.0,
            "the midpoint of the ramp is a blend, got {blended:?}"
        );
        // Zero alpha (a marched miss) leaves the radiance exactly alone.
        assert_eq!(ssr_resolve(radiance, [5.0, 5.0, 5.0, 0.0], 0.1), radiance);
    }

    #[test]
    fn the_uniform_blocks_pack_in_the_declared_order() {
        let proj = projection();
        let inv = projection_inverse();
        let packed = pack_ssr_uniform(
            &proj,
            &inv,
            SsrParams::at_frame(7),
            [1.0 / 960.0, 1.0 / 540.0],
            [960.0, 540.0],
        );
        assert_eq!(packed.len(), SSR_UNIFORM_FLOATS);
        assert_eq!(&packed[0..16], &proj);
        assert_eq!(&packed[16..32], &inv);
        assert_eq!(packed[32], SSR_MAX_DISTANCE);
        assert_eq!(packed[33], SSR_THICKNESS);
        assert_eq!(packed[34], 7.0);
        assert_eq!(packed[35], 1.0);
        assert_eq!(packed[36], 1.0 / 960.0);
        assert_eq!(packed[38], 960.0);
        assert_eq!(packed[39], 540.0);
        let blur = pack_ssr_blur_uniform([1.0 / 960.0, 0.0], [960.0, 540.0]);
        assert_eq!(blur.len(), SSR_BLUR_UNIFORM_FLOATS);
        assert_eq!(blur, [1.0 / 960.0, 0.0, 960.0, 540.0]);
    }

    /// The value types name themselves and compare as values.
    ///
    /// The same shape `gbuffer::tests` uses on `GBufferChannel`: a derived
    /// `Debug` on a data type is part of what makes a marching pass debuggable at
    /// all — a failing parity assertion prints these — and a derived `PartialEq`
    /// is a claim that two of these are the same run, which two different frames
    /// are not.
    #[test]
    fn the_value_types_report_themselves_and_compare_as_values() {
        let params = SsrParams::at_frame(3);
        let rendered = format!("{params:?}");
        assert!(
            rendered.contains("max_distance") && rendered.contains("thickness"),
            "SsrParams must name its lanes: {rendered}"
        );
        assert_ne!(
            params,
            SsrParams::at_frame(4),
            "two frames are not the same parameters"
        );
        assert_eq!(params, SsrParams::at_frame(3));

        let image = ScreenImage::from_fn(2, 1, |x, _| [x as f32, 0.0, 0.0, 0.0]);
        assert_eq!(image.clone(), image, "a cloned image is the same image");
        assert_ne!(image, ScreenImage::from_fn(2, 1, |_, _| [9.0; 4]));
        assert!(format!("{image:?}").contains("ScreenImage"));

        let depth = wall(2, 1, 5.0);
        let normal = wall_normals(2, 1);
        let proj = projection();
        let inv = projection_inverse();
        let inputs = SsrInputs {
            depth: &depth,
            normal: &normal,
            velocity: &depth,
            color: &normal,
            proj: &proj,
            proj_inv: &inv,
        };
        assert!(
            format!("{inputs:?}").contains("proj_inv"),
            "SsrInputs must name the matrices it marches with"
        );

        let state = MarchState {
            t: 0.5,
            prev_t: 0.25,
        };
        assert!(format!("{state:?}").contains("prev_t"));
        let hit = SsrHit {
            uv: [0.5, 0.5],
            diff: 0.1,
            t: 2.0,
        };
        assert!(format!("{hit:?}").contains("diff"));
        assert_ne!(
            hit,
            SsrHit {
                t: 3.0,
                ..hit
            },
            "two hits at different distances are different hits"
        );
    }

    #[test]
    fn the_target_is_half_resolution_and_never_zero() {
        assert_eq!(ssr_target_size(1920, 1080), (960, 540));
        assert_eq!(ssr_target_size(1, 1), (1, 1), "Math.max(1, w >> 1)");
        assert_eq!(ssr_target_size(0, 0), (1, 1));
    }
}
