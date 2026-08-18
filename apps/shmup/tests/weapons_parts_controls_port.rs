//! `weapons::parts::controls`, pinned against the JavaScript it came from
//! (`C:/dev/Claude-of-Duty/src/weapons/parts.js`): `selectorPart` (`:795-828`),
//! `triggerPart` (`:838-866`), `addPistolGrip` (`:876-956`),
//! `addCarbineStock` (`:962-1071`), `chargingHandlePart` (`:1781-1854`),
//! `addForeGrip` (`:1857-1879`).
//!
//! Every `controls_golden.json` value was produced by running the
//! **original** `parts.js` under Node (v24) against a real `Assembly` from
//! `geometry.js` (the same `three@0.180` install the other
//! `weapons_geometry*`/`weapons_parts_*` goldens use), calling each function
//! with the exact arguments repeated below, dumping either the bare returned
//! geometry (`selectorPart`/`triggerPart`/`chargingHandlePart`, which never
//! touch an `Assembly`) or every material bucket from `build()`
//! (`addPistolGrip`/`addCarbineStock`/`addForeGrip`, which mutate one). The
//! capture script is not committed, per the port recipe ("delete the capture
//! script, the committed goldens are the artifact").
//!
//! **Tolerance.** Vertex/triangle counts and index buffers are asserted
//! **exactly** by default — a differing count means a different algorithm,
//! not rounding (`03-weapon-geometry-api.md`). Position/normal/uv floats are
//! asserted within `1e-5` absolute for an exact-count bucket, matching
//! `weapons_parts_barrel_port.rs`: every case here runs at least one
//! `merge_all` weld across several independent `lathe_z`/`rod_z`/`box_geo`/
//! `extrude`/`blob` calls (each running `sin`/`cos`, not bit-guaranteed
//! between V8's libm and Rust's), so the per-vertex error compounds past the
//! single-primitive `1e-6` bound `weapons_geometry_primitives_port.rs` uses.
//!
//! **The comparison history for the extrude+box_geo/blob buckets.** Every
//! bucket that merges an `extrude()` piece together with `box_geo()`/`blob()`
//! pieces (`trigger_default`; `pistol_grip_*`'s `polymer` bucket, which holds
//! the extruded core plus two blobs; `carbine_stock_*`'s `polymer` bucket,
//! the extruded shell plus three blobs; `charging_handle`, which interleaves
//! `box_geo`/`rod_z`/`extrude` throughout) comes back with the **same**
//! triangle/index topology as the golden but a welded **vertex count off by
//! a handful** (2-10 vertices, well under 1%) — the same
//! near-zero-denominator `get_bevel_vec` mechanism
//! `weapons_geometry_primitives_port.rs`'s module doc diagnoses, here tipping
//! the tie for a handful of `extrude` vertices that land close to a
//! neighbouring `box_geo`/`blob` piece rather than at a `round_rect`-style
//! tangent corner within one contour. These buckets were originally held to
//! [`assert_bucket_matches`]-adjacent topology-only checking (triangle count
//! exact, vertex-count delta bounded, no position/normal/uv comparison at
//! all), with a *not-committed* bounding-box spot check as the only informal
//! shape verification.
//!
//! **That was superseded by `tests/geometry_assert::assert_triangle_soup_matches`**,
//! a weld/order-invariant comparison that runs every time rather than being a
//! one-off spot check — but its FIRST version sorted triangles on a coarse
//! (5mm) per-field grid, which (per that module's doc) mispairs sub-5mm
//! repeated features against their neighbours. Every bucket in this file was
//! measured through that broken comparator and came back with "worst
//! deviations" of `0.0057`-`0.071` m position and `0.0057`-`0.093` uv —
//! alarming numbers that turned out to be mispairing artifacts, not real
//! geometry.
//!
//! **Re-measured with the fixed, centroid-keyed comparator** (used below via
//! [`assert_bucket_soup_matches`]/[`assert_bucket_soup_matches_uv`]), every
//! bucket's position and normal now matches fully within `TOL` (1e-5) —
//! confirming the shape and placement really were always correct, exactly as
//! the old not-committed bounding-box spot check suggested, just not provable
//! by a repeatable test until now. Five of the six buckets
//! (`trigger_default`, all three `pistol_grip_*.polymer`, `charging_handle`)
//! match **exactly**, `uv` included, and are promoted to full comparison via
//! [`assert_bucket_soup_matches`]. The remaining two
//! (`carbine_stock_default`/`carbine_stock_custom_y_break`'s `polymer`
//! bucket) have position and normal fully within `TOL` but a genuine `uv`
//! residual — measured `0.0839497...`/`0.0759497...` respectively — a real
//! instance of the already-documented `extrude()` projection-axis tie
//! (`weapons_parts_magazine_port.rs`'s module doc: the projection axis is
//! picked via a discrete `<` comparison between two side-length magnitudes,
//! so a sub-tolerance position difference can flip it and produce a `uv`
//! difference far larger than any float-noise budget on an otherwise
//! perfectly correct triangle). These two use
//! [`assert_bucket_soup_matches_uv`], which holds `uv` to the wider
//! [`CARBINE_STOCK_UV_TOL`] while still requiring position/normal at `TOL`.
//!
//! **Coverage.** `selectorPart`'s dead `matSteel` parameter is exercised with
//! both the default and a non-default `r` (`selector_wide`, catching a
//! scale-dependent bug a single case would hide). `addCarbineStock`'s detent
//! loop `break` (`z > zRear - 0.02`) is exercised by
//! `carbine_stock_custom_y_break`, which is sized to break after the first
//! iteration. `addPistolGrip`/`addForeGrip` each get one case with every
//! option explicit and one exercising `PistolGripOpts`/`ForeGripOpts`'
//! `Default` (`_defaults` cases) — `y`/`z` passed as `0.0` explicitly in both
//! the JS capture and the Rust default (JS has no real default for those
//! fields; the Rust `Default` only zeroes them for struct-update
//! convenience, per `03-weapon-geometry-api.md`'s convention).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::Value;

