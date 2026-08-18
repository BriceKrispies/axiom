//! `weapons::parts::magazine` (`buildMagazine`, `addRollmark`,
//! `addFrontSight`, `addRearSight`), pinned against the real
//! `C:/dev/Claude-of-Duty/src/weapons/parts.js` running under Node (v24)
//! against the real `three@0.180` package, per the port recipe's
//! golden-capture method.
//!
//! Every `golden_magazine.json` value was produced by a capture script (not
//! committed, per the recipe) that built a fresh `Assembly`, called the
//! JS builder once per case below with the exact arguments repeated here,
//! then dumped each material bucket's `position`/`normal`/`uv`/`index` via
//! `Assembly.build()`. Two goldens are deliberately tiny synthetic
//! parameter sets rather than a real weapon's numbers, chosen only to keep
//! this file's committed JSON small while still exercising the branches a
//! real call site does not: `magazine_edge` hits `segs = 2` (the rib loop
//! `i > 0 && i < segs - 1` never fires, so `merge_all(rib_parts)` is `None`
//! and the `if let Some(ribs)` guard is exercised as false) and
//! `witness = 0` (the witness-hole loop never runs, and `Math.max(1,
//! holes - 1)`'s zero-holes edge is exercised); `rollmark_custom` combines a
//! short 5-entry pattern (covering both the `p == 0` skip and the `p == 3`
//! crossbar arm) with a nonzero `sx`, covering the source's truthy (not
//! nullish) `if (o.sx)` mirror-scale check in the same capture.
//!
//! **Tolerance and topology**, per `03-weapon-geometry-api.md`: material
//! bucket *sets* are asserted exactly, and every bucket's **triangle**
//! count is asserted exactly (fixed by `earcut`'s triangulation of the
//! un-bevelled contour, so a differing count is a different algorithm, not
//! a rounding difference). A whole-part builder like these composes many
//! bevelled `extrude()` calls into one merged bucket, and
//! `primitives::extrude`'s module doc documents a real `f32` precision
//! boundary in that path: a `pts: &[[f32; 2]]` profile's bevel-vector
//! division can tip `weld_vertices`'s `1e-6` quantization hash to a
//! different bucket than the source's own `mergeVertices`, changing the
//! **welded vertex count** without changing the shape. So vertex count is
//! asserted exactly when it matches (the common case) and, when it
//! doesn't, within the same small budget
//! `weapons_geometry_primitives_port.rs` uses for the identical reason.
//! When vertex counts match, position and normal floats are asserted
//! within `1e-5` absolute (composing several sequential `f32` rotations,
//! per part, widens the tolerance `weapons_geometry_port.rs`'s Euler test
//! already establishes for the same reason: real `f32`-vs-`f64` accumulated
//! rounding, not an algorithm defect). `uv` is intentionally not compared
//! index-for-index here: `extrude()`'s `WorldUVGenerator`-equivalent picks
//! its projection axis via a discrete `<` comparison between two
//! side-length magnitudes: on a quad whose sides are nearly equal, a
//! sub-tolerance position difference can flip that axis choice, producing a
//! UV value that differs far more than any float-noise budget while the
//! shape (position, normal, every triangle index) is exactly right.
//! `weapons_geometry_primitives_port.rs` already proves this exact UV
//! algorithm bit-for-bit on isolated, unmerged primitives, where no such
//! tie exists; a whole magazine composes dozens of extrudes, which is
//! exactly where a tie becomes likely.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::Value;

use axiom_claude_of_duty::weapons::geometry::{Assembly, Geo};
use axiom_claude_of_duty::weapons::parts::magazine::{
    add_front_sight, add_rear_sight, add_rollmark, build_magazine, MagazineDims, MagazineOpts, RollmarkOpts,
};

/// Absolute tolerance for a position/normal float once vertex counts match.
/// See the module doc for why this is wider than the `1e-6` primitives use.
const TOL: f64 = 1e-5;

fn golden() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| serde_json::from_str(include_str!("parts/golden_magazine.json")).expect("golden_magazine.json parses"))
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

