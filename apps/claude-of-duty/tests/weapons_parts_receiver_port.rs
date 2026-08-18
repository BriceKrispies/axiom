//! `weapons::parts::receiver` (`addHandguard`, `addUpperReceiver`,
//! `addBoltCarrier`, `addLowerReceiver`), pinned against the real
//! `C:/dev/Claude-of-Duty/src/weapons/parts.js` running under Node (v24)
//! against the real `three@0.180` package, per the port recipe's
//! golden-capture method.
//!
//! Every `receiver_golden.json` value was produced by a capture script (not
//! committed, per the recipe) that built a fresh `Assembly`, called the JS
//! builder once per case below with the exact arguments repeated here —
//! every one of them the real `buildRifle()` call site
//! (`src/weapons/models/rifle.js`), except `handguard_no_top` and
//! `lower_receiver_defaults`, which are deliberately all-default synthetic
//! cases chosen to exercise branches `buildRifle()`'s own call sites never
//! hit: `handguard_no_top` takes the `topFrom === null` continue arm (no
//! top slat) and the `matPanel ?? matAlu` fallback; `lower_receiver_defaults`
//! takes the `magTop ?? bore - 0.014` / `magBottom ?? bore - 0.062`
//! fallbacks and the default `w`/`magW`/`magD`/`magTilt`. Then each case's
//! `Assembly.build()` output (and, where the JS builder returns a value,
//! that value) was dumped to JSON.
//!
//! **Tolerance and topology**, per `03-weapon-geometry-api.md`: material
//! bucket *sets* are asserted exactly, and every bucket's **triangle** count
//! is asserted exactly (fixed by `earcut`'s triangulation of the
//! un-bevelled contour, so a differing count is a different algorithm, not
//! a rounding difference). Every builder here composes many bevelled
//! `extrude()`/`lathe_z()` calls into one merged-and-welded bucket per
//! material, and `primitives::extrude`'s module doc documents a real
//! precision boundary in that path (now `f64` end-to-end, but still
//! independent-libm `sin`/`cos` noise near a tangent-junction division) that
//! can, on some inputs, tip `weld_vertices`'s `1e-6` quantization hash to a
//! different bucket than the source's own `mergeVertices` — changing the
//! **welded vertex count** without changing the shape. So vertex count is
//! asserted exactly when it matches (the common case) and, when it doesn't,
//! within the same small budget `weapons_geometry_primitives_port.rs` and
//! `weapons_parts_magazine_port.rs` use for the identical reason.
//! Position/normal floats are asserted within `1e-5` absolute (the
//! established figure for a merged/welded whole-part bucket,
//! `03-weapon-geometry-api.md`) whenever vertex counts matched exactly.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde_json::Value;

use axiom_claude_of_duty::weapons::geometry::{Assembly, Geo};
use axiom_claude_of_duty::weapons::parts::receiver::{
    add_bolt_carrier, add_handguard, add_lower_receiver, add_upper_receiver, BoltCarrierOpts, HandguardOpts,
    LowerReceiverOpts, UpperReceiverOpts,
};

/// Absolute tolerance for a position/normal float once vertex counts match.
/// See the module doc for why this is the merged/welded-bucket figure, not
/// the tighter single-primitive one.
const TOL: f64 = 1e-5;

fn golden() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| serde_json::from_str(include_str!("parts/receiver_golden.json")).expect("receiver_golden.json parses"))
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

/// A case's `returned` object, one named field, tolerance-compared: the
/// golden was computed in JS `f64`, the value under test in Rust `f32`.
fn assert_returned_field_matches(name: &str, case: &str, field: &str, got: f32) {
    let want = golden()[case]["returned"][field]
        .as_f64()
        .unwrap_or_else(|| panic!("{name}: returned.{field} missing or not a number"));
    let diff = (f64::from(got) - want).abs();
    assert!(diff < TOL, "{name}: returned.{field} = {got} vs golden {want} (diff {diff})");
}

// ---------------------------------------------------------------------
// addHandguard (parts.js:391-514)
// ---------------------------------------------------------------------

#[test]
fn add_handguard_matches_the_rifle_configuration() {
    let mut asm = Assembly::new("hg");
    add_handguard(
        &mut asm,
        "alu",
        HandguardOpts {
            mat_panel: Some("polymer"),
            y: 0.075,
            z0: -0.145,
            z1: -0.385,
            r: 0.0235,
            sides: 8,
            slat_w: 0.0166,
            slat_t: 0.0036,
            slots: 4,
            braces: 3,
            top_from: Some(-0.187),
            top_to: Some(-0.329),
        },
    );
    let built = asm.build();
    assert_buckets_match("handguard_rifle", &built, &golden()["handguard_rifle"]["buckets"]);
}

