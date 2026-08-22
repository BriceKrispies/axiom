//! **De-tiling and the height blend** — the layer that stops a tiled texture
//! reading as a grid.
//!
//! Ported from Claude-of-Duty `src/materials/shader.js`: `owHeightBlend` in
//! `PARS_FRAGMENT`, the `#ifdef OW_DETILE` second-sample path in
//! `MAIN_FRAGMENT`, and the `detile` knob in `DEFAULT_PARAMS` ("de-tiling
//! second-sample blend amount (0 disables the extra fetches)").
//!
//! ## What it does
//!
//! A tiled texture repeats, and the eye finds the repeat. De-tiling takes a
//! **second sample of the same texture** at a de-correlated place — a rotation,
//! a rescale and an offset of the uv — and blends the two. Blending them 50/50
//! would only produce mush, so the blend is **height-preserving**: each sample's
//! own height (its albedo alpha) buys it weight, all but the top `0.18` of
//! weight is subtracted away, and what survives is renormalised. The taller
//! sample wins its pixels outright, so the result still reads as *one* material
//! with grain and edges rather than two averaged ones.
//!
//! Which of the two dominates is chosen by a **low-frequency mask** read from
//! the macro texture, so the crossover itself wanders over metres and does not
//! introduce a second grid.
//!
//! ## Why this cannot live in the field algebra
//!
//! The second sample must be taken with **explicit derivatives**
//! (`textureGrad`, WGSL's `textureSampleGrad`): the warped uv has a different
//! screen-space footprint from the base uv, and letting the hardware infer it
//! would pick the wrong mip and re-introduce the shimmer the layer exists to
//! remove. `08-material-shader-plan.md` names this as one of the two reasons
//! this shader is hand-written WGSL.
//!
//! ## `detile == 0` must not merely be quiet — it must not run
//!
//! The source disables the layer with a preprocessor `#define`:
//!
//! ```js
//! if (p.detile > 0 && p.uvMode !== 'triplanar') defines.OW_DETILE = '';
//! ```
//!
//! That is a *compile-time* exclusion, and [`detile_enabled`] is its port.
//! Feeding `t = 0` to [`height_blend`] instead is **not** equivalent, which this
//! module measures rather than assumes: at `t == 0` the surviving weight is
//! `~0.18` and the result is `a * 0.18 * (1 / 0.18)`, a round trip through two
//! roundings that is off by one ulp for roughly one input in six
//! (`a_runtime_zero_blend_is_not_bit_identical_to_the_undetiled_path`). So the
//! gate is structural: the composed program either contains this block or it
//! does not.
//!
//! ## Transcription notes
//!
//! Everything here is `f32` on both sides — the GPU has no other width, and a
//! CPU reference computing in `f64` would need a tolerance to hide the
//! difference rather than measure it. The source's groupings are reproduced
//! literally, including `1.0 / max(wa + wb, 1e-4)` computed as a **reciprocal
//! and then a multiply** — here that is what the source says, so re-writing it
//! as three divisions would be the defect, not the fix. `normalize` is the CPU
//! reference's one place where the hardware is free to differ (an `inversesqrt`
//! against a division); it is written as `v / length(v)`, GLSL's definition, and
//! the parity tolerance is measured from that choice.

use axiom_math::{Vec2, Vec3, Vec4};

/// The de-tiling and height-blend WGSL.
///
/// Every entry point is a free function taking its textures and samplers as
/// parameters, so the orchestrator can splice this into `axiom_surface` and wire
/// the bindings without this layer assuming a binding index.
///
/// | WGSL | source |
/// |---|---|
/// | `axiom_detile_warp(vec2) -> vec2` | the shared rotate+rescale of `uv2`/`ddx2`/`ddy2` |
/// | `axiom_detile_uv(vec2) -> vec2` | `uv2` |
/// | `axiom_detile_second_sample(...) -> AxiomDetileSample` | `alb2`/`orm2`/`n2` |
/// | `axiom_detile_mask_uv(vec3, f32) -> vec2` | the `owMacroTex` lookup coordinate |
/// | `axiom_detile_mask(...) -> f32` | `dtm` |
/// | `axiom_detile_fold_detail_normal(...) -> vec3` | the second sample's detail-normal fold |
/// | `axiom_detile_height_blend(ptr, ptr, ptr, ...)` | `owHeightBlend`'s three `inout`s |
pub(crate) const DETILE_WGSL: &str = r#"
// ---------------------------------------------------------------------------
// De-tiling and the height blend.
//
// Ported from Claude-of-Duty src/materials/shader.js: owHeightBlend in
// PARS_FRAGMENT and the #ifdef OW_DETILE block in MAIN_FRAGMENT.
//
// The caller composes these exactly where the source does:
//   1. axiom_detile_second_sample(...)          -- the extra fetches
//   2. axiom_detile_fold_detail_normal(...)     -- the detail normal, on sample two
//   3. axiom_detile_mask(...)                   -- the low-frequency crossover mask
//   4. axiom_detile_height_blend(&alb, &orm, &nT, ..., mask * detile)
//
// The whole block is emitted only when the detile amount is positive and the uv
// mode is not triplanar (the source's `#ifdef OW_DETILE`). It is NOT guarded at
// runtime: a zero blend amount is not bit-identical to omitting the block.
// ---------------------------------------------------------------------------

struct AxiomDetileSample {
    albedo: vec4<f32>,
    orm: vec3<f32>,
    normal: vec3<f32>,
};

// The de-correlating warp: a ~36.6-degree rotation (cos 0.803, sin 0.596 — not
// quite unit, and deliberately transcribed as written) followed by a 0.617
// rescale. Shared by the uv and both derivative vectors; only the uv also takes
// the (0.37, 0.71) offset.
fn axiom_detile_warp(v: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(v.x * 0.803 - v.y * 0.596, v.x * 0.596 + v.y * 0.803) * 0.617;
}

fn axiom_detile_uv(uv: vec2<f32>) -> vec2<f32> {
    return axiom_detile_warp(uv) + vec2<f32>(0.37, 0.71);
}

// Second sample of the same texture set, rotated and rescaled. textureSampleGrad
// and not textureSample: the warped uv has its own screen-space footprint, and
// inferring it would select the wrong mip.
fn axiom_detile_second_sample(
    base_map: texture_2d<f32>,
    base_sampler: sampler,
    rough_map: texture_2d<f32>,
    rough_sampler: sampler,
    normal_map: texture_2d<f32>,
    normal_sampler: sampler,
    uv: vec2<f32>,
    ddx: vec2<f32>,
    ddy: vec2<f32>,
    normal_amp: f32,
) -> AxiomDetileSample {
    let uv2 = axiom_detile_uv(uv);
    let ddx2 = axiom_detile_warp(ddx);
    let ddy2 = axiom_detile_warp(ddy);
    let alb2 = textureSampleGrad(base_map, base_sampler, uv2, ddx2, ddy2);
    let orm2 = textureSampleGrad(rough_map, rough_sampler, uv2, ddx2, ddy2).rgb;
    let raw2 = textureSampleGrad(normal_map, normal_sampler, uv2, ddx2, ddy2).xyz * 2.0 - 1.0;
    return AxiomDetileSample(alb2, orm2, vec3<f32>(raw2.xy * normal_amp, raw2.z));
}

