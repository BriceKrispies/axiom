//! The cascade fragment stage, as **shader text** — the one definition.
//!
//! `shading.rs` is the CPU reference for this arithmetic and
//! `adapter_proof.rs` compares the two on a real adapter, probe for probe. That
//! proof is only worth anything if the text it compiles is the *same text the
//! pipeline runs*, so the text lives here, in one non-test constant, and both
//! the proof and `scene_wgsl.rs` splice it. A copy in each would make the proof
//! compare one transcription to a different one.
//!
//! Split into three pieces because the two consumers agree on everything except
//! **where the bindings live**: the proof owns a whole bind group and declares
//! `csm` / `csm_maps` / `csm_samp` at group 0, while the main pass has to fit
//! them into the tail of its existing shadow group. The functions reference the
//! three by name and never by location, so only the declaration lines differ.
//!
//! Three WGSL deltas from the source GLSL, all deliberate:
//!
//! - `texture(...)` → `textureSampleLevel(..., 0.0)`. An explicit LOD is what
//!   makes the source's early `return`s legal in WGSL, which requires uniform
//!   control flow around an implicit-derivative sample.
//! - `sc.xyz / sc.w * 0.5 + 0.5` → `ndc.z` with a flipped `v`. The wgpu clip
//!   range is `[0, 1]` and the framebuffer's `v` counts down — the same two
//!   conventions `scene_wgsl.rs`'s existing `shadow_factor` applies.
//! - `smoothstep` written out. WGSL leaves `smoothstep(low, high, x)`
//!   indeterminate when `low >= high`, and the far fade-out calls it that way on
//!   purpose (a descending ramp).

#[cfg(test)]
use crate::cascade::MAX_CASCADES;

/// The bind group the main pass puts the cascade trio in, and the three
/// bindings it puts them at.
///
/// Group 2 is the shadow group and already holds the single-volume map, its
/// comparison sampler, the shadow uniform, the resolved ambient occlusion, its
/// sampler and the contact shadow — bindings 0..5. WebGPU's default
/// `maxBindGroups` is **four**, so 0..3 is the whole budget and there is no
/// group of its own to move into; the cascades take the tail of the group whose
/// subject they share.
///
/// These are the ONE definition of the numbers. `scene_renderer` builds its bind
/// group layout from them and [`csm_bindings_wgsl`] writes the same three into
/// the shader, so the layout and the text cannot drift apart the way two
/// hand-written lists would.
pub(crate) const CSM_GROUP: u32 = 2;
/// The `CsmU` uniform.
pub(crate) const CSM_UNIFORM_BINDING: u32 = 6;
/// The layered atlas: `texture_2d_array<f32>`, one layer per cascade.
pub(crate) const CSM_ATLAS_BINDING: u32 = 7;
/// The atlas's sampler. **Non-filtering**, which is not a preference: the atlas
/// is `R32Float`, which is `unfilterable-float` in core WebGPU, and that is
/// exactly the source's own `NearestFilter` configuration.
pub(crate) const CSM_SAMPLER_BINDING: u32 = 8;

/// The four compile-time constants the fragment stage is specialised on.
///
/// **Four cascades, and the source's top tap tier, fixed.** Both are deliberate,
/// and neither is a placeholder:
///
/// - Four is the *shape of the data*, not a budget. Every per-cascade lane is a
///   `vec4` and the atlas is [`MAX_CASCADES`] layers, so a three-cascade
///   configuration would not save a lane or a layer — it would only introduce
///   the sentinel row into the shader's selection scan and its far fade-out, for
///   nothing measured.
/// - The tap counts are the source's `quality >= 3` tier. The cheaper tiers
///   exist in [`crate::cascade::quality_tier`] and are reachable, but binding
///   them here would mean a second, quieter performance policy beside the one
///   the capability system already states: a device that cannot afford the
///   cascade path does not carry
///   [`axiom_host::RenderCapability::CascadedShadows`], and then this whole
///   chunk early-outs on `params.x <= 0.0` at its first line. Inventing a
///   half-on tier without a measurement is what the capability docs warn
///   against.
///
/// One `const`, spliced by both consumers, so the proof compiles the taps the
/// pipeline runs.
pub(crate) const CSM_CONSTANTS_WGSL: &str = r#"
const OW_CASCADES: i32 = 4;
const OW_BLOCKER_TAPS: i32 = 16;
const OW_PCF_TAPS: i32 = 20;
const OW_PCSS: bool = true;
"#;

