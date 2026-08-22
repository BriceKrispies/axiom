//! The twelve layers, composed into one `axiom_surface`.
//!
//! Every sibling in this module is a *fragment* of Claude-of-Duty
//! `src/materials/shader.js`: a `&str` of WGSL, a CPU reference, and a parity
//! test. None of them is a program. This file is the one that makes them one —
//! it concatenates their WGSL and then hand-writes the `MAIN_FRAGMENT` the
//! source wrote, calling each layer in **the source's order**.
//!
//! ## The order is the specification
//!
//! Float arithmetic is not associative, so "which layer runs first" is not a
//! style question — a layer applied out of order compiles, renders, and looks
//! plausible. The order below is `MAIN_FRAGMENT`'s, line by line, and the line
//! numbers in the WGSL comments are `shader.js`'s so the two can be diffed by
//! eye:
//!
//! | `shader.js` | layer | what it does |
//! |---|---|---|
//! | 255-267 | — | `owDist`, `owFaceDir`, `owNw`, `owP`, `owNp` |
//! | 323-336 | `uv_mode` + `frames` | the projection frame and its uv |
//! | 338-351 | `pom` | derivatives, then the parallax march |
//! | 353-356 | — | the three base fetches; `tint_wear`'s `owNormalAmp` |
//! | 358-369 | `detile` | the second, rotated sample *(structurally gated)* |
//! | 371-378 | `detail` | the micro normal, folded into the base sample |
//! | 379-383 | `detile` | fold into sample two, mask, height blend |
//! | 385-393 | `detail` | micro albedo, cavity roughness, `owHeightS` |
//! | 396-449 | `macro_variation` | two bands, hue, roughness, relief |
//! | 451-490 | `patches` | repair patches on vertical faces |
//! | 492-565 | `weathering` | dust, rain runoff, ground splash, wedge |
//! | 567-596 | `masks` | cavity grime + the vertex-colour masks |
//! | 598-618 | `cloth` | underside, fold |
//! | 620-628 | `tint_wear` | tint, roughness remap, the channel assignment |
//! | 650-666 | `cloth` | the transmission channel |
//!
//! ## `CLOTH_WGSL` is **not** concatenated here
//!
//! [`crate::surface_program::wgsl_template::scene_shader`] splices it into
//! *every* scene shader already, because the lighting stage (in the suffix)
//! calls `axiom_cloth_light` and `axiom_cloth_transmitted`. Including it again
//! would be a duplicate definition and the module would not compile. The
//! composition calls its functions; they are in scope.
//!
//! ## De-tiling is gated **structurally**, which is why this is a function
//!
//! The source gates the block with a preprocessor define — `if (p.detile > 0 &&
//! p.uvMode !== 'triplanar') defines.OW_DETILE = ''` — and the `detile` layer
//! measured that a runtime `t = 0` is **not** bit-identical to omitting it (1
//! ULP on 17.2% of operands). So the block is present or absent in the emitted
//! text, and a program text with two shapes cannot be a `&str` constant.
//! [`material_surface_wgsl`] is therefore a function of the gate, and
//! [`material_program`] derives the gate from the same [`MaterialParams`] it
//! packs — one decision, not two that can drift.
//!
//! ## What the composition could not thread, and why
//!
//! Each of these is a **contract gap**, not a shortcut taken here. They are
//! listed in `docs/work-manifests/shmup-port/notes/material-compose.md` with the
//! exact change each needs.
//!
//! * **View distance.** `owDist` is `length( vViewPosition )`, and `SurfaceIn`
//!   carries `view_dir` *normalised*, so the distance is gone. [`OW_DIST`] is
//!   bound to `0.0` — every fragment is treated as at the camera, which makes
//!   both distance fades (POM's `parallaxFade`, detail's `detail[3]`) evaluate
//!   to exactly `1.0`. That is the near-field behaviour, so both layers do their
//!   full work and are observable; what is missing is the fade to nothing.
//! * **`gl_FrontFacing`.** `owFaceDir` is `+1` here. A back face reads its own
//!   normal rather than the flipped one.
//! * **Vertex-colour masks.** `SurfaceIn` now has a `vertex_color` lane, but it
//!   is the wrong quantity for this: it is the vertex colour times the
//!   *instance* colour, which this layer multiplies into the albedo the way
//!   three does. The source's `vColor` is a per-vertex **mask** (wear, grime,
//!   AO), because it *overrides* `<color_fragment>` precisely so the lane stays
//!   a mask rather than a tint. Those are different quantities that happen to
//!   share a name.
//!
//!   So the mask still passes `vec3<f32>(0.0, 0.0, 0.0)` — exactly what the
//!   default (`vertexMasks: false`) means — and the flag is threaded so the
//!   plumbing is right the moment a real mask lane exists. Conflating the two
//!   would tint by a mask and mask by a tint.
//! * **View space.** The source's `nShade` is a view-space normal and three
//!   layers perturb it there. This composition builds the shading normal in
//!   **world** space and passes the identity for `mat3( viewMatrix )`. A
//!   rotation commutes with a linear combination and with `normalize`, so the
//!   macro-relief tilt and the weathering layer's two softening mixes are the
//!   same value in either space. The **cloth fold** is the exception: it adds a
//!   *view-space* `xy` offset, and there is no world-space equivalent without
//!   the view matrix. It is applied in world space, and that is the one
//!   knowingly-inexact substitution in this file.
//! * **AO.** `owORM.r` has no `SurfaceOut` lane. `aoStrength` is deliberately
//!   **not** applied here: the `masks` layer found it belongs at the lighting
//!   stage (`shader.js:678`, `( owORM.r - 1.0 ) * owAoAmt + 1.0`, a lerp toward
//!   1). `axiom_masks_ambient_occlusion` is therefore defined and uncalled, and
//!   the AO every layer computes reaches only `axiom_cloth_transmission`, which
//!   does read it.
//! * **Triplanar.** `OW_TRIPLANAR` is a third *permutation* with its own
//!   nine-fetch sampling and its own detail arm, not a runtime mode. Planar and
//!   mesh differ only in the frame, so those two are selected with `select` and
//!   share one program; triplanar is not composed.

// `patches.rs` is `#![cfg(any(test, target_arch = "wasm32", feature = "offscreen"))]`,
// `patches.rs` used to carry a file-wide
// `#![cfg(any(test, target_arch = "wasm32", feature = "offscreen"))]`, so
// `PATCHES_WGSL` did not exist on a default-feature native build and this file
// inherited the same gate.
//
// Both gates are gone. `surface_program::cache::generate` — which is compiled on
// every target — now composes a runtime material's program, so the composition
// has to exist everywhere too. Nothing here is GPU-conditional: every layer is a
// `&str` and a CPU reference, and only the *tests* need an adapter. Gating a
// string on a rendering feature was the accident; the eleven other layers were
// already ungated.

use super::detail::DETAIL_WGSL;
use super::detile::{detile_enabled, DETILE_WGSL};
use super::frames::FRAMES_WGSL;
use super::macro_variation::MACRO_VARIATION_WGSL;
use super::masks::MASKS_WGSL;
use super::params::{MaterialParams, UvMode, SLOT_COUNT};
use super::patches::PATCHES_WGSL;
use super::pom::POM_WGSL;
use super::tint_wear::TINT_WEAR_WGSL;
use super::uv_mode::UV_MODE_WGSL;
use super::weathering::WEATHERING_WGSL;

/// `owDist`, as this composition can supply it.
///
/// The source's `float owDist = length( vViewPosition );`. `SurfaceIn::view_dir`
/// is normalised, so the length is not recoverable and this is `0.0`. Named
/// rather than inlined because it is a one-line change the moment `SurfaceIn`
/// carries a view distance, and because a test asserts the composition binds it
/// — a silent `0.0` in the middle of a 200-line shader is not findable.
pub(crate) const OW_DIST: &str = "0.0";

