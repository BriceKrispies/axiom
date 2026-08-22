//! **The cloth layer**: fabric transmission, and the fabric surface override.
//!
//! Ported from Claude-of-Duty `src/materials/shader.js` — the `OW_CLOTH` block
//! of `MAIN_FRAGMENT`, the `OW_CLOTH_LIGHT` macro, the `CLOTH_LIGHT` chunk that
//! `OVERRIDES` splices after `<lights_fragment_end>`, and the `cloth` entry of
//! `DEFAULT_PARAMS`: `[ transmission 0..1, underside albedo multiplier, fold
//! amount, unused ]`.
//!
//! A sunlit canvas awning is not opaque. Fifteen to twenty-five per cent of the
//! beam comes through it, so from underneath you see a glowing sheet whose folds
//! read as density and whose edge is bright. The source's own comment says that
//! single term "is most of what makes cloth read as cloth rather than as painted
//! card", and it is the reason hanging laundry and market awnings have to be
//! rendered at all rather than painted.
//!
//! # Placement: a `SurfaceOut` CHANNEL, not a fourth `LightingModel`
//!
//! This is the only layer of the runtime material shader that touches
//! **lighting** rather than a surface channel, so it runs straight into the
//! design's flat statement that *a surface program supplies channel values,
//! never a way of being lit*. That statement is right and this layer does not
//! bend it. What follows is the argument, because the answer is not obvious and
//! the wrong answer is cheap to reach for.
//!
//! ## What the source's term actually is
//!
//! ```glsl
//! owTrans += owCl.color * ( owBackLit * ( 0.30 + 0.90 * owFwd * owFwd ) );  // per light
//! reflectedLight.directDiffuse += owTrans * diffuseColor.rgb
//!   * ( owClothP.x * clamp( owORM.r, 0.0, 1.0 ) );                          // once
//! ```
//!
//! Three separable parts:
//!
//! 1. a **fixed lobe shape** — back-lit wrap times a forward-scatter term,
//!    `0.30 + 0.90 f²`, with no authored numbers in it at all;
//! 2. a **per-light gather** over the directional lights, needing `L` and the
//!    light colour, which only the lighting stage has;
//! 3. a **per-surface scalar amount**, `owClothP.x * clamp(ao, 0, 1)`, which
//!    only the surface knows.
//!
//! Part 1 is a constant of the model, part 2 belongs to the lighting stage, and
//! part 3 is a channel. Written that way the term stops being an exception.
//!
//! ## Why NOT a fourth `LightingModel` variant
//!
//! [`crate::surface_program::emit_lighting`] emits the model as a **nullary
//! constant function** — `fn axiom_lighting_model() -> u32`. A variant therefore
//! carries no data, and part 3 above is data. A fourth variant could only
//! *select* a term whose magnitude still had to arrive by some other route, so
//! it would cost a new variant in the `axiom-surface` layer, a new
//! `RenderPipelineKind` mirror in `axiom-render`, **and** a value carrier. Two
//! mechanisms where one suffices.
//!
//! Worse, it would be the wrong *kind* of thing. `Unlit`/`Lambert`/
//! `LambertSpecular` is not a set of BSDFs; it is a monotone ladder of *how much
//! of the standard maths this surface takes*, lowered to `diffuse_gate` and
//! `specular_gate`. Transmission is orthogonal to that ladder — a cloth awning
//! still wants Lambert, and still wants its specular sheen. Making it a fourth
//! rung forces an author to choose between "lit like everything else" and
//! "transmits", and the moment a second orthogonal term appears (a second
//! scatter lobe, a rim term) the closed set is 2x3 = 6. That combinatorial
//! blow-up is precisely the variant multiplication `emit_lighting`'s header
//! refuses.
//!
//! ## Why NOT the `emission` channel
//!
//! This is the near miss, and it deserves naming because the *slot* is right.
//! `emission` is added after every light term, unattenuated by N·L, ambient or
//! shadow, and before fog — which is exactly where three.js's
//! `reflectedLight.directDiffuse +=` at `<lights_fragment_end>` lands relative
//! to `<fog_fragment>`. The accumulation semantics match to the letter.
//!
//! What emission cannot supply is the **light rig**. `axiom_surface` runs before
//! `fs` has looked at a single light, so an emission-encoded transmission must
//! either bake a sun direction into the parameter block — a second, staleable
//! copy of the frame's own light — or drop `owBackLit` and `owFwd` and emit a
//! constant fraction of albedo. The cost of dropping them is the whole feature:
//!
//! * the term's entire visual job is that the awning is **dark when the sun is
//!   in front of it and blazes when the sun is behind it**. A constant emission
//!   glows identically at every sun angle, which is the "painted card with a
//!   knife edge" failure the source wrote this term to avoid;
//! * `0.30 + 0.90 f²` makes looking along the beam **four times** brighter than
//!   looking across it (`1.20` against `0.30`). That gradient across a single
//!   awning is the fold read;
//! * the sum is over *every* directional light, so the moon lights fabric at
//!   night from its own direction. One baked direction cannot;
//! * and a constant cannot go dark when the surface turns away, so silhouettes
//!   invert.
//!
//! ## The conclusion
//!
//! `SurfaceOut` gains a **seventh channel, a scalar `transmission`**, and
//! `scene_wgsl`'s existing light loop grows one gated term. Not "cloth" — cloth
//! is one author of it; foliage, paper lanterns, curtains and skin are others,
//! and `0.30 + 0.90 f²` is a standard wrap-plus-forward-scatter approximation,
//! not a fabric special case.
//!
//! The precedent is already in that file and is exact in shape: **`roughness` is
//! a `SurfaceOut` scalar that multiplies a per-light term** (`gloss`), gated to
//! zero for a matte material, bit-identical when zero. Transmission is the same
//! shape — a scalar, per-light, zero-is-identity — and the lobe's constants sit
//! next to `SPECULAR_POWER`, which is the existing precedent for a fixed shape
//! constant living in the shader rather than in a parameter.
//!
//! The honest price, stated plainly: unlike a pipeline variant, the term's ALU
//! is executed by **every fragment of every draw in the engine**, gated to zero
//! — two dot products, a square and two multiply-adds per light. That is the
//! same bargain the three lighting models and the twelve capability bits already
//! strike, and it is the bargain the no-variant doctrine is made of; it is not
//! free, and the orchestrator should weigh it knowing the number.
//!
//! ## The change this file does NOT make
//!
//! `scene_wgsl.rs` and `surface_program/` are shared and live. The exact edit is
//! written out in `docs/work-manifests/shmup-port/notes/material-cloth.md` for
//! the orchestrator. Everything below is a free function over explicit
//! arguments, so it composes into that edit without needing it.
//!
//! # Transcription notes
//!
//! * **The `#define` is expanded in index order, then scaled once.**
//!   `OW_CLOTH_LIGHT( IDX )` is a braced block whose only outer effect is
//!   `owTrans += <pure expression of its own locals>`, so hoisting it to
//!   [`cloth_light`] is faithful — but only if the caller accumulates in light
//!   order and applies `diffuseColor.rgb * (clothP.x * clamp(ao,0,1))` **once,
//!   after the sum**. Folding that scale into the per-light term re-associates
//!   a multiply chain and is a different float. See [`transmitted`].
//! * **Every GLSL builtin is written out** — `clamp`, `mix`, `smoothstep`,
//!   `dot`, `normalize` — on both sides, the doctrine
//!   [`crate::surface_program::emit_ops`] already follows, because the builtins'
//!   factoring is unspecified and a parity run would then be measuring the
//!   builtins rather than this layer.
//! * **`normalize` is a division here, not a reciprocal-multiply.** GLSL defines
//!   it as `v / length(v)`; `emit_ops::normalize` deliberately spells the
//!   reciprocal instead because it mirrors the field evaluator, and this is not
//!   that. Turning a division into a reciprocal-multiply is the single defect
//!   class this port has found most often, so the literal form wins.
//! * **`0.90 * owFwd * owFwd` is left-associative** — `(0.90 * f) * f`, not
//!   `0.90 * (f * f)`. Likewise `(f0 - 0.5) * z * 0.9` and `tilt * z * 9.0`.
//! * **Dead computation is ported.** `tiltC` is built as a `vec3` whose `z` is
//!   already `0.0` and is then re-wrapped as `vec3(tiltC.x, tiltC.y, 0.0)`.
//!   Kept.
//! * **The defaults disable the layer, and that is a gate, not a preprocessor.**
//!   `cloth = [0, 1, 0, 0]` leaves `defines.OW_CLOTH` undefined in the source, so
//!   the whole block vanishes at compile time. Axiom has no preprocessor, so the
//!   define's own condition — `(cloth[0] > 0) || (cloth[1] < 1)` — is carried as
//!   data by [`enabled`] and applied with `select`, which takes the *value*.
//!   That matters: `orm.g += owDown * 0.05` is **not** an identity at the
//!   defaults, so a gate expressed as arithmetic would silently roughen every
//!   downward-facing fragment in the engine.

