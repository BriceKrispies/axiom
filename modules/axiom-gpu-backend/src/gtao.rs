//! **Ground-Truth Ambient Occlusion** (Jimenez et al. 2016) — the visibility-arc
//! integral, transcribed from `src/render/gtao.js` (324 lines).
//!
//! This is the pass that makes the reference image look *grounded*. Compare
//! `docs/work-manifests/shmup-port/reference/original-street.png` with
//! `axiom-street-agx.png`: the reference darkens in every corner, under every
//! ledge, and in the seam where each prop meets the ground; Axiom does not, and
//! its props read as decals floating a centimetre above the road. Nothing else
//! in the frame graph supplies that term — the hemisphere ambient in
//! [`crate::scene_wgsl`]'s suffix is a per-normal constant and cannot know that
//! there is a wall two centimetres away.
//!
//! # This is GTAO, not SSAO, and the difference is the integral
//!
//! A hemisphere-sample SSAO asks *"how many of N random points around me are
//! behind geometry"* and averages a count. GTAO asks a geometric question and
//! answers it in closed form: for each of a few **slices** — planes containing
//! the view vector — it finds the two **horizon angles** the depth buffer
//! actually exhibits, and integrates the cosine-weighted visible arc between
//! them analytically ([`reference::arc`]). The count is replaced by an integral,
//! which is why three slices of eight steps beats thirty-two random taps, and
//! why the constants are not interchangeable with an SSAO's.
//!
//! Every one of those constants is transcribed, and they are listed in this
//! module rather than left inline because *this is where the look lives*:
//!
//! | | value | source |
//! |---|---|---|
//! | slices | **3** ([`SLICES`]) | `#define OW_SLICES 3` |
//! | steps per slice per side | **8** ([`STEPS`]) | `#define OW_STEPS 8` |
//! | world radius | **1.35 m** ([`SHIPPED_RADIUS_METRES`]) | `index.js:386 aoRadius` |
//! | pixel-radius clamp | **6 .. 128 px** | `clamp( radiusPx, 6.0, 128.0 )` |
//! | step distribution | **quadratic**, `+1 px` floor | `radiusPx * ft * ft + 1.0` |
//! | falloff | `clamp(len²/r², 0, 1)`, **squared** | `fall *= fall` |
//! | per-slice arc clamp | `±π/2` **about `n`** | `n + max( h1 - n, -HALF_PI )` |
//! | slice sum clamp | `0 .. 4` after `/ 3` | `clamp( visibility / 3.0, 0.0, 4.0 )` |
//! | temporal feedback | **0.92** ([`TEMPORAL_FEEDBACK`]) | `uFeedback` |
//! | temporal rejection | `exp( -rel * 30 )` | depth-relative disocclusion |
//! | neighbourhood window | `±0.45`, at **2 texels** | `clamp( hist.x, mn-0.45, mx+0.45 )` |
//! | blur | 7-tap, `w0 = 0.4/(i+1)`, depth-aware `exp(-Δd·22/d)` | `AO_BLUR` |
//! | intensity curve | `pow( ao, 1.1 )` ([`SHIPPED_INTENSITY`]) | `index.js:389 aoIntensity` |
//!
//! # The two constants the source's own comments get wrong
//!
//! Both are pinned by [`tests`], because a reader who trusts the prose ports the
//! wrong number.
//!
//! 1. **The header says "Two slices x eight steps". The code says three.**
//!    `#define OW_SLICES 3` is the specification; the doc comment is stale.
//!    [`SLICES`] is 3.
//! 2. **The constructor's radius is dead.** `new THREE.Vector4(0.9, 1.35, 0, 0.4)`
//!    puts `0.9` in `uParams.x`, but `index.js:855` calls `setRadius(s.aoRadius)`
//!    every settings apply and `s.aoRadius` is **1.35** (`index.js:386`). The step
//!    loop's own comment — *"A 1.35 m radius on a wall three metres away is
//!    316 px"* — is the one telling the truth. Same for the intensity: the blur
//!    constructor's `1.25` is overwritten by `setIntensity(s.aoIntensity)` with
//!    **1.1**. Both pairs are recorded ([`CONSTRUCTOR_RADIUS_METRES`],
//!    [`SHIPPED_RADIUS_METRES`], [`CONSTRUCTOR_INTENSITY`], [`SHIPPED_INTENSITY`])
//!    so the dead one cannot be mistaken for the live one later.
//!
//! # There is no thickness term, and `uParams.w` is never read
//!
//! `uParams` is documented `x radius(m) y intensity z frame w thickness`, and
//! `AO_CORE` reads **only `.x` and `.z`**. `uParams.y` (1.35) and `uParams.w`
//! (0.4) are dead uniform lanes — the intensity is applied by `AO_BLUR` from its
//! *own* `uParams.y`, and the thickness heuristic is not a constant at all. It is
//! the falloff blend in [`reference::horizon_update`]:
//!
//! ```glsl
//! float fall = clamp( len2 * invR2, 0.0, 1.0 );
//! fall *= fall;
//! cosHPos = max( cosHPos, mix( c, cosHPos, fall ) );
//! ```
//!
//! A tap at the radius (`fall == 1`) contributes *nothing* — `mix` returns the
//! existing horizon — while a tap at the origin contributes its raw cosine. That
//! quartic ramp (a squared quadratic in distance) **is** the thickness model: it
//! is what stops a distant silhouette from occluding as though it were an
//! infinitely deep wall, without the explicit `thickness` parameter the uniform
//! block reserves and the shader never spends. Ported as dead lanes with names,
//! per "dead computation in the source is still part of the source".
//!
//! # Storage width is part of the algorithm
//!
//! Every target is `hdrTarget(w, h, { type: THREE.HalfFloatType, format:
//! THREE.RGFormat })` — **RG16Float**, not full float. The chain is
//! core → temporal → blur-H → blur-V, so the visibility *and the depth it
//! carries in `g`* round-trip through `f16` three times before anything reads
//! them. The depth channel is the one that matters: the temporal pass's
//! disocclusion test is `abs(hist.y - cur.y) / max(0.05, cur.y)` and the blur's
//! weight is `exp(-|Δd|·22/max(0.1, d))`, both of which are *sensitive to the
//! quantisation* at street depths — an `f16` step at 30 m is 0.016 m, which the
//! blur's `22/d` scale turns into a weight change of about 1%.
//! [`reference::store_rg16f`] is that quantisation, and the note file records
//! where in the chain each of the three applies.
//!
//! # Two convention corrections, both forced by WebGPU, both named
//!
//! The source runs on WebGL, whose framebuffer `v` runs **up**. WebGPU's runs
//! **down**. `crate::gbuffer` already pays this once
//! ([`crate::gbuffer::VELOCITY_TEXTURE_V_SIGN`]); this pass pays it twice more,
//! and both are silent-wrong-picture bugs rather than compile errors:
//!
//! - [`NDC_UV_V_SIGN`] — `owViewPos` reconstructs from `uv * 2 - 1`, which is only
//!   NDC if `v` runs up. The pass flips `v` once before calling
//!   [`reference::view_pos`]; the transcribed function itself is left exactly as
//!   the GLSL writes it, so what is corrected is the *caller*, not the maths.
//! - [`SCREEN_STEP_V_SIGN`] — `sliceDir` is `vec3( dir2, 0.0 )`, a **view-space**
//!   vector, so stepping along `+dir2` must move the sample *up* the screen when
//!   `dir2.y > 0`, i.e. toward **smaller** `v`. Get this wrong and the `+dir` and
//!   `-dir` horizons swap relative to `orthoDir` — which is exactly the failure
//!   the source's own comment warns about: *"Getting this the wrong way round
//!   collapses the visibility arc on every grazing surface."* It would not look
//!   like a sign bug; it would look like GTAO not working on any wall.
//!
//! # What this module contains, and what it does not
//!
//! Contains: the WGSL ([`wgsl`]) — a binding-free library of the transcribed
//! functions plus the three real passes that call them — and the CPU reference
//! ([`reference`]) that is the semantic definition of those functions, in the
//! relationship `crate::surface_program::parity` and `crate::bloom_pyramid`
//! establish.
//!
//! Does **not** contain: the render targets, the history flip, or the frame
//! graph. Nothing in this crate compiles or binds this yet — see
//! [`tests::nothing_in_the_present_path_compiles_this_yet`]. Wiring it needs a
//! `GBufferTargets` to already be rendering and three `RG16Float` targets with a
//! ping-pong history, which is `crate::live_gpu_binding`/`crate::offscreen`'s
//! line to write, not this one.

