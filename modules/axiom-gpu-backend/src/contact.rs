//! **Screen-space contact shadows**: a short ray marched through the G-buffer's
//! depth toward the sun, putting back the last few centimetres of occlusion that
//! a cascaded shadow map always loses.
//!
//! Ported from `C:/dev/Claude-of-Duty/src/render/contact.js` (168 lines) — both
//! of its shaders (the march and the depth-aware bilateral that follows it) —
//! plus the four lines of `src/render/materialpatch.js` that consume the result,
//! because *which light this multiplies onto* is part of this pass's contract.
//!
//! # Why the pass exists at all
//!
//! A cascade texel at 40 m is wider than the gap between a crate and the ground.
//! However well the cascade is filtered, that gap contains no shadow, and without
//! one every prop in the frame reads as a sticker laid on the floor rather than
//! an object resting on it. This marches fourteen steps along the sun direction
//! through the depth buffer and puts the missing contact back.
//!
//! It is consumed **multiplied onto the sun term only** — never onto ambient,
//! never onto another light. The ray runs along one direction, so it says nothing
//! about any other light's occlusion, and applying it to one would be inventing
//! shadow. [`contact_shadow_for_light`] is that gate.
//!
//! # The two early exits, and what each writes
//!
//! The target is `Rg16Float`: `r` is the shadow multiplier, `g` is the linear
//! view depth the bilateral blur uses as its edge-stopping signal. Both exits
//! write a shadow of `1.0` (fully lit) and differ only in the depth they publish:
//!
//! 1. **No coverage** (`nrm.z < 0.5`) writes [`CONTACT_UNCOVERED_DEPTH`] — `1e4`.
//!    Not a placeholder: it is a *sentinel* that drives the bilateral's
//!    exponential weight to zero, so a covered pixel next to the sky never
//!    averages the sky's shadow value into its own. See
//!    [`tests::the_uncovered_sentinel_annihilates_the_bilateral_weight`].
//! 2. **Facing away from the sun** (`NdL <= 0.02`) writes the real depth. This
//!    pixel is already fully shadowed by the `N·L` term in the material; marching
//!    it would spend fourteen samples to compute a number nothing multiplies.
//!
//! # Storage width is part of the algorithm
//!
//! - The march and the blur run at **full** resolution (unlike
//!   [`crate::ssr`]'s half), into `THREE.HalfFloatType` / `THREE.RGFormat` —
//!   `Rg16Float`.
//! - `1e4` is exactly representable in `f16` (the spacing at 8192 is 8, and
//!   `10000 / 8` is an integer), so the sentinel survives the store intact. A
//!   sentinel that did not would be a slow leak of sky into geometry.
//! - The depth lane is a *half*, so at 40 m its resolution is `0.03125` m. The
//!   bilateral's edge test is therefore coarse by construction, which is why its
//!   exponent is scaled by `40 / max( 0.1, depth )` rather than being an absolute
//!   threshold.
//!
//! # What this module shares with [`crate::ssr`], and what it deliberately does
//! not
//!
//! `glsl.js`'s `COMMON` is textually inlined into **both** passes by the source's
//! `${COMMON}`, so the WGSL here carries its own `contact_ign`,
//! `contact_view_pos`, `contact_project` and `contact_decode_normal` — a second,
//! independently written transcription of the same GLSL.
//!
//! The **Rust** reference does the opposite: it calls [`crate::ssr::ign`],
//! [`crate::ssr::view_pos`] and [`crate::ssr::project_uv`]. That is deliberate,
//! and it is the arrangement that buys the most checking for the least code. The
//! risk this port keeps paying for is *one author writing both the Rust and the
//! "independent" transcription meant to check it*; two Rust copies by the same
//! hand would not reduce that risk at all, while two **languages** compared on
//! real hardware do. So the cross-check that matters — this module's WGSL against
//! `ssr`'s Rust — is exactly what this module's `parity` submodule runs.
//!
//! [`crate::ssr::ScreenImage`] is shared for the plainer reason that it is
//! harness scaffolding rather than transcribed source: two samplers that disagree
//! about clamp-to-edge would be a defect with no upside.
//!
//! The same expiry applies as in `ssr`: when a third consumer of `COMMON` lands
//! (`gtao.js`, `taa.js`, `motionblur.js` all include it), lift `ign`/`view_pos`/
//! `project_uv` into `modules/axiom-gpu-backend/src/gbuffer.rs`, beside
//! [`crate::gbuffer::decode_normal`], whose own documentation already argues the
//! case.
//!
//! # Transcription notes
//!
//! - `occ = max( occ, 1.0 - t * t )` is followed immediately by `break`, and
//!   `occ` is `0.0` on every path that reaches it, so the `max` can never
//!   choose its first argument. **Dead computation in the source is still part of
//!   the source**: it is transcribed, and named as dead here rather than
//!   silently dropped.
//! - `NdL` is computed, used as a gate, and never used again — it does **not**
//!   scale the occlusion. Transcribed as written.
//! - The loop's `continue` on `cov < 0.5` is *not* a `break`: an uncovered texel
//!   along the ray is skipped and the march carries on past it. Collapsing the
//!   two would stop every ray at the first sliver of sky it crosses.
//! - `bias = 0.004 + sceneDepth * 0.0025` uses the **scene's** depth at the
//!   sample, not the shading point's.
//! - `stepV = L * ( len / float( OW_CS_STEPS ) )` — the division is inside the
//!   parentheses, so the step is computed once and scaled, never `L * len / 14`.
//! - The bilateral's exponent is
//!   `-abs( a.g - c.g ) * 40.0 / max( 0.1, c.g )`: negate, then multiply, then
//!   divide — three operations whose grouping is the specification.
//! - `materialpatch.js` fetches with `gl_FragCoord.xy * owScreenTexel`, a genuine
//!   **reciprocal-multiply in the source**. Not to be tidied into a division.
//!
//! The two WebGPU adaptations — the NDC `v` flip and WebGL's bottom-up
//! `gl_FragCoord` — are [`crate::ssr`]'s, described in that module's header, and
//! both apply here unchanged.

use crate::gbuffer::decode_normal;
use crate::ssr::{glsl_clamp, ign, project_uv, view_pos, ScreenImage};

/// `#define OW_CS_STEPS 14` — the march's step count. **This is the algorithm.**
/// It divides the ray length as well as bounding the loop, so changing it
/// changes both the reach and the sample spacing.
pub(crate) const CONTACT_STEPS: i32 = 14;

/// `uParams.x`: the world-space ray length in metres at 1x distance scaling,
/// from `new THREE.Vector4( 0.4, 0.42, 0, 1.0 )`.
///
/// The source states what this number is for, and the statement is worth keeping
/// because it is the whole justification for the pass: `0.40` m with
/// [`contact_ray_length`]'s ramp spans roughly `0.30 .. 1.0` m of world travel,
/// which is what puts the last few centimetres of occlusion back.
pub(crate) const CONTACT_LENGTH: f32 = 0.4;