fn axiom_detile_mask_uv(object_pos: vec3<f32>, macro_scale: f32) -> vec2<f32> {
    return (object_pos.xz + object_pos.y * 0.7) * macro_scale * 5.0 + 0.21;
}

// The crossover mask. The source reads this one with texture2D, NOT with the
// explicit-gradient macro, so it is textureSample here.
fn axiom_detile_mask(
    macro_map: texture_2d<f32>,
    macro_sampler: sampler,
    object_pos: vec3<f32>,
    macro_scale: f32,
) -> f32 {
    let muv = axiom_detile_mask_uv(object_pos, macro_scale);
    return clamp((textureSample(macro_map, macro_sampler, muv).g - 0.36) * 2.4, 0.0, 1.0);
}

// The micro detail normal, folded into the SECOND sample the same way the base
// sample already had it folded in, so the blend mixes like with like.
fn axiom_detile_fold_detail_normal(
    n2: vec3<f32>,
    dn: vec3<f32>,
    detail_normal_amt: f32,
    detail_fade: f32,
) -> vec3<f32> {
    return normalize(vec3<f32>(n2.xy + dn.xy * detail_normal_amt * detail_fade, n2.z));
}

// Height-preserving blend of two texture samples (kills the mushy 50% lerp).
//
// The source's three `inout` parameters are pointers here, and the writes keep
// the source's order and its read-before-write structure: `wa`/`wb` are read to
// form `k`, then both are overwritten using that same `k`, and `a` is read on
// the right of its own assignment.
fn axiom_detile_height_blend(
    a: ptr<function, vec4<f32>>,
    orm_a: ptr<function, vec3<f32>>,
    n_a: ptr<function, vec3<f32>>,
    b: vec4<f32>,
    orm_b: vec3<f32>,
    n_b: vec3<f32>,
    t: f32,
) {
    var wa = (1.0 - t) + (*a).a * 0.6;
    var wb = t + b.a * 0.6;
    let k = max(wa, wb) - 0.18;
    wa = max(wa - k, 0.0);
    wb = max(wb - k, 0.0);
    let inv = 1.0 / max(wa + wb, 1e-4);
    *a = ((*a) * wa + b * wb) * inv;
    *orm_a = ((*orm_a) * wa + orm_b * wb) * inv;
    *n_a = normalize(((*n_a) * wa + n_b * wb) * inv);
}
"#;

/// GLSL's `normalize`, which is `v / length(v)` with no error path — unlike
/// [`Vec3::normalize`], which is checked and fails on the zero vector. The
/// source calls the GLSL one, so this is the semantics being ported; a division,
/// not a reciprocal-multiply.
fn glsl_normalize(v: Vec3) -> Vec3 {
    let len = v.length();
    Vec3::new(v.x / len, v.y / len, v.z / len)
}

/// The source's `#ifdef OW_DETILE` condition:
/// `p.detile > 0 && p.uvMode !== 'triplanar'`.
///
/// This is a **compile-time** decision, exactly as in the source. When it is
/// false the de-tiling block must not be emitted at all — driving
/// [`height_blend`] with `t = 0` is not bit-identical to omitting it.
pub(crate) fn detile_enabled(detile: f32, triplanar: bool) -> bool {
    // Deferred to the layer so the rule has ONE definition. It has to live there
    // as well as here: `axiom_surface::SurfaceKind::code` needs it, because
    // de-tiled and un-de-tiled are two different programs and a surface's
    // identity is a layer concern. Two copies of a gate is how two materials
    // end up sharing a digest and rendering each other's shader.
    axiom_surface::MaterialParams {
        detile,
        uv_mode: [
            axiom_surface::UvMode::Planar,
            axiom_surface::UvMode::Triplanar,
        ][usize::from(triplanar)],
        ..axiom_surface::MaterialParams::default()
    }
    .detile_enabled()
}

/// The de-correlating rotate + rescale shared by `uv2`, `ddx2` and `ddy2`.
///
/// `vec2( v.x*0.803 - v.y*0.596, v.x*0.596 + v.y*0.803 ) * 0.617`
pub(crate) fn detile_warp(v: Vec2) -> Vec2 {
    Vec2::new(v.x * 0.803 - v.y * 0.596, v.x * 0.596 + v.y * 0.803).mul_scalar(0.617)
}

/// `uv2` — the warp plus the `(0.37, 0.71)` offset. The derivatives take the
/// warp *without* the offset, which is why the two are separate.
pub(crate) fn detile_uv(uv: Vec2) -> Vec2 {
    detile_warp(uv).add(Vec2::new(0.37, 0.71))
}

/// The second sample's normal: `texel.xyz * 2.0 - 1.0`, then `n2.xy *= owNormalAmp`.
pub(crate) fn detile_decode_normal(texel: Vec3, normal_amp: f32) -> Vec3 {
    let n = Vec3::new(
        texel.x * 2.0 - 1.0,
        texel.y * 2.0 - 1.0,
        texel.z * 2.0 - 1.0,
    );
    Vec3::new(n.x * normal_amp, n.y * normal_amp, n.z)
}

/// The `owMacroTex` lookup coordinate behind `dtm`:
/// `( owP.xz + owP.y * 0.7 ) * owMacroP.x * 5.0 + 0.21`.
pub(crate) fn detile_mask_uv(object_pos: Vec3, macro_scale: f32) -> Vec2 {
    Vec2::new(
        (object_pos.x + object_pos.y * 0.7) * macro_scale * 5.0 + 0.21,
        (object_pos.z + object_pos.y * 0.7) * macro_scale * 5.0 + 0.21,
    )
}

/// `dtm`, given the green channel already fetched:
/// `clamp( ( g - 0.36 ) * 2.4, 0.0, 1.0 )`.
pub(crate) fn detile_mask_from_green(green: f32) -> f32 {
    ((green - 0.36) * 2.4).clamp(0.0, 1.0)
}

/// The micro detail normal folded into the second sample:
/// `normalize( vec3( n2.xy + dn.xy * owDetailP.y * detFade, n2.z ) )`.
pub(crate) fn detile_fold_detail_normal(
    n2: Vec3,
    dn: Vec3,
    detail_normal_amt: f32,
    detail_fade: f32,
) -> Vec3 {
    glsl_normalize(Vec3::new(
        n2.x + dn.x * detail_normal_amt * detail_fade,
        n2.y + dn.y * detail_normal_amt * detail_fade,
        n2.z,
    ))
}

/// The three lanes `owHeightBlend` writes through its `inout` parameters, in the
/// order the source writes them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HeightBlend {
    /// `a` — albedo, with the height in `w`.
    pub(crate) albedo: Vec4,
    /// `ormA` — ao / roughness / metalness.
    pub(crate) orm: Vec3,
    /// `nA` — the tangent-space shading normal, renormalised.
    pub(crate) normal: Vec3,
}