use axiom_math::{Vec2, Vec3, Vec4};

/// The WGSL for the cloth layer.
///
/// Entry points, all free functions over explicit arguments:
///
/// | function | mirror |
/// |---|---|
/// | `axiom_cloth_enabled(cloth) -> bool` | [`enabled`] |
/// | `axiom_cloth_fold_uv(world_pos) -> vec2<f32>` | [`fold_uv`] |
/// | `axiom_cloth_underside(albedo, roughness, world_normal, cloth) -> vec4<f32>` | [`underside`] |
/// | `axiom_cloth_fold(albedo, shade_normal, cloth, f0, fx, fy) -> AxiomClothFold` | [`fold`] |
/// | `axiom_cloth_light(normal, view_dir, light_dir, light_color) -> vec3<f32>` | [`cloth_light`] |
/// | `axiom_cloth_transmission(cloth, ao) -> f32` | [`transmission`] |
/// | `axiom_cloth_transmitted(trans_sum, diffuse_color, transmission) -> vec3<f32>` | [`transmitted`] |
///
/// The three macro-noise fetches the fold needs are **not** here: this layer
/// owns their coordinates (`axiom_cloth_fold_uv` plus the two offset constants)
/// and the arithmetic that consumes them, while the texture and sampler belong
/// to whoever binds `owMacroTex`. Keeping the fetch at the call site is also
/// what lets both sides of the parity run be pure maths.
pub(crate) const CLOTH_WGSL: &str = r#"
// --------------------------------------------------------------------------
// Cloth: fabric transmission and the fabric surface override.
// Ported from Claude-of-Duty `src/materials/shader.js` — OW_CLOTH,
// OW_CLOTH_LIGHT, CLOTH_LIGHT.
//
// `cloth` is the source's `owClothP` uniform:
//   x = transmission 0..1, y = underside albedo multiplier,
//   z = fold amount,       w = unused.
// `cloth = vec4(0.0, 1.0, 0.0, 0.0)` is the DEFAULT and disables the whole
// layer, exactly as the source's absent `#define OW_CLOTH` does.
//
// Every GLSL builtin below is written out rather than called: the builtins'
// factoring is unspecified, and the CPU reference in `cloth.rs` must be able to
// be the same expression, not a similar one.
// --------------------------------------------------------------------------

// The fold's finite-difference offsets, in the `axiom_cloth_fold_uv` space.
const AXIOM_CLOTH_FOLD_DX: vec2<f32> = vec2<f32>(0.05, 0.0);
const AXIOM_CLOTH_FOLD_DY: vec2<f32> = vec2<f32>(0.0, 0.05);

struct AxiomClothFold {
    albedo: vec3<f32>,
    normal: vec3<f32>,
};

// GLSL `clamp(x, lo, hi)` == `min(max(x, lo), hi)`.
fn axiom_cloth_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    return min(max(x, lo), hi);
}

// GLSL `smoothstep(edge0, edge1, x)`, written out. The source calls it with
// edge0 > edge1 (a DESCENDING ramp, 0.10 down to -0.70), which the builtin
// leaves indeterminate in both GLSL and WGSL — so it may not be the builtin.
fn axiom_cloth_smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = axiom_cloth_clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

// `defines.OW_CLOTH = ''` in the source:
//   (p.cloth?.[0] ?? 0) > 0 || (p.cloth?.[1] ?? 1) < 1
// The source's preprocessor condition, carried as data because Axiom has no
// preprocessor. `|` is WGSL's non-short-circuiting boolean or.
fn axiom_cloth_enabled(cloth: vec4<f32>) -> bool {
    return (cloth.x > 0.0) | (cloth.y < 1.0);
}

// The fold sample's world-anchored coordinate. 8-14 cm drape structure: the
// tile carries the weave and the camo blotches but nothing at the scale of a
// fold, so the shading normal is tilted by the gradient of the macro band and
// the cloth then catches the sun in ridges.
fn axiom_cloth_fold_uv(world_pos: vec3<f32>) -> vec2<f32> {
    return vec2<f32>(
        world_pos.x + world_pos.z * 0.63,
        world_pos.y * 0.7 + world_pos.z * 0.4
    ) * 3.4;
}

// The unconditional half of the OW_CLOTH block: the underside is never the same
// value as the top — it sits in its own shadow, collects soot off the street,
// and the only sun that reaches it comes through the weave.
//
// Returns `vec4(albedo, roughness)`. Disabled -> the inputs, unchanged, by
// `select` on the VALUE: `roughness + owDown * 0.05` is not an identity at the
// default parameters, so an arithmetic gate would not be one either.
fn axiom_cloth_underside(
    albedo: vec3<f32>,
    roughness: f32,
    world_normal: vec3<f32>,
    cloth: vec4<f32>,
) -> vec4<f32> {
    let ow_down = axiom_cloth_smoothstep(0.10, -0.70, world_normal.y);
    // GLSL `mix(1.0, cloth.y, ow_down)` == `x * (1 - a) + y * a`.
    let scale = 1.0 * (1.0 - ow_down) + cloth.y * ow_down;
    let lit_alb = albedo * scale;
    let lit_rough = axiom_cloth_clamp(roughness + ow_down * 0.05, 0.0, 1.0);
    let on = axiom_cloth_enabled(cloth);
    return vec4<f32>(select(albedo, lit_alb, on), select(roughness, lit_rough, on));
}