/// The layers' WGSL, in dependency order.
///
/// `frames` and `uv_mode` first because they build the projection the rest
/// sample through; `tint_wear` last because it names `SurfaceOut`. `cloth` is
/// **absent** — see this module's header.
const LAYERS: [&str; 10] = [
    FRAMES_WGSL,
    UV_MODE_WGSL,
    POM_WGSL,
    DETILE_WGSL,
    DETAIL_WGSL,
    MACRO_VARIATION_WGSL,
    PATCHES_WGSL,
    WEATHERING_WGSL,
    MASKS_WGSL,
    TINT_WEAR_WGSL,
];

/// The composition, from the signature down to the first `#ifdef OW_DETILE`.
const SURFACE_HEAD: &str = r#"
// ===========================================================================
// The runtime material shader: Claude-of-Duty `src/materials/shader.js`'s
// MAIN_FRAGMENT, composed from the twelve layers of
// `modules/axiom-gpu-backend/src/material_shader/`.
//
// Every `shader.js:NNN` below is that file's line. The ORDER IS THE
// SPECIFICATION: float arithmetic is not associative, so a layer applied out of
// order compiles and looks plausible.
// ===========================================================================

// three's `MeshStandardMaterial` gathers diffuse AND a highlight, which is
// Axiom's `LambertSpecular` (code 2) — the same model every existing draw runs.
fn axiom_lighting_model() -> u32 {
    return 2u;
}

