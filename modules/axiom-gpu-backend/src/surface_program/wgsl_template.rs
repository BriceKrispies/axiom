//! The fixed WGSL a generated surface program is spliced into, and the shared
//! helpers it calls.
//!
//! Three constants and one function, and the split between them is the whole
//! design:
//!
//! * [`SURFACE_PRELUDE_WGSL`] — the fixed vocabulary every generated program is
//!   written against: the three structs of the surface calling convention, and
//!   the deterministic lattice-hash / gradient-noise helpers that
//!   [`crate::surface_program::emit_ops`] calls for `Noise` and `Fbm`. It is one
//!   `&str` rather than text the emitter assembles, because none of it varies
//!   with the surface.
//! * [`DEFAULT_SURFACE_WGSL`] — the program a draw naming `surface_program == 0`
//!   runs: the identity, reproducing today's behaviour exactly from the instance
//!   lanes the vertex stage already interpolates.
//! * [`scene_shader`] — the splice, by **concatenation**, exactly as
//!   [`crate::surface_encode::shader_source`] composes the sRGB curve onto a
//!   pass. There is no preprocessor, no `#ifdef` and no textual substitution: the
//!   main pass's WGSL is two halves with a program-shaped hole between them.
//!
//! ## Why the noise helpers are a fixed constant rather than generated per node
//!
//! WGSL has no noise. `axiom-noise` keys `axiom_kernel::StableHash` — FNV-1a over
//! 64-bit words — by an integer lattice cell, which **is** expressible in WGSL
//! once a `u64` is carried as a `vec2<u32>` of `(high, low)` halves. That mirror
//! is the single highest-risk piece of the whole emitter: one wrong bit in the
//! hash and every noise-driven surface differs on the GPU from the CPU and from
//! every bake. So it is written **once**, here, and pinned by a parity test that
//! renders the hash itself and compares it against `StableHash::of_words`.
//!
//! The helpers are emitted into every program whether or not it samples noise.
//! That is deliberate: the semantic graph was already const-folded, CSE'd and
//! dead-code-eliminated upstream, and running a second, textual eliminator over
//! the generated WGSL would be a ceremonial optimiser. An unused WGSL function
//! costs nothing at runtime.

/// The fixed vocabulary a generated `axiom_surface` is written against.
///
/// **The calling convention.** `SurfaceIn` is what the main pass hands a
/// program: the object-space sample position and normal (a surface's expressions
/// are evaluated in object space — see `crates/axiom-surface/ARCHITECTURE.md`),
/// the interpolated uv, the presentation time, and the two lanes the existing
/// instance stream already carries so the *default* program can reproduce
/// today's frame from them alone. `SurfaceParams` is the shared uniform region
/// [`crate::surface_program::params`] lays out. `SurfaceOut` is the channel
/// result the fragment stage's lighting maths consumes.
///
/// **The lattice hash.** `axiom_mul32` / `axiom_mul64` / `axiom_fnv_step` are a
/// 64-bit FNV-1a built out of 32-bit arithmetic, because WGSL has no `u64`. A
/// `u64` is a `vec2<u32>` of `(high, low)`; multiplication is the schoolbook
/// 16-bit split, and every intermediate is `u32`, whose overflow WGSL defines as
/// wrapping. `axiom_mod12` reduces the 64-bit digest modulo twelve without ever
/// forming the 64-bit quotient: `2^32 % 12 == 4`, so the whole reduction is two
/// `u32` remainders and a multiply.
pub(crate) const SURFACE_PRELUDE_WGSL: &str = r#"
// ---------------------------------------------------------------------------
// The surface calling convention. Fixed: a generated program may not change it.
// ---------------------------------------------------------------------------

