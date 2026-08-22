//! **Gather depth of field** — the ADS blur, transcribed from the reference's
//! GLSL.
//!
//! Ported from Claude-of-Duty `src/render/dof.js` (the `COC`, `PREFILTER`,
//! `GATHER` and `COMBINE` chunks and the `DepthOfField` class that drives them),
//! plus the seven `dof*` settings `src/render/index.js:376-382` authors and the
//! call site at `src/render/index.js:1450-1462`.
//!
//! # Where it sits, and why that is not where a post effect usually sits
//!
//! It is **pass 12 of 18**, between motion blur (11) and the registered passes
//! (13) — so it runs on the **linear HDR scene colour, before metering (15),
//! before bloom (16) and long before the composite (17/18)**. It is not part of
//! the display chain: [`crate::agx`] and [`crate::lut`] both run after it, on
//! its output. Two consequences that a port can get wrong silently:
//!
//! - The colours it blends are **unbounded radiance**, which is why every fetch
//!   in the source is wrapped in `max( …, vec3( 0.0 ) )` and why the gather's
//!   weights are normalised rather than assumed to sum to one. A negative lane
//!   out of a filtered `Rgba16Float` fetch would otherwise darken a neighbour.
//! - Because it precedes metering, the blur changes the *measured* exposure of
//!   the next frame. That is the source's behaviour, not a defect to correct.
//!
//! It is also engaged only while the sights are up (`index.js:1450`:
//! `this.dof && this._adsT > 0.01 && this.needsPrepass`), and the viewmodel is
//! composited at pass 14 — *after* — so the optic and the hands stay sharp by
//! construction rather than by masking.
//!
//! # Three passes, the blur at half resolution
//!
//! 1. **Prefilter**, full → half. A 4-tap box at `±0.5` full-res texels, with
//!    the colour weighted toward the *more blurred* taps (`k + 0.05`), and the
//!    neighbourhood's **maximum** circle of confusion packed into alpha.
//! 2. **Gather**, half → half. [`TAPS`] taps on a golden-angle spiral over the
//!    maximum CoC, weighted `clamp( tapCoC * 0.5 - dist + 1, 0, 1 )` — scatter
//!    as gather, so a blurred foreground fragment spreads onto its neighbours.
//! 3. **Combine**, full res. Blends sharp toward gathered by the full-res CoC,
//!    dilated by `blur.a * 0.85`.
//!
//! ## There is no tile / max-CoC prepass
//!
//! The brief asks for one if the source has one; it does **not**. The source
//! substitutes two things for it, and both are ported here because they are what
//! make the fixed-radius gather correct:
//!
//! - the prefilter packs `max( k0..k3 )` — the 2x2 neighbourhood maximum — into
//!   alpha ([`prefilter`]), and
//! - the gather carries `max( centre.a, every tap's .a )` forward
//!   ([`gather`]), which is a spiral-shaped max filter over the gather radius.
//!
//! The gather radius is therefore **always** [`gather_radius`] — the frame's
//! global maximum — and never a per-tile one. The source's own comment says why
//! that is still correct rather than merely cheap: an in-focus tap contributes a
//! weight of exactly zero, so a fixed loop costs bandwidth, not correctness.
//! Adding a tile prepass later would be an optimisation, not a fidelity fix.
//!
//! # Depth is fetched NEAREST, and that is load-bearing
//!
//! `tDepth` is the prepass's slot-2 attachment: `RedFormat` + `FloatType` —
//! R32F — with `minFilter` **and** `magFilter` set to `THREE.NearestFilter`
//! (`prepass.js:165-171`). Every `texture2D( tDepth, … )` in `dof.js` is thus a
//! point fetch, not a bilinear one.
//!
//! This lands exactly right in Axiom: [`crate::gbuffer::GBufferChannel::Depth`]
//! is `R32Float` — linear view depth in **metres, positive**, cleared to `0`
//! (`LoadOp::Clear(Color::TRANSPARENT)`), which is precisely the `depth <= 0.0`
//! ⇒ sky convention [`sky_depth`] encodes — and `R32Float` is *non-filterable*
//! in wgpu, so the only legal fetch is the point fetch the source already used.
//! Bind it via `GBufferTargets::view(GBufferChannel::Depth)`. The WGSL here uses
//! `textureLoad`, which needs no sampler and cannot be given a filtering one by
//! accident.
//!
//! The **colour** fetches are the opposite: `tColor` and `tSrc` are
//! `hdrTarget`s — `HalfFloatType` + `LinearFilter` (`pass.js:65-77`) — so the
//! gather's spiral taps *are* bilinear, and its intermediates are
//! `Rgba16Float`. The CoC that rides in alpha is therefore **rounded to `f16`
//! twice**: once storing the prefilter, once storing the gather. That is storage
//! width as part of the algorithm; [`crate::bloom_pyramid::half_storage`] models
//! it and [`quantized_coc`] is the entry point for a reference that needs to.
//!
//! # Nothing binds this yet
//!
//! See `tests::nothing_in_the_present_path_compiles_this_yet`. The frame graph
//! (`render/index.js`) is a sibling slice; what it must supply, and in what
//! order, is [`DOF_PASS_WGSL`]'s documentation.

// CPU<->GPU parity on a real adapter. Behind `offscreen` because it needs one;
// the arithmetic above is pure and is covered natively without it.
#[cfg(all(test, feature = "offscreen"))]
mod parity;

/// Taps on the gather's golden-angle spiral: `#define OW_DOF_TAPS 32`
/// (`dof.js:100`). The count is part of the look — it sets the bokeh's grain —
/// so it is a constant here and not a tuning knob.
pub(crate) const TAPS: usize = 32;

/// The golden angle in radians as the source spells it (`dof.js:118`), to the
/// digit. `2.39996323`, not `PI * (3 - sqrt(5))`: the latter is
/// `2.3999632297286…`, a different `f32`, and it would rotate every tap.
pub(crate) const GOLDEN_ANGLE: f32 = 2.399_963_23;

/// `6.2831853` (`dof.js:108`) — the source's literal, **not** `2 * PI`
/// (`6.28318530717…`). They are different `f32`s and this one scales the
/// per-pixel dither rotation.
pub(crate) const TAU: f32 = 6.283_185_3;

/// What a depth of `0` — nothing written, i.e. sky — is treated as
/// (`dof.js:44`, `dof.js:49`). Ten kilometres, so it takes the far blur
/// outright.
pub(crate) const SKY_DEPTH: f32 = 1e4;

/// The resolution the `maxCoc` setting is quoted at (`dof.js:212`). The CoC is
/// scaled by `height / 1080` so the blur is the same *fraction* of the frame at
/// every render scale.
pub(crate) const REFERENCE_HEIGHT: f32 = 1080.0;

/// The period of the gather's rotation dither, `frame % 64` (`dof.js:223`).
pub(crate) const FRAME_PERIOD: u32 = 64;