/// `uParams.y`: the thickness window in metres. A depth difference above
/// [`CONTACT_BIAS_BASE`] and below this counts as an occluder; anything thicker
/// is a *different surface* further away, not something casting onto this one.
pub(crate) const CONTACT_THICKNESS: f32 = 0.42;

/// `uParams.w`: how much of the sun term a full contact hit removes, `0..1`.
pub(crate) const CONTACT_STRENGTH: f32 = 1.0;

/// `uParams.value.z = frame % 64` — the dither's temporal cycle length.
pub(crate) const CONTACT_FRAME_CYCLE: u32 = 64;

/// The dither's per-frame offset: `owIGN( gl_FragCoord.xy + uParams.z * 3.1717 )`.
/// A different constant from [`crate::ssr::SSR_JITTER_FRAME_SCALE`] on purpose —
/// two passes sharing one dither offset would correlate their noise and defeat
/// the temporal filter that resolves both.
pub(crate) const CONTACT_JITTER_FRAME_SCALE: f32 = 3.1717;

/// `if ( NdL <= 0.02 )` — at or below this the surface faces away from the sun
/// and the march is skipped.
pub(crate) const CONTACT_NDL_CUTOFF: f32 = 0.02;

/// The depth an uncovered pixel publishes: `vec4( 1.0, 1e4, 0.0, 1.0 )`. A
/// sentinel, not a placeholder — see the module header.
pub(crate) const CONTACT_UNCOVERED_DEPTH: f32 = 1.0e4;

/// `bias = 0.004 + sceneDepth * 0.0025`, the constant term.
pub(crate) const CONTACT_BIAS_BASE: f32 = 0.004;

/// `bias = 0.004 + sceneDepth * 0.0025`, the per-metre term. A self-hit at 40 m
/// therefore needs 10.4 cm of separation before it counts, which is roughly the
/// depth quantisation at that range.
pub(crate) const CONTACT_BIAS_PER_METRE: f32 = 0.0025;

/// The origin's surface offset: `P + N * ( 0.012 + depth * 0.0015 )`, constant
/// term.
pub(crate) const CONTACT_ORIGIN_BIAS_BASE: f32 = 0.012;

/// The origin's surface offset, per-metre term.
pub(crate) const CONTACT_ORIGIN_BIAS_PER_METRE: f32 = 0.0015;

/// `dot( lightDirView, owSunDirView ) < 0.999` returns `1.0` — this pass applies
/// to the sun and to nothing else. `0.999` is about 2.6 degrees, which separates
/// the sun from any other directional light without being so tight that a
/// renormalisation rounding excludes the sun from its own shadow.
pub(crate) const CONTACT_SUN_DOT_THRESHOLD: f32 = 0.999;

/// The bilateral's depth-difference falloff: `exp( -|Δd| * 40.0 / max( 0.1, d ) )`.
pub(crate) const CONTACT_BILATERAL_FALLOFF: f32 = 40.0;

/// The floor under the bilateral's depth normaliser, `max( 0.1, c.g )`. Without
/// it, a pixel one centimetre from the near plane divides by nearly zero and
/// every neighbour's weight collapses.
pub(crate) const CONTACT_BILATERAL_DEPTH_FLOOR: f32 = 0.1;

/// `uParams` for the march: `x length(m)  y thickness(m)  z frame  w strength`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ContactParams {
    /// `uParams.x`, metres at 1x distance scaling. Settable at runtime through
    /// the source's `setLength`.
    pub(crate) length: f32,
    /// `uParams.y`, metres.
    pub(crate) thickness: f32,
    /// `uParams.z`, `frame % 64` as a float.
    pub(crate) frame: f32,
    /// `uParams.w`, `0..1`. Settable at runtime through the source's
    /// `setStrength`.
    pub(crate) strength: f32,
}

impl ContactParams {
    /// The source's constructor defaults, with `frame` supplied and reduced
    /// modulo [`CONTACT_FRAME_CYCLE`] — `uParams.value.z = frame % 64` is the
    /// source's line, and doing it here is what stops a caller losing the
    /// dither's cycle.
    pub(crate) fn at_frame(frame: u32) -> ContactParams {
        ContactParams {
            length: CONTACT_LENGTH,
            thickness: CONTACT_THICKNESS,
            frame: (frame % CONTACT_FRAME_CYCLE) as f32,
            strength: CONTACT_STRENGTH,
        }
    }

    /// `setLength( m )` — the world-space ray length in metres at 1x.
    pub(crate) fn with_length(self, metres: f32) -> ContactParams {
        ContactParams {
            length: metres,
            ..self
        }
    }

    /// `setStrength( s )` — `0..1`, how much of the sun term a full hit removes.
    pub(crate) fn with_strength(self, strength: f32) -> ContactParams {
        ContactParams { strength, ..self }
    }
}

/// The two textures the march reads, plus the camera pair and the sun.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ContactInputs<'a> {
    /// G-buffer slot 2: linear view depth in metres, positive, `R32Float`.
    /// Nearest-sampled.
    pub(crate) depth: &'a ScreenImage,
    /// G-buffer slot 0: oct **view** normal in `xy`, coverage in `z`,
    /// `Rgba16Float`. Nearest-sampled.
    pub(crate) normal: &'a ScreenImage,
    /// `uProj`, column-major.
    pub(crate) proj: &'a [f32; 16],
    /// `uProjInv`, column-major.
    pub(crate) proj_inv: &'a [f32; 16],
    /// `uSunDirView`: the direction **toward** the sun, in view space,
    /// **normalised**. The march scales it by `len / 14` to get its step, so a
    /// non-unit vector silently rescales the ray length and the pass reaches the
    /// wrong distance.
    pub(crate) sun_dir_view: [f32; 3],
}

/// `len = uParams.x * clamp( depth * 0.08 + 0.75, 0.75, 2.5 )` — the ray grows
/// with distance, because a metre of world at 40 m is a handful of pixels and a
/// fixed-length ray would sample nothing there.
///
/// At the default `0.4` this spans exactly `0.30 m` near the camera to `1.0 m`
/// beyond `21.875 m`, which is the range the source's own comment claims;
/// [`tests::the_distance_ramp_spans_the_range_the_source_claims`] pins both ends.
pub(crate) fn contact_ray_length(base_length: f32, depth: f32) -> f32 {
    base_length * glsl_clamp(depth * 0.08 + 0.75, 0.75, 2.5)
}

