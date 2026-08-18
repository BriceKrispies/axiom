//! The ported weapon geometry buffer, merge layer, and `Assembly` builder,
//! pinned against `C:/dev/Claude-of-Duty/src/weapons/geometry.js` and the
//! real `three@0.180` `BufferGeometryUtils`/`Euler`/`Matrix3` it leans on.
//!
//! Golden values below were captured by running small Node scripts against
//! the real `three` package installed in `C:/dev/Claude-of-Duty` (see the
//! port recipe's golden-capture method). Everything here is built only from
//! `+ - * /`, comparisons, and integer indices (no `sin`/`cos`/`sqrt` in the
//! merge/weld path), so those are asserted exactly; the one path that does
//! involve trig — Euler-to-quaternion composition — is asserted within
//! `1e-5`, wide enough to absorb both `f32` (this port) vs `f64` (the
//! JavaScript) precision and the `f32` literal's own rounding.

use std::collections::BTreeMap;

use axiom_claude_of_duty::weapons::geometry::{merge_all, Assembly, Geo, Xform};
use axiom_math::{Mat4, Quat, Vec3};

fn assert_close(actual: f32, expected: f32, label: &str) {
    assert!(
        (actual - expected).abs() < 1e-5,
        "{label}: expected {expected}, got {actual}"
    );
}

fn assert_slice_close(actual: &[f32], expected: &[f32], label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label}: length mismatch");
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        assert_close(*a, *e, &format!("{label}[{i}]"));
    }
}

// ---------------------------------------------------------------------
// Geo: vert_count / tri_count (geometry.js:441-444)
// ---------------------------------------------------------------------

#[test]
fn vert_count_and_tri_count_match_indexed_and_non_indexed_shapes() {
    // Non-indexed: a single triangle, 3 verts, 1 tri.
    let tri = Geo {
        pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        ..Geo::default()
    };
    assert_eq!(tri.vert_count(), 3);
    assert_eq!(tri.tri_count(), 1);

    // Indexed: a quad, 4 verts, 2 tris via 6 indices.
    let quad = Geo {
        pos: vec![
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0,
        ],
        index: vec![0, 1, 2, 0, 2, 3],
        ..Geo::default()
    };
    assert_eq!(quad.vert_count(), 4);
    assert_eq!(quad.tri_count(), 2);
}

// ---------------------------------------------------------------------
// Geo::normalize_attributes (geometry.js:32-45)
// ---------------------------------------------------------------------

#[test]
fn normalize_attributes_fills_missing_uv_and_computes_missing_flat_normal() {
    let mut g = Geo {
        pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        ..Geo::default()
    };
    g.normalize_attributes();
    assert_eq!(g.uv, vec![0.0; 6]);
    // Non-indexed triangle soup: computeVertexNormals gives every vertex the
    // same flat face normal, unit-length.
    assert_slice_close(&g.normal, &[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0], "normal");
}

#[test]
fn normalize_attributes_leaves_present_attributes_alone() {
    let mut g = Geo {
        pos: vec![0.0, 0.0, 0.0],
        normal: vec![1.0, 2.0, 3.0],
        uv: vec![0.5, 0.5],
        ..Geo::default()
    };
    g.normalize_attributes();
    assert_eq!(g.normal, vec![1.0, 2.0, 3.0]);
    assert_eq!(g.uv, vec![0.5, 0.5]);
}

// ---------------------------------------------------------------------
// Geo::flip_winding (geometry.js:82-100)
// ---------------------------------------------------------------------

#[test]
fn flip_winding_swaps_the_first_and_last_index_of_every_triangle_and_negates_normals() {
    let mut g = Geo {
        pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        normal: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
        uv: vec![0.0; 6],
        index: vec![0, 1, 2],
    };
    g.flip_winding();
    assert_eq!(g.index, vec![2, 1, 0]);
    assert_eq!(g.normal, vec![0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0]);
}

// ---------------------------------------------------------------------
// Assembly::add: negative-scale winding flip (geometry.js:380-396)
//
// Golden from a real `three@0.180` script: a single indexed triangle
// (0,0,0)-(1,0,0)-(0,1,0), index [0,1,2], transformed with
// `{ sx: -1, sy: 1, sz: 1 }` and no rotation/translation.
// ---------------------------------------------------------------------