struct SurfaceIn {
    // The OBJECT-space sample position. A world-space pattern swims when the
    // object moves; this is the lane that makes a pattern ride with it.
    object_pos: vec3<f32>,
    // The interpolated surface parameterisation.
    uv: vec2<f32>,
    // The OBJECT-space normal, for the same reason as `object_pos`.
    object_normal: vec3<f32>,
    // Presentation time in seconds — the frame's `axiom_host::FramePacket::time`,
    // explicitly supplied engine time and never a wall clock. It is zero for a
    // frame whose surfaces read no clock, so a static surface costs exactly what
    // it did before there was one.
    time: f32,
    // What the existing pipeline already resolved for this fragment: the sampled
    // albedo times the per-vertex and per-instance colour. The DEFAULT program
    // returns it unchanged, which is what makes every existing app pixel-identical.
    albedo: vec4<f32>,
    // The draw's material emissive, from the instance stream.
    emissive: vec3<f32>,
    // ---- World space. Added for the runtime material shader. -------------
    //
    // The object-space lanes above exist because a world-space pattern swims
    // when the object moves. These are the deliberate opposite: a runtime
    // material's weathering is *anchored to the world* on purpose — rain runs
    // down, ground splash is measured up from a world `groundY`, the dust wedge
    // sits at the wall/ground junction, and triplanar projects on world axes. A
    // pattern that rode with the object would be wrong for all four.
    //
    // Additive: a generated field-algebra program names the lanes it reads and
    // simply never names these, so every existing program emits the same WGSL
    // and renders the same pixels. `surface_program::parity*` proves it.
    world_pos: vec3<f32>,
    world_normal: vec3<f32>,
    // Fragment-to-camera, normalised. Parallax occlusion mapping needs a view
    // vector and cannot reconstruct one: the surface program runs before the
    // lighting stage that knows where the camera is.
    view_dir: vec3<f32>,
    // The per-vertex colour times the per-instance colour, on its own.
    //
    // `albedo` above already has this multiplied in, which is right for a
    // program that returns it unchanged. It is NOT enough for a program that
    // re-samples the albedo texture at its own projected uv — a runtime material
    // projects planar or triplanar in world space, so it must take the sample
    // itself and would otherwise drop the colour entirely. Recovering it by
    // dividing `albedo` by the texel is not an option: the texel can be zero.
    //
    // A program that ignores this lane is unchanged, so every existing surface
    // is unaffected.
    vertex_color: vec4<f32>,
};

struct SurfaceParams {
    slots: array<vec4<f32>, 32>,
};

struct SurfaceOut {
    base_color: vec4<f32>,
    roughness: f32,
    metallic: f32,
    normal: vec3<f32>,
    emission: vec3<f32>,
    opacity: f32,
    // How much back-incident light this surface re-emits toward the eye —
    // fabric transmission, from `materials/shader.js`'s `OW_CLOTH_LIGHT`.
    //
    // A seventh channel rather than a fourth `LightingModel`, deliberately.
    // `axiom_lighting_model()` is a nullary constant, so a model cannot carry a
    // per-surface *amount*; and Unlit -> Lambert -> LambertSpecular is a
    // monotone ladder of how much standard maths a surface takes, whereas
    // transmission is orthogonal to it (cloth still wants Lambert plus
    // specular). A fourth rung would make the closed set 2x3 the moment a
    // second orthogonal term appeared.
    //
    // Emission was the near miss: the slot is exactly right — unshadowed,
    // post-light, pre-fog — but `axiom_surface` has no light rig, so it would
    // lose the back-lit direction and render as the painted card the source
    // wrote this term to avoid.
    //
    // 0.0 for every program that does not author it, which multiplies the
    // transmission term in `fs` to an exact zero and leaves every existing
    // frame unchanged to the bit.
    transmission: f32,
};

// ---------------------------------------------------------------------------
// The deterministic lattice hash: `axiom_kernel::StableHash` (FNV-1a over 64-bit
// words), mirrored. A `u64` is a `vec2<u32>` of (high, low).
// ---------------------------------------------------------------------------

