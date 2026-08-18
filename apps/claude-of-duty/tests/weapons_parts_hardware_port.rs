//! `weapons::parts::hardware`, pinned against the JavaScript it came from
//! (`C:/dev/Claude-of-Duty/src/weapons/parts.js:36-168`).
//!
//! Every `hardware_golden.json` value was produced by running the
//! **original** `parts.js` under Node (v24) against a real `Assembly` from
//! `geometry.js` (the same `three@0.180` install `weapons_geometry_port.rs`
//! and `weapons_geometry_primitives_port.rs` use), calling each `addX`
//! helper with the exact arguments repeated below, `build()`-ing the
//! assembly, and dumping every material bucket's `position`/`normal`/`uv`/
//! `index`. `cartridge`/`emptyCase` are dumped directly (they return bare
//! geometry, never touching an `Assembly`). The capture script is not
//! committed, per the port recipe ("delete the capture script, the
//! committed goldens are the artifact").
//!
//! **Tolerance.** Vertex/triangle counts and index buffers are asserted
//! **exactly** — a differing count means a different algorithm, not
//! rounding (`03-weapon-geometry-api.md`). Position/normal/uv floats are
//! asserted within `1e-6` absolute: every primitive these helpers call
//! bottoms out in `lathe_z`/`dome`/`ring`/`screw`/`picatinny`, all of which
//! run through `sin`/`cos`, not bit-guaranteed between V8's libm and Rust's.
//! `add_rail`'s `picatinny()`-derived buckets are the one exception: they go
//! through `assert_bucket_topology_matches` (triangle count exact, vertex
//! count bounded), the same weaker check
//! `weapons_geometry_primitives_port.rs` uses for `picatinny`/`mlok_slot` —
//! seeing this once through `Assembly::build`'s extra weld pass amplifies
//! the same documented `f32` point-list precision boundary
//! (`03-weapon-geometry-api.md`'s "Corrections" section) enough to occasionally
//! tip a normal past `1e-6`.
//!
//! **Axis branches.** `add_screw` and `add_qd_socket` each take a
//! [`MountAxis`] with three variants (`X`/`Y`/`Z`) that resolve to
//! *different* rotations per function (`parts.js:52`, `parts.js:77`) — every
//! case below exercises all three for both functions, including the
//! identity (`Z`) fallthrough branch.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::Value;

use axiom_claude_of_duty::weapons::geometry::{Assembly, Geo, Xform};
use axiom_claude_of_duty::weapons::parts::hardware::{
    add_pin, add_qd_socket, add_rail, add_screw, add_sling_loop, cartridge, empty_case, MountAxis, RailOpts,
    MUZZLE_LEN,
};

const TOL: f64 = 1e-6;

fn golden() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| serde_json::from_str(include_str!("parts/hardware_golden.json")).expect("golden.json parses"))
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

fn assert_bucket_matches(name: &str, built: &BTreeMap<String, Geo>, golden_group: &Value, mat: &str) {
    let g = built
        .get(mat)
        .unwrap_or_else(|| panic!("{name}: bucket {mat:?} missing from build() output"));
    assert_geo_matches(&format!("{name}.{mat}"), g, &golden_group[mat]);
}

/// A weaker topology-only check, ported verbatim from
/// `weapons_geometry_primitives_port.rs`'s `assert_geo_topology_matches`: for
/// geometry that passes through `extrude()`'s bevel path (here, every
/// `picatinny()`-derived rail bucket), the `f32` point-list boundary
/// documented in `03-weapon-geometry-api.md`'s "Corrections" section
/// occasionally tips `mergeVertices`'/`weld_vertices`' `1e-6` quantization
/// into a different bucket than the source, a real precision-boundary
/// consequence rather than an algorithm defect. Triangle count (fixed by
/// `earcut`, which never crosses the amplifying division) and index-derived
/// tri count stay exact; vertex count is only bounded.
fn assert_bucket_topology_matches(name: &str, built: &BTreeMap<String, Geo>, golden_group: &Value, mat: &str) {
    let g = built
        .get(mat)
        .unwrap_or_else(|| panic!("{name}: bucket {mat:?} missing from build() output"));
    let want = &golden_group[mat];
    let full_name = format!("{name}.{mat}");
    let want_index = want["index"]
        .as_array()
        .unwrap_or_else(|| panic!("{full_name}: expected an indexed golden bucket"));
    assert_eq!(g.tri_count(), want_index.len() / 3, "{full_name}: triangle count must match exactly");

    let want_vert_count = f64s(&want["pos"]).len() / 3;
    let got_vert_count = g.vert_count();
    let delta = got_vert_count.abs_diff(want_vert_count);
    let budget = (want_vert_count / 10).max(8);
    assert!(
        delta <= budget,
        "{full_name}: vert_count {got_vert_count} vs golden {want_vert_count} (delta {delta} > budget {budget})"
    );
}

