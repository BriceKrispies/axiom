//! **tint, the wear material, and the final channel remap** — the tail of
//! `MAIN_FRAGMENT`, and the layer that hands `SurfaceOut` to the lighting stage.
//!
//! Ported from Claude-of-Duty `src/materials/shader.js`. Three source regions
//! land here, transcribed from the GLSL text:
//!
//! * **the wear material** (`shader.js:588-590`, inside `#ifdef OW_VCOL_MASKS`) —
//!   the wear *mask* is the `masks` layer's; what the mask **selects** is here.
//! * **the tint block** (`shader.js:620-624`) — the tint multiply and the
//!   roughness remap.
//! * **the final channel remap** (`shader.js:626-633` plus the `OVERRIDES` table
//!   at `shader.js:671-673`) — `alb`/`orm`/`nShade` into the six channels.
//!
//! Plus `normalStrength` (`shader.js:299`, `:357`, `:369`), which is one
//! operation applied at three upstream sites, and `alphaMask`
//! (`shader.js:632-634`), which is a cutout rather than a blend.
//!
//! ## The `wearMaterial` metalness default is a fixed bug, and it is pinned
//!
//! `DEFAULT_PARAMS.wearMaterial` is `[ roughness, METALNESS, unused, tint amount ]`
//! at full mask. Its metalness **used to default to `0.5`**, which turned every
//! worn edge on concrete, plaster, brick, timber, hessian and the road half
//! metal and gave it a specular tint it has no business having. Only the metal
//! library entries, which set their own `wearMaterial`, should ever raise it.
//! The source's default is now `0.0`; [`DEFAULT_WEAR_MATERIAL`] carries the
//! fixed value and
//! [`the_wear_material_metalness_default_is_zero_not_the_half_metal_bug`] is the
//! test that will not let it drift back.
//!
//! ## `tint` and `wearColor` are hex sRGB, converted by **three's** curve
//!
//! Both reach the shader through `new THREE.Color(hex)`, i.e. three's
//! `SRGBToLinear`: `c * 0.0773993808` below `0.04045` and
//! `(c * 0.9478672986 + 0.0521327014)^2.4` above. That is *algebraically* the
//! GLSL `(c + 0.055) / 1.055` form and **numerically different on 254 of the 256
//! byte values in f64** — the defect this port already found and fixed once on
//! the app side. [`srgb_hex_to_linear`] uses three's form, in `f64` (JavaScript's
//! width) narrowed to the `f32` a uniform carries.
//!
//! The honest measurement, pinned by
//! [`the_two_srgb_forms_differ_in_f64_but_not_once_narrowed_to_the_uniforms_f32`]:
//! the two forms differ on 254 of 256 bytes at `f64`, and on **none** of them
//! once narrowed to `f32`. The divergence sits below the uniform's resolution.
//! Three's form is still what is used, because it is what the source computes —
//! but the record says the fix was correctness, not pixels.
//!
//! ## The roughness remap's order is the specification
//!
//! `roughness` is `[scale, offset, minimum]`, reaching the shader as
//! `owRoughP = (scale, offset, detile, minimum)`. The source
//! (`shader.js:624`) is:
//!
//! ```glsl
//! orm.g = clamp( orm.g * owRoughP.x + owRoughP.y, max( owRoughP.w, 0.015 ), 1.0 );
//! ```
//!
//! Scale, **then** offset, **then** the minimum as the lower clamp bound — the
//! floor is applied *after* the offset, not before it, and it is itself floored
//! at a hard [`ROUGHNESS_HARD_FLOOR`] so tile, glass and painted metal stay
//! glossy enough to catch a highlight. A reordered remap is a different surface,
//! so the grouping is transcribed literally and
//! [`the_roughness_remap_offsets_before_it_floors`] pins the order by exhibiting
//! an input where the two orders disagree.
//!
//! ## `normalStrength` scales xy, never z
//!
//! `n.xy *= owNormalAmp` on a **tangent-space** normal, with no renormalise at
//! the site. Scaling z as well would renormalise to something plausible and
//! subtly flatten every surface in the frame.
//!
//! ## Storage width
//!
//! Every function here computes in `f32`, because the GPU does. The single
//! exception is [`srgb_hex_to_linear`], which mirrors JavaScript: `THREE.Color`
//! holds `f64` and the uniform upload narrows to `f32`, so the reference does
//! the same, in that order.

/// GLSL/WGSL `mix`, whose spec is `x * (1 - a) + y * a` — **not** the
/// `x + (y - x) * a` rearrangement, which is a different float expression.
fn mix(x: f32, y: f32, a: f32) -> f32 {
    x * (1.0 - a) + y * a
}

/// GLSL/WGSL `clamp`, whose spec is `min(max(x, low), high)`. Written out rather
/// than calling `f32::clamp`, which asserts on its bounds and is a different
/// function at the edges.
fn clamp(x: f32, low: f32, high: f32) -> f32 {
    x.max(low).min(high)
}

// ---------------------------------------------------------------------------
// The defaults, from `DEFAULT_PARAMS` (shader.js:697-778).
// ---------------------------------------------------------------------------

/// `wearMaterial` — `[ roughness, METALNESS, unused, tint amount ]` where the
/// wear mask is 1. See this module's header: the metalness is `0.0` because a
/// `0.5` default turned every worn edge on a non-metal half metal.
pub(crate) const DEFAULT_WEAR_MATERIAL: [f32; 4] = [0.42, 0.0, 0.0, 0.5];

/// `tint` — `0xffffff`, i.e. the identity multiply. (shader.js:770)
pub(crate) const DEFAULT_TINT_HEX: u32 = 0x00ff_ffff;

/// `wearColor` — `0x8d8b86`, the source's rubbed-through grey. (shader.js:766)
pub(crate) const DEFAULT_WEAR_COLOR_HEX: u32 = 0x008d_8b86;

/// `normalStrength` — `1`. (shader.js:771)
pub(crate) const DEFAULT_NORMAL_STRENGTH: f32 = 1.0;

/// `roughness` — `[ scale, offset, minimum ]`. (shader.js:773)
pub(crate) const DEFAULT_ROUGHNESS: [f32; 3] = [1.0, 0.0, 0.06];

/// `alphaMask` — off. (shader.js:775)
pub(crate) const DEFAULT_ALPHA_MASK: bool = false;