#[test]
fn add_with_negative_scale_determinant_flips_winding_and_position_and_normal() {
    let tri = Geo {
        pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        normal: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
        uv: vec![0.0; 6],
        index: vec![0, 1, 2],
    };

    let mut asm = Assembly::new("test");
    asm.add(
        tri,
        "mat",
        Some(Xform {
            sx: -1.0,
            ..Xform::default()
        }),
    );
    let built = asm.build();
    let g = built.get("mat").expect("bucket must exist");

    // `mergeAll` on a length-1 list returns the geometry as-is: still
    // indexed, no re-weld.
    assert_eq!(g.index, vec![2, 1, 0]);
    assert_slice_close(
        &g.pos,
        &[0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        "pos",
    );
    assert_slice_close(
        &g.normal,
        &[0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0],
        "normal",
    );
}

// ---------------------------------------------------------------------
// Assembly::add: Euler 'XYZ' rotation order (geometry.js:384-385).
//
// Golden from a real `three@0.180` script:
//   new THREE.Euler(0.3, -0.5, 0.7, 'XYZ')
//   new THREE.Quaternion().setFromEuler(e)
// gives (x, y, z, w) =
//   (0.052132410889547995, -0.2794438940784743,
//    0.29377717233096856, 0.9126271389863014).
//
// This is `qx * qy * qz` (Hamilton product); `axiom_math::Quat::from_euler_xyz`
// composes `qz * qy * qx` instead and gives a genuinely different rotation
// for the same angles — captured separately as
// (0.21989576632910457, -0.1801458579968856, 0.36323736972823584,
//  0.8872721876797527). `Assembly::add` must reproduce the first, not the
// second.
// ---------------------------------------------------------------------

#[test]
fn add_rotation_matches_three_euler_xyz_order_not_axiom_math_from_euler_xyz() {
    let point = Geo {
        pos: vec![1.0, 0.0, 0.0],
        normal: vec![0.0, 0.0, 1.0],
        uv: vec![0.0, 0.0],
        index: vec![],
    };

    let mut asm = Assembly::new("test");
    asm.add(
        point,
        "mat",
        Some(Xform {
            rx: 0.3,
            ry: -0.5,
            rz: 0.7,
            ..Xform::default()
        }),
    );
    let built = asm.build();
    let g = built.get("mat").expect("bucket must exist");

    let three_xyz = Quat::new(
        0.052132410889547995,
        -0.2794438940784743,
        0.29377717233096856,
        0.9126271389863014,
    );
    let expected_pos = three_xyz.rotate(Vec3::new(1.0, 0.0, 0.0));
    let expected_normal = three_xyz.rotate(Vec3::new(0.0, 0.0, 1.0));

    assert_slice_close(
        &g.pos,
        &[expected_pos.x, expected_pos.y, expected_pos.z],
        "pos",
    );
    assert_slice_close(
        &g.normal,
        &[expected_normal.x, expected_normal.y, expected_normal.z],
        "normal",
    );

    // Sanity: the axiom_math from_euler_xyz order gives a visibly different
    // answer for the same angles, so a regression to that order would fail
    // the assertions above rather than passing by coincidence.
    let axiom_order = Quat::new(
        0.21989576632910457,
        -0.1801458579968856,
        0.36323736972823584,
        0.8872721876797527,
    );
    let diverges = axiom_order.rotate(Vec3::new(1.0, 0.0, 0.0));
    let disagreement = ((diverges.x - expected_pos.x).powi(2)
        + (diverges.y - expected_pos.y).powi(2)
        + (diverges.z - expected_pos.z).powi(2))
    .sqrt();
    assert!(
        disagreement > 0.05,
        "the two Euler orders must disagree for this to be a meaningful pin"
    );
}

// ---------------------------------------------------------------------
// merge_all: two disjoint triangles (geometry.js:423-438), plain
// concatenation, no welding.
//
// Golden from a real `three@0.180` script running the exact `mergeAll`
// sequence (toNonIndexed -> normalizeAttributes -> mergeGeometries ->
// mergeVertices(1e-6) -> normalizeAttributes) on two triangles that share no
// vertices.
// ---------------------------------------------------------------------

#[test]
fn merge_all_concatenates_disjoint_triangles_with_identity_index() {
    let a = Geo {
        pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        ..Geo::default()
    };
    let b = Geo {
        pos: vec![2.0, 0.0, 0.0, 3.0, 0.0, 0.0, 2.0, 1.0, 0.0],
        ..Geo::default()
    };

    let merged = merge_all(vec![a, b]).expect("two geometries must merge");
    assert_eq!(merged.vert_count(), 6);
    assert_slice_close(
        &merged.pos,
        &[
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 2.0, 0.0, 0.0, 3.0, 0.0, 0.0, 2.0, 1.0, 0.0,
        ],
        "pos",
    );
    assert_slice_close(
        &merged.normal,
        &[
            0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0,
        ],
        "normal",
    );
    assert_eq!(merged.uv, vec![0.0; 12]);
    assert_eq!(merged.index, vec![0, 1, 2, 3, 4, 5]);
}

// ---------------------------------------------------------------------
// merge_all -> mergeVertices: genuinely coincident vertices weld
// (BufferGeometryUtils.js:644-800), a hard edge (mismatched normals) does
// not.
//
// Golden from a real `three@0.180` script: a unit square split on its
// diagonal into two consistently-wound (both +Z-facing) triangles sharing
// the edge (0,0,0)-(1,1,0). `mergeVertices(merged, 1e-6)` welds 6 vertices
// down to 4, with the shared-edge pair each collapsing to one entry.
// ---------------------------------------------------------------------

#[test]
fn merge_all_welds_coincident_position_and_normal_vertices() {
    // Both triangles pre-carry the flat normal `computeVertexNormals` would
    // derive, so this test pins the weld itself, not normal computation.
    let a = Geo {
        pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0],
        normal: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
        uv: vec![0.0; 6],
        index: vec![],
    };
    let b = Geo {
        pos: vec![0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0],
        normal: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
        uv: vec![0.0; 6],
        index: vec![],
    };

    let welded = merge_all(vec![a, b]).expect("two geometries must merge");
    assert_eq!(welded.vert_count(), 4);
    assert_slice_close(
        &welded.pos,
        &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0],
        "pos",
    );
    assert_eq!(welded.index, vec![0, 1, 2, 0, 2, 3]);
}

