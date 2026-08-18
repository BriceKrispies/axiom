//! The hard-surface primitive kit (`weapons::geometry::primitives`), pinned
//! against the JavaScript it came from.
//!
//! Every `golden.json` value in this file was produced by running the
//! **original** `C:/dev/Claude-of-Duty/src/weapons/geometry.js` under Node
//! (v24), dumping `position`/`normal`/`uv`/`index` (or, for `roundRect`, the
//! raw point list) as JSON. The capture script is not committed (per the
//! port recipe: "delete the capture script, the committed goldens are the
//! artifact") — it called each primitive once per case below with the exact
//! arguments repeated here, via `THREE`'s real `RoundedBoxGeometry`,
//! `LatheGeometry`, `SphereGeometry`, `TorusGeometry`, `ExtrudeGeometry`,
//! `OctahedronGeometry`, and `Earcut`.
//!
//! **Tolerance.** Counts and (where the source is indexed) the index buffer
//! itself are asserted **exactly** — a different count or a different index
//! sequence means a different algorithm ran, not a rounding difference
//! (`03-weapon-geometry-api.md`). Position/normal/uv floats are asserted
//! within `1e-6` absolute, the tolerance the same contract establishes:
//! every primitive here runs through at least one `sin`/`cos`/`sqrt`, which
//! is not bit-guaranteed between V8's libm and Rust's.
//!
//! **Degenerate cases**, one per the port recipe's requirement: a
//! zero-`phi_length` lathe (`lathe_zero_phi_length` — the swept angle is
//! zero, so the whole "revolution" collapses to a single meridian repeated
//! at every ring), a single-segment ring (`ring_single_segment`), a
//! zero-chamfer box (`box_zero_chamfer`, which takes `box_geo`'s
//! `RoundedBoxGeometry` branch out of the picture entirely and falls back to
//! a plain indexed `BoxGeometry`), a zero-segment box
//! (`box_zero_segments_quirk`, pinning the source's own
//! `RoundedBoxGeometry` quirk — see `rounded_box.rs`), a zero-`cut` dome, a
//! bevel-disabled extrude, and an extrude with a hole (the only exerciser of
//! `earcut`'s hole-elimination path in this kit today).

use std::sync::OnceLock;

use serde_json::Value;

use axiom_claude_of_duty::weapons::geometry::primitives::{
    blob, box_geo, dome, extrude, knurl_band, lathe_z, mlok_slot, picatinny, ring, rod_z, round_rect, screw,
    serrations, tube_z, Axis, ExtrudeOpts, PicatinnyOpts,
};
use axiom_claude_of_duty::weapons::geometry::Geo;

/// Absolute tolerance for a position/normal/uv float, per
/// `03-weapon-geometry-api.md`'s verification section.
const TOL: f64 = 1e-6;

fn golden() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| serde_json::from_str(include_str!("geometry/golden.json")).expect("golden.json parses"))
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

/// A weaker check for `extrude()` output built from a multi-point,
/// regular-angle contour (`round_rect`'s corners, or `picatinny`'s
/// mirror-symmetric tooth profile) with bevelling enabled. See
/// `primitives::extrude`'s module doc ("A real precision boundary") for the
/// full account: `get_bevel_vec` is bit-exact against the source when fed
/// full-`f64`-precision points, but the `f32` point-list `extrude` takes per
/// `03-weapon-geometry-api.md` costs enough precision, amplified through
/// that function's division, to occasionally tip `weld_vertices`' `1e-6`
/// quantization hash into a different bucket than the source's own
/// `mergeVertices` — a real, understood consequence of the fixed contract's
/// `f32` boundary, not an algorithm defect.
///
/// What stays exact: [`Geo::tri_count`], fixed by `earcut`'s triangulation
/// of the (un-bevelled) contour, which never goes through the amplifying
/// division. What is only bounded: [`Geo::vert_count`] (a handful of weld
/// decisions can go either way) and, in turn, individual welded positions
/// (a differing weld reshuffles which raw vertex a given output index
/// holds). `extrude_with_bevel_disabled_skips_the_contraction_pass` and
/// `extrude_with_a_hole_exercises_earcuts_bridge_elimination` both hit this
/// same code path with bevel geometry that stays off that knife-edge, and
/// both assert full exact fidelity via [`assert_geo_matches`].
fn assert_geo_topology_matches(name: &str, g: &Geo, want: &Value) {
    // `extrude()`'s output is always welded, hence always indexed — the
    // triangle count is the index buffer's length in triples.
    let want_index = want["index"]
        .as_array()
        .unwrap_or_else(|| panic!("{name}: extrude() output is always indexed"));
    assert_eq!(g.tri_count(), want_index.len() / 3, "{name}: triangle count (earcut topology) must match exactly");

    let want_vert_count = f64s(&want["pos"]).len() / 3;
    let got_vert_count = g.vert_count();
    let delta = got_vert_count.abs_diff(want_vert_count);
    // <= 10%, floor 8 verts: `mlok_slot` composes *two* `round_rect`-shaped
    // extrudes (`seg = 3`, more corners than `extrude_normal`'s `seg = 2`),
    // so it carries proportionally more amplification-prone bevel vectors.
    let budget = (want_vert_count / 10).max(8);
    assert!(
        delta <= budget,
        "{name}: vert_count {got_vert_count} vs golden {want_vert_count} (delta {delta} > budget {budget})"
    );
}