/// `owCsmParams` and friends: the whole per-frame cascade uniform, 368 bytes.
///
/// `matrices` is one light view-projection per cascade; `split` / `split_near` /
/// `texel` / `range` are the per-cascade lanes packed as `vec4`s (which is why
/// [`MAX_CASCADES`] is four — it is the shape of the data, not a budget).
/// `map_size` carries the atlas edge and its reciprocal, `sun_world` points FROM
/// the scene TOWARD the sun, and `params` is
/// `(strength, tan(sun radius), max PCF texels, jitter phase)`.
pub(crate) const CSM_UNIFORM_WGSL: &str = r#"
struct CsmU {
    matrices: array<mat4x4<f32>, 4>,
    split: vec4<f32>,
    split_near: vec4<f32>,
    texel: vec4<f32>,
    range: vec4<f32>,
    map_size: vec4<f32>,
    sun_world: vec4<f32>,
    params: vec4<f32>,
};
"#;

/// The three cascade bindings, written at the numbers above.
pub(crate) fn csm_bindings_wgsl() -> String {
    let group = CSM_GROUP;
    let uniform = CSM_UNIFORM_BINDING;
    let atlas = CSM_ATLAS_BINDING;
    let sampler = CSM_SAMPLER_BINDING;
    [
        format!("@group({group}) @binding({uniform}) var<uniform> csm: CsmU;\n"),
        format!("@group({group}) @binding({atlas}) var csm_maps: texture_2d_array<f32>;\n"),
        format!("@group({group}) @binding({sampler}) var csm_samp: sampler;\n"),
    ]
    .concat()
}

/// The whole chunk the main pass splices: constants, uniform struct, the three
/// group-2 bindings, then the fragment stage.
///
/// Spliced by `surface_program::wgsl_template::scene_shader`, which is where the
/// engine's other always-present layers (the cloth functions, the indirect
/// lighting composition) already go in — and for the same reason: the LIGHTING
/// stage calls into it, and the lighting stage lives in the scene WGSL's suffix,
/// which every scene shader carries.
pub(crate) fn csm_chunk() -> String {
    [
        CSM_CONSTANTS_WGSL,
        CSM_UNIFORM_WGSL,
        &csm_bindings_wgsl(),
        CSM_FUNCTIONS_WGSL,
    ]
    .concat()
}

/// `owMix` … `owSunShadow` — the fragment stage itself, verbatim from
/// `render/csm.js`'s shader chunk with the three deltas the module docs list.
///
/// References `csm`, `csm_maps` and `csm_samp` by name; the consumer declares
/// them. Keeps its own `if`s and `for`s: this is shader text, and the Branchless
/// Law governs Rust control flow, not the contents of a `&str`. The Rust peer in
/// `shading.rs` is the branchless one.
pub(crate) const CSM_FUNCTIONS_WGSL: &str = r#"
// GLSL's spec factoring, `x * (1 - a) + y * a` — not the algebraically-equal,
// numerically-different `a + (b - a) * t`. See `shading.rs`'s `mix`.
fn ow_mix(a: f32, b: f32, t: f32) -> f32 {
    return a * (1.0 - t) + b * t;
}

// Written out: WGSL leaves smoothstep(low, high, x) indeterminate when
// low >= high, and the far fade-out calls it that way on purpose.
fn ow_smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = min(max((x - e0) / (e1 - e0), 0.0), 1.0);
    return t * t * (3.0 - 2.0 * t);
}