fn axiom_surface(in: SurfaceIn, params: SurfaceParams) -> SurfaceOut {
    // ---- the parameter block ------------------------------------------------
    // `material_shader/params.rs`'s slot map, which is pinned by a test there.
    // Read by index, never invented.
    let p_uv = params.slots[0];      // uv_mode, local_space, scale, parallax
    let p_off = params.slots[1];     // offset.xy, parallax fade near / far
    let p_misc = params.slots[2];    // parallax layers, detail_world, relief, detile
    let p_detail = params.slots[3];  // detail[0..4] — .x is DERIVED below
    let macro_p = params.slots[4];   // owMacroP
    let macro_big = params.slots[5]; // owMacroBig
    let patch_p = params.slots[6];   // owPatchP
    let cloth_p = params.slots[7];   // owClothP
    let weather_p = params.slots[8]; // owWeatherP
    let wear_p = params.slots[9];    // owWearP
    let wear_mat = params.slots[10]; // owWearMat
    let p_rough = params.slots[11];  // rough scale, offset, MINIMUM, ao_strength
    let p_flags = params.slots[12];  // normal_strength, ground_y, alpha_mask, vcol_masks
    let tint_col = params.slots[14].xyz;
    let wear_col = params.slots[15].xyz;
    let dust_col = params.slots[16].xyz;
    let grime_col = params.slots[17].xyz;
    let rust_col = params.slots[18].xyz;

    // `owRoughP` is NOT slot 11. `extendMaterial` (shader.js:836-843) packs it as
    // ( roughness[0], roughness[1], DETILE, roughness[2] ) — the de-tiling amount
    // rides in .z and the per-surface floor in .w. `tint_wear` reads .w as the
    // floor and `detile` reads .z as the blend amount, so the vector has to be
    // rebuilt here or both would read the wrong lane.
    let rough_p = vec4<f32>(p_rough.x, p_rough.y, p_misc.w, p_rough.z);
    let normal_amp = p_flags.x;
    let ground_y = p_flags.y;
    let alpha_mask = p_flags.z;
    let vcol_masks = p_flags.w;
    // `vColor`. `SurfaceIn` has no vertex-colour lane and the source's OVERRIDES
    // keep the lane a MASK rather than a tint, so it cannot be recovered from
    // `in.albedo` (which already has it multiplied in). Zero is exactly what the
    // default `vertexMasks: false` means. See this module's header.
    let vertex_color = vec3<f32>(0.0, 0.0, 0.0);

    // `extendMaterial:793` — mesh UV treats `scale` as a repeat count, projected
    // modes as metres per tile.
    let is_mesh = p_uv.x > 1.5;
    let local_space = p_uv.y;
    let scale = p_uv.z;
    let tile_scale = select(1.0 / scale, scale, is_mesh);
    let tile = vec4<f32>(tile_scale, tile_scale, p_off.x, p_off.y);
    // `extendMaterial:805-810` — owDetailP.x is DERIVED from `scale` so the micro
    // tooth keeps a fixed size in metres. `Math.max(1.2, x)` propagates a NaN,
    // which `max` does not, so it is the select the source's JavaScript is.
    let dw = p_misc.y;
    let derived_tiles = scale / dw;
    let detail_p = vec4<f32>(
        select(
            select(derived_tiles, 1.2, 1.2 > derived_tiles),
            p_detail.x,
            is_mesh | !(dw > 0.0) | (scale < 0.3),
        ),
        p_detail.y,
        p_detail.z,
        p_detail.w,
    );

    // ---- shader.js:255-267 --------------------------------------------------
    // `float owDist = length( vViewPosition );` — see this module's header: the
    // view distance is not on `SurfaceIn`, so both distance fades read 1.0.
    let ow_dist = 0.0;
    // `float owFaceDir = gl_FrontFacing ? 1.0 : -1.0;` — no front-facing lane.
    let face_dir = 1.0;
    let ow_nw = normalize(in.world_normal) * face_dir;
    let ow_p = axiom_uv_projection_pos(in.object_pos, in.world_pos, local_space);
    let ow_np = axiom_uv_projection_normal(in.object_normal, in.world_normal, face_dir, local_space);

    // ---- shader.js:323-336 — the frame --------------------------------------
    // OW_MESH_UV takes the interpolated parameterisation and a screen-space
    // Mikkelsen frame; the projected modes take the dominant-axis frame
    // re-anchored on the true normal. Both are arithmetic, so one program
    // carries both and `select` picks — unlike triplanar, which is a permutation.
    let base_uv = axiom_uv_tile(in.uv, tile);
    let tbn = owTangentFrameScreen(in.world_pos, ow_nw, base_uv);
    var f = owAxisFrame(ow_p, ow_np, axiom_uv_dominant_axis(ow_np), tile);
    owOrthonormalise(&f, ow_np);
    let f_uv = select(f.uv, base_uv, is_mesh);
    let f_t = select(f.T, tbn[0], is_mesh);
    let f_b = select(f.B, tbn[1], is_mesh);
    let f_n = select(f.N, tbn[2], is_mesh);

    // ---- shader.js:338-351 — derivatives, then the parallax march -----------
    let ddx = dpdx(f_uv);
    let ddy = dpdy(f_uv);
    let vt = axiom_pom_view_tangent(in.view_dir, f_t, f_b, f_n);
    // `owPOM` returns `uv` unchanged for `depth <= 0.0`, which IS the source's
    // `#ifdef OW_PARALLAX` gate (`p.parallax > 0`) — an exact identity, not an
    // approximation of one, so this needs no permutation.
    let uv = axiom_pom(
        f_uv, vt, ddx, ddy, p_uv.w,
        axiom_pom_fade(ow_dist, p_off.z, p_off.w),
        p_misc.x, albedo_tex, albedo_sampler,
    );

    // ---- shader.js:353-356 — the three base fetches -------------------------
    // `OW_TEX` is `textureGrad` unless OW_NOGRAD; slot 13's `no_grad` selects the
    // implicit-derivative fallback, which this composition does not carry (it
    // would be a fourth permutation for a debugging flag).
    // The per-vertex and per-instance colour multiplies the sample, exactly as
    // three multiplies `diffuse * vColor` into `diffuseColor` before the
    // material shader touches it.
    //
    // It has to be applied HERE rather than inherited from `in.albedo`, because
    // this layer re-samples the albedo at its OWN projected uv — planar or
    // triplanar, in world space — and `in.albedo` was sampled at the mesh uv.
    // Taking our own sample and forgetting the colour is what turned an app's
    // whole palette grey the first time this ran end to end.
    var alb = textureSampleGrad(albedo_tex, albedo_sampler, uv, ddx, ddy) * in.vertex_color;
    var orm = textureSampleGrad(material_orm_tex, albedo_sampler, uv, ddx, ddy).rgb;
    // `nT.xy *= owNormalAmp;` — tangent xy only, z untouched, no renormalise.
    var n_t = axiom_mat_normal_strength(
        textureSampleGrad(normal_tex, normal_sampler, uv, ddx, ddy).xyz * 2.0 - 1.0,
        normal_amp,
    );
"#;

/// `#ifdef OW_DETILE`, first half — `shader.js:358-369`.
///
/// Emitted only when [`detile_enabled`]. A runtime `t = 0` through the height
/// blend is **not** bit-identical to omitting these fetches, which is why the
/// gate is textual.
const DETILE_SAMPLE: &str = r#"
    // ---- shader.js:358-369 — #ifdef OW_DETILE, the second sample ------------
    // The same texture set, rotated ~36.6 degrees and rescaled, with its own
    // screen-space footprint. Present in the emitted text only when de-tiling is
    // on: a runtime zero is not the same bits as an absent block.
    let detile_s2 = axiom_detile_second_sample(
        albedo_tex, albedo_sampler,
        material_orm_tex, albedo_sampler,
        normal_tex, normal_sampler,
        uv, ddx, ddy, normal_amp,
    );
"#;

/// Between the two `#ifdef OW_DETILE` halves — `shader.js:371-378`.
const SURFACE_MID: &str = r#"
    // ---- shader.js:371-378 — the micro detail normal ------------------------
    // `owDetailTex` is `shared.detailAlbedo ?? shared.detailNormal`
    // (extendMaterial:812), and group 0 binds one detail map, so both fetches
    // take it — the source's own fallback, not a substitution invented here.
    let det_fade = axiom_detail_fade(detail_p.w, ow_dist);
    let det_uv = axiom_detail_uv(uv, detail_p.x);
    let det_ddx = axiom_detail_uv(ddx, detail_p.x);
    let det_ddy = axiom_detail_uv(ddy, detail_p.x);
    // Binding 5 packs `(normal.x, normal.y, micro_albedo, height)`: ONE bound map
    // serves the source's two, `owDetailNrm` and `owDetailTex`. That is lossless
    // because between them the source consumes exactly four scalars — the
    // normal's xy, `dTex.r` and `dTex.a` — and no consumer of `dn` reads `dn.z`
    // (both are UDN: they sum the tangent xy and keep the BASE z).
    //
    // `.b` therefore is NOT the normal's z. Reconstruct z from the xy rather
    // than letting the speckle lane masquerade as it, so a future consumer that
    // does read `dn.z` inherits a real normal instead of a silent wrong answer.
    let dn_xy = textureSampleGrad(material_detail_tex, albedo_sampler, det_uv, det_ddx, det_ddy).xy
        * 2.0 - 1.0;
    let dn = vec3<f32>(dn_xy, sqrt(max(0.0, 1.0 - dot(dn_xy, dn_xy))));
    // UDN: sum the tangent xy, keep the BASE z, renormalise.
    n_t = axiom_detail_blend_normal(n_t, dn, detail_p.y, det_fade);
"#;

/// `#ifdef OW_DETILE`, second half — `shader.js:379-383`.
///
/// The detail normal is folded into sample two **the same way** it was just
/// folded into the base sample, so the height blend mixes like with like. That
/// interleave is why this is two chunks and not one.
const DETILE_BLEND: &str = r#"
    // ---- shader.js:379-383 — #ifdef OW_DETILE, the height blend -------------
    let detile_n2 = axiom_detile_fold_detail_normal(detile_s2.normal, dn, detail_p.y, det_fade);
    let detile_mask = axiom_detile_mask(material_macro_tex, albedo_sampler, ow_p, macro_p.x);
    // `owRoughP.z` is the de-tiling amount — see the rebuild of `rough_p` above.
    axiom_detile_height_blend(
        &alb, &orm, &n_t,
        detile_s2.albedo, detile_s2.orm, detile_n2,
        detile_mask * rough_p.z,
    );
"#;

/// Everything after the second `#ifdef OW_DETILE` — `shader.js:385-628`.
const SURFACE_TAIL: &str = r#"
    // ---- shader.js:385-393 — micro albedo, cavity roughness, owHeightS ------
    // The same packed texel, unpacked into the logical `owDetailTex` the source
    // samples: `.b` is the micro-albedo lane `dTex.r` and `.a` is the height.
    // Unpacking HERE — not inside `axiom_detail_albedo` — keeps
    // `material_shader::detail` the faithful two-texture definition of the
    // source, and confines the one-map packing to the composition that chose it.
    let d_packed = textureSampleGrad(material_detail_tex, albedo_sampler, det_uv, det_ddx, det_ddy);
    let d_tex = vec4<f32>(d_packed.b, d_packed.b, d_packed.b, d_packed.a);
    let micro = axiom_detail_micro(d_tex);
    alb = vec4<f32>(axiom_detail_albedo(alb.rgb, d_tex, micro, detail_p.z, det_fade), alb.a);
    // `orm.r` is AO. A trough is a tiny occluded pocket, so it darkens.
    orm.r = axiom_detail_roughness(orm.r, micro, detail_p.z, det_fade);
    var height_s = axiom_detail_height(alb.a, micro, det_fade);
    // `nShade = normalize( owP2V * ( f.T*nT.x + f.B*nT.y + f.N*nT.z ) )`, in
    // WORLD space: the composition has no view matrix, and a rotation commutes
    // with both the linear combination and the normalize. See the header.
    let n_world = normalize(f_t * n_t.x + f_b * n_t.y + f_n * n_t.z);

    // ---- shader.js:396-449 — macro variation --------------------------------
    // `mac1`, `mac2` and `owUpFace` escape this call because five later sections
    // read them; recomputing them would double four fetches and fork the uv.
    let mv = axiom_macro_variation(
        in.world_pos, ow_nw, alb.rgb, orm.g, n_world,
        // `mat3( viewMatrix )` — the identity, because the normal above is
        // already in the space the relief tilt is built in.
        mat3x3<f32>(1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0),
        micro, det_fade, macro_p, macro_big, p_misc.z,
        material_macro_tex, albedo_sampler,
    );
    alb = vec4<f32>(mv.albedo, alb.a);
    orm.g = mv.roughness;
    var n_shade = mv.shade_normal;

    // ---- shader.js:451-490 — repair patches ---------------------------------
    // `#ifdef OW_PATCH` is `patch[0] > 0`, and coverage 0 makes `has` an exact
    // `step(1.0, r0)` with `r0 = fract(...) < 1`, i.e. an exact zero mask and an
    // exact identity. No permutation needed, unlike de-tiling.
    //
    // `owVert` / `owSAxis` (shader.js:446-447) are derived INSIDE this call and
    // again inside the weathering stack. They are not hoisted: both layers take
    // them from `world_pos` and `nw` with byte-identical expressions, and
    // hoisting would mean changing two layer files' signatures, which this file
    // may not do. The cost is a handful of duplicated ALU ops. Same for
    // `owHash11`, which is `axiom_patch_hash11` in one layer and `ow_hash11` in
    // the other.
    let patched = axiom_patch_apply(
        in.world_pos, ow_nw, mv.mac2.rg, patch_p, alb.rgb, orm.g, height_s,
    );
    alb = vec4<f32>(patched.albedo, alb.a);
    orm.g = patched.roughness;
    height_s = patched.height;

    // ---- shader.js:492-565 — weathering -------------------------------------
    // `n_flat` is `normalize( owP2V * owNp )`; in world space that is `owNp`.
    let weathered = ow_weather_stack(
        OwWeatherState(alb.rgb, orm, n_shade),
        in.world_pos, ow_nw, ow_np, mv.mac1, mv.mac2,
        vertex_color, vcol_masks, weather_p, ground_y,
        dust_col, grime_col, rust_col,
        material_macro_tex, albedo_sampler,
    );
    alb = vec4<f32>(weathered.albedo, alb.a);
    orm = weathered.orm;
    n_shade = weathered.n_shade;

    // ---- shader.js:567-596 — cavity grime + the vertex-colour masks ---------
    // `height_s` is taken AFTER the repair patches raised it (shader.js:487).
    let masked = axiom_masks_apply(
        alb.rgb, orm, height_s, vertex_color, mv.mac1, mv.mac2,
        grime_col, wear_col, wear_mat, wear_p, weather_p,
        vcol_masks > 0.5,
    );
    alb = vec4<f32>(masked.albedo, alb.a);
    orm = masked.orm;

    // ---- shader.js:598-618 — cloth ------------------------------------------
    // `CLOTH_WGSL` is spliced into every scene shader by `scene_shader`, so its
    // functions are in scope and must NOT be concatenated again here.
    let underside = axiom_cloth_underside(alb.rgb, orm.g, ow_nw, cloth_p);
    alb = vec4<f32>(underside.xyz, alb.a);
    orm.g = underside.w;
    // The three fold fetches. The source guards them with `if ( owClothP.z > 0 )`
    // to SKIP them; `axiom_cloth_fold` is total and returns its inputs unchanged
    // when the fold is off, so the guard is a fetch optimisation this
    // composition trades for uniform control flow around an implicit-LOD sample.
    let fold_uv = axiom_cloth_fold_uv(in.world_pos);
    let fold = axiom_cloth_fold(
        alb.rgb, n_shade, cloth_p,
        textureSample(material_macro_tex, albedo_sampler, fold_uv).b,
        textureSample(material_macro_tex, albedo_sampler, fold_uv + AXIOM_CLOTH_FOLD_DX).b,
        textureSample(material_macro_tex, albedo_sampler, fold_uv + AXIOM_CLOTH_FOLD_DY).b,
    );

    // ---- shader.js:620-628 + the OVERRIDES — the channel assignment ---------
    // tint (:621), the roughness remap (:624), then all six fixed channels.
    // `in.albedo.w` is three's `diffuseColor.a`, the material opacity the
    // pipeline already resolved; `alpha_mask` folds `owAlbedo.a` into it.
    var out = axiom_mat_finish(
        vec4<f32>(fold.albedo, alb.a), orm, fold.normal, in.emissive,
        tint_col, rough_p, in.albedo.w, alpha_mask,
    );
    // The seventh channel. `CLOTH_LIGHT` (:650-666) multiplies the per-light sum
    // by `owClothP.x * clamp( owORM.r, 0, 1 )`; the sum lives in the lighting
    // stage, this scalar is the surface's half of it. It is also the ONLY
    // consumer of `orm.r` — `SurfaceOut` has no AO lane.
    out.transmission = axiom_cloth_transmission(cloth_p, orm.r);
    return out;
}
"#;