/// `owHeightBlend`. Each sample's height buys it weight, all but the top `0.18`
/// is subtracted away, and the survivors are renormalised — so the taller sample
/// wins its pixels outright instead of both being averaged into mush.
///
/// `t` is `dtm * owRoughP.z` at the call site: the crossover mask times the
/// de-tiling amount.
pub(crate) fn height_blend(
    a: Vec4,
    orm_a: Vec3,
    n_a: Vec3,
    b: Vec4,
    orm_b: Vec3,
    n_b: Vec3,
    t: f32,
) -> HeightBlend {
    let wa0 = (1.0 - t) + a.w * 0.6;
    let wb0 = t + b.w * 0.6;
    let k = wa0.max(wb0) - 0.18;
    let wa = (wa0 - k).max(0.0);
    let wb = (wb0 - k).max(0.0);
    let inv = 1.0 / (wa + wb).max(1.0e-4);
    HeightBlend {
        albedo: a.mul_scalar(wa).add(b.mul_scalar(wb)).mul_scalar(inv),
        orm: orm_a.mul_scalar(wa).add(orm_b.mul_scalar(wb)).mul_scalar(inv),
        normal: glsl_normalize(n_a.mul_scalar(wa).add(n_b.mul_scalar(wb)).mul_scalar(inv)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rotation constants are the layer's identity: a different offset is a
    /// different texture. Pinned against values computed from the GLSL text by
    /// hand, in the source's grouping.
    #[test]
    fn the_warp_is_the_sources_rotation_rescale_and_offset() {
        let v = Vec2::new(0.75, -0.25);
        let expected = Vec2::new(
            (0.75_f32 * 0.803 - (-0.25_f32) * 0.596) * 0.617,
            (0.75_f32 * 0.596 + (-0.25_f32) * 0.803) * 0.617,
        );
        let warped = detile_warp(v);
        assert_eq!(warped.x, expected.x);
        assert_eq!(warped.y, expected.y);
        // The uv takes the offset; the derivatives do not.
        let uv2 = detile_uv(v);
        assert_eq!(uv2.x, expected.x + 0.37);
        assert_eq!(uv2.y, expected.y + 0.71);
    }

    /// It is a rotation, not a hash — the warp is linear, and the pair
    /// `(0.803, 0.596)` is very nearly (but transcribed as *not* exactly) unit.
    #[test]
    fn the_warp_is_linear_and_very_nearly_a_rotation() {
        let a = Vec2::new(1.0, 0.0);
        let b = Vec2::new(0.0, 1.0);
        let sum = detile_warp(Vec2::new(1.0, 1.0));
        let parts = detile_warp(a).add(detile_warp(b));
        assert!((sum.x - parts.x).abs() <= 1.0e-7);
        assert!((sum.y - parts.y).abs() <= 1.0e-7);
        let scale = detile_warp(a).length();
        assert!((scale - 0.617).abs() < 1.0e-4, "warp scale was {scale}");
        assert_ne!(0.803_f32 * 0.803 + 0.596_f32 * 0.596, 1.0);
    }

    /// The taller sample wins. With `t` at the midpoint, a sample whose height
    /// is `1.0` against one whose height is `0.0` takes essentially the whole
    /// pixel — that is the property the layer exists for.
    #[test]
    fn the_taller_sample_takes_the_pixel() {
        let tall = Vec4::new(1.0, 0.0, 0.0, 1.0);
        let short = Vec4::new(0.0, 0.0, 1.0, 0.0);
        let unit = Vec3::new(0.0, 0.0, 1.0);
        let out = height_blend(tall, Vec3::new(1.0, 0.0, 0.0), unit, short, Vec3::ZERO, unit, 0.5);
        assert!(out.albedo.x > 0.98, "red must dominate: {:?}", out.albedo);
        assert!(out.albedo.z < 0.02, "blue must be crushed: {:?}", out.albedo);
        // The winner follows the HEIGHT, not the slot: swapping the two samples
        // leaves the tall one (red) in charge.
        let back = height_blend(short, Vec3::ZERO, unit, tall, Vec3::new(1.0, 0.0, 0.0), unit, 0.5);
        assert!(back.albedo.x > 0.98, "red must still dominate: {:?}", back.albedo);
        assert!(back.albedo.z < 0.02, "blue must still be crushed: {:?}", back.albedo);
    }

    /// The blend is not a lerp: at equal heights and `t = 0.5` the two samples
    /// weigh the same, and the surviving weight is the documented `0.18` band.
    #[test]
    fn equal_heights_at_the_midpoint_weigh_the_same() {
        let a = Vec4::new(1.0, 0.0, 0.0, 0.5);
        let b = Vec4::new(0.0, 1.0, 0.0, 0.5);
        let unit = Vec3::new(0.0, 0.0, 1.0);
        let out = height_blend(a, Vec3::ZERO, unit, b, Vec3::ONE, unit, 0.5);
        assert!((out.albedo.x - 0.5).abs() < 1.0e-6, "{:?}", out.albedo);
        assert!((out.albedo.y - 0.5).abs() < 1.0e-6, "{:?}", out.albedo);
        assert!((out.orm.x - 0.5).abs() < 1.0e-6, "{:?}", out.orm);
    }

    /// The normal lane is the only one that is renormalised, and it is written
    /// last. Two unit normals blend to a unit normal.
    #[test]
    fn the_blended_normal_is_renormalised() {
        let unit_a = glsl_normalize(Vec3::new(0.3, -0.4, 0.86));
        let unit_b = glsl_normalize(Vec3::new(-0.5, 0.2, 0.84));
        let out = height_blend(
            Vec4::new(0.2, 0.3, 0.4, 0.4),
            Vec3::new(0.9, 0.5, 0.0),
            unit_a,
            Vec4::new(0.6, 0.1, 0.2, 0.7),
            Vec3::new(0.8, 0.6, 0.1),
            unit_b,
            0.35,
        );
        assert!((out.normal.length() - 1.0).abs() < 1.0e-6);
        // The albedo and orm lanes are NOT renormalised, so they stay inside the
        // convex hull of their inputs rather than being pushed to unit length.
        assert!((0.2..=0.6).contains(&out.albedo.x), "{:?}", out.albedo);
        assert!((0.0..=0.1).contains(&out.orm.z), "{:?}", out.orm);
    }

    /// **The `detile == 0` trap, measured rather than assumed.**
    ///
    /// A runtime `t = 0` leaves the surviving weight at `~0.18` and produces
    /// `a * 0.18 * (1 / 0.18)`. That round trip is off by one ulp for a large
    /// fraction of inputs, so it is **not** bit-identical to the un-detiled path
    /// — which is why [`detile_enabled`] gates the block at compile time, the
    /// way the source's `#ifdef OW_DETILE` does.
    #[test]
    fn a_runtime_zero_blend_is_not_bit_identical_to_the_undetiled_path() {
        let a = Vec4::new(0.794_379_47, 0.35, 0.61, 0.453_184_37);
        let b = Vec4::new(0.11, 0.22, 0.33, 0.299_767);
        let unit = Vec3::new(0.0, 0.0, 1.0);
        let out = height_blend(a, Vec3::new(0.4, 0.5, 0.6), unit, b, Vec3::ONE, unit, 0.0);
        // Close — but not the same bits.
        assert!((out.albedo.x - a.x).abs() < 1.0e-6);
        assert_ne!(
            out.albedo.x, a.x,
            "if this ever becomes exact the compile-time gate is still required, \
             because exactness here is an accident of these operands"
        );
        // The second sample contributes nothing at t = 0 — the error is purely
        // the weight round trip, not leakage from `b`.
        let b_free = height_blend(a, Vec3::new(0.4, 0.5, 0.6), unit, Vec4::ZERO, Vec3::ZERO, unit, 0.0);
        assert_eq!(out.albedo, b_free.albedo);
    }

    /// And the gate itself is the source's condition, both halves of it.
    #[test]
    fn the_layer_is_gated_on_a_positive_amount_and_a_non_triplanar_uv_mode() {
        assert!(detile_enabled(0.35, false));
        assert!(!detile_enabled(0.0, false), "DEFAULT_PARAMS.detile is 0");
        assert!(!detile_enabled(-0.1, false));
        assert!(!detile_enabled(0.35, true), "triplanar has its own path");
        assert!(!detile_enabled(0.0, true));
    }

    /// The `1e-4` guard is not decoration: a height large enough that
    /// `max - 0.18` rounds back to `max` collapses both weights to zero, and the
    /// guard is what keeps the result finite instead of `0/0`.
    #[test]
    fn the_epsilon_guard_keeps_a_collapsed_weight_finite() {
        let huge = Vec4::new(0.5, 0.25, 0.125, 1.0e9);
        let unit = Vec3::new(0.0, 0.0, 1.0);
        let out = height_blend(huge, Vec3::ONE, unit, huge, Vec3::ONE, unit, 0.5);
        assert_eq!(out.albedo, Vec4::ZERO);
        assert_eq!(out.orm, Vec3::ZERO);
        // The normal lane divides by a zero length, which GLSL leaves as NaN —
        // the source's own behaviour, ported rather than papered over.
        assert!(out.normal.x.is_nan());
    }

    /// The mask is a hard-clamped expansion of a narrow band of the macro
    /// texture's green channel: below `0.36` nothing, above `~0.7667` full.
    #[test]
    fn the_mask_expands_and_clamps_the_macro_green_band() {
        assert_eq!(detile_mask_from_green(0.0), 0.0);
        assert_eq!(detile_mask_from_green(0.36), 0.0);
        assert_eq!(detile_mask_from_green(1.0), 1.0);
        let mid = detile_mask_from_green(0.56);
        assert!((mid - (0.56_f32 - 0.36) * 2.4).abs() <= f32::EPSILON);
        assert!((f32::MIN_POSITIVE..1.0).contains(&mid), "mid was {mid}");
    }

    /// The mask's lookup coordinate mixes world height into both lanes, so a
    /// wall and the floor beneath it do not share a crossover pattern.
    #[test]
    fn the_mask_coordinate_folds_height_into_both_lanes() {
        let p = Vec3::new(2.0, 3.0, -4.0);
        let uv = detile_mask_uv(p, 0.045);
        assert_eq!(uv.x, (2.0_f32 + 3.0 * 0.7) * 0.045 * 5.0 + 0.21);
        assert_eq!(uv.y, (-4.0_f32 + 3.0 * 0.7) * 0.045 * 5.0 + 0.21);
        // Height alone moves both lanes.
        let raised = detile_mask_uv(Vec3::new(2.0, 4.0, -4.0), 0.045);
        assert_ne!(raised.x, uv.x);
        assert_ne!(raised.y, uv.y);
    }

    /// The normal decode scales only `xy`, and by the shared amplitude — `z` is
    /// left alone, which is what keeps the amplitude a tilt rather than a scale.
    #[test]
    fn the_second_samples_normal_decodes_and_scales_only_xy() {
        let n = detile_decode_normal(Vec3::new(0.75, 0.25, 1.0), 1.5);
        assert_eq!(n.x, (0.75_f32 * 2.0 - 1.0) * 1.5);
        assert_eq!(n.y, (0.25_f32 * 2.0 - 1.0) * 1.5);
        assert_eq!(n.z, 1.0_f32 * 2.0 - 1.0);
        // A flat texel decodes to +Z regardless of amplitude.
        let flat = detile_decode_normal(Vec3::new(0.5, 0.5, 1.0), 4.0);
        assert_eq!(flat, Vec3::new(0.0, 0.0, 1.0));
    }

    /// The detail fold adds the micro normal's `xy` into the second sample, at
    /// the detail strength times the distance fade, then renormalises.
    #[test]
    fn the_detail_fold_tilts_the_second_normal_and_renormalises() {
        let n2 = Vec3::new(0.1, -0.2, 0.97);
        let dn = Vec3::new(0.6, 0.4, 0.7);
        let out = detile_fold_detail_normal(n2, dn, 0.55, 0.8);
        assert!((out.length() - 1.0).abs() < 1.0e-6);
        let expected = glsl_normalize(Vec3::new(
            0.1_f32 + 0.6 * 0.55 * 0.8,
            -0.2_f32 + 0.4 * 0.55 * 0.8,
            0.97,
        ));
        assert_eq!(out, expected);
        // A zero fade leaves the direction alone (up to the renormalise).
        let unfaded = detile_fold_detail_normal(n2, dn, 0.55, 0.0);
        assert_eq!(unfaded, glsl_normalize(n2));
    }

    /// `glsl_normalize` is GLSL's, not [`Vec3::normalize`]'s: it has no error
    /// path and yields NaN on the zero vector rather than a `MathError`.
    #[test]
    fn glsl_normalize_has_no_error_path() {
        let v = Vec3::new(3.0, 4.0, 0.0);
        assert_eq!(glsl_normalize(v), Vec3::new(0.6, 0.8, 0.0));
        assert!(glsl_normalize(Vec3::ZERO).x.is_nan());
        assert!(Vec3::ZERO.normalize().is_err());
    }

    /// The WGSL text carries the constants that define the layer. If a
    /// transcription slip ever changed one, the shader would sample a different
    /// texture and no CPU test would notice — so the text itself is pinned.
    #[test]
    fn the_wgsl_carries_the_de_correlating_constants_and_the_explicit_gradients() {
        assert!(DETILE_WGSL
            .contains("vec2<f32>(v.x * 0.803 - v.y * 0.596, v.x * 0.596 + v.y * 0.803) * 0.617"));
        assert!(DETILE_WGSL.contains("+ vec2<f32>(0.37, 0.71)"));
        // Explicit derivatives on all three second-sample fetches, and nowhere a
        // plain textureSample of the material set.
        assert_eq!(DETILE_WGSL.matches("textureSampleGrad(").count(), 3);
        // The one implicit-derivative fetch is the crossover mask, which the
        // source also reads with plain texture2D.
        assert_eq!(DETILE_WGSL.matches("textureSample(").count(), 1);
        assert!(DETILE_WGSL.contains("textureSample(macro_map, macro_sampler, muv).g"));
        // The three inout lanes, as pointers.
        assert_eq!(DETILE_WGSL.matches("ptr<function,").count(), 3);
        assert!(DETILE_WGSL.contains("let inv = 1.0 / max(wa + wb, 1e-4);"));
        // The calling convention: free functions over explicit arguments, so the
        // orchestrator owns every binding. No globals of any kind here.
        ["@group", "@binding", "var<uniform>", "var<private>"]
            .iter()
            .for_each(|forbidden| {
                assert!(
                    !DETILE_WGSL.contains(forbidden),
                    "a layer must not reach for {forbidden}"
                );
            });
    }
}

/// **CPU↔GPU parity on a real adapter**, in the shape
/// `crate::surface_program::parity` establishes: acquire an adapter and fail
/// loudly rather than skip, render one fragment per sample into an
/// `Rgba32Float` target, read the lanes back, compare against the CPU reference
/// above at a tolerance derived from a measurement.
///
/// The texture set is filled **procedurally** so the CPU side can name the exact
/// texel the GPU fetched: four `64x64` `Rgba8Unorm` textures whose bytes come
/// from [`parity::texel`], sampled `Nearest`/`Repeat` with a single mip. Nearest
/// and unorm because both sides then agree *exactly* — `byte / 255.0` is
/// correctly rounded on either side, and no filter weights (which some hardware
/// evaluates in reduced precision) enter the comparison.
///
/// The cost of a single mip is that the gradients do not change the value, so
/// the gradients get their own two proofs: their arithmetic is compared lane by
/// lane against [`detile_warp`], and a separate two-mip texture proves the
/// warped gradients actually reach `textureSampleGrad` by selecting a different
/// level for a small and a large footprint.
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod parity {
    use super::*;

    /// One fragment per sample; also the render target's width.
    const SAMPLES: usize = 24;
    /// `vec4`s of uniform per sample.
    const LANES: usize = 10;
    /// Edge of the procedural material textures.
    const DIM: u32 = 64;
    /// Edge of the two-mip texture used for the gradient proof. Chosen so both
    /// levels' rows are a multiple of the 256-byte copy alignment.
    const MIP_DIM: u32 = 128;
    /// `copy_texture_to_buffer` row alignment.
    const ROW_ALIGN: u32 = 256;

    /// The **measured** absolute tolerance for this layer. See
    /// `docs/work-manifests/shmup-port/notes/material-detile.md`: the worst lane
    /// delta observed across every entry point is reported by
    /// `the_measured_worst_delta_justifies_the_tolerance`, and this budget is
    /// set from it, not fitted to a miss.
    ///
    /// Every compared lane is held under `4.0` in magnitude on purpose, so one
    /// absolute budget means the same thing everywhere: at that magnitude a
    /// single-rounding `fma` contraction — which the hardware is free to do and
    /// which is what the measurement actually sees — is worth about `2.4e-7`.
    const TOLERANCE: f32 = 1.0e-6;

    /// One parity sample's inputs.
    #[derive(Clone, Copy)]
    struct Inputs {
        uv: Vec2,
        ddx: Vec2,
        ddy: Vec2,
        a: Vec4,
        b: Vec4,
        orm_a: Vec3,
        orm_b: Vec3,
        n_a: Vec3,
        n_b: Vec3,
        dn: Vec3,
        object_pos: Vec3,
        t: f32,
        normal_amp: f32,
        detail_amt: f32,
        detail_fade: f32,
        macro_scale: f32,
        mip_grad: f32,
    }

    /// A deterministic `[0, 1)` from an integer, so the sample set is fixed
    /// across runs and machines.
    fn rnd(seed: u32) -> f32 {
        let x = seed.wrapping_mul(2_654_435_761).wrapping_add(0x9E37_79B9);
        let x = x ^ (x >> 15);
        let x = x.wrapping_mul(0x85EB_CA6B);
        let x = x ^ (x >> 13);
        (x % 100_003) as f32 / 100_003.0
    }

    /// Does a uv land clear of a texel boundary in both lanes?
    ///
    /// Nearest sampling at a boundary is a coin flip between two texels, and a
    /// CPU reference cannot honestly predict which way the hardware rounds. So
    /// the fixture stays away from boundaries — that is a property of the
    /// *fixture*, not a weakening of the comparison: away from a boundary the
    /// comparison is exact rather than tolerant.
    fn clears_texel_boundaries(uv: Vec2) -> bool {
        [uv.x, uv.y].iter().all(|c| {
            let u = c * DIM as f32;
            let frac = u - u.floor();
            (frac > 0.05) & (frac < 0.95)
        })
    }

    /// The first uv in a deterministic sequence whose *warped* counterpart
    /// clears the texel boundaries.
    fn clear_uv(index: u32) -> Vec2 {
        (0..64_u32)
            .map(|k| {
                let s = index + k * 7919;
                Vec2::new(rnd(s) * 6.0 - 3.0, rnd(s + 100) * 6.0 - 3.0)
            })
            .find(|uv| clears_texel_boundaries(detile_uv(*uv)))
            .expect("64 candidates must contain one clear of a texel boundary")
    }

    /// The same, for the mask's world position: the *mask* uv it derives must
    /// clear the boundaries. Positions stay within a few metres so every
    /// compared lane keeps a comparable float magnitude.
    fn clear_object_pos(index: u32, macro_scale: f32) -> Vec3 {
        (0..64_u32)
            .map(|k| {
                let s = index + k * 6197;
                Vec3::new(
                    rnd(s) * 8.0 - 4.0,
                    rnd(s + 100) * 4.0,
                    rnd(s + 200) * 8.0 - 4.0,
                )
            })
            .find(|p| clears_texel_boundaries(detile_mask_uv(*p, macro_scale)))
            .expect("64 candidates must contain one clear of a texel boundary")
    }

    /// The [`SAMPLES`] inputs, chosen to exercise what is easy to get wrong:
    /// negative uv (the warp and the repeat wrap must both survive it), a blend
    /// amount that runs past `1.0` (the source does not clamp `dtm * detile`),
    /// heights across the whole `0..=1` range, and gradients spanning fifteen
    /// octaves so the mip proof has both ends.
    fn inputs() -> Vec<Inputs> {
        (0..SAMPLES)
            .map(|index| {
                let i = index as u32;
                let f = index as f32;
                let macro_scale = 0.02 + rnd(i + 3300) * 0.08;
                Inputs {
                    uv: clear_uv(i),
                    ddx: Vec2::new(rnd(i + 200) * 0.01, rnd(i + 300) * 0.01 - 0.005),
                    ddy: Vec2::new(rnd(i + 400) * 0.01 - 0.005, rnd(i + 500) * 0.01),
                    a: Vec4::new(rnd(i + 600), rnd(i + 700), rnd(i + 800), f / 23.0),
                    b: Vec4::new(rnd(i + 900), rnd(i + 1000), rnd(i + 1100), 1.0 - f / 23.0),
                    orm_a: Vec3::new(rnd(i + 1200), rnd(i + 1300), rnd(i + 1400)),
                    orm_b: Vec3::new(rnd(i + 1500), rnd(i + 1600), rnd(i + 1700)),
                    n_a: glsl_normalize(Vec3::new(
                        rnd(i + 1800) * 2.0 - 1.0,
                        rnd(i + 1900) * 2.0 - 1.0,
                        rnd(i + 2000) + 0.4,
                    )),
                    n_b: glsl_normalize(Vec3::new(
                        rnd(i + 2100) * 2.0 - 1.0,
                        rnd(i + 2200) * 2.0 - 1.0,
                        rnd(i + 2300) + 0.4,
                    )),
                    dn: Vec3::new(
                        rnd(i + 2400) * 2.0 - 1.0,
                        rnd(i + 2500) * 2.0 - 1.0,
                        rnd(i + 2600),
                    ),
                    object_pos: clear_object_pos(i, macro_scale),
                    t: f * 0.061,
                    normal_amp: 0.4 + rnd(i + 3000) * 1.6,
                    detail_amt: rnd(i + 3100),
                    detail_fade: rnd(i + 3200),
                    macro_scale,
                    mip_grad: [1.0e-4_f32, 3.0][index % 2],
                }
            })
            .collect()
    }

    /// The uniform bytes: [`LANES`] `vec4`s per sample, in the order `fs_*`
    /// unpacks them.
    fn uniform_bytes(all: &[Inputs]) -> Vec<u8> {
        let mut bytes: Vec<u8> = all
            .iter()
            .flat_map(|s| {
                [
                    s.uv.x,
                    s.uv.y,
                    s.ddx.x,
                    s.ddx.y,
                    s.ddy.x,
                    s.ddy.y,
                    s.t,
                    s.normal_amp,
                    s.a.x,
                    s.a.y,
                    s.a.z,
                    s.a.w,
                    s.b.x,
                    s.b.y,
                    s.b.z,
                    s.b.w,
                    s.orm_a.x,
                    s.orm_a.y,
                    s.orm_a.z,
                    s.detail_amt,
                    s.orm_b.x,
                    s.orm_b.y,
                    s.orm_b.z,
                    s.detail_fade,
                    s.n_a.x,
                    s.n_a.y,
                    s.n_a.z,
                    s.macro_scale,
                    s.n_b.x,
                    s.n_b.y,
                    s.n_b.z,
                    s.mip_grad,
                    s.dn.x,
                    s.dn.y,
                    s.dn.z,
                    0.0,
                    s.object_pos.x,
                    s.object_pos.y,
                    s.object_pos.z,
                    0.0,
                ]
            })
            .flat_map(f32::to_le_bytes)
            .collect();
        bytes.resize(SAMPLES * LANES * 16, 0);
        bytes
    }

    /// The procedural texture: one byte quadruple per texel, per texture.
    /// `which` separates the four textures so a swapped binding cannot pass.
    fn texel(which: u32, x: u32, y: u32) -> [u8; 4] {
        [0_u32, 1, 2, 3].map(|channel| {
            let seed = which
                .wrapping_mul(0x9E37_79B1)
                ^ x.wrapping_mul(73_856_093)
                ^ y.wrapping_mul(19_349_663)
                ^ channel.wrapping_mul(83_492_791);
            (rnd(seed) * 255.0) as u8
        })
    }

    /// The CPU's model of `Nearest`/`Repeat` sampling of [`texel`].
    fn fetch(which: u32, uv: Vec2) -> Vec4 {
        let dim = DIM as i32;
        let x = (uv.x * DIM as f32).floor() as i32;
        let y = (uv.y * DIM as f32).floor() as i32;
        let bytes = texel(which, x.rem_euclid(dim) as u32, y.rem_euclid(dim) as u32);
        Vec4::new(
            f32::from(bytes[0]) / 255.0,
            f32::from(bytes[1]) / 255.0,
            f32::from(bytes[2]) / 255.0,
            f32::from(bytes[3]) / 255.0,
        )
    }

    /// The harness. One vertex stage, one entry point per thing under test.
    const HARNESS_WGSL: &str = r#"
struct DetileIn { items: array<vec4<f32>, 240> };
@group(0) @binding(0) var<uniform> inputs: DetileIn;
@group(0) @binding(1) var t_map: texture_2d<f32>;
@group(0) @binding(2) var t_rough: texture_2d<f32>;
@group(0) @binding(3) var t_normal: texture_2d<f32>;
@group(0) @binding(4) var t_macro: texture_2d<f32>;
@group(0) @binding(5) var t_mips: texture_2d<f32>;
@group(0) @binding(6) var samp: sampler;

fn lane(i: u32, n: u32) -> vec4<f32> { return inputs.items[i * 10u + n]; }

@vertex
fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

@fragment
fn fs_warp(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    return vec4<f32>(axiom_detile_uv(lane(i, 0u).xy), axiom_detile_warp(lane(i, 0u).zw));
}

@fragment
fn fs_warp_ddy(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    return vec4<f32>(axiom_detile_warp(lane(i, 1u).xy), 0.0, 0.0);
}

fn sample_of(i: u32) -> AxiomDetileSample {
    return axiom_detile_second_sample(
        t_map, samp, t_rough, samp, t_normal, samp,
        lane(i, 0u).xy, lane(i, 0u).zw, lane(i, 1u).xy, lane(i, 1u).w,
    );
}

@fragment
fn fs_sample_albedo(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return sample_of(u32(position.x)).albedo;
}

@fragment
fn fs_sample_orm(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(sample_of(u32(position.x)).orm, 0.0);
}

@fragment
fn fs_sample_normal(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(sample_of(u32(position.x)).normal, 0.0);
}

@fragment
fn fs_mask(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let p = lane(i, 9u).xyz;
    let s = lane(i, 6u).w;
    return vec4<f32>(axiom_detile_mask(t_macro, samp, p, s), axiom_detile_mask_uv(p, s), 0.0);
}

@fragment
fn fs_fold(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    return vec4<f32>(
        axiom_detile_fold_detail_normal(lane(i, 7u).xyz, lane(i, 8u).xyz, lane(i, 4u).w, lane(i, 5u).w),
        0.0,
    );
}

fn blend_of(i: u32) -> AxiomDetileSample {
    var a = lane(i, 2u);
    var orm = lane(i, 4u).xyz;
    var n = lane(i, 6u).xyz;
    axiom_detile_height_blend(&a, &orm, &n, lane(i, 3u), lane(i, 5u).xyz, lane(i, 7u).xyz, lane(i, 1u).z);
    return AxiomDetileSample(a, orm, n);
}

@fragment
fn fs_blend_albedo(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return blend_of(u32(position.x)).albedo;
}

@fragment
fn fs_blend_orm(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(blend_of(u32(position.x)).orm, 0.0);
}

@fragment
fn fs_blend_normal(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return vec4<f32>(blend_of(u32(position.x)).normal, 0.0);
}

// Proof that the WARPED gradients reach textureSampleGrad: the two-mip texture
// is one colour at level 0 and another at level 1, so the selected level is
// visible in the value.
@fragment
fn fs_mip(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let g = axiom_detile_warp(vec2<f32>(lane(i, 7u).w, 0.0));
    return textureSampleGrad(t_mips, samp, axiom_detile_uv(lane(i, 0u).xy), g, g);
}
"#;

    /// A real adapter, the textures, and the one pipeline layout they share.
    struct Gpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
        backend: wgpu::Backend,
        module: wgpu::ShaderModule,
        layout: wgpu::BindGroupLayout,
        bind_group: wgpu::BindGroup,
    }

    impl Gpu {
        fn acquire(all: &[Inputs]) -> Gpu {
            // The crate's ONE instance + adapter + device (see `crate::test_gpu`):
            // ~50 tests each opening their own is what crashes the driver.
            let gpu = crate::test_gpu::TestGpu::shared();
            let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
            let backend = gpu.backend;
            // The error scope is the SHARED device's, so it is entered exclusively;
            // see `crate::test_gpu::validating`.
            let (module, failure) = crate::test_gpu::validating(&device, || {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("axiom-detile-parity-shader"),
                    source: wgpu::ShaderSource::Wgsl([DETILE_WGSL, HARNESS_WGSL].concat().into()),
                })
            });
            let compile_error = failure;
            assert!(
                compile_error.is_none(),
                "the de-tiling WGSL must compile: {compile_error:?}"
            );

            let views: Vec<wgpu::TextureView> = (0..4)
                .map(|which| material_texture(&device, &queue, which))
                .chain(std::iter::once(mip_texture(&device, &queue)))
                .collect();
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("axiom-detile-parity-sampler"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                address_mode_w: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                mipmap_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            });
            let uniform = wgpu::util::DeviceExt::create_buffer_init(
                &device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("axiom-detile-parity-uniform"),
                    contents: &uniform_bytes(all),
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            let entries: Vec<wgpu::BindGroupLayoutEntry> =
                std::iter::once(wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                })
                .chain((1..=5).map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                }))
                .chain(std::iter::once(wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                }))
                .collect();
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("axiom-detile-parity-bgl"),
                entries: &entries,
            });
            let bind_entries: Vec<wgpu::BindGroupEntry> = std::iter::once(wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            })
            .chain(views.iter().enumerate().map(|(index, view)| {
                wgpu::BindGroupEntry {
                    binding: index as u32 + 1,
                    resource: wgpu::BindingResource::TextureView(view),
                }
            }))
            .chain(std::iter::once(wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::Sampler(&sampler),
            }))
            .collect();
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-detile-parity-bg"),
                layout: &layout,
                entries: &bind_entries,
            });
            Gpu {
                device,
                queue,
                backend,
                module,
                layout,
                bind_group,
            }
        }

        /// Render one entry point over a `SAMPLES x 1` `Rgba32Float` target and
        /// read every pixel's four lanes back.
        fn render(&self, entry_point: &str) -> Vec<[f32; 4]> {
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("axiom-detile-parity-pl"),
                        bind_group_layouts: &[&self.layout],
                        push_constant_ranges: &[],
                    });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("axiom-detile-parity-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &self.module,
                        entry_point: Some("vs"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &self.module,
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
                label: Some("axiom-detile-parity-target"),
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
                label: Some("axiom-detile-parity-readback"),
                size: u64::from(row_bytes),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("axiom-detile-parity-pass"),
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
                pass.set_bind_group(0, &self.bind_group, &[]);
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

    /// One `DIM x DIM` `Rgba8Unorm` texture filled from [`texel`].
    fn material_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        which: u32,
    ) -> wgpu::TextureView {
        let data: Vec<u8> = (0..DIM)
            .flat_map(|y| (0..DIM).flat_map(move |x| texel(which, x, y)))
            .collect();
        upload(device, queue, DIM, 1, &[data])
    }

    /// A `MIP_DIM` two-level texture: level 0 all red, level 1 all green. Which
    /// level a fetch lands on is therefore visible in the value.
    fn mip_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::TextureView {
        let level0 = vec![[255_u8, 0, 0, 255]; (MIP_DIM * MIP_DIM) as usize]
            .concat()
            .to_vec();
        let half = MIP_DIM / 2;
        let level1 = vec![[0_u8, 255, 0, 255]; (half * half) as usize]
            .concat()
            .to_vec();
        upload(device, queue, MIP_DIM, 2, &[level0, level1])
    }

    /// Create an `Rgba8Unorm` texture of `edge` and write every supplied level.
    fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        edge: u32,
        levels: u32,
        data: &[Vec<u8>],
    ) -> wgpu::TextureView {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-detile-parity-texture"),
            size: wgpu::Extent3d {
                width: edge,
                height: edge,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        data.iter().enumerate().for_each(|(level, bytes)| {
            let dim = edge >> level as u32;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(dim * 4),
                    rows_per_image: Some(dim),
                },
                wgpu::Extent3d {
                    width: dim,
                    height: dim,
                    depth_or_array_layers: 1,
                },
            );
        });
        texture.create_view(&wgpu::TextureViewDescriptor::default())
    }

    /// The worst absolute lane delta between two lane sets — the measurement a
    /// tolerance has to be derived from.
    fn worst_delta(cpu: &[[f32; 4]], gpu: &[[f32; 4]]) -> f32 {
        cpu.iter()
            .zip(gpu.iter())
            .flat_map(|(expected, actual)| {
                [0_usize, 1, 2, 3].map(|lane| (expected[lane] - actual[lane]).abs())
            })
            .fold(0.0_f32, f32::max)
    }

    fn assert_parity(what: &str, cpu: &[[f32; 4]], gpu: &[[f32; 4]]) {
        cpu.iter()
            .zip(gpu.iter())
            .enumerate()
            .for_each(|(sample, (expected, actual))| {
                (0..4).for_each(|lane| {
                    let delta = (expected[lane] - actual[lane]).abs();
                    assert!(
                        delta <= TOLERANCE,
                        "{what} disagrees at sample {sample} lane {lane}: \
                         CPU {} vs GPU {} (delta {delta}, tolerance {TOLERANCE})",
                        expected[lane],
                        actual[lane]
                    );
                });
            });
    }

    /// Every entry point's CPU reference, in the order the GPU renders them.
    fn expectations(all: &[Inputs]) -> Vec<(&'static str, Vec<[f32; 4]>)> {
        vec![
            (
                "fs_warp",
                all.iter()
                    .map(|s| {
                        let uv2 = detile_uv(s.uv);
                        let dx2 = detile_warp(s.ddx);
                        [uv2.x, uv2.y, dx2.x, dx2.y]
                    })
                    .collect(),
            ),
            (
                "fs_warp_ddy",
                all.iter()
                    .map(|s| {
                        let dy2 = detile_warp(s.ddy);
                        [dy2.x, dy2.y, 0.0, 0.0]
                    })
                    .collect(),
            ),
            (
                "fs_sample_albedo",
                all.iter()
                    .map(|s| {
                        let v = fetch(0, detile_uv(s.uv));
                        [v.x, v.y, v.z, v.w]
                    })
                    .collect(),
            ),
            (
                "fs_sample_orm",
                all.iter()
                    .map(|s| {
                        let v = fetch(1, detile_uv(s.uv));
                        [v.x, v.y, v.z, 0.0]
                    })
                    .collect(),
            ),
            (
                "fs_sample_normal",
                all.iter()
                    .map(|s| {
                        let v = fetch(2, detile_uv(s.uv));
                        let n =
                            detile_decode_normal(Vec3::new(v.x, v.y, v.z), s.normal_amp);
                        [n.x, n.y, n.z, 0.0]
                    })
                    .collect(),
            ),
            (
                "fs_mask",
                all.iter()
                    .map(|s| {
                        let muv = detile_mask_uv(s.object_pos, s.macro_scale);
                        let green = fetch(3, muv).y;
                        [detile_mask_from_green(green), muv.x, muv.y, 0.0]
                    })
                    .collect(),
            ),
            (
                "fs_fold",
                all.iter()
                    .map(|s| {
                        let n =
                            detile_fold_detail_normal(s.n_b, s.dn, s.detail_amt, s.detail_fade);
                        [n.x, n.y, n.z, 0.0]
                    })
                    .collect(),
            ),
            (
                "fs_blend_albedo",
                all.iter()
                    .map(|s| {
                        let out =
                            height_blend(s.a, s.orm_a, s.n_a, s.b, s.orm_b, s.n_b, s.t);
                        [out.albedo.x, out.albedo.y, out.albedo.z, out.albedo.w]
                    })
                    .collect(),
            ),
            (
                "fs_blend_orm",
                all.iter()
                    .map(|s| {
                        let out =
                            height_blend(s.a, s.orm_a, s.n_a, s.b, s.orm_b, s.n_b, s.t);
                        [out.orm.x, out.orm.y, out.orm.z, 0.0]
                    })
                    .collect(),
            ),
            (
                "fs_blend_normal",
                all.iter()
                    .map(|s| {
                        let out =
                            height_blend(s.a, s.orm_a, s.n_a, s.b, s.orm_b, s.n_b, s.t);
                        [out.normal.x, out.normal.y, out.normal.z, 0.0]
                    })
                    .collect(),
            ),
        ]
    }

    /// **The sweep.** Every WGSL entry point in this layer against its CPU
    /// reference, on a real adapter, at the measured tolerance.
    #[test]
    fn every_detile_entry_point_agrees_with_the_cpu_reference() {
        let all = inputs();
        let gpu = Gpu::acquire(&all);
        assert_ne!(
            gpu.backend,
            wgpu::Backend::Noop,
            "the parity proof is worthless unless a real backend ran it"
        );
        expectations(&all).iter().for_each(|(name, cpu)| {
            assert_parity(name, cpu, &gpu.render(name));
        });
    }

    /// The tolerance is derived from the measurement, not fitted to a miss: the
    /// worst delta across every entry point is reported here and must sit at
    /// least an order of magnitude *under* [`TOLERANCE`] — if it ever does not,
    /// the budget was set from a failure rather than from the hardware.
    #[test]
    fn the_measured_worst_delta_justifies_the_tolerance() {
        let all = inputs();
        let gpu = Gpu::acquire(&all);
        let measured: Vec<(&str, f32)> = expectations(&all)
            .iter()
            .map(|(name, cpu)| (*name, worst_delta(cpu, &gpu.render(name))))
            .collect();
        let table: Vec<String> = measured
            .iter()
            .map(|(name, delta)| format!("{name} {delta:e}"))
            .collect();
        let worst = measured
            .iter()
            .fold(("none", 0.0_f32), |acc, entry| {
                [acc, *entry][usize::from(entry.1 > acc.1)]
            });
        // The budget is derived from the hardware, not fitted to a miss: it must
        // sit at least 2x above what was measured (so a run-to-run wobble does
        // not turn into a red gate) and no more than 10x above it (a budget
        // looser than that is itself a failure, per the port's brief).
        assert!(
            worst.1 * 2.0 <= TOLERANCE,
            "worst delta {} (at {}) leaves under 2x headroom below {TOLERANCE}\n{}",
            worst.1,
            worst.0,
            table.join("\n")
        );
        assert!(
            worst.1 * 10.0 >= TOLERANCE,
            "the tolerance {TOLERANCE} is more than 10x the measured worst delta \
             {} (at {}); a budget that loose proves nothing\n{}",
            worst.1,
            worst.0,
            table.join("\n")
        );
    }

    /// The fixture is not vacuous: the twenty-four warped uvs must land on
    /// distinct texels, or the fetch parity would be comparing one texel
    /// twenty-four times and a broken `uv2` could still pass.
    #[test]
    fn the_second_sample_lands_on_many_distinct_texels() {
        let mut texels: Vec<(i32, i32)> = inputs()
            .iter()
            .map(|s| {
                let uv2 = detile_uv(s.uv);
                let dim = DIM as i32;
                (
                    ((uv2.x * DIM as f32).floor() as i32).rem_euclid(dim),
                    ((uv2.y * DIM as f32).floor() as i32).rem_euclid(dim),
                )
            })
            .collect();
        texels.sort_unstable();
        texels.dedup();
        assert!(
            texels.len() >= SAMPLES - 2,
            "only {} distinct texels across {SAMPLES} samples",
            texels.len()
        );
    }

    /// **The warped gradients really are handed to `textureSampleGrad`.** With a
    /// single-mip texture the derivatives change no value, so this is the proof
    /// that they are live: a tiny footprint reads level 0 (red) and a large one
    /// level 1 (green), through the same `axiom_detile_warp` the uv path uses.
    #[test]
    fn the_warped_gradients_select_the_mip_level() {
        let all = inputs();
        let gpu = Gpu::acquire(&all);
        let rendered = gpu.render("fs_mip");
        all.iter()
            .zip(rendered.iter())
            .enumerate()
            .for_each(|(index, (s, lanes))| {
                let big = s.mip_grad > 1.0;
                let expected = [[1.0_f32, 0.0], [0.0, 1.0]][usize::from(big)];
                assert!(
                    (lanes[0] - expected[0]).abs() < 1.0e-6
                        && (lanes[1] - expected[1]).abs() < 1.0e-6,
                    "sample {index} with gradient {} read {lanes:?}; expected level {}",
                    s.mip_grad,
                    usize::from(big)
                );
            });
    }

    /// A shader that will not compile must not slip through as a silently black
    /// draw: `Gpu::acquire` asserts on the validation scope, and this proves the
    /// scope is armed.
    #[test]
    fn the_validation_scope_catches_a_broken_shader() {
        let all = inputs();
        let gpu = Gpu::acquire(&all);
        // The error scope is the SHARED device's, so it is entered exclusively;
        // see `crate::test_gpu::validating`.
        let (_, failure) = crate::test_gpu::validating(&gpu.device, || {
            gpu
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: None,
                    source: wgpu::ShaderSource::Wgsl("fn broken( {".into()),
                })
        });
        assert!(failure.is_some());
    }
}