fn ow_ig_noise(p: vec2<f32>) -> f32 {
    let d = p.x * 0.06711056 + p.y * 0.00583715;
    let f0 = d - floor(d);
    let m = 52.9829189 * f0;
    return m - floor(m);
}

fn ow_vogel(i: i32, n: i32, phi: f32) -> vec2<f32> {
    let r = sqrt((f32(i) + 0.5) / f32(n));
    let theta = f32(i) * 2.39996323 + phi;
    return vec2<f32>(cos(theta), sin(theta)) * r;
}

fn ow_csm_tap(layer: i32, uv: vec2<f32>) -> f32 {
    return textureSampleLevel(csm_maps, csm_samp, uv, layer, 0.0).r;
}

fn ow_csm_cascade(c: i32, w_pos: vec3<f32>, w_n: vec3<f32>, ndl: f32, rot: f32) -> f32 {
    let texel_world = csm.texel[c];
    let range = csm.range[c];

    // normal offset - pushes the sample point off the surface by roughly one
    // shadow texel, scaled up at grazing angles where the texel projects wide.
    let p = w_pos + w_n * (texel_world * (0.55 + 1.1 * (1.0 - ndl)));
    let sc = csm.matrices[c] * vec4<f32>(p, 1.0);
    let ndc = sc.xyz / sc.w;
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, ndc.y * -0.5 + 0.5);
    let depth = ndc.z;
    if (depth >= 1.0 || depth <= 0.0) { return 1.0; }
    let edge = min(uv, vec2<f32>(1.0, 1.0) - uv);
    if (min(edge.x, edge.y) <= 0.0) { return 1.0; }

    let slope = min(max(sqrt(max(0.0, 1.0 - ndl * ndl)) / max(ndl, 0.12), 0.0), 5.0);
    let bias = (texel_world * (0.7 + 1.15 * slope)) / range;
    let recv = depth - bias;

    let inv_tex = csm.map_size.y;
    let extent = texel_world * csm.map_size.x;
    let max_r = csm.params.z * inv_tex;
    var filter_r = 1.4 * inv_tex;

    if (OW_PCSS) {
        let search_r = min(max_r, 10.0 * inv_tex);
        var blocker = 0.0;
        var count = 0.0;
        for (var i = 0; i < OW_BLOCKER_TAPS; i = i + 1) {
            let d = ow_csm_tap(c, uv + ow_vogel(i, OW_BLOCKER_TAPS, rot) * search_r);
            if (d < recv) { blocker = blocker + d; count = count + 1.0; }
        }
        if (count < 0.5) { return 1.0; }
        blocker = blocker / count;
        let gap = max(0.0, (recv - blocker) * range);
        let penumbra = gap * csm.params.y;
        filter_r = min(max(penumbra / extent, 1.0 * inv_tex), max_r);
    }

    var sum = 0.0;
    for (var i = 0; i < OW_PCF_TAPS; i = i + 1) {
        let d = ow_csm_tap(c, uv + ow_vogel(i, OW_PCF_TAPS, rot) * filter_r);
        sum = sum + step(recv, d);
    }
    return sum / f32(OW_PCF_TAPS);
}