/// The march: `OW_CS_STEPS` evenly spaced samples along the sun direction,
/// stopping at the first occluder inside the thickness window.
///
/// Written as a `try_fold` whose `Err` carries the occlusion, because the Rust
/// spine is branchless. The source's three control-flow shapes are all present
/// and all distinct:
///
/// - `break` when the sample leaves the screen — the ray is gone, stop.
/// - `continue` when the sample lands on an uncovered texel — skip it and keep
///   going. **Not** a `break`.
/// - `break` with an occlusion when the depth difference is inside the window.
///
/// The order is the source's: a sample that has left the screen cannot record an
/// occlusion even though its clamped fetch returns a value.
fn march(
    inputs: &ContactInputs<'_>,
    params: ContactParams,
    origin: [f32; 3],
    step_v: [f32; 3],
    jitter: f32,
) -> f32 {
    (0..CONTACT_STEPS)
        .try_fold(0.0_f32, |occ, i| {
            let travelled = i as f32 + jitter;
            let sp = [0, 1, 2].map(|axis| origin[axis] + step_v[axis] * travelled);
            let suv = project_uv(sp, inputs.proj);
            // `if ( suv.x <= 0.0 || suv.x >= 1.0 || suv.y <= 0.0 || suv.y >= 1.0 ) break;`
            let off_screen =
                (suv[0] <= 0.0) | (suv[0] >= 1.0) | (suv[1] <= 0.0) | (suv[1] >= 1.0);

            let scene_depth = inputs.depth.nearest(suv)[0];
            let cov = inputs.normal.nearest(suv)[2];
            // `if ( cov < 0.5 ) continue;`
            let skipped = cov < 0.5;

            let diff = -sp[2] - scene_depth;
            let bias = CONTACT_BIAS_BASE + scene_depth * CONTACT_BIAS_PER_METRE;
            // `if ( diff > bias && diff < uParams.y )`
            let occluded = (diff > bias) & (diff < params.thickness);

            // fade with distance travelled so the shadow dissolves rather than
            // ends. The `max` is the source's and is DEAD — `occ` is 0.0 on
            // every path that reaches this line, and the line is followed by a
            // break.
            let t = travelled / CONTACT_STEPS as f32;
            let faded = occ.max(1.0 - t * t);

            let records = occluded & !skipped & !off_screen;
            let stop = off_screen | records;
            [Ok(occ), Err([occ, faded][usize::from(records)])][usize::from(stop)]
        })
        .unwrap_or_else(|stopped| stopped)
}

/// **The pass, for one pixel** — the semantic definition the WGSL is checked
/// against.
///
/// `frag_coord` is `@builtin(position).xy`, i.e. `(x + 0.5, y + 0.5)` with `y`
/// counting **down**; `target_size` is the target's size in pixels. Both are
/// needed because the source's dither is a function of WebGL's bottom-up
/// `gl_FragCoord`, reconstructed as `size.y - position.y`.
///
/// Returns the source's `vec4( shadow, depth, 0.0, 1.0 )`. Only the first two
/// lanes reach the `Rg16Float` target; the other two are the source's literals
/// and are returned so this reference and the GLSL read the same.
pub(crate) fn contact_pixel(
    inputs: &ContactInputs<'_>,
    params: ContactParams,
    frag_coord: [f32; 2],
    target_size: [f32; 2],
) -> [f32; 4] {
    let v_uv = [frag_coord[0] / target_size[0], frag_coord[1] / target_size[1]];

    let nrm = inputs.normal.nearest(v_uv);
    // `if ( nrm.z < 0.5 ) { gl_FragColor = vec4( 1.0, 1e4, 0.0, 1.0 ); return; }`
    let covered = nrm[2] >= 0.5;

    let depth = inputs.depth.nearest(v_uv)[0];
    let p = view_pos(v_uv, depth, inputs.proj_inv);
    let n = decode_normal([nrm[0], nrm[1]]);
    let l = inputs.sun_dir_view;

    let ndl = n[0] * l[0] + n[1] * l[1] + n[2] * l[2];
    // `if ( NdL <= 0.02 ) { gl_FragColor = vec4( 1.0, depth, 0.0, 1.0 ); return; }`
    let lit = ndl > CONTACT_NDL_CUTOFF;

    let len = contact_ray_length(params.length, depth);
    let gl_frag_coord = [frag_coord[0], target_size[1] - frag_coord[1]];
    let offset = params.frame * CONTACT_JITTER_FRAME_SCALE;
    let jitter = ign([gl_frag_coord[0] + offset, gl_frag_coord[1] + offset]);

    let origin_bias = CONTACT_ORIGIN_BIAS_BASE + depth * CONTACT_ORIGIN_BIAS_PER_METRE;
    let origin = [0, 1, 2].map(|axis| p[axis] + n[axis] * origin_bias);
    // The division is INSIDE the parentheses: one step, then scaled.
    let step_length = len / CONTACT_STEPS as f32;
    let step_v = [0, 1, 2].map(|axis| l[axis] * step_length);

    let occ = march(inputs, params, origin, step_v, jitter);
    let shadow = 1.0 - occ * params.strength;

    // Three outcomes, in the source's order: uncovered, unlit, marched.
    let choice = usize::from(covered) + usize::from(covered & lit);
    [
        [1.0, CONTACT_UNCOVERED_DEPTH, 0.0, 1.0],
        [1.0, depth, 0.0, 1.0],
        [shadow, depth, 0.0, 1.0],
    ][choice]
}

/// **The depth-aware bilateral**, for one pixel:
///
/// ```glsl
/// vec2 c = texture2D( tSrc, vUv ).rg;
/// float sum = c.r * 0.5;
/// float wsum = 0.5;
/// for ( int i = 1; i <= 2; i ++ ) {
///   vec2 o = uDirection * float( i );
///   vec2 a = texture2D( tSrc, vUv + o ).rg;
///   vec2 b = texture2D( tSrc, vUv - o ).rg;
///   float w = 0.3 / float( i );
///   float wa = w * exp( -abs( a.g - c.g ) * 40.0 / max( 0.1, c.g ) );
///   float wb = w * exp( -abs( b.g - c.g ) * 40.0 / max( 0.1, c.g ) );
///   sum += a.r * wa + b.r * wb;
///   wsum += wa + wb;
/// }
/// gl_FragColor = vec4( sum / wsum, c.g, 0.0, 1.0 );
/// ```
///
/// Note that the centre weight is `0.5` here and `0.4` in [`crate::ssr`]'s blur;
/// they are different filters and the numbers are not interchangeable. Note also
/// that `sum += a.r * wa + b.r * wb` accumulates **one** term whose two products
/// are added first — `sum + (a * wa + b * wb)`, not `(sum + a * wa) + b * wb` —
/// and that `wsum += wa + wb` does the same.
///
/// The depth lane is passed through unfiltered, so a chain of two of these keeps
/// its own edge-stopping signal exact.
pub(crate) fn contact_blur_pixel(
    src: &ScreenImage,
    uv: [f32; 2],
    direction: [f32; 2],
) -> [f32; 4] {
    let c = src.bilinear(uv);
    let normaliser = c[1].max(CONTACT_BILATERAL_DEPTH_FLOOR);
    let (sum, wsum) = (1..=2_i32).fold((c[0] * 0.5, 0.5_f32), |(sum, wsum), i| {
        let o = [direction[0] * i as f32, direction[1] * i as f32];
        let a = src.bilinear([uv[0] + o[0], uv[1] + o[1]]);
        let b = src.bilinear([uv[0] - o[0], uv[1] - o[1]]);
        let w = 0.3 / i as f32;
        let wa = w * ((-(a[1] - c[1]).abs()) * CONTACT_BILATERAL_FALLOFF / normaliser).exp();
        let wb = w * ((-(b[1] - c[1]).abs()) * CONTACT_BILATERAL_FALLOFF / normaliser).exp();
        (sum + (a[0] * wa + b[0] * wb), wsum + (wa + wb))
    });
    [sum / wsum, c[1], 0.0, 1.0]
}