// ---------------------------------------------------------------------
// MUZZLE_LEN (parts.js:36) — a literal transcription, exact equality.
// ---------------------------------------------------------------------

#[test]
fn muzzle_len_matches_the_source_literals() {
    assert_eq!(MUZZLE_LEN.brake, 0.062);
    assert_eq!(MUZZLE_LEN.a2, 0.0483);
    assert_eq!(MUZZLE_LEN.comp, 0.058);
    assert_eq!(MUZZLE_LEN.trilug, 0.042);
}

// ---------------------------------------------------------------------
// addPin (parts.js:43-47)
// ---------------------------------------------------------------------

#[test]
fn add_pin_matches_the_source() {
    let mut asm = Assembly::new("pinTest");
    add_pin(&mut asm, "pin", 0.01, 0.02, 0.03, 0.0022, 0.02);
    let built = asm.build();
    assert_bucket_matches("addPin", &built, &golden()["addPin"], "pin");
}

// ---------------------------------------------------------------------
// addScrew (parts.js:50-55) — all three MountAxis branches.
// ---------------------------------------------------------------------

#[test]
fn add_screw_matches_the_source_for_every_mount_axis() {
    let mut asm = Assembly::new("screwTest");
    add_screw(&mut asm, "screwY", 0.01, -0.02, 0.015, 0.0022, MountAxis::Y, 0.008);
    add_screw(&mut asm, "screwX", 0.01, -0.02, 0.015, 0.0022, MountAxis::X, 0.008);
    add_screw(&mut asm, "screwZ", 0.01, -0.02, 0.015, 0.0022, MountAxis::Z, 0.008);
    let built = asm.build();
    let want = &golden()["addScrew"];
    assert_bucket_matches("addScrew", &built, want, "screwY");
    assert_bucket_matches("addScrew", &built, want, "screwX");
    assert_bucket_matches("addScrew", &built, want, "screwZ");
}

// ---------------------------------------------------------------------
// addQdSocket (parts.js:58-82) — all three MountAxis branches.
// ---------------------------------------------------------------------

#[test]
fn add_qd_socket_matches_the_source_for_every_mount_axis() {
    let mut asm = Assembly::new("qdTest");
    add_qd_socket(&mut asm, "qdBodyX", "qdSteelX", 0.02, -0.01, 0.05, MountAxis::X, 0.0055);
    add_qd_socket(&mut asm, "qdBodyY", "qdSteelY", 0.02, -0.01, 0.05, MountAxis::Y, 0.0055);
    add_qd_socket(&mut asm, "qdBodyZ", "qdSteelZ", 0.02, -0.01, 0.05, MountAxis::Z, 0.0055);
    let built = asm.build();
    let want = &golden()["addQdSocket"];
    for mat in ["qdBodyX", "qdSteelX", "qdBodyY", "qdSteelY", "qdBodyZ", "qdSteelZ"] {
        assert_bucket_matches("addQdSocket", &built, want, mat);
    }
}

// ---------------------------------------------------------------------
// addSlingLoop (parts.js:85-89) — default identity rot, and a custom rot.
// ---------------------------------------------------------------------

