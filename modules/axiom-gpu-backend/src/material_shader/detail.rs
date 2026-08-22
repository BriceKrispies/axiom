//! **The micro detail layer** — a sub-millimetre normal, albedo speckle and
//! cavity darkening from a shared detail set, faded out with distance.
//!
//! Ported from Claude-of-Duty `src/materials/shader.js`: the
//! `// ---- micro detail normal, faded by distance ----` section of
//! `MAIN_FRAGMENT` (source lines 372-393), its triplanar twin (source lines
//! 310-321), and the `detail` / `detailWorld` entries of `DEFAULT_PARAMS`
//! together with the `detailTiles` derivation in `extendMaterial`.
//!
//! ## What the layer is for
//!
//! At half a metre a wall is not flat colour: plaster has a tooth, concrete has
//! aggregate, timber has grain. One shared, tiny, high-frequency set — a normal
//! map and a height/albedo map — is tiled far denser than the base material and
//! layered on top. It has to *fade out* with distance, because a
//! sub-millimetre pattern seen at thirty metres is below a pixel and turns into
//! shimmer, so the layer buys near-field detail and pays nothing far away.
//!
//! ## `detailWorld`: the derivation is part of the algorithm
//!
//! `detail[0]` is authored as **tiles per base tile**, which silently ties the
//! micro layer's world scale to the macro layer's. The source documents the
//! measured consequence at length: a prop-scale variant (`wood_prop`, `scale`
//! 0.55 m) with `detail[0] = 10` mapped the 0.25 m detail bake into 55 mm, so a
//! 1.6 mm grain became 0.35 mm — under one pixel at 0.5 m. The whole micro
//! layer filtered away to nothing and every prop read as flat colour up close.
//! The proof it was dead: *"cranking `detail[2]` from 0.42 to 2.5 on the market
//! stall changed the frame by nothing at all."*
//!
//! So `detail[0]` is **derived from `scale`** unless `detailWorld` is 0, which
//! pins the micro tooth at a fixed physical size however the surface is mapped.
//! [`detail_tiles`] is that derivation, and it is pinned by its own tests —
//! including the two carve-outs the source keeps: mesh-UV mode (where `scale`
//! is a repeat count, not metres) and surfaces mapped finer than 0.3 m (a
//! viewmodel part wants detail an order of magnitude finer than a wall's;
//! forcing 0.26 m on it would put a 2 mm aggregate tooth on a bolt carrier).
//!
//! ## The three things that land together
//!
//! [`DETAIL_WGSL`] is the shader text, the free functions below are the CPU
//! reference that *defines* what it must mean, and the `parity` submodule drives
//! both on a real adapter. The WGSL takes its textures and sampler as function
//! parameters — WGSL permits that — so the layer is self-contained and the
//! orchestrator wires the shared detail bindings without this file naming a
//! binding index.
//!
//! ## Transcription notes
//!
//! - **The normal blend is UDN, not addition, not whiteout, not a lerp.** The
//!   source writes `normalize( vec3( nT.xy + dn.xy * s * f, nT.z ) )`: the two
//!   tangent-space `xy` are summed and the **base** `z` is kept unchanged, then
//!   the whole thing is renormalised. Whiteout would multiply the two `z`
//!   (`nT.z * dn.z`) and the partial-derivative blend would cross-multiply
//!   (`nT.xy * dn.z + dn.xy * nT.z`); both differ visibly from this at grazing
//!   angles, which is exactly where a micro layer is seen.
//! - **`detail[1]` and `detail[2]` are separate strengths** and are never
//!   collapsed: `.y` scales the normal only, `.z` scales the albedo speckle
//!   *and* the cavity darkening, which are the same physical signal.
//! - **The groupings are the source's.** `dn.xy * owDetailP.y * detFade` is
//!   `(dn.xy * y) * fade`, `micro * 0.95 + (r - 0.5) * 1.25` is not factored,
//!   and `smoothstep`'s `(x - e0) / (e1 - e0)` stays a division. Float
//!   arithmetic is not associative; the source's grouping is the specification.
//! - **Storage width.** The CPU reference computes in `f32` throughout,
//!   matching the GPU, so a parity delta is the hardware's rounding and never a
//!   width mismatch this file introduced.

/// `DEFAULT_PARAMS.detail` — `[ tiles per base tile, normal strength, albedo
/// strength, fade metres ]`.
pub(crate) const DEFAULT_DETAIL: [f32; 4] = [11.0, 0.55, 0.35, 16.0];

/// `DEFAULT_PARAMS.detailWorld` — the metres one shared detail tile should span
/// in the world. `0.26` matches the bake's authored `worldSize` of 0.25 m.
pub(crate) const DEFAULT_DETAIL_WORLD: f32 = 0.26;

/// The coarsest mapping the world-anchored derivation applies to. Finer than
/// this — a viewmodel part at 0.02-0.12 m — keeps its authored `detail[0]`.
const DERIVATION_MIN_SCALE: f32 = 0.3;

/// The floor the derived tile count is held to, so a very coarsely mapped
/// surface still gets at least a little over one detail tile per base tile.
const DERIVED_TILES_FLOOR: f32 = 1.2;

/// The micro layer's WGSL.
///
/// Every entry point is a free function taking explicit arguments, textures and
/// sampler included; nothing here reads a global or assumes a binding index.
///
/// The layer is exposed both as its parts and as one composition, because the
/// source **interleaves**: under `OW_DETILE` the de-tiling height blend runs
/// *between* the normal blend and the albedo modulation. `axiom_detail` is the
/// un-de-tiled composition; an orchestrator that also runs de-tiling calls the
/// parts in the source's order and splices the blend in between.
pub(crate) const DETAIL_WGSL: &str = r#"
// The micro detail layer, from Claude-of-Duty `src/materials/shader.js`
// lines 372-393 (tangent space) and 310-321 (triplanar).
//
// `detail_p` is the source's `owDetailP`:
//   .x tiles      .y normal strength      .z albedo strength      .w fade metres

struct AxiomDetailOut {
    normal_tangent: vec3<f32>,
    albedo: vec3<f32>,
    roughness: f32,
    micro: f32,
    fade: f32,
    height: f32,
};

// float detFade = 1.0 - smoothstep( owDetailP.w * 0.45, owDetailP.w, owDist );
//
// Exactly 1.0 at and below 0.45 * fade metres, and exactly 0.0 at and beyond
// fade metres: at `dist == detail_p.w` the smoothstep argument is exactly 1.0,
// so `1.0 - 1.0` is an exact zero and every term this multiplies vanishes.
fn axiom_detail_fade(fade_metres: f32, dist: f32) -> f32 {
    return 1.0 - smoothstep(fade_metres * 0.45, fade_metres, dist);
}

// `uv * owDetailP.x` — and, applied to a derivative, `ddx * owDetailP.x`. The
// one multiply the whole `detailWorld` derivation exists to get right.
fn axiom_detail_uv(uv: vec2<f32>, tiles: f32) -> vec2<f32> {
    return uv * tiles;
}