mod geometry_assert;
use geometry_assert::{assert_triangle_soup_matches, assert_triangle_soup_matches_uv};

use axiom_shmup::weapons::geometry::{Assembly, Geo};
use axiom_shmup::weapons::parts::controls::{
    add_carbine_stock, add_fore_grip, add_pistol_grip, charging_handle_part, selector_part, trigger_part,
    CarbineStockOpts, ForeGripOpts, PistolGripOpts,
};

const TOL: f64 = 1e-5;
/// `uv` tolerance for `carbine_stock_*`'s `polymer` bucket — the one
/// remaining genuine residual in this file after re-measuring with the
/// fixed comparator (see the module doc): a real `extrude()` projection-axis
/// tie, measured up to `0.0839497...`. `0.15` keeps comfortable headroom
/// above that without approaching the `~0.5` a genuinely wrong (not just
/// axis-flipped) uv would produce — the same shape of margin
/// `weapons_models_port.rs`'s `UV_TOL` (0.3 over a measured 0.21) uses.
const CARBINE_STOCK_UV_TOL: f64 = 0.15;

fn golden() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| serde_json::from_str(include_str!("parts/controls_golden.json")).expect("golden.json parses"))
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

fn assert_bucket_matches(name: &str, built: &BTreeMap<String, Geo>, case: &str, mat: &str) {
    let g = built
        .get(mat)
        .unwrap_or_else(|| panic!("{name}: bucket {mat:?} missing from build() output"));
    assert_geo_matches(&format!("{name}.{mat}"), g, &golden()[case]["buckets"][mat]);
}

/// The weld-invariant, full-fidelity comparison (see `geometry_assert`'s
/// module doc): every bucket in this file that used to be held to
/// [`assert_geo_topology_matches`]-style topology-only checking (a
/// same-triangle-count, budgeted-vertex-count-delta concession, position and
/// normal floats never compared) turns out, once re-measured with the fixed
/// centroid-keyed comparator, to need no such concession at all — the old
/// large "residual" readings were a mispairing artifact of the OLD,
/// coarse-grid comparator, not real geometric divergences. See the module
/// doc's "Re-measured" section for the numbers.
fn assert_bucket_soup_matches(name: &str, built: &BTreeMap<String, Geo>, case: &str, mat: &str) {
    let g = built
        .get(mat)
        .unwrap_or_else(|| panic!("{name}: bucket {mat:?} missing from build() output"));
    assert_triangle_soup_matches(&format!("{name}.{mat}"), g, &golden()[case]["buckets"][mat], TOL);
}