/// The composed program: the ten concatenated layer constants, then the
/// hand-written `axiom_lighting_model` + `axiom_surface`.
///
/// A **function** and not the `&str` constant it would rather be, because
/// de-tiling is gated structurally (see this module's header) and one constant
/// cannot hold two shapes. Splicing the two `#ifdef OW_DETILE` chunks into a
/// shared body is what keeps the other ~200 lines singular; the alternative was
/// two copies of the whole composition, which is two places for the source's
/// order to drift apart.
pub(crate) fn material_surface_wgsl(detile: bool) -> String {
    let gate = usize::from(detile);
    [
        LAYERS.as_slice(),
        &[
            SURFACE_HEAD,
            ["", DETILE_SAMPLE][gate],
            SURFACE_MID,
            ["", DETILE_BLEND][gate],
            SURFACE_TAIL,
        ],
    ]
    .concat()
    .concat()
}

/// One material, ready to hand to the pipeline: the program text and the
/// parameter block it reads.
///
/// The two travel together because they are one decision. The de-tiling gate is
/// read from the same `MaterialParams` that is packed, so the emitted text and
/// the numbers behind it can never disagree about whether the block exists.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MaterialProgram {
    /// The WGSL to splice into `scene_shader`'s program-shaped hole.
    pub(crate) wgsl: String,
    /// `SurfaceParams`, as the slot map in `material_shader/params.rs` lays it out.
    pub(crate) params: [[u32; 4]; SLOT_COUNT],
}