// nT = normalize( vec3( nT.xy + dn.xy * owDetailP.y * detFade, nT.z ) );
//
// UDN: sum the tangent xy, keep the BASE z, renormalise. Not whiteout
// (`nT.z * dn.z`), not the partial-derivative blend, not a lerp.
fn axiom_detail_blend_normal(
    n_tangent: vec3<f32>,
    dn: vec3<f32>,
    normal_strength: f32,
    fade: f32,
) -> vec3<f32> {
    return normalize(vec3<f32>(n_tangent.xy + dn.xy * normal_strength * fade, n_tangent.z));
}

// nP = normalize( nP + ( dW - fd.N * dot( dW, fd.N ) ) * owDetailP.y * detFade );
//
// The triplanar arm's blend: the detail normal is already in world space (the
// dominant-plane frame rotated it), so its component along the face normal is
// projected out and the remainder added. Same two strengths, same fade.
fn axiom_detail_blend_normal_projected(
    n_world: vec3<f32>,
    d_world: vec3<f32>,
    face_normal: vec3<f32>,
    normal_strength: f32,
    fade: f32,
) -> vec3<f32> {
    return normalize(
        n_world + (d_world - face_normal * dot(d_world, face_normal)) * normal_strength * fade
    );
}

// owMicro = ( dTex.a - 0.5 ) * 2.0;  -- sub-millimetre height, -1..1.
fn axiom_detail_micro(detail_texel: vec4<f32>) -> f32 {
    return (detail_texel.a - 0.5) * 2.0;
}

// alb.rgb *= 1.0 + ( owMicro * 0.95 + ( dTex.r - 0.5 ) * 1.25 ) * owDetailP.z * detFade;
fn axiom_detail_albedo(
    albedo: vec3<f32>,
    detail_texel: vec4<f32>,
    micro: f32,
    albedo_strength: f32,
    fade: f32,
) -> vec3<f32> {
    return albedo * (1.0 + (micro * 0.95 + (detail_texel.r - 0.5) * 1.25) * albedo_strength * fade);
}

// orm.r *= 1.0 - max( -owMicro, 0.0 ) * 0.30 * owDetailP.z * detFade;
//
// Aggregate reads dark in its troughs even in full sun, because a trough is a
// tiny occluded pocket. Modulating only the albedo gives a washed pattern;
// darkening the cavity as well is what makes it read as depth. Troughs only —
// a peak (positive micro) leaves roughness alone.
fn axiom_detail_roughness(
    roughness: f32,
    micro: f32,
    albedo_strength: f32,
    fade: f32,
) -> f32 {
    return roughness * (1.0 - max(-micro, 0.0) * 0.30 * albedo_strength * fade);
}

// owHeightS = clamp( alb.a + owMicro * 0.16 * detFade, 0.0, 1.0 );
fn axiom_detail_height(albedo_alpha: f32, micro: f32, fade: f32) -> f32 {
    return clamp(albedo_alpha + micro * 0.16 * fade, 0.0, 1.0);
}

// The whole layer, un-de-tiled, in the source's order.
fn axiom_detail(
    detail_normal_tex: texture_2d<f32>,
    detail_tex: texture_2d<f32>,
    detail_sampler: sampler,
    uv: vec2<f32>,
    ddx: vec2<f32>,
    ddy: vec2<f32>,
    dist: f32,
    detail_p: vec4<f32>,
    n_tangent: vec3<f32>,
    albedo: vec4<f32>,
    roughness: f32,
) -> AxiomDetailOut {
    let det_fade = axiom_detail_fade(detail_p.w, dist);
    let det_uv = axiom_detail_uv(uv, detail_p.x);
    let det_ddx = axiom_detail_uv(ddx, detail_p.x);
    let det_ddy = axiom_detail_uv(ddy, detail_p.x);
    let dn = textureSampleGrad(detail_normal_tex, detail_sampler, det_uv, det_ddx, det_ddy).xyz
        * 2.0 - 1.0;
    let blended = axiom_detail_blend_normal(n_tangent, dn, detail_p.y, det_fade);
    let d_tex = textureSampleGrad(detail_tex, detail_sampler, det_uv, det_ddx, det_ddy);
    let micro = axiom_detail_micro(d_tex);
    var out: AxiomDetailOut;
    out.normal_tangent = blended;
    out.albedo = axiom_detail_albedo(albedo.rgb, d_tex, micro, detail_p.z, det_fade);
    out.roughness = axiom_detail_roughness(roughness, micro, detail_p.z, det_fade);
    out.micro = micro;
    out.fade = det_fade;
    out.height = axiom_detail_height(albedo.a, micro, det_fade);
    return out;
}
"#;

/// GLSL/WGSL `smoothstep`, written out: `t = clamp((x - e0) / (e1 - e0), 0, 1)`
/// then `t * t * (3 - 2t)`. The division stays a division, and the clamp is
/// `min(max(t, 0), 1)` — the expansion WGSL specifies, not Rust's `f32::clamp`,
/// whose NaN handling differs.
fn glsl_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).max(0.0).min(1.0);
    t * t * (3.0 - 2.0 * t)
}

/// GLSL/WGSL `normalize`: `v / length(v)`, a per-component **division** by the
/// length — never a multiply by a precomputed reciprocal, which rounds twice.
fn glsl_normalize(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / length, v[1] / length, v[2] / length]
}

/// Everything the micro layer produces at one shading point.
///
/// Deliberately underived: a `Debug` or `PartialEq` impl that no passing test
/// executes is an uncovered function, and the tests compare fields, which names
/// the one that moved when a comparison fails.
pub(crate) struct DetailOut {
    /// `nT` after the UDN blend — tangent space, renormalised.
    pub(crate) normal_tangent: [f32; 3],
    /// `alb.rgb` after the speckle.
    pub(crate) albedo: [f32; 3],
    /// `orm.r` after the cavity darkening.
    pub(crate) roughness: f32,
    /// `owMicro`, -1..1 — read by the weathering and mask layers downstream.
    pub(crate) micro: f32,
    /// `owDetFade` — read downstream (the source's line 429 uses it again).
    pub(crate) fade: f32,
    /// `owHeightS` — the surface height the cavity and wear masks read.
    pub(crate) height: f32,
}

/// `float detFade = 1.0 - smoothstep( owDetailP.w * 0.45, owDetailP.w, owDist );`
pub(crate) fn detail_fade(fade_metres: f32, dist: f32) -> f32 {
    1.0 - glsl_smoothstep(fade_metres * 0.45, fade_metres, dist)
}

/// `uv * owDetailP.x` — also how the derivatives are scaled before the fetch.
pub(crate) fn detail_uv(uv: [f32; 2], tiles: f32) -> [f32; 2] {
    [uv[0] * tiles, uv[1] * tiles]
}

/// `nT = normalize( vec3( nT.xy + dn.xy * owDetailP.y * detFade, nT.z ) );`
///
/// UDN: the two tangent `xy` sum, the **base** `z` survives untouched.
pub(crate) fn blend_normal(
    n_tangent: [f32; 3],
    dn: [f32; 3],
    normal_strength: f32,
    fade: f32,
) -> [f32; 3] {
    glsl_normalize([
        n_tangent[0] + dn[0] * normal_strength * fade,
        n_tangent[1] + dn[1] * normal_strength * fade,
        n_tangent[2],
    ])
}