/// One material bucket's geometry against its golden `{ pos, normal, uv,
/// index }`: triangle count exactly; vertex count exactly if it matches,
/// else within a small budget; position/normal floats only when the vertex
/// count matched exactly (see module doc for both).
fn assert_bucket(name: &str, g: &Geo, want: &Value) {
    let want_pos = f64s(&want["pos"]);
    let want_vert_count = want_pos.len() / 3;
    // A bucket fed by exactly one `asm.add` call never goes through
    // `mergeAll`'s weld pass (`if (clean.length === 1) return clean[0];`,
    // `geometry.js:426`), so it can stay non-indexed if its sole occupant
    // was (e.g. a single un-merged `box()`/`RoundedBoxGeometry`).
    let want_tri_count = match &want["index"] {
        Value::Null => want_vert_count / 3,
        Value::Array(arr) => arr.len() / 3,
        other => panic!("{name}: unexpected index field shape: {other}"),
    };
    assert_eq!(g.tri_count(), want_tri_count, "{name}: triangle count (topology) must match exactly");

    let got_vert_count = g.vert_count();
    if got_vert_count == want_vert_count {
        close_slice(name, "pos", &g.pos, &want_pos);
        close_slice(name, "normal", &g.normal, &f64s(&want["normal"]));
        match &want["index"] {
            Value::Null => assert!(g.index.is_empty(), "{name}: expected non-indexed (JS index is null)"),
            Value::Array(arr) => {
                let want_index: Vec<u32> = arr.iter().map(|x| x.as_u64().unwrap() as u32).collect();
                assert_eq!(g.index, want_index, "{name}: index buffer must match exactly");
            }
            other => panic!("{name}: unexpected index field shape: {other}"),
        }
    } else {
        let delta = got_vert_count.abs_diff(want_vert_count);
        let budget = (want_vert_count / 10).max(8);
        assert!(
            delta <= budget,
            "{name}: vert_count {got_vert_count} vs golden {want_vert_count} (delta {delta} > budget {budget})"
        );
    }
}

/// Compares a full `Assembly::build()` output against a golden
/// `{ matKey: { pos, normal, uv, index } }` object: the bucket *set* must
/// match exactly (a missing or extra material bucket is a structural bug),
/// and every bucket's geometry is compared with [`assert_bucket`].
fn assert_buckets_match(case: &str, built: &BTreeMap<String, Geo>, want: &Value) {
    let want_obj = want.as_object().unwrap_or_else(|| panic!("{case}: golden buckets must be an object"));
    let mut want_keys: Vec<&String> = want_obj.keys().collect();
    want_keys.sort();
    let mut got_keys: Vec<&String> = built.keys().collect();
    got_keys.sort();
    assert_eq!(got_keys, want_keys, "{case}: material bucket set must match exactly");

    for (mat, g) in built {
        assert_bucket(&format!("{case}:{mat}"), g, &want[mat]);
    }
}

// ---------------------------------------------------------------------
// buildMagazine (parts.js:1082-1202)
// ---------------------------------------------------------------------

#[test]
fn build_magazine_matches_the_rifle_configuration() {
    let mut asm = Assembly::new("mag-rifle");
    let dims = build_magazine(
        &mut asm,
        (),
        MagazineOpts {
            w: 0.0255,
            d: 0.0655,
            len: 0.212,
            curve: 0.03,
            segs: 8,
            witness: 4,
            poly: "polymer",
            ..Default::default()
        },
    );
    assert_eq!(
        dims,
        MagazineDims {
            len: 0.212,
            w: 0.0255,
            d: 0.0655,
        },
        "buildMagazine return value ({{ len, w, d }}, parts.js:1201)"
    );

    let built = asm.build();
    let case = golden()["magazine_rifle"].clone();
    assert_buckets_match("magazine_rifle", &built, &case["buckets"]);
}

/// `segs = 2` (the rib loop never fires -> `merge_all(rib_parts)` is `None`)
/// and `witness = 0` (the witness-hole loop never runs). See module doc.
#[test]
fn build_magazine_segs_two_and_witness_zero_skip_ribs_and_witness_holes() {
    let mut asm = Assembly::new("mag-edge");
    let dims = build_magazine(
        &mut asm,
        (),
        MagazineOpts {
            w: 0.018,
            d: 0.024,
            len: 0.05,
            curve: 0.008,
            segs: 2,
            witness: 0,
            case_len: 0.0192,
            rim_r: 0.00478,
            bullet_len: 0.012,
            poly: "polymer",
        },
    );
    assert_eq!(
        dims,
        MagazineDims {
            len: 0.05,
            w: 0.018,
            d: 0.024,
        }
    );

    let built = asm.build();
    // No witness holes were requested, so the `cavity` bucket must be absent
    // entirely -- not present-and-empty.
    assert!(!built.contains_key("cavity"), "witness = 0 must produce no cavity bucket");
    let case = golden()["magazine_edge"].clone();
    assert_buckets_match("magazine_edge", &built, &case["buckets"]);
}

