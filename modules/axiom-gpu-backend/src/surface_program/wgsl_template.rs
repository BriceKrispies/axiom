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
/// [`crate::surface_program::params`] lays out. `SurfaceOut` is the six-channel
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
    // Presentation time in seconds. No frame time reaches this pass yet, so the
    // main pass fills it with zero and a surface reading it fails the capability
    // gate rather than being lowered against a frozen clock.
    time: f32,
    // What the existing pipeline already resolved for this fragment: the sampled
    // albedo times the per-vertex and per-instance colour. The DEFAULT program
    // returns it unchanged, which is what makes every existing app pixel-identical.
    albedo: vec4<f32>,
    // The draw's material emissive, from the instance stream.
    emissive: vec3<f32>,
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
"#;

/// The program a draw naming `surface_program == 0` runs.
///
/// It is the **identity**: every channel takes the value the existing pipeline
/// already resolved, so the frame the main pass produces with this spliced in is
/// the frame it produced before there was a splice at all. The unbound-channel
/// values (`roughness`, `metallic`, `normal`) are
/// `axiom_surface::SurfaceChannel::default_value`'s, which nothing in the
/// lighting model reads yet.
///
/// `params` is unread here, and is unread by *every* program the main pass runs
/// today: this backend binds no parameter buffer yet, so the main pass hands the
/// zero value. Binding it is pipeline work.
pub(crate) const DEFAULT_SURFACE_WGSL: &str = r#"
fn axiom_surface(in: SurfaceIn, params: SurfaceParams) -> SurfaceOut {
    var out: SurfaceOut;
    out.base_color = in.albedo;
    out.roughness = 0.5;
    out.metallic = 0.0;
    out.normal = vec3<f32>(0.0, 0.0, 1.0);
    out.emission = in.emissive;
    out.opacity = in.albedo.w;
    return out;
}
"#;

/// The main pass's whole shader source: the scene WGSL's first half, the fixed
/// surface vocabulary, one generated program, then the scene WGSL's second half.
///
/// Concatenation, never substitution — the precedent is
/// [`crate::surface_encode::shader_source`], and the property that matters is
/// that the same `program` always yields the same bytes, which is what makes the
/// string cacheable by the surface's digest.
pub(crate) fn scene_shader(prefix: &str, program: &str, suffix: &str) -> String {
    [prefix, SURFACE_PRELUDE_WGSL, program, suffix].concat()
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
        let spliced = scene_shader("PREFIX\n", "PROGRAM\n", "SUFFIX\n");
        assert!(spliced.starts_with("PREFIX\n"));
        assert!(spliced.ends_with("SUFFIX\n"));
        let program_at = spliced.find("PROGRAM\n").expect("the program is spliced in");
        let prelude_at = spliced
            .find("struct SurfaceIn")
            .expect("the prelude is spliced in");
        assert!(
            prelude_at < program_at,
            "a program is written against the vocabulary, so the vocabulary comes first"
        );
        assert_eq!(spliced, scene_shader("PREFIX\n", "PROGRAM\n", "SUFFIX\n"));
        assert_eq!(
            spliced.len(),
            "PREFIX\n".len() + SURFACE_PRELUDE_WGSL.len() + "PROGRAM\n".len() + "SUFFIX\n".len()
        );
    }
}