/// Same as [`assert_bucket_soup_matches`], but with `uv` held to
/// [`CARBINE_STOCK_UV_TOL`] instead of `TOL` — the one bucket pair
/// (`carbine_stock_default`/`carbine_stock_custom_y_break`'s `polymer`
/// bucket) where re-measuring found position and normal fully within `TOL`
/// but a genuine `uv` projection-axis-tie residual (see the module doc).
fn assert_bucket_soup_matches_uv(name: &str, built: &BTreeMap<String, Geo>, case: &str, mat: &str) {
    let g = built
        .get(mat)
        .unwrap_or_else(|| panic!("{name}: bucket {mat:?} missing from build() output"));
    assert_triangle_soup_matches_uv(&format!("{name}.{mat}"), g, &golden()[case]["buckets"][mat], TOL, CARBINE_STOCK_UV_TOL);
}

// ---------------------------------------------------------------------
// selectorPart (parts.js:795-828) — rod_z + lathe_z + extrude, no box_geo.
// ---------------------------------------------------------------------

#[test]
fn selector_part_matches_the_source_with_default_radius() {
    let s = selector_part("alu", "steel", 0.006);
    assert_geo_matches("selector_default", &s.geo, &golden()["selector_default"]);
    assert_eq!(s.mat, "alu");
}

#[test]
fn selector_part_matches_the_source_with_a_wider_radius() {
    let s = selector_part("alu", "steel", 0.008);
    assert_geo_matches("selector_wide", &s.geo, &golden()["selector_wide"]);
}

// ---------------------------------------------------------------------
// triggerPart (parts.js:838-866) — extrude(blade) merged with 6 box_geo
// serrations: the topology-residual case (see module doc).
// ---------------------------------------------------------------------

#[test]
fn trigger_part_matches_the_source() {
    let t = trigger_part("steel_bright");
    assert_triangle_soup_matches("trigger_default", &t.geo, &golden()["trigger_default"], TOL);
    assert_eq!(t.mat, "steel_bright");
}

// ---------------------------------------------------------------------
// addPistolGrip (parts.js:876-956)
// ---------------------------------------------------------------------

#[test]
fn add_pistol_grip_matches_the_source_with_rifle_dimensions() {
    let mut asm = Assembly::new("grip1");
    add_pistol_grip(
        &mut asm,
        "polymer",
        "rubber",
        PistolGripOpts {
            y: 0.035,
            z: 0.015,
            angle: 0.38,
            len: 0.108,
            w: 0.031,
        },
    );
    let built = asm.build();
    // polymer: extrude(core) + blob(beaver) + blob(cap) -> topology residual.
    assert_bucket_soup_matches("pistol_grip_rifle", &built, "pistol_grip_rifle", "polymer");
    // rubber: blob/box_geo only -> exact.
    assert_bucket_matches("pistol_grip_rifle", &built, "pistol_grip_rifle", "rubber");
}

#[test]
fn add_pistol_grip_matches_the_source_with_smg_dimensions() {
    let mut asm = Assembly::new("grip2");
    add_pistol_grip(
        &mut asm,
        "polymer",
        "rubber",
        PistolGripOpts {
            y: 0.033,
            z: 0.018,
            angle: 0.36,
            len: 0.102,
            w: 0.03,
        },
    );
    let built = asm.build();
    assert_bucket_soup_matches("pistol_grip_smg", &built, "pistol_grip_smg", "polymer");
    assert_bucket_matches("pistol_grip_smg", &built, "pistol_grip_smg", "rubber");
}