/// Compose one material into its program and its parameter block.
///
/// The gate is the source's, exactly: `p.detile > 0 && p.uvMode !== 'triplanar'`
/// (`extendMaterial:851`), which is what [`detile_enabled`] ports.
pub(crate) fn material_program(material: &MaterialParams) -> MaterialProgram {
    let detile = detile_enabled(material.detile, material.uv_mode == UvMode::Triplanar);
    MaterialProgram {
        wgsl: material_surface_wgsl(detile),
        // Bit patterns, not floats: a program identity is compared and hashed,
        // and `f32` is neither `Eq` nor `Hash`. The packing is `params.rs`'s.
        params: material.pack().map(|slot| slot.map(f32::to_bits)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface_program::wgsl_template::{scene_shader, DEFAULT_DISPLACE_WGSL};

    /// Every WGSL entry point the composition is obliged to reach, and the layer
    /// that owns it.
    ///
    /// This table **is** the anti-regression: a composed shader that silently
    /// drops a layer compiles, renders, and looks plausible, so "the layer's
    /// function is defined somewhere in the text" proves nothing. Each name here
    /// must appear as a CALL inside the body of `axiom_surface`.
    const REACHED: [(&str, &str); 26] = [
        ("uv_mode", "axiom_uv_projection_pos"),
        ("uv_mode", "axiom_uv_projection_normal"),
        ("uv_mode", "axiom_uv_tile"),
        ("uv_mode", "axiom_uv_dominant_axis"),
        ("frames", "owTangentFrameScreen"),
        ("frames", "owAxisFrame"),
        ("frames", "owOrthonormalise"),
        ("pom", "axiom_pom_view_tangent"),
        ("pom", "axiom_pom_fade"),
        ("pom", "axiom_pom"),
        ("detail", "axiom_detail_fade"),
        ("detail", "axiom_detail_uv"),
        ("detail", "axiom_detail_blend_normal"),
        ("detail", "axiom_detail_micro"),
        ("detail", "axiom_detail_albedo"),
        ("detail", "axiom_detail_roughness"),
        ("detail", "axiom_detail_height"),
        ("macro_variation", "axiom_macro_variation"),
        ("patches", "axiom_patch_apply"),
        ("weathering", "ow_weather_stack"),
        ("masks", "axiom_masks_apply"),
        ("tint_wear", "axiom_mat_normal_strength"),
        ("tint_wear", "axiom_mat_finish"),
        ("cloth", "axiom_cloth_underside"),
        ("cloth", "axiom_cloth_fold"),
        ("cloth", "axiom_cloth_transmission"),
    ];

    /// The three entry points de-tiling adds, present only in the gated text.
    const REACHED_WHEN_DETILING: [&str; 3] = [
        "axiom_detile_second_sample",
        "axiom_detile_fold_detail_normal",
        "axiom_detile_height_blend",
    ];

    /// The slice of the program after `fn axiom_surface`, i.e. the composition's
    /// own body rather than the layer definitions above it.
    fn body(program: &str) -> &str {
        let at = program
            .find("fn axiom_surface(")
            .expect("the composition must define axiom_surface");
        &program[at..]
    }

    /// A material with every layer switched on, so nothing is inert.
    pub(super) fn loaded() -> MaterialParams {
        MaterialParams {
            parallax: 0.05,
            detile: 0.6,
            macro_relief: 0.5,
            macro_big: [1.0, 0.4, 0.028, 0.0],
            patch: [0.5, 2.6, 0.12, -0.08],
            cloth: [0.4, 0.7, 0.5, 0.0],
            ..MaterialParams::default()
        }
    }

    #[test]
    fn the_program_declares_both_halves_of_a_surface_program() {
        let program = material_surface_wgsl(false);
        assert!(program.contains("fn axiom_lighting_model() -> u32 {"));
        assert!(program
            .contains("fn axiom_surface(in: SurfaceIn, params: SurfaceParams) -> SurfaceOut"));
    }

    /// The layers land in dependency order, each exactly once.
    #[test]
    fn every_layer_is_concatenated_once_and_before_the_composition() {
        let program = material_surface_wgsl(true);
        let composition_at = program
            .find("fn axiom_surface(")
            .expect("the composition must exist");
        let positions: Vec<usize> = LAYERS
            .iter()
            .map(|layer| {
                let head = layer.lines().nth(1).expect("a layer has a second line");
                assert_eq!(
                    program.matches(layer).count(),
                    1,
                    "a layer concatenated twice is a duplicate WGSL definition: {head}"
                );
                program.find(layer).expect("every layer is spliced")
            })
            .collect();
        let ordered = positions.windows(2).all(|pair| pair[0] < pair[1]);
        assert!(ordered, "the layers must be concatenated in order: {positions:?}");
        let last = positions.last().copied().expect("ten layers");
        assert!(last < composition_at, "the composition comes after every layer");
    }

    /// The failure mode this whole file exists to prevent.
    #[test]
    fn every_layer_entry_point_is_called_from_the_composition() {
        let program = material_surface_wgsl(true);
        let composition = body(&program);
        REACHED.iter().for_each(|(layer, entry)| {
            let call = format!("{entry}(");
            assert!(
                composition.contains(&call),
                "the {layer} layer's `{entry}` is never called from axiom_surface"
            );
        });
        REACHED_WHEN_DETILING.iter().for_each(|entry| {
            let call = format!("{entry}(");
            assert!(
                composition.contains(&call),
                "the detile layer's `{entry}` is never called from axiom_surface"
            );
        });
    }

    /// De-tiling is a permutation, not a runtime zero: its measured cost is 1 ULP
    /// on 17.2% of operands, so an "off" program must not contain the block.
    #[test]
    fn de_tiling_is_absent_from_the_text_when_it_is_off() {
        let off = material_surface_wgsl(false);
        let on = material_surface_wgsl(true);
        REACHED_WHEN_DETILING.iter().for_each(|entry| {
            let call = format!("{entry}(");
            assert!(
                !body(&off).contains(&call),
                "`{entry}` must not be CALLED by the de-tiling-off program"
            );
            assert!(body(&on).contains(&call), "`{entry}` must be called when de-tiling");
        });
        assert!(off.len() < on.len(), "the gated chunks carry text");
        // The layer's DEFINITIONS stay either way — an unused WGSL function is
        // dead-stripped by the compiler, and dropping the definitions too would
        // make the two programs differ in more than the gate.
        assert!(off.contains("fn axiom_detile_second_sample("));
    }

    /// `CLOTH_WGSL` is spliced by `scene_shader` into every scene shader. A
    /// second copy here would be a duplicate definition and would not compile.
    ///
    /// Gated on the rendering feature because it names `scene_wgsl`, which only
    /// exists where there is a scene to render. The composition itself is not
    /// gated — `surface_program::cache` composes a runtime material on every
    /// target — so this gate belongs on the one test that reaches for the scene
    /// shader, not on the file.
    #[cfg(any(target_arch = "wasm32", feature = "offscreen"))]
    #[test]
    fn the_cloth_layer_is_called_but_never_redefined() {
        use crate::scene_wgsl::{SCENE_WGSL_PREFIX, SCENE_WGSL_SUFFIX};
        let program = material_surface_wgsl(true);
        ["fn axiom_cloth_light(", "fn axiom_cloth_underside(", "fn axiom_cloth_fold("]
            .iter()
            .for_each(|definition| {
                assert!(
                    !program.contains(definition),
                    "the composition must not redefine cloth: {definition}"
                );
            });
        let scene = scene_shader(
            SCENE_WGSL_PREFIX,
            DEFAULT_DISPLACE_WGSL,
            &program,
            SCENE_WGSL_SUFFIX,
        );
        assert_eq!(
            scene.matches("fn axiom_cloth_underside(").count(),
            1,
            "exactly one definition reaches the scene shader"
        );
    }

    /// All seven `SurfaceOut` channels are written. Six come from `tint_wear`'s
    /// `axiom_mat_channels`; `transmission` is the composition's own.
    #[test]
    fn all_seven_surface_out_channels_are_written() {
        let program = material_surface_wgsl(false);
        [
            "out.base_color",
            "out.roughness",
            "out.metallic",
            "out.normal",
            "out.emission",
            "out.opacity",
        ]
        .iter()
        .for_each(|channel| {
            assert!(program.contains(channel), "tint_wear must write {channel}");
        });
        assert!(
            body(&program).contains("out.transmission = axiom_cloth_transmission("),
            "the seventh channel is the composition's own"
        );
    }

    /// A slot that moves silently re-reads someone else's parameter, so the
    /// composition's reads are pinned against the map in `params.rs`.
    #[test]
    fn the_composition_reads_only_the_slots_the_map_defines() {
        let composition = material_surface_wgsl(true);
        let composition = body(&composition);
        // Slot 13 (`no_grad`) is deliberately unread — see the OW_NOGRAD note.
        let read: Vec<usize> = (0..SLOT_COUNT)
            .filter(|index| composition.contains(&format!("params.slots[{index}]")))
            .collect();
        assert_eq!(
            read,
            vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14, 15, 16, 17, 18],
            "the composition reads exactly the mapped slots, minus slot 13 (no_grad)"
        );
    }

    /// `owRoughP` is `( roughness[0], roughness[1], DETILE, roughness[2] )` and
    /// slot 11 is `( scale, offset, minimum, ao_strength )`. Two consumers read
    /// two different lanes of it, so the rebuild is load-bearing.
    #[test]
    fn the_rough_vector_is_rebuilt_because_it_is_not_slot_eleven() {
        let composition = material_surface_wgsl(true);
        assert!(body(&composition)
            .contains("let rough_p = vec4<f32>(p_rough.x, p_rough.y, p_misc.w, p_rough.z);"));
    }

    /// The view distance is bound in one findable place, so the one-line change
    /// is one line when `SurfaceIn` grows the lane.
    #[test]
    fn the_missing_view_distance_is_bound_where_a_reader_can_find_it() {
        let composition = material_surface_wgsl(false);
        assert!(body(&composition).contains(&format!("let ow_dist = {OW_DIST};")));
        // Both fades read it, and nothing else does.
        assert_eq!(body(&composition).matches("ow_dist").count(), 3);
    }

    /// The gate comes from the material, not from a second decision.
    #[test]
    fn the_program_takes_its_de_tiling_gate_from_the_material() {
        let default = material_program(&MaterialParams::default());
        assert!(!body(&default.wgsl).contains("axiom_detile_second_sample("));
        let detiled = material_program(&MaterialParams { detile: 0.6, ..MaterialParams::default() });
        assert!(body(&detiled.wgsl).contains("axiom_detile_second_sample("));
        // `p.detile > 0 && p.uvMode !== 'triplanar'` — triplanar disables it.
        let triplanar = material_program(&MaterialParams {
            detile: 0.6,
            uv_mode: UvMode::Triplanar,
            ..MaterialParams::default()
        });
        assert!(!body(&triplanar.wgsl).contains("axiom_detile_second_sample("));
    }

    /// The parameter block travels with the text, and it is `params.rs`'s.
    #[test]
    fn the_program_carries_the_packed_parameter_block() {
        let material = loaded();
        let program = material_program(&material);
        let packed = material.pack().map(|slot| slot.map(f32::to_bits));
        assert_eq!(program.params, packed);
        assert_eq!(program.params.len(), SLOT_COUNT);
        // `Clone`/`PartialEq`/`Debug` are the program-identity surface a cache
        // needs: two materials that pack the same numbers and emit the same text
        // are the same pipeline, and a mismatch has to be reportable.
        assert_eq!(program.clone(), program);
        assert_ne!(program, material_program(&MaterialParams::default()));
        let rendered = format!("{program:?}");
        assert!(rendered.starts_with("MaterialProgram"), "{rendered:.32}");
    }

    /// The composition names the group-0 bindings the scene shader declares, and
    /// no binding index of its own.
    #[test]
    fn the_composition_names_the_bound_maps_and_declares_no_binding() {
        let composition = material_surface_wgsl(true);
        let composition = body(&composition);
        ["albedo_tex", "albedo_sampler", "normal_tex", "normal_sampler",
         "material_orm_tex", "material_detail_tex", "material_macro_tex"]
            .iter()
            .for_each(|binding| {
                assert!(composition.contains(binding), "the composition must sample {binding}");
            });
        assert!(!composition.contains("@group("), "bindings belong to scene_wgsl.rs");
    }

    /// The vertex-colour lane does not exist yet, and the composition must be
    /// honest about that rather than passing something that looks authored.
    #[test]
    fn the_absent_vertex_colour_lane_is_an_explicit_zero() {
        let composition = material_surface_wgsl(false);
        assert!(body(&composition).contains("let vertex_color = vec3<f32>(0.0, 0.0, 0.0);"));
    }
}

/// The composed program on a **real adapter**: it compiles inside the real scene
/// shader, and it renders something that is not a constant.
///
/// A composition that type-checks as text proves nothing — every failure mode
/// this file guards against (a dropped layer, a mis-threaded slot, a
/// non-uniform implicit-LOD sample) survives a string assertion. The module
/// **asserts** an adapter was acquired rather than skipping: a GPU test that
/// silently passes when nothing ran is worse than no GPU test.
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod gpu {
    use super::tests::loaded;
    use super::{material_program, material_surface_wgsl};
    use crate::material_shader::cloth::CLOTH_WGSL;
    use crate::material_shader::params::{MaterialParams, UvMode};
    use crate::scene_wgsl::{SCENE_WGSL_PREFIX, SCENE_WGSL_SUFFIX};
    use crate::surface_program::wgsl_template::{
        scene_shader, DEFAULT_DISPLACE_WGSL, SURFACE_PRELUDE_WGSL,
    };

    /// Fragments across the target; also its width. Each evaluates the composed
    /// surface at a different world position, so a constant result is a failure.
    const SAMPLES: usize = 32;

    /// `wgpu`'s copy alignment.
    const ROW_ALIGN: u32 = 256;

    /// The side of each harness map. Small and non-constant is all that is
    /// needed: this test asks whether the composition *varies*, not what it
    /// equals — the per-layer parity tests own the numbers.
    const MAP_SIZE: u32 = 16;

    /// The harness's own bindings and entry points, wrapped around the composed
    /// program. It declares the same seven group-0 names `scene_wgsl.rs` does,
    /// so the composition is compiled verbatim against the names it was written
    /// for.
    const HARNESS_WGSL: &str = r#"
@group(0) @binding(0) var albedo_tex: texture_2d<f32>;
@group(0) @binding(1) var albedo_sampler: sampler;
@group(0) @binding(2) var normal_tex: texture_2d<f32>;
@group(0) @binding(3) var normal_sampler: sampler;
@group(0) @binding(4) var material_orm_tex: texture_2d<f32>;
@group(0) @binding(5) var material_detail_tex: texture_2d<f32>;
@group(0) @binding(6) var material_macro_tex: texture_2d<f32>;
@group(0) @binding(7) var<uniform> compose_params: SurfaceParams;
"#;

    /// The vertex stage and the two fragment stages, appended after the program.
    const HARNESS_ENTRY_WGSL: &str = r#"
@vertex
fn compose_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var pts = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0), vec2<f32>(-1.0, 1.0), vec2<f32>(3.0, 1.0)
    );
    return vec4<f32>(pts[index], 0.0, 1.0);
}