/// **How the material consumes this pass**, from `materialpatch.js`:
///
/// ```glsl
/// float owContactShadow( vec3 lightDirView ) {
///   if ( owFeat.y < 0.5 ) return 1.0;
///   if ( dot( lightDirView, owSunDirView ) < 0.999 ) return 1.0;
///   return texture2D( owContactTex, gl_FragCoord.xy * owScreenTexel ).r;
/// }
/// ```
///
/// It lives here and not in the material's module because *which light this
/// multiplies onto* is a property of the pass: the ray runs along one direction,
/// so the term is meaningless for any other light, and a port that dropped the
/// `0.999` test would silently darken every point light in the frame by the sun's
/// contact shadow.
///
/// `enabled` is `owFeat.y > 0.5` — the frame graph sets it when the pass ran at
/// all — and `sampled` is the `r` lane already fetched. The fetch itself stays in
/// the material, where its UV is `gl_FragCoord.xy * owScreenTexel`, a genuine
/// **reciprocal-multiply in the source**.
pub(crate) fn contact_shadow_for_light(enabled: bool, dot_light_sun: f32, sampled: f32) -> f32 {
    let applies = enabled & (dot_light_sun >= CONTACT_SUN_DOT_THRESHOLD);
    [1.0, sampled][usize::from(applies)]
}

/// Floats in the march's `ContactUniform` block: `proj` (16) + `proj_inv` (16) +
/// `sun_dir_view` as a padded `vec4` (4) + `params` (4) + `size` (2) + tail
/// padding (2) = 44, i.e. 176 bytes.
///
/// The sun is a `vec4` rather than a `vec3` because a `vec3` in the uniform
/// address space is 16-byte aligned but 12 bytes wide, and the following `vec4`
/// would then sit at an offset the Rust packer and the WGSL layout have to agree
/// about implicitly. Making the padding explicit costs one float and removes a
/// class of silent mis-binding.
pub(crate) const CONTACT_UNIFORM_FLOATS: usize = 44;

/// Pack the march's uniform block. `sun_dir_view` must be **normalised**; see
/// [`ContactInputs::sun_dir_view`].
pub(crate) fn pack_contact_uniform(
    proj: &[f32; 16],
    proj_inv: &[f32; 16],
    sun_dir_view: [f32; 3],
    params: ContactParams,
    size: [f32; 2],
) -> [f32; CONTACT_UNIFORM_FLOATS] {
    let mut out = [0.0_f32; CONTACT_UNIFORM_FLOATS];
    out[0..16].copy_from_slice(proj);
    out[16..32].copy_from_slice(proj_inv);
    out[32..35].copy_from_slice(&sun_dir_view);
    out[36] = params.length;
    out[37] = params.thickness;
    out[38] = params.frame;
    out[39] = params.strength;
    out[40] = size[0];
    out[41] = size[1];
    out
}

/// Floats in the bilateral's uniform block: `direction` (2) + `size` (2).
pub(crate) const CONTACT_BLUR_UNIFORM_FLOATS: usize = 4;

/// Pack the bilateral's uniform block. `direction` is `( texel.x, 0 )` on the
/// horizontal pass and `( 0, texel.y )` on the vertical one, in that order.
pub(crate) fn pack_contact_blur_uniform(
    direction: [f32; 2],
    size: [f32; 2],
) -> [f32; CONTACT_BLUR_UNIFORM_FLOATS] {
    [direction[0], direction[1], size[0], size[1]]
}

/// **The pure half of the pass's WGSL** — a second, independently written
/// transcription of `glsl.js`'s `COMMON` (the source inlines it into every
/// screen-space pass), plus this pass's own arithmetic. No bindings, no entry
/// points, so the parity harness can compile exactly these functions with its own
/// entry points.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) const CONTACT_COMMON_WGSL: &str = r#"
// ---------------------------------------------------------------------------
// glsl.js COMMON. Transcribed here a second time because `${COMMON}` is inlined
// into every pass in the source, and because a WGSL transcription compared
// against `crate::ssr`'s RUST reference is a genuinely independent check.
// ---------------------------------------------------------------------------

// The WebGPU texture-v to clip-y sign. See crate::ssr's module header.
const CONTACT_NDC_V_SIGN: f32 = -1.0;

fn contact_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    return min(max(x, lo), hi);
}

fn contact_dot3(a: vec3<f32>, b: vec3<f32>) -> f32 {
    return a.x * b.x + a.y * b.y + a.z * b.z;
}

fn contact_normalize3(v: vec3<f32>) -> vec3<f32> {
    return v / sqrt(contact_dot3(v, v));
}

// `owIGN`: fract( 52.9829189 * fract( dot( p, vec2( 0.06711056, 0.00583715 ) ) ) )
fn contact_ign(p: vec2<f32>) -> f32 {
    let inner = fract(p.x * 0.06711056 + p.y * 0.00583715);
    return fract(52.9829189 * inner);
}

// `owDecodeNormal`.
fn contact_decode_normal(f: vec2<f32>) -> vec3<f32> {
    let nz = 1.0 - abs(f.x) - abs(f.y);
    let t = max(-nz, 0.0);
    let nx = f.x + select(t, -t, f.x >= 0.0);
    let ny = f.y + select(t, -t, f.y >= 0.0);
    return contact_normalize3(vec3<f32>(nx, ny, nz));
}

// `owViewPos( uv, depth, projInv )`, with the NDC `y` negated.
fn contact_view_pos(uv: vec2<f32>, depth: f32, proj_inv: mat4x4<f32>) -> vec3<f32> {
    let ndc = uv * 2.0 - 1.0;
    let h = proj_inv * vec4<f32>(ndc.x, ndc.y * CONTACT_NDC_V_SIGN, 1.0, 1.0);
    var dir = h.xyz / h.w;
    dir = dir / max(1e-6, -dir.z);
    return dir * depth;
}