/// `nP = normalize( nP + ( dW - fd.N * dot( dW, fd.N ) ) * owDetailP.y * detFade );`
///
/// The triplanar arm: the detail normal arrives in world space, its component
/// along the face normal is projected out, and the remainder is added.
pub(crate) fn blend_normal_projected(
    n_world: [f32; 3],
    d_world: [f32; 3],
    face_normal: [f32; 3],
    normal_strength: f32,
    fade: f32,
) -> [f32; 3] {
    let along =
        d_world[0] * face_normal[0] + d_world[1] * face_normal[1] + d_world[2] * face_normal[2];
    glsl_normalize([
        n_world[0] + (d_world[0] - face_normal[0] * along) * normal_strength * fade,
        n_world[1] + (d_world[1] - face_normal[1] * along) * normal_strength * fade,
        n_world[2] + (d_world[2] - face_normal[2] * along) * normal_strength * fade,
    ])
}

/// `owMicro = ( dTex.a - 0.5 ) * 2.0;`
pub(crate) fn micro_height(detail_texel: [f32; 4]) -> f32 {
    (detail_texel[3] - 0.5) * 2.0
}

/// `alb.rgb *= 1.0 + ( owMicro * 0.95 + ( dTex.r - 0.5 ) * 1.25 ) * owDetailP.z * detFade;`
pub(crate) fn modulate_albedo(
    albedo: [f32; 3],
    detail_texel: [f32; 4],
    micro: f32,
    albedo_strength: f32,
    fade: f32,
) -> [f32; 3] {
    let gain = 1.0 + (micro * 0.95 + (detail_texel[0] - 0.5) * 1.25) * albedo_strength * fade;
    [albedo[0] * gain, albedo[1] * gain, albedo[2] * gain]
}

/// `orm.r *= 1.0 - max( -owMicro, 0.0 ) * 0.30 * owDetailP.z * detFade;`
pub(crate) fn modulate_roughness(
    roughness: f32,
    micro: f32,
    albedo_strength: f32,
    fade: f32,
) -> f32 {
    roughness * (1.0 - (-micro).max(0.0) * 0.30 * albedo_strength * fade)
}

/// `owHeightS = clamp( alb.a + owMicro * 0.16 * detFade, 0.0, 1.0 );`
pub(crate) fn surface_height(albedo_alpha: f32, micro: f32, fade: f32) -> f32 {
    (albedo_alpha + micro * 0.16 * fade).max(0.0).min(1.0)
}

/// The whole un-de-tiled layer, in the source's order — the CPU definition of
/// what `axiom_detail` must mean. The two texels are the values
/// `textureGrad(owDetailNrm, …)` and `textureGrad(owDetailTex, …)` returned;
/// the fetch itself is the caller's, because a fetch has no CPU meaning.
pub(crate) fn detail(
    detail_p: [f32; 4],
    dist: f32,
    detail_normal_texel: [f32; 4],
    detail_texel: [f32; 4],
    n_tangent: [f32; 3],
    albedo: [f32; 4],
    roughness: f32,
) -> DetailOut {
    let fade = detail_fade(detail_p[3], dist);
    // `OW_TEX( owDetailNrm, … ).xyz * 2.0 - 1.0`
    let dn = [
        detail_normal_texel[0] * 2.0 - 1.0,
        detail_normal_texel[1] * 2.0 - 1.0,
        detail_normal_texel[2] * 2.0 - 1.0,
    ];
    let normal_tangent = blend_normal(n_tangent, dn, detail_p[1], fade);
    let micro = micro_height(detail_texel);
    DetailOut {
        normal_tangent,
        albedo: modulate_albedo(
            [albedo[0], albedo[1], albedo[2]],
            detail_texel,
            micro,
            detail_p[2],
            fade,
        ),
        roughness: modulate_roughness(roughness, micro, detail_p[2], fade),
        micro,
        fade,
        height: surface_height(albedo[3], micro, fade),
    }
}

/// **`owDetailP.x`, derived.** The `detailWorld` fix, transcribed from
/// `extendMaterial`:
///
/// ```js
/// const dw = p.detailWorld ?? DEFAULT_PARAMS.detailWorld;
/// const detailTiles =
///   p.uvMode === 'mesh' || !(dw > 0) || p.scale < 0.3
///     ? p.detail[0]
///     : Math.max(1.2, p.scale / dw);
/// ```
///
/// The authored `detail[0]` survives in exactly three cases and the derivation
/// takes over otherwise:
///
/// - **mesh UV** — there `scale` is a repeat count, not metres, so the ratio is
///   meaningless;
/// - **`detailWorld` not positive** — the documented opt-out (and, written as
///   `!(dw > 0)`, it also catches a NaN, which is why the negation is on the
///   *outside* of the comparison and is transcribed that way here);
/// - **`scale < 0.3`** — a viewmodel part mapped at 0.02-0.12 m wants detail an
///   order of magnitude finer than a wall's.
///
/// `scale / detail_world` is a division in the source and stays one. The floor
/// is `Math.max(1.2, x)`, which propagates a NaN where Rust's `f32::max` would
/// swallow it, so it is written as the select that matches JavaScript exactly.
pub(crate) fn detail_tiles(
    uv_mode_is_mesh: bool,
    scale: f32,
    detail_world: Option<f32>,
    authored_tiles: f32,
) -> f32 {
    let dw = detail_world.unwrap_or(DEFAULT_DETAIL_WORLD);
    let authored = uv_mode_is_mesh | !(dw > 0.0) | (scale < DERIVATION_MIN_SCALE);
    let derived = scale / dw;
    // `Math.max(1.2, derived)`: `1.2` when it is strictly greater, otherwise
    // `derived` — which returns NaN for a NaN, as JavaScript does.
    let floored = [derived, DERIVED_TILES_FLOOR][usize::from(DERIVED_TILES_FLOOR > derived)];
    [floored, authored_tiles][usize::from(authored)]
}

#[cfg(test)]
mod tests {
    use super::{
        blend_normal, blend_normal_projected, detail, detail_fade, detail_tiles, detail_uv,
        glsl_normalize, glsl_smoothstep, micro_height, modulate_albedo, modulate_roughness,
        surface_height, DEFAULT_DETAIL, DEFAULT_DETAIL_WORLD, DETAIL_WGSL,
    };

    /// A representative `owDetailP`: the source's defaults.
    const P: [f32; 4] = DEFAULT_DETAIL;

    #[test]
    fn the_defaults_are_the_sources_defaults() {
        assert_eq!(DEFAULT_DETAIL, [11.0, 0.55, 0.35, 16.0]);
        assert_eq!(DEFAULT_DETAIL_WORLD, 0.26);
    }

    #[test]
    fn smoothstep_is_the_glsl_definition_including_both_clamped_ends() {
        assert_eq!(glsl_smoothstep(2.0, 6.0, 1.0), 0.0);
        assert_eq!(glsl_smoothstep(2.0, 6.0, 2.0), 0.0);
        assert_eq!(glsl_smoothstep(2.0, 6.0, 6.0), 1.0);
        assert_eq!(glsl_smoothstep(2.0, 6.0, 9.0), 1.0);
        assert_eq!(glsl_smoothstep(2.0, 6.0, 4.0), 0.5);
    }

