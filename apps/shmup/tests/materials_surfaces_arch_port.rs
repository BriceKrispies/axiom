//! The four architectural `owSurface` generators
//! (`src/materials/surfaces/arch.rs`), pinned against a from-scratch JS
//! transcription of the GLSL they were ported from.
//!
//! **This is a weaker oracle than the rest of the port.** `surfaces-arch.js`
//! embeds each `owSurface` body as GLSL inside a JS template literal (like
//! `noise.js`/`generator.js`) — there is no importable JS function to call as
//! ground truth. `tests/materials_arch/capture.mjs` re-implements
//! `concrete`/`brick`/`plaster`/`tile` in plain JS doubles, transcribed
//! line-by-line directly from `C:/dev/Claude-of-Duty/src/materials/glsl/
//! surfaces-arch.js`, independently of the Rust port in `arch.rs` (not
//! derived from it), so this is a two-independent-transcriptions check
//! rather than a port-checked-against-a-real-module check. See
//! `docs/work-manifests/claude-of-duty-port/notes/materials-surfaces-arch.md`
//! for the full caveat and `tests/materials_arch/capture.mjs`'s own header.
//!
//! `concrete_surface` is pinned three ways: the library's real `concrete`
//! params (seed 11, `param = [1, 0, 0, 0]`, board-formed wall), its real
//! `concrete_floor` params (seed 47, `param = [0, 1, 0, 0]`, saw-cut slab),
//! and a third "neither flag" variant (seed 11, `param = [0, 0, 0, 0]`) that
//! exercises the formwork/joint terms with both amounts at zero — the source
//! has no such library entry, but zeroing `uParam` is a real, reachable input
//! to the shader (any surface using the `concrete` generator with a param
//! this port hasn't wired to a `LIBRARY` entry yet would hit exactly this
//! path), and it is the only one of the three that stresses the `formAmt ==
//! 0 && jointAmt == 0` branch-free arithmetic (every `* formAmt`/`*
//! jointAmt` term collapsing to zero) rather than always multiplying by 1.
//!
//! ## Tolerance
//!
//! Every generator chains `owFbm01`/`owWorley`/`owWarp`/`owCracks`/`owSRGB` —
//! all built on `sin`/`cos`/`sqrt`/`pow` — many times per sample (concrete
//! alone evaluates well over a dozen noise calls per texel), so this uses a
//! wider tolerance than the `1e-12` single-transcendental-call figure
//! established in `tests/core_port.rs`: **`1e-7`**, still many orders tighter
//! than anything visually distinguishable in a baked texture, but loose
//! enough to absorb compounded libm cross-implementation drift across a long
//! arithmetic chain (the same reasoning `materials/bake.rs`'s `1e-6` tile
//! test gives for its 9-sample Sobel stencil, widened slightly further for
//! chains several times longer).

use std::sync::OnceLock;

use serde_json::Value;

use axiom_shmup::materials::bake::SurfaceSample;
use axiom_shmup::materials::surfaces::arch::{
    brick_surface, concrete_surface, plaster_surface, tile_surface,
};
use axiom_shmup::materials::noise::{Vec2, Vec4};

fn golden() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| {
        serde_json::from_str(include_str!("materials_arch/golden.json")).expect("golden.json parses")
    })
}

const TOL: f64 = 1e-7;

fn assert_close(actual: f64, expected: f64, at: &str) {
    assert!(
        (actual - expected).abs() < TOL,
        "{at}: expected {expected:.17}, got {actual:.17}"
    );
}

/// The fixed uv grid `capture.mjs::pts()` samples — kept in the same order so
/// index `i` here matches golden sample `i`.
fn pts() -> [Vec2; 6] {
    [
        Vec2::new(0.02, 0.02),
        Vec2::new(0.13, 0.77),
        Vec2::new(0.42, 0.09),
        Vec2::new(0.65, 0.34),
        Vec2::new(0.91, 0.36),
        Vec2::new(0.99, 0.99),
    ]
}

fn assert_sample_matches(sample: SurfaceSample, expected: &Value, at: &str) {
    assert_close(sample.albedo.x, expected["alb"]["x"].as_f64().unwrap(), &format!("{at} albedo.x"));
    assert_close(sample.albedo.y, expected["alb"]["y"].as_f64().unwrap(), &format!("{at} albedo.y"));
    assert_close(sample.albedo.z, expected["alb"]["z"].as_f64().unwrap(), &format!("{at} albedo.z"));
    assert_close(sample.height, expected["h"].as_f64().unwrap(), &format!("{at} height"));
    assert_close(sample.roughness, expected["rough"].as_f64().unwrap(), &format!("{at} roughness"));
    assert_close(sample.metal, expected["metal"].as_f64().unwrap(), &format!("{at} metal"));
    assert_close(sample.ao, expected["ao"].as_f64().unwrap(), &format!("{at} ao"));
}

// ============================================================================
// concrete / concrete_floor
// ============================================================================

#[test]
fn concrete_wall_matches_the_javascript_transcription() {
    let g = golden();
    let entry = &g["concrete_wall"];
    let seed = entry["seed"].as_f64().unwrap();
    let samples = entry["samples"].as_array().unwrap();
    let param = Vec4::new(1.0, 0.0, 0.0, 0.0);

    for (i, uv) in pts().into_iter().enumerate() {
        let sample = concrete_surface(uv, seed, param);
        assert_sample_matches(sample, &samples[i], &format!("concrete_wall[{i}]"));
    }
}