// `clip.xy / clip.w * 0.5 + 0.5`, with the same `y` flip.
fn contact_project(p: vec3<f32>, proj: mat4x4<f32>) -> vec2<f32> {
    let clip = proj * vec4<f32>(p, 1.0);
    return vec2<f32>(clip.x, clip.y * CONTACT_NDC_V_SIGN) / clip.w * 0.5 + 0.5;
}

// ---------------------------------------------------------------------------
// contact.js
// ---------------------------------------------------------------------------

const OW_CS_STEPS: i32 = 14;

// `len = uParams.x * clamp( depth * 0.08 + 0.75, 0.75, 2.5 )`
fn contact_ray_length(base_length: f32, depth: f32) -> f32 {
    return base_length * contact_clamp( depth * 0.08 + 0.75, 0.75, 2.5 );
}

// `materialpatch.js`'s owContactShadow, minus the fetch (which stays there) and
// with the feature bit passed in. The source's two `if`s are kept: WGSL is
// exempt from the Branchless Law and a shader should say what the GLSL says.
fn contact_shadow_for_light(enabled: f32, dot_light_sun: f32, sampled: f32) -> f32 {
    if ( enabled < 0.5 ) { return 1.0; }
    if ( dot_light_sun < 0.999 ) { return 1.0; }
    return sampled;
}
"#;