    #[test]
    fn normalize_divides_by_the_length() {
        let unit = glsl_normalize([3.0, 0.0, 4.0]);
        assert_eq!(unit, [3.0 / 5.0, 0.0, 4.0 / 5.0]);
    }

    /// **The far end.** At and beyond `detail[3]` metres the fade is an exact
    /// zero — not merely small — so every term it multiplies vanishes.
    #[test]
    fn the_fade_reaches_exactly_zero_at_the_far_end_and_stays_there() {
        assert_eq!(detail_fade(16.0, 16.0), 0.0);
        assert_eq!(detail_fade(16.0, 16.000_01), 0.0);
        assert_eq!(detail_fade(16.0, 400.0), 0.0);
    }

    /// **The near end.** At and below `0.45 * detail[3]` the fade is an exact
    /// one, so the layer is at full authored strength with no attenuation
    /// creeping in from the smoothstep.
    #[test]
    fn the_fade_is_exactly_one_at_and_below_the_near_edge() {
        assert_eq!(detail_fade(16.0, 16.0 * 0.45), 1.0);
        assert_eq!(detail_fade(16.0, 3.0), 1.0);
        assert_eq!(detail_fade(16.0, 0.0), 1.0);
        // And it really does fade in between, or the two ends prove nothing.
        let mid = detail_fade(16.0, 11.0);
        assert!(mid > 0.0, "mid-window fade was {mid}");
        assert!(mid < 1.0, "mid-window fade was {mid}");
    }

    /// **Zero contribution means bit-identical.** At the far end the layer's
    /// outputs must equal the undetailed values exactly, for *any* detail
    /// texels — that is what "reaches zero" has to buy. The one output that is
    /// not literally the input is the normal, and only because the source
    /// renormalises unconditionally on this line: with the fade at zero the
    /// blend collapses to `normalize(nT)`, which is what the undetailed path
    /// hands the lighting anyway.
    #[test]
    fn at_the_far_end_the_layer_is_bit_identical_to_the_undetailed_path() {
        let albedo = [0.42, 0.31, 0.27, 0.63];
        let n = [0.11, -0.23, 0.9663];
        [
            [0.97, 0.02, 0.44, 0.99],
            [0.03, 0.88, 0.51, 0.01],
            [0.5, 0.5, 0.5, 0.5],
        ]
        .iter()
        .for_each(|texel| {
            let out = detail(P, 16.0, *texel, *texel, n, albedo, 0.73);
            assert_eq!(out.fade, 0.0);
            // `owMicro` itself is NOT faded. The source keeps the raw height
            // and multiplies by `owDetFade` at each use site — including once
            // more downstream, at source line 429 — so the value survives the
            // far end even though every use of it here vanishes. Pinned so a
            // tidy-up cannot fold the fade into the value.
            assert_eq!(out.micro, micro_height(*texel));
            assert_eq!(out.albedo, [albedo[0], albedo[1], albedo[2]]);
            assert_eq!(out.roughness, 0.73);
            assert_eq!(out.height, albedo[3]);
            assert_eq!(out.normal_tangent, glsl_normalize(n));
        });
    }

    /// The near end is where the layer actually does something: with the same
    /// inputs at full fade, every output moves.
    #[test]
    fn at_the_near_end_every_output_moves() {
        let albedo = [0.42, 0.31, 0.27, 0.63];
        let n = [0.11, -0.23, 0.9663];
        let texel = [0.97, 0.02, 0.44, 0.16];
        let out = detail(P, 1.0, texel, texel, n, albedo, 0.73);
        assert_eq!(out.fade, 1.0);
        assert_ne!(out.albedo, [albedo[0], albedo[1], albedo[2]]);
        assert_ne!(out.roughness, 0.73);
        assert_ne!(out.height, albedo[3]);
        assert_ne!(out.normal_tangent, glsl_normalize(n));
    }

    /// `detail[1]` and `detail[2]` are separate knobs. Moving the normal
    /// strength must not touch the albedo, and moving the albedo strength must
    /// not touch the normal — the collapse this layer is most likely to suffer.
    #[test]
    fn the_normal_and_albedo_strengths_are_independent() {
        let albedo = [0.42, 0.31, 0.27, 0.63];
        let n = [0.11, -0.23, 0.9663];
        let texel = [0.97, 0.02, 0.44, 0.16];
        let base = detail(P, 1.0, texel, texel, n, albedo, 0.73);
        let normal_only = detail(
            [P[0], P[1] * 3.0, P[2], P[3]],
            1.0,
            texel,
            texel,
            n,
            albedo,
            0.73,
        );
        let albedo_only = detail(
            [P[0], P[1], P[2] * 3.0, P[3]],
            1.0,
            texel,
            texel,
            n,
            albedo,
            0.73,
        );
        assert_ne!(normal_only.normal_tangent, base.normal_tangent);
        assert_eq!(normal_only.albedo, base.albedo);
        assert_eq!(normal_only.roughness, base.roughness);
        assert_eq!(albedo_only.normal_tangent, base.normal_tangent);
        assert_ne!(albedo_only.albedo, base.albedo);
        assert_ne!(albedo_only.roughness, base.roughness);
    }

    /// **UDN, and provably not the alternatives.** The blend keeps the base
    /// `z` and sums the `xy`; whiteout (`nT.z * dn.z`), the partial-derivative
    /// blend and a plain lerp all give different answers on the same inputs.
    #[test]
    fn the_normal_blend_is_udn_and_not_whiteout_or_a_lerp() {
        let n = [0.2, -0.35, 0.915];
        let dn = [0.6, 0.44, 0.667];
        let got = blend_normal(n, dn, 0.55, 1.0);
        let expected = glsl_normalize([
            n[0] + dn[0] * 0.55 * 1.0,
            n[1] + dn[1] * 0.55 * 1.0,
            // The BASE z, untouched.
            n[2],
        ]);
        assert_eq!(got, expected);
        let whiteout = glsl_normalize([n[0] + dn[0], n[1] + dn[1], n[2] * dn[2]]);
        let partial_derivative = glsl_normalize([
            n[0] * dn[2] + dn[0] * n[2],
            n[1] * dn[2] + dn[1] * n[2],
            n[2] * dn[2],
        ]);
        let lerp = glsl_normalize([
            n[0] + (dn[0] - n[0]) * 0.55,
            n[1] + (dn[1] - n[1]) * 0.55,
            n[2] + (dn[2] - n[2]) * 0.55,
        ]);
        assert_ne!(got, whiteout);
        assert_ne!(got, partial_derivative);
        assert_ne!(got, lerp);
    }

