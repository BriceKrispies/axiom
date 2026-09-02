//! The nineteen generators as WGSL — the shader half of the GPU bake.
//!
//! Ported from Claude-of-Duty `src/materials/glsl/`: `noise.js` plus the four
//! `surfaces-*.js` files, transcribed from the GLSL text into WGSL. The bake
//! machinery that runs them (render targets, the four passes, the Sobel, the
//! read-back) is `axiom_gpu_backend`'s `texture_bake`, which is the same seam
//! `generator.js` draws between the `TextureForge` and the GLSL it bakes.
//!
//! ## Why this exists — and why it is not the field algebra
//!
//! `01-engine-gaps.md` records the load-bearing decision as *"bake-time texture
//! generation belongs in the field/proc-texture path … the 19 procedural
//! surface generators are straight-line noise math with no sampling and no
//! derivatives"*. Having now read all 1,885 lines of the GLSL, **that
//! characterisation does not hold**, and the decision it supports has to be
//! revised rather than executed:
//!
//! * `owWorley` is a 3×3 loop carrying an F1/F2 comparison chain that also
//!   selects a `vec2` payload; `owVoronoiEdge` is a **two-pass** 3×3 + 5×5 loop
//!   whose second pass is centred on a cell the first pass *found at runtime*;
//!   `FOLIAGE` is a 3×3 loop with nested `if (cover > 0.01) { if (depth >
//!   bestDepth) …}` carrying five accumulators.
//! * `owFbm`/`owRidged`/`owBillow` divide (`s / max(n, 1e-4)`), as do `owRemap`
//!   and `owSRGB`.
//! * The whole library is periodic, which means `floor`, `fract` and GLSL
//!   `mod` on every lattice access.
//!
//! The field algebra has, deliberately and by name, no loops, no comparison or
//! selection operator, no division, and no `floor`/`fract`/`mod` at all
//! (`axiom_field::FieldOp`'s catalog and its excluded-operator table). Its
//! `Fbm` is 3D non-periodic FNV-1a gradient noise with no period parameter, so
//! it cannot even stand in for `owFbm`. And the budget is 256 nodes across a
//! *whole surface*: a static count of the fully inlined, fully unrolled scalar
//! expression graph puts these generators at **2.1 k to 43.4 k nodes**.
//!
//! So this is hand-written WGSL, for the same reason `material_shader/` is —
//! and `axiom_surface::SurfaceKind::RuntimeMaterial`'s own module doc already
//! names that reason: *"the algebra has no loops, no derivatives and no
//! sampling, and its branchlessness is the Branchless Law itself, so those
//! absences are immovable."* Raising the node budget (gap G15) would not help;
//! the missing operators are semantics, not size.
//!
//! ## What checks what
//!
//! Three independent readings of the same GLSL now exist:
//!
//! 1. `materials::noise` + `materials::surfaces::*` — the CPU library in `f64`,
//!    already golden-pinned against captures from the original JavaScript;
//! 2. this WGSL, transcribed separately from the GLSL text;
//! 3. `materials::bake` (CPU) and `axiom_gpu_backend::texture_bake` (GPU),
//!    likewise a pair.
//!
//! `apps/shmup/tests/materials_gpu_bake_port.rs` runs (2)+(3) against (1)+(3).
//! A disagreement is a real finding in one of them, which is the whole reason
//! neither was written from the other.
//!
//! ## The three renames
//!
//! `macro` is a **reserved word** in WGSL and is a hard parse error. Six source
//! locals are called `macro` and are renamed at every use: `macro_` in
//! [`arch::CONCRETE`], [`arch::PLASTER`], [`ground::ASPHALT`] and
//! [`ground::DIRT`]; `macroNoise` in [`metal::METAL_BRUSHED`]; `macroF` in
//! [`organic::FABRIC`], [`organic::BURLAP`] and [`organic::RUBBER`]. Nothing
//! else is renamed, and no expression is regrouped.
//!
//! ## Where the shader text lives
//!
//! Each generator's WGSL is a sibling `.wgsl` file — `arch.rs`'s `CONCRETE` is
//! `concrete.wgsl` — pulled in with `include_str!`, so the constant is still a
//! `&'static str` known at compile time and `library_wgsl` still concatenates
//! them. The Rust files keep the doc comment for each generator, which is where
//! the `surfaces-*.js:NNN` provenance lives.
//!
//! Shader text sitting in a `.wgsl` file rather than a Rust string literal is
//! the difference between text a WGSL formatter, highlighter and validator can
//! read and text none of them can. Two consequences worth knowing:
//!
//! * `.gitattributes` pins `*.wgsl` to `eol=lf`, and that is load-bearing, not
//!   cosmetic. Rust's lexer folds a CRLF inside a string literal to a single
//!   LF; `include_str!` folds nothing. A CRLF checkout of these files would
//!   hand the compiler a different string than the literal held, silently.
//! * `ax wgsl <path> --verify` re-checks that every one of these files is still
//!   byte-identical to the literal it replaced, against any revision.
//!
//! The extraction itself was mechanical (`ax wgsl --apply`), not retyped.
//!
//! ## None of these files is a compilable unit, and that is correct
//!
//! A surface body reaches for the bake header's `U`; `noise.wgsl` calls
//! `owMod2`, which `gl_semantics.wgsl` declares. Opened alone, every one of
//! them fails to parse. Making each whole would mean copying the 11,400-byte
//! prelude (header + library + footer) into all eighteen — 72% boilerplate and
//! the noise library in eighteen copies — so the complete unit is the
//! *program*, and `axiom_gpu_backend`'s `GpuBackendApi::bake_program_wgsl` is
//! what composes it.
//!
//! `tests/materials_wgsl_validates.rs` is the gate that follows from that: it
//! composes all eighteen and runs `naga` over them, with **no GPU** — the first
//! proof these compile that can run on a machine without an adapter, where
//! `every_generator_compiles_and_bakes` needs a real device. It also writes each
//! composed program to `target/wgsl-programs/`, which is the only place a
//! complete, LSP-readable version of these shaders exists.