pub(crate) mod reference;

#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) mod wgsl;

/// The pipelines, targets and bind groups that put [`wgsl`]'s three entry points
/// in a real frame. Gated with `wgsl` because it is the only consumer.
#[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
pub(crate) mod pass;

#[cfg(all(test, feature = "offscreen"))]
mod parity;

/// Slices per pixel per frame: `#define OW_SLICES 3`.
///
/// **Three, not two.** The file's header comment says *"Two slices x eight steps
/// per frame"* and is stale; the `#define` immediately above `owArc` is the
/// specification. Pinned by [`tests::the_slice_count_is_the_defines_not_the_headers`].
pub(crate) const SLICES: usize = 3;

/// Steps per slice **per side**: `#define OW_STEPS 8`. Each step takes one `+dir`
/// tap and one `-dir` tap, so a pixel costs `3 * 8 * 2 = 48` depth taps plus the
/// same number of coverage taps.
pub(crate) const STEPS: usize = 8;

/// The world-space occlusion radius the engine actually runs, in metres:
/// `index.js:386  aoRadius: 1.35`, pushed in by `setRadius` on every settings
/// apply. **This is the live value.** See [`CONSTRUCTOR_RADIUS_METRES`].
pub(crate) const SHIPPED_RADIUS_METRES: f32 = 1.35;