// The 64-bit product of two 32-bit words, as (high, low). Schoolbook over
// 16-bit halves; every intermediate fits a `u32`, whose overflow WGSL defines
// as wrapping, so no term is lost.
fn axiom_mul32(a: u32, b: u32) -> vec2<u32> {
    let al = a & 0xFFFFu;
    let ah = a >> 16u;
    let bl = b & 0xFFFFu;
    let bh = b >> 16u;
    let t0 = al * bl;
    let t1 = ah * bl;
    let t2 = al * bh;
    let t3 = ah * bh;
    let carry = (t0 >> 16u) + (t1 & 0xFFFFu) + (t2 & 0xFFFFu);
    let lo = (t0 & 0xFFFFu) | ((carry & 0xFFFFu) << 16u);
    let hi = t3 + (t1 >> 16u) + (t2 >> 16u) + (carry >> 16u);
    return vec2<u32>(hi, lo);
}

// `a * b` modulo 2^64. The high half needs only the low-by-high cross terms:
// everything above 2^64 is discarded by the wrap anyway.
fn axiom_mul64(a: vec2<u32>, b: vec2<u32>) -> vec2<u32> {
    let p = axiom_mul32(a.y, b.y);
    return vec2<u32>(p.x + a.y * b.x + a.x * b.y, p.y);
}

// The FNV-1a offset basis, 0xcbf29ce484222325.
fn axiom_fnv_basis() -> vec2<u32> {
    return vec2<u32>(0xcbf29ce4u, 0x84222325u);
}

// One FNV-1a round over a 64-bit word: xor, then multiply by the FNV prime
// 0x00000100000001b3.
fn axiom_fnv_step(acc: vec2<u32>, word: vec2<u32>) -> vec2<u32> {
    return axiom_mul64(acc ^ word, vec2<u32>(0x00000100u, 0x000001b3u));
}

// A (possibly negative) lattice coordinate sign-extended into the 64-bit word
// space, matching `xi as i64 as u64` on the CPU.
fn axiom_sext(v: i32) -> vec2<u32> {
    return vec2<u32>(bitcast<u32>(v >> 31u), bitcast<u32>(v));
}

// `StableHash::of_words(&[seed, xi, yi, zi])`.
fn axiom_hash_cell(seed: vec2<u32>, c: vec3<i32>) -> vec2<u32> {
    var h = axiom_fnv_basis();
    h = axiom_fnv_step(h, seed);
    h = axiom_fnv_step(h, axiom_sext(c.x));
    h = axiom_fnv_step(h, axiom_sext(c.y));
    h = axiom_fnv_step(h, axiom_sext(c.z));
    return h;
}

// A 64-bit digest modulo twelve, without forming the 64-bit quotient:
// 2^32 % 12 == 4, so (hi * 2^32 + lo) % 12 == ((hi % 12) * 4 + (lo % 12)) % 12.
fn axiom_mod12(h: vec2<u32>) -> u32 {
    return ((h.x % 12u) * 4u + (h.y % 12u)) % 12u;
}

// The 12 classic Perlin gradient directions, in `axiom-noise`'s order.
fn axiom_grad(index: u32) -> vec3<f32> {
    var table = array<vec3<f32>, 12>(
        vec3<f32>(1.0, 1.0, 0.0),
        vec3<f32>(-1.0, 1.0, 0.0),
        vec3<f32>(1.0, -1.0, 0.0),
        vec3<f32>(-1.0, -1.0, 0.0),
        vec3<f32>(1.0, 0.0, 1.0),
        vec3<f32>(-1.0, 0.0, 1.0),
        vec3<f32>(1.0, 0.0, -1.0),
        vec3<f32>(-1.0, 0.0, -1.0),
        vec3<f32>(0.0, 1.0, 1.0),
        vec3<f32>(0.0, -1.0, 1.0),
        vec3<f32>(0.0, 1.0, -1.0),
        vec3<f32>(0.0, -1.0, -1.0),
    );
    return table[index];
}

// Perlin's quintic fade, 6t^5 - 15t^4 + 10t^3, in the CPU's exact factoring.
fn axiom_fade(t: f32) -> f32 {
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}

// `a + (b - a) * t` — the CPU's spelling, not `a * (1 - t) + b * t`, which is a
// different last bit.
fn axiom_lerp(a: f32, b: f32, t: f32) -> f32 {
    return a + (b - a) * t;
}