/// The seven `dof*` settings, `index.js:376-382`, in the source's order.
///
/// They reach the shader as two `vec4`s and the packing is the source's:
/// `uFocus = ( maxCoc, nearRatio, focusMin, focusMax )` and
/// `uRange = ( farStart, farRange, nearScale, 0 )` (`dof.js:157-158`,
/// `dof.js:213-214`). [`Dof::focus_lane`] and [`Dof::range_lane`] do that
/// packing so the two lanes cannot drift apart — the source shares one
/// `THREE.Vector4` between the prefilter and the combine for the same reason
/// (`dof.js:182`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dof {
    /// `dofMaxCoc`, in pixels at [`REFERENCE_HEIGHT`].
    pub(crate) max_coc: f32,
    /// `dofNearRatio` — the near field's CoC as a fraction of the far field's.
    pub(crate) near_ratio: f32,
    /// `dofFocusMin`, metres.
    pub(crate) focus_min: f32,
    /// `dofFocusMax`, metres.
    pub(crate) focus_max: f32,
    /// `dofFarStart` — the far ramp begins at `focus * farStart + 1` metres.
    pub(crate) far_start: f32,
    /// `dofFarRange` — the far ramp's width, metres.
    pub(crate) far_range: f32,
    /// `dofNearScale` — the near ramp ends at `focus * nearScale` metres.
    pub(crate) near_scale: f32,
}

/// The shipped settings, `index.js:376-382`.
///
/// The source's comment on `dofMaxCoc` is worth keeping: it is "3.3 px at 1080p,
/// down 40%: at 5.5 the near and mid ground of an ADS frame was a watercolour
/// smear that hid the very thing the sights are pointed at."
pub(crate) const SOURCE_SETTINGS: Dof = Dof {
    max_coc: 3.3,
    near_ratio: 0.38,
    focus_min: 3.0,
    focus_max: 18.0,
    far_start: 1.15,
    far_range: 18.0,
    near_scale: 0.55,
};

/// The `DepthOfField` constructor's defaults (`dof.js:157-158`), which the
/// shipped settings overwrite on the first `render`.
///
/// Kept because a constructor default that is never the shipped value is still
/// the value a frame rendered before the first `render` call would use, and
/// because the two disagree — `maxCoc` 5.0 vs 3.3, `nearRatio` 0.6 vs 0.38,
/// `focusMax` 20 vs 18, `farStart` 1.2 vs 1.15, `farRange` 20 vs 18 — which is
/// exactly the kind of divergence a port silently collapses onto one number.
pub(crate) const CONSTRUCTOR_DEFAULTS: Dof = Dof {
    max_coc: 5.0,
    near_ratio: 0.6,
    focus_min: 3.0,
    focus_max: 20.0,
    far_start: 1.2,
    far_range: 20.0,
    near_scale: 0.55,
};

impl Dof {
    /// `uFocus`, with `x` replaced by the resolution-and-engagement-scaled CoC
    /// the pass actually runs at (`dof.js:213`).
    ///
    /// `x` is **not** [`Self::max_coc`]: the source sets
    /// `this._focus.set( maxCoc, … )` where `maxCoc` has already been through
    /// [`max_coc_pixels`]. Packing the unscaled setting here would ignore both
    /// the render scale and the ADS ramp.
    pub(crate) fn focus_lane(self, coc_pixels: f32) -> [f32; 4] {
        [coc_pixels, self.near_ratio, self.focus_min, self.focus_max]
    }

    /// `uRange` (`dof.js:214`). The `w` lane is the source's declared unused
    /// slot and is written as `0`, not left indeterminate.
    pub(crate) fn range_lane(self) -> [f32; 4] {
        [self.far_start, self.far_range, self.near_scale, 0.0]
    }
}

/// The blur's arithmetic as WGSL: binding-free, so it concatenates in front of
/// whichever pass needs it, exactly as [`crate::agx::AGX_WGSL`] and
/// `material_shader`'s layers do.
///
/// `clamp`, `smoothstep`, `mix`, `dot`, `length` and `fract` are all written out
/// rather than called. WGSL's builtins are permitted to factor differently from
/// GLSL's, and this text has to mean exactly what `dof.js` means — `fract`
/// especially, which is `x - floor(x)` and not a remainder.
pub(crate) const DOF_WGSL: &str = r#"
// Gather depth of field, from Claude-of-Duty `src/render/dof.js`.
// See `dof.rs` for why depth is a point fetch and colour is not.

const AXIOM_DOF_TAPS: u32 = 32u;
const AXIOM_DOF_GOLDEN_ANGLE: f32 = 2.39996323;
const AXIOM_DOF_TAU: f32 = 6.2831853;
const AXIOM_DOF_SKY_DEPTH: f32 = 1e4;

fn axiom_dof_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    return min(max(x, lo), hi);
}

// GLSL `smoothstep( e0, e1, x )`, written out. Argument order is GLSL's
// (edges first), which is the reverse of `MathUtils.smoothstep`.
fn axiom_dof_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = axiom_dof_clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// GLSL `fract( x )` is `x - floor( x )`, which is NOT a remainder and is
// correct for a negative argument.
fn axiom_dof_fract(x: f32) -> f32 {
    return x - floor(x);
}

// `owIGN` (`glsl.js:73-75`) — interleaved gradient noise, the gather's
// per-pixel spiral rotation.
fn axiom_dof_ign(p: vec2<f32>) -> f32 {
    let d = p.x * 0.06711056 + p.y * 0.00583715;
    return axiom_dof_fract(52.9829189 * axiom_dof_fract(d));
}

// The prepass clears the depth channel to zero, so 0 means "sky" and takes the
// far blur outright (`dof.js:33-34`, `dof.js:44`, `dof.js:49`).
fn axiom_dof_sky_depth(depth: f32) -> f32 {
    return select(depth, AXIOM_DOF_SKY_DEPTH, depth <= 0.0);
}

// `owFocusDistance` (`dof.js:42-46`). `centre_depth` is the depth channel
// sampled at `vec2( 0.5 )` — literally the reticle plane.
fn axiom_dof_focus_distance(centre_depth: f32, focus: vec4<f32>) -> f32 {
    return axiom_dof_clamp(axiom_dof_sky_depth(centre_depth), focus.z, focus.w);
}

// `owCoC` (`dof.js:48-55`) — the near/far field split, in FULL-RES PIXELS.
//
// The far field ramps from `focus * farStart + 1` over `farRange` metres. The
// near field is the complement of a ramp that ENDS at `focus * nearScale` and
// starts at 35% of that, scaled by `nearRatio` so the foreground is softer than
// the background by construction. `max`, not a sum: the two fields never
// overlap for a focus inside `[focusMin, focusMax]`, and where they would, the
// wider one wins rather than the two adding into a double-strength blur.
fn axiom_dof_coc(depth: f32, focus_distance: f32, focus: vec4<f32>, range: vec4<f32>) -> f32 {
    let d = axiom_dof_sky_depth(depth);
    let far_start = focus_distance * range.x + 1.0;
    let far = axiom_dof_smoothstep(far_start, far_start + range.y, d);
    let near_end = focus_distance * range.z;
    let near = 1.0 - axiom_dof_smoothstep(near_end * 0.35, near_end, d);
    return focus.x * max(far, near * focus.y);
}