pub mod arch;
pub mod ground;
pub mod metal;
pub mod noise;
pub mod organic;

/// The shared library every generator is compiled against:
/// `GL_SEMANTICS + NOISE_GLSL + RUST_HELPERS`, which is the source's
/// `NOISE_GLSL + RUST_HELPERS` (`generator.js:224`) with the GLSL-builtin shims
/// prepended.
///
/// `GL_SEMANTICS` has no counterpart in the source because GLSL *is* the
/// source's semantics; see [`noise::GL_SEMANTICS`] for why `mix`, `clamp`,
/// `step`, `smoothstep`, `mod` and `sign` are written out rather than taken
/// from WGSL.
///
/// The engine supplies `HEADER` and `FOOTER` around this
/// (`axiom_gpu_backend`'s `bake_program_wgsl`), so the four-part splice the
/// source performs is reproduced exactly.
pub fn library_wgsl() -> String {
    [noise::GL_SEMANTICS, noise::NOISE, metal::RUST_HELPERS].concat()
}

/// `def.glsl` → the transcribed `owSurface` body, dispatched on the generator's
/// **name**.
///
/// This is the exact twin of `materials::system::sample_surface`, deliberately
/// including its dispatch-on-name shape and its arm order: the two must resolve
/// the same eighteen names to the same eighteen bodies, and
/// `the_gpu_and_cpu_dispatch_tables_name_the_same_generators` pins that they do.
/// `concrete` and `concrete_floor` share one body selected by
/// `uParam.x`/`uParam.y`, exactly as the source does.
///
/// # Panics
///
/// On a generator name no `wgsl` module implements — the same contract, and the
/// same wording, `sample_surface` has.
pub fn generator_wgsl(generator: &str) -> &'static str {
    match generator {
        "concrete" => arch::CONCRETE,
        "brick" => arch::BRICK,
        "plaster" => arch::PLASTER,
        "tile" => arch::TILE,
        "asphalt" => ground::ASPHALT,
        "sand" => ground::SAND,
        "dirt" => ground::DIRT,
        "gravel" => ground::GRAVEL,
        "metal_rust" => metal::METAL_RUST,
        "metal_painted" => metal::METAL_PAINTED,
        "metal_brushed" => metal::METAL_BRUSHED,
        "corrugated" => metal::CORRUGATED,
        "wood" => organic::WOOD,
        "fabric" => organic::FABRIC,
        "burlap" => organic::BURLAP,
        "foliage" => organic::FOLIAGE,
        "rubber" => organic::RUBBER,
        "glass" => organic::GLASS,
        other => panic!("materials: no owSurface WGSL named \"{other}\""),
    }
}