#[test]
fn concrete_floor_matches_the_javascript_transcription() {
    let g = golden();
    let entry = &g["concrete_floor"];
    let seed = entry["seed"].as_f64().unwrap();
    let samples = entry["samples"].as_array().unwrap();
    let param = Vec4::new(0.0, 1.0, 0.0, 0.0);

    for (i, uv) in pts().into_iter().enumerate() {
        let sample = concrete_surface(uv, seed, param);
        assert_sample_matches(sample, &samples[i], &format!("concrete_floor[{i}]"));
    }
}

/// Not a `LIBRARY` entry — `param = [0, 0, 0, 0]` zeroes both `formAmt` and
/// `jointAmt`, the one combination the two real library entries never
/// exercise (`concrete` always has `formAmt = 1`, `concrete_floor` always
/// has `jointAmt = 1`). See the module doc for why this is worth pinning
/// anyway.
#[test]
fn concrete_with_neither_param_flag_matches_the_javascript_transcription() {
    let g = golden();
    let entry = &g["concrete_neither"];
    let seed = entry["seed"].as_f64().unwrap();
    let samples = entry["samples"].as_array().unwrap();
    let param = Vec4::new(0.0, 0.0, 0.0, 0.0);

    for (i, uv) in pts().into_iter().enumerate() {
        let sample = concrete_surface(uv, seed, param);
        assert_sample_matches(sample, &samples[i], &format!("concrete_neither[{i}]"));
    }
}

// ============================================================================
// brick / plaster / tile
// ============================================================================

#[test]
fn brick_matches_the_javascript_transcription() {
    let g = golden();
    let entry = &g["brick"];
    let seed = entry["seed"].as_f64().unwrap();
    let samples = entry["samples"].as_array().unwrap();

    for (i, uv) in pts().into_iter().enumerate() {
        let sample = brick_surface(uv, seed);
        assert_sample_matches(sample, &samples[i], &format!("brick[{i}]"));
    }
}

#[test]
fn plaster_matches_the_javascript_transcription() {
    let g = golden();
    let entry = &g["plaster"];
    let seed = entry["seed"].as_f64().unwrap();
    let samples = entry["samples"].as_array().unwrap();

    for (i, uv) in pts().into_iter().enumerate() {
        let sample = plaster_surface(uv, seed);
        assert_sample_matches(sample, &samples[i], &format!("plaster[{i}]"));
    }
}

#[test]
fn tile_matches_the_javascript_transcription() {
    let g = golden();
    let entry = &g["tile"];
    let seed = entry["seed"].as_f64().unwrap();
    let samples = entry["samples"].as_array().unwrap();

    for (i, uv) in pts().into_iter().enumerate() {
        let sample = tile_surface(uv, seed);
        assert_sample_matches(sample, &samples[i], &format!("tile[{i}]"));
    }
}

// ============================================================================
// Physical-plausibility clamp bounds — every generator's own documented
// output range, checked against a wide uv sweep rather than the golden's 6
// fixed points, so this is a Rust-only property test (no JS capture needed:
// it follows from `gl_clamp`'s definition, not from a specific value).
// ============================================================================

#[test]
fn every_generator_stays_within_its_documented_clamp_bounds() {
    let steps = 9;
    for i in 0..steps {
        for j in 0..steps {
            let uv = Vec2::new(i as f64 / (steps - 1) as f64, j as f64 / (steps - 1) as f64);

            let concrete = concrete_surface(uv, 11.0, Vec4::new(1.0, 0.0, 0.0, 0.0));
            assert_in_bounds(concrete, 0.02, 0.85, 0.48, 0.98, 0.15, 1.0, "concrete");

            let floor = concrete_surface(uv, 47.0, Vec4::new(0.0, 1.0, 0.0, 0.0));
            assert_in_bounds(floor, 0.02, 0.85, 0.48, 0.98, 0.15, 1.0, "concrete_floor");

            let brick = brick_surface(uv, 23.0);
            assert_in_bounds(brick, 0.02, 0.85, 0.35, 0.99, 0.12, 1.0, "brick");

            let plaster = plaster_surface(uv, 5.0);
            assert_in_bounds(plaster, 0.02, 0.88, 0.35, 0.99, 0.15, 1.0, "plaster");

            let tile = tile_surface(uv, 31.0);
            assert_in_bounds(tile, 0.02, 0.85, 0.12, 0.95, 0.15, 1.0, "tile");
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn assert_in_bounds(
    s: SurfaceSample,
    alb_lo: f64,
    alb_hi: f64,
    rough_lo: f64,
    rough_hi: f64,
    ao_lo: f64,
    ao_hi: f64,
    at: &str,
) {
    for (name, v) in [("albedo.x", s.albedo.x), ("albedo.y", s.albedo.y), ("albedo.z", s.albedo.z)] {
        assert!(
            (alb_lo..=alb_hi).contains(&v),
            "{at} {name} = {v} out of documented clamp [{alb_lo}, {alb_hi}]"
        );
    }
    assert!(
        (0.0..=1.0).contains(&s.height),
        "{at} height = {} out of [0, 1]",
        s.height
    );
    assert!(
        (rough_lo..=rough_hi).contains(&s.roughness),
        "{at} roughness = {} out of documented clamp [{rough_lo}, {rough_hi}]",
        s.roughness
    );
    assert_eq!(s.metal, 0.0, "{at} metal must be exactly 0.0 — no generator in this file sets it");
    assert!(
        (ao_lo..=ao_hi).contains(&s.ao),
        "{at} ao = {} out of documented clamp [{ao_lo}, {ao_hi}]",
        s.ao
    );
}