// One `SurfaceIn` per fragment column. The position walks a wall-like surface
// through world space so the world-anchored layers see genuinely different
// coordinates, and the uv walks with it so the screen-space derivatives the
// frame and the detail layer take are non-zero.
//
// The STRIDE is chosen, not arbitrary: the repair-patch lattice is 2.6 m and
// the runoff columns are ~0.65 m, so a walk confined to one cell samples one
// random rectangle and reports a working layer as a dead one. This spans about
// three patch cells horizontally and five vertically.
fn compose_input(x: f32) -> SurfaceIn {
    let world = vec3<f32>(x * 0.90 - 12.0, x * 0.42 + 0.15, x * -0.66 + 6.0);
    var si: SurfaceIn;
    si.object_pos = world;
    si.uv = vec2<f32>(x * 0.037, 1.0 - x * 0.029);
    si.object_normal = normalize(vec3<f32>(0.62, 0.21, -0.75));
    si.time = 0.0;
    si.albedo = vec4<f32>(0.78, 0.73, 0.66, 1.0);
    si.emissive = vec3<f32>(0.0, 0.0, 0.0);
    si.world_pos = world;
    si.world_normal = normalize(vec3<f32>(0.62 + x * 0.004, 0.21, -0.75));
    si.view_dir = normalize(vec3<f32>(0.31, 0.28, 0.91));
    // White — the identity. `var si: SurfaceIn` zero-initialises in WGSL, and a
    // zero here multiplies the albedo to nothing, which made every layer look
    // dead. Set explicitly rather than left to the default for that reason.
    si.vertex_color = vec4<f32>(1.0, 1.0, 1.0, 1.0);
    return si;
}

@fragment
fn compose_scalars_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let s = axiom_surface(compose_input(position.x), compose_params);
    return vec4<f32>(s.base_color.r, s.roughness, s.metallic, s.transmission);
}

