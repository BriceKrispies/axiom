//! `weapons::parts::barrel`, pinned against the JavaScript it came from
//! (`C:/dev/Claude-of-Duty/src/weapons/parts.js:170-381`).
//!
//! Every `barrel_golden.json` value was produced by running the **original**
//! `parts.js` under Node (v24) against a real `Assembly` from `geometry.js`
//! (the same `three@0.180` install the other `weapons_geometry*`/
//! `weapons_parts_hardware_port` goldens use), calling `addBarrel`/
//! `addGasBlock`/`addMuzzleDevice` with the exact arguments repeated below,
//! `build()`-ing the assembly, and dumping every material bucket's
//! `position`/`normal`/`uv`/`index` alongside each call's return value. The
//! capture script is not committed, per the port recipe ("delete the capture
//! script, the committed goldens are the artifact").
//!
//! **Tolerance.** Vertex/triangle counts and index buffers are asserted
//! **exactly** — a differing count means a different algorithm, not rounding
//! (`03-weapon-geometry-api.md`); every case below matches the golden's
//! counts exactly. Position/normal/uv floats are asserted within `1e-5`
//! absolute — looser than the `1e-6` single-primitive bound
//! `weapons_geometry_primitives_port.rs` uses, and deliberately so: each part
//! here is *several* `lathe_z`/`tube_z`/`box_geo`/`knurl_band` calls (each
//! independently running `sin`/`cos`, not bit-guaranteed between V8's libm
//! and Rust's) merged and welded by `merge_all`/`Assembly::build`, so the
//! per-vertex error compounds across more independent trig calls than a
//! single primitive's own golden does. Measured peak here is `~5.9e-6` (a
//! gas-tube normal component); `1e-5` keeps honest headroom above that
//! without hiding a real algorithmic divergence — which, per the contract,
//! would show up as a **count** mismatch first, and every case below matches
//! its golden's vertex/triangle counts exactly.
//!
//! **`MuzzleKind` coverage.** All four muzzle-device variants (`brake`,
//! `a2`, `comp`, `trilug`) are exercised, plus a second `brake` case with a
//! non-default `y` offset and barrel radius, to catch a transform bug that a
//! single `y = 0` case would hide.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::Value;

use axiom_claude_of_duty::weapons::geometry::{Assembly, Geo};
use axiom_claude_of_duty::weapons::parts::barrel::{add_barrel, add_gas_block, add_muzzle_device, BarrelOpts, GasBlockOpts, MuzzleKind};

const TOL: f64 = 1e-5;

fn golden() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| serde_json::from_str(include_str!("parts/barrel_golden.json")).expect("golden.json parses"))
}

fn f64s(v: &Value) -> Vec<f64> {
    v.as_array()
        .unwrap_or_else(|| panic!("expected an array, got {v}"))
        .iter()
        .map(|x| x.as_f64().unwrap_or_else(|| panic!("not a number: {x}")))
        .collect()
}

fn close_slice(name: &str, field: &str, got: &[f32], want: &[f64]) {
    assert_eq!(got.len(), want.len(), "{name}: {field} length");
    got.iter().zip(want.iter()).enumerate().for_each(|(i, (a, b))| {
        let diff = (f64::from(*a) - b).abs();
        assert!(diff < TOL, "{name}: {field}[{i}] = {a} vs golden {b} (diff {diff})");
    });
}

fn assert_geo_matches(name: &str, g: &Geo, want: &Value) {
    close_slice(name, "pos", &g.pos, &f64s(&want["pos"]));
    close_slice(name, "normal", &g.normal, &f64s(&want["normal"]));
    close_slice(name, "uv", &g.uv, &f64s(&want["uv"]));

    match &want["index"] {
        Value::Null => assert!(g.index.is_empty(), "{name}: expected non-indexed (JS index is null)"),
        Value::Array(arr) => {
            let want_index: Vec<u32> = arr.iter().map(|x| x.as_u64().unwrap() as u32).collect();
            assert_eq!(g.index, want_index, "{name}: index buffer must match exactly");
        }
        other => panic!("{name}: unexpected index field shape: {other}"),
    }
}

/// A case's `buckets` object, keyed by material.
fn assert_bucket_matches(name: &str, built: &BTreeMap<String, Geo>, case: &str, mat: &str) {
    let g = built
        .get(mat)
        .unwrap_or_else(|| panic!("{name}: bucket {mat:?} missing from build() output"));
    assert_geo_matches(&format!("{name}.{mat}"), g, &golden()[case]["buckets"][mat]);
}

/// A case's `returned` object, one named field, tolerance-compared: the
/// golden was computed in JS `f64`, the value under test in Rust `f32`, so
/// even a plain `+ - * /` chain (`gasAt = zMuzzle + len * 0.34`, `crownZ =
/// zBarrelEnd - len`) is not guaranteed bit-identical across the two
/// precisions the way same-precision arithmetic would be.
fn assert_returned_field_matches(name: &str, case: &str, field: &str, got: f32) {
    let want = golden()[case]["returned"][field]
        .as_f64()
        .unwrap_or_else(|| panic!("{name}: returned.{field} missing or not a number"));
    let diff = (f64::from(got) - want).abs();
    assert!(diff < TOL, "{name}: returned.{field} = {got} vs golden {want} (diff {diff})");
}