// ---------------------------------------------------------------------
// addRollmark (parts.js:1646-1675)
// ---------------------------------------------------------------------

/// The default `pattern`/`h`/`stroke`/`depth`/`pitch`, positioned per the
/// real call site (`models/rifle.js:123`: `{ x: -0.0149, y: 0.0355, z:
/// -0.031, h: 0.0036 }` -- `h` here just restates the default).
#[test]
fn add_rollmark_default_pattern_matches() {
    let mut asm = Assembly::new("rollmark-default");
    add_rollmark(
        &mut asm,
        "cavity",
        RollmarkOpts {
            x: Some(-0.0149),
            y: Some(0.0355),
            z: Some(-0.031),
            h: 0.0036,
            ..Default::default()
        },
    );
    let built = asm.build();
    assert_buckets_match("rollmark_default", &built, &golden()["rollmark_default"]);
}

/// A short custom `pattern` (exercising the `p == 0` skip and the `p == 3`
/// crossbar arm with fewer strokes) plus a nonzero `sx`, exercising the
/// source's truthy (not nullish) `if (o.sx)` mirror-scale check.
#[test]
fn add_rollmark_custom_pattern_and_sx_mirror_matches() {
    let mut asm = Assembly::new("rollmark-custom");
    add_rollmark(
        &mut asm,
        "cavity",
        RollmarkOpts {
            h: 0.0024,
            pitch: 0.0014,
            pattern: vec![2, 3, 1, 0, 2],
            x: Some(-0.0149),
            y: Some(0.0272),
            z: Some(-0.033),
            sx: Some(-1.0),
            ..Default::default()
        },
    );
    let built = asm.build();
    assert_buckets_match("rollmark_custom", &built, &golden()["rollmark_custom"]);
}

/// `o.sx: Some(0.0)` must NOT trigger the mirror scale (`parts.js:1672`'s
/// `if (o.sx)` is falsy on `0`, unlike a `??` nullish check) -- a source
/// quirk this port reproduces via `.filter(|&s| s != 0.0)`, not a bug.
#[test]
fn add_rollmark_sx_zero_is_falsy_not_a_mirror_scale() {
    let mut with_zero = Assembly::new("rollmark-sx-zero");
    add_rollmark(
        &mut with_zero,
        "cavity",
        RollmarkOpts {
            sx: Some(0.0),
            ..Default::default()
        },
    );
    let mut without = Assembly::new("rollmark-sx-none");
    add_rollmark(&mut without, "cavity", RollmarkOpts::default());

    let built_zero = with_zero.build();
    let built_none = without.build();
    assert_eq!(built_zero["cavity"].pos, built_none["cavity"].pos, "sx: Some(0.0) must behave like sx: None");
}

// ---------------------------------------------------------------------
// addFrontSight (parts.js:1678-1717)
// ---------------------------------------------------------------------

#[test]
fn add_front_sight_up_matches() {
    let mut asm = Assembly::new("front-sight-up");
    add_front_sight(&mut asm, "steel", "alu", 0.0, 0.02, -0.358, true);
    let built = asm.build();
    assert_buckets_match("front_sight_up", &built, &golden()["front_sight_up"]);
}

#[test]
fn add_front_sight_folded_matches() {
    let mut asm = Assembly::new("front-sight-folded");
    add_front_sight(&mut asm, "polymer", "alu", 0.0, 0.02, -0.358, false);
    let built = asm.build();
    assert_buckets_match("front_sight_folded", &built, &golden()["front_sight_folded"]);
}

// ---------------------------------------------------------------------
// addRearSight (parts.js:1720-1778)
// ---------------------------------------------------------------------

#[test]
fn add_rear_sight_up_matches() {
    let mut asm = Assembly::new("rear-sight-up");
    add_rear_sight(&mut asm, "steel", "alu", 0.0, 0.02, -0.112, true);
    let built = asm.build();
    assert_buckets_match("rear_sight_up", &built, &golden()["rear_sight_up"]);
}

#[test]
fn add_rear_sight_folded_matches() {
    let mut asm = Assembly::new("rear-sight-folded");
    add_rear_sight(&mut asm, "polymer", "alu", 0.0, 0.02, -0.112, false);
    let built = asm.build();
    assert_buckets_match("rear_sight_folded", &built, &golden()["rear_sight_folded"]);
}