/// The radius the `Gtao` constructor writes into `uParams.x`
/// (`new THREE.Vector4(0.9, …)`) — **dead**, overwritten by
/// [`SHIPPED_RADIUS_METRES`] before the first frame. Recorded so a later reader
/// cannot mistake it for the tuning.
pub(crate) const CONSTRUCTOR_RADIUS_METRES: f32 = 0.9;

/// The exponent `AO_BLUR` raises the resolved AO to on its final stage, as the
/// engine runs it: `index.js:389  aoIntensity: 1.1`. **The live value.**
pub(crate) const SHIPPED_INTENSITY: f32 = 1.1;

/// The intensity the blur pass's constructor writes (`new THREE.Vector2(0, 1.25)`)
/// — **dead**, overwritten by [`SHIPPED_INTENSITY`]. The peer of
/// [`CONSTRUCTOR_RADIUS_METRES`].
pub(crate) const CONSTRUCTOR_INTENSITY: f32 = 1.25;

/// `uParams.y` of the **core** pass, labelled `intensity` by the uniform's
/// comment and **never read by `AO_CORE`**. The intensity that reaches the
/// picture is the blur's, not this. Dead, named, and ported.
pub(crate) const CORE_UNREAD_INTENSITY: f32 = 1.35;

/// `uParams.w` of the core pass, labelled `thickness` and **never read**. There
/// is no thickness term; the module docs explain what plays its part. Dead,
/// named, and ported.
pub(crate) const CORE_UNREAD_THICKNESS: f32 = 0.4;

/// The temporal accumulator's history weight before rejection: `uFeedback: 0.92`.
/// Twelve and a half frames of effective history, which is what turns three
/// slices per frame into the ~16-slice look the module header claims.
pub(crate) const TEMPORAL_FEEDBACK: f32 = 0.92;

/// The period the frame counter is folded to before it drives the noise:
/// `frame % 64`. Sixty-four distinct interleaved-gradient rotations, cycling.
pub(crate) const FRAME_PERIOD: u32 = 64;

/// What the frame index is multiplied by before being added to `gl_FragCoord.xy`
/// for the IGN rotation: `owIGN( gl_FragCoord.xy + uParams.z * 5.588238 )`.
///
/// The magic number is the source's and it is added to **both** components (a
/// `vec2 + float` broadcast in GLSL). It is chosen so successive frames land on
/// unrelated points of the gradient lattice rather than marching along it; a
/// tidier value such as `5.0` would put frame `n` and frame `n+1` on nearly the
/// same rotation and the temporal accumulator would converge to a *biased*
/// three-slice answer instead of a sixteen-slice one.
pub(crate) const FRAME_NOISE_STRIDE: f32 = 5.588_238;

/// What `uParams.z` is multiplied into for the **step** jitter, which uses a
/// different hash and a different scale from the slice rotation:
/// `owHash12( gl_FragCoord.xy * 0.371 + uParams.z )`.
///
/// Note the asymmetry, and that it is deliberate: the slice angle is jittered by
/// interleaved-gradient noise (spatially smooth, so the bilateral blur can
/// resolve it) while the step position is jittered by a *white* hash (spatially
/// uncorrelated, so the temporal accumulator resolves it instead). Using one for
/// both is the classic way GTAO acquires either banding or boiling.
pub(crate) const STEP_HASH_COORD_SCALE: f32 = 0.371;

