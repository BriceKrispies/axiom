//! Golden capture for the ported `src/materials/surfaces/organic.rs`
//! (wood/fabric/burlap/foliage/rubber/glass), pinned against
//! `tests/materials_surfaces_organic/golden.json`.
//!
//! **This oracle is hand-written, not genuine.** `surfaces-organic.js` (and
//! the `noise.js` library it builds on) is GLSL held inside JavaScript
//! template-string literals — shader source that never ran anywhere but a
//! browser GPU. There is no JS function to `import` and call, so
//! `tests/materials_surfaces_organic/capture.mjs` hand-transcribes each GLSL
//! body into plain JS doubles, independently of (but line-referenced
//! against, the same as) this crate's Rust transcription in
//! `src/materials/surfaces/organic.rs`. A match between the two catches
//! drift between the Rust port and *a* careful reading of the GLSL — it
//! cannot catch a mistake both transcriptions share. See that script's
//! module doc, and `tests/materials_surfaces_ground_port.rs` for the same
//! discipline applied to the sibling ground surfaces.
//!
//! ## Tolerance
//!
//! **`1e-9` relative**, the same figure `tests/materials_surfaces_ground_port.rs`
//! uses: every generator here chains `sin`/`cos`/`exp`/`atan2`/`pow`, none of
//! which are bit-guaranteed across V8 and Rust's libm.

use std::sync::OnceLock;

use serde_json::Value;

use axiom_claude_of_duty::materials::bake::SurfaceSample;
use axiom_claude_of_duty::materials::noise::{ow_srgb, Vec2, Vec3};
use axiom_claude_of_duty::materials::surfaces::organic::{
    burlap, fabric, foliage, glass, rubber, wood,
};

const REL: f64 = 1e-9;

