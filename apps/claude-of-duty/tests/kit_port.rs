//! The `kit.js` modular-building-kit port, pinned against the JavaScript it
//! came from.
//!
//! Every `golden.json` value here was produced by running the **original**
//! `C:/dev/Claude-of-Duty/src/world/kit.js` (plus its `util.js`/
//! `builder.js`/`palette.js` dependencies) under Node (v24). The capture
//! script is committed next to the data at `tests/kit/capture.mjs`, so the
//! goldens are reproducible: re-run it against the source and the file
//! should come out identical.
//!
//! ## What is pinned, and how tightly
//!
//! * **Exactly (as integer counts)** — every element's per-palette-key
//!   vertex/triangle counts (`buckets`), `solidSlabs`' rectangles (`x/y/w/h`
//!   are built only from `+ - * /` on panel-space floats, so exact
//!   `f32`-tolerance equality), and `windowState`'s selection distribution
//!   across a floor/damage/allowLit sweep (500 draws per combination from a
//!   fixed seed — a differing count anywhere is a different threshold, per
//!   the port recipe's "a differing count is a different algorithm, never
//!   rounding").
//! * **Within `1e-6` absolute** — `pockGeometry`'s full position/normal/color
//!   arrays: it is a hand-rolled indexed mesh with a fixed, deterministic
//!   vertex order on both sides (no extrude/weld involved anywhere in its
//!   construction), so a direct array comparison is meaningful.
//! * **Triangle-soup, weld-invariant** — `spallPatch`'s position/normal
//!   only (no uv/color channel in the shared comparator — see
//!   `tests/geometry_assert/mod.rs`'s doc): it is built through
//!   `poly_prism` -> `weapons::geometry::primitives::extrude`, which welds
//!   vertices at `1e-6` and can reorder them relative to the JavaScript's
//!   raw (unwelded) `ExtrudeGeometry`, exactly the same class of divergence
//!   `wall_panel` already documents and this suite's `world_port.rs` already
//!   works around with the same comparator.

use std::sync::OnceLock;

use serde_json::Value;

mod geometry_assert;
use geometry_assert::assert_triangle_soup_matches_raw;

use axiom_claude_of_duty::rng::Rng;
use axiom_claude_of_duty::world::assembler::{Assembler, StaticMesh};
use axiom_claude_of_duty::world::kit::{
    awning, balcony, door_unit, drainpipe, facade_wall, parapet, pock_geometry, rubble_mound, shopfront, spall_patch, stair_run, striped_cloth,
    striped_cloth_default_seg_x, trs, window_state, window_unit, AwningOpts, BalconyOpts, BalconyRailing, DoorOpts, DrainpipeOpts, FacadeSpec, ParapetOpts,
    RubbleOpts, ShopfrontOpts, StairOpts, StairRailing, StripedClothOpts, WallHole, WallTop, WindowOpts, WindowState,
};

const TOL: f64 = 1e-6;

fn golden() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| serde_json::from_str(include_str!("kit/golden.json")).expect("golden.json parses"))
}

fn f64s(v: &Value) -> Vec<f64> {
    v.as_array().unwrap_or_else(|| panic!("expected an array, got {v}")).iter().map(|x| x.as_f64().unwrap_or_else(|| panic!("not a number: {x}"))).collect()
}