/// The hard floor under the per-surface roughness minimum (shader.js:624).
pub(crate) const ROUGHNESS_HARD_FLOOR: f32 = 0.015;

/// The cutout threshold. `alphaMask` is a **define**; the threshold is three's
/// `material.alphaTest`, and the only library entry that sets `alphaMask: true`
/// — `foliage` — sets `alphaTest: 0.45` beside it (`materials/library.js:328`,
/// `:335`). three's `<alphatest_fragment>` is `if ( diffuseColor.a < alphaTest )
/// discard;` — strictly less-than, and a discard, never a blend.
pub(crate) const FOLIAGE_ALPHA_TEST: f32 = 0.45;

/// `owRoughP` — the vec4 the tint block reads, assembled at `shader.js:833-840`
/// as `(roughness[0], roughness[1], detile, roughness[2])`. `detile` rides in
/// `.z` and is the `detile` layer's; this layer reads `.x`, `.y` and `.w`.
pub(crate) fn rough_p(roughness: [f32; 3], detile: f32) -> [f32; 4] {
    [roughness[0], roughness[1], detile, roughness[2]]
}

// ---------------------------------------------------------------------------
// The colour conversion the CPU does before the uniform is uploaded.
// ---------------------------------------------------------------------------

/// `new THREE.Color(hex)` — a hex sRGB triple in the linear working space.
///
/// three's `Color.setHex` takes each byte over 255, then
/// `ColorManagement.colorSpaceToWorking` applies `SRGBToLinear`
/// (`three.core.js:6491`):
///
/// ```js
/// ( c < 0.04045 ) ? c * 0.0773993808 : Math.pow( c * 0.9478672986 + 0.0521327014, 2.4 )
/// ```
///
/// **Three's form, not the GLSL `(c + 0.055) / 1.055` form.** The two are
/// algebraically equal and numerically different; see this module's header for
/// the measurement. `f64` throughout, because JavaScript is, then narrowed once
/// at the end, because the uniform is `f32`.
pub(crate) fn srgb_hex_to_linear(hex: u32) -> [f32; 3] {
    [16_u32, 8, 0].map(|shift| {
        let c = f64::from((hex >> shift) & 255) / 255.0;
        let below = c * 0.0773993808;
        let above = (c * 0.9478672986 + 0.0521327014).powf(2.4);
        // Branchless selection over the 0.04045 knee: a table read, not an `if`.
        [above, below][usize::from(c < 0.04045)] as f32
    })
}

// ---------------------------------------------------------------------------
// The CPU reference. Each function is one line of the source's GLSL.
// ---------------------------------------------------------------------------

/// `shader.js:588` — `alb.rgb = mix( alb.rgb, owWearCol, wearM * owWearMat.w );`
///
/// `wear_material[3]` is the *tint amount*: how far the rubbed-through colour
/// reaches at full mask.
pub(crate) fn wear_albedo(
    alb: [f32; 3],
    wear_color: [f32; 3],
    wear_mask: f32,
    wear_material: [f32; 4],
) -> [f32; 3] {
    [0_usize, 1, 2].map(|lane| mix(alb[lane], wear_color[lane], wear_mask * wear_material[3]))
}

/// `shader.js:589-590` — the wear material's roughness and **metalness**.
///
/// ```glsl
/// orm.g = mix( orm.g, owWearMat.x, wearM );
/// orm.b = mix( orm.b, owWearMat.y, wearM );
/// ```
///
/// `orm` is `(ao, roughness, metalness)`; `.r` is not touched here. This is the
/// site the half-metal defect lived at: with `wear_material[1] == 0.5` every
/// fully-worn texel on a non-metal came out at metalness `0.5`.
pub(crate) fn wear_orm(orm: [f32; 3], wear_mask: f32, wear_material: [f32; 4]) -> [f32; 3] {
    [
        orm[0],
        mix(orm[1], wear_material[0], wear_mask),
        mix(orm[2], wear_material[1], wear_mask),
    ]
}

/// `shader.js:299`, `:357`, `:369` — `n.xy *= owNormalAmp;`
///
/// A **tangent-space** normal's xy, and only its xy. `z` is left exactly as it
/// was and nothing is renormalised at the site; the source renormalises later,
/// after the detail layer has added its own xy.
pub(crate) fn normal_strength(n: [f32; 3], amp: f32) -> [f32; 3] {
    [n[0] * amp, n[1] * amp, n[2]]
}

/// `shader.js:621` — `alb.rgb *= owTintCol;`
pub(crate) fn tint(alb: [f32; 3], tint_color: [f32; 3]) -> [f32; 3] {
    [0_usize, 1, 2].map(|lane| alb[lane] * tint_color[lane])
}

/// `shader.js:624` — scale, then offset, then floor.
///
/// ```glsl
/// orm.g = clamp( orm.g * owRoughP.x + owRoughP.y, max( owRoughP.w, 0.015 ), 1.0 );
/// ```
pub(crate) fn roughness_remap(roughness: f32, rough_p: [f32; 4]) -> f32 {
    clamp(
        roughness * rough_p[0] + rough_p[1],
        rough_p[3].max(ROUGHNESS_HARD_FLOOR),
        1.0,
    )
}

/// three's `<alphatest_fragment>`: `if ( diffuseColor.a < alphaTest ) discard;`
///
/// A **cutout**. `true` means the fragment is discarded outright — it neither
/// shades nor writes depth — not that it is blended at `alpha`.
pub(crate) fn alpha_cut(alpha: f32, alpha_test: f32) -> bool {
    alpha < alpha_test
}

/// The six channels this layer hands the lighting stage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Channels {
    /// `diffuseColor.rgb *= owAlbedo.rgb` (shader.js:631). The alpha lane rides
    /// along unchanged; the scene shader reads only `.rgb` from it and takes the
    /// alpha from `opacity`.
    pub(crate) base_color: [f32; 4],
    /// `float roughnessFactor = roughness * owORM.g;` (shader.js:671).
    pub(crate) roughness: f32,
    /// `float metalnessFactor = metalness * owORM.b;` (shader.js:672).
    pub(crate) metallic: f32,
    /// `normal = owNormalV;` (shader.js:673).
    pub(crate) normal: [f32; 3],
    /// Passed through; the source's emissive never enters `MAIN_FRAGMENT`.
    pub(crate) emission: [f32; 3],
    /// `diffuseColor.a`, times `owAlbedo.a` under `OW_ALPHA_MASK`
    /// (shader.js:632-634).
    pub(crate) opacity: f32,
}