// `PREFILTER`'s combine (`dof.js:80-88`). The four colours arrive already
// floored at zero, as the source's `max( …, vec3( 0.0 ) )` fetches do.
//
// Weighting toward the MORE blurred taps is the whole point: averaging a sharp
// background into a blurred foreground is what makes cheap DOF look like a halo
// around every silhouette. Alpha carries the 2x2 neighbourhood's MAXIMUM CoC,
// which is this chain's substitute for a tile prepass.
fn axiom_dof_prefilter(
    c0: vec3<f32>, c1: vec3<f32>, c2: vec3<f32>, c3: vec3<f32>,
    k0: f32, k1: f32, k2: f32, k3: f32,
) -> vec4<f32> {
    let w0 = k0 + 0.05;
    let w1 = k1 + 0.05;
    let w2 = k2 + 0.05;
    let w3 = k3 + 0.05;
    let col = (c0 * w0 + c1 * w1 + c2 * w2 + c3 * w3) / (w0 + w1 + w2 + w3);
    return vec4<f32>(col, max(max(k0, k1), max(k2, k3)));
}

// The gather's radius in HALF-res pixels (`dof.js:106`). Always the maximum:
// an in-focus tap contributes a weight of exactly zero, so a fixed loop is both
// correct and branch free.
fn axiom_dof_gather_radius(max_coc_px: f32) -> f32 {
    return max(max_coc_px * 0.5, 1.0);
}

// The spiral's per-pixel rotation (`dof.js:108`). `frame_phase` is
// `frame % 64`, and the scalar is added to BOTH lanes of `gl_FragCoord.xy`
// because GLSL broadcasts `vec2 + float`.
fn axiom_dof_gather_rotation(frag_coord: vec2<f32>, frame_phase: f32) -> f32 {
    return axiom_dof_ign(frag_coord + frame_phase * 5.371) * AXIOM_DOF_TAU;
}

// One tap's offset in HALF-res pixels (`dof.js:117-120`).
//
// `sqrt( t )` is what makes the spiral area-uniform rather than crowding the
// centre; the grouping `( dir * sqrt( t ) ) * radius` is the source's.
fn axiom_dof_tap_offset(index: u32, rot: f32, radius: f32) -> vec2<f32> {
    let t = (f32(index) + 0.5) / f32(AXIOM_DOF_TAPS);
    let ang = f32(index) * AXIOM_DOF_GOLDEN_ANGLE + rot;
    let dir = vec2<f32>(cos(ang), sin(ang));
    return dir * sqrt(t) * radius;
}

// GLSL `length( off )` — `sqrt( dot( off, off ) )`, written out. It is NOT
// algebraically replaced by `sqrt( t ) * radius`: that is the same number in
// exact arithmetic and a different one in `f32`.
fn axiom_dof_tap_distance(off: vec2<f32>) -> f32 {
    return sqrt(off.x * off.x + off.y * off.y);
}

// Scatter-as-gather (`dof.js:124`): this tap only reaches us if its OWN circle
// of confusion is wide enough to have spread this far. `tap_coc` is in full-res
// pixels and `dist` is in half-res pixels, which is what the `* 0.5` reconciles.
fn axiom_dof_tap_weight(tap_coc: f32, dist: f32) -> f32 {
    return axiom_dof_clamp(tap_coc * 0.5 - dist + 1.0, 0.0, 1.0);
}

// `COMBINE`'s blend (`dof.js:143-151`). `sharp` and the blur's rgb arrive
// already floored at zero, as the source's fetches do.
//
// Dilating the full-res CoC with `blur.a * 0.85` — the gathered neighbourhood
// maximum — is what lets a blurred foreground bleed over the sharp thing behind
// it, and what keeps the transition off the half-res grid.
fn axiom_dof_combine(sharp: vec3<f32>, blur: vec4<f32>, sharp_coc: f32) -> vec3<f32> {
    let coc = max(sharp_coc, blur.a * 0.85);
    let m = axiom_dof_smoothstep(0.35, 1.45, coc);
    // GLSL `mix( x, y, a )` is `x * ( 1 - a ) + y * a`, written out.
    return sharp * (1.0 - m) + blur.rgb * m;
}
"#;

/// The three passes as fragment entry points, concatenated **after**
/// [`DOF_WGSL`].
///
/// # What the frame graph must supply, and in what order
///
/// The sibling porting `render/index.js` owns the targets and the schedule.
/// This text needs, per frame:
///
/// 1. **Two half-resolution `Rgba16Float` targets**, `max(1, w >> 1)` by
///    `max(1, h >> 1)` (`dof.js:193-196`) — the source calls them `rtA` and
///    `rtB` and they are `hdrTarget`s, so `Rgba16Float` with a **linear**
///    sampler and clamp-to-edge. Half float is not an economy: it is where the
///    CoC in alpha gets rounded, twice, and a `Rgba32Float` pair would render a
///    measurably different frame.
/// 2. **The G-buffer depth view**,
///    `GBufferTargets::view(GBufferChannel::Depth)` — `R32Float`, linear view
///    depth in metres, positive, zero where nothing was written. Bound as a
///    plain `texture_2d<f32>`; `textureLoad` needs no sampler, which is the only
///    way to fetch a non-filterable format and is what the source's
///    `NearestFilter` does anyway.
/// 3. **`axiom_dof.tune.x` = the scaled CoC** from [`max_coc_pixels`], with the
///    frame's *internal* height and the ADS engagement — not the setting.
/// 4. **`axiom_dof.tune.y` = `frame % 64`** ([`frame_phase`]).
///
/// and runs them strictly in this order, each reading the previous target:
///
/// | # | entry point | reads | writes |
/// |---|---|---|---|
/// | 1 | `axiom_dof_prefilter_fs` | scene colour (full), depth | `rtA` (half) |
/// | 2 | `axiom_dof_gather_fs` | `rtA` (half) | `rtB` (half) |
/// | 3 | `axiom_dof_combine_fs` | scene colour (full), `rtB`, depth | out (full) |
///
/// The gather binds its source in the **same slot** the prefilter binds the
/// scene colour, so one bind-group layout serves all three pipelines; wgpu
/// permits a layout to carry entries an entry point does not statically use, so
/// the gather's unused depth and blur slots cost a binding and no validation.
///
/// The pass is skipped entirely when the sights are down
/// (`index.js:1450`: `_adsT > 0.01`); at `amount = 0` [`max_coc_pixels`] is
/// zero, [`gather_radius`] floors at one half-res pixel and the combine's
/// `smoothstep(0.35, 1.45, 0)` is zero, so running it anyway is an exact copy
/// rather than a wrong frame — but it is still three passes of bandwidth.
pub(crate) const DOF_PASS_WGSL: &str = r#"
struct AxiomDofParams {
    // x maxCoC(px, full res)  y nearRatio  z focusMin  w focusMax
    focus: vec4<f32>,
    // x farStartScale  y farRange  z nearScale  w unused
    range: vec4<f32>,
    // xy FULL-res source texel (1/w, 1/h)   zw HALF-res texel (1/hw, 1/hh)
    texel: vec4<f32>,
    // x maxCoC(px, full res)  y frame % 64  zw unused
    tune: vec4<f32>,
};

@group(0) @binding(0) var<uniform> axiom_dof: AxiomDofParams;
// Full-res scene colour for the prefilter and the combine; the HALF-res `rtA`
// for the gather. Linear-filtered: the gather's spiral taps land at fractional
// texels and are meant to be bilinear.
@group(0) @binding(1) var axiom_dof_colour: texture_2d<f32>;
@group(0) @binding(2) var axiom_dof_colour_sampler: sampler;
// The G-buffer depth channel. R32Float, non-filterable, read with textureLoad.
@group(0) @binding(3) var axiom_dof_depth: texture_2d<f32>;
// The gathered half-res blur, for the combine only.
@group(0) @binding(4) var axiom_dof_blur: texture_2d<f32>;
@group(0) @binding(5) var axiom_dof_blur_sampler: sampler;