fn ow_sun_shadow(view_depth: f32, w_pos: vec3<f32>, w_n: vec3<f32>, frag: vec2<f32>) -> f32 {
    if (csm.params.x <= 0.0) { return 1.0; }
    if (view_depth >= csm.split[OW_CASCADES - 1]) { return 1.0; }
    let ndl = dot(w_n, csm.sun_world.xyz);
    if (ndl <= 0.0) { return 1.0; }

    let rot = ow_ig_noise(frag + vec2<f32>(csm.params.w, csm.params.w)) * 6.2831853;

    var c = OW_CASCADES - 1;
    for (var i = 0; i < OW_CASCADES; i = i + 1) {
        if (view_depth < csm.split[i]) { c = i; break; }
    }

    var s = ow_csm_cascade(c, w_pos, w_n, ndl, rot);

    // cross-fade the last 12% of a cascade into the next one
    if (c < OW_CASCADES - 1) {
        let a = csm.split_near[c];
        let b = csm.split[c];
        let t = ow_smoothstep(ow_mix(a, b, 0.88), b, view_depth);
        if (t > 0.001) { s = ow_mix(s, ow_csm_cascade(c + 1, w_pos, w_n, ndl, rot), t); }
    }

    // fade the whole thing out at the far edge so there is no hard terminator
    let last = csm.split[OW_CASCADES - 1];
    let fade_out = ow_smoothstep(last, last * 0.88, view_depth);
    s = ow_mix(1.0, s, fade_out);

    return ow_mix(1.0, s, csm.params.x);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// The chunk the main pass splices carries all four parts, in an order WGSL
    /// can compile: the constants and the struct before the bindings that use
    /// them, and the bindings before the functions that read them.
    #[test]
    fn the_chunk_declares_its_constants_struct_bindings_and_functions_in_order() {
        let chunk = csm_chunk();
        let consts = chunk.find("const OW_CASCADES: i32 = 4;").expect("constants");
        let decl = chunk.find("struct CsmU").expect("uniform struct");
        let bind = chunk.find("var<uniform> csm: CsmU;").expect("uniform binding");
        let func = chunk.find("fn ow_sun_shadow(").expect("fragment stage");
        assert!(consts < decl, "constants precede the struct");
        assert!(decl < bind, "the struct precedes the binding that names it");
        assert!(bind < func, "the bindings precede the functions that read them");
        // The tap tier is the source's top one, and four cascades is the shape
        // of the `vec4` lanes - see `CSM_CONSTANTS_WGSL`.
        assert!(chunk.contains("const OW_BLOCKER_TAPS: i32 = 16;"));
        assert!(chunk.contains("const OW_PCF_TAPS: i32 = 20;"));
        assert!(chunk.contains("const OW_PCSS: bool = true;"));
        assert_eq!(MAX_CASCADES, 4);
    }

    /// The Rust binding numbers and the shader's are one definition: the layout
    /// `scene_renderer` builds and the text spliced here are both written from
    /// these constants, so a renumbering that touched only one is impossible.
    #[test]
    fn the_bindings_are_written_at_the_numbers_the_layout_uses() {
        let text = csm_bindings_wgsl();
        assert_eq!((CSM_GROUP, CSM_UNIFORM_BINDING), (2, 6));
        assert_eq!((CSM_ATLAS_BINDING, CSM_SAMPLER_BINDING), (7, 8));
        assert!(text.contains("@group(2) @binding(6) var<uniform> csm: CsmU;"));
        assert!(text.contains("@group(2) @binding(7) var csm_maps: texture_2d_array<f32>;"));
        assert!(text.contains("@group(2) @binding(8) var csm_samp: sampler;"));
    }

    /// The functions reference the three bindings by NAME only - that is what
    /// lets the adapter proof declare them at group 0 and the main pass at the
    /// tail of group 2 while both compile the identical text. A `@group` inside
    /// this constant would silently pin one consumer's layout onto the other.
    #[test]
    fn the_function_text_declares_no_bindings_of_its_own() {
        assert!(!CSM_FUNCTIONS_WGSL.contains("@group"));
        assert!(!CSM_FUNCTIONS_WGSL.contains("@binding"));
        assert!(CSM_FUNCTIONS_WGSL.contains("fn ow_sun_shadow("));
        // The explicit LOD is one of the three deltas from the source GLSL, and
        // it is the one that makes the early returns legal.
        assert!(CSM_FUNCTIONS_WGSL.contains("textureSampleLevel(csm_maps, csm_samp"));
        assert!(!CSM_UNIFORM_WGSL.contains("@group"));
        assert!(CSM_UNIFORM_WGSL.contains("struct CsmU"));
    }
}
