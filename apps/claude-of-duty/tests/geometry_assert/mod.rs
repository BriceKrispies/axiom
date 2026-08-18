//! Shared triangle-soup comparison helper for the weapon-geometry golden
//! test suites (`weapons_geometry_primitives_port.rs`,
//! `weapons_parts_hardware_port.rs`, `weapons_parts_magazine_port.rs`,
//! `weapons_parts_controls_port.rs`).
//!
//! **Why not compare the raw welded buffers.** Two independently-rounded
//! `f64::sin`/`f64::cos` implementations (Rust's libm and V8's) can disagree
//! by roughly one ULP. At a handful of near-degenerate junctions (a
//! `round_rect` corner built exactly tangent to its adjacent straight edge —
//! see `primitives::extrude`'s module doc) that ULP noise gets amplified by
//! a near-zero-denominator division in `get_bevel_vec`, and can nudge a
//! vertex across `weld_vertices`'s `1e-6` quantization grid — merging a
//! different set of vertices than the JavaScript source did. Once that
//! happens, the welded **vertex count**, the welded **vertex order**, and
//! even (per-vertex) **which nearby point survives the merge** are no longer
//! reliable comparison keys, even though the underlying triangles are
//! (almost always) geometrically identical: a bucket that welds a different
//! pair can keep an equal vertex count while still landing a merged vertex
//! on the wrong side of a `~1e-4`-scale gap between two originally-close
//! points.
//!
//! **What a triangle-soup comparison does instead.** Expand the index
//! buffer into actual triangles — three whole corners per triangle,
//! duplicating a shared vertex across every triangle that touches it, i.e.
//! exactly undoing what welding collapsed away. Canonicalize each triangle
//! by rotating its three corners so the smallest (by a fixed field order)
//! sorts first — never a free sort of the three corners, which would also
//! discard winding and hide a flipped-normal bug. Sort the whole triangle
//! list into a deterministic order. Compare the two sorted lists elementwise
//! within a tolerance.
//!
//! This is invariant to vertex ordering and to weld decisions — exactly the
//! two things a libm ULP difference can legitimately change — while staying
//! fully sensitive to a triangle that is actually in the wrong place, the
//! wrong size, or facing the wrong way. **Triangle count must still match
//! exactly**; only vertex count is allowed to differ, and this module never
//! looks at vertex count at all.
//!
//! A residual that survives this comparison (a real per-corner position
//! difference bigger than the tolerance, not merely a different weld
//! decision) is a genuine geometric divergence, not a bookkeeping artifact —
//! see each call site for whether that happened and what was measured.

use serde_json::Value;

use axiom_claude_of_duty::weapons::geometry::Geo;

/// One triangle corner: position, normal, and uv, always widened to `f64` so
/// the comparison and sort keys are computed at consistent precision
/// regardless of whether the source was `Geo`'s `f32` or golden JSON's `f64`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Corner {
    pos: [f64; 3],
    normal: [f64; 3],
    uv: [f64; 2],
}

type Triangle = [Corner; 3];

fn f64s(v: &Value) -> Vec<f64> {
    v.as_array()
        .unwrap_or_else(|| panic!("expected an array, got {v}"))
        .iter()
        .map(|x| x.as_f64().unwrap_or_else(|| panic!("not a number: {x}")))
        .collect()
}

fn corner(pos: &[f64], normal: &[f64], uv: &[f64], i: usize) -> Corner {
    Corner {
        pos: [pos[i * 3], pos[i * 3 + 1], pos[i * 3 + 2]],
        normal: [normal[i * 3], normal[i * 3 + 1], normal[i * 3 + 2]],
        uv: [uv[i * 2], uv[i * 2 + 1]],
    }
}