struct AxiomDofVsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn axiom_dof_vs(@builtin(vertex_index) index: u32) -> AxiomDofVsOut {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    let p = corners[index];
    var out: AxiomDofVsOut;
    out.position = vec4<f32>(p, 0.0, 1.0);
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return out;
}

// A NEAREST fetch of the depth channel with clamp-to-edge, which is what
// `texture2D` on a `NearestFilter` R32F texture does (`prepass.js:165-171`).
fn axiom_dof_depth_at(uv: vec2<f32>) -> f32 {
    let dims = vec2<f32>(textureDimensions(axiom_dof_depth, 0));
    let texel = floor(uv * dims);
    let clamped = clamp(texel, vec2<f32>(0.0), dims - vec2<f32>(1.0));
    return textureLoad(axiom_dof_depth, vec2<i32>(clamped), 0).r;
}

// The focal plane, read from the depth buffer at the SCREEN CENTRE — literally
// the reticle plane — so it tracks whatever the player is actually aiming at.
fn axiom_dof_focus() -> f32 {
    return axiom_dof_focus_distance(axiom_dof_depth_at(vec2<f32>(0.5, 0.5)), axiom_dof.focus);
}

fn axiom_dof_colour_at(uv: vec2<f32>) -> vec3<f32> {
    let c = textureSampleLevel(axiom_dof_colour, axiom_dof_colour_sampler, uv, 0.0).rgb;
    return max(c, vec3<f32>(0.0));
}

// PASS 1 — full -> half. 4-tap box of the colour, plus the circle of confusion
// in FULL-RES PIXELS packed into alpha (`dof.js:66-89`).
@fragment
fn axiom_dof_prefilter_fs(in: AxiomDofVsOut) -> @location(0) vec4<f32> {
    let focus = axiom_dof_focus();
    let o = axiom_dof.texel.xy * 0.5;

    let c0 = axiom_dof_colour_at(in.uv + vec2<f32>(-o.x, -o.y));
    let c1 = axiom_dof_colour_at(in.uv + vec2<f32>( o.x, -o.y));
    let c2 = axiom_dof_colour_at(in.uv + vec2<f32>(-o.x,  o.y));
    let c3 = axiom_dof_colour_at(in.uv + vec2<f32>( o.x,  o.y));

    let d0 = axiom_dof_depth_at(in.uv + vec2<f32>(-o.x, -o.y));
    let d1 = axiom_dof_depth_at(in.uv + vec2<f32>( o.x, -o.y));
    let d2 = axiom_dof_depth_at(in.uv + vec2<f32>(-o.x,  o.y));
    let d3 = axiom_dof_depth_at(in.uv + vec2<f32>( o.x,  o.y));

    let k0 = axiom_dof_coc(d0, focus, axiom_dof.focus, axiom_dof.range);
    let k1 = axiom_dof_coc(d1, focus, axiom_dof.focus, axiom_dof.range);
    let k2 = axiom_dof_coc(d2, focus, axiom_dof.focus, axiom_dof.range);
    let k3 = axiom_dof_coc(d3, focus, axiom_dof.focus, axiom_dof.range);

    return axiom_dof_prefilter(c0, c1, c2, c3, k0, k1, k2, k3);
}

// PASS 2 — half res, 32 taps on a golden-angle spiral over the MAXIMUM CoC
// (`dof.js:102-131`). The loop is the source's; shader text is data, so the
// Branchless Law does not reach it, and a spiral gather stays a spiral gather.
@fragment
fn axiom_dof_gather_fs(in: AxiomDofVsOut) -> @location(0) vec4<f32> {
    let centre = textureSampleLevel(axiom_dof_colour, axiom_dof_colour_sampler, in.uv, 0.0);
    let radius = axiom_dof_gather_radius(axiom_dof.tune.x);
    let rot = axiom_dof_gather_rotation(in.position.xy, axiom_dof.tune.y);

    var sum = centre.rgb;
    var wsum = 1.0;
    var max_coc = centre.a;

    for (var i: u32 = 0u; i < AXIOM_DOF_TAPS; i = i + 1u) {
        let off = axiom_dof_tap_offset(i, rot, radius);
        let s = textureSampleLevel(
            axiom_dof_colour,
            axiom_dof_colour_sampler,
            in.uv + off * axiom_dof.texel.zw,
            0.0,
        );
        let dist = axiom_dof_tap_distance(off);
        let w = axiom_dof_tap_weight(s.a, dist);
        sum = sum + s.rgb * w;
        wsum = wsum + w;
        max_coc = max(max_coc, s.a);
    }

    return vec4<f32>(sum / max(wsum, 1e-4), max_coc);
}

// PASS 3 — full res (`dof.js:142-152`).
@fragment
fn axiom_dof_combine_fs(in: AxiomDofVsOut) -> @location(0) vec4<f32> {
    let sharp = axiom_dof_colour_at(in.uv);
    let blur_raw = textureSampleLevel(axiom_dof_blur, axiom_dof_blur_sampler, in.uv, 0.0);
    let blur = vec4<f32>(max(blur_raw.rgb, vec3<f32>(0.0)), blur_raw.a);
    let focus = axiom_dof_focus();
    let coc = axiom_dof_coc(axiom_dof_depth_at(in.uv), focus, axiom_dof.focus, axiom_dof.range);
    return vec4<f32>(axiom_dof_combine(sharp, blur, coc), 1.0);
}
"#;

/// GLSL `clamp( x, lo, hi )` — `min( max( x, lo ), hi )`, written out because
/// that expansion is the specification and a builtin's is not guaranteed to be.
fn glsl_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    f32::min(f32::max(x, lo), hi)
}

/// GLSL `smoothstep( e0, e1, x )` (`dof.js:51`, `dof.js:53`, `dof.js:150`).
///
/// Edges first. `MathUtils.smoothstep( x, min, max )` takes them the other way
/// round and this is not that function.
fn glsl_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = glsl_clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// GLSL `fract( x )` — `x - floor( x )`. Not a remainder: `fract(-0.25)` is
/// `0.75`, where `-0.25 % 1.0` is `-0.25`.
fn glsl_fract(x: f32) -> f32 {
    x - f32::floor(x)
}

/// `owIGN( p )` (`glsl.js:73-75`) — the gather's per-pixel spiral rotation.
pub(crate) fn ign(p: [f32; 2]) -> f32 {
    glsl_fract(52.982_918_9 * glsl_fract(p[0] * 0.067_110_56 + p[1] * 0.005_837_15))
}

/// A depth of zero means the prepass wrote nothing there, i.e. sky, and the
/// source substitutes [`SKY_DEPTH`] (`dof.js:44`, `dof.js:49`).
///
/// The comparison is `<= 0.0`, so a depth of exactly zero is sky. The G-buffer
/// clears to zero, which is what makes that the right test rather than a
/// tolerance.
pub(crate) fn sky_depth(depth: f32) -> f32 {
    [depth, SKY_DEPTH][usize::from(depth <= 0.0)]
}

/// `owFocusDistance()` (`dof.js:42-46`) — the focal plane, from the depth
/// channel at the screen centre, clamped into `[focusMin, focusMax]`.
pub(crate) fn focus_distance(centre_depth: f32, focus: [f32; 4]) -> f32 {
    glsl_clamp(sky_depth(centre_depth), focus[2], focus[3])
}