/// The sign a **`v` in texture space** must be given to become the source's
/// `vUv` before [`reference::view_pos`] reconstructs from `uv * 2.0 - 1.0`.
///
/// Applied as `vec2(uv.x, 1.0 - uv.y)`, not as a bare multiply — the flip is
/// about the `0.5` midpoint of the uv range, not about zero — so this constant is
/// the *statement of direction* and the pass shows the arithmetic. See the module
/// docs; the peer of [`crate::gbuffer::VELOCITY_TEXTURE_V_SIGN`].
pub(crate) const NDC_UV_V_SIGN: f32 = -1.0;

/// The sign the **`y` of a view-space screen direction** must be given to become
/// a texture-space `v` step.
///
/// `sliceDir = vec3( dir2, 0.0 )` is a view-space vector, so `+dir2.y` is
/// *upward* on screen, which in a WebGPU framebuffer is a *decrease* in `v`. The
/// horizon search must step `vec2(dir2.x, dir2.y * SCREEN_STEP_V_SIGN) * off *
/// texel`. Getting it wrong exchanges `cosHPos` and `cosHNeg` relative to
/// `orthoDir` and collapses the arc on grazing surfaces — the exact failure the
/// source's comment names.
pub(crate) const SCREEN_STEP_V_SIGN: f32 = -1.0;

/// The coverage value below which the core pass writes the "no geometry here"
/// sentinel and stops: `if ( nrm.z < 0.5 )`. The same `0.5` every
/// [`crate::gbuffer`] consumer tests against, so both `COVERAGE_STATIC` (1.0) and
/// `COVERAGE_DYNAMIC` (0.7) read as covered.
pub(crate) const COVERAGE_THRESHOLD: f32 = 0.5;

/// The depth the core pass writes into `g` on an uncovered pixel:
/// `gl_FragColor = vec4( 1.0, 1e4, 0.0, 1.0 )`.
///
/// Ten kilometres, and the choice is load-bearing rather than arbitrary: it makes
/// every *downstream* depth-aware weight reject the pixel automatically. The
/// blur's `exp( -|Δd| * 22 / max(0.1, d) )` against a real 30 m neighbour is
/// `exp(-7300)`, i.e. exactly zero, so sky never bleeds its `1.0` visibility into
/// a silhouette. A sentinel of `0.0` would have done the opposite.
pub(crate) const UNCOVERED_DEPTH_SENTINEL: f32 = 1.0e4;

/// The visibility the core pass writes on an uncovered pixel: fully unoccluded.
pub(crate) const UNCOVERED_VISIBILITY: f32 = 1.0;