/// **The march pass**: bindings, the full-screen triangle, and the fragment
/// stage. Concatenate after [`CONTACT_COMMON_WGSL`].
///
/// The fragment returns `vec2<f32>`, not the source's `vec4`. That is the same
/// divergence [`crate::gbuffer`] states for its velocity slot: three.js declares
/// every output `vec4` and lets the render target's channel count discard the
/// rest, while a WGSL fragment output must match the attachment's component
/// count. The values written are the same numbers; the discarded `vec4( …, 0.0,
/// 1.0 )` tail is a literal in both.
///
/// Every fetch is `textureSampleLevel(…, 0.0)`: the march samples inside a loop
/// with `break`/`continue`, which is non-uniform control flow, where an
/// implicit-derivative sample is invalid.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) const CONTACT_PASS_WGSL: &str = r#"
struct ContactUniform {
    proj: mat4x4<f32>,
    proj_inv: mat4x4<f32>,
    // `uSunDirView` in xyz; w is padding so `params` lands at a known offset.
    sun_dir_view: vec4<f32>,
    // x length(m)  y thickness(m)  z frame  w strength
    params: vec4<f32>,
    // NOT in the source: the target's size. WebGL's gl_FragCoord.y counts up
    // from the bottom, @builtin(position).y counts down from the top, and the
    // dither is a function of that coordinate. It also gives the UV by DIVISION.
    size: vec2<f32>,
    pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> contact_u: ContactUniform;
// Nearest + clamp-to-edge: `prepass.js` sets NearestFilter on both attachments,
// and the depth slot is R32Float, which is not filterable.
@group(0) @binding(1) var contact_point: sampler;
@group(0) @binding(2) var contact_depth_tex: texture_2d<f32>;
@group(0) @binding(3) var contact_normal_tex: texture_2d<f32>;

@vertex
fn contact_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

@fragment
fn contact_fs(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec2<f32> {
    let v_uv = frag_coord.xy / contact_u.size;

    let nrm = textureSampleLevel(contact_normal_tex, contact_point, v_uv, 0.0);
    // `vec4( 1.0, 1e4, 0.0, 1.0 )` -- 1e4 is a SENTINEL that annihilates the
    // bilateral's weight, not a placeholder.
    if ( nrm.z < 0.5 ) { return vec2<f32>(1.0, 1e4); }

    let depth = textureSampleLevel(contact_depth_tex, contact_point, v_uv, 0.0).r;
    let P = contact_view_pos(v_uv, depth, contact_u.proj_inv);
    let N = contact_decode_normal(nrm.xy);
    let L = contact_u.sun_dir_view.xyz;

    // NdL gates the march and is not used again. Transcribed as written.
    let NdL = contact_dot3(N, L);
    if ( NdL <= 0.02 ) { return vec2<f32>(1.0, depth); }

    let len = contact_ray_length(contact_u.params.x, depth);
    // WebGL's bottom-up gl_FragCoord, reconstructed.
    let gl_frag_coord = vec2<f32>(frag_coord.x, contact_u.size.y - frag_coord.y);
    let jitter = contact_ign(gl_frag_coord + contact_u.params.z * 3.1717);

    let origin = P + N * ( 0.012 + depth * 0.0015 );
    let stepV = L * ( len / f32( OW_CS_STEPS ) );

    var occ = 0.0;
    for ( var i = 0; i < OW_CS_STEPS; i = i + 1 ) {
        let sp = origin + stepV * ( f32( i ) + jitter );
        let suv = contact_project(sp, contact_u.proj);
        if ( suv.x <= 0.0 || suv.x >= 1.0 || suv.y <= 0.0 || suv.y >= 1.0 ) { break; }

        let sceneDepth = textureSampleLevel(contact_depth_tex, contact_point, suv, 0.0).r;
        let cov = textureSampleLevel(contact_normal_tex, contact_point, suv, 0.0).z;
        // CONTINUE, not break: an uncovered texel along the ray is stepped over.
        if ( cov < 0.5 ) { continue; }

        let diff = -sp.z - sceneDepth;
        let bias = 0.004 + sceneDepth * 0.0025;
        if ( diff > bias && diff < contact_u.params.y ) {
            // fade with distance travelled so the shadow dissolves rather than ends
            let t = ( f32( i ) + jitter ) / f32( OW_CS_STEPS );
            // The `max` is the source's and is DEAD: occ is 0.0 here, always.
            occ = max( occ, 1.0 - t * t );
            break;
        }
    }

    let shadow = 1.0 - occ * contact_u.params.w;
    // The source writes `vec4( shadow, depth, 0.0, 1.0 )` into an RG target; a
    // WGSL output must match the attachment's two components.
    return vec2<f32>(shadow, depth);
}
"#;

/// **The depth-aware bilateral pass**, `BILATERAL` from `contact.js`. Its own
/// bindings, so it compiles as its own module.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) const CONTACT_BLUR_WGSL: &str = r#"
struct ContactBlurUniform {
    direction: vec2<f32>,
    size: vec2<f32>,
};

@group(0) @binding(0) var<uniform> contact_blur_u: ContactBlurUniform;
@group(0) @binding(1) var contact_blur_linear: sampler;
@group(0) @binding(2) var contact_blur_src: texture_2d<f32>;

@vertex
fn contact_blur_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

@fragment
fn contact_blur_fs(@builtin(position) frag_coord: vec4<f32>) -> @location(0) vec2<f32> {
    let v_uv = frag_coord.xy / contact_blur_u.size;
    let c = textureSampleLevel(contact_blur_src, contact_blur_linear, v_uv, 0.0).rg;
    var sum = c.r * 0.5;
    var wsum = 0.5;
    for ( var i = 1; i <= 2; i = i + 1 ) {
        let o = contact_blur_u.direction * f32( i );
        let a = textureSampleLevel(contact_blur_src, contact_blur_linear, v_uv + o, 0.0).rg;
        let b = textureSampleLevel(contact_blur_src, contact_blur_linear, v_uv - o, 0.0).rg;
        let w = 0.3 / f32( i );
        let wa = w * exp( -abs( a.g - c.g ) * 40.0 / max( 0.1, c.g ) );
        let wb = w * exp( -abs( b.g - c.g ) * 40.0 / max( 0.1, c.g ) );
        sum = sum + ( a.r * wa + b.r * wb );
        wsum = wsum + ( wa + wb );
    }
    // The source writes `vec4( sum / wsum, c.g, 0.0, 1.0 )` into an RG target.
    return vec2<f32>(sum / wsum, c.g);
}
"#;

#[cfg(all(test, feature = "offscreen"))]
mod parity;

// `pub(crate)` so the sibling parity modules can reach the synthetic scenes and
// the camera matrices; the same shape `bloom_pyramid::reference` uses.
#[cfg(test)]
pub(crate) mod tests {
    use super::{
        contact_blur_pixel, contact_pixel, contact_ray_length, contact_shadow_for_light,
        pack_contact_blur_uniform, pack_contact_uniform, ContactInputs, ContactParams,
        CONTACT_BIAS_BASE, CONTACT_BIAS_PER_METRE, CONTACT_BILATERAL_DEPTH_FLOOR,
        CONTACT_BILATERAL_FALLOFF, CONTACT_BLUR_UNIFORM_FLOATS, CONTACT_FRAME_CYCLE,
        CONTACT_JITTER_FRAME_SCALE, CONTACT_LENGTH, CONTACT_NDL_CUTOFF,
        CONTACT_ORIGIN_BIAS_BASE, CONTACT_ORIGIN_BIAS_PER_METRE, CONTACT_STEPS,
        CONTACT_STRENGTH, CONTACT_SUN_DOT_THRESHOLD, CONTACT_THICKNESS,
        CONTACT_UNCOVERED_DEPTH, CONTACT_UNIFORM_FLOATS,
    };
    use crate::ssr::tests::{projection, projection_inverse};
    use crate::ssr::ScreenImage;

    /// A flat wall at `depth` metres filling the frame, facing the camera, fully
    /// covered — the baseline the occluder tests perturb.
    ///
    /// `owEncodeNormal([0, 0, 1])` is `[0, 0]`, so the oct lanes are zero and the
    /// coverage lane is `1.0`.
    pub(super) fn flat_scene(size: u32, depth: f32) -> (ScreenImage, ScreenImage) {
        (
            ScreenImage::from_fn(size, size, |_, _| [depth, 0.0, 0.0, 0.0]),
            ScreenImage::from_fn(size, size, |_, _| [0.0, 0.0, 1.0, 0.0]),
        )
    }

    /// The baseline wall with a **step** in it: the left half is `depth` metres
    /// away and the right half is `depth - step` — a slab standing proud of the
    /// wall, whose edge is exactly the contact a cascade cannot resolve.
    ///
    /// The sun is placed marching left-to-right in screen space, so a pixel just
    /// left of the step marches into the slab and is occluded, while a pixel far
    /// from it is not.
    pub(super) fn stepped_scene(size: u32, depth: f32, step: f32) -> (ScreenImage, ScreenImage) {
        (
            ScreenImage::from_fn(size, size, |x, _| {
                [[depth, depth - step][usize::from(x >= size / 2)], 0.0, 0.0, 0.0]
            }),
            ScreenImage::from_fn(size, size, |_, _| [0.0, 0.0, 1.0, 0.0]),
        )
    }

    /// A sun direction in view space, normalised, tilted mostly along `+x` with
    /// enough `+z` that `N·L` clears the cutoff on a camera-facing wall.
    pub(super) fn sun() -> [f32; 3] {
        let v = [0.94_f32, 0.12, 0.32];
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / len, v[1] / len, v[2] / len]
    }

    #[test]
    fn the_march_constants_are_the_sources() {
        assert_eq!(CONTACT_STEPS, 14, "OW_CS_STEPS");
        assert_eq!(CONTACT_FRAME_CYCLE, 64, "frame % 64");
        assert!(
            (CONTACT_LENGTH - 0.4).abs() < f32::EPSILON
                && ((CONTACT_THICKNESS - 0.42).abs() < f32::EPSILON)
                && ((CONTACT_STRENGTH - 1.0).abs() < f32::EPSILON),
            "uParams defaults are (0.4, 0.42, 0, 1.0), got ({CONTACT_LENGTH}, \
             {CONTACT_THICKNESS}, {CONTACT_STRENGTH})"
        );
        assert!(
            (CONTACT_JITTER_FRAME_SCALE - 3.1717).abs() < f32::EPSILON,
            "the dither offset is 3.1717, got {CONTACT_JITTER_FRAME_SCALE}"
        );
        assert!(
            (CONTACT_NDL_CUTOFF - 0.02).abs() < f32::EPSILON
                && ((CONTACT_BIAS_BASE - 0.004).abs() < f32::EPSILON)
                && ((CONTACT_BIAS_PER_METRE - 0.0025).abs() < f32::EPSILON)
                && ((CONTACT_ORIGIN_BIAS_BASE - 0.012).abs() < f32::EPSILON)
                && ((CONTACT_ORIGIN_BIAS_PER_METRE - 0.0015).abs() < f32::EPSILON),
            "a bias constant drifted"
        );
        assert_eq!(CONTACT_UNCOVERED_DEPTH, 1.0e4);
        assert!(
            (CONTACT_SUN_DOT_THRESHOLD - 0.999).abs() < f32::EPSILON
                && ((CONTACT_BILATERAL_FALLOFF - 40.0).abs() < f32::EPSILON)
                && ((CONTACT_BILATERAL_DEPTH_FLOOR - 0.1).abs() < f32::EPSILON),
            "a consumption or bilateral constant drifted"
        );
    }

    /// **The distance ramp spans the range the source's own comment claims** —
    /// "roughly 0.30 .. 1.0 m of world travel". Both ends and the clamp on each
    /// side of them.
    #[test]
    fn the_distance_ramp_spans_the_range_the_source_claims() {
        assert_eq!(contact_ray_length(CONTACT_LENGTH, 0.0), 0.4 * 0.75);
        // Below the lower clamp the length cannot shrink further.
        assert_eq!(
            contact_ray_length(CONTACT_LENGTH, -100.0),
            contact_ray_length(CONTACT_LENGTH, 0.0)
        );
        // The ramp saturates at depth * 0.08 + 0.75 == 2.5, i.e. 21.875 m.
        let saturated = contact_ray_length(CONTACT_LENGTH, 21.875);
        assert!(
            (saturated - 1.0).abs() < 1.0e-6,
            "the ramp must reach 1.0 m at 21.875 m, got {saturated}"
        );
        assert_eq!(contact_ray_length(CONTACT_LENGTH, 1000.0), saturated);
        // And it is monotone in between.
        let middle = contact_ray_length(CONTACT_LENGTH, 10.0);
        assert!(
            middle > 0.3 && middle < 1.0,
            "the ramp is monotone between its clamps, got {middle} at 10 m"
        );
    }

    /// An uncovered pixel publishes the sentinel depth, fully lit.
    #[test]
    fn an_uncovered_pixel_publishes_the_sentinel_depth() {
        let (depth, _) = flat_scene(8, 6.0);
        let normal = ScreenImage::from_fn(8, 8, |_, _| [0.0, 0.0, 0.0, 0.0]);
        let proj = projection();
        let inv = projection_inverse();
        let inputs = ContactInputs {
            depth: &depth,
            normal: &normal,
            proj: &proj,
            proj_inv: &inv,
            sun_dir_view: sun(),
        };
        assert_eq!(
            contact_pixel(&inputs, ContactParams::at_frame(0), [4.5, 4.5], [8.0, 8.0]),
            [1.0, CONTACT_UNCOVERED_DEPTH, 0.0, 1.0]
        );
    }

    /// A surface facing away from the sun is skipped, and publishes its **real**
    /// depth — not the sentinel, because the bilateral must still be able to
    /// smooth across it.
    #[test]
    fn a_surface_facing_away_from_the_sun_is_skipped_but_keeps_its_depth() {
        let (depth, normal) = flat_scene(8, 6.0);
        let proj = projection();
        let inv = projection_inverse();
        // The camera-facing wall's normal is +z; a sun at -z is behind it.
        let inputs = ContactInputs {
            depth: &depth,
            normal: &normal,
            proj: &proj,
            proj_inv: &inv,
            sun_dir_view: [0.0, 0.0, -1.0],
        };
        assert_eq!(
            contact_pixel(&inputs, ContactParams::at_frame(0), [4.5, 4.5], [8.0, 8.0]),
            [1.0, 6.0, 0.0, 1.0]
        );
        // Exactly at the cutoff is also skipped: the source's test is `<=`.
        let grazing = ContactInputs {
            sun_dir_view: [
                (1.0_f32 - CONTACT_NDL_CUTOFF * CONTACT_NDL_CUTOFF).sqrt(),
                0.0,
                CONTACT_NDL_CUTOFF,
            ],
            ..inputs
        };
        assert_eq!(
            contact_pixel(&grazing, ContactParams::at_frame(0), [4.5, 4.5], [8.0, 8.0]),
            [1.0, 6.0, 0.0, 1.0]
        );
    }

    /// **An unoccluded lit surface is fully lit.** A flat wall has nothing to
    /// cast onto itself, so if the bias is wrong this test goes dark — it is the
    /// self-shadowing acne check, and the reason the origin bias exists.
    #[test]
    fn a_flat_lit_wall_casts_no_shadow_on_itself() {
        let (depth, normal) = flat_scene(16, 6.0);
        let proj = projection();
        let inv = projection_inverse();
        let inputs = ContactInputs {
            depth: &depth,
            normal: &normal,
            proj: &proj,
            proj_inv: &inv,
            sun_dir_view: sun(),
        };
        let params = ContactParams::at_frame(5);
        let darkest = (0..16)
            .flat_map(|y| (0..16).map(move |x| (x, y)))
            .map(|(x, y)| {
                contact_pixel(&inputs, params, [x as f32 + 0.5, y as f32 + 0.5], [16.0, 16.0])[0]
            })
            .fold(1.0_f32, f32::min);
        assert_eq!(
            darkest, 1.0,
            "a flat wall shadowed itself; the origin bias or the depth bias is wrong"
        );
    }

    /// **A step in the depth buffer casts a contact shadow**, and only on the
    /// side the sun comes from. If this finds nothing the pass is a no-op.
    #[test]
    fn a_step_in_the_depth_buffer_casts_a_contact_shadow() {
        let (depth, normal) = stepped_scene(32, 6.0, 0.2);
        let proj = projection();
        let inv = projection_inverse();
        let inputs = ContactInputs {
            depth: &depth,
            normal: &normal,
            proj: &proj,
            proj_inv: &inv,
            // Marching toward +x in view space, so a pixel on the FAR (left)
            // half walks rightward into the near slab and is occluded by it.
            // The reverse direction would walk away from the step and find
            // nothing, which is the easiest way to write a test that passes
            // because it proves nothing.
            sun_dir_view: [0.94, 0.0, 0.341_46],
        };
        let params = ContactParams::at_frame(2);
        let row = |x: u32| {
            contact_pixel(&inputs, params, [x as f32 + 0.5, 16.5], [32.0, 32.0])[0]
        };
        let near_step = (8..16).map(row).fold(1.0_f32, f32::min);
        assert!(
            near_step < 1.0,
            "no pixel beside the step was occluded; the march finds nothing"
        );
        assert!(
            near_step >= 0.0,
            "the shadow multiplier went negative: {near_step}"
        );
        // Far from the step, on the lit side of it, nothing is occluded: the ray
        // is at most 0.3 m long and the step is many pixels away.
        assert_eq!(
            row(0),
            1.0,
            "a pixel a long way from the step was shadowed anyway"
        );
    }

    /// Strength scales the removal linearly, and zero strength disables the pass
    /// without disabling the march (the depth lane still publishes).
    #[test]
    fn strength_scales_the_occlusion_linearly() {
        let (depth, normal) = stepped_scene(32, 6.0, 0.2);
        let proj = projection();
        let inv = projection_inverse();
        let inputs = ContactInputs {
            depth: &depth,
            normal: &normal,
            proj: &proj,
            proj_inv: &inv,
            sun_dir_view: [0.94, 0.0, 0.341_46],
        };
        let at = |params: ContactParams| {
            (8..16)
                .map(|x| contact_pixel(&inputs, params, [x as f32 + 0.5, 16.5], [32.0, 32.0])[0])
                .fold(1.0_f32, f32::min)
        };
        let full = at(ContactParams::at_frame(2));
        let half = at(ContactParams::at_frame(2).with_strength(0.5));
        let none = at(ContactParams::at_frame(2).with_strength(0.0));
        assert_eq!(none, 1.0, "zero strength must remove nothing");
        assert!(
            (half - (1.0 + full) * 0.5).abs() < 1.0e-6,
            "half strength must remove half the occlusion: full {full}, half {half}"
        );
        // A longer ray reaches further and cannot find less.
        let longer = at(ContactParams::at_frame(2).with_length(1.2));
        assert!(
            longer <= full,
            "a longer ray found less occlusion: {longer} vs {full}"
        );
    }

    #[test]
    fn the_dither_depends_on_the_frame() {
        assert_eq!(ContactParams::at_frame(64).frame, 0.0);
        assert_eq!(ContactParams::at_frame(65).frame, 1.0);
        assert_ne!(
            ContactParams::at_frame(0).frame,
            ContactParams::at_frame(1).frame
        );
    }

    /// The bilateral leaves a flat buffer alone, and passes the depth lane
    /// through untouched.
    #[test]
    fn the_bilateral_preserves_a_flat_buffer() {
        let src = ScreenImage::from_fn(16, 16, |_, _| [0.6, 7.0, 0.0, 1.0]);
        let out = contact_blur_pixel(&src, [0.5, 0.5], [1.0 / 16.0, 0.0]);
        assert!(
            (out[0] - 0.6).abs() < 1.0e-6,
            "a flat shadow buffer changed: {out:?}"
        );
        assert_eq!(out[1], 7.0, "the depth lane must pass through unfiltered");
        assert_eq!(out[2], 0.0);
        assert_eq!(out[3], 1.0);
    }

    /// **The sentinel annihilates the bilateral's weight**, which is the whole
    /// reason `1e4` is written rather than `0.0`. A covered pixel next to
    /// uncovered sky must not average the sky's shadow into its own.
    #[test]
    fn the_uncovered_sentinel_annihilates_the_bilateral_weight() {
        // A 1-D strip: texel 8 is real geometry at 6 m with a dark shadow; every
        // other texel is "sky" carrying the sentinel and a shadow of 1.0.
        let src = ScreenImage::from_fn(16, 1, |x, _| {
            let real = x == 8;
            [
                [1.0_f32, 0.0][usize::from(real)],
                [CONTACT_UNCOVERED_DEPTH, 6.0][usize::from(real)],
                0.0,
                1.0,
            ]
        });
        let out = contact_blur_pixel(&src, [8.5 / 16.0, 0.5], [1.0 / 16.0, 0.0]);
        assert!(
            out[0] < 1.0e-6,
            "the sky's shadow of 1.0 leaked into a covered pixel: {out:?}"
        );
        assert_eq!(out[1], 6.0, "the centre keeps its own depth");
    }

    /// With no depth difference at all the bilateral degenerates to the plain
    /// weights `0.5, 0.3, 0.3, 0.15, 0.15`, normalised by `1.4`. That pins the
    /// weight table independently of the exponential.
    #[test]
    fn the_bilateral_weights_are_the_sources_when_no_edge_separates_them() {
        let src = ScreenImage::from_fn(16, 1, |x, _| {
            [[0.0_f32, 1.0][usize::from(x == 8)], 6.0, 0.0, 1.0]
        });
        let texel = 1.0 / 16.0;
        let at = |x: u32| contact_blur_pixel(&src, [(x as f32 + 0.5) * texel, 0.5], [texel, 0.0])[0];
        assert!(
            (at(8) - 0.5 / 1.4).abs() < 1.0e-5,
            "the centre tap is 0.5/1.4, got {}",
            at(8)
        );
        assert!(
            (at(9) - 0.3 / 1.4).abs() < 1.0e-5,
            "the +-1 tap is 0.3/1.4, got {}",
            at(9)
        );
        assert!(
            (at(10) - 0.15 / 1.4).abs() < 1.0e-5,
            "the +-2 tap is 0.15/1.4, got {}",
            at(10)
        );
        assert_eq!(at(7), at(9), "the bilateral is symmetric on a flat depth");
    }

    /// The consumption gate: this term multiplies onto the **sun** and nothing
    /// else, and only when the pass ran.
    #[test]
    fn the_contact_term_reaches_only_the_sun() {
        assert_eq!(contact_shadow_for_light(true, 1.0, 0.3), 0.3);
        assert_eq!(
            contact_shadow_for_light(true, CONTACT_SUN_DOT_THRESHOLD, 0.3),
            0.3,
            "exactly at the threshold the source's `< 0.999` is false, so it applies"
        );
        assert_eq!(
            contact_shadow_for_light(true, 0.998, 0.3),
            1.0,
            "a light 3.6 degrees off the sun must not receive the sun's contact shadow"
        );
        assert_eq!(
            contact_shadow_for_light(false, 1.0, 0.3),
            1.0,
            "with the feature off the term is identity"
        );
    }

    /// The value types name themselves and compare as values — the same shape
    /// `gbuffer::tests` uses on `GBufferChannel`, and for the same reason: a
    /// failing parity assertion prints these.
    #[test]
    fn the_value_types_report_themselves_and_compare_as_values() {
        let params = ContactParams::at_frame(3);
        let rendered = format!("{params:?}");
        assert!(
            rendered.contains("length") && rendered.contains("strength"),
            "ContactParams must name its lanes: {rendered}"
        );
        assert_eq!(params, ContactParams::at_frame(3));
        assert_ne!(
            params,
            params.with_strength(0.5),
            "two strengths are not the same parameters"
        );
        assert_ne!(params, params.with_length(0.9));

        let (depth, normal) = flat_scene(2, 5.0);
        let proj = projection();
        let inv = projection_inverse();
        let inputs = ContactInputs {
            depth: &depth,
            normal: &normal,
            proj: &proj,
            proj_inv: &inv,
            sun_dir_view: sun(),
        };
        assert!(
            format!("{inputs:?}").contains("sun_dir_view"),
            "ContactInputs must name the sun it marches toward"
        );
    }

    #[test]
    fn the_uniform_blocks_pack_in_the_declared_order() {
        let proj = projection();
        let inv = projection_inverse();
        let packed = pack_contact_uniform(
            &proj,
            &inv,
            [0.0, 1.0, 0.0],
            ContactParams::at_frame(13),
            [1920.0, 1080.0],
        );
        assert_eq!(packed.len(), CONTACT_UNIFORM_FLOATS);
        assert_eq!(&packed[0..16], &proj);
        assert_eq!(&packed[16..32], &inv);
        assert_eq!(&packed[32..35], &[0.0, 1.0, 0.0]);
        assert_eq!(packed[35], 0.0, "the sun's vec4 pad lane");
        assert_eq!(packed[36], CONTACT_LENGTH);
        assert_eq!(packed[37], CONTACT_THICKNESS);
        assert_eq!(packed[38], 13.0);
        assert_eq!(packed[39], CONTACT_STRENGTH);
        assert_eq!(packed[40], 1920.0);
        assert_eq!(packed[41], 1080.0);
        assert_eq!(&packed[42..44], &[0.0, 0.0], "the tail pad lanes");
        let blur = pack_contact_blur_uniform([0.0, 1.0 / 1080.0], [1920.0, 1080.0]);
        assert_eq!(blur.len(), CONTACT_BLUR_UNIFORM_FLOATS);
        assert_eq!(blur, [0.0, 1.0 / 1080.0, 1920.0, 1080.0]);
    }
}