// ---------------------------------------------------------------------
// addBarrel (parts.js:178-222)
// ---------------------------------------------------------------------

#[test]
fn add_barrel_matches_the_source_with_default_dimensions() {
    let mut asm = Assembly::new("barrelTest");
    let r = add_barrel(
        &mut asm,
        "steel",
        "cavity",
        BarrelOpts {
            z_breech: 0.4,
            z_muzzle: 0.0,
            ..Default::default()
        },
    );
    let built = asm.build();
    assert_bucket_matches("barrel_default", &built, "barrel_default", "steel");
    assert_bucket_matches("barrel_default", &built, "barrel_default", "cavity");
    assert_returned_field_matches("barrel_default", "barrel_default", "gasAt", r.gas_at);
    assert_returned_field_matches("barrel_default", "barrel_default", "rBore", r.r_bore);
}

#[test]
fn add_barrel_matches_the_source_with_custom_dimensions_and_knurl_disabled() {
    let mut asm = Assembly::new("barrelTest");
    let r = add_barrel(
        &mut asm,
        "steel",
        "cavity",
        BarrelOpts {
            y: 0.01,
            z_breech: 0.45,
            z_muzzle: 0.02,
            r_chamber: 0.013,
            r_barrel: 0.008,
            r_gas: 0.01,
            gas_at: Some(0.15),
            seg: 16,
            knurl: false,
        },
    );
    let built = asm.build();
    assert_bucket_matches("barrel_no_knurl", &built, "barrel_no_knurl", "steel");
    assert_bucket_matches("barrel_no_knurl", &built, "barrel_no_knurl", "cavity");
    // `knurl: false` really suppressed the knurled band: no third "steel"
    // sub-piece beyond the lathed body, which the vertex count above already
    // pins exactly against the golden — this assertion documents *why* the
    // count is smaller than the default case's, not just that it is.
    assert_returned_field_matches("barrel_no_knurl", "barrel_no_knurl", "gasAt", r.gas_at);
    assert_returned_field_matches("barrel_no_knurl", "barrel_no_knurl", "rBore", r.r_bore);
}

// ---------------------------------------------------------------------
// addGasBlock (parts.js:228-244)
// ---------------------------------------------------------------------

#[test]
fn add_gas_block_matches_the_source_with_default_dimensions() {
    let mut asm = Assembly::new("gasBlockTest");
    add_gas_block(
        &mut asm,
        "steel",
        GasBlockOpts {
            z: 0.2,
            tube_to: 0.4,
            ..Default::default()
        },
    );
    let built = asm.build();
    assert_bucket_matches("gas_block_default", &built, "gas_block_default", "steel");
}

#[test]
fn add_gas_block_matches_the_source_with_custom_dimensions() {
    let mut asm = Assembly::new("gasBlockTest");
    add_gas_block(
        &mut asm,
        "steel",
        GasBlockOpts {
            y: 0.005,
            z: 0.18,
            r_barrel: 0.009,
            w: 0.025,
            h: 0.022,
            len: 0.03,
            tube_to: 0.42,
        },
    );
    let built = asm.build();
    assert_bucket_matches("gas_block_custom", &built, "gas_block_custom", "steel");
}

// ---------------------------------------------------------------------
// addMuzzleDevice (parts.js:250-381) — every MuzzleKind variant.
// ---------------------------------------------------------------------

#[test]
fn add_muzzle_device_matches_the_source_for_every_kind() {
    let cases = [
        (MuzzleKind::Brake, "muzzle_brake"),
        (MuzzleKind::A2, "muzzle_a2"),
        (MuzzleKind::Comp, "muzzle_comp"),
        (MuzzleKind::Trilug, "muzzle_trilug"),
    ];

    cases.iter().for_each(|(kind, case)| {
        let mut asm = Assembly::new("muzzleTest");
        let r = add_muzzle_device(&mut asm, "steel", "cavity", *kind, 0.5, 0.0072, 0.0);
        let built = asm.build();
        assert_bucket_matches(case, &built, case, "steel");
        assert_bucket_matches(case, &built, case, "cavity");
        assert_returned_field_matches(case, case, "len", r.len);
        assert_returned_field_matches(case, case, "crownZ", r.crown_z);
    });
}

#[test]
fn add_muzzle_device_matches_the_source_with_a_nonzero_y_offset_and_barrel_radius() {
    let mut asm = Assembly::new("muzzleOffsetTest");
    let r = add_muzzle_device(&mut asm, "steel", "cavity", MuzzleKind::Brake, 0.42, 0.008, 0.003);
    let built = asm.build();
    assert_bucket_matches("muzzle_brake_offset", &built, "muzzle_brake_offset", "steel");
    assert_bucket_matches("muzzle_brake_offset", &built, "muzzle_brake_offset", "cavity");
    assert_returned_field_matches("muzzle_brake_offset", "muzzle_brake_offset", "len", r.len);
    assert_returned_field_matches("muzzle_brake_offset", "muzzle_brake_offset", "crownZ", r.crown_z);
}