    /// The triplanar arm projects the detail normal's face-normal component out
    /// before adding it, so a detail normal parallel to the face changes
    /// nothing at all.
    #[test]
    fn the_projected_blend_removes_the_face_normal_component() {
        let face = [0.0, 1.0, 0.0];
        let n = [0.1, 0.99, -0.05];
        let parallel = blend_normal_projected(n, [0.0, 0.8, 0.0], face, 0.55, 1.0);
        assert_eq!(parallel, glsl_normalize(n));
        let tangential = blend_normal_projected(n, [0.7, 0.3, -0.2], face, 0.55, 1.0);
        assert_ne!(tangential, glsl_normalize(n));
        // The y term keeps only the part that was already there.
        let along = 0.7 * face[0] + 0.3 * face[1] + -0.2 * face[2];
        assert_eq!(
            tangential,
            glsl_normalize([
                n[0] + (0.7 - face[0] * along) * 0.55 * 1.0,
                n[1] + (0.3 - face[1] * along) * 0.55 * 1.0,
                n[2] + (-0.2 - face[2] * along) * 0.55 * 1.0,
            ])
        );
    }

    #[test]
    fn micro_maps_the_alpha_channel_onto_minus_one_to_one() {
        assert_eq!(micro_height([0.0, 0.0, 0.0, 0.0]), -1.0);
        assert_eq!(micro_height([0.0, 0.0, 0.0, 0.5]), 0.0);
        assert_eq!(micro_height([0.0, 0.0, 0.0, 1.0]), 1.0);
    }

    #[test]
    fn the_albedo_speckle_uses_both_the_height_and_the_red_channel() {
        let texel = [0.8, 0.0, 0.0, 0.7];
        let micro = micro_height(texel);
        let got = modulate_albedo([0.5, 0.25, 0.125], texel, micro, 0.35, 1.0);
        let gain = 1.0 + (micro * 0.95 + (0.8 - 0.5) * 1.25) * 0.35 * 1.0;
        assert_eq!(got, [0.5 * gain, 0.25 * gain, 0.125 * gain]);
        assert!(gain > 1.0, "a peak with a bright speckle brightens");
    }

    /// Only troughs darken the cavity: `max(-micro, 0)` is zero for a peak, so
    /// roughness is returned untouched — an exact identity, not an approximate
    /// one.
    #[test]
    fn only_troughs_darken_the_cavity() {
        assert_eq!(modulate_roughness(0.73, 0.9, 0.35, 1.0), 0.73);
        assert_eq!(modulate_roughness(0.73, 0.0, 0.35, 1.0), 0.73);
        let trough = modulate_roughness(0.73, -0.9, 0.35, 1.0);
        assert_eq!(trough, 0.73 * (1.0 - 0.9 * 0.30 * 0.35 * 1.0));
        assert!(trough < 0.73);
    }

    #[test]
    fn the_surface_height_clamps_at_both_ends() {
        assert_eq!(surface_height(0.99, 1.0, 1.0), 1.0);
        assert_eq!(surface_height(0.01, -1.0, 1.0), 0.0);
        assert_eq!(surface_height(0.5, 0.5, 1.0), 0.5 + 0.5 * 0.16);
    }

    #[test]
    fn the_detail_uv_scales_by_the_tile_count() {
        assert_eq!(detail_uv([0.25, -1.5], 11.0), [0.25 * 11.0, -1.5 * 11.0]);
        // The same helper scales a derivative, which is the whole point of
        // `owDetailP.x` reaching the `textureGrad` arguments too.
        assert_eq!(detail_uv([0.002, 0.004], 11.0), [0.002 * 11.0, 0.004 * 11.0]);
    }

    /// **The `detailWorld` derivation.** The measured bug the source documents:
    /// a prop at `scale` 0.55 m with an authored `detail[0]` of 10 used to map
    /// a 0.25 m bake into 55 mm. Derived, it gets `0.55 / 0.26` tiles instead —
    /// about 2.1, keeping the tooth at its authored physical size.
    #[test]
    fn the_tile_count_is_derived_from_scale_so_the_tooth_keeps_its_size() {
        let derived = detail_tiles(false, 0.55, Some(0.26), 10.0);
        assert_eq!(derived, 0.55 / 0.26);
        assert!(derived < 10.0, "the authored count was the bug");
        // A wall at 2 m: 2 / 0.26 tiles, and the same 0.26 m tooth.
        assert_eq!(detail_tiles(false, 2.0, Some(0.26), 11.0), 2.0 / 0.26);
        // The tooth is the SAME physical size at both scales, which is the
        // property the derivation exists to hold.
        let prop_metres_per_tile = 0.55 / detail_tiles(false, 0.55, Some(0.26), 10.0);
        let wall_metres_per_tile = 2.0 / detail_tiles(false, 2.0, Some(0.26), 11.0);
        assert_eq!(prop_metres_per_tile, wall_metres_per_tile);
    }

    #[test]
    fn the_derivation_defers_to_the_authored_count_in_the_three_documented_cases() {
        // Mesh UV: `scale` is a repeat count, not metres.
        assert_eq!(detail_tiles(true, 2.0, Some(0.26), 11.0), 11.0);
        // detailWorld opted out with 0, and a negative is the same opt-out.
        assert_eq!(detail_tiles(false, 2.0, Some(0.0), 11.0), 11.0);
        assert_eq!(detail_tiles(false, 2.0, Some(-1.0), 11.0), 11.0);
        // `!(dw > 0)` also catches a NaN, which `dw <= 0` would not.
        assert_eq!(detail_tiles(false, 2.0, Some(f32::NAN), 11.0), 11.0);
        // Finer than 0.3 m: a viewmodel part keeps its own, finer, detail.
        assert_eq!(detail_tiles(false, 0.12, Some(0.26), 11.0), 11.0);
        // 0.3 m exactly is NOT finer, so it derives — 0.3 / 0.26 is 1.154, under
        // the 1.2 floor, so the floor is what comes out. Either way it is not
        // the authored 11.0, which is the point.
        assert_eq!(detail_tiles(false, 0.3, Some(0.26), 11.0), 1.2);
        assert_eq!(detail_tiles(false, 0.35, Some(0.26), 11.0), 0.35 / 0.26);
    }

    #[test]
    fn the_derived_tile_count_has_a_floor_of_one_point_two() {
        // A very coarse mapping would derive fewer than 1.2 tiles.
        assert_eq!(detail_tiles(false, 0.31, Some(4.0), 11.0), 1.2);
        // And a NaN propagates through the floor as `Math.max` does.
        assert!(detail_tiles(false, f32::NAN, Some(0.26), 11.0).is_nan());
    }

    #[test]
    fn an_absent_detail_world_falls_back_to_the_default() {
        assert_eq!(
            detail_tiles(false, 2.0, None, 11.0),
            detail_tiles(false, 2.0, Some(DEFAULT_DETAIL_WORLD), 11.0)
        );
    }

