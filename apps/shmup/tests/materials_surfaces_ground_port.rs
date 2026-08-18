//! Golden capture for the ported `src/materials/surfaces/ground.rs`
//! (asphalt/sand/dirt/gravel), pinned against
//! `tests/materials_surfaces_ground/golden.json`.
//!
//! **This oracle is hand-written, not genuine.** `surfaces-ground.js` (and the
//! `noise.js` library it builds on) is GLSL held inside JavaScript
//! template-string literals — shader source that never ran anywhere but a
//! browser GPU. There is no JS function to `import` and call, so
//! `tests/materials_surfaces_ground/capture.mjs` hand-transcribes each GLSL
//! body into plain JS doubles, independently of (but line-referenced against,
//! the same as) this crate's Rust transcription in
//! `src/materials/surfaces/ground.rs`. A match between the two catches drift
//! between the Rust port and *a* careful reading of the GLSL — it cannot
//! catch a mistake both transcriptions share. See that script's module doc
//! for the full reasoning (the same situation `tests/sky_port.rs` documents
//! for `sky/`'s shader-only bodies).
//!
//! ## Tolerance
//!
//! **`1e-9` relative**, the same figure `tests/sky_port.rs` uses for its
//! shader-only (no-oracle) half: every generator here chains `sin`/`pow`
//! (asphalt's crack network, sand's ripple profile) which are not
//! bit-guaranteed across V8 and Rust's libm. Tighter than that would be
//! fighting libm cross-implementation drift, not catching real bugs; looser
//! would risk missing a genuine one-term mistake.

use std::sync::OnceLock;

use serde_json::Value;

use axiom_shmup::materials::noise::Vec2;
use axiom_shmup::materials::surfaces::ground::{asphalt, dirt, gravel, sand};

const REL: f64 = 1e-9;

fn golden() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| {
        serde_json::from_str(include_str!("materials_surfaces_ground/golden.json"))
            .expect("golden.json parses")
    })
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn close(actual: f64, expected: f64, what: &str) {
    if actual == expected {
        return;
    }
    let scale = expected.abs().max(1.0);
    assert!(
        (actual - expected).abs() <= REL * scale,
        "{what}: expected {expected:.17e}, got {actual:.17e} (rel {:.3e})",
        (actual - expected).abs() / scale
    );
}

fn uv_of(row: &Value) -> Vec2 {
    Vec2::new(num(&row["uv"][0]), num(&row["uv"][1]))
}

/// Runs one generator's golden `samples` array against the Rust function,
/// checking every field of the `owSurface` out-parameters.
fn check_generator(
    name: &str,
    f: impl Fn(Vec2, f64) -> axiom_shmup::materials::bake::SurfaceSample,
) {
    let g = &golden()[name];
    let seed = num(&g["seed"]);
    for row in g["samples"].as_array().unwrap() {
        let uv = uv_of(row);
        let expected = &row["out"];
        let actual = f(uv, seed);

        close(actual.albedo.x, num(&expected["alb"][0]), &format!("{name}.albedo.x @ {uv:?}"));
        close(actual.albedo.y, num(&expected["alb"][1]), &format!("{name}.albedo.y @ {uv:?}"));
        close(actual.albedo.z, num(&expected["alb"][2]), &format!("{name}.albedo.z @ {uv:?}"));
        close(actual.height, num(&expected["h"]), &format!("{name}.height @ {uv:?}"));
        close(actual.roughness, num(&expected["rough"]), &format!("{name}.roughness @ {uv:?}"));
        assert_eq!(actual.metal, num(&expected["metal"]), "{name}.metal @ {uv:?}");
        close(actual.ao, num(&expected["ao"]), &format!("{name}.ao @ {uv:?}"));
    }
}

#[test]
fn asphalt_matches_the_hand_transcribed_glsl() {
    check_generator("asphalt", asphalt);
}

#[test]
fn sand_matches_the_hand_transcribed_glsl() {
    check_generator("sand", sand);
}

#[test]
fn dirt_matches_the_hand_transcribed_glsl() {
    check_generator("dirt", dirt);
}

#[test]
fn gravel_matches_the_hand_transcribed_glsl() {
    check_generator("gravel", gravel);
}

/// The gravel AO band is the one figure the port recipe calls out as "easy to
/// lose and hard to spot later": on a ground plane, `orm.r` (AO) is very
/// nearly the only shading term, so a wide AO ripple at the aggregate period
/// reads as salt-and-pepper (see `ground.rs`'s `gravel` doc). Checked against
/// a dense 17x17 grid from the *transcription*, not just the Rust port's own
/// re-derivation of the same bound (which `src/materials/surfaces/ground.rs`'s
/// unit test already covers) — this is the independent half of that pin.
#[test]
fn gravel_ao_band_matches_the_transcription_and_stays_in_0_87_to_1_0() {
    let g = &golden()["gravelDenseAo"];
    let seed = num(&golden()["gravel"]["seed"]);
    let rows = g.as_array().unwrap();
    assert_eq!(rows.len(), 17 * 17, "dense grid must be the full 17x17");

    let mut min_seen = f64::INFINITY;
    let mut max_seen = f64::NEG_INFINITY;
    for row in rows {
        let uv = uv_of(row);
        let expected_ao = num(&row["ao"]);
        let actual_ao = gravel(uv, seed).ao;
        close(actual_ao, expected_ao, &format!("gravel.ao (dense) @ {uv:?}"));
        assert!(
            (0.87..=1.0).contains(&actual_ao),
            "gravel ao {actual_ao} at uv {uv:?} escaped the documented 0.87..1.0 band"
        );
        min_seen = min_seen.min(actual_ao);
        max_seen = max_seen.max(actual_ao);
    }
    // Sanity: the band is actually exercised, not trivially satisfied by a
    // constant — the source's own doc describes real variation across it.
    assert!(min_seen < 0.99, "gravel AO grid never dipped below 0.99: {min_seen}");
    assert!(max_seen > 0.90, "gravel AO grid never rose above 0.90: {max_seen}");
}