// One lattice corner's contribution. The dot is written out rather than handed
// to the `dot` builtin, whose summation order is unspecified.
fn axiom_corner(seed: vec2<u32>, c: vec3<i32>, d: vec3<f32>) -> f32 {
    let g = axiom_grad(axiom_mod12(axiom_hash_cell(seed, c)));
    return g.x * d.x + g.y * d.y + g.z * d.z;
}

// `axiom_noise::gradient_noise::raw_gradient_noise`, mirrored.
fn axiom_gradient_noise(seed: vec2<u32>, p: vec3<f32>) -> f32 {
    let pf = floor(p);
    let i0 = vec3<i32>(pf);
    let i1 = i0 + vec3<i32>(1, 1, 1);
    let f = p - pf;
    let u = axiom_fade(f.x);
    let v = axiom_fade(f.y);
    let w = axiom_fade(f.z);
    let n000 = axiom_corner(seed, vec3<i32>(i0.x, i0.y, i0.z), vec3<f32>(f.x, f.y, f.z));
    let n100 = axiom_corner(seed, vec3<i32>(i1.x, i0.y, i0.z), vec3<f32>(f.x - 1.0, f.y, f.z));
    let n010 = axiom_corner(seed, vec3<i32>(i0.x, i1.y, i0.z), vec3<f32>(f.x, f.y - 1.0, f.z));
    let n110 = axiom_corner(seed, vec3<i32>(i1.x, i1.y, i0.z), vec3<f32>(f.x - 1.0, f.y - 1.0, f.z));
    let n001 = axiom_corner(seed, vec3<i32>(i0.x, i0.y, i1.z), vec3<f32>(f.x, f.y, f.z - 1.0));
    let n101 = axiom_corner(seed, vec3<i32>(i1.x, i0.y, i1.z), vec3<f32>(f.x - 1.0, f.y, f.z - 1.0));
    let n011 = axiom_corner(seed, vec3<i32>(i0.x, i1.y, i1.z), vec3<f32>(f.x, f.y - 1.0, f.z - 1.0));
    let n111 = axiom_corner(seed, vec3<i32>(i1.x, i1.y, i1.z), vec3<f32>(f.x - 1.0, f.y - 1.0, f.z - 1.0));
    let nx00 = axiom_lerp(n000, n100, u);
    let nx10 = axiom_lerp(n010, n110, u);
    let nx01 = axiom_lerp(n001, n101, u);
    let nx11 = axiom_lerp(n011, n111, u);
    let nxy0 = axiom_lerp(nx00, nx10, v);
    let nxy1 = axiom_lerp(nx01, nx11, v);
    return clamp(axiom_lerp(nxy0, nxy1, w), -1.0, 1.0);
}

// `axiom_noise::NoiseValue::from_signal`: clamp into [-1, 1], and read a
// non-finite signal as zero.
fn axiom_signal(value: f32) -> f32 {
    return select(0.0, clamp(value, -1.0, 1.0), value == value);
}

// `axiom_noise::value_noise`.
fn axiom_noise(seed: vec2<u32>, p: vec3<f32>) -> f32 {
    return axiom_signal(axiom_gradient_noise(seed, p));
}

// `StableHash::of_words(&[seed, octave])` — the per-octave seed an FBM derives.
fn axiom_octave_seed(seed: vec2<u32>, octave: u32) -> vec2<u32> {
    var h = axiom_fnv_basis();
    h = axiom_fnv_step(h, seed);
    h = axiom_fnv_step(h, vec2<u32>(0u, octave));
    return h;
}

// `axiom_noise::Fbm::sample`. The octave count is at least one, so the total
// amplitude is at least 1.0 and the normalisation never divides by zero.
fn axiom_fbm(
    seed: vec2<u32>,
    octaves: u32,
    frequency: f32,
    lacunarity: f32,
    gain: f32,
    p: vec3<f32>,
) -> f32 {
    let count = max(octaves, 1u);
    var sum = 0.0;
    var total = 0.0;
    var amp = 1.0;
    var freq = frequency;
    for (var i = 0u; i < count; i = i + 1u) {
        let n = axiom_gradient_noise(axiom_octave_seed(seed, i), p * freq);
        sum = sum + n * amp;
        total = total + amp;
        amp = amp * gain;
        freq = freq * lacunarity;
    }
    return axiom_signal(sum / total);
}