    /// The WGSL names every entry point the orchestrator composes, and carries
    /// the two multiplies whose grouping the parity harness cannot see — the
    /// derivative scaling, which a single-mip parity texture cannot
    /// distinguish. Pinned as text so a "tidy-up" that drops them is caught.
    #[test]
    fn the_wgsl_defines_the_layers_entry_points_and_scales_the_derivatives() {
        [
            "fn axiom_detail_fade(",
            "fn axiom_detail_uv(",
            "fn axiom_detail_blend_normal(",
            "fn axiom_detail_blend_normal_projected(",
            "fn axiom_detail_micro(",
            "fn axiom_detail_albedo(",
            "fn axiom_detail_roughness(",
            "fn axiom_detail_height(",
            "fn axiom_detail(",
            "struct AxiomDetailOut {",
        ]
        .iter()
        .for_each(|name| {
            assert!(DETAIL_WGSL.contains(name), "the WGSL must define {name}");
        });
        assert!(DETAIL_WGSL.contains("let det_ddx = axiom_detail_uv(ddx, detail_p.x);"));
        assert!(DETAIL_WGSL.contains("let det_ddy = axiom_detail_uv(ddy, detail_p.x);"));
        assert!(DETAIL_WGSL.contains("textureSampleGrad(detail_normal_tex, detail_sampler, det_uv, det_ddx, det_ddy)"));
        assert!(DETAIL_WGSL.contains("textureSampleGrad(detail_tex, detail_sampler, det_uv, det_ddx, det_ddy)"));
    }
}

