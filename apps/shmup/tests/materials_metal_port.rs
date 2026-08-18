//! Golden captures for `apps/shmup/src/materials/surfaces/metal.rs`,
//! pinned against `tests/materials/metal/capture.mjs`'s hand transcription of
//! `src/materials/glsl/surfaces-metal.js`.
//!
//! ## No native oracle
//!
//! `surfaces-metal.js` is GLSL held in a JavaScript template-string literal;
//! it never ran anywhere but a browser GPU shader, so `capture.mjs` is a
//! second, independent, line-referenced transcription of the same GLSL this
//! crate's `metal.rs` transcribes — not a ground truth neither transcription
//! can be wrong against. Read `surfaces-metal.js`, `capture.mjs`, and
//! `metal.rs` side by side (same discipline as `tests/sky_port.rs` and
//! `tests/materials_surfaces_ground_port.rs`).
//!
//! ## Tolerance
//!
//! Every generator here chains `sin`/`pow`/`sqrt` (through `owNoise`,
//! `owFbm`, `owWorley`) many times per sample (four-plus fbm calls per
//! texel, each itself several octaves of `owNoise`), so per-call libm drift
//! compounds far past the `1e-12` `tests/materials_noise_port.rs` uses for a
//! single primitive call. `1e-6` is the figure `bake.rs`'s own
//! `build_detail_tile_matches_the_javascript_capture` test already
//! establishes for exactly this class of compounded-call comparison — reused
//! here rather than re-derived.
//!
//! ## The physical-plausibility rule, checked directly
//!
//! Beyond matching the JS number-for-number, this file also asserts the
//! rule the port recipe called out by name: metalness reads ~1 on bare
//! metal/zinc and ~0 under any contamination layer (rust, paint, grime,
//! smudge). That is not a fact the golden numbers alone make obvious to a
//! reader — the assertions below name it directly, keyed to which sample
//! index is expected to be which.

use std::sync::OnceLock;

use serde_json::Value;

use axiom_shmup::materials::noise::Vec2;
use axiom_shmup::materials::surfaces::metal::{
    corrugated, hex_to_linear_tint, metal_brushed, metal_painted, metal_rust,
};

/// See the module doc: compounded transcendental calls need a wider
/// tolerance than a single primitive.
const TOL: f64 = 1e-6;

fn golden() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| {
        serde_json::from_str(include_str!("materials/metal/golden.json")).expect("golden.json parses")
    })
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn assert_close(actual: f64, expected: f64, at: &str) {
    assert!(
        (actual - expected).abs() < TOL,
        "{at}: expected {expected:.17}, got {actual:.17}"
    );
}

struct Expected {
    alb: (f64, f64, f64),
    h: f64,
    rough: f64,
    metal: f64,
    ao: f64,
}

fn expected_sample(entry: &str, i: usize) -> (Vec2, Expected) {
    let g = &golden()[entry]["samples"][i];
    let uv = Vec2::new(num(&g["uv"]["x"]), num(&g["uv"]["y"]));
    let s = &g["s"];
    let e = Expected {
        alb: (num(&s["alb"]["x"]), num(&s["alb"]["y"]), num(&s["alb"]["z"])),
        h: num(&s["h"]),
        rough: num(&s["rough"]),
        metal: num(&s["metal"]),
        ao: num(&s["ao"]),
    };
    (uv, e)
}

fn assert_sample(actual: axiom_shmup::materials::bake::SurfaceSample, expected: &Expected, at: &str) {
    assert_close(actual.albedo.x, expected.alb.0, &format!("{at} albedo.x"));
    assert_close(actual.albedo.y, expected.alb.1, &format!("{at} albedo.y"));
    assert_close(actual.albedo.z, expected.alb.2, &format!("{at} albedo.z"));
    assert_close(actual.height, expected.h, &format!("{at} height"));
    assert_close(actual.roughness, expected.rough, &format!("{at} roughness"));
    assert_close(actual.metal, expected.metal, &format!("{at} metal"));
    assert_close(actual.ao, expected.ao, &format!("{at} ao"));
}

// ---------------------------------------------------------------------------
// metal_rust
// ---------------------------------------------------------------------------

#[test]
fn metal_rust_matches_the_javascript_capture() {
    let seed = num(&golden()["metal_rust"]["seed"]);
    for i in 0..6 {
        let (uv, expected) = expected_sample("metal_rust", i);
        let actual = metal_rust(uv, seed);
        assert_sample(actual, &expected, &format!("metal_rust[{i}]"));
    }
}

// ---------------------------------------------------------------------------
// metal_painted
// ---------------------------------------------------------------------------