// The `if ( owClothP.z > 0.0 )` half: tilt the shading normal by the macro
// band's gradient, and darken the ridge crowns.
//
// `f0`/`fx`/`fy` are the BLUE channel of the macro texture at
// `axiom_cloth_fold_uv(world_pos)`, `+ AXIOM_CLOTH_FOLD_DX` and
// `+ AXIOM_CLOTH_FOLD_DY`. The caller keeps the source's `if` around the three
// fetches — that guard exists to SKIP THE FETCHES; this function is total, and
// returns its inputs unchanged when the layer or the fold is off.
fn axiom_cloth_fold(
    albedo: vec3<f32>,
    shade_normal: vec3<f32>,
    cloth: vec4<f32>,
    f0: f32,
    fx: f32,
    fy: f32,
) -> AxiomClothFold {
    // `tiltC.z` is already 0.0 and is re-wrapped as 0.0 one line later. Dead in
    // the source, kept here: dead computation in the source is still the source.
    let tilt_c = vec3<f32>(-(fx - f0), -(fy - f0), 0.0) * cloth.z * 9.0;
    let tilted = shade_normal + vec3<f32>(tilt_c.x, tilt_c.y, 0.0);
    // GLSL `normalize(v)` == `v / length(v)`, a DIVISION. Not a
    // reciprocal-multiply: see this layer's Rust header.
    let len = sqrt(tilted.x * tilted.x + tilted.y * tilted.y + tilted.z * tilted.z);
    let lit_n = tilted / len;
    let lit_alb = albedo * (1.0 - (f0 - 0.5) * cloth.z * 0.9);
    let on = axiom_cloth_enabled(cloth) & (cloth.z > 0.0);
    return AxiomClothFold(select(albedo, lit_alb, on), select(shade_normal, lit_n, on));
}

// ONE directional light's contribution to fabric transmission — the body of the
// source's `OW_CLOTH_LIGHT( IDX )` macro, which is a macro rather than a loop
// only because GLSL ES 1.00 will not index a uniform array of structs with a
// running variable.
//
// `light_dir` points TOWARD the light (three.js `IncidentLight.direction`), and
// `light_color` is the light colour with its intensity already folded in.
//
// back-lit = the beam landing on the face we are NOT looking at.
// forward  = the forward-scatter lobe, brightest looking nearly along the beam.
//
// NOT shadowed: the source calls `getDirectionalLightInfo` afresh rather than
// reusing the shadow-attenuated `directLight`, so a canopy in a cast shadow
// still transmits. Occlusion comes from the baked AO in
// `axiom_cloth_transmission` instead, which is what stops a canopy inside an
// arcade from glowing.
fn axiom_cloth_light(
    normal: vec3<f32>,
    view_dir: vec3<f32>,
    light_dir: vec3<f32>,
    light_color: vec3<f32>,
) -> vec3<f32> {
    let n_dot_l = normal.x * light_dir.x + normal.y * light_dir.y + normal.z * light_dir.z;
    let back_lit = max(0.0, -n_dot_l);
    let away = -light_dir;
    let v_dot_a = view_dir.x * away.x + view_dir.y * away.y + view_dir.z * away.z;
    let forward = max(0.0, v_dot_a);
    return light_color * (back_lit * (0.30 + 0.90 * forward * forward));
}

// The per-surface transmission amount: `owClothP.x * clamp( owORM.r, 0, 1 )`.
// This is the scalar the SurfaceOut `transmission` channel should carry — see
// the placement argument in `cloth.rs`. It needs no enable gate: the layer can
// only be on via `cloth.y < 1` with `cloth.x == 0`, and `0.0 * anything finite`
// is an exact zero, which `axiom_cloth_transmitted` turns into an exact
// identity.
fn axiom_cloth_transmission(cloth: vec4<f32>, ao: f32) -> f32 {
    return cloth.x * axiom_cloth_clamp(ao, 0.0, 1.0);
}

// The `CLOTH_LIGHT` chunk's final line. `trans_sum` is the per-light sum
// accumulated IN LIGHT ORDER; the scale is applied ONCE, here, after the sum.
// Folding it into the per-light term re-associates the multiply chain.
fn axiom_cloth_transmitted(
    trans_sum: vec3<f32>,
    diffuse_color: vec3<f32>,
    transmission: f32,
) -> vec3<f32> {
    return trans_sum * diffuse_color * transmission;
}
"#;

/// The fold's finite-difference offset along u, in [`fold_uv`] space.
///
/// Named rather than left as a literal at the two call sites, because the
/// gradient's step size and the `* 9.0` tilt gain are a matched pair: change one
/// without the other and the drape gets stronger or vanishes.
pub(crate) const FOLD_DX: Vec2 = Vec2 { x: 0.05, y: 0.0 };

/// The fold's finite-difference offset along v. See [`FOLD_DX`].
pub(crate) const FOLD_DY: Vec2 = Vec2 { x: 0.0, y: 0.05 };

/// GLSL `clamp(x, lo, hi)`, which is `min(max(x, lo), hi)`.
///
/// Not `f32::clamp`: that is a different function (it is specified by three
/// comparisons, propagates NaN where GLSL's does not, and carries a `lo <= hi`
/// panic whose region no test can honestly reach).
fn glsl_clamp(x: f32, lo: f32, hi: f32) -> f32 {
    f32::min(f32::max(x, lo), hi)
}