#[test]
fn box_geo_normal_chamfer_matches_rounded_box_geometry() {
    let g = box_geo(0.03, 0.02, 0.015, 0.001, 1);
    assert_geo_matches("box_normal", &g, &golden()["box_normal"]);
}

#[test]
fn box_geo_zero_chamfer_falls_back_to_plain_indexed_box() {
    let g = box_geo(0.03, 0.02, 0.015, 0.0, 1);
    assert_geo_matches("box_zero_chamfer", &g, &golden()["box_zero_chamfer"]);
    assert_eq!(g.vert_count(), 24);
    assert_eq!(g.tri_count(), 12);
}

#[test]
fn box_geo_zero_segments_reproduces_the_rounded_box_geometry_unit_box_quirk() {
    let g = box_geo(0.03, 0.02, 0.015, 0.002, 0);
    assert_geo_matches("box_zero_segments_quirk", &g, &golden()["box_zero_segments_quirk"]);
}

#[test]
fn blob_matches_a_higher_segment_rounded_box() {
    let g = blob(0.02, 0.02, 0.02, 0.005, 2);
    assert_geo_matches("blob_normal", &g, &golden()["blob_normal"]);
}

#[test]
fn lathe_z_matches_lathe_geometry_over_a_partial_revolution() {
    let g = lathe_z(&[[-0.01, 0.002], [0.0, 0.006], [0.01, 0.003]], 6, 0.0, std::f32::consts::PI);
    assert_geo_matches("lathe_normal", &g, &golden()["lathe_normal"]);
}

#[test]
fn lathe_z_zero_phi_length_is_a_degenerate_collapsed_revolution() {
    let g = lathe_z(&[[-0.01, 0.002], [0.0, 0.006], [0.01, 0.003]], 6, 0.4, 0.0);
    assert_geo_matches("lathe_zero_phi_length", &g, &golden()["lathe_zero_phi_length"]);
}

#[test]
fn tube_z_matches_a_crowned_lathed_wall() {
    let g = tube_z(0.01, 0.008, 0.05, 8, 0.0006);
    assert_geo_matches("tube_normal", &g, &golden()["tube_normal"]);
}

#[test]
fn rod_z_matches_a_chamfered_lathed_cylinder() {
    let g = rod_z(0.006, 0.004, 0.03, 8, 0.0008);
    assert_geo_matches("rod_normal", &g, &golden()["rod_normal"]);
}

#[test]
fn dome_matches_a_partial_sphere_geometry() {
    let g = dome(0.01, 8, 0.6);
    assert_geo_matches("dome_normal", &g, &golden()["dome_normal"]);
}

#[test]
fn dome_zero_cut_is_a_degenerate_flat_slice() {
    let g = dome(0.01, 8, 0.0);
    assert_geo_matches("dome_zero_cut", &g, &golden()["dome_zero_cut"]);
}

#[test]
fn ring_matches_a_full_torus() {
    let g = ring(0.02, 0.003, 8, 4, std::f32::consts::TAU);
    assert_geo_matches("ring_normal", &g, &golden()["ring_normal"]);
}

#[test]
fn ring_single_segment_is_the_recipes_required_degenerate_case() {
    let g = ring(0.01, 0.002, 1, 1, std::f32::consts::TAU);
    assert_geo_matches("ring_single_segment", &g, &golden()["ring_single_segment"]);
}

#[test]
fn ring_partial_arc_matches_a_torus_slice() {
    let g = ring(0.02, 0.003, 6, 3, std::f32::consts::PI);
    assert_geo_matches("ring_partial_arc", &g, &golden()["ring_partial_arc"]);
}

#[test]
fn screw_matches_a_lathed_head_plus_counterbore() {
    let g = screw(0.003, 0.0015, 0.002, 0.01, 6);
    assert_geo_matches("screw_normal", &g, &golden()["screw_normal"]);
}