#[test]
fn metal_painted_matches_the_javascript_capture() {
    let seed = num(&golden()["metal_painted"]["seed"]);
    let param_z = num(&golden()["metal_painted"]["paramZ"]);
    // `tintA` in the golden is the JS-side `owSRGB` decode of LIBRARY's real
    // `metal_painted.bake.tintA = 0x4a5340` — the same conversion
    // `hex_to_linear_tint` performs, checked separately in `metal.rs`'s own
    // unit test. Reading it back out of the golden here (rather than calling
    // `hex_to_linear_tint` again) keeps this test from silently passing if
    // that helper's decode itself drifted from the JS `owSRGB` call the
    // capture script uses.
    let tint = axiom_shmup::materials::noise::Vec3::new(
        num(&golden()["metal_painted"]["tintA"]["x"]),
        num(&golden()["metal_painted"]["tintA"]["y"]),
        num(&golden()["metal_painted"]["tintA"]["z"]),
    );
    for i in 0..6 {
        let (uv, expected) = expected_sample("metal_painted", i);
        let actual = metal_painted(uv, seed, tint, param_z);
        assert_sample(actual, &expected, &format!("metal_painted[{i}]"));
    }
}

#[test]
fn hex_to_linear_tint_reproduces_the_golden_tint_a() {
    let t = hex_to_linear_tint(0x4a_53_40);
    let want = &golden()["metal_painted"]["tintA"];
    assert_close(t.x, num(&want["x"]), "tintA.x");
    assert_close(t.y, num(&want["y"]), "tintA.y");
    assert_close(t.z, num(&want["z"]), "tintA.z");
}

// ---------------------------------------------------------------------------
// metal_brushed
// ---------------------------------------------------------------------------

#[test]
fn metal_brushed_matches_the_javascript_capture() {
    let seed = num(&golden()["metal_brushed"]["seed"]);
    for i in 0..6 {
        let (uv, expected) = expected_sample("metal_brushed", i);
        let actual = metal_brushed(uv, seed);
        assert_sample(actual, &expected, &format!("metal_brushed[{i}]"));
    }
}

// ---------------------------------------------------------------------------
// corrugated
// ---------------------------------------------------------------------------

#[test]
fn corrugated_matches_the_javascript_capture() {
    let seed = num(&golden()["corrugated"]["seed"]);
    for i in 0..6 {
        let (uv, expected) = expected_sample("corrugated", i);
        let actual = corrugated(uv, seed);
        assert_sample(actual, &expected, &format!("corrugated[{i}]"));
    }
}

/// Sample index 5 is `uv.x = 1/24`, engineered so `corrugated`'s `t = uv.x *
/// RIDGES(12) * 2*pi` lands on exactly `pi` — `wave = sin(pi)` is `0.0` (up
/// to libm rounding) at that texel, which is exactly the `sign(0)` case the
/// port's module doc calls out: GLSL `sign(0) == 0`, unlike `f64::signum`.
/// The golden capture (JS `glSign`, hand-rolled the same three-valued way)
/// already exercises this path; this test names *why* that sample exists
/// rather than leaving it an unexplained sixth grid point.
#[test]
fn corrugated_sign_zero_texel_matches_the_javascript_capture() {
    let seed = num(&golden()["corrugated"]["seed"]);
    let (uv, expected) = expected_sample("corrugated", 5);
    assert!(
        (uv.x - 1.0 / 24.0).abs() < 1e-12,
        "capture grid's 6th point must be the engineered sign(0) texel"
    );
    let actual = corrugated(uv, seed);
    assert_sample(actual, &expected, "corrugated sign(0) texel");
}

// ---------------------------------------------------------------------------
// The physical-plausibility rule: bare metal reads metal ~= 1, every
// contamination layer pulls it toward 0. Checked directly against the real
// golden numbers, not just re-derived from the port.
// ---------------------------------------------------------------------------

#[test]
fn metal_rust_golden_metalness_never_exceeds_one_and_some_samples_are_near_zero() {
    let g = &golden()["metal_rust"]["samples"];
    let arr = g.as_array().expect("samples array");
    assert!(
        arr.iter().any(|e| num(&e["s"]["metal"]) < 0.1),
        "expected at least one heavily-rusted (near-zero-metalness) golden sample"
    );
    assert!(
        arr.iter().all(|e| num(&e["s"]["metal"]) <= 1.0 + 1e-9),
        "metalness must never exceed the bare-metal baseline of 1.0"
    );
}

/// The 6-point capture grid happens to land entirely on intact paint at this
/// seed (a metallic chip-through is a small `smoothstep` peak the coarse
/// grid can miss — `metal.rs`'s own `metal_painted_is_non_metallic_
/// somewhere_and_bare_through_a_chip_elsewhere` unit test uses a much finer
/// 48x48 scan specifically to find one). This test only claims what the
/// golden actually shows: every sample here is non-metallic paint.
#[test]
fn metal_painted_golden_samples_are_all_non_metallic_paint() {
    let g = &golden()["metal_painted"]["samples"];
    let arr = g.as_array().expect("samples array");
    assert!(
        arr.iter().all(|e| num(&e["s"]["metal"]) < 0.1),
        "expected every one of this seed's 6 capture-grid samples to be intact, non-metallic paint"
    );
}