/// GLSL `smoothstep(edge0, edge1, x)`, written out.
///
/// The source calls it **descending** (`edge0 = 0.10`, `edge1 = -0.70`), which
/// both GLSL and WGSL leave indeterminate for the builtin, so neither side may
/// call the builtin.
fn glsl_smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = glsl_clamp((x - edge0) / (edge1 - edge0), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// GLSL `dot(a, b)` for a `Vec3`, left-associated.
///
/// [`Vec3::dot`] is already `x*x + y*y + z*z` in that order, so this is a name
/// for the guarantee rather than a second implementation of it — the WGSL side
/// spells the same sum out because the `dot` builtin's summation order is
/// unspecified.
fn glsl_dot(a: Vec3, b: Vec3) -> f32 {
    a.dot(b)
}

/// GLSL unary `-v` on a `Vec3`: exact, including the sign of a zero lane.
fn glsl_negate(v: Vec3) -> Vec3 {
    v.mul_scalar(-1.0)
}

/// Whether the cloth layer is on at all — the source's `defines.OW_CLOTH`
/// condition, `(cloth[0] > 0) || (cloth[1] < 1)`, carried as data.
///
/// The default `[0, 1, 0, 0]` is `false`, and every function in this layer
/// returns its input unchanged when it is.
pub(crate) fn enabled(cloth: Vec4) -> bool {
    (cloth.x > 0.0) | (cloth.y < 1.0)
}

/// The world-anchored coordinate the three macro-noise fold samples are taken
/// at. The other two are this plus [`FOLD_DX`] and [`FOLD_DY`].
pub(crate) fn fold_uv(world_pos: Vec3) -> Vec2 {
    Vec2::new(
        world_pos.x + world_pos.z * 0.63,
        world_pos.y * 0.7 + world_pos.z * 0.4,
    )
    .mul_scalar(3.4)
}

/// The unconditional half of the `OW_CLOTH` block: the underside albedo
/// multiplier and the matching roughness lift.
///
/// Returns `(albedo, roughness)`, or the inputs unchanged when [`enabled`] is
/// false.
pub(crate) fn underside(
    albedo: Vec3,
    roughness: f32,
    world_normal: Vec3,
    cloth: Vec4,
) -> (Vec3, f32) {
    let ow_down = glsl_smoothstep(0.10, -0.70, world_normal.y);
    // GLSL `mix(1.0, cloth.y, ow_down)` is `x * (1 - a) + y * a`.
    let scale = 1.0 * (1.0 - ow_down) + cloth.y * ow_down;
    let lit_alb = albedo.mul_scalar(scale);
    let lit_rough = glsl_clamp(roughness + ow_down * 0.05, 0.0, 1.0);
    let on = usize::from(enabled(cloth));
    ([albedo, lit_alb][on], [roughness, lit_rough][on])
}

/// The `if ( owClothP.z > 0.0 )` half: the drape tilt and the ridge darkening.
///
/// `f0`/`fx`/`fy` are the blue channel of the macro texture at [`fold_uv`],
/// `+ FOLD_DX` and `+ FOLD_DY`. Returns `(albedo, shade_normal)`, or the inputs
/// unchanged when the layer or the fold is off.
pub(crate) fn fold(
    albedo: Vec3,
    shade_normal: Vec3,
    cloth: Vec4,
    f0: f32,
    fx: f32,
    fy: f32,
) -> (Vec3, Vec3) {
    // `tilt_c.z` is 0.0 and is re-wrapped as 0.0 below: dead in the source, and
    // dead computation in the source is still part of the source.
    let tilt_c = Vec3::new(-(fx - f0), -(fy - f0), 0.0)
        .mul_scalar(cloth.z)
        .mul_scalar(9.0);
    let tilted = shade_normal.add(Vec3::new(tilt_c.x, tilt_c.y, 0.0));
    // GLSL `normalize(v)` is `v / length(v)` — a division, spelled out rather
    // than routed through `Vec3::normalize`, whose zero-length arm is a
    // `MathResult` this expression does not have.
    let len = glsl_dot(tilted, tilted).sqrt();
    let lit_n = Vec3::new(tilted.x / len, tilted.y / len, tilted.z / len);
    let lit_alb = albedo.mul_scalar(1.0 - (f0 - 0.5) * cloth.z * 0.9);
    let on = usize::from(enabled(cloth) & (cloth.z > 0.0));
    ([albedo, lit_alb][on], [shade_normal, lit_n][on])
}

/// One directional light's contribution to fabric transmission — the body of
/// `OW_CLOTH_LIGHT( IDX )`.
///
/// `light_dir` points **toward** the light; `light_color` has intensity folded
/// in. A light that is not present is representable as a zero colour, which
/// contributes an exact `(0, 0, 0)` — so a caller with fewer than three
/// directional lights reproduces the source's shorter macro expansion bit for
/// bit rather than approximately.
pub(crate) fn cloth_light(
    normal: Vec3,
    view_dir: Vec3,
    light_dir: Vec3,
    light_color: Vec3,
) -> Vec3 {
    let back_lit = f32::max(0.0, -glsl_dot(normal, light_dir));
    let away = glsl_negate(light_dir);
    let forward = f32::max(0.0, glsl_dot(view_dir, away));
    light_color.mul_scalar(back_lit * (0.30 + 0.90 * forward * forward))
}

/// The per-surface transmission amount: `cloth.x * clamp(ao, 0, 1)`.
///
/// This is the scalar a `SurfaceOut.transmission` channel would carry. See this
/// module's placement argument for why that, and not a fourth `LightingModel`.
pub(crate) fn transmission(cloth: Vec4, ao: f32) -> f32 {
    cloth.x * glsl_clamp(ao, 0.0, 1.0)
}

/// The `CLOTH_LIGHT` chunk's final line: the accumulated per-light sum, scaled
/// **once**, by the albedo and the transmission amount.
///
/// `trans_sum` must have been accumulated in light order, and the scale must not
/// be folded into the per-light term: both are re-associations, and float
/// arithmetic is not associative.
pub(crate) fn transmitted(trans_sum: Vec3, diffuse_color: Vec3, transmission: f32) -> Vec3 {
    Vec3::new(
        trans_sum.x * diffuse_color.x,
        trans_sum.y * diffuse_color.y,
        trans_sum.z * diffuse_color.z,
    )
    .mul_scalar(transmission)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The source's `DEFAULT_PARAMS.cloth`: `[ transmission, underside
    /// multiplier, fold amount, unused ]`. Transmission 0 and multiplier 1
    /// disable the whole cloth layer.
    const DEFAULT_CLOTH: Vec4 = Vec4 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
        w: 0.0,
    };

    fn cloth(x: f32, y: f32, z: f32) -> Vec4 {
        Vec4::new(x, y, z, 0.0)
    }

    #[test]
    fn the_default_parameters_disable_the_layer() {
        assert!(
            !enabled(DEFAULT_CLOTH),
            "DEFAULT_PARAMS.cloth must leave OW_CLOTH undefined"
        );
    }

    #[test]
    fn either_half_of_the_define_condition_turns_the_layer_on() {
        // `(cloth[0] > 0) || (cloth[1] < 1)`, both arms and the boundary.
        assert!(enabled(cloth(0.2, 1.0, 0.0)), "transmission alone enables");
        assert!(enabled(cloth(0.0, 0.4, 0.0)), "an underside tint alone enables");
        assert!(enabled(cloth(0.2, 0.4, 0.0)), "both together enable");
        assert!(!enabled(cloth(0.0, 1.0, 0.9)), "a fold alone does NOT enable");
        assert!(!enabled(cloth(0.0, 1.4, 0.0)), "a multiplier above 1 does not");
    }

    /// The trap this layer exists to avoid: `orm.g + owDown * 0.05` is not an
    /// identity, so a disabled layer must be gated on the VALUE.
    #[test]
    fn a_disabled_layer_is_bit_identical_to_no_cloth_at_all() {
        let albedo = Vec3::new(0.31, 0.62, 0.17);
        let normal = Vec3::new(0.2, -0.9, 0.3);
        // Every world normal from straight up to straight down, so the two ends
        // of the descending smoothstep and its whole curve are covered.
        (0..41).for_each(|step| {
            let y = step as f32 * 0.05 - 1.0;
            let (alb, rough) = underside(albedo, 0.42, Vec3::new(0.0, y, 0.0), DEFAULT_CLOTH);
            assert_eq!(alb, albedo, "albedo moved at y = {y}");
            assert_eq!(rough, 0.42, "roughness moved at y = {y}");
            let (falb, fnorm) = fold(albedo, normal, DEFAULT_CLOTH, 0.7, 0.2, 0.9);
            assert_eq!(falb, albedo, "fold albedo moved at y = {y}");
            assert_eq!(fnorm, normal, "fold normal moved at y = {y}");
        });
        assert_eq!(transmission(DEFAULT_CLOTH, 0.8), 0.0);
        assert_eq!(
            transmitted(Vec3::new(3.0, 4.0, 5.0), albedo, 0.0),
            Vec3::ZERO,
            "a zero transmission contributes an exact zero"
        );
    }

    /// Even a layer that is ON contributes exactly nothing to lighting while its
    /// transmission is zero — the `cloth[1] < 1` arm of the define.
    #[test]
    fn an_underside_only_cloth_transmits_exactly_nothing() {
        let on = cloth(0.0, 0.55, 0.0);
        assert!(enabled(on));
        assert_eq!(transmission(on, 1.0), 0.0);
        let lit = Vec3::new(0.4, 0.5, 0.6);
        assert_eq!(
            lit.add(transmitted(Vec3::new(9.0, 8.0, 7.0), lit, transmission(on, 1.0))),
            lit
        );
    }

    #[test]
    fn the_underside_darkens_downward_faces_and_lifts_their_roughness() {
        let albedo = Vec3::ONE;
        let on = cloth(0.2, 0.5, 0.0);
        // Straight up: past the ramp's first edge, so owDown is 0.
        let (up_alb, up_rough) = underside(albedo, 0.4, Vec3::UNIT_Y, on);
        assert_eq!(up_alb, Vec3::ONE);
        assert_eq!(up_rough, 0.4);
        // Straight down: past the second edge, so owDown is 1 and the multiplier
        // applies in full.
        let (down_alb, down_rough) = underside(albedo, 0.4, Vec3::new(0.0, -1.0, 0.0), on);
        assert_eq!(down_alb, Vec3::new(0.5, 0.5, 0.5));
        assert!((down_rough - 0.45).abs() < 1.0e-7, "{down_rough}");
        // Mid-ramp: strictly between, and monotone.
        let (mid_alb, mid_rough) = underside(albedo, 0.4, Vec3::new(0.0, -0.3, 0.0), on);
        assert!(mid_alb.x > 0.5, "{}", mid_alb.x);
        assert!(mid_alb.x < 1.0, "{}", mid_alb.x);
        assert!(mid_rough > 0.4, "{mid_rough}");
        assert!(mid_rough < 0.45, "{mid_rough}");
    }

    #[test]
    fn the_roughness_lift_is_clamped_into_the_unit_range() {
        let (_, rough) = underside(
            Vec3::ONE,
            0.99,
            Vec3::new(0.0, -1.0, 0.0),
            cloth(0.2, 0.5, 0.0),
        );
        assert_eq!(rough, 1.0);
    }

    #[test]
    fn the_fold_tilts_the_normal_toward_the_falling_gradient_and_renormalises() {
        let on = cloth(0.2, 1.0, 0.5);
        let flat = Vec3::UNIT_Z;
        let (alb, n) = fold(Vec3::ONE, flat, on, 0.5, 0.6, 0.4);
        // fx > f0 pushes x negative; fy < f0 pushes y positive.
        assert!(n.x < 0.0, "{n:?}");
        assert!(n.y > 0.0, "{n:?}");
        let len = glsl_dot(n, n).sqrt();
        assert!((len - 1.0).abs() < 1.0e-6, "not unit length: {len}");
        // f0 == 0.5 is the ridge midpoint, so the albedo is untouched there.
        assert_eq!(alb, Vec3::ONE);
    }

    #[test]
    fn the_fold_darkens_ridge_crowns_and_brightens_troughs() {
        let on = cloth(0.2, 1.0, 1.0);
        let (crown, _) = fold(Vec3::ONE, Vec3::UNIT_Z, on, 1.0, 1.0, 1.0);
        let (trough, _) = fold(Vec3::ONE, Vec3::UNIT_Z, on, 0.0, 0.0, 0.0);
        assert!(crown.x < 1.0, "{crown:?}");
        assert!(trough.x > 1.0, "{trough:?}");
    }

    #[test]
    fn a_zero_fold_amount_leaves_the_surface_untouched_even_when_enabled() {
        let on = cloth(0.6, 0.5, 0.0);
        assert!(enabled(on));
        let n = Vec3::new(0.1, 0.2, 0.97);
        let (alb, out) = fold(Vec3::ONE, n, on, 0.9, 0.1, 0.3);
        assert_eq!(alb, Vec3::ONE);
        assert_eq!(out, n, "a normalize must not run when the fold is off");
    }

    #[test]
    fn the_fold_uv_is_world_anchored_and_matches_the_source_expression() {
        let uv = fold_uv(Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(uv.x, (2.0 + 4.0 * 0.63) * 3.4);
        assert_eq!(uv.y, (3.0 * 0.7 + 4.0 * 0.4) * 3.4);
        assert_eq!(FOLD_DX, Vec2::new(0.05, 0.0));
        assert_eq!(FOLD_DY, Vec2::new(0.0, 0.05));
    }

    #[test]
    fn a_light_in_front_of_the_cloth_transmits_nothing() {
        // N and L on the same side: back-lit is clamped to zero.
        let out = cloth_light(Vec3::UNIT_Z, Vec3::UNIT_Z, Vec3::UNIT_Z, Vec3::ONE);
        assert_eq!(out, Vec3::ZERO);
    }

    #[test]
    fn a_light_behind_the_cloth_transmits_and_peaks_along_the_beam() {
        // Cloth faces +Z, the light is beyond it at -Z, the eye is at +Z.
        let n = Vec3::UNIT_Z;
        let l = Vec3::new(0.0, 0.0, -1.0);
        let along = cloth_light(n, Vec3::UNIT_Z, l, Vec3::ONE);
        // Looking along the beam: back_lit = 1, forward = 1 -> 0.30 + 0.90.
        assert!((along.x - 1.2).abs() < 1.0e-7, "{along:?}");
        // Looking across the beam: forward = 0 -> the 0.30 floor alone.
        let across = cloth_light(n, Vec3::UNIT_X, l, Vec3::ONE);
        assert!((across.x - 0.3).abs() < 1.0e-7, "{across:?}");
        // The lobe is the whole point: four times brighter along the beam.
        assert!(along.x > across.x * 3.9, "{} vs {}", along.x, across.x);
    }

    #[test]
    fn an_absent_light_is_a_zero_colour_and_sums_to_an_exact_identity() {
        let present = cloth_light(
            Vec3::UNIT_Z,
            Vec3::UNIT_Z,
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(1.0, 0.9, 0.7),
        );
        let absent = cloth_light(
            Vec3::UNIT_Z,
            Vec3::UNIT_Z,
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::ZERO,
        );
        assert_eq!(absent, Vec3::ZERO);
        assert_eq!(present.add(absent), present);
    }

    #[test]
    fn the_transmission_amount_is_occluded_by_ambient_occlusion() {
        let on = cloth(0.8, 1.0, 0.0);
        assert_eq!(transmission(on, 0.0), 0.0);
        assert!((transmission(on, 0.5) - 0.4).abs() < 1.0e-7);
        assert!((transmission(on, 1.0) - 0.8).abs() < 1.0e-7);
        // The clamp is GLSL's `min(max(..))`, so an out-of-range AO saturates.
        assert!((transmission(on, 3.0) - 0.8).abs() < 1.0e-7);
        assert_eq!(transmission(on, -2.0), 0.0);
    }

    /// `transmitted` must be `(trans_sum * diffuse_color) * transmission`, the
    /// source's left-associated chain — not the same three factors regrouped.
    #[test]
    fn the_final_scale_is_applied_once_after_the_sum() {
        // A triple for which the two groupings of one vector-by-scalar chain are
        // genuinely different f32s, found by search rather than assumed.
        let sum = Vec3::new(0.013_929_999, 1.0, 1.0);
        let albedo = Vec3::new(0.020_11, 0.5, 0.5);
        let scale = 0.026_330_002_f32;
        let source = transmitted(sum, albedo, scale);
        // The mis-transcription the trap names: a vector-by-scalar chain folded
        // the other way, `sum * (albedo * scale)`.
        let regrouped = sum.x * (albedo.x * scale);
        assert_ne!(
            source.x, regrouped,
            "this case must actually distinguish the two associations"
        );
        assert_eq!(source.x, (sum.x * albedo.x) * scale);
        // The other named regrouping: scaling per light instead of once, after
        // the sum.
        let (a, b, c, s) = (0.012_62_f32, 0.017_74, 0.023_22, 0.027_339_999);
        assert_ne!(((a + b) + c) * s, ((a * s) + (b * s)) + (c * s));
    }

    /// The `#define` is expanded per light **in index order**, and the caller's
    /// accumulator must respect that. This is the fact that makes it a contract
    /// rather than a preference.
    #[test]
    fn accumulating_the_lights_out_of_order_is_a_different_float() {
        // Three contributions whose forward and reverse sums differ: the first
        // swamps the other two individually but not together.
        let terms = [1.0_f32, 5.0e-8, 5.0e-8];
        let forward = terms.iter().fold(0.0_f32, |acc, t| acc + t);
        let reverse = terms.iter().rev().fold(0.0_f32, |acc, t| acc + t);
        assert_ne!(
            forward, reverse,
            "light order must be load-bearing, or this port cannot claim it preserved it"
        );
        // And `cloth_light` really can span that range within one frame: a light
        // square-on to the eye against one at a grazing angle with a dim colour.
        let n = Vec3::UNIT_Z;
        let behind = Vec3::new(0.0, 0.0, -1.0);
        let strong = cloth_light(n, Vec3::UNIT_Z, behind, Vec3::ONE);
        let faint = cloth_light(n, Vec3::UNIT_X, behind, Vec3::ONE.mul_scalar(1.0e-7));
        assert!(strong.x > faint.x * 1.0e6, "{} vs {}", strong.x, faint.x);
    }

    /// The descending smoothstep the source calls with `edge0 > edge1`, which
    /// the builtin leaves indeterminate — hence the hand-written form.
    #[test]
    fn the_descending_smoothstep_saturates_at_both_ends() {
        assert_eq!(glsl_smoothstep(0.10, -0.70, 0.5), 0.0);
        assert_eq!(glsl_smoothstep(0.10, -0.70, 0.10), 0.0);
        assert_eq!(glsl_smoothstep(0.10, -0.70, -0.70), 1.0);
        assert_eq!(glsl_smoothstep(0.10, -0.70, -3.0), 1.0);
        let mid = glsl_smoothstep(0.10, -0.70, -0.30);
        assert!((mid - 0.5).abs() < 1.0e-6, "{mid}");
    }

    #[test]
    fn glsl_negate_flips_every_lane_including_a_zero() {
        let out = glsl_negate(Vec3::new(1.5, 0.0, -2.0));
        assert_eq!(out, Vec3::new(-1.5, 0.0, 2.0));
        assert!(out.y.is_sign_negative(), "GLSL's -0.0 is a negative zero");
    }

    #[test]
    fn glsl_clamp_is_min_of_max() {
        assert_eq!(glsl_clamp(-1.0, 0.0, 1.0), 0.0);
        assert_eq!(glsl_clamp(2.0, 0.0, 1.0), 1.0);
        assert_eq!(glsl_clamp(0.25, 0.0, 1.0), 0.25);
    }

    /// The WGSL and the Rust are two transcriptions of one GLSL text, so the
    /// names they share have to actually be shared — a renamed entry point is a
    /// silently un-composed layer.
    #[test]
    fn the_wgsl_declares_every_entry_point_this_layer_promises() {
        [
            "fn axiom_cloth_enabled(",
            "fn axiom_cloth_fold_uv(",
            "fn axiom_cloth_underside(",
            "fn axiom_cloth_fold(",
            "fn axiom_cloth_light(",
            "fn axiom_cloth_transmission(",
            "fn axiom_cloth_transmitted(",
            "const AXIOM_CLOTH_FOLD_DX",
            "const AXIOM_CLOTH_FOLD_DY",
        ]
        .iter()
        .for_each(|needle| {
            assert!(CLOTH_WGSL.contains(needle), "CLOTH_WGSL is missing {needle}");
        });
    }

    /// The builtins whose factoring is unspecified must not be called by either
    /// side, or a parity run measures the builtin instead of this layer.
    ///
    /// A *bare* call, so `axiom_cloth_smoothstep(` — this layer's own written-out
    /// replacement — is not mistaken for the builtin it exists to avoid.
    #[test]
    fn the_wgsl_calls_no_unspecified_builtin() {
        // `//` comments stripped first, the way `xtask`'s source scans do: the
        // comments name every one of these builtins, on purpose, to say which
        // written-out form replaces it.
        let code: String = CLOTH_WGSL
            .lines()
            .map(|line| line.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        let bytes = code.as_bytes();
        ["smoothstep(", "mix(", "dot(", "normalize(", "clamp("]
            .iter()
            .for_each(|needle| {
                let bare = code.match_indices(needle).any(|(at, _)| {
                    at.checked_sub(1)
                        .map(|prev| bytes[prev])
                        .is_none_or(|b| !(b.is_ascii_alphanumeric() | (b == b'_')))
                });
                assert!(!bare, "CLOTH_WGSL calls the {needle} builtin");
            });
        // ... and the guard itself has teeth: the written-out names are present
        // and are exactly what the scan must tolerate.
        assert!(CLOTH_WGSL.contains("axiom_cloth_smoothstep("));
        assert!(CLOTH_WGSL.contains("axiom_cloth_clamp("));
    }
}

// The CPU reference above is the semantic definition; this holds it up against a
// real GPU running `CLOTH_WGSL`. Compiled only with `--features offscreen`, and
// it ASSERTS an adapter was acquired rather than skipping — a parity test that
// passes when nothing ran proves nothing. The pattern, including the harness
// shape and the `Rgba32Float` readback, is `crate::surface_program::parity`'s;
// this module cannot reuse that code because it is `pub(super)` to a sibling
// module and this layer may not edit it.
#[cfg(all(test, feature = "offscreen", not(target_arch = "wasm32")))]
mod parity {
    use super::*;

    /// How many contexts one run compares, and the target's width.
    const SAMPLES: usize = 24;

    /// Sixteen-byte lanes per context in the uniform block.
    const LANES: usize = 9;

    /// `copy_texture_to_buffer` requires each row aligned to this many bytes.
    const ROW_ALIGN: u32 = 256;

    /// The agreement budget, **relative above unit magnitude**: a deviation is
    /// compared against `TOLERANCE * max(|expected|, 1)`.
    ///
    /// Relative and not purely absolute, because one of the lanes this layer
    /// produces is a world-anchored *coordinate* (`axiom_cloth_fold_uv`), whose
    /// magnitude is unbounded — a metre of world moves it by 3.4, and an
    /// absolute budget on it is a budget that gets looser the closer the camera
    /// is to the origin. The `max(_, 1)` floor keeps it absolute for the
    /// channel-valued lanes, which live in `0..=1`.
    ///
    /// Where the deviation comes from: every GLSL builtin is written out
    /// identically on both sides, and WGSL requires `+`, `-` and `*` to be
    /// correctly rounded, so those agree bit for bit. What is left is what the
    /// hardware is *allowed* to do — contract `a * b + c` into a single-rounding
    /// `fma`, and evaluate `/` to 2.5 ULP where Rust's is correctly rounded.
    /// Both sides compute in `f32`, so no storage-width difference is folded in.
    ///
    /// **Measured**, not fitted: `MEASURED_WORST` below is what the run reports
    /// on this machine, and it is one ULP — a single contracted multiply-add in
    /// `world_pos.x + world_pos.z * 0.63`. The budget is ~8 ULP, which is the
    /// smallest round number that leaves room for a backend that contracts two
    /// of them; a run that gets within 10x of it should be investigated rather
    /// than have this number raised.
    const TOLERANCE: f32 = 1.0e-6;

    /// The worst scaled deviation this layer has actually been measured at, on
    /// a Vulkan adapter. Printed by the parity run so drift is visible.
    const MEASURED_WORST: f32 = 1.4e-7;

    /// One context: everything the seven entry points read.
    struct Context {
        shade_normal: Vec3,
        view_dir: Vec3,
        light_dir: Vec3,
        light_color: Vec3,
        world_pos: Vec3,
        world_normal: Vec3,
        albedo: Vec3,
        trans_sum: Vec3,
        cloth: Vec4,
        roughness: f32,
        ao: f32,
        f0: f32,
        fx: f32,
        fy: f32,
    }

    /// The contexts, chosen to cross every regime the layer has: a disabled
    /// layer and both arms of the enable condition, a zero and a non-zero fold
    /// amount, world normals spanning the descending ramp's saturated ends and
    /// its curve, lights in front of and behind the cloth, view directions along
    /// and across the beam, and an out-of-range AO on both sides of the clamp.
    fn contexts() -> Vec<Context> {
        (0..SAMPLES)
            .map(|index| {
                let t = index as f32;
                let s = t * 0.1307;
                Context {
                    shade_normal: unit(Vec3::new(s - 0.7, 0.31 - s * 0.5, 0.9 - s * 0.2)),
                    view_dir: unit(Vec3::new(0.2 - s * 0.4, s * 0.3 - 0.15, 1.0 - s * 0.6)),
                    light_dir: unit(Vec3::new(s * 0.5 - 0.6, 0.8 - s * 0.7, s * 0.55 - 1.1)),
                    light_color: Vec3::new(1.0 - s * 0.2, 0.83 + s * 0.11, 0.61 + s * 0.03),
                    world_pos: Vec3::new(t * 0.37 - 4.0, t * -0.53 + 2.5, t * 0.19 - 1.25),
                    // -1.15 .. +1.15 in y, so both saturated ends of the ramp
                    // and the whole curve between them are visited.
                    world_normal: Vec3::new(0.3, t * 0.1 - 1.15, -0.2),
                    albedo: Vec3::new(0.13 + s * 0.2, 0.71 - s * 0.13, 0.42 + s * 0.07),
                    trans_sum: Vec3::new(s * 1.7, 0.9 - s * 0.3, s * s),
                    // index % 4 walks: disabled, transmission-only,
                    // underside-only, both — each with a distinct fold amount,
                    // including 0.
                    cloth: [
                        Vec4::new(0.0, 1.0, 0.0, 0.0),
                        Vec4::new(0.2 + s * 0.1, 1.0, 0.0, 0.0),
                        Vec4::new(0.0, 0.9 - s * 0.05, 0.35 + s * 0.2, 0.0),
                        Vec4::new(0.15 + s * 0.3, 0.4 + s * 0.02, s * 0.4, 0.0),
                    ][index % 4],
                    roughness: 0.02 + s * 0.42,
                    // Crosses 1.0 so the clamp's upper arm is exercised.
                    ao: t * 0.06 - 0.05,
                    f0: 0.5 + (t * 0.041).sin() * 0.5,
                    fx: 0.5 + (t * 0.317 + 1.1).sin() * 0.5,
                    fy: 0.5 + (t * 0.233 - 0.4).sin() * 0.5,
                }
            })
            .collect()
    }

    /// A unit vector, by the same division `normalize` uses on both sides.
    fn unit(v: Vec3) -> Vec3 {
        let len = glsl_dot(v, v).sqrt();
        Vec3::new(v.x / len, v.y / len, v.z / len)
    }

    /// The harness: a fullscreen triangle whose fragment stage evaluates the
    /// entry point at the context its pixel column names.
    const HARNESS_WGSL: &str = r#"
struct ClothContexts { items: array<vec4<f32>, 216> };
@group(0) @binding(0) var<uniform> ctx: ClothContexts;

fn lane(index: u32, slot: u32) -> vec4<f32> { return ctx.items[index * 9u + slot]; }

@vertex
fn cloth_vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(3.0, 1.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

@fragment
fn cloth_underside_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    return axiom_cloth_underside(lane(i, 6u).xyz, lane(i, 0u).w, lane(i, 7u).xyz, lane(i, 5u));
}

@fragment
fn cloth_fold_albedo_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let out = axiom_cloth_fold(
        lane(i, 6u).xyz, lane(i, 0u).xyz, lane(i, 5u),
        lane(i, 2u).w, lane(i, 3u).w, lane(i, 4u).w,
    );
    return vec4<f32>(out.albedo, f32(axiom_cloth_enabled(lane(i, 5u))));
}

@fragment
fn cloth_fold_normal_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let out = axiom_cloth_fold(
        lane(i, 6u).xyz, lane(i, 0u).xyz, lane(i, 5u),
        lane(i, 2u).w, lane(i, 3u).w, lane(i, 4u).w,
    );
    let uv = axiom_cloth_fold_uv(lane(i, 4u).xyz);
    return vec4<f32>(out.normal, uv.x);
}

@fragment
fn cloth_light_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let out = axiom_cloth_light(lane(i, 0u).xyz, lane(i, 1u).xyz, lane(i, 2u).xyz, lane(i, 3u).xyz);
    return vec4<f32>(out, axiom_cloth_transmission(lane(i, 5u), lane(i, 1u).w));
}