/// Expands a flat `pos`/`normal`/`uv` attribute set plus an optional `index`
/// buffer into a triangle list — three whole corners per triangle. Mirrors
/// `Geo::tri_count`'s own indexed/non-indexed branch (`geo.rs`).
fn triangles(pos: &[f64], normal: &[f64], uv: &[f64], index: &[u32]) -> Vec<Triangle> {
    let vert_count = pos.len() / 3;
    let tri_indices: Vec<[usize; 3]> = if index.is_empty() {
        (0..vert_count).step_by(3).map(|i| [i, i + 1, i + 2]).collect()
    } else {
        index.chunks_exact(3).map(|t| [t[0] as usize, t[1] as usize, t[2] as usize]).collect()
    };
    tri_indices
        .into_iter()
        .map(|[a, b, c]| [corner(pos, normal, uv, a), corner(pos, normal, uv, b), corner(pos, normal, uv, c)])
        .collect()
}

fn corner_key(c: &Corner) -> [f64; 8] {
    [c.pos[0], c.pos[1], c.pos[2], c.normal[0], c.normal[1], c.normal[2], c.uv[0], c.uv[1]]
}

const CORNER_FIELDS: [&str; 8] = ["pos.x", "pos.y", "pos.z", "normal.x", "normal.y", "normal.z", "uv.u", "uv.v"];

/// Sort/canonicalization grid, one per field group — deliberately **coarser**
/// than any residual this suite has measured (position residuals up to
/// `~1.8e-3`, see `weapons_geometry_primitives_port.rs` and
/// `weapons_parts_magazine_port.rs`'s `TOPOLOGY_ONLY` doc), and coarser than
/// the `~1e-6` libm-ULP noise. This is **not** the comparison tolerance —
/// it never touches [`max_diff`], which still compares raw, unquantized
/// floats. It exists only to make the *sort order* stable in the presence of
/// that noise.
///
/// Why this is needed: a real mesh (an extruded/lathed part) routinely has
/// many corners that share a near-constant coordinate across a wide span of
/// the *other* coordinates — e.g. every vertex on a stock's flat side shares
/// `pos.x` to within noise, for its whole length. A raw-float lexicographic
/// sort keyed on that near-constant field first does not "fall through" to
/// the next field the way an *exact* tie would: two noisy-but-nearly-equal
/// `f64`s still compare strictly less/greater, so the sort order is
/// effectively decided by sub-tolerance noise instead of real geometry —
/// observed directly: quantizing (below) moved a genuinely mismatched pair
/// (triangles ~1.2cm apart in `z`) back into correct correspondence.
/// Rounding every field to a grid coarser than the noise turns those
/// near-ties into *real* ties, so the comparator falls through to the next
/// field — which does discriminate — exactly as an exact-tie lexicographic
/// sort would.
const POS_GRID: f64 = 5e-3;
const NORMAL_GRID: f64 = 2e-2;
const UV_GRID: f64 = 1e-2;

fn quantize(x: f64, grid: f64) -> i64 {
    (x / grid).round() as i64
}

fn sort_key(c: &Corner) -> [i64; 8] {
    [
        quantize(c.pos[0], POS_GRID),
        quantize(c.pos[1], POS_GRID),
        quantize(c.pos[2], POS_GRID),
        quantize(c.normal[0], NORMAL_GRID),
        quantize(c.normal[1], NORMAL_GRID),
        quantize(c.normal[2], NORMAL_GRID),
        quantize(c.uv[0], UV_GRID),
        quantize(c.uv[1], UV_GRID),
    ]
}