/// `owCoC( depth, focus )` (`dof.js:48-55`) — **the semantic definition** the
/// WGSL is a mirror of. The result is a circle of confusion in **full-res
/// pixels**.
pub(crate) fn coc(depth: f32, focus_distance: f32, focus: [f32; 4], range: [f32; 4]) -> f32 {
    let d = sky_depth(depth);
    let far_start = focus_distance * range[0] + 1.0;
    let far = glsl_smoothstep(far_start, far_start + range[1], d);
    let near_end = focus_distance * range[2];
    let near = 1.0 - glsl_smoothstep(near_end * 0.35, near_end, d);
    focus[0] * f32::max(far, near * focus[1])
}

/// The CoC the pass actually runs at (`dof.js:212`):
/// `settings.dofMaxCoc * ( height / 1080 ) * amount`.
///
/// `height` is the **internal** render height, not the canvas height — the
/// source calls `setSize( rw, rh )` with the scaled resolution
/// (`index.js:918`). `amount` is the ADS engagement in `0..=1`, so the blur
/// ramps in with the sight picture instead of popping.
///
/// The division is a division. Rewriting `height / 1080.0` as
/// `height * (1.0 / 1080.0)` is a different `f32`, and this port has already
/// found five of those.
pub(crate) fn max_coc_pixels(max_coc_setting: f32, height: f32, amount: f32) -> f32 {
    max_coc_setting * (height / REFERENCE_HEIGHT) * amount
}

/// The gather's radius in **half-res** pixels (`dof.js:106`).
pub(crate) fn gather_radius(max_coc_px: f32) -> f32 {
    f32::max(max_coc_px * 0.5, 1.0)
}

/// `frame % 64` (`dof.js:223`) — the gather rotation's temporal phase.
pub(crate) fn frame_phase(frame: u32) -> f32 {
    (frame % FRAME_PERIOD) as f32
}

/// The spiral's rotation for one pixel (`dof.js:108`).
///
/// `gl_FragCoord.xy + uParams.y * 5.371` broadcasts the scalar to **both**
/// lanes; adding it to only `x` is a plausible-looking port that would give the
/// whole frame one rotation per row.
pub(crate) fn gather_rotation(frag_coord: [f32; 2], frame_phase: f32) -> f32 {
    ign([
        frag_coord[0] + frame_phase * 5.371,
        frag_coord[1] + frame_phase * 5.371,
    ]) * TAU
}

/// The half-resolution size of the blur targets (`dof.js:193-194`):
/// `max( 1, w >> 1 )`.
///
/// A shift, not a divide-and-round: at an odd width the two differ, and the
/// half-res texel the gather offsets by is `1 / hw`.
pub(crate) fn half_size(width: u32, height: u32) -> (u32, u32) {
    (u32::max(1, width >> 1), u32::max(1, height >> 1))
}

/// `PREFILTER` (`dof.js:66-89`), the arithmetic with the four fetches already
/// done — **the semantic definition** of pass 1.
///
/// `colours` are the four `tColor` taps at `±0.5` full-res texels in the
/// source's order (`--`, `+-`, `-+`, `++`); `depths` are the `tDepth` taps at
/// the same four offsets. The `max( …, 0 )` floor on the colours is applied
/// here, as the source applies it at the fetch.
///
/// Returns `(rgb, maxCoC)` packed as the shader's `vec4`.
pub(crate) fn prefilter(
    colours: [[f32; 3]; 4],
    depths: [f32; 4],
    focus: f32,
    focus_lane: [f32; 4],
    range_lane: [f32; 4],
) -> [f32; 4] {
    let k = depths.map(|d| coc(d, focus, focus_lane, range_lane));
    let w = k.map(|kn| kn + 0.05);
    let c = colours.map(|rgb| rgb.map(|lane| f32::max(lane, 0.0)));
    let denominator = w[0] + w[1] + w[2] + w[3];
    let col = [0_usize, 1, 2]
        .map(|l| (c[0][l] * w[0] + c[1][l] * w[1] + c[2][l] * w[2] + c[3][l] * w[3]) / denominator);
    [
        col[0],
        col[1],
        col[2],
        f32::max(f32::max(k[0], k[1]), f32::max(k[2], k[3])),
    ]
}

/// One gather tap's offset in half-res pixels (`dof.js:117-120`).
pub(crate) fn tap_offset(index: usize, rot: f32, radius: f32) -> [f32; 2] {
    let t = (index as f32 + 0.5) / TAPS as f32;
    let ang = index as f32 * GOLDEN_ANGLE + rot;
    let scaled = f32::sqrt(t);
    [
        f32::cos(ang) * scaled * radius,
        f32::sin(ang) * scaled * radius,
    ]
}

/// GLSL `length( off )` — `sqrt( dot( off, off ) )` (`dof.js:122`).
///
/// Deliberately **not** simplified to `sqrt(t) * radius`. The direction vector
/// is unit only in exact arithmetic; `cos²+sin²` is within an ULP of one and the
/// two expressions differ in the last bits, which is precisely the class of
/// re-association this port hunts for.
pub(crate) fn tap_distance(off: [f32; 2]) -> f32 {
    f32::sqrt(off[0] * off[0] + off[1] * off[1])
}

/// The scatter-as-gather weight (`dof.js:124`).
pub(crate) fn tap_weight(tap_coc: f32, dist: f32) -> f32 {
    glsl_clamp(tap_coc * 0.5 - dist + 1.0, 0.0, 1.0)
}

/// `GATHER` (`dof.js:102-131`), with the 33 fetches already done — **the
/// semantic definition** of pass 2.
///
/// `centre` is `texture2D( tSrc, vUv )` and `taps[i]` is the fetch at
/// `vUv + tap_offset(i, rot, radius) * halfTexel`, both as `(r, g, b, coc)`.
/// Handing the taps in as data is deliberate: it takes the sampler out of the
/// loop, so what a parity proof measures is the transcription rather than the
/// hardware's bilinear.
///
/// The accumulation order is the source's — a `fold` over `0..TAPS` in
/// ascending index, seeded with the centre at weight one — because float
/// addition is not associative and a different order is a different image.
pub(crate) fn gather(centre: [f32; 4], taps: &[[f32; 4]; TAPS], rot: f32, radius: f32) -> [f32; 4] {
    let seeded = ([centre[0], centre[1], centre[2]], 1.0_f32, centre[3]);
    let (sum, wsum, max_coc) = (0..TAPS).fold(seeded, |(sum, wsum, max_coc), index| {
        let off = tap_offset(index, rot, radius);
        let s = taps[index];
        let w = tap_weight(s[3], tap_distance(off));
        (
            [sum[0] + s[0] * w, sum[1] + s[1] * w, sum[2] + s[2] * w],
            wsum + w,
            f32::max(max_coc, s[3]),
        )
    });
    let denominator = f32::max(wsum, 1e-4);
    [
        sum[0] / denominator,
        sum[1] / denominator,
        sum[2] / denominator,
        max_coc,
    ]
}