/// The final channel remap: `alb`/`orm`/`nShade` into the six channels.
///
/// `alpha_mask` is `0.0`/`1.0` — the runtime stand-in for the source's
/// `#ifdef OW_ALPHA_MASK`, so that one program serves both. At `0.0` the
/// `mix` is an exact `1.0` and at `1.0` an exact `alb[3]`, so neither case
/// pays a rounding for the other's existence.
///
/// **Three of the source's factors are absent on purpose, not dropped.** three
/// multiplies `owAlbedo.rgb` by the material's `diffuse`, `owORM.g` by its
/// `roughness` and `owORM.b` by its `metalness`; `materials/index.js:203-207`
/// constructs *every* extended material at `color: 0xffffff, roughness: 1,
/// metalness: 1` and no library entry overrides any of the three, so all three
/// are the identity for every surface that exists. `material_opacity` is the one
/// that genuinely varies (`library.js:377` sets `0.22` on glass), so it is a
/// parameter.
pub(crate) fn channels(
    alb: [f32; 4],
    orm: [f32; 3],
    shade_normal: [f32; 3],
    emission: [f32; 3],
    material_opacity: f32,
    alpha_mask: f32,
) -> Channels {
    Channels {
        base_color: alb,
        roughness: orm[1],
        metallic: orm[2],
        normal: shade_normal,
        emission,
        opacity: material_opacity * mix(1.0, alb[3], alpha_mask),
    }
}

/// The tint block and the channel remap, **in the source's order**: tint the
/// albedo (`shader.js:621`), remap the roughness (`:624`), then assign
/// (`:626-628`).
#[allow(clippy::too_many_arguments, reason = "\
    one argument per value the source's tint block reads; packing them into a \
    struct would invent a data contract this layer does not own — the params \
    layer owns the packing, and this layer is called with explicit arguments \
    exactly so twelve layers can be written without sharing a file")]
pub(crate) fn finish(
    alb: [f32; 4],
    orm: [f32; 3],
    shade_normal: [f32; 3],
    emission: [f32; 3],
    tint_color: [f32; 3],
    rough_p: [f32; 4],
    material_opacity: f32,
    alpha_mask: f32,
) -> Channels {
    let tinted = tint([alb[0], alb[1], alb[2]], tint_color);
    let remapped = roughness_remap(orm[1], rough_p);
    channels(
        [tinted[0], tinted[1], tinted[2], alb[3]],
        [orm[0], remapped, orm[2]],
        shade_normal,
        emission,
        material_opacity,
        alpha_mask,
    )
}

// ---------------------------------------------------------------------------
// The WGSL.
// ---------------------------------------------------------------------------

/// The WGSL for this layer: free functions taking explicit arguments, composed
/// by the orchestrator into `axiom_surface`. Nothing here reads a global, a
/// binding index or `params.slots`.
///
/// `axiom_mat_channels` and `axiom_mat_finish` name `SurfaceOut`, which
/// `surface_program::wgsl_template::SURFACE_PRELUDE_WGSL` declares — the same
/// prelude every generated program is written against.
pub(crate) const TINT_WEAR_WGSL: &str = r#"
// ---------------------------------------------------------------------------
// tint / wearMaterial / normalStrength / alphaMask / the final channel remap.
// Claude-of-Duty src/materials/shader.js, MAIN_FRAGMENT :588-590 and :620-634,
// plus the OVERRIDES table at :671-673.
// ---------------------------------------------------------------------------

// shader.js:588 — alb.rgb = mix( alb.rgb, owWearCol, wearM * owWearMat.w );
// wear_material.w is the TINT AMOUNT, not a colour.
fn axiom_mat_wear_albedo(
    alb: vec3<f32>,
    wear_color: vec3<f32>,
    wear_mask: f32,
    wear_material: vec4<f32>,
) -> vec3<f32> {
    return mix(alb, wear_color, wear_mask * wear_material.w);
}

// shader.js:589-590 — the wear material's ROUGHNESS (.x) and METALNESS (.y).
//   orm.g = mix( orm.g, owWearMat.x, wearM );
//   orm.b = mix( orm.b, owWearMat.y, wearM );
// `orm` is (ao, roughness, metalness); .r is untouched here. The metalness
// default is 0.0 and must stay 0.0: at 0.5 every worn edge on concrete,
// plaster, brick, timber, hessian and the road turned half metal.
fn axiom_mat_wear_orm(orm: vec3<f32>, wear_mask: f32, wear_material: vec4<f32>) -> vec3<f32> {
    return vec3<f32>(
        orm.r,
        mix(orm.g, wear_material.x, wear_mask),
        mix(orm.b, wear_material.y, wear_mask),
    );
}

// shader.js:299 / :357 / :369 — n.xy *= owNormalAmp;
// TANGENT-space xy only. z is left alone and nothing is renormalised here.
fn axiom_mat_normal_strength(n: vec3<f32>, amp: f32) -> vec3<f32> {
    return vec3<f32>(n.x * amp, n.y * amp, n.z);
}

// shader.js:621 — alb.rgb *= owTintCol;
fn axiom_mat_tint(alb: vec3<f32>, tint_color: vec3<f32>) -> vec3<f32> {
    return alb * tint_color;
}

// shader.js:624 — scale, then offset, then the minimum as the LOWER CLAMP BOUND.
//   orm.g = clamp( orm.g * owRoughP.x + owRoughP.y, max( owRoughP.w, 0.015 ), 1.0 );
// owRoughP = ( roughness[0] scale, roughness[1] offset, detile, roughness[2] min ).
// The 0.015 is a hard floor under the per-surface floor: tile, glass and painted
// metal must stay glossy enough to actually catch a highlight.
fn axiom_mat_roughness_remap(roughness: f32, rough_p: vec4<f32>) -> f32 {
    return clamp(roughness * rough_p.x + rough_p.y, max(rough_p.w, 0.015), 1.0);
}

// three's <alphatest_fragment>: `if ( diffuseColor.a < alphaTest ) discard;`.
// A CUTOUT, strictly less-than — the caller discards, it does not blend.
fn axiom_mat_alpha_cut(alpha: f32, alpha_test: f32) -> bool {
    return alpha < alpha_test;
}