/// Orders two corners primarily by [`sort_key`] (a noise-stable grid), and
/// only falls back to the exact [`corner_key`] floats to break a genuine
/// grid tie deterministically — which, once two corners tie on the grid,
/// means they are already within one grid cell of each other on every
/// field, so this tie-break cannot itself introduce a mismatched pairing.
fn cmp_corner(a: &Corner, b: &Corner) -> std::cmp::Ordering {
    let (qa, qb) = (sort_key(a), sort_key(b));
    qa.cmp(&qb).then_with(|| {
        let (ka, kb) = (corner_key(a), corner_key(b));
        ka.iter()
            .zip(kb.iter())
            .map(|(x, y)| x.total_cmp(y))
            .find(|o| !o.is_eq())
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

/// Rotates a triangle's three corners so the smallest (per [`cmp_corner`])
/// sorts first, **without** freely reordering the other two — a free
/// permutation of all three corners would collapse a front-facing and a
/// back-facing (flipped-winding) triangle into the same canonical form,
/// silently hiding exactly the bug this comparison exists to catch.
fn canonicalize(tri: Triangle) -> Triangle {
    let start = (0..3usize).min_by(|&a, &b| cmp_corner(&tri[a], &tri[b])).unwrap_or(0);
    [tri[start], tri[(start + 1) % 3], tri[(start + 2) % 3]]
}

fn cmp_triangle(a: &Triangle, b: &Triangle) -> std::cmp::Ordering {
    cmp_corner(&a[0], &b[0]).then_with(|| cmp_corner(&a[1], &b[1])).then_with(|| cmp_corner(&a[2], &b[2]))
}

fn sorted_canonical(tris: Vec<Triangle>) -> Vec<Triangle> {
    let mut out: Vec<Triangle> = tris.into_iter().map(canonicalize).collect();
    out.sort_by(cmp_triangle);
    out
}

fn geo_soup(g: &Geo) -> Vec<Triangle> {
    let pos: Vec<f64> = g.pos.iter().map(|&x| f64::from(x)).collect();
    let normal: Vec<f64> = g.normal.iter().map(|&x| f64::from(x)).collect();
    let uv: Vec<f64> = g.uv.iter().map(|&x| f64::from(x)).collect();
    sorted_canonical(triangles(&pos, &normal, &uv, &g.index))
}

fn golden_soup(want: &Value) -> Vec<Triangle> {
    let pos = f64s(&want["pos"]);
    let normal = f64s(&want["normal"]);
    let uv = f64s(&want["uv"]);
    let index: Vec<u32> = match &want["index"] {
        Value::Null => Vec::new(),
        Value::Array(arr) => arr
            .iter()
            .map(|x| x.as_u64().unwrap_or_else(|| panic!("index entry not a u64: {x}")) as u32)
            .collect(),
        other => panic!("unexpected index field shape: {other}"),
    };
    sorted_canonical(triangles(&pos, &normal, &uv, &index))
}

/// The worst elementwise absolute difference between two same-length,
/// same-order (already canonicalized + sorted) triangle lists, plus where it
/// was found — so a failure reports the exact measured deviation, not just
/// "not equal".
struct SoupDiff {
    max: f64,
    at: String,
}

fn max_diff(got: &[Triangle], want: &[Triangle]) -> SoupDiff {
    let mut worst = SoupDiff {
        max: 0.0,
        at: "no corners compared".to_string(),
    };
    got.iter().zip(want.iter()).enumerate().for_each(|(ti, (gt, wt))| {
        gt.iter().zip(wt.iter()).enumerate().for_each(|(ci, (gc, wc))| {
            let (gk, wk) = (corner_key(gc), corner_key(wc));
            gk.iter().zip(wk.iter()).enumerate().for_each(|(fi, (a, b))| {
                let d = (a - b).abs();
                if d > worst.max {
                    worst = SoupDiff {
                        max: d,
                        at: format!("triangle[{ti}].corner[{ci}].{}", CORNER_FIELDS[fi]),
                    };
                }
            });
        });
    });
    worst
}

/// Triangle-soup comparison: invariant to vertex ordering and to weld
/// decisions (see the module doc), while staying fully sensitive to a
/// triangle that is actually in the wrong place, the wrong size, or facing
/// the wrong way.
///
/// Triangle count is asserted exactly. Every corner's position/normal/uv is
/// then compared within `tol` absolute, against the same canonicalized,
/// sorted representation on both sides. On failure the panic reports the
/// single worst-deviating field and its magnitude — the number a caller
/// needs to tell a real geometric divergence from noise.
pub fn assert_triangle_soup_matches(name: &str, g: &Geo, want: &Value, tol: f64) {
    let got = geo_soup(g);
    let expected = golden_soup(want);
    assert_eq!(
        got.len(),
        expected.len(),
        "{name}: triangle count (triangle-soup) must match exactly (got {} vs golden {})",
        got.len(),
        expected.len()
    );
    let diff = max_diff(&got, &expected);
    assert!(
        diff.max < tol,
        "{name}: triangle-soup worst deviation {} at {} (tolerance {tol})",
        diff.max,
        diff.at
    );
}