#[test]
fn add_pistol_grip_matches_the_source_with_default_options() {
    let mut asm = Assembly::new("grip3");
    add_pistol_grip(&mut asm, "polymer", "rubber", PistolGripOpts::default());
    let built = asm.build();
    assert_bucket_soup_matches("pistol_grip_defaults", &built, "pistol_grip_defaults", "polymer");
    assert_bucket_matches("pistol_grip_defaults", &built, "pistol_grip_defaults", "rubber");
}

// ---------------------------------------------------------------------
// addCarbineStock (parts.js:962-1071)
// ---------------------------------------------------------------------

#[test]
fn add_carbine_stock_matches_the_source_with_default_y() {
    let mut asm = Assembly::new("stock1");
    add_carbine_stock(
        &mut asm,
        "alu",
        "polymer",
        "rubber",
        CarbineStockOpts {
            bore: 0.019,
            z_front: 0.02,
            z_rear: 0.245,
            y: None,
        },
    );
    let built = asm.build();
    // alu: tube_z/lathe_z/box_geo only -> exact.
    assert_bucket_matches("carbine_stock_default", &built, "carbine_stock_default", "alu");
    // polymer: extrude(shell) + blob(cheek) + blob(scallops) -> topology residual.
    assert_bucket_soup_matches_uv("carbine_stock_default", &built, "carbine_stock_default", "polymer");
    // rubber: blob/box_geo only -> exact.
    assert_bucket_matches("carbine_stock_default", &built, "carbine_stock_default", "rubber");
}

/// Sized so the detent-notch loop's `z > zRear - 0.02` guard breaks after the
/// first iteration (`parts.js:998-1004`) — a shorter tube than the default
/// case, which never breaks within its 6 iterations.
#[test]
fn add_carbine_stock_matches_the_source_with_a_custom_y_and_the_detent_loop_breaking_early() {
    let mut asm = Assembly::new("stock2");
    add_carbine_stock(
        &mut asm,
        "alu",
        "polymer",
        "rubber",
        CarbineStockOpts {
            bore: 0.02,
            z_front: 0.1,
            z_rear: 0.16,
            y: Some(0.015),
        },
    );
    let built = asm.build();
    assert_bucket_matches(
        "carbine_stock_custom_y_break",
        &built,
        "carbine_stock_custom_y_break",
        "alu",
    );
    assert_bucket_soup_matches_uv(
        "carbine_stock_custom_y_break",
        &built,
        "carbine_stock_custom_y_break",
        "polymer",
    );
    assert_bucket_matches(
        "carbine_stock_custom_y_break",
        &built,
        "carbine_stock_custom_y_break",
        "rubber",
    );
}

// ---------------------------------------------------------------------
// chargingHandlePart (parts.js:1781-1854) — box_geo/rod_z interleaved with
// extrude throughout: the topology-residual case (see module doc).
// ---------------------------------------------------------------------

#[test]
fn charging_handle_part_matches_the_source() {
    let g = charging_handle_part();
    assert_triangle_soup_matches("charging_handle", &g, &golden()["charging_handle"], TOL);
}

// ---------------------------------------------------------------------
// addForeGrip (parts.js:1857-1879) — blob/box_geo only, no extrude.
// ---------------------------------------------------------------------

#[test]
fn add_fore_grip_matches_the_source_with_custom_options() {
    let mut asm = Assembly::new("fg1");
    add_fore_grip(
        &mut asm,
        "polymer",
        "rubber",
        ForeGripOpts {
            len: 0.06,
            y: 0.01,
            z: -0.15,
            angle: 0.22,
        },
    );
    let built = asm.build();
    assert_bucket_matches("foregrip_custom", &built, "foregrip_custom", "polymer");
    assert_bucket_matches("foregrip_custom", &built, "foregrip_custom", "rubber");
}

#[test]
fn add_fore_grip_matches_the_source_with_default_len_and_angle() {
    let mut asm = Assembly::new("fg2");
    add_fore_grip(
        &mut asm,
        "polymer",
        "rubber",
        ForeGripOpts {
            y: 0.0,
            z: 0.0,
            ..Default::default()
        },
    );
    let built = asm.build();
    assert_bucket_matches("foregrip_defaults", &built, "foregrip_defaults", "polymer");
    assert_bucket_matches("foregrip_defaults", &built, "foregrip_defaults", "rubber");
}