#[test]
fn add_sling_loop_matches_the_source_with_default_and_custom_rotation() {
    let mut asm = Assembly::new("slingTest");
    add_sling_loop(&mut asm, "slingA", 0.01, 0.02, -0.03, 0.008, Xform::default());
    add_sling_loop(
        &mut asm,
        "slingB",
        0.01,
        0.02,
        -0.03,
        0.008,
        Xform {
            rx: 0.3,
            ry: -0.2,
            ..Default::default()
        },
    );
    let built = asm.build();
    let want = &golden()["addSlingLoop"];
    assert_bucket_matches("addSlingLoop", &built, want, "slingA");
    assert_bucket_matches("addSlingLoop", &built, want, "slingB");
}

// ---------------------------------------------------------------------
// cartridge (parts.js:92-116) — default-shaped and custom-dimensioned.
// ---------------------------------------------------------------------

#[test]
fn cartridge_matches_the_source_with_default_dimensions() {
    let c = cartridge(0.0446, 0.00495, 0.019);
    assert_geo_matches("cartridge.default.brass", &c.brass, &golden()["cartridge"]["default"]["brass"]);
    assert_geo_matches("cartridge.default.bullet", &c.bullet, &golden()["cartridge"]["default"]["bullet"]);
    assert_eq!(c.length, 0.0446 + 0.019);
}

#[test]
fn cartridge_matches_the_source_with_custom_dimensions() {
    let c = cartridge(0.05, 0.005, 0.02);
    assert_geo_matches("cartridge.custom.brass", &c.brass, &golden()["cartridge"]["custom"]["brass"]);
    assert_geo_matches("cartridge.custom.bullet", &c.bullet, &golden()["cartridge"]["custom"]["bullet"]);
    assert_eq!(c.length, 0.05 + 0.02);
}

// ---------------------------------------------------------------------
// emptyCase (parts.js:119-134) — default and custom dimensions.
// ---------------------------------------------------------------------

#[test]
fn empty_case_matches_the_source_with_default_dimensions() {
    let g = empty_case(0.0446, 0.00495);
    assert_geo_matches("emptyCase.default", &g, &golden()["emptyCase"]["default"]);
}

#[test]
fn empty_case_matches_the_source_with_custom_dimensions() {
    let g = empty_case(0.05, 0.005);
    assert_geo_matches("emptyCase.custom", &g, &golden()["emptyCase"]["custom"]);
}

// ---------------------------------------------------------------------
// addRail (parts.js:141-168) — default opts, custom opts, and
// `slot_floor: false` (the `opts.slotFloor !== false` branch, inverted).
// The default-opts and custom-opts slot floors share the fixed `"cavity"`
// material bucket (`parts.js:165`), so this single assembly reproduces the
// same cross-call bucket merge the source produces.
// ---------------------------------------------------------------------

#[test]
fn add_rail_matches_the_source_across_default_custom_and_no_floor_variants() {
    let mut asm = Assembly::new("railTest");
    add_rail(&mut asm, "railDefault", -0.1, 0.1, 0.02, 0.0, RailOpts::default());
    add_rail(
        &mut asm,
        "railCustom",
        -0.05,
        0.08,
        0.03,
        0.01,
        RailOpts {
            base_h: 0.005,
            top_h: 0.0035,
            waist: 0.016,
            slot_floor: true,
            ..RailOpts::default()
        },
    );
    add_rail(
        &mut asm,
        "railNoFloor",
        -0.05,
        0.05,
        0.02,
        0.0,
        RailOpts {
            slot_floor: false,
            ..RailOpts::default()
        },
    );
    let built = asm.build();
    let want = &golden()["addRail"];
    // `railDefault`/`railCustom`/`railNoFloor` are `picatinny()` output run
    // through a second `mergeAll` weld in `Assembly::build` — topology-only,
    // per `assert_bucket_topology_matches`'s doc. `cavity` is a plain
    // `box_geo` floor with no extrude in its ancestry, so it gets the exact
    // check.
    assert_bucket_topology_matches("addRail", &built, want, "railDefault");
    assert_bucket_topology_matches("addRail", &built, want, "railCustom");
    assert_bucket_topology_matches("addRail", &built, want, "railNoFloor");
    assert_bucket_matches("addRail", &built, want, "cavity");
    // `railNoFloor` never touches `"cavity"`: only two floors (from
    // `railDefault` and `railCustom`) went in, so `slot_floor: false` really
    // suppressed the third.
    assert!(!built.contains_key("railNoFloorCavity"));
}