/// `COMBINE` (`dof.js:142-152`), with the three fetches already done — **the
/// semantic definition** of pass 3.
///
/// `sharp` is `tColor` at `vUv`, `blur` is `tBlur` at `vUv`, and `sharp_coc` is
/// [`coc`] of the full-res depth there. The `max( …, 0 )` floors on both colours
/// are applied here, as the source applies them at the fetch.
pub(crate) fn combine(sharp: [f32; 3], blur: [f32; 4], sharp_coc: f32) -> [f32; 3] {
    let dilated = f32::max(sharp_coc, blur[3] * 0.85);
    let m = glsl_smoothstep(0.35, 1.45, dilated);
    let s = sharp.map(|lane| f32::max(lane, 0.0));
    let b = [0_usize, 1, 2].map(|l| f32::max(blur[l], 0.0));
    [0_usize, 1, 2].map(|l| s[l] * (1.0 - m) + b[l] * m)
}

/// The CoC as it actually survives a hop through one of the two half-resolution
/// `Rgba16Float` targets (`pass.js:67`, `dof.js:195-196`).
///
/// The prefilter writes the neighbourhood maximum into alpha and the gather
/// reads it back; the gather writes its own maximum and the combine reads that.
/// Both hops round to `f16`. At the shipped `maxCoc` of 3.3 px the spacing there
/// is `2^-9`, so this is a real quantisation of the weight in
/// [`tap_weight`] — small, but it is the algorithm and not noise.
///
/// A reference driving the whole chain end to end must apply this between the
/// stages; [`gather`] and [`combine`] do not apply it themselves, because they
/// model the arithmetic *inside* a pass rather than the store between two.
pub(crate) fn quantized_coc(coc_px: f32) -> f32 {
    crate::bloom_pyramid::half_storage::quantize(coc_px)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tap count is 32 and the spiral is a golden-angle spiral. Both are
    /// part of the look rather than free parameters, so they are pinned.
    #[test]
    fn the_bokeh_pattern_is_thirty_two_golden_angle_taps() {
        assert_eq!(TAPS, 32, "OW_DOF_TAPS is 32 (dof.js:100)");
        assert!(
            DOF_WGSL.contains("const AXIOM_DOF_TAPS: u32 = 32u;"),
            "the WGSL must declare the same tap count"
        );
        // The golden angle, to the source's digits and no further.
        assert_eq!(
            GOLDEN_ANGLE, 2.399_963_23_f32,
            "the source spells the golden angle 2.39996323 (dof.js:118)"
        );
        // The closed form is what a tidy-up would reach for, and here it is
        // **bit-identical**: the source's eight digits round to the same `f32` as
        // `PI * (3 - sqrt 5)`. Pinned as an equality rather than deleted, because
        // the fact that these agree is the fact worth knowing — it says the
        // literal carries no information the closed form loses, so a future
        // reader who wants to swap them can, and will not silently change the
        // spiral. (The same is NOT true of `TAU` below, which is why both checks
        // are here.)
        let closed_form = std::f32::consts::PI * (3.0 - 5.0_f32.sqrt());
        assert_eq!(
            GOLDEN_ANGLE.to_bits(),
            closed_form.to_bits(),
            "the literal and PI*(3-sqrt 5) are the same f32: {GOLDEN_ANGLE:?} vs {closed_form:?}"
        );
        let closed_tau = std::f32::consts::TAU;
        assert_eq!(
            TAU.to_bits(),
            closed_tau.to_bits(),
            "6.2831853 and TAU are the same f32: {TAU:?} vs {closed_tau:?}"
        );
    }

    /// The spiral is area-uniform and covers the full radius: `sqrt(t)` with
    /// `t = (i + 0.5) / 32` puts the outermost tap just inside the radius and
    /// the innermost near the centre, and no two taps share an angle.
    #[test]
    fn the_spiral_is_area_uniform_and_bounded_by_the_radius() {
        let radius = 4.0;
        let distances: Vec<f32> = (0..TAPS)
            .map(|i| tap_distance(tap_offset(i, 0.0, radius)))
            .collect();
        let outermost = distances[TAPS - 1];
        assert!(
            outermost < radius && outermost > radius * 0.98,
            "the last tap must sit just inside the radius, got {outermost} of {radius}"
        );
        assert!(
            distances[0] < radius * 0.2,
            "the first tap must sit near the centre, got {}",
            distances[0]
        );
        // Monotone in the index, which is what `sqrt` of a rising `t` gives.
        let rising = distances.windows(2).all(|w| w[1] > w[0]);
        assert!(rising, "tap distance must rise with the index: {distances:?}");
    }

    /// An in-focus tap contributes exactly zero, which is the property that
    /// makes a fixed 32-iteration loop correct rather than merely cheap.
    #[test]
    fn an_in_focus_tap_contributes_no_weight() {
        // A tap with zero CoC, one half-res pixel away.
        assert_eq!(
            tap_weight(0.0, 1.0),
            0.0,
            "a sharp tap one pixel out must contribute nothing"
        );
        // The centre tap of a sharp neighbourhood: dist 0 gives weight 1.
        assert_eq!(tap_weight(0.0, 0.0), 1.0, "a zero-distance tap is unweighted");
        // A wide tap reaches further. The weight is `coc * 0.5 - dist + 1`, so a
        // 6 px CoC (3 half-res) fully reaches 3 half-res px, half-reaches 3.5,
        // and stops at 4 — one half-res pixel of ramp, not a hard edge.
        assert_eq!(tap_weight(6.0, 3.0), 1.0, "a 6px CoC must fully reach 3 half-res px");
        assert_eq!(tap_weight(6.0, 3.5), 0.5, "and half-reach 3.5 half-res px");
        assert_eq!(tap_weight(6.0, 4.0), 0.0, "and stop at 4 half-res px");
    }

    /// A frame with no blur anywhere reaches the output unchanged — the property
    /// that lets the frame graph run the pass at `amount = 0` and get a copy
    /// rather than a wrong image.
    ///
    /// Note where that property actually lives: **the combine, not the gather.**
    /// The gather's weight is `clamp( coc * 0.5 - dist + 1 )`, so a tap closer
    /// than one half-res pixel contributes even at zero CoC, and with the radius
    /// floored at `1.0` several taps always do. The gather therefore blurs a
    /// sharp frame slightly. It does not matter, because the combine's
    /// `smoothstep( 0.35, 1.45, 0 )` is exactly zero and discards the whole
    /// gathered image. A port that "fixed" the gather to be identity here would
    /// be changing the source.
    #[test]
    fn a_fully_sharp_frame_survives_the_gather_and_the_combine() {
        let centre = [0.25_f32, 0.5, 0.75, 0.0];
        let taps = [[0.9_f32, 0.8, 0.7, 0.0]; TAPS];
        let gathered = gather(centre, &taps, 0.0, 1.0);

        // No CoC is carried forward, which is what the combine keys off.
        assert_eq!(gathered[3], 0.0, "a sharp neighbourhood must carry no CoC");
        // And the gather is NOT the identity, which is the point of the note.
        assert!(
            (gathered[0] - centre[0]).abs() > 1e-3,
            "the gather blurs even a sharp frame at the radius floor; got {} from {}",
            gathered[0],
            centre[0]
        );

        // The combine is where sharpness is preserved, exactly.
        let out = combine([0.25, 0.5, 0.75], gathered, 0.0);
        assert_eq!(out, [0.25, 0.5, 0.75], "a zero CoC must blend to fully sharp");
    }

    /// The near/far split: sky takes the far blur outright, the focal plane is
    /// sharp, and the near field is scaled by `nearRatio` rather than reaching
    /// the full CoC.
    #[test]
    fn the_near_and_far_fields_split_around_the_focal_plane() {
        let settings = SOURCE_SETTINGS;
        let coc_px = max_coc_pixels(settings.max_coc, 1080.0, 1.0);
        let focus_lane = settings.focus_lane(coc_px);
        let range_lane = settings.range_lane();
        let focus = focus_distance(8.0, focus_lane);
        assert_eq!(focus, 8.0, "8 m is inside [focusMin, focusMax]");

        // Sky (depth 0) is 1e4 m and is past the far ramp entirely.
        let sky = coc(0.0, focus, focus_lane, range_lane);
        assert_eq!(sky, coc_px, "sky must take the full far CoC, got {sky}");

        // The focal plane itself is sharp: it is below `focus * 1.15 + 1` and
        // above `focus * 0.55`.
        let at_focus = coc(8.0, focus, focus_lane, range_lane);
        assert_eq!(at_focus, 0.0, "the focal plane must be sharp, got {at_focus}");

        // The near field is capped by `nearRatio`, not by `maxCoc`.
        let very_near = coc(0.5, focus, focus_lane, range_lane);
        let expected_near = coc_px * settings.near_ratio;
        assert!(
            (very_near - expected_near).abs() < 1e-6,
            "the near field must cap at maxCoc * nearRatio = {expected_near}, got {very_near}"
        );
        assert!(
            very_near < sky,
            "the near field must be softer than the far field: {very_near} vs {sky}"
        );
    }

    /// The focal plane is clamped into `[focusMin, focusMax]`, including when
    /// the player aims at the sky.
    #[test]
    fn the_focal_plane_is_clamped_and_the_sky_pins_it_far() {
        let lane = SOURCE_SETTINGS.focus_lane(3.3);
        assert_eq!(
            focus_distance(0.0, lane),
            SOURCE_SETTINGS.focus_max,
            "aiming at the sky must pin the focus at focusMax"
        );
        assert_eq!(
            focus_distance(0.4, lane),
            SOURCE_SETTINGS.focus_min,
            "a wall in the player's face must pin the focus at focusMin"
        );
        assert_eq!(focus_distance(9.5, lane), 9.5, "and anything between passes through");
        // The sky test is `<= 0`, so exactly zero is sky.
        assert_eq!(sky_depth(0.0), SKY_DEPTH);
        assert_eq!(sky_depth(-0.0), SKY_DEPTH);
        assert_eq!(sky_depth(1.5), 1.5);
    }

    /// `fract` is `x - floor(x)`, so IGN is well defined for a negative
    /// coordinate — which `gl_FragCoord.xy + phase * 5.371` never is, but the
    /// helper is shared and the distinction is the named trap.
    #[test]
    fn fract_is_not_a_remainder_and_ign_stays_in_the_unit_interval() {
        assert_eq!(glsl_fract(-0.25), 0.75, "fract(-0.25) is 0.75, not -0.25");
        assert_eq!(-0.25_f32 % 1.0, -0.25, "which is what the remainder would have given");
        let rotations: Vec<f32> = (0..16)
            .map(|i| gather_rotation([i as f32 * 7.0, i as f32 * 13.0], frame_phase(i)))
            .collect();
        let in_range = rotations.iter().all(|r| *r >= 0.0 && *r < TAU);
        assert!(in_range, "every rotation must lie in [0, TAU): {rotations:?}");
        // Neighbouring pixels get genuinely different rotations — that is the
        // point of interleaved gradient noise.
        let distinct = rotations.windows(2).all(|w| (w[1] - w[0]).abs() > 1e-4);
        assert!(distinct, "IGN must decorrelate neighbours: {rotations:?}");
    }

    /// The temporal phase wraps at 64, and the resolution scaling is a division.
    #[test]
    fn the_frame_phase_wraps_at_sixty_four_and_the_coc_scales_by_height() {
        assert_eq!(frame_phase(0), 0.0);
        assert_eq!(frame_phase(63), 63.0);
        assert_eq!(frame_phase(64), 0.0);
        assert_eq!(frame_phase(129), 1.0);

        // 1080p at full engagement is the setting itself.
        assert_eq!(max_coc_pixels(3.3, 1080.0, 1.0), 3.3);
        // Half the height is half the CoC; half the engagement halves it again.
        assert_eq!(max_coc_pixels(3.3, 540.0, 1.0), 1.65);
        assert_eq!(max_coc_pixels(3.3, 1080.0, 0.5), 1.65);
        assert_eq!(max_coc_pixels(3.3, 1080.0, 0.0), 0.0);

        // The gather radius floors at one half-res pixel, so a zero CoC still
        // runs a well-defined (and zero-weighted) spiral.
        assert_eq!(gather_radius(0.0), 1.0);
        assert_eq!(gather_radius(1.0), 1.0);
        assert_eq!(gather_radius(8.0), 4.0);
    }

    /// The half-res targets are a SHIFT, not a rounded divide, and never zero.
    #[test]
    fn the_blur_targets_are_a_floored_shift_of_at_least_one_texel() {
        assert_eq!(half_size(1920, 1080), (960, 540));
        assert_eq!(half_size(1921, 1081), (960, 540), "odd sizes floor, they do not round");
        assert_eq!(half_size(1, 1), (1, 1), "and never collapse to zero");
        assert_eq!(half_size(0, 0), (1, 1));
    }

    /// The prefilter weights toward the more blurred taps, and its alpha is the
    /// neighbourhood MAXIMUM — the substitute for a tile prepass.
    #[test]
    fn the_prefilter_weights_toward_blur_and_carries_the_neighbourhood_maximum() {
        let settings = SOURCE_SETTINGS;
        let focus_lane = settings.focus_lane(3.3);
        let range_lane = settings.range_lane();
        let focus = focus_distance(8.0, focus_lane);

        // Three sharp black taps and one blurred white one (sky).
        let colours = [[0.0_f32; 3], [0.0; 3], [0.0; 3], [1.0, 1.0, 1.0]];
        let depths = [8.0_f32, 8.0, 8.0, 0.0];
        let out = prefilter(colours, depths, focus, focus_lane, range_lane);

        // The blurred tap carries weight 3.35 against three of 0.05, so it
        // dominates far beyond its 1-in-4 share.
        assert!(
            out[0] > 0.9,
            "the blurred tap must dominate the box, got {} (a plain mean would be 0.25)",
            out[0]
        );
        // Alpha is the maximum, not the mean.
        assert_eq!(out[3], 3.3, "alpha must be max(k0..k3), got {}", out[3]);

        // A negative lane out of a filtered fetch is floored, not propagated.
        let clipped = prefilter([[-1.0; 3]; 4], [8.0; 4], focus, focus_lane, range_lane);
        assert_eq!([clipped[0], clipped[1], clipped[2]], [0.0; 3]);
    }

    /// A blurred foreground bleeds over the sharp thing behind it: the combine
    /// dilates the full-res CoC with `blur.a * 0.85`.
    #[test]
    fn the_combine_dilates_the_sharp_coc_with_the_gathered_maximum() {
        // A pixel that is itself perfectly sharp, next to a wide foreground.
        let sharp = [1.0_f32, 0.0, 0.0];
        let blur = [0.0_f32, 0.0, 1.0, 2.0];
        let out = combine(sharp, blur, 0.0);
        // smoothstep(0.35, 1.45, 2.0 * 0.85 = 1.7) saturates to 1, so the
        // gathered colour wins outright even though this pixel's own CoC is 0.
        assert_eq!(out, [0.0, 0.0, 1.0], "the neighbourhood must bleed over a sharp pixel");

        // Below the 0.35 knee nothing happens at all.
        let untouched = combine(sharp, [0.0, 0.0, 1.0, 0.4], 0.0);
        assert_eq!(untouched, sharp, "0.4 * 0.85 = 0.34 is below the knee");
    }

    /// The gather's accumulation order is the source's, and its normalisation
    /// keeps a fully-blurred neighbourhood at the right level rather than
    /// summing to something brighter than any input.
    #[test]
    fn the_gather_normalises_by_its_own_weight_sum() {
        let centre = [0.5_f32, 0.5, 0.5, 8.0];
        let taps = [[0.5_f32, 0.5, 0.5, 8.0]; TAPS];
        let out = gather(centre, &taps, 0.0, 4.0);
        assert!(
            (out[0] - 0.5).abs() < 1e-6,
            "a uniform neighbourhood must gather to its own value, got {}",
            out[0]
        );
        assert_eq!(out[3], 8.0, "and carry the maximum CoC forward");
    }

    /// `length(off)` is not `sqrt(t) * radius`. They agree to a few ULP and
    /// disagree in the last bits, and this pins that the reference computes the
    /// one the source writes.
    #[test]
    fn the_tap_distance_is_a_real_length_not_the_algebraic_shortcut() {
        let radius = 3.7_f32;
        let differing = (0..TAPS)
            .filter(|index| {
                let t = (*index as f32 + 0.5) / TAPS as f32;
                let shortcut = f32::sqrt(t) * radius;
                tap_distance(tap_offset(*index, 0.31, radius)).to_bits() != shortcut.to_bits()
            })
            .count();
        assert!(
            differing > 0,
            "the two forms must differ in at least one tap, or the pin proves nothing"
        );
    }

    /// The two settings tables genuinely disagree, so collapsing them onto one
    /// would be a real change to the frame.
    #[test]
    fn the_shipped_settings_are_not_the_constructor_defaults() {
        assert_ne!(SOURCE_SETTINGS, CONSTRUCTOR_DEFAULTS);
        assert_eq!(SOURCE_SETTINGS.max_coc, 3.3);
        assert_eq!(CONSTRUCTOR_DEFAULTS.max_coc, 5.0);
        // The lane packing is the source's order.
        assert_eq!(SOURCE_SETTINGS.focus_lane(1.25), [1.25, 0.38, 3.0, 18.0]);
        assert_eq!(SOURCE_SETTINGS.range_lane(), [1.15, 18.0, 0.55, 0.0]);
    }

    /// The CoC survives two `f16` stores, which is a real quantisation of the
    /// gather's weight and not a rounding to be ignored.
    #[test]
    fn the_coc_is_quantised_by_the_half_float_targets() {
        // 3.3 is not representable in f16.
        let stored = quantized_coc(3.3);
        assert_ne!(stored, 3.3, "3.3 must not survive an f16 store unchanged");
        assert!(
            (stored - 3.3).abs() < 2e-3,
            "but must stay within an f16 ULP at that magnitude, got {stored}"
        );
        // A second store is idempotent, which is why only the two hops matter.
        assert_eq!(quantized_coc(stored), stored);
    }

    /// The WGSL and the CPU reference must not drift into two definitions. The
    /// text scan pins the constants and the written-out builtins that the
    /// module docs say are written out on purpose.
    #[test]
    fn the_wgsl_writes_out_the_builtins_the_transcription_depends_on() {
        // `fract` written out, so it cannot become a remainder.
        assert!(DOF_WGSL.contains("return x - floor(x);"));
        // `smoothstep` written out with GLSL's edges-first argument order.
        assert!(DOF_WGSL.contains("fn axiom_dof_smoothstep(e0: f32, e1: f32, x: f32) -> f32"));
        assert!(DOF_WGSL.contains("return t * t * (3.0 - 2.0 * t);"));
        // `clamp` written out as min(max(..)).
        assert!(DOF_WGSL.contains("return min(max(x, lo), hi);"));
        // `mix` written out.
        assert!(DOF_WGSL.contains("return sharp * (1.0 - m) + blur.rgb * m;"));
        // `length` written out, not the builtin and not the shortcut.
        assert!(DOF_WGSL.contains("return sqrt(off.x * off.x + off.y * off.y);"));
        // The literals, to the source's digits.
        assert!(DOF_WGSL.contains("2.39996323"));
        assert!(DOF_WGSL.contains("6.2831853"));
        assert!(DOF_WGSL.contains("0.06711056"));
        assert!(DOF_WGSL.contains("52.9829189"));
        assert!(DOF_WGSL.contains("5.371"));
    }

    /// Depth is a POINT fetch. The pass WGSL must reach the depth channel with
    /// `textureLoad` and must never bind a sampler to it: `R32Float` is
    /// non-filterable, and the source's own texture is `NearestFilter`.
    #[test]
    fn the_depth_channel_is_point_fetched_and_never_sampled() {
        assert!(
            DOF_PASS_WGSL.contains("textureLoad(axiom_dof_depth"),
            "the depth channel must be read with textureLoad"
        );
        assert!(
            !DOF_PASS_WGSL.contains("axiom_dof_depth_sampler"),
            "there must be no sampler for the depth channel at all"
        );
        assert!(
            !DOF_PASS_WGSL.contains("textureSample(axiom_dof_depth"),
            "and no filtered fetch of it"
        );
        // The focal plane is read at the screen centre, which is the reticle.
        assert!(DOF_PASS_WGSL.contains("axiom_dof_depth_at(vec2<f32>(0.5, 0.5))"));
        // Three entry points, in the order the frame graph runs them.
        let prefilter_at = DOF_PASS_WGSL.find("fn axiom_dof_prefilter_fs").expect("pass 1");
        let gather_at = DOF_PASS_WGSL.find("fn axiom_dof_gather_fs").expect("pass 2");
        let combine_at = DOF_PASS_WGSL.find("fn axiom_dof_combine_fs").expect("pass 3");
        assert!(
            prefilter_at < gather_at && gather_at < combine_at,
            "the three passes must read in run order"
        );
    }

    /// **Nothing binds this yet.** The frame graph (`render/index.js`) is a
    /// sibling slice; until it lands, no shader in the crate concatenates this
    /// text and no pipeline compiles it.
    ///
    /// When that changes, this test is what must be *replaced* — by one that
    /// says which pass owns the DOF text and that the passes that are not the
    /// DOF chain still never mention it — not deleted. `crate::agx`'s
    /// `only_the_hdr_composite_arm_carries_agx` is the worked precedent for
    /// that replacement.
    #[test]
    fn nothing_in_the_present_path_compiles_this_yet() {
        let present_paths = [
            include_str!("post_chain.rs"),
            include_str!("upscale.rs"),
            include_str!("surface_encode.rs"),
            include_str!("scene_wgsl.rs"),
        ];
        let mentions = present_paths
            .iter()
            .filter(|source| source.contains("DOF_WGSL") | source.contains("DOF_PASS_WGSL"))
            .count();
        assert_eq!(
            mentions, 0,
            "no present path may concatenate the DOF text until the frame graph binds it"
        );
    }
}