/// Every generator name [`generator_wgsl`] answers to, in dispatch order.
pub const GENERATOR_NAMES: [&str; 18] = [
    "concrete",
    "brick",
    "plaster",
    "tile",
    "asphalt",
    "sand",
    "dirt",
    "gravel",
    "metal_rust",
    "metal_painted",
    "metal_brushed",
    "corrugated",
    "wood",
    "fabric",
    "burlap",
    "foliage",
    "rubber",
    "glass",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::materials::noise::{Vec2, Vec3, Vec4};
    use crate::materials::system::sample_surface;

    #[test]
    fn every_generator_name_resolves_to_a_body() {
        GENERATOR_NAMES.iter().for_each(|name| {
            let body = generator_wgsl(name);
            assert!(
                body.contains("fn owSurface(uv: vec2<f32>"),
                "{name} must define owSurface"
            );
            assert!(
                body.contains("*albOut = alb;") && body.contains("*aoOut = ao;"),
                "{name} must write back all five out-parameters"
            );
        });
    }

    #[test]
    fn the_gpu_and_cpu_dispatch_tables_name_the_same_generators() {
        // `sample_surface` is the CPU twin; if one grows an arm the other must.
        GENERATOR_NAMES.iter().for_each(|name| {
            let sample = sample_surface(
                name,
                Vec2::new(0.25, 0.75),
                1.0,
                Vec3::new(1.0, 1.0, 1.0),
                Vec3::new(1.0, 1.0, 1.0),
                Vec4::new(0.0, 0.0, 0.0, 0.0),
            );
            assert!(
                sample.height.is_finite(),
                "the CPU twin must answer to {name} too"
            );
        });
    }

    #[test]
    #[should_panic(expected = "no owSurface WGSL named \"nope\"")]
    fn an_unknown_generator_is_a_panic_not_a_fallback() {
        let _ = generator_wgsl("nope");
    }

    #[test]
    fn the_library_is_the_sources_three_part_concatenation() {
        let library = library_wgsl();
        let semantics = library.find("fn owMod(").expect("the GLSL shims");
        let noise_at = library.find("fn owHash11(").expect("the noise library");
        let helpers = library.find("fn owRustColour(").expect("RUST_HELPERS");
        assert!(
            semantics < noise_at && noise_at < helpers,
            "shims, then NOISE_GLSL, then RUST_HELPERS (generator.js:224): \
             {semantics} {noise_at} {helpers}"
        );
    }

    #[test]
    fn the_library_declares_every_helper_the_generators_call() {
        let library = library_wgsl();
        let bodies: String = GENERATOR_NAMES
            .iter()
            .map(|name| generator_wgsl(name))
            .chain([noise::DETAIL, noise::MACRO])
            .collect();
        // Every `owXxx(` a generator calls must be declared exactly once in the
        // library (or be the generator's own `owSurface`).
        let word_start = |at: usize| {
            at == 0
                || !bodies.as_bytes()[at - 1].is_ascii_alphanumeric()
                    && bodies.as_bytes()[at - 1] != b'_'
        };
        let called: std::collections::BTreeSet<&str> = bodies
            .match_indices("ow")
            .filter(|(at, _)| word_start(*at))
            .filter_map(|(at, _)| {
                let tail = &bodies[at..];
                let end = tail.find('(')?;
                let name = &tail[..end];
                // `owFoo(` only — a bare `ow` or an `ow` inside `shadow`/`row`
                // is not a call, and neither is text with a space before the
                // paren.
                (name.len() > 2 && name.chars().all(|c| c.is_ascii_alphanumeric()))
                    .then_some(name)
            })
            .filter(|name| *name != "owSurface")
            .collect();
        assert!(
            !called.is_empty(),
            "the generators must call the noise library at all"
        );
        called.iter().for_each(|name| {
            assert!(
                library.contains(&format!("fn {name}(")),
                "{name} is called by a generator but not declared in the library"
            );
        });
    }

    #[test]
    fn the_shims_keep_glsls_definitions_not_wgsls() {
        // The two that are genuinely not interchangeable with their WGSL
        // namesakes; the rest are written out because WGSL is permitted to
        // factor them differently, not because they differ today.
        assert!(
            noise::GL_SEMANTICS.contains("return x - y * floor(x / y);"),
            "GLSL mod is a floored modulus, and lattice coordinates go negative"
        );
        assert!(
            noise::GL_SEMANTICS
                .contains("return select(0.0, -1.0, x < 0.0) + select(0.0, 1.0, x > 0.0);"),
            "GLSL sign returns 0.0 for zero, which CORRUGATED's ridges rely on"
        );
    }

    #[test]
    fn no_generator_uses_a_wgsl_reserved_word_as_a_local() {
        // `macro` is the one collision the transcription hit; it is renamed at
        // every use. This pins that no un-renamed one crept back in.
        GENERATOR_NAMES
            .iter()
            .map(|name| (*name, generator_wgsl(name)))
            .chain([("detail", noise::DETAIL), ("macro-map", noise::MACRO)])
            .for_each(|(name, body)| {
                ["let macro ", "var macro ", "macro =", "macro;", "macro +"]
                    .iter()
                    .for_each(|needle| {
                        assert!(
                            !body.contains(needle),
                            "{name} still spells the reserved word `macro` ({needle})"
                        );
                    });
            });
    }

    #[test]
    fn the_two_shared_maps_are_the_sources_inline_generators() {
        // generator.js:91-120 and 126-138 — the only two owSurface bodies the
        // source defines in generator.js rather than in glsl/.
        assert!(noise::DETAIL.contains("owWorley(p * 20.0, P * 20.0, 1.0)"));
        assert!(noise::DETAIL.contains("let P = vec2<f32>(8.0);"));
        assert!(noise::MACRO.contains("let P = vec2<f32>(6.0);"));
        assert!(
            noise::MACRO.contains("owWarp(p * 1.0, P, 1.1, 3)"),
            "the macro map's G band is a warped fbm"
        );
    }
}