@fragment
fn cloth_transmitted_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let amount = axiom_cloth_transmission(lane(i, 5u), lane(i, 1u).w);
    let out = axiom_cloth_transmitted(lane(i, 8u).xyz, lane(i, 6u).xyz, amount);
    let uv = axiom_cloth_fold_uv(lane(i, 4u).xyz);
    return vec4<f32>(out, uv.y);
}

// The CLOTH_LIGHT chunk whole: three macro expansions in index order, summed,
// then scaled ONCE. The third light is handed a zero colour, which is how the
// source's `#if NUM_DIR_LIGHTS > 2` absence is spelled without a branch.
@fragment
fn cloth_chunk_fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let i = u32(position.x);
    let n = lane(i, 0u).xyz;
    let v = lane(i, 1u).xyz;
    var ow_trans = vec3<f32>(0.0, 0.0, 0.0);
    ow_trans = ow_trans + axiom_cloth_light(n, v, lane(i, 2u).xyz, lane(i, 3u).xyz);
    ow_trans = ow_trans + axiom_cloth_light(n, v, lane(i, 1u).xyz, lane(i, 6u).xyz);
    ow_trans = ow_trans + axiom_cloth_light(n, v, lane(i, 0u).xyz, vec3<f32>(0.0, 0.0, 0.0));
    let amount = axiom_cloth_transmission(lane(i, 5u), lane(i, 1u).w);
    return vec4<f32>(axiom_cloth_transmitted(ow_trans, lane(i, 6u).xyz, amount), 0.0);
}
"#;

    struct Gpu {
        device: wgpu::Device,
        queue: wgpu::Queue,
        backend: wgpu::Backend,
    }

    impl Gpu {
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

        fn render(&self, module: &wgpu::ShaderModule, entry: &str, uniform: &[u8]) -> Vec<[f32; 4]> {
            let layout = self
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("axiom-cloth-parity-bgl"),
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
                    label: Some("axiom-cloth-parity-uniform"),
                    contents: uniform,
                    usage: wgpu::BufferUsages::UNIFORM,
                },
            );
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("axiom-cloth-parity-bg"),
                layout: &layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            let pipeline_layout =
                self.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("axiom-cloth-parity-pl"),
                        bind_group_layouts: &[&layout],
                        push_constant_ranges: &[],
                    });
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("axiom-cloth-parity-pipeline"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module,
                        entry_point: Some("cloth_vs"),
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
                label: Some("axiom-cloth-parity-target"),
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
                label: Some("axiom-cloth-parity-readback"),
                size: u64::from(row_bytes),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("axiom-cloth-parity-pass"),
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

    /// The uniform block: nine `vec4` per context, in the order `lane()` unpacks.
    fn uniform_bytes(contexts: &[Context]) -> Vec<u8> {
        let mut bytes: Vec<u8> = contexts
            .iter()
            .flat_map(|c| {
                [
                    [c.shade_normal.x, c.shade_normal.y, c.shade_normal.z, c.roughness],
                    [c.view_dir.x, c.view_dir.y, c.view_dir.z, c.ao],
                    [c.light_dir.x, c.light_dir.y, c.light_dir.z, c.f0],
                    [c.light_color.x, c.light_color.y, c.light_color.z, c.fx],
                    [c.world_pos.x, c.world_pos.y, c.world_pos.z, c.fy],
                    [c.cloth.x, c.cloth.y, c.cloth.z, c.cloth.w],
                    [c.albedo.x, c.albedo.y, c.albedo.z, 0.0],
                    [c.world_normal.x, c.world_normal.y, c.world_normal.z, 0.0],
                    [c.trans_sum.x, c.trans_sum.y, c.trans_sum.z, 0.0],
                ]
            })
            .flatten()
            .flat_map(f32::to_le_bytes)
            .collect();
        bytes.resize(SAMPLES * LANES * 16, 0);
        bytes
    }

    /// Compare one entry point's four lanes against the CPU reference, and
    /// return the worst absolute deviation seen.
    fn compare(gpu: &Gpu, module: &wgpu::ShaderModule, entry: &str, expected: &[[f32; 4]]) -> f32 {
        let actual = gpu.render(module, entry, &uniform_bytes(&contexts()));
        actual
            .iter()
            .zip(expected)
            .enumerate()
            .flat_map(|(sample, (got, want))| {
                got.iter()
                    .zip(want)
                    .enumerate()
                    .map(move |(lane, (g, w))| (sample, lane, *g, *w))
            })
            .map(|(sample, lane, got, want)| {
                let scaled = (got - want).abs() / f32::max(want.abs(), 1.0);
                assert!(
                    scaled <= TOLERANCE,
                    "{entry} disagrees at sample {sample} lane {lane}: \
                     GPU {got} vs CPU {want} (scaled delta {scaled:e})"
                );
                scaled
            })
            .fold(0.0_f32, f32::max)
    }

    #[test]
    fn cloth_wgsl_agrees_with_the_cpu_reference_on_a_real_adapter() {
        let gpu = Gpu::acquire();
        // The error scope is the SHARED device's, so it is entered exclusively;
        // see `crate::test_gpu::validating`.
        let (module, failure) = crate::test_gpu::validating(&gpu.device, || {
            gpu
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("axiom-cloth-parity-shader"),
                    source: wgpu::ShaderSource::Wgsl(
                        format!("{CLOTH_WGSL}\n{HARNESS_WGSL}").into(),
                    ),
                })
        });
        assert!(
            failure.is_none(),
            "CLOTH_WGSL must compile"
        );

        let ctx = contexts();
        let underside_expected: Vec<[f32; 4]> = ctx
            .iter()
            .map(|c| {
                let (alb, rough) = underside(c.albedo, c.roughness, c.world_normal, c.cloth);
                [alb.x, alb.y, alb.z, rough]
            })
            .collect();
        let fold_albedo_expected: Vec<[f32; 4]> = ctx
            .iter()
            .map(|c| {
                let (alb, _) = fold(c.albedo, c.shade_normal, c.cloth, c.f0, c.fx, c.fy);
                [alb.x, alb.y, alb.z, f32::from(u8::from(enabled(c.cloth)))]
            })
            .collect();
        let fold_normal_expected: Vec<[f32; 4]> = ctx
            .iter()
            .map(|c| {
                let (_, n) = fold(c.albedo, c.shade_normal, c.cloth, c.f0, c.fx, c.fy);
                [n.x, n.y, n.z, fold_uv(c.world_pos).x]
            })
            .collect();
        let light_expected: Vec<[f32; 4]> = ctx
            .iter()
            .map(|c| {
                let out = cloth_light(c.shade_normal, c.view_dir, c.light_dir, c.light_color);
                [out.x, out.y, out.z, transmission(c.cloth, c.ao)]
            })
            .collect();
        let transmitted_expected: Vec<[f32; 4]> = ctx
            .iter()
            .map(|c| {
                let out = transmitted(c.trans_sum, c.albedo, transmission(c.cloth, c.ao));
                [out.x, out.y, out.z, fold_uv(c.world_pos).y]
            })
            .collect();
        let chunk_expected: Vec<[f32; 4]> = ctx
            .iter()
            .map(|c| {
                let n = c.shade_normal;
                let v = c.view_dir;
                let sum = cloth_light(n, v, c.light_dir, c.light_color)
                    .add(cloth_light(n, v, c.view_dir, c.albedo))
                    .add(cloth_light(n, v, c.shade_normal, Vec3::ZERO));
                let out = transmitted(sum, c.albedo, transmission(c.cloth, c.ao));
                [out.x, out.y, out.z, 0.0]
            })
            .collect();

        let worst = [
            ("cloth_underside_fs", underside_expected),
            ("cloth_fold_albedo_fs", fold_albedo_expected),
            ("cloth_fold_normal_fs", fold_normal_expected),
            ("cloth_light_fs", light_expected),
            ("cloth_transmitted_fs", transmitted_expected),
            ("cloth_chunk_fs", chunk_expected),
        ]
        .iter()
        .map(|(entry, expected)| compare(&gpu, &module, entry, expected))
        .fold(0.0_f32, f32::max);

        assert!(
            worst <= TOLERANCE,
            "cloth parity on {:?}: worst scaled delta {worst:e} exceeds the budget {TOLERANCE:e}",
            gpu.backend
        );
        // The budget must stay a *measurement* plus headroom, never a number
        // fitted to the miss that happened to be observed — so the measurement
        // is asserted, not merely printed. (Not printed at all: console output
        // is banned in a module, and the hygiene scan is not `cfg(test)`-aware.)
        // If this fires, the hardware moved and the header's ULP account has to
        // be redone — not the constant nudged.
        assert!(
            worst <= MEASURED_WORST,
            "cloth parity on {:?}: this adapter deviates by {worst:e}, \
             more than the recorded {MEASURED_WORST:e}; redo the ULP account",
            gpu.backend
        );
    }
}