// ---------------------------------------------------------------------
// merge_all edge cases (geometry.js:424-426).
// ---------------------------------------------------------------------

#[test]
fn merge_all_of_empty_list_is_none() {
    assert!(merge_all(Vec::new()).is_none());
}

#[test]
fn merge_all_of_a_single_geometry_returns_it_unchanged() {
    // `if (clean.length === 1) return clean[0];` — no non-indexing, no
    // normalize, no weld. An intentionally "dirty" single geometry (missing
    // uv, non-canonical index) proves it passes through untouched.
    let g = Geo {
        pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        normal: vec![9.0, 9.0, 9.0, 9.0, 9.0, 9.0, 9.0, 9.0, 9.0],
        uv: Vec::new(),
        index: vec![0, 1, 2],
    };
    let out = merge_all(vec![g.clone()]).unwrap();
    assert_eq!(out, g);
}

// ---------------------------------------------------------------------
// Geo::apply (BufferGeometry.applyMatrix4 + getNormalMatrix): the point path
// is a direct Mat4::transform_point sanity check, and the normal path is
// checked with a non-uniform scale where the raw-matrix and normal-matrix
// answers would visibly differ if apply() used the wrong one.
// ---------------------------------------------------------------------

#[test]
fn apply_transforms_points_directly_and_normals_via_the_inverse_transpose() {
    let mut g = Geo {
        pos: vec![1.0, 0.0, 0.0],
        normal: vec![1.0, 0.0, 0.0],
        uv: vec![0.5, 0.5],
        index: vec![],
    };
    // Non-uniform scale: (2, 1, 1) with a translation. The RAW matrix would
    // send the normal (1,0,0) to (2,0,0) -> normalized (1,0,0) (coincidence
    // for this axis-aligned case) — so exercise a scale that is NOT aligned
    // with the normal to actually distinguish raw vs normal-matrix. Use a
    // scale of (1, 2, 1) against a normal of (1, 0, 0): the normal matrix is
    // diag(1, 1/2, 1), leaving (1,0,0) unchanged after normalize, which is
    // also degenerate. Use a diagonal-off-axis normal instead.
    g.normal = vec![1.0, 1.0, 0.0];
    let m = Mat4::translation(Vec3::new(5.0, 0.0, 0.0)).multiply(Mat4::scale(Vec3::new(1.0, 2.0, 1.0)));
    g.apply(&m);

    // Position: (1,0,0) scaled by (1,2,1) then translated by (5,0,0) -> (6,0,0).
    assert_slice_close(&g.pos, &[6.0, 0.0, 0.0], "pos");

    // Normal matrix for diag(1,2,1) is diag(1, 1/2, 1); applied to (1,1,0)
    // gives (1, 0.5, 0), normalized to (1,0.5,0)/|..| — the raw matrix would
    // instead give (1,2,0) normalized, a visibly different direction.
    let raw_matrix_answer = Vec3::new(1.0, 2.0, 0.0).normalize().unwrap();
    let normal_matrix_answer = Vec3::new(1.0, 0.5, 0.0).normalize().unwrap();
    assert!(
        (g.normal[0] - raw_matrix_answer.x).abs() > 1e-3
            || (g.normal[1] - raw_matrix_answer.y).abs() > 1e-3,
        "apply() must not use the raw matrix for normals"
    );
    assert_slice_close(
        &g.normal,
        &[normal_matrix_answer.x, normal_matrix_answer.y, normal_matrix_answer.z],
        "normal",
    );

    // uv and index are untouched by applyMatrix4.
    assert_eq!(g.uv, vec![0.5, 0.5]);
    assert!(g.index.is_empty());
}