/// Sorted `(key, verts, tris)` triples, matching `finalizeBuckets`'
/// `capture.mjs` output shape.
fn buckets(statics: &[StaticMesh]) -> Vec<(String, u64, u64)> {
    let mut v: Vec<(String, u64, u64)> = statics.iter().map(|s| (s.key.clone(), s.geo.vert_count() as u64, s.geo.tri_count() as u64)).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

fn golden_buckets(name: &str) -> Vec<(String, u64, u64)> {
    golden()[name]["buckets"]
        .as_array()
        .unwrap_or_else(|| panic!("{name}.buckets is not an array"))
        .iter()
        .map(|b| (b["key"].as_str().unwrap().to_string(), b["verts"].as_u64().unwrap(), b["tris"].as_u64().unwrap()))
        .collect()
}

fn assert_buckets_match(name: &str, got: &[StaticMesh]) {
    let got = buckets(got);
    let want = golden_buckets(name);
    assert_eq!(got, want, "{name}: per-key vertex/triangle counts");
}

fn fixed_panel_matrix() -> axiom_math::Mat4 {
    trs(1.2, 0.4, 3.4, 0.3, 1.0, 1.0, 1.0, 0.0, 0.0)
}

// =============================================================== solidSlabs ==
#[test]
fn solid_slabs_matches_the_javascript_across_four_layouts() {
    use axiom_claude_of_duty::world::kit::solid_slabs;

    let assert_layout = |name: &str, got: Vec<axiom_claude_of_duty::world::kit::SolidSlab>| {
        let want = golden()["solid_slabs"][name].as_array().expect("array");
        assert_eq!(got.len(), want.len(), "{name}: slab count");
        for (g, w) in got.iter().zip(want.iter()) {
            assert!((f64::from(g.x) - w["x"].as_f64().unwrap()).abs() < TOL);
            assert!((f64::from(g.y) - w["y"].as_f64().unwrap()).abs() < TOL);
            assert!((f64::from(g.w) - w["w"].as_f64().unwrap()).abs() < TOL);
            assert!((f64::from(g.h) - w["h"].as_f64().unwrap()).abs() < TOL);
        }
    };

    assert_layout("no_holes", solid_slabs(2.0, 3.0, &[]));
    assert_layout("one_centered_hole", solid_slabs(2.0, 3.0, &[WallHole { x: 0.0, y: 1.5, w: 0.6, h: 0.8, arch: 0.0, ragged: 0.0 }]));
    assert_layout(
        "two_holes",
        solid_slabs(
            4.0,
            3.0,
            &[
                WallHole { x: -1.0, y: 1.0, w: 0.8, h: 1.4, arch: 0.0, ragged: 0.0 },
                WallHole { x: 1.0, y: 1.0, w: 0.8, h: 1.4, arch: 0.0, ragged: 0.0 },
            ],
        ),
    );
    assert_layout("hole_touching_edge", solid_slabs(2.0, 2.0, &[WallHole { x: -1.0, y: 1.0, w: 1.0, h: 2.0, arch: 0.0, ragged: 0.0 }]));
}

// ============================================================== windowState ==
#[test]
fn window_state_distribution_matches_the_javascript_sweep() {
    let floors = [-1, 0, 1, 2];
    let damages = [0.0, 0.2, 0.5, 0.8];
    let allow_lit_options = [true, false];
    const N: u32 = 500;

    let name_of = |s: WindowState| match s {
        WindowState::Boarded => "boarded",
        WindowState::Open => "open",
        WindowState::Shuttered => "shuttered",
        WindowState::Ajar => "ajar",
        WindowState::Curtain => "curtain",
        WindowState::Lit => "lit",
        WindowState::Glazed => "glazed",
    };

    let mut idx = 0;
    for &floor in &floors {
        for &damage in &damages {
            for &allow_lit in &allow_lit_options {
                let entry = &golden()["window_state_distribution"][idx];
                assert_eq!(entry["floor"].as_i64().unwrap(), i64::from(floor));
                assert!((entry["damage"].as_f64().unwrap() - damage).abs() < TOL);
                assert_eq!(entry["allowLit"].as_bool().unwrap(), allow_lit);

                let mut rng = Rng::new(0xc0ffee);
                let mut counts: std::collections::BTreeMap<&str, u32> = std::collections::BTreeMap::new();
                for _ in 0..N {
                    let s = window_state(&mut rng, floor, damage, allow_lit);
                    *counts.entry(name_of(s)).or_insert(0) += 1;
                }

                let want_counts = entry["counts"].as_object().expect("counts object");
                for (state, count) in &counts {
                    let want = want_counts.get(*state).map_or(0, |v| v.as_u64().unwrap());
                    assert_eq!(u64::from(*count), want, "floor={floor} damage={damage} allowLit={allow_lit} state={state}");
                }
                for (state, v) in want_counts {
                    let got = counts.get(state.as_str()).copied().unwrap_or(0);
                    assert_eq!(u64::from(got), v.as_u64().unwrap(), "floor={floor} damage={damage} allowLit={allow_lit} state={state} (golden-only key)");
                }
                idx += 1;
            }
        }
    }
}

// ================================================================ facadeWall ==
#[test]
fn facade_wall_buckets_match_the_javascript() {
    let mut asm = Assembler::new(Rng::new(1));
    let pm = fixed_panel_matrix();
    let openings = [
        WallHole { x: -1.0, y: 1.5, w: 0.7, h: 1.0, arch: 0.0, ragged: 0.0 },
        WallHole { x: 1.0, y: 1.5, w: 0.7, h: 1.0, arch: 0.5, ragged: 0.0 },
    ];
    let mut rng = Rng::new(2);
    facade_wall(
        &mut asm,
        &pm,
        FacadeSpec { w: 4.0, h: 3.2, t: 0.3, key: "plaster_cream", openings: &openings, rng: Some(&mut rng), bevel: 0.022, top: WallTop::Flat { jag: 0.0 }, warp: 0.018, paint: None },
    );
    // Triangle count only, not the full (key, verts, tris) tuple: `facadeWall`
    // builds on `wall_panel`, which reuses `weapons::geometry::primitives::
    // extrude` and therefore welds vertices at `1e-6` — exactly the
    // documented `wall_panel` divergence from the source's own never-welded
    // `wallPanel` (see `world_port.rs`'s "wallPanel's documented divergence"
    // and `wall_panel`'s own doc comment). Welding only ever changes vertex
    // *count*, never triangle count, so triangle count is still the
    // "differing count = different algorithm" signal here.
    let got_statics = asm.finalize().statics;
    assert_eq!(got_statics.len(), 1);
    assert_eq!(got_statics[0].key, "plaster_cream");
    let want = golden_buckets("facade_wall");
    assert_eq!(want.len(), 1);
    assert_eq!(got_statics[0].geo.tri_count() as u64, want[0].2, "facade_wall: triangle count");
}

// ================================================================ windowUnit ==
#[test]
fn window_unit_buckets_match_the_javascript_for_every_state() {
    let cases: [(&str, WindowState); 7] = [
        ("boarded", WindowState::Boarded),
        ("open", WindowState::Open),
        ("shuttered", WindowState::Shuttered),
        ("ajar", WindowState::Ajar),
        ("curtain", WindowState::Curtain),
        ("lit", WindowState::Lit),
        ("glazed", WindowState::Glazed),
    ];
    for (name, state) in cases {
        let mut asm = Assembler::new(Rng::new(1));
        let pm = fixed_panel_matrix();
        let o = WallHole { x: 0.0, y: 1.5, w: 1.0, h: 1.4, arch: 0.0, ragged: 0.0 };
        let mut rng = Rng::new(3);
        window_unit(
            &mut asm,
            &pm,
            &o,
            &mut rng,
            WindowOpts {
                t: 0.34,
                frame_key: "wood_dark",
                depth: 0.34 * 0.62,
                state,
                broken: state == WindowState::Open,
                back: true,
                back_set: 0.19,
                no_glass: false,
                sill: true,
                lintel: true,
                grille: true,
                shutters: true,
                shutter_key: "metal_blue",
                curtain: state == WindowState::Curtain,
                curtain_key: "fabric_cream",
            },
        );
        let got = buckets(&asm.finalize().statics);
        let want = golden()["window_unit"][name]["buckets"]
            .as_array()
            .unwrap_or_else(|| panic!("window_unit.{name}.buckets is not an array"))
            .iter()
            .map(|b| (b["key"].as_str().unwrap().to_string(), b["verts"].as_u64().unwrap(), b["tris"].as_u64().unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(got, want, "window_unit[{name}]: per-key vertex/triangle counts");
    }
}

// =================================================================== doorUnit ==
#[test]
fn door_unit_buckets_match_the_javascript() {
    let mut asm = Assembler::new(Rng::new(1));
    let pm = fixed_panel_matrix();
    let o = WallHole { x: 0.0, y: 1.05, w: 1.0, h: 2.1, arch: 0.0, ragged: 0.0 };
    let mut rng = Rng::new(4);
    door_unit(&mut asm, &pm, &o, &mut rng, DoorOpts { t: 0.34, frame_key: "wood_dark", leaf: true, leaf_key: "metal_green", open: 0.4 });
    assert_buckets_match("door_unit", &asm.finalize().statics);
}

// =============================================================== shopfront ==
#[test]
fn shopfront_buckets_match_the_javascript() {
    let mut asm = Assembler::new(Rng::new(1));
    let pm = fixed_panel_matrix();
    let o = WallHole { x: 0.0, y: 1.1, w: 3.0, h: 2.2, arch: 0.0, ragged: 0.0 };
    let mut rng = Rng::new(5);
    shopfront(&mut asm, &pm, &o, &mut rng, ShopfrontOpts { t: 0.34, drop: Some(0.5), counter: true, inside: true });
    assert_buckets_match("shopfront", &asm.finalize().statics);
}

// ================================================================= balcony ==
#[test]
fn balcony_buckets_match_the_javascript_for_both_railing_kinds() {
    let mut asm_metal = Assembler::new(Rng::new(1));
    let pm = fixed_panel_matrix();
    let mut rng = Rng::new(6);
    balcony(&mut asm_metal, &pm, 0.0, 3.0, 1.8, &mut rng, BalconyOpts { depth: 1.15, key: "concrete", railing: BalconyRailing::Metal("metal_rust") });
    assert_buckets_match("balcony_metal", &asm_metal.finalize().statics);

    let mut asm_concrete = Assembler::new(Rng::new(1));
    let mut rng2 = Rng::new(6);
    balcony(&mut asm_concrete, &pm, 0.0, 3.0, 1.8, &mut rng2, BalconyOpts { depth: 1.15, key: "concrete", railing: BalconyRailing::Concrete });
    assert_buckets_match("balcony_concrete", &asm_concrete.finalize().statics);
}

// ================================================================= parapet ==
#[test]
fn parapet_buckets_and_return_value_match_the_javascript() {
    let mut asm = Assembler::new(Rng::new(1));
    let mut rng = Rng::new(7);
    let top = parapet(&mut asm, "roof_screed", 0.0, 0.0, 6.0, 4.0, 8.0, &mut rng, ParapetOpts { h: 0.72, t: 0.24, coping_key: "concrete" });
    assert!((f64::from(top) - golden()["parapet"]["top"].as_f64().unwrap()).abs() < TOL);
    assert_buckets_match("parapet", &asm.finalize().statics);
}

// =================================================================== stairs ==
#[test]
fn stair_run_buckets_and_return_value_match_the_javascript() {
    let mut asm = Assembler::new(Rng::new(1));
    let pm = fixed_panel_matrix();
    let result = stair_run(&mut asm, &pm, 0.0, 0.0, 0.0, 1.2, 6, 0.18, 0.28, StairOpts { key: "concrete", stringer: true, railing: StairRailing::Both });
    let want = &golden()["stair_run"]["result"];
    assert!((f64::from(result.top) - want["top"].as_f64().unwrap()).abs() < TOL);
    assert!((f64::from(result.end_z) - want["endZ"].as_f64().unwrap()).abs() < TOL);
    assert_buckets_match("stair_run", &asm.finalize().statics);
}

// ============================================================ stripedCloth ==
#[test]
fn striped_cloth_buckets_match_the_javascript() {
    let mut asm = Assembler::new(Rng::new(1));
    let pm = fixed_panel_matrix();
    let mut rng = Rng::new(8);
    striped_cloth(
        &mut asm,
        &["fabric_red", "fabric_cream"],
        &pm,
        2.0,
        1.0,
        StripedClothOpts { bands: 4, seg_x: striped_cloth_default_seg_x(4), skip_band: 1, ..StripedClothOpts::default() },
        Some(&mut rng),
    );
    assert_buckets_match("striped_cloth", &asm.finalize().statics);
}

// ================================================================== awning ==
#[test]
fn awning_buckets_and_return_value_match_the_javascript() {
    let mut asm = Assembler::new(Rng::new(1));
    let pm = fixed_panel_matrix();
    let mut rng = Rng::new(9);
    let result = awning(&mut asm, &pm, 0.0, 2.2, 2.0, &mut rng, AwningOpts { depth: 1.5, slope: 0.32, keys: ["fabric_red", "fabric_cream"], legs: true });
    let want = &golden()["awning"]["result"];
    assert!((f64::from(result.x) - want["x"].as_f64().unwrap()).abs() < TOL);
    assert!((f64::from(result.y) - want["y"].as_f64().unwrap()).abs() < TOL);
    assert!((f64::from(result.w) - want["w"].as_f64().unwrap()).abs() < TOL);
    assert!((f64::from(result.d) - want["d"].as_f64().unwrap()).abs() < TOL);
    assert_buckets_match("awning", &asm.finalize().statics);
}

// =============================================================== drainpipe ==
#[test]
fn drainpipe_buckets_match_the_javascript() {
    let mut asm = Assembler::new(Rng::new(1));
    let pm = fixed_panel_matrix();
    let mut rng = Rng::new(10);
    drainpipe(&mut asm, &pm, 0.0, 5.0, 4.8, &mut rng, DrainpipeOpts { r: 0.055, key: "metal_rust", z: -0.075 });
    assert_buckets_match("drainpipe", &asm.finalize().statics);
}

// ============================================================== rubbleMound ==
#[test]
fn rubble_mound_buckets_match_the_javascript() {
    let mut asm = Assembler::new(Rng::new(1));
    let mut rng = Rng::new(11);
    rubble_mound(&mut asm, &mut rng, 0.0, 0.0, 0.0, 2.0, 12, RubbleOpts { key: "concrete" });
    assert_buckets_match("rubble_mound", &asm.finalize().statics);
}

// ============================================================ pockGeometry ==
#[test]
fn pock_geometry_matches_the_javascript_exactly() {
    let mut rng = Rng::new(12);
    let g = pock_geometry(&mut rng, 0.05);
    let want = &golden()["pock_geometry"];
    let want_pos = f64s(&want["pos"]);
    let want_normal = f64s(&want["normal"]);
    let want_color = f64s(&want["color"]);
    assert_eq!(g.pos.len(), want_pos.len());
    for (i, (a, b)) in g.pos.iter().zip(want_pos.iter()).enumerate() {
        assert!((f64::from(*a) - b).abs() < TOL, "pock_geometry.pos[{i}] = {a} vs golden {b}");
    }
    for (i, (a, b)) in g.normal.iter().zip(want_normal.iter()).enumerate() {
        assert!((f64::from(*a) - b).abs() < TOL, "pock_geometry.normal[{i}] = {a} vs golden {b}");
    }
    for (i, (a, b)) in g.color.iter().zip(want_color.iter()).enumerate() {
        assert!((f64::from(*a) - b).abs() < TOL, "pock_geometry.color[{i}] = {a} vs golden {b}");
    }
}

// ============================================================== spallPatch ==
#[test]
fn spall_patch_triangle_soup_matches_the_javascript() {
    let mut rng = Rng::new(13);
    let g = spall_patch(&mut rng, 1.0, 0.8, 0.03);
    assert_triangle_soup_matches_raw("spall_patch", &g.pos, &g.normal, &vec![0.0; g.pos.len() / 3 * 2], &g.index, &fudge_uv(&golden()["spall_patch"]), TOL);
}

/// `spallPatch`'s dump has no `uv` field (the source never sets one) — the
/// shared comparator wants a `uv` array to key the position/normal split on
/// (see `tests/geometry_assert/mod.rs`), so this fills a zero `uv` for every
/// vertex on the golden side, matching the zero `uv` this test also feeds on
/// the `got` side above.
fn fudge_uv(v: &Value) -> Value {
    let mut v = v.clone();
    let n = v["pos"].as_array().unwrap().len() / 3;
    v["uv"] = Value::Array(vec![Value::from(0.0); n * 2]);
    v
}