/// The frame phase that drives both noises: `temporalOn ? frame % 64 : 0`.
///
/// With the temporal accumulator off, the phase is pinned to zero — a *fixed*
/// dither, which is stable and slightly patterned, rather than a rotating one
/// with nothing to resolve it, which would boil. The ternary is the source's and
/// its `false` arm is not a fallback, it is the correct answer for that mode.
pub(crate) fn frame_phase(frame: u32, temporal_on: bool) -> f32 {
    [0.0, (frame % FRAME_PERIOD) as f32][usize::from(temporal_on)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slice_count_is_the_defines_not_the_headers() {
        // `src/render/gtao.js` header: "Two slices x eight steps per frame".
        // `src/render/gtao.js` body:   "#define OW_SLICES 3".
        assert_eq!(
            SLICES, 3,
            "OW_SLICES is 3; the file header's 'two slices' is stale prose"
        );
        assert_eq!(STEPS, 8, "OW_STEPS is 8");
    }

    #[test]
    fn the_live_tuning_is_the_settings_not_the_constructor() {
        assert_ne!(
            SHIPPED_RADIUS_METRES, CONSTRUCTOR_RADIUS_METRES,
            "if these ever agree the distinction has been lost, not resolved"
        );
        assert_ne!(SHIPPED_INTENSITY, CONSTRUCTOR_INTENSITY);
        // The step loop's own comment reasons about "a 1.35 m radius", which is
        // the settings value and not the constructor's 0.9.
        assert_eq!(SHIPPED_RADIUS_METRES, 1.35);
        assert_eq!(SHIPPED_INTENSITY, 1.1);
        assert_eq!(CONSTRUCTOR_RADIUS_METRES, 0.9);
        assert_eq!(CONSTRUCTOR_INTENSITY, 1.25);
    }

    #[test]
    fn the_two_core_uniform_lanes_the_shader_never_reads_are_still_recorded() {
        assert_eq!(CORE_UNREAD_INTENSITY, 1.35);
        assert_eq!(CORE_UNREAD_THICKNESS, 0.4);
        // The core's dead `intensity` lane is NOT the blur's live one.
        assert_ne!(CORE_UNREAD_INTENSITY, SHIPPED_INTENSITY);
    }

    #[test]
    fn the_frame_phase_cycles_with_the_accumulator_and_pins_without_it() {
        assert_eq!(frame_phase(0, true), 0.0);
        assert_eq!(frame_phase(63, true), 63.0);
        assert_eq!(frame_phase(64, true), 0.0);
        assert_eq!(frame_phase(65, true), 1.0);
        // Off: pinned, whatever the frame.
        assert_eq!(frame_phase(0, false), 0.0);
        assert_eq!(frame_phase(1_000_003, false), 0.0);
        assert_eq!(
            frame_phase(1_000_003, true),
            (1_000_003_u32 % FRAME_PERIOD) as f32
        );
    }

    #[test]
    fn the_two_noise_drivers_are_different_constants_on_purpose() {
        assert_eq!(FRAME_NOISE_STRIDE, 5.588_238);
        assert_eq!(STEP_HASH_COORD_SCALE, 0.371);
        assert_ne!(
            FRAME_NOISE_STRIDE, STEP_HASH_COORD_SCALE,
            "the slice rotation and the step jitter use different hashes at \
             different scales; collapsing them bands or boils"
        );
    }

    #[test]
    fn the_uncovered_sentinel_rejects_itself_under_every_downstream_weight() {
        // The blur's depth weight against a real 30 m neighbour.
        let weight = reference::blur_tap_weight(0.2, UNCOVERED_DEPTH_SENTINEL, 30.0);
        assert_eq!(
            weight, 0.0,
            "a 1e4 sentinel must weigh exactly zero beside real geometry, \
             so sky never bleeds into a silhouette; got {weight}"
        );
        // And the temporal pass's disocclusion test rejects it just as hard.
        let w = reference::temporal_weight(
            TEMPORAL_FEEDBACK,
            [0.5, 0.5],
            UNCOVERED_DEPTH_SENTINEL,
            30.0,
        );
        assert_eq!(w, 0.0, "history at the sentinel depth must be rejected; got {w}");
        assert_eq!(UNCOVERED_VISIBILITY, 1.0);
        assert_eq!(COVERAGE_THRESHOLD, 0.5);
    }

    #[test]
    fn both_webgpu_v_corrections_point_the_same_way_as_the_gbuffers() {
        assert_eq!(NDC_UV_V_SIGN, -1.0);
        assert_eq!(SCREEN_STEP_V_SIGN, -1.0);
        assert_eq!(
            SCREEN_STEP_V_SIGN,
            crate::gbuffer::VELOCITY_TEXTURE_V_SIGN,
            "all three are the same fact: WebGPU's framebuffer v runs down"
        );
    }

    /// Nothing in the crate binds this pass yet. When that changes, this test is
    /// the one that must be deleted **in the same change** as the wiring — a
    /// deferral needs an expiry check, and this is it.
    ///
    /// What would make it live: `crate::live_gpu_binding` (or
    /// `crate::offscreen`) rendering `crate::gbuffer`'s targets, then allocating
    /// three `RG16Float` targets plus a two-entry history and running
    /// [`wgsl::GTAO_CORE_PASS_WGSL`] / `GTAO_TEMPORAL_PASS_WGSL` /
    /// `GTAO_BLUR_PASS_WGSL` in that order.
    #[test]
    fn nothing_in_the_present_path_compiles_this_yet() {
        let renderer = include_str!("scene_renderer.rs");
        let post = include_str!("post_chain.rs");
        assert!(
            !renderer.contains("gtao") & !post.contains("gtao"),
            "gtao is now bound; delete this test and state the tolerance the \
             integration run measured"
        );
    }
}