// shader.js:626-634 + the OVERRIDES at :671-673 — the final channel remap.
// `alpha_mask` is 0.0/1.0, the runtime stand-in for #ifdef OW_ALPHA_MASK; at 0.0
// the mix is an exact 1.0 and at 1.0 an exact alb.a.
fn axiom_mat_channels(
    alb: vec4<f32>,
    orm: vec3<f32>,
    shade_normal: vec3<f32>,
    emission: vec3<f32>,
    material_opacity: f32,
    alpha_mask: f32,
) -> SurfaceOut {
    var out: SurfaceOut;
    out.base_color = alb;
    out.roughness = orm.g;
    out.metallic = orm.b;
    out.normal = shade_normal;
    out.emission = emission;
    out.opacity = material_opacity * mix(1.0, alb.a, alpha_mask);
    return out;
}

// The tint block then the remap, in the source's order: tint (:621), roughness
// remap (:624), assign (:626-628).
fn axiom_mat_finish(
    alb: vec4<f32>,
    orm: vec3<f32>,
    shade_normal: vec3<f32>,
    emission: vec3<f32>,
    tint_color: vec3<f32>,
    rough_p: vec4<f32>,
    material_opacity: f32,
    alpha_mask: f32,
) -> SurfaceOut {
    let tinted = axiom_mat_tint(alb.rgb, tint_color);
    let remapped = axiom_mat_roughness_remap(orm.g, rough_p);
    return axiom_mat_channels(
        vec4<f32>(tinted, alb.a),
        vec3<f32>(orm.r, remapped, orm.b),
        shade_normal,
        emission,
        material_opacity,
        alpha_mask,
    );
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wear_material_metalness_default_is_zero_not_the_half_metal_bug() {
        // The fixed default, byte for byte: [ roughness, METALNESS, unused, tint ].
        assert_eq!(DEFAULT_WEAR_MATERIAL, [0.42, 0.0, 0.0, 0.5]);
        // And the behaviour, at the site that mattered: a fully-worn concrete
        // texel keeps its metalness at zero.
        let concrete = [0.9, 0.85, 0.04];
        let worn = wear_orm(concrete, 1.0, DEFAULT_WEAR_MATERIAL);
        assert_eq!(worn[2], 0.0, "a worn edge on a non-metal is not metal");
        assert_eq!(worn[1], 0.42, "and it takes the wear roughness");
        assert_eq!(worn[0], 0.9, "ao is not the wear material's business");
        // The bug it replaced, exhibited so the two cannot be confused: the old
        // 0.5 default made every fully-worn texel half metal.
        let bugged = wear_orm(concrete, 1.0, [0.42, 0.5, 0.0, 0.5]);
        assert_eq!(bugged[2], 0.5);
    }

    #[test]
    fn the_wear_material_lerps_by_the_mask_and_the_albedo_by_the_tint_amount() {
        let alb = [0.6, 0.5, 0.4];
        let wear_color = srgb_hex_to_linear(DEFAULT_WEAR_COLOR_HEX);
        // Mask 0 leaves everything exactly alone.
        assert_eq!(wear_albedo(alb, wear_color, 0.0, DEFAULT_WEAR_MATERIAL), alb);
        // Mask 1 reaches only as far as the tint amount (0.5), not all the way.
        let full = wear_albedo(alb, wear_color, 1.0, DEFAULT_WEAR_MATERIAL);
        (0..3).for_each(|lane| {
            assert!((full[lane] - mix(alb[lane], wear_color[lane], 0.5)).abs() < 1e-7);
        });
        // Halfway on both mask and roughness.
        let orm = wear_orm([0.7, 0.9, 0.1], 0.5, DEFAULT_WEAR_MATERIAL);
        assert!((orm[1] - mix(0.9, 0.42, 0.5)).abs() < 1e-7);
        assert!((orm[2] - mix(0.1, 0.0, 0.5)).abs() < 1e-7);
    }

    #[test]
    fn normal_strength_scales_xy_and_leaves_z_exactly_alone() {
        let n = [0.3, -0.4, 0.866];
        let scaled = normal_strength(n, 2.5);
        assert_eq!(scaled, [0.3 * 2.5, -0.4 * 2.5, 0.866]);
        // The failure mode this pins: scaling z too renormalises to something
        // plausible and flattens the surface. The z lane must be untouched.
        assert_eq!(scaled[2], n[2]);
        assert_eq!(normal_strength(n, DEFAULT_NORMAL_STRENGTH), n);
    }

    #[test]
    fn the_roughness_remap_offsets_before_it_floors() {
        // scale 2, offset +0.5, minimum 0.2 — chosen so each wrong order lands
        // somewhere visibly different, and each wrong order is COMPUTED here
        // rather than asserted about in a comment.
        let p = rough_p([2.0, 0.5, 0.2], 0.0);
        let input = 0.02_f32;
        let correct = roughness_remap(input, p);
        // scale, offset, then the floor as the lower clamp bound.
        assert!((correct - 0.54).abs() < 1e-7);

        // Wrong order 1: floor the input BEFORE the offset.
        let floor_first = clamp(
            input.max(p[3].max(ROUGHNESS_HARD_FLOOR)) * p[0] + p[1],
            0.0,
            1.0,
        );
        assert!((floor_first - 0.9).abs() < 1e-7);
        assert!((correct - floor_first).abs() > 0.3, "the orders differ");

        // Wrong order 2: offset BEFORE the scale.
        let offset_first = clamp((input + p[1]) * p[0], p[3], 1.0);
        assert!((offset_first - 1.0).abs() < 1e-7, "it saturates");
        assert!(correct < offset_first, "the orders differ");
    }

    #[test]
    fn the_roughness_floor_is_the_per_surface_minimum_over_a_hard_0_015() {
        // A per-surface minimum above the hard floor wins.
        let glossy = rough_p([1.0, 0.0, 0.3], 0.0);
        assert_eq!(roughness_remap(0.0, glossy), 0.3);
        // A per-surface minimum below it does not: 0.015 is the hard floor.
        let mirror = rough_p([1.0, 0.0, 0.0], 0.0);
        assert_eq!(roughness_remap(0.0, mirror), ROUGHNESS_HARD_FLOOR);
        assert_eq!(roughness_remap(-5.0, mirror), ROUGHNESS_HARD_FLOOR);
        // And the upper bound is 1.0.
        assert_eq!(roughness_remap(4.0, mirror), 1.0);
        // The default remap is the identity over the interior.
        let default = rough_p(DEFAULT_ROUGHNESS, 0.0);
        assert_eq!(roughness_remap(0.5, default), 0.5);
        assert_eq!(roughness_remap(0.0, default), DEFAULT_ROUGHNESS[2]);
    }

    #[test]
    fn rough_p_carries_detile_in_the_lane_the_source_gives_it() {
        // shader.js:833-840 — (roughness[0], roughness[1], detile, roughness[2]).
        assert_eq!(rough_p([1.5, -0.25, 0.06], 0.8), [1.5, -0.25, 0.8, 0.06]);
    }

    #[test]
    fn the_tint_is_a_plain_componentwise_multiply_and_white_is_the_identity() {
        let alb = [0.2, 0.55, 0.9];
        assert_eq!(tint(alb, srgb_hex_to_linear(DEFAULT_TINT_HEX)), alb);
        assert_eq!(tint(alb, [0.5, 2.0, 0.0]), [0.1, 1.1, 0.0]);
    }

    #[test]
    fn the_hex_colours_convert_through_threes_curve_not_the_glsl_one() {
        // Three's SRGBToLinear at f64, narrowed once — the value a THREE.Color
        // uniform actually uploads.
        assert_eq!(srgb_hex_to_linear(DEFAULT_TINT_HEX), [1.0, 1.0, 1.0]);
        assert_eq!(srgb_hex_to_linear(0x0000_0000), [0.0, 0.0, 0.0]);
        let wear = srgb_hex_to_linear(DEFAULT_WEAR_COLOR_HEX);
        // Captured from node with three's own expression on 0x8d8b86.
        let expected = [0.266_355_6_f32, 0.258_182_85, 0.238_397_57];
        (0..3).for_each(|lane| assert!((wear[lane] - expected[lane]).abs() < 1e-7));
        // The below-knee arm: byte 8 is under 0.04045 * 255 = 10.31.
        let dark = srgb_hex_to_linear(0x0008_0000);
        assert!((f64::from(dark[0]) - (8.0 / 255.0) * 0.0773993808).abs() < 1e-9);
        assert_eq!(dark[1], 0.0);
    }

    #[test]
    fn the_two_srgb_forms_differ_in_f64_but_not_once_narrowed_to_the_uniforms_f32() {
        // The GLSL form, written out here in the test only — this is the form the
        // port must NOT use, kept so the divergence is a measurement rather than
        // a claim.
        let glsl = |c: f64| -> f64 {
            [((c + 0.055) / 1.055).powf(2.4), c / 12.92][usize::from(c < 0.04045)]
        };
        let three = |c: f64| -> f64 {
            [
                (c * 0.9478672986 + 0.0521327014).powf(2.4),
                c * 0.0773993808,
            ][usize::from(c < 0.04045)]
        };
        let agree_f64 = (0..256).filter(|b| {
            let c = f64::from(*b) / 255.0;
            glsl(c) == three(c)
        });
        // 254 of 256 differ; the two that agree are the endpoints.
        assert_eq!(agree_f64.clone().count(), 2);
        assert_eq!(agree_f64.collect::<Vec<i32>>(), vec![0, 255]);
        // But narrowed to the f32 a uniform carries, every one of the 256 agrees.
        // Three's form is still what is used, because it is what the source
        // computes — the record is that the fix was correctness, not pixels.
        let agree_f32 = (0..256)
            .filter(|b| {
                let c = f64::from(*b) / 255.0;
                glsl(c) as f32 == three(c) as f32
            })
            .count();
        assert_eq!(agree_f32, 256);
    }

    #[test]
    fn alpha_mask_is_a_cutout_at_a_strict_less_than() {
        assert!(!DEFAULT_ALPHA_MASK, "alphaMask is off by default");
        assert_eq!(FOLIAGE_ALPHA_TEST, 0.45);
        assert!(alpha_cut(0.449, FOLIAGE_ALPHA_TEST));
        // Strictly less-than: exactly at the threshold the fragment SURVIVES.
        assert!(!alpha_cut(FOLIAGE_ALPHA_TEST, FOLIAGE_ALPHA_TEST));
        assert!(!alpha_cut(1.0, FOLIAGE_ALPHA_TEST));
    }

    #[test]
    fn the_channel_remap_routes_orm_g_to_roughness_and_orm_b_to_metallic() {
        let out = channels(
            [0.1, 0.2, 0.3, 0.4],
            [0.7, 0.55, 0.9],
            [0.0, 0.0, 1.0],
            [0.05, 0.0, 0.02],
            1.0,
            0.0,
        );
        assert_eq!(out.base_color, [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(out.roughness, 0.55, "orm.g is roughness");
        assert_eq!(out.metallic, 0.9, "orm.b is metalness");
        assert_eq!(out.normal, [0.0, 0.0, 1.0]);
        assert_eq!(out.emission, [0.05, 0.0, 0.02]);
        // ao (orm.r = 0.7) has no SurfaceOut lane; see the note file.
    }

    #[test]
    fn opacity_takes_the_albedo_alpha_only_when_the_alpha_mask_is_on() {
        let call = |alpha_mask, material_opacity| {
            channels(
                [1.0, 1.0, 1.0, 0.3],
                [1.0, 0.5, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0, 0.0],
                material_opacity,
                alpha_mask,
            )
            .opacity
        };
        // Off: the material's own opacity, exactly, with no rounding paid.
        assert_eq!(call(0.0, 1.0), 1.0);
        assert_eq!(call(0.0, 0.22), 0.22);
        // On: times the albedo alpha, exactly.
        assert_eq!(call(1.0, 1.0), 0.3);
        assert_eq!(call(1.0, 0.5), 0.15);
    }

    #[test]
    fn finish_tints_before_it_remaps_the_roughness_and_leaves_the_rest_alone() {
        let out = finish(
            [0.5, 0.5, 0.5, 0.8],
            [0.6, 0.25, 0.1],
            [0.1, -0.2, 0.97],
            [0.0, 0.03, 0.0],
            [2.0, 1.0, 0.5],
            rough_p([2.0, 0.1, 0.4], 0.0),
            1.0,
            1.0,
        );
        assert_eq!(out.base_color, [1.0, 0.5, 0.25, 0.8]);
        // 0.25 * 2 + 0.1 = 0.6, above the 0.4 floor.
        assert!((out.roughness - 0.6).abs() < 1e-7);
        assert_eq!(out.metallic, 0.1);
        assert_eq!(out.normal, [0.1, -0.2, 0.97]);
        assert_eq!(out.emission, [0.0, 0.03, 0.0]);
        assert_eq!(out.opacity, 0.8);
        // The tint is applied to the albedo the remap never sees, and the remap
        // to the roughness the tint never sees — they commute here only because
        // they touch disjoint lanes, which is exactly what this pins.
        let unremapped = finish(
            [0.5, 0.5, 0.5, 0.8],
            [0.6, 0.25, 0.1],
            [0.1, -0.2, 0.97],
            [0.0, 0.03, 0.0],
            [2.0, 1.0, 0.5],
            rough_p(DEFAULT_ROUGHNESS, 0.0),
            1.0,
            1.0,
        );
        assert_eq!(unremapped.base_color, out.base_color);
        assert_eq!(unremapped.roughness, 0.25);
    }

    #[test]
    fn the_wgsl_declares_every_entry_point_this_layer_owns() {
        [
            "fn axiom_mat_wear_albedo(",
            "fn axiom_mat_wear_orm(",
            "fn axiom_mat_normal_strength(",
            "fn axiom_mat_tint(",
            "fn axiom_mat_roughness_remap(",
            "fn axiom_mat_alpha_cut(",
            "fn axiom_mat_channels(",
            "fn axiom_mat_finish(",
        ]
        .iter()
        .for_each(|entry| assert!(TINT_WEAR_WGSL.contains(entry), "missing {entry}"));
        // The two groupings that must not be tidied, as text.
        assert!(TINT_WEAR_WGSL
            .contains("clamp(roughness * rough_p.x + rough_p.y, max(rough_p.w, 0.015), 1.0)"));
        assert!(TINT_WEAR_WGSL.contains("mix(alb, wear_color, wear_mask * wear_material.w)"));
        // z is not in the normalStrength multiply.
        assert!(TINT_WEAR_WGSL.contains("vec3<f32>(n.x * amp, n.y * amp, n.z)"));
    }

    #[test]
    fn mix_and_clamp_are_the_glsl_definitions() {
        // mix is x*(1-a) + y*a, and the endpoints are exact.
        assert_eq!(mix(3.0, 7.0, 0.0), 3.0);
        assert_eq!(mix(3.0, 7.0, 1.0), 7.0);
        assert_eq!(mix(3.0, 7.0, 0.25), 4.0);
        // Extrapolation is defined, not clamped.
        assert_eq!(mix(0.0, 4.0, 2.0), 8.0);
        // clamp is min(max(x, low), high).
        assert_eq!(clamp(-1.0, 0.2, 0.8), 0.2);
        assert_eq!(clamp(5.0, 0.2, 0.8), 0.8);
        assert_eq!(clamp(0.5, 0.2, 0.8), 0.5);
    }
}

// ---------------------------------------------------------------------------
// CPU <-> GPU parity, on a real adapter.
// ---------------------------------------------------------------------------

/// The proof that [`TINT_WEAR_WGSL`] and the CPU reference above are the same
/// maths, driven through a real device.
///
/// Compiled only under `--features offscreen`, which is what makes an adapter
/// available, and it **asserts** one was acquired rather than skipping — a
/// parity test that passes when nothing ran proves nothing.
///
/// This carries its own small harness rather than reusing
/// `surface_program::parity::ParityGpu`, which is `pub(super)` to that module.
/// Twelve layers each growing one is a real duplication and it is reported to
/// the orchestrator; it is not something a layer may fix by editing a shared
/// file mid-fan-out.
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod parity {
    use super::*;
    use crate::surface_program::wgsl_template::SURFACE_PRELUDE_WGSL;

    /// How many input sets one run compares; also the target's width.
    const SAMPLES: usize = 16;

    /// `vec4`s of input per sample, matching what the harness unpacks.
    const SLOTS: usize = 8;

    /// `copy_texture_to_buffer` requires each row aligned to this many bytes.
    const ROW_ALIGN: u32 = 256;

    /// The absolute tolerance every lane of this layer is held to.
    ///
    /// **Derived from the measurement below, not fitted to a miss.** The only
    /// operations here are a multiply, a `mix`, a fused multiply-add and a
    /// `clamp`; the hardware is free to contract `roughness * scale + offset`
    /// into a single-rounding `fma` and to factor `mix` either way, and that is
    /// the whole of the available disagreement. [`MEASURED_WORST_DELTA`] records
    /// what a real adapter actually showed, and
    /// [`the_tolerance_is_not_looser_than_the_hardware_needs`] fails if this
    /// constant drifts more than 10x above it.
    ///
    /// `2.4e-7` is **four f32 ulp at 1.0**, i.e. 4.03x the measurement — headroom
    /// for a different adapter's factoring, and comfortably inside the 10x
    /// ceiling. It is four orders of magnitude tighter than the `1e-4` the field
    /// algebra's exact tier carries, which is right: that tier compares whole
    /// generated expression trees, this one compares four arithmetic operations.
    const TOLERANCE: f32 = 2.4e-7;

    /// The worst absolute lane delta measured on a real adapter, committed as
    /// data. A constant is set from the worst adapter measured, never the best.
    ///
    /// Measured `5.9604645e-8` on a **Vulkan** adapter across all six entry
    /// points and all [`SAMPLES`] input sets — exactly `2^-24`, one f32 ulp at
    /// `1.0`. That is the single-rounding difference between the CPU's
    /// `roughness * scale + offset` and the GPU's contracted `fma` of the same,
    /// and it is the whole of the disagreement: every other lane here is a plain
    /// multiply or a `mix` and came back bit-identical.
    const MEASURED_WORST_DELTA: f32 = 5.960_464_5e-8;

    /// The harness: one fragment stage per group of lanes under test, each
    /// reading its sample's inputs from the uniform its pixel column names.
    const HARNESS_WGSL: &str = r#"
struct MatInputs { items: array<vec4<f32>, 128> };
@group(0) @binding(0) var<uniform> mat_in: MatInputs;

@vertex
fn parity_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

struct MatSample {
    alb: vec4<f32>,
    orm: vec3<f32>,
    wear_mask: f32,
    shade_normal: vec3<f32>,
    amp: f32,
    tint_color: vec3<f32>,
    material_opacity: f32,
    rough_p: vec4<f32>,
    wear_color: vec3<f32>,
    alpha_mask: f32,
    wear_material: vec4<f32>,
    emission: vec3<f32>,
    alpha_test: f32,
};

fn mat_sample(position: vec4<f32>) -> MatSample {
    let base = u32(position.x) * 8u;
    let a = mat_in.items[base + 0u];
    let b = mat_in.items[base + 1u];
    let c = mat_in.items[base + 2u];
    let d = mat_in.items[base + 3u];
    let e = mat_in.items[base + 4u];
    let f = mat_in.items[base + 5u];
    let g = mat_in.items[base + 6u];
    let h = mat_in.items[base + 7u];
    return MatSample(
        a, b.xyz, b.w, c.xyz, c.w, d.xyz, d.w, e, f.xyz, f.w, g, h.xyz, h.w,
    );
}

@fragment
fn parity_mat_wear_albedo_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let s = mat_sample(position);
    return vec4<f32>(
        axiom_mat_wear_albedo(s.alb.rgb, s.wear_color, s.wear_mask, s.wear_material),
        0.0,
    );
}

@fragment
fn parity_mat_wear_orm_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let s = mat_sample(position);
    return vec4<f32>(axiom_mat_wear_orm(s.orm, s.wear_mask, s.wear_material), 0.0);
}

@fragment
fn parity_mat_normal_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let s = mat_sample(position);
    let cut = axiom_mat_alpha_cut(s.alb.a, s.alpha_test);
    return vec4<f32>(
        axiom_mat_normal_strength(s.shade_normal, s.amp),
        select(0.0, 1.0, cut),
    );
}