@fragment
fn compose_vectors_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let s = axiom_surface(compose_input(position.x), compose_params);
    return vec4<f32>(s.normal, s.opacity);
}
"#;

    /// A real GPU, or a loud failure.
    struct Gpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
    }

    impl Gpu {
        fn acquire() -> Gpu {
            // The crate's ONE instance + adapter + device (see `crate::test_gpu`):
            // ~50 tests each opening their own is what crashes the driver.
            let gpu = crate::test_gpu::TestGpu::shared();
            let (device, queue) = (gpu.device.clone(), gpu.queue.clone());
            Gpu { device, queue }
        }

        /// Compile `source`, returning the validation message on failure.
        fn compile(&self, source: &str) -> Result<wgpu::ShaderModule, String> {
            // The error scope is the SHARED device's, so it is entered exclusively;
            // see `crate::test_gpu::validating`.
            let (module, failure) = crate::test_gpu::validating(&self.device, || {
                self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("axiom-material-compose-shader"),
                        source: wgpu::ShaderSource::Wgsl(source.into()),
                    })
            });
            failure
                .map_or(Ok(module), |error| Err(error.to_string()))
        }

        /// A deterministic, non-constant `MAP_SIZE` square `Rgba8Unorm` map.
        fn map(&self, seed: u32) -> wgpu::TextureView {
            let texels: Vec<u8> = (0..MAP_SIZE * MAP_SIZE)
                .flat_map(|index| {
                    // A cheap integer hash: the map only has to be non-constant
                    // and free of any pattern aligned with the sample walk.
                    let h = (index.wrapping_mul(2_654_435_761)).wrapping_add(seed.wrapping_mul(97));
                    [0_u32, 8, 16, 24].map(|shift| ((h >> shift) & 0xff) as u8)
                })
                .collect();
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-material-compose-map"),
                size: wgpu::Extent3d {
                    width: MAP_SIZE,
                    height: MAP_SIZE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &texels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(MAP_SIZE * 4),
                    rows_per_image: Some(MAP_SIZE),
                },
                wgpu::Extent3d {
                    width: MAP_SIZE,
                    height: MAP_SIZE,
                    depth_or_array_layers: 1,
                },
            );
            texture.create_view(&wgpu::TextureViewDescriptor::default())
        }

        /// Render one fragment entry point over a `SAMPLES x 1` `Rgba32Float`
        /// target and read every pixel's four lanes back.
        fn render(
            &self,
            module: &wgpu::ShaderModule,
            entry_point: &str,
            params: &[[u32; 4]; 32],
        ) -> Vec<[f32; 4]> {
            let maps = [0_u32, 1, 2, 3, 4].map(|seed| self.map(seed));
            let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("axiom-material-compose-sampler"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                address_mode_w: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            });
            let bytes: Vec<u8> = params
                .iter()
                .flatten()
                .flat_map(|word| word.to_le_bytes())
                .collect();
            let uniform = wgpu::util::DeviceExt::create_buffer_init(
                &self.device,
                &wgpu::util::BufferInitDescriptor {
                    label: Some("axiom-material-compose-params"),
                    contents: &bytes,
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            let texture_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            };
            let sampler_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            };
            let layout =
                self.device
                    .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: Some("axiom-material-compose-bgl"),
                        entries: &[
                            texture_entry(0),
                            sampler_entry(1),
                            texture_entry(2),
                            sampler_entry(3),
                            texture_entry(4),
                            texture_entry(5),
                            texture_entry(6),
                            wgpu::BindGroupLayoutEntry {
                                binding: 7,
                                visibility: wgpu::ShaderStages::FRAGMENT,
                                ty: wgpu::BindingType::Buffer {
                                    ty: wgpu::BufferBindingType::Uniform,
                                    has_dynamic_offset: false,
                                    min_binding_size: None,
                                },
                                count: None,
                            },
                        ],
                    });
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-material-compose-bg"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&maps[0]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&maps[1]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(&maps[2]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(&maps[3]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::TextureView(&maps[4]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            });
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("axiom-material-compose-pl"),
                        bind_group_layouts: &[&layout],
                        push_constant_ranges: &[],
                    });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("axiom-material-compose-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("compose_vs"),
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
            let target = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("axiom-material-compose-target"),
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
            let view = target.create_view(&wgpu::TextureViewDescriptor::default());
            let row_bytes = (SAMPLES as u32 * 16).div_ceil(ROW_ALIGN) * ROW_ALIGN;
            let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("axiom-material-compose-readback"),
                size: u64::from(row_bytes),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("axiom-material-compose-pass"),
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

    /// The harness module: the fixed prelude, the cloth layer (which
    /// `scene_shader` would otherwise supply), the bindings, the composed
    /// program, and the entry points.
    fn harness(detile: bool) -> String {
        [
            SURFACE_PRELUDE_WGSL,
            CLOTH_WGSL,
            HARNESS_WGSL,
            &material_surface_wgsl(detile),
            HARNESS_ENTRY_WGSL,
        ]
        .concat()
    }

    /// The spread of one lane across the sample walk.
    fn spread(rendered: &[[f32; 4]], lane: usize) -> f32 {
        let values: Vec<f32> = rendered.iter().map(|texel| texel[lane]).collect();
        let lo = values.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        hi - lo
    }

    /// Every lane is finite. A NaN reads as "varies" to a naive spread check and
    /// as a black hole on screen, so it is excluded explicitly.
    fn all_finite(rendered: &[[f32; 4]]) -> bool {
        rendered
            .iter()
            .flatten()
            .all(|value| value.is_finite())
    }

    /// The composition compiles **inside the real scene shader** — the same
    /// splice `scene_shader` performs for every draw — in both de-tiling
    /// permutations and for both uv modes that share the program.
    #[test]
    fn the_composed_program_compiles_inside_the_real_scene_shader() {
        let gpu = Gpu::acquire();
        [false, true].iter().for_each(|detile| {
            let scene = scene_shader(
                SCENE_WGSL_PREFIX,
                DEFAULT_DISPLACE_WGSL,
                &material_surface_wgsl(*detile),
                SCENE_WGSL_SUFFIX,
            );
            let outcome = gpu.compile(&scene);
            assert!(
                outcome.is_ok(),
                "the scene shader must compile with de-tiling = {detile}: {}",
                outcome.err().unwrap_or_default()
            );
        });
    }

    /// The harness module compiles too — which is what lets the render test
    /// below exist at all, and which pins the composition against the same
    /// binding names the scene shader declares.
    #[test]
    fn the_harness_module_compiles() {
        let gpu = Gpu::acquire();
        let outcome = gpu.compile(&harness(true));
        assert!(
            outcome.is_ok(),
            "the harness must compile: {}",
            outcome.err().unwrap_or_default()
        );
    }

    /// It renders, and what it renders is not a constant.
    ///
    /// Four scalar channels and the normal are checked separately: a composition
    /// that dropped every layer would still produce a *plausible* flat colour,
    /// so "the image varies" is checked per channel rather than in aggregate.
    #[test]
    fn the_composed_surface_renders_a_non_constant_image() {
        let gpu = Gpu::acquire();
        let program = material_program(&loaded());
        let module = gpu
            .compile(&harness(true))
            .expect("the harness compiles — see the_harness_module_compiles");
        let scalars = gpu.render(&module, "compose_scalars_fs", &program.params);
        assert!(all_finite(&scalars), "every scalar channel must be finite: {scalars:?}");
        [
            (0, "base_color.r"),
            (1, "roughness"),
            (2, "metallic"),
            (3, "transmission"),
        ]
        .iter()
        .for_each(|(lane, name)| {
            let width = spread(&scalars, *lane);
            assert!(
                width > 1e-4,
                "{name} is constant across the sample walk (spread {width:e}) — a layer \
                 that reached nothing renders exactly like this"
            );
        });
        let vectors = gpu.render(&module, "compose_vectors_fs", &program.params);
        assert!(all_finite(&vectors), "the normal and opacity must be finite: {vectors:?}");
        let normal_spread = [0, 1, 2].map(|lane| spread(&vectors, lane));
        let moved = normal_spread.iter().copied().fold(0.0_f32, f32::max);
        assert!(
            moved > 1e-4,
            "the shading normal is constant (per-axis spread {normal_spread:?}) — the \
             detail, macro-relief, weathering and cloth normal work all reached nothing"
        );
    }

    /// **Every layer moves the picture.**
    ///
    /// The text test upstream proves each layer's entry point is *called*; this
    /// proves the call *reaches the result*. A layer threaded with the wrong
    /// argument, or whose output is assigned to a local nothing downstream
    /// reads, passes the text test and fails this one — and that is precisely
    /// the failure a composed shader hides, because it still renders a plausible
    /// surface.
    ///
    /// Each row switches one layer's own parameter between off and on, from the
    /// same baseline, and demands the rendered channels change.
    #[test]
    fn switching_any_one_layer_on_changes_what_is_rendered() {
        let gpu = Gpu::acquire();
        let module = gpu
            .compile(&harness(false))
            .expect("the harness compiles — see the_harness_module_compiles");
        // The baseline: every optional layer off. The three that are never off —
        // the base fetch, `masks`' cavity term and `tint_wear`'s remap — have
        // their own rows below, driven by the parameters they DO gate on.
        let base = MaterialParams {
            parallax: 0.0,
            detail: [11.0, 0.0, 0.0, 16.0],
            macro_: [0.045, 0.0, 0.0, 0.0],
            macro_big: [1.0, 0.0, 0.03, 0.0],
            macro_relief: 0.0,
            patch: [0.0, 2.6, 0.12, -0.08],
            weather: [0.0, 0.0, 0.0, 0.0],
            cloth: [0.0, 1.0, 0.0, 0.0],
            ..MaterialParams::default()
        };
        let reference = gpu.render(&module, "compose_scalars_fs", &material_program(&base).params);
        let normals = gpu.render(&module, "compose_vectors_fs", &material_program(&base).params);
        assert!(all_finite(&reference) && all_finite(&normals));

        let cases: [(&str, MaterialParams); 8] = [
            // `pom` — the parallax march displaces the uv every later fetch uses.
            ("pom", MaterialParams { parallax: 0.09, ..base }),
            // `detail` — the micro albedo speckle and the cavity roughness.
            ("detail", MaterialParams { detail: [11.0, 0.55, 0.35, 16.0], ..base }),
            // `macro_variation` — the two bands and the hue term.
            ("macro_variation", MaterialParams { macro_: [0.045, 0.35, 0.1, 0.35], ..base }),
            // `macro_variation`'s second band, which is separately gated.
            ("macro_big", MaterialParams { macro_big: [1.0, 0.4, 0.028, 0.0], ..base }),
            // `patches` — coverage 0 is an exact identity, so any change is the
            // layer. Coverage is driven to 1.0: at the authored 0.1-0.6 the
            // layer is deliberately SPARSE (a patch is a rectangle inside a
            // 2.6 m cell that only some cells have), and a sparse layer moving
            // three of thirty-two samples is indistinguishable from a layer
            // wired to the wrong argument. Full coverage makes the question
            // "does it reach the result", which is what this test asks.
            ("patches", MaterialParams { patch: [1.0, 2.6, 0.25, -0.2], ..base }),
            // `weathering` — dust, rain, splash, wedge.
            ("weathering", MaterialParams { weather: [0.35, 0.3, 0.55, 0.0], ..base }),
            // `masks` — the cavity grime term, gated on weather.w.
            ("masks", MaterialParams { weather: [0.0, 0.0, 0.0, 0.8], ..base }),
            // `tint_wear` — the tint multiply and the roughness remap.
            ("tint_wear", MaterialParams { tint: 0xff_4020, roughness: [0.4, 0.3, 0.5], ..base }),
        ];
        cases.iter().for_each(|(layer, material)| {
            let rendered = gpu.render(
                &module,
                "compose_scalars_fs",
                &material_program(material).params,
            );
            assert!(all_finite(&rendered), "{layer} rendered a non-finite channel");
            let moved = reference
                .iter()
                .zip(rendered.iter())
                .filter(|(a, b)| {
                    [0_usize, 1, 2, 3]
                        .iter()
                        .any(|lane| (a[*lane] - b[*lane]).abs() > 1e-5)
                })
                .count();
            assert!(
                moved > SAMPLES / 4,
                "switching the {layer} layer on moved only {moved} of {SAMPLES} samples; \
                 its entry point is called but its result is not reaching SurfaceOut"
            );
        });

        // `cloth` writes the normal and the transmission channel, so it is
        // checked against the vector target as well as the scalar one.
        let clothed = MaterialParams { cloth: [0.4, 0.7, 0.5, 0.0], ..base };
        let params = material_program(&clothed).params;
        let scalars = gpu.render(&module, "compose_scalars_fs", &params);
        let vectors = gpu.render(&module, "compose_vectors_fs", &params);
        assert!(all_finite(&scalars) && all_finite(&vectors));
        let transmitting = scalars.iter().filter(|texel| texel[3] > 1e-5).count();
        assert!(
            transmitting > SAMPLES / 2,
            "only {transmitting} of {SAMPLES} samples transmit; the cloth layer's \
             seventh channel is not reaching SurfaceOut"
        );
        let tilted = normals
            .iter()
            .zip(vectors.iter())
            .filter(|(a, b)| {
                [0_usize, 1, 2].iter().any(|lane| (a[*lane] - b[*lane]).abs() > 1e-5)
            })
            .count();
        assert!(
            tilted > SAMPLES / 4,
            "the cloth fold tilted only {tilted} of {SAMPLES} shading normals"
        );
    }

    /// De-tiling is the twelfth layer and the one that is a **permutation**, so
    /// it cannot be swept with the others: switching it on is a different
    /// module, not a different uniform.
    #[test]
    fn the_de_tiling_permutation_changes_what_is_rendered() {
        let gpu = Gpu::acquire();
        let material = MaterialParams { detile: 0.7, ..loaded() };
        let params = material_program(&material).params;
        let off = gpu
            .compile(&harness(false))
            .expect("the harness compiles — see the_harness_module_compiles");
        let on = gpu
            .compile(&harness(true))
            .expect("the harness compiles — see the_harness_module_compiles");
        let a = gpu.render(&off, "compose_scalars_fs", &params);
        let b = gpu.render(&on, "compose_scalars_fs", &params);
        assert!(all_finite(&a) && all_finite(&b));
        let moved = a
            .iter()
            .zip(b.iter())
            .filter(|(x, y)| (x[0] - y[0]).abs() > 1e-5)
            .count();
        assert!(
            moved > SAMPLES / 4,
            "the de-tiling block moved only {moved} of {SAMPLES} samples; the second \
             sample and the height blend are not reaching the base sample"
        );
    }

    /// The three uv modes are not interchangeable, and the two that share one
    /// program must actually differ in the frame they build.
    #[test]
    fn the_mesh_uv_mode_renders_differently_from_the_planar_one() {
        let gpu = Gpu::acquire();
        let module = gpu
            .compile(&harness(false))
            .expect("the harness compiles — see the_harness_module_compiles");
        let planar = material_program(&MaterialParams::default());
        let mesh = material_program(&MaterialParams {
            uv_mode: UvMode::Mesh,
            ..MaterialParams::default()
        });
        let a = gpu.render(&module, "compose_scalars_fs", &planar.params);
        let b = gpu.render(&module, "compose_scalars_fs", &mesh.params);
        assert!(all_finite(&a) && all_finite(&b));
        let differences = a
            .iter()
            .zip(b.iter())
            .filter(|(x, y)| (x[0] - y[0]).abs() > 1e-5)
            .count();
        assert!(
            differences > SAMPLES / 4,
            "only {differences} of {SAMPLES} samples differ between mesh and planar uv; \
             the `select` on the frame is not reaching the sample"
        );
    }
}