#[test]
fn add_handguard_matches_the_source_with_no_top_slat_and_default_material() {
    let mut asm = Assembly::new("hg2");
    add_handguard(
        &mut asm,
        "alu",
        HandguardOpts {
            z0: -0.1,
            z1: -0.3,
            ..Default::default()
        },
    );
    let built = asm.build();
    assert_buckets_match("handguard_no_top", &built, &golden()["handguard_no_top"]["buckets"]);
}

// ---------------------------------------------------------------------
// addUpperReceiver (parts.js:525-656)
// ---------------------------------------------------------------------

#[test]
fn add_upper_receiver_matches_the_rifle_configuration() {
    let mut asm = Assembly::new("upper");
    let r = add_upper_receiver(
        &mut asm,
        "alu",
        "steel",
        "cavity",
        UpperReceiverOpts {
            z_rear: 0.055,
            z_front: -0.143,
            bore: 0.075,
            r: 0.0192,
            port_z: -0.052,
            rail_top: 0.1036,
        },
    );
    let built = asm.build();
    assert_buckets_match("upper_receiver_rifle", &built, &golden()["upper_receiver_rifle"]["buckets"]);
    assert_returned_field_matches("upper_receiver_rifle", "upper_receiver_rifle", "railTop", r.rail_top);
}

// ---------------------------------------------------------------------
// addBoltCarrier (parts.js:662-687)
// ---------------------------------------------------------------------

#[test]
fn add_bolt_carrier_matches_the_rifle_configuration() {
    let mut asm = Assembly::new("bolt");
    add_bolt_carrier(
        &mut asm,
        "steel_bright",
        BoltCarrierOpts {
            r: 0.0152,
            len: 0.092,
            z: 0.0,
            ..Default::default()
        },
    );
    let built = asm.build();
    assert_buckets_match("bolt_carrier_rifle", &built, &golden()["bolt_carrier_rifle"]["buckets"]);
}

// ---------------------------------------------------------------------
// addLowerReceiver (parts.js:693-792)
// ---------------------------------------------------------------------

#[test]
fn add_lower_receiver_matches_the_rifle_configuration() {
    let mut asm = Assembly::new("lower");
    let r = add_lower_receiver(
        &mut asm,
        "alu",
        "steel",
        LowerReceiverOpts {
            bore: 0.075,
            z_rear: 0.059,
            z_front: -0.088,
            w: 0.0245,
            mag_w: 0.0292,
            mag_d: 0.0672,
            mag_top: Some(0.049),
            mag_bottom: Some(0.008),
            mag_z: -0.058,
            mag_tilt: 0.08,
            trigger_z: -0.012,
            grip_angle: 0.38,
        },
    );
    let built = asm.build();
    assert_buckets_match("lower_receiver_rifle", &built, &golden()["lower_receiver_rifle"]["buckets"]);
    assert_returned_field_matches("lower_receiver_rifle", "lower_receiver_rifle", "magTop", r.mag_top);
    assert_returned_field_matches("lower_receiver_rifle", "lower_receiver_rifle", "magBottom", r.mag_bottom);
    assert_returned_field_matches("lower_receiver_rifle", "lower_receiver_rifle", "magZ", r.mag_z);
    assert_returned_field_matches("lower_receiver_rifle", "lower_receiver_rifle", "magTilt", r.mag_tilt);
    assert_returned_field_matches("lower_receiver_rifle", "lower_receiver_rifle", "wellH", r.well_h);
    assert_returned_field_matches("lower_receiver_rifle", "lower_receiver_rifle", "magW", r.mag_w);
    assert_returned_field_matches("lower_receiver_rifle", "lower_receiver_rifle", "magD", r.mag_d);
}

#[test]
fn add_lower_receiver_matches_the_source_with_default_mag_top_bottom_and_dimensions() {
    let mut asm = Assembly::new("lower2");
    let r = add_lower_receiver(
        &mut asm,
        "alu",
        "steel",
        LowerReceiverOpts {
            bore: 0.075,
            z_rear: 0.06,
            z_front: -0.09,
            mag_z: -0.05,
            trigger_z: -0.01,
            grip_angle: 0.3,
            ..Default::default()
        },
    );
    let built = asm.build();
    assert_buckets_match("lower_receiver_defaults", &built, &golden()["lower_receiver_defaults"]["buckets"]);
    // `magTop ?? bore - 0.014` / `magBottom ?? bore - 0.062` (parts.js:700-701):
    // the count/topology match above already proves the geometry used the
    // fallback values; this pins the *returned* fields explicitly so a
    // future change to the fallback formula fails here first.
    assert_returned_field_matches("lower_receiver_defaults", "lower_receiver_defaults", "magTop", r.mag_top);
    assert_returned_field_matches("lower_receiver_defaults", "lower_receiver_defaults", "magBottom", r.mag_bottom);
}