// ---------------------------------------------------------------------------
// The lighting-model discriminant: `axiom_surface::LightingModel`, mirrored.
// Its wire code IS its table index there, and these are the same four codes,
// pinned by `the_wgsl_lighting_codes_are_the_surface_layers_discriminants`.
//
// A generated program RETURNS one of these from `axiom_lighting_model`, and the
// main pass's `fs` spends it on `select`s and multiplies — never on a branch and
// never on a second module. Four models times N surfaces is N programs.
// ---------------------------------------------------------------------------

// Base colour and emission presented as-is: no ambient, no light, no shadow and
// no specular. Fog still applies — it is a depth effect, not a lighting one.
const AXIOM_LIGHT_UNLIT: u32 = 0u;
// Diffuse gathering only: hemisphere ambient plus the sum of N.L, no highlight.
const AXIOM_LIGHT_LAMBERT: u32 = 1u;
// Diffuse gathering plus the Blinn-Phong term this pass has always computed.
// The DEFAULT, so every existing draw is unchanged.
const AXIOM_LIGHT_LAMBERT_SPECULAR: u32 = 2u;
// Cook-Torrance: GGX distribution, Smith height-correlated visibility, Schlick
// Fresnel, Lambert diffuse - three.js r180's `MeshStandardMaterial`. The one
// model that reads `SurfaceOut.roughness` and `SurfaceOut.metallic`; the maths
// lives in `crate::scene_wgsl`'s prefix as `axiom_pbr_*`.
const AXIOM_LIGHT_PHYSICAL: u32 = 3u;
"#;

/// The program a draw naming `surface_program == 0` runs.
///
/// It is the **identity**: every channel takes the value the existing pipeline
/// already resolved, so the frame the main pass produces with this spliced in is
/// the frame it produced before there was a splice at all. The unbound-channel
/// values (`roughness`, `metallic`, `normal`) are
/// `axiom_surface::SurfaceChannel::default_value`'s; `metallic` is read by no
/// lighting model, deliberately — see
/// [`crate::surface_program::emit_lighting`].
///
/// A fragment program is **two** functions, and this is both of them:
/// `axiom_lighting_model`, which says how the surface participates in lighting,
/// and `axiom_surface`, which says what its six channels are. The default model
/// is `axiom_surface::LightingModel::LambertSpecular`
/// ([`AXIOM_LIGHT_LAMBERT_SPECULAR`], code `2`), which is exactly what this pass
/// has always computed — so a draw naming no surface, and a surface authoring no
/// model, both render pixel-identically to the frame before this existed.
///
/// **This is why the model is a value the PROGRAM returns rather than a lane in
/// the parameter buffer.** This pass binds no parameter buffer yet and hands
/// every program the zero value, and a zero lane would decode as
/// [`AXIOM_LIGHT_UNLIT`] — unlighting every frame in the engine. A value stated
/// by the program cannot default to zero by accident. It costs no pipeline
/// either way: a lighting model is a per-surface constant of the same authored
/// surface the six channels come from, generated with them and keyed by the same
/// digest.
///
/// `params` is unread here, and is unread by *every* program the main pass runs
/// today: this backend binds no parameter buffer yet, so the main pass hands the
/// zero value. Binding it is pipeline work.
///
/// [`AXIOM_LIGHT_UNLIT`]: SURFACE_PRELUDE_WGSL
/// [`AXIOM_LIGHT_LAMBERT_SPECULAR`]: SURFACE_PRELUDE_WGSL
pub(crate) const DEFAULT_SURFACE_WGSL: &str = r#"
fn axiom_lighting_model() -> u32 {
    return 2u;
}