// ---------------------------------------------------------------------
// Assembly::build determinism: BTreeMap, not HashMap.
// ---------------------------------------------------------------------

#[test]
fn build_returns_a_deterministically_ordered_map_and_clears_buckets_but_not_nodes() {
    let mut asm = Assembly::new("rig");
    asm.node("muzzle", 0.0, 0.0, -0.5, 0.0, 0.0, 0.0);
    asm.add(
        Geo {
            pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            ..Geo::default()
        },
        "zebra",
        None,
    );
    asm.add(
        Geo {
            pos: vec![1.0, 1.0, 1.0, 2.0, 1.0, 1.0, 1.0, 2.0, 1.0],
            ..Geo::default()
        },
        "alpha",
        None,
    );

    let built: BTreeMap<String, Geo> = asm.build();
    let keys: Vec<&String> = built.keys().collect();
    assert_eq!(keys, vec!["alpha", "zebra"]);

    // build() clears buckets (a second call yields nothing) but leaves nodes.
    let second = asm.build();
    assert!(second.is_empty());
    assert_eq!(asm.nodes().len(), 1);
    assert!(asm.nodes().contains_key("muzzle"));
    assert_eq!(asm.name(), "rig");
}

// ---------------------------------------------------------------------
// Assembly::add_mirrored: same piece on both sides (geometry.js:399-403).
// ---------------------------------------------------------------------

#[test]
fn add_mirrored_places_the_piece_at_negated_x_and_negated_sx() {
    let g = Geo {
        pos: vec![1.0, 0.0, 0.0],
        normal: vec![1.0, 0.0, 0.0],
        uv: vec![0.0, 0.0],
        index: vec![],
    };
    let mut asm = Assembly::new("test");
    asm.add_mirrored(
        g,
        "mat",
        Xform {
            x: 2.0,
            ..Xform::default()
        },
    );
    // The single bucket now holds two pieces; merge them via build() and
    // check the vertex count reflects both (2 verts, since each geo is a
    // single point).
    let built = asm.build();
    let merged = built.get("mat").unwrap();
    assert_eq!(merged.vert_count(), 2);
    // First copy at x = 1 + 2 = 3; mirrored copy at x = -(2) + (-1) = -3
    // (translate x=-2, then sx=-1 applied to local x=1 -> -1, then no
    // further translate composition beyond the matrix; check both x values
    // are present regardless of merge order).
    let mut xs: Vec<f32> = merged.pos.iter().copied().step_by(3).collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_slice_close(&xs, &[-3.0, 3.0], "mirrored x positions");
}