@fragment
fn parity_mat_finish_color_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let s = mat_sample(position);
    return axiom_mat_finish(
        s.alb, s.orm, s.shade_normal, s.emission, s.tint_color, s.rough_p,
        s.material_opacity, s.alpha_mask,
    ).base_color;
}

@fragment
fn parity_mat_finish_scalars_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let s = mat_sample(position);
    let out = axiom_mat_finish(
        s.alb, s.orm, s.shade_normal, s.emission, s.tint_color, s.rough_p,
        s.material_opacity, s.alpha_mask,
    );
    return vec4<f32>(out.roughness, out.metallic, out.opacity, out.emission.x);
}

@fragment
fn parity_mat_finish_normal_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let s = mat_sample(position);
    let out = axiom_mat_finish(
        s.alb, s.orm, s.shade_normal, s.emission, s.tint_color, s.rough_p,
        s.material_opacity, s.alpha_mask,
    );
    return vec4<f32>(out.normal, out.emission.y);
}
"#;

    /// One sample's inputs, on both sides.
    #[derive(Clone, Copy)]
    struct Sample {
        alb: [f32; 4],
        orm: [f32; 3],
        wear_mask: f32,
        shade_normal: [f32; 3],
        amp: f32,
        tint_color: [f32; 3],
        material_opacity: f32,
        rough_p: [f32; 4],
        wear_color: [f32; 3],
        alpha_mask: f32,
        wear_material: [f32; 4],
        emission: [f32; 3],
        alpha_test: f32,
    }

    /// The [`SAMPLES`] input sets, chosen to hit what is easy to get wrong: a
    /// roughness that clamps at both ends, a wear mask at exactly 0 and exactly
    /// 1, an alpha mask at exactly 0 and exactly 1, an albedo alpha crossing the
    /// cutout threshold, negative normals, and a tint that is not white.
    fn samples() -> Vec<Sample> {
        (0..SAMPLES)
            .map(|index| {
                let t = index as f32;
                Sample {
                    alb: [0.05 + t * 0.06, 0.9 - t * 0.05, 0.3 + t * 0.02, t * 0.0625],
                    orm: [0.2 + t * 0.05, -0.1 + t * 0.09, 0.15 + t * 0.03],
                    wear_mask: t / (SAMPLES as f32 - 1.0),
                    shade_normal: [t * 0.11 - 0.8, 0.6 - t * 0.07, 0.5 + t * 0.03],
                    amp: 0.25 + t * 0.35,
                    tint_color: [1.0 - t * 0.05, 0.4 + t * 0.03, 0.7 - t * 0.02],
                    material_opacity: 1.0 - t * 0.05,
                    rough_p: rough_p([0.5 + t * 0.2, -0.3 + t * 0.05, t * 0.07], t * 0.03),
                    wear_color: srgb_hex_to_linear(DEFAULT_WEAR_COLOR_HEX),
                    alpha_mask: (index % 2) as f32,
                    wear_material: [0.42, t * 0.0666, 0.0, 0.5],
                    emission: [t * 0.01, 0.4 - t * 0.02, t * 0.003],
                    alpha_test: FOLIAGE_ALPHA_TEST,
                }
            })
            .collect()
    }

    /// The uniform's bytes: [`SLOTS`] `vec4` per sample, in the order
    /// `mat_sample` unpacks them.
    fn input_bytes(samples: &[Sample]) -> Vec<u8> {
        let mut bytes: Vec<u8> = samples
            .iter()
            .flat_map(|s| {
                [
                    s.alb[0],
                    s.alb[1],
                    s.alb[2],
                    s.alb[3],
                    s.orm[0],
                    s.orm[1],
                    s.orm[2],
                    s.wear_mask,
                    s.shade_normal[0],
                    s.shade_normal[1],
                    s.shade_normal[2],
                    s.amp,
                    s.tint_color[0],
                    s.tint_color[1],
                    s.tint_color[2],
                    s.material_opacity,
                    s.rough_p[0],
                    s.rough_p[1],
                    s.rough_p[2],
                    s.rough_p[3],
                    s.wear_color[0],
                    s.wear_color[1],
                    s.wear_color[2],
                    s.alpha_mask,
                    s.wear_material[0],
                    s.wear_material[1],
                    s.wear_material[2],
                    s.wear_material[3],
                    s.emission[0],
                    s.emission[1],
                    s.emission[2],
                    s.alpha_test,
                ]
            })
            .flat_map(f32::to_le_bytes)
            .collect();
        bytes.resize(128 * 16, 0);
        bytes
    }

    /// A real GPU, and the one render this harness needs.
    struct Gpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
        backend: wgpu::Backend,
    }

    impl Gpu {
        /// Acquire a native adapter, or fail the test loudly.
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

        /// Render `entry_point` over a `SAMPLES x 1` `Rgba32Float` target — the
        /// float format matters, an `Rgba8Unorm` target quantises to 1/255,
        /// which is four orders of magnitude coarser than the tolerance.
        fn render(&self, module: &wgpu::ShaderModule, entry: &str, inputs: &[u8]) -> Vec<[f32; 4]> {
            let layout = self
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("axiom-mat-parity-bgl"),
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
                    label: Some("axiom-mat-parity-uniform"),
                    contents: inputs,
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-mat-parity-bg"),
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("axiom-mat-parity-pl"),
                        bind_group_layouts: &[&layout],
                        push_constant_ranges: &[],
                    });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("axiom-mat-parity-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("parity_vs"),
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
                label: Some("axiom-mat-parity-target"),
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
                label: Some("axiom-mat-parity-readback"),
                size: u64::from(row_bytes),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("axiom-mat-parity-pass"),
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

    /// The CPU side of every entry point, in the order the entry points appear.
    fn expected(s: &Sample) -> Vec<(&'static str, [f32; 4])> {
        let wa = wear_albedo(
            [s.alb[0], s.alb[1], s.alb[2]],
            s.wear_color,
            s.wear_mask,
            s.wear_material,
        );
        let wo = wear_orm(s.orm, s.wear_mask, s.wear_material);
        let ns = normal_strength(s.shade_normal, s.amp);
        let cut = f32::from(u8::from(alpha_cut(s.alb[3], s.alpha_test)));
        let out = finish(
            s.alb,
            s.orm,
            s.shade_normal,
            s.emission,
            s.tint_color,
            s.rough_p,
            s.material_opacity,
            s.alpha_mask,
        );
        vec![
            ("parity_mat_wear_albedo_fs", [wa[0], wa[1], wa[2], 0.0]),
            ("parity_mat_wear_orm_fs", [wo[0], wo[1], wo[2], 0.0]),
            ("parity_mat_normal_fs", [ns[0], ns[1], ns[2], cut]),
            ("parity_mat_finish_color_fs", out.base_color),
            (
                "parity_mat_finish_scalars_fs",
                [out.roughness, out.metallic, out.opacity, out.emission[0]],
            ),
            (
                "parity_mat_finish_normal_fs",
                [
                    out.normal[0],
                    out.normal[1],
                    out.normal[2],
                    out.emission[1],
                ],
            ),
        ]
    }

    /// Compile the layer against the shared prelude and the harness, run every
    /// entry point, and return the worst absolute lane delta seen — asserting
    /// each lane against `TOLERANCE` on the way through.
    fn worst_delta(gpu: &Gpu) -> f32 {
        assert_ne!(
            gpu.backend,
            wgpu::Backend::Noop,
            "the parity proof is worthless unless a real backend ran it"
        );
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("axiom-mat-tint-wear-parity-shader"),
                source: wgpu::ShaderSource::Wgsl(
                    [SURFACE_PRELUDE_WGSL, TINT_WEAR_WGSL, HARNESS_WGSL]
                        .concat()
                        .into(),
                ),
            });
        let samples = samples();
        let bytes = input_bytes(&samples);
        let cpu: Vec<Vec<(&'static str, [f32; 4])>> = samples.iter().map(expected).collect();
        (0..cpu[0].len()).fold(0.0_f32, |worst, case| {
            let entry = cpu[0][case].0;
            let rendered = gpu.render(&module, entry, &bytes);
            samples
                .iter()
                .enumerate()
                .zip(rendered.iter())
                .fold(worst, |acc, ((index, _), actual)| {
                    let want = cpu[index][case].1;
                    (0..4).fold(acc, |inner, lane| {
                        let delta = (want[lane] - actual[lane]).abs();
                        assert!(
                            delta <= TOLERANCE,
                            "{entry} lane {lane} disagrees at sample {index}: \
                             cpu {} vs gpu {} (delta {delta:e}, tolerance {TOLERANCE:e})",
                            want[lane],
                            actual[lane]
                        );
                        inner.max(delta)
                    })
                })
        })
    }

    /// **The parity proof.** Every entry point this layer defines, on a real
    /// adapter, against the CPU reference that is its semantic definition.
    #[test]
    fn the_tint_wear_wgsl_agrees_with_the_cpu_reference_on_a_real_adapter() {
        let gpu = Gpu::acquire();
        let delta = worst_delta(&gpu);
        assert!(delta <= TOLERANCE, "worst lane delta {delta:e}");
    }

    /// The other half: a tolerance more than 10x looser than the hardware needs
    /// is itself a failure, because it hides the next regression.
    #[test]
    fn the_tolerance_is_not_looser_than_the_hardware_needs() {
        let gpu = Gpu::acquire();
        let delta = worst_delta(&gpu);
        assert!(
            delta <= MEASURED_WORST_DELTA,
            "the live worst delta {delta:e} has drifted clear of the committed \
             record {MEASURED_WORST_DELTA:e}; re-measure and update the constant"
        );
        assert!(
            TOLERANCE >= MEASURED_WORST_DELTA,
            "the tolerance must cover the measurement"
        );
        assert!(
            TOLERANCE <= MEASURED_WORST_DELTA * 10.0,
            "{TOLERANCE:e} is more than 10x the measured {MEASURED_WORST_DELTA:e}"
        );
    }

    /// The inputs must actually reach the interesting cases, or the parity above
    /// is vacuous: a wear mask at both ends, an alpha mask at both ends, the
    /// roughness clamped at both ends, and the cutout on both sides.
    #[test]
    fn the_samples_exercise_both_ends_of_every_gate() {
        let all = samples();
        assert!(all.iter().any(|s| s.wear_mask == 0.0));
        assert!(all.iter().any(|s| s.wear_mask == 1.0));
        assert!(all.iter().any(|s| s.alpha_mask == 0.0));
        assert!(all.iter().any(|s| s.alpha_mask == 1.0));
        assert!(all.iter().any(|s| alpha_cut(s.alb[3], s.alpha_test)));
        assert!(all.iter().any(|s| !alpha_cut(s.alb[3], s.alpha_test)));
        let remapped: Vec<f32> = all
            .iter()
            .map(|s| roughness_remap(s.orm[1], s.rough_p))
            .collect();
        assert!(remapped.iter().any(|r| *r >= 1.0), "the high clamp is hit");
        assert!(
            remapped
                .iter()
                .zip(all.iter())
                .any(|(r, s)| *r == s.rough_p[3].max(ROUGHNESS_HARD_FLOOR)),
            "the low clamp is hit"
        );
        assert_eq!(input_bytes(&all).len(), 128 * 16);
        assert_eq!(SLOTS * SAMPLES, 128);
    }
}