fn golden() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| {
        serde_json::from_str(include_str!("materials_surfaces_organic/golden.json"))
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

/// `new THREE.Color(hex)` under this project's r180 default color
/// management decodes a hex literal as sRGB into the linear working color
/// space — the same transform [`ow_srgb`] performs on every other
/// hard-coded albedo constant in this file. Mirrors
/// `tests/materials_surfaces_organic/capture.mjs`'s `hexToLinear`.
fn hex_to_linear(hex: u32) -> Vec3 {
    ow_srgb(Vec3::new(
        f64::from((hex >> 16) & 0xff) / 255.0,
        f64::from((hex >> 8) & 0xff) / 255.0,
        f64::from(hex & 0xff) / 255.0,
    ))
}

fn check_sample(name: &str, uv: Vec2, expected: &Value, actual: SurfaceSample) {
    close(actual.albedo.x, num(&expected["alb"][0]), &format!("{name}.albedo.x @ {uv:?}"));
    close(actual.albedo.y, num(&expected["alb"][1]), &format!("{name}.albedo.y @ {uv:?}"));
    close(actual.albedo.z, num(&expected["alb"][2]), &format!("{name}.albedo.z @ {uv:?}"));
    close(actual.height, num(&expected["h"]), &format!("{name}.height @ {uv:?}"));
    close(actual.roughness, num(&expected["rough"]), &format!("{name}.roughness @ {uv:?}"));
    assert_eq!(actual.metal, num(&expected["metal"]), "{name}.metal @ {uv:?}");
    close(actual.ao, num(&expected["ao"]), &format!("{name}.ao @ {uv:?}"));
}

/// Runs one seed-only generator's golden `samples` array against the Rust
/// function, checking every field of the `owSurface` out-parameters.
fn check_generator(name: &str, f: impl Fn(Vec2, f64) -> SurfaceSample) {
    let g = &golden()[name];
    let seed = num(&g["seed"]);
    for row in g["samples"].as_array().unwrap() {
        let uv = uv_of(row);
        check_sample(name, uv, &row["out"], f(uv, seed));
    }
}

#[test]
fn wood_matches_the_hand_transcribed_glsl() {
    check_generator("wood", wood);
}

#[test]
fn burlap_matches_the_hand_transcribed_glsl() {
    check_generator("burlap", burlap);
}

#[test]
fn foliage_matches_the_hand_transcribed_glsl() {
    check_generator("foliage", foliage);
}

#[test]
fn rubber_matches_the_hand_transcribed_glsl() {
    check_generator("rubber", rubber);
}

#[test]
fn glass_matches_the_hand_transcribed_glsl() {
    check_generator("glass", glass);
}

/// `fabric` alone takes `uTintA`/`uTintB` (`src/materials/mod.rs::LIBRARY`'s
/// `fabric` entry: seed 43, `tint_a = 0x5a5445`, `tint_b = 0x3a3830`), so it
/// needs its own driver rather than [`check_generator`]'s seed-only shape.
#[test]
fn fabric_matches_the_hand_transcribed_glsl() {
    let g = &golden()["fabric"];
    let seed = num(&g["seed"]);
    let tint_a = hex_to_linear(0x5a5445);
    let tint_b = hex_to_linear(0x3a3830);

    // The golden's own captured tints must agree with this file's
    // independent re-derivation of `hexToLinear`, or the two transcriptions
    // have silently diverged on what "the tint uniform" even means.
    close(tint_a.x, num(&g["tintA"][0]), "fabric tintA.x");
    close(tint_a.y, num(&g["tintA"][1]), "fabric tintA.y");
    close(tint_a.z, num(&g["tintA"][2]), "fabric tintA.z");
    close(tint_b.x, num(&g["tintB"][0]), "fabric tintB.x");
    close(tint_b.y, num(&g["tintB"][1]), "fabric tintB.y");
    close(tint_b.z, num(&g["tintB"][2]), "fabric tintB.z");

    for row in g["samples"].as_array().unwrap() {
        let uv = uv_of(row);
        check_sample("fabric", uv, &row["out"], fabric(uv, seed, tint_a, tint_b));
    }
}

/// The port recipe calls this out explicitly: foliage's `h` is the alpha-test
/// **cutout mask** (`bestCover`), not a height — the one place in this file
/// where the bake's height/alpha channel means something different. A real
/// height channel spreads continuously across a surface; a cutout mask
/// should cluster near `0.0` (no leaf covers this texel) and `1.0` (dead
/// center of a leaf), with only a thin serrated-edge band in between.
/// Checked against a dense 25x25 grid from the *transcription*, not just the
/// Rust port's own re-derivation of the same shape.
#[test]
fn foliage_h_is_a_binary_ish_cutout_mask_not_a_smooth_height() {
    let g = &golden()["foliageDenseH"];
    let seed = num(&golden()["foliage"]["seed"]);
    let rows = g.as_array().unwrap();
    assert_eq!(rows.len(), 25 * 25, "dense grid must be the full 25x25");

    let mut near_extreme = 0usize;
    let mut mid_band = 0usize;
    for row in rows {
        let uv = uv_of(row);
        let expected_h = num(&row["h"]);
        let actual_h = foliage(uv, seed).height;
        close(actual_h, expected_h, &format!("foliage.height (dense) @ {uv:?}"));
        assert!((0.0..=1.0).contains(&actual_h), "foliage h {actual_h} at {uv:?} escaped [0, 1]");
        if actual_h < 0.05 || actual_h > 0.95 {
            near_extreme += 1;
        } else {
            mid_band += 1;
        }
    }
    // A genuine cutout mask spends most of its area at the extremes (leaf
    // interior / bare background) with only a thin serrated-edge transition
    // band in between — the opposite distribution a continuous height field
    // would show, where most texels sit away from 0/1.
    assert!(
        near_extreme > mid_band,
        "foliage h ({near_extreme} extreme vs {mid_band} mid-band) does not read as a \
         binary-ish cutout mask over the dense grid"
    );
}

/// The port recipe calls this out too: glass's look is carried entirely by
/// roughness — the albedo stays near-black everywhere, `rough` does the
/// work. Checked against the same dense-grid discipline as the foliage
/// assertion above, independently of the point samples.
#[test]
fn glass_albedo_stays_near_black_while_roughness_carries_the_variation() {
    let seed = num(&golden()["glass"]["seed"]);
    let mut min_rough = f64::INFINITY;
    let mut max_rough = f64::NEG_INFINITY;
    for iy in 0..=16u32 {
        for ix in 0..=16u32 {
            let uv = Vec2::new(f64::from(ix) / 16.0, f64::from(iy) / 16.0);
            let s = glass(uv, seed);
            // "Near-black": the unclamped base is owSRGB(0.045, 0.050, 0.052)
            // plus small additive terms (dirt tint, scratch tint) bounded by
            // the source's own `clamp(c, vec3(0.02), vec3(0.5))` — well under
            // a mid-grey 0.2 anywhere on the tile.
            assert!(s.albedo.x < 0.2 && s.albedo.y < 0.2 && s.albedo.z < 0.2, "glass albedo too bright at {uv:?}: {:?}", s.albedo);
            min_rough = min_rough.min(s.roughness);
            max_rough = max_rough.max(s.roughness);
        }
    }
    // Roughness must actually vary (smear/dust/scratches), not sit flat —
    // otherwise "the look is entirely roughness" would be vacuously true.
    assert!(max_rough - min_rough > 0.05, "glass roughness barely varies: {min_rough}..{max_rough}");
    assert!((0.02..=0.7).contains(&min_rough), "glass roughness escaped its documented clamp: {min_rough}");
    assert!((0.02..=0.7).contains(&max_rough), "glass roughness escaped its documented clamp: {max_rough}");
}