fn axiom_surface(in: SurfaceIn, params: SurfaceParams) -> SurfaceOut {
    var out: SurfaceOut;
    out.base_color = in.albedo;
    out.roughness = 0.5;
    out.metallic = 0.0;
    out.normal = vec3<f32>(0.0, 0.0, 1.0);
    out.emission = in.emissive;
    out.opacity = in.albedo.w;
    out.transmission = 0.0;
    return out;
}
"#;

/// The program the **vertex** stage runs for a draw that displaces nothing: the
/// zero offset.
///
/// `vs` adds this to the object-space position before the MVP multiply, so with
/// the default spliced in the vertex it transforms is the vertex it was handed —
/// bit for bit, because adding an exact zero to an IEEE float is the identity on
/// every input including infinities and negative zero. That is what keeps every
/// existing frame pixel-identical now that the splice exists.
pub(crate) const DEFAULT_DISPLACE_WGSL: &str = r#"
fn axiom_displace(pos: vec3<f32>, nrm: vec3<f32>, uv: vec2<f32>, t: f32, params: SurfaceParams) -> vec3<f32> {
    return vec3<f32>(0.0, 0.0, 0.0);
}
"#;

/// The main pass's whole shader source: the fixed surface vocabulary, one
/// generated **vertex** program, the scene WGSL's first half, one generated
/// **fragment** program, then the scene WGSL's second half.
///
/// Concatenation, never substitution — the precedent is
/// [`crate::surface_encode::shader_source`], and the property that matters is
/// that the same `(displace, program)` pair always yields the same bytes, which
/// is what makes the string cacheable by the surface's digest.
///
/// **The order is forced, not stylistic.** WGSL requires a declaration before
/// its use, `vs` lives in `prefix` and calls `axiom_displace`, and `fs` lives in
/// `suffix` and calls `axiom_surface` — so the vertex program has to precede the
/// scene's first half while the fragment program has to follow it. Both halves
/// of one surface land in **one** module keyed by one digest: a displacing
/// surface must never force a second pipeline for the same material.
pub(crate) fn scene_shader(prefix: &str, displace: &str, program: &str, suffix: &str) -> String {
    [
        SURFACE_PRELUDE_WGSL,
        displace,
        prefix,
        // The cloth layer's functions are spliced into EVERY scene shader, not
        // only into a composed material program, because the *lighting* stage
        // calls two of them (`axiom_cloth_light` in the light loop and
        // `axiom_cloth_transmitted` after it) and the lighting stage is part of
        // `suffix`, which is always present.
        //
        // Splicing here rather than from the material composition is what keeps
        // the two definitions singular: if the composed program also carried
        // them, a scene using it would declare each twice and fail to compile.
        // The cost is ~150 lines of WGSL in shaders that never call them, which
        // the shader compiler dead-strips; the alternative — splitting the layer
        // so two of its functions live somewhere else — would put one source
        // section in two files for a benefit the compiler already provides.
        crate::material_shader::cloth::CLOTH_WGSL,
        program,
        suffix,
    ]
    .concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface_program::params::MAX_SURFACE_PARAMS;

    #[test]
    fn the_uniform_region_the_prelude_declares_is_the_one_the_layout_lays_out() {
        // The array length is the parameter cap. If `MAX_SURFACE_PARAMS` moves
        // and this text does not, a program would index past its region — so the
        // two are pinned together here rather than merely agreeing by habit.
        assert_eq!(MAX_SURFACE_PARAMS, 32);
        assert!(
            SURFACE_PRELUDE_WGSL.contains("slots: array<vec4<f32>, 32>"),
            "the prelude's parameter array must be MAX_SURFACE_PARAMS long"
        );
    }

    #[test]
    fn the_prelude_declares_the_whole_calling_convention() {
        ["struct SurfaceIn", "struct SurfaceParams", "struct SurfaceOut"]
            .iter()
            .for_each(|decl| {
                assert!(SURFACE_PRELUDE_WGSL.contains(decl), "missing {decl}");
            });
        // The six channels of the fixed result struct, in order.
        [
            "base_color: vec4<f32>",
            "roughness: f32",
            "metallic: f32",
            "normal: vec3<f32>",
            "emission: vec3<f32>",
            "opacity: f32",
        ]
        .iter()
        .for_each(|field| {
            assert!(SURFACE_PRELUDE_WGSL.contains(field), "missing {field}");
        });
    }

    #[test]
    fn the_prelude_carries_every_helper_the_emitter_calls() {
        ["fn axiom_noise(", "fn axiom_fbm(", "fn axiom_hash_cell("]
            .iter()
            .for_each(|helper| {
                assert!(SURFACE_PRELUDE_WGSL.contains(helper), "missing {helper}");
            });
        // The FNV-1a constants, as the halves a `vec2<u32>` carries them in.
        assert!(SURFACE_PRELUDE_WGSL.contains("vec2<u32>(0xcbf29ce4u, 0x84222325u)"));
        assert!(SURFACE_PRELUDE_WGSL.contains("vec2<u32>(0x00000100u, 0x000001b3u)"));
    }

    #[test]
    fn the_default_program_is_the_identity_over_the_instance_lanes() {
        assert!(DEFAULT_SURFACE_WGSL.contains("fn axiom_surface(in: SurfaceIn, params: SurfaceParams) -> SurfaceOut"));
        assert!(DEFAULT_SURFACE_WGSL.contains("out.base_color = in.albedo;"));
        assert!(DEFAULT_SURFACE_WGSL.contains("out.emission = in.emissive;"));
        assert!(DEFAULT_SURFACE_WGSL.contains("out.opacity = in.albedo.w;"));
    }

    #[test]
    fn the_splice_is_concatenation_in_order_and_is_byte_stable() {
        let spliced = scene_shader("PREFIX\n", "DISPLACE\n", "PROGRAM\n", "SUFFIX\n");
        assert!(spliced.ends_with("SUFFIX\n"));
        let prelude_at = spliced
            .find("struct SurfaceIn")
            .expect("the prelude is spliced in");
        let displace_at = spliced.find("DISPLACE\n").expect("the vertex program");
        let prefix_at = spliced.find("PREFIX\n").expect("the scene's first half");
        let program_at = spliced.find("PROGRAM\n").expect("the fragment program");
        // The order WGSL's declaration-before-use rule forces: vocabulary,
        // then the vertex program `vs` calls, then `vs` itself, then the
        // fragment program `fs` calls, then `fs`.
        assert!(prelude_at < displace_at);
        assert!(displace_at < prefix_at);
        assert!(prefix_at < program_at);
        // Cloth sits between the prefix and the program: the fragment stage's
        // lighting maths (in the suffix) calls `axiom_cloth_light` and
        // `axiom_cloth_transmitted`, and WGSL requires declaration before use.
        let cloth_at = spliced
            .find("fn axiom_cloth_light(")
            .expect("the cloth layer must be spliced into every scene shader");
        assert!(prefix_at < cloth_at);
        assert!(cloth_at < program_at);
        assert_eq!(
            spliced,
            scene_shader("PREFIX\n", "DISPLACE\n", "PROGRAM\n", "SUFFIX\n")
        );
        // Length is the whole splice, cloth included. The concatenation must
        // add nothing of its own — no separator, no trailing newline — because
        // a byte the splice invents is a byte no one authored and no test pins.
        assert_eq!(
            spliced.len(),
            SURFACE_PRELUDE_WGSL.len()
                + "DISPLACE\n".len()
                + "PREFIX\n".len()
                + crate::material_shader::cloth::CLOTH_WGSL.len()
                + "PROGRAM\n".len()
                + "SUFFIX\n".len()
        );
    }

    #[test]
    fn the_default_vertex_program_is_the_exact_zero_offset() {
        assert!(DEFAULT_DISPLACE_WGSL.contains(
            "fn axiom_displace(pos: vec3<f32>, nrm: vec3<f32>, uv: vec2<f32>, t: f32, \
             params: SurfaceParams) -> vec3<f32>"
        ));
        assert!(DEFAULT_DISPLACE_WGSL.contains("return vec3<f32>(0.0, 0.0, 0.0);"));
    }
}