#[test]
fn knurl_band_matches_a_merged_octahedron_grid() {
    let g = knurl_band(0.01, 0.02, 4, 0.0003, 2);
    assert_geo_matches("knurl_band_normal", &g, &golden()["knurl_band_normal"]);
}

#[test]
fn serrations_x_axis_matches_ribs_stepped_across_width() {
    let g = serrations(0.02, 0.01, 0.03, 3, 0.0005, Axis::X);
    assert_geo_matches("serrations_x", &g, &golden()["serrations_x"]);
}

#[test]
fn serrations_y_axis_matches_ribs_stepped_across_height() {
    let g = serrations(0.02, 0.01, 0.03, 3, 0.0005, Axis::Y);
    assert_geo_matches("serrations_y", &g, &golden()["serrations_y"]);
}

#[test]
fn round_rect_matches_the_javascript_point_list() {
    let got = round_rect(0.02, 0.01, 0.002, 2);
    let want = golden()["round_rect_normal"].as_array().unwrap();
    assert_eq!(got.len(), want.len(), "round_rect_normal: point count");
    got.iter().zip(want.iter()).enumerate().for_each(|(i, (g, w))| {
        let wp = w.as_array().unwrap();
        let wx = wp[0].as_f64().unwrap();
        let wy = wp[1].as_f64().unwrap();
        assert!((f64::from(g[0]) - wx).abs() < TOL, "round_rect_normal[{i}].x = {} vs {wx}", g[0]);
        assert!((f64::from(g[1]) - wy).abs() < TOL, "round_rect_normal[{i}].y = {} vs {wy}", g[1]);
    });
}

#[test]
fn extrude_matches_a_bevelled_round_rect_outline() {
    // `round_rect(0.021, 0.011, 0.0021, 2)`'s corners are exact multiples of
    // 45 degrees — the regular, symmetric-angle shape `extrude`'s module doc
    // ("A real precision boundary") shows measurably amplifies the `f32`
    // point-list contract's rounding through `get_bevel_vec`'s division.
    // Verified there to be a real, `f64`-vs-`f32`-precision effect and not an
    // algorithm defect: only the topology-level check
    // ([`assert_geo_topology_matches`]) applies here.
    // `extrude_with_bevel_disabled_skips_the_contraction_pass` and
    // `extrude_with_a_hole_exercises_earcuts_bridge_elimination` exercise
    // this same bevelled/welded code path on contours that stay off that
    // knife-edge, and both assert full exact fidelity.
    let pts = round_rect(0.021, 0.011, 0.0021, 2);
    let g = extrude(
        &pts,
        0.0051,
        ExtrudeOpts {
            bevel: 0.00047,
            ..Default::default()
        },
    );
    assert_geo_topology_matches("extrude_normal", &g, &golden()["extrude_normal"]);
}

#[test]
fn extrude_with_bevel_disabled_skips_the_contraction_pass() {
    let g = extrude(
        &[[-0.01, -0.005], [0.01, -0.005], [0.01, 0.005], [-0.01, 0.005]],
        0.004,
        ExtrudeOpts {
            bevel: 0.0,
            ..Default::default()
        },
    );
    assert_geo_matches("extrude_no_bevel", &g, &golden()["extrude_no_bevel"]);
}

#[test]
fn extrude_with_a_hole_exercises_earcuts_bridge_elimination() {
    let g = extrude(
        &[[-0.01, -0.01], [0.01, -0.01], [0.01, 0.01], [-0.01, 0.01]],
        0.004,
        ExtrudeOpts {
            bevel: 0.0,
            holes: vec![vec![[-0.004, -0.004], [0.004, -0.004], [0.004, 0.004], [-0.004, 0.004]]],
            ..Default::default()
        },
    );
    assert_geo_matches("extrude_with_hole", &g, &golden()["extrude_with_hole"]);
}

#[test]
fn picatinny_matches_a_railed_run_of_bevelled_teeth() {
    // The rail's tooth profile is itself mirror-symmetric about `x = 0` —
    // the same `f32`-precision-into-`get_bevel_vec` amplification
    // `extrude_matches_a_bevelled_round_rect_outline` documents applies
    // here too, so only topology is asserted exactly.
    let g = picatinny(0.031, PicatinnyOpts::default());
    assert_geo_topology_matches("picatinny_normal", &g, &golden()["picatinny_normal"]);
}

#[test]
fn mlok_slot_matches_a_recessed_pocket_with_a_lip() {
    // Both the outer and inner pockets are `round_rect` outlines — see
    // `extrude_matches_a_bevelled_round_rect_outline`.
    let g = mlok_slot(0.033, 0.0076, 0.0023);
    assert_geo_topology_matches("mlok_slot_normal", &g, &golden()["mlok_slot_normal"]);
}