/// The CPU↔GPU parity proof for this layer, on a real adapter.
///
/// Compiled only with the `offscreen` feature, which is what makes an adapter
/// available, and it **asserts** one was acquired rather than skipping — a
/// parity test that passes when nothing ran proves nothing. The shape follows
/// `crate::surface_program::parity`; the harness is this file's own because
/// that module's is uniform-only and this layer samples textures.
///
/// ## The parity texture is procedural, nearest-sampled, and exact
///
/// Both detail textures are 8x8 single-mip `Rgba8Unorm`, filled by a fixed
/// integer recurrence (see [`parity_texel`]), and the sampler is `Repeat` with
/// **nearest** min/mag/mip. Two consequences, both deliberate:
///
/// - the fetched value is exactly `byte / 255.0` on both sides — an 8-bit unorm
///   converts exactly, so nothing about texture *filtering* precision (which is
///   only 8-bit fixed-point on plenty of hardware) leaks into a tolerance that
///   is supposed to measure this layer's arithmetic;
/// - the CPU can name the same texel, so the reference and the shader see
///   identical inputs rather than merely similar ones.
///
/// The cost is that the derivative arguments cannot change the answer through a
/// single mip, so parity pins the *values* and
/// `the_wgsl_defines_the_layers_entry_points_and_scales_the_derivatives` pins
/// that `owDetailP.x` reaches them. That limit is stated rather than papered
/// over.
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod parity {
    use super::{detail, detail_uv, DetailOut};

    /// How many shading points one run compares. Also the target's width.
    const SAMPLES: usize = 12;

    /// Rows of the readback target: the layer produces ten floats, which is
    /// three `Rgba32Float` pixels.
    const ROWS: u32 = 3;

    /// The parity textures' edge, in texels.
    const TEX_DIM: u32 = 8;

    /// `copy_texture_to_buffer` requires each row aligned to this many bytes.
    const ROW_ALIGN: u32 = 256;

    /// **The measured ceiling.** The worst absolute lane delta this comparison
    /// produced on a real adapter: `1.1920929e-7`, which is exactly `2^-23` —
    /// **one f32 ULP at unit magnitude**. The two sides differ by at most a
    /// single last-bit rounding, which is what a faithful transcription of a
    /// short arithmetic chain should look like once the texture fetch is made
    /// exact. Re-measured every run and asserted below, so the budget stays
    /// anchored to the measurement instead of to a guess.
    const MEASURED_WORST: f32 = 1.2e-7;

    /// The tolerance the layer is held to: 8.4x the measurement, inside the
    /// ten-times ceiling the brief sets. `1e-4` — the exact tier's budget next
    /// door — would be eight hundred times looser than this hardware needs,
    /// which is itself a failure.
    const TOLERANCE: f32 = 1.0e-6;

    /// One shading point: everything `axiom_detail` takes, apart from the two
    /// textures and the sampler.
    struct Sample {
        detail_p: [f32; 4],
        uv: [f32; 2],
        ddx: [f32; 2],
        ddy: [f32; 2],
        dist: f32,
        roughness: f32,
        n_tangent: [f32; 3],
        albedo: [f32; 4],
    }

    /// `(owDetailP, uv, owDist)` per sample, chosen so the run straddles every
    /// boundary this layer has: below the fade's near edge, on it, inside the
    /// window, exactly on the far edge, past it; four tile counts; and albedo
    /// alphas that drive `owHeightS` into both clamps.
    const POINTS: [([f32; 4], [f32; 2], f32); SAMPLES] = [
        ([11.0, 0.55, 0.35, 16.0], [0.313, 0.174], 3.0),
        ([11.0, 0.55, 0.35, 16.0], [0.730, 0.412], 10.0),
        // Exactly the far edge: the fade must be an exact zero here.
        ([11.0, 0.55, 0.35, 16.0], [0.061, 0.947], 16.0),
        ([11.0, 0.55, 0.35, 16.0], [0.463, 0.287], 40.0),
        // Exactly the near edge (0.45 * 16): the fade must be an exact one.
        ([11.0, 0.55, 0.35, 16.0], [0.881, 0.633], 7.2),
        ([4.0, 1.2, 0.9, 6.0], [0.226, 0.820], 4.1),
        ([7.0, 0.25, 2.5, 24.0], [0.545, 0.131], 13.7),
        ([13.0, 0.8, 0.6, 9.0], [0.372, 0.495], 5.5),
        ([2.0, 0.35, 1.5, 20.0], [0.649, 0.827], 12.3),
        ([5.0, 0.95, 0.45, 14.0], [0.194, 0.456], 6.6),
        ([9.0, 0.6, 3.0, 30.0], [0.839, 0.052], 18.9),
        ([3.0, 1.5, 0.8, 11.0], [0.428, 0.678], 2.2),
    ];

    /// Albedo alphas, i.e. the base height channel: two near 1 and two near 0,
    /// so `clamp( alb.a + owMicro * 0.16 * detFade, 0, 1 )` saturates on the GPU
    /// as well as on the CPU.
    const ALPHAS: [f32; SAMPLES] = [
        0.63, 0.99, 0.50, 0.12, 0.98, 0.05, 0.77, 0.99, 0.31, 0.02, 0.55, 0.99,
    ];

    /// The [`SAMPLES`] shading points. The normal is deliberately **not** unit
    /// length: the source does not promise one, and the blend renormalises.
    fn samples() -> Vec<Sample> {
        POINTS
            .iter()
            .zip(ALPHAS.iter())
            .enumerate()
            .map(|(index, ((detail_p, uv, dist), alpha))| {
                let i = index as f32;
                Sample {
                    detail_p: *detail_p,
                    uv: *uv,
                    ddx: [0.000_9 + 0.000_2 * i, -0.000_4],
                    ddy: [0.000_3, 0.001_1 - 0.000_05 * i],
                    dist: *dist,
                    roughness: 0.2 + 0.06 * i,
                    n_tangent: [0.30 - 0.05 * i, -0.22 + 0.06 * i, 0.90],
                    albedo: [0.2 + 0.05 * i, 0.5 - 0.02 * i, 0.3 + 0.03 * i, *alpha],
                }
            })
            .collect()
    }

    /// One texel of a parity texture: a fixed integer recurrence over the texel
    /// coordinates, distinct per texture, spread across the whole byte range so
    /// `owMicro` lands on both sides of zero and the red speckle both brightens
    /// and darkens.
    fn parity_texel(which: u32, x: u32, y: u32) -> [u8; 4] {
        let odd = u32::from(which == 1);
        let (rm, gm, bm, am) = ([31, 43][odd as usize], [53, 7][odd as usize], [13, 167][odd as usize], [71, 101][odd as usize]);
        let (rn, gn, bn, an) = ([17, 61][odd as usize], [89, 151][odd as usize], [197, 23][odd as usize], [29, 37][odd as usize]);
        let (rc, gc, bc, ac) = ([5, 19][odd as usize], [41, 97][odd as usize], [3, 131][odd as usize], [11, 7][odd as usize]);
        [
            ((x * rm + y * rn + rc) % 256) as u8,
            ((x * gm + y * gn + gc) % 256) as u8,
            ((x * bm + y * bn + bc) % 256) as u8,
            ((x * am + y * an + ac) % 256) as u8,
        ]
    }

    /// A whole parity texture, row-major RGBA8.
    fn parity_texture(which: u32) -> Vec<u8> {
        (0..TEX_DIM)
            .flat_map(|y| (0..TEX_DIM).flat_map(move |x| parity_texel(which, x, y)))
            .collect()
    }

    /// The texel a `Repeat`, nearest-filtered fetch returns for `uv`: wrap into
    /// `0..1`, scale by the dimension, take the floor. Exactly what the sampler
    /// does, and exact because an 8-bit unorm converts to `n / 255` with no
    /// rounding.
    fn sample_nearest(which: u32, uv: [f32; 2]) -> [f32; 4] {
        let wrapped = [uv[0] - uv[0].floor(), uv[1] - uv[1].floor()];
        let coord = wrapped.map(|c| ((c * TEX_DIM as f32).floor() as u32).min(TEX_DIM - 1));
        parity_texel(which, coord[0], coord[1]).map(|byte| f32::from(byte) / 255.0)
    }

    /// How far the fetched point sits from the nearest texel boundary, in
    /// texels. A sample that landed on a boundary could resolve to a different
    /// texel on the GPU than on the CPU for reasons that have nothing to do with
    /// this layer, so the run asserts a margin rather than hoping.
    fn boundary_margin(uv: [f32; 2]) -> f32 {
        uv.iter()
            .map(|c| {
                let wrapped = c - c.floor();
                let within = (wrapped * TEX_DIM as f32).fract();
                within.min(1.0 - within)
            })
            .fold(f32::MAX, f32::min)
    }

    /// The harness: a fullscreen triangle whose fragment stage evaluates
    /// `axiom_detail` at the shading point its pixel *column* names and returns
    /// the third of the result its pixel *row* names.
    const HARNESS_WGSL: &str = r#"
struct DetailParityInputs { items: array<vec4<f32>, 60> };

@group(0) @binding(0) var<uniform> detail_inputs: DetailParityInputs;
@group(0) @binding(1) var detail_normal_tex: texture_2d<f32>;
@group(0) @binding(2) var detail_albedo_tex: texture_2d<f32>;
@group(0) @binding(3) var detail_sampler: sampler;

@vertex
fn detail_parity_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

@fragment
fn detail_parity_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let index = u32(position.x);
    let row = u32(position.y);
    let a = detail_inputs.items[index * 5u + 0u];
    let b = detail_inputs.items[index * 5u + 1u];
    let c = detail_inputs.items[index * 5u + 2u];
    let d = detail_inputs.items[index * 5u + 3u];
    let e = detail_inputs.items[index * 5u + 4u];
    let result = axiom_detail(
        detail_normal_tex,
        detail_albedo_tex,
        detail_sampler,
        b.xy,
        b.zw,
        c.xy,
        c.z,
        a,
        d.xyz,
        e,
        c.w,
    );
    var rows = array<vec4<f32>, 3>(
        vec4<f32>(result.normal_tangent, result.height),
        vec4<f32>(result.albedo, result.roughness),
        vec4<f32>(result.micro, result.fade, 0.0, 0.0),
    );
    return rows[row];
}
"#;

    /// The uniform's bytes: five `vec4` per sample, in the order
    /// `detail_parity_fs` unpacks them.
    fn input_bytes(samples: &[Sample]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|s| {
                [
                    s.detail_p[0],
                    s.detail_p[1],
                    s.detail_p[2],
                    s.detail_p[3],
                    s.uv[0],
                    s.uv[1],
                    s.ddx[0],
                    s.ddx[1],
                    s.ddy[0],
                    s.ddy[1],
                    s.dist,
                    s.roughness,
                    s.n_tangent[0],
                    s.n_tangent[1],
                    s.n_tangent[2],
                    0.0,
                    s.albedo[0],
                    s.albedo[1],
                    s.albedo[2],
                    s.albedo[3],
                ]
            })
            .flat_map(f32::to_le_bytes)
            .collect()
    }

    /// The CPU reference's answer for one sample, laid out the way the harness
    /// lays out its three rows, so the comparison is lane-for-lane.
    fn cpu_rows(sample: &Sample) -> [[f32; 4]; 3] {
        let uv = detail_uv(sample.uv, sample.detail_p[0]);
        let out: DetailOut = detail(
            sample.detail_p,
            sample.dist,
            sample_nearest(0, uv),
            sample_nearest(1, uv),
            sample.n_tangent,
            sample.albedo,
            sample.roughness,
        );
        [
            [
                out.normal_tangent[0],
                out.normal_tangent[1],
                out.normal_tangent[2],
                out.height,
            ],
            [out.albedo[0], out.albedo[1], out.albedo[2], out.roughness],
            [out.micro, out.fade, 0.0, 0.0],
        ]
    }

    /// Acquire a native adapter, or fail loudly.
    fn acquire() -> (wgpu::Device, wgpu::Queue, wgpu::Backend) {
        // The crate's ONE instance + adapter + device (see `crate::test_gpu`):
        // ~50 tests each opening their own is what crashes the driver.
        let gpu = crate::test_gpu::TestGpu::shared();
        let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
        let backend = gpu.backend;
        (device, queue, backend)
    }

    /// Upload one parity texture and return its view.
    fn upload(device: &wgpu::Device, queue: &wgpu::Queue, which: u32) -> wgpu::TextureView {
        let size = wgpu::Extent3d {
            width: TEX_DIM,
            height: TEX_DIM,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-detail-parity-texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &parity_texture(which),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * TEX_DIM),
                rows_per_image: Some(TEX_DIM),
            },
            size,
        );
        texture.create_view(&wgpu::TextureViewDescriptor::default())
    }

    /// Render the harness over a `SAMPLES x ROWS` `Rgba32Float` target and read
    /// every pixel back, as `[sample][row][lane]`.
    fn render(device: &wgpu::Device, queue: &wgpu::Queue, samples: &[Sample]) -> Vec<[[f32; 4]; 3]> {
        let module = {
            // The error scope is the SHARED device's, so it is entered exclusively;
            // see `crate::test_gpu::validating`.
            let (module, failure) = crate::test_gpu::validating(&device, || {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("axiom-detail-parity-shader"),
                    source: wgpu::ShaderSource::Wgsl(
                        [super::DETAIL_WGSL, HARNESS_WGSL].concat().into(),
                    ),
                })
            });
            assert!(
                failure.is_none(),
                "the detail layer's WGSL must compile: {failure:?}"
            );
            module
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("axiom-detail-parity-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let uniform = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("axiom-detail-parity-uniform"),
                contents: &input_bytes(samples),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let normal_view = upload(device, queue, 0);
        let albedo_view = upload(device, queue, 1);
        // Nearest everywhere, and `Repeat` because `uv * owDetailP.x` walks well
        // past 1. Nearest is what makes the fetched value exactly `n / 255` on
        // both sides.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("axiom-detail-parity-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("axiom-detail-parity-bg"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&normal_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&albedo_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("axiom-detail-parity-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("axiom-detail-parity-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("detail_parity_vs"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("detail_parity_fs"),
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
        let size = wgpu::Extent3d {
            width: SAMPLES as u32,
            height: ROWS,
            depth_or_array_layers: 1,
        };
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-detail-parity-target"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let row_bytes = (SAMPLES as u32 * 16).div_ceil(ROW_ALIGN) * ROW_ALIGN;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axiom-detail-parity-readback"),
            size: u64::from(row_bytes * ROWS),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axiom-detail-parity-pass"),
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
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_bytes),
                    rows_per_image: Some(ROWS),
                },
            },
            size,
        );
        queue.submit(Some(encoder.finish()));
        let slice = readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::Wait)
            .expect("the readback must complete");
        let mapped = slice.get_mapped_range();
        (0..SAMPLES)
            .map(|sample| {
                [0_usize, 1, 2].map(|row| {
                    [0_usize, 1, 2, 3].map(|lane| {
                        let at = row * row_bytes as usize + sample * 16 + lane * 4;
                        f32::from_le_bytes([
                            mapped[at],
                            mapped[at + 1],
                            mapped[at + 2],
                            mapped[at + 3],
                        ])
                    })
                })
            })
            .collect()
    }

    /// **The parity proof.** Every shading point, both sides, on a real adapter,
    /// at a tolerance derived from the measurement this test re-takes.
    #[test]
    fn the_detail_layer_agrees_with_its_cpu_reference_on_a_real_adapter() {
        let (device, queue, backend) = acquire();
        assert_ne!(
            backend,
            wgpu::Backend::Noop,
            "the parity proof is worthless unless a real backend ran it"
        );
        let samples = samples();
        // A sample that landed on a texel boundary would make the comparison
        // about the sampler's rounding rather than about this layer.
        samples.iter().enumerate().for_each(|(index, sample)| {
            let margin = boundary_margin(detail_uv(sample.uv, sample.detail_p[0]));
            assert!(
                margin > 0.05,
                "sample {index} sits {margin} texels from a boundary; move its uv"
            );
        });
        let rendered = render(&device, &queue, &samples);
        let worst = samples
            .iter()
            .zip(rendered.iter())
            .enumerate()
            .fold(0.0_f32, |worst, (index, (sample, gpu))| {
                let cpu = cpu_rows(sample);
                (0..3).fold(worst, |worst, row| {
                    (0..4).fold(worst, |worst, lane| {
                        let delta = (cpu[row][lane] - gpu[row][lane]).abs();
                        assert!(
                            delta <= TOLERANCE,
                            "the detail layer disagrees at sample {index} row {row} lane {lane}: \
                             CPU {} vs GPU {} (delta {delta}, tolerance {TOLERANCE})",
                            cpu[row][lane],
                            gpu[row][lane]
                        );
                        worst.max(delta)
                    })
                })
            });
        assert!(
            worst <= MEASURED_WORST,
            "the measured worst delta drifted to {worst}, above the recorded \
             {MEASURED_WORST}; re-derive the tolerance rather than widening it"
        );
        assert!(
            TOLERANCE <= 10.0 * MEASURED_WORST,
            "a tolerance more than ten times the measurement is itself a failure"
        );
    }

    /// The parity run is only worth its runtime if it actually straddles the
    /// boundaries it claims to. Proven on the **GPU's** own output, so a CPU-side
    /// mistake cannot certify it.
    #[test]
    fn the_parity_run_straddles_every_boundary_it_claims_to() {
        let (device, queue, backend) = acquire();
        assert_ne!(backend, wgpu::Backend::Noop);
        let samples = samples();
        let rendered = render(&device, &queue, &samples);
        let fades: Vec<f32> = rendered.iter().map(|rows| rows[2][1]).collect();
        let micros: Vec<f32> = rendered.iter().map(|rows| rows[2][0]).collect();
        let heights: Vec<f32> = rendered.iter().map(|rows| rows[0][3]).collect();
        assert!(
            fades.iter().any(|f| *f == 0.0),
            "no sample reached the far end of the fade: {fades:?}"
        );
        assert!(
            fades.iter().any(|f| *f == 1.0),
            "no sample reached the near end of the fade: {fades:?}"
        );
        assert!(
            fades.iter().any(|f| *f > 0.0 && *f < 1.0),
            "no sample sat inside the fade window: {fades:?}"
        );
        assert!(
            micros.iter().any(|m| *m < 0.0) && micros.iter().any(|m| *m > 0.0),
            "owMicro must land on both sides of zero, or the cavity darkening \
             is never exercised: {micros:?}"
        );
        assert!(
            heights.iter().any(|h| *h == 1.0) && heights.iter().any(|h| *h == 0.0),
            "owHeightS must saturate at both clamps: {heights:?}"
        );
    }

    /// **The far end, on the GPU.** At `owDist == owDetailP.w` the layer must
    /// contribute exactly nothing: the albedo, the roughness and the height come
    /// back bit-identical to the values that went in, whatever the detail texels
    /// said. Asserted on the rendered pixels, because "exactly zero" is a claim
    /// about the hardware's arithmetic and not only about the reference's.
    #[test]
    fn the_far_end_contributes_exactly_nothing_on_the_gpu() {
        let (device, queue, backend) = acquire();
        assert_ne!(backend, wgpu::Backend::Noop);
        let samples = samples();
        let rendered = render(&device, &queue, &samples);
        // Samples 2 (dist == fade metres, exactly) and 3 (well past it).
        [2_usize, 3].iter().for_each(|index| {
            let sample = &samples[*index];
            let rows = rendered[*index];
            assert_eq!(rows[2][1], 0.0, "sample {index} must have a zero fade");
            assert_eq!(
                [rows[1][0], rows[1][1], rows[1][2]],
                [sample.albedo[0], sample.albedo[1], sample.albedo[2]],
                "sample {index}'s albedo must be untouched"
            );
            assert_eq!(rows[1][3], sample.roughness);
            assert_eq!(rows[0][3], sample.albedo[3]);
        });
    }
}
