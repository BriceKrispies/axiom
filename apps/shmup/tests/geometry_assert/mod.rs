//! Shared triangle-soup comparison helper for the weapon-geometry golden
//! test suites (`weapons_geometry_primitives_port.rs`,
//! `weapons_parts_hardware_port.rs`, `weapons_parts_magazine_port.rs`,
//! `weapons_parts_controls_port.rs`, `weapons_parts_optics_port.rs`,
//! `weapons_models_port.rs`, `world_port.rs`).
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
//! (almost always) geometrically identical.
//!
//! **What a triangle-soup comparison does instead.** Expand the index
//! buffer into actual triangles — three whole corners per triangle,
//! duplicating a shared vertex across every triangle that touches it, i.e.
//! exactly undoing what welding collapsed away. Canonicalize each triangle
//! by rotating its three corners so the smallest (by position) sorts first —
//! never a free sort of the three corners, which would also discard winding
//! and hide a flipped-normal bug. Then **pair each `got` triangle with its
//! `want` correspondent by centroid**, not by a global sort, and compare the
//! paired corners (trying every cyclic rotation of the pairing, since two
//! independent welds are not guaranteed to canonicalize to the same starting
//! corner) within a tolerance.
//!
//! **Why centroid-keyed, not a coarse per-field sort grid.** An earlier
//! version of this helper sorted the whole triangle list on a single global
//! key quantized to a `5e-3` (5 mm) grid, on the theory that this was
//! coarser than any real residual and would only ever fall through to a real
//! tie-break. That reasoning failed on real meshes: a knurl band, a
//! Picatinny rail's ~9 mm tooth pitch, an M-LOK slot pocket, or pistol-grip
//! stipple (as close as 2.6 mm) all repeat a nearly-identical feature at a
//! pitch *inside* that grid cell. Every repeat of that feature quantized to
//! the *same* sort key, so the sort fell back to raw-float ordering across
//! physically different triangles and paired one tooth against its
//! neighbour — reporting the gap *between* them as geometric error. Proven
//! directly (`7fb1fde5`): whole weapon assemblies appeared to diverge by up
//! to 0.071 m under the grid-sort comparator, while a centroid-keyed
//! re-pairing of the exact same triangles found every one matching to
//! `1e-9`-`1e-7`. The comparator was wrong, not the geometry.
//!
//! A triangle's own centroid, quantized to a grid far finer than any
//! repeated feature's pitch (`1e-5` m, i.e. ~0.01 mm — see [`CENTROID_GRID`]),
//! does not have this problem: two triangles collide in the same centroid
//! bucket only if they are, in fact, the same triangle (up to weld/order
//! noise), never merely nearby siblings of a repeated feature.
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
//! decision or a mispaired sibling) is a genuine geometric divergence, not a
//! bookkeeping artifact — see each call site for whether that happened and
//! what was measured.

use std::collections::HashMap;

use serde_json::Value;

use axiom_shmup::weapons::geometry::Geo;

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

/// Index (exclusive) of the last position/normal field in [`corner_key`] /
/// [`CORNER_FIELDS`] — everything before this index is position+normal,
/// everything from here on is uv. Used to score uv deviation separately from
/// position/normal (see [`assert_triangle_soup_matches_uv`]'s doc for why).
const UV_FIELD_START: usize = 6;

/// Centroid quantization grid: far finer than any repeated feature's pitch
/// this suite has ever measured (the tightest is 2.6 mm pistol-grip
/// stipple), so two triangles collide in the same bucket only if they are
/// really the same triangle up to weld/rounding noise — never merely nearby
/// siblings of a repeated feature. See the module doc for the direct proof
/// that the coarser (5 mm) grid this replaced did not have that property.
const CENTROID_GRID: f64 = 1e-5;

fn centroid(tri: &Triangle) -> [f64; 3] {
    let sum = |i: usize| tri[0].pos[i] + tri[1].pos[i] + tri[2].pos[i];
    [sum(0) / 3.0, sum(1) / 3.0, sum(2) / 3.0]
}

fn centroid_key(tri: &Triangle) -> (i64, i64, i64) {
    let c = centroid(tri);
    let q = |x: f64| (x / CENTROID_GRID).round() as i64;
    (q(c[0]), q(c[1]), q(c[2]))
}

fn centroid_dist(a: &Triangle, b: &Triangle) -> f64 {
    let (ca, cb) = (centroid(a), centroid(b));
    ca.iter().zip(cb.iter()).map(|(x, y)| (x - y).powi(2)).sum::<f64>().sqrt()
}

/// Rotates a triangle's three corners so corner `k` becomes corner `0`,
/// preserving winding order — never a free permutation of all three, which
/// would collapse a front-facing and a back-facing (flipped-winding)
/// triangle onto the same form and hide exactly the bug this comparison
/// exists to catch.
fn rotate(tri: &Triangle, k: usize) -> Triangle {
    [tri[k % 3], tri[(k + 1) % 3], tri[(k + 2) % 3]]
}

/// Canonicalizes a triangle by rotating so its position-smallest corner
/// sorts first. This gives every triangle a single, deterministic starting
/// corner for identification/debugging, but — because two independently
/// welded meshes are not guaranteed to break a near-tie in position the same
/// way — it is *not* on its own sufficient to align two corresponding
/// triangles' corners; [`corner_max_diff`] tries every rotation for that.
fn canonicalize(tri: Triangle) -> Triangle {
    let start = (0..3usize)
        .min_by(|&a, &b| {
            tri[a]
                .pos
                .iter()
                .zip(tri[b].pos.iter())
                .map(|(x, y)| x.total_cmp(y))
                .find(|o| !o.is_eq())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0);
    rotate(&tri, start)
}

/// The worst deviation found in one field group (position+normal, or uv),
/// plus where it was found — so a failure reports the exact measured
/// deviation, not just "not equal".
#[derive(Clone)]
struct FieldDiff {
    max: f64,
    at: String,
}

impl FieldDiff {
    fn zero() -> Self {
        FieldDiff { max: 0.0, at: "no corners compared".to_string() }
    }
}

/// Worst per-field diff between two triangles at a FIXED corner alignment,
/// split into (position+normal, uv) — see [`assert_triangle_soup_matches_uv`]
/// for why `uv` is scored separately from position/normal.
fn field_diff_aligned(ti: usize, a: &Triangle, b: &Triangle) -> (FieldDiff, FieldDiff) {
    let mut pos_normal = FieldDiff::zero();
    let mut uv = FieldDiff::zero();
    for (ci, (ca, cb)) in a.iter().zip(b.iter()).enumerate() {
        let (ka, kb) = (corner_key(ca), corner_key(cb));
        for (fi, (x, y)) in ka.iter().zip(kb.iter()).enumerate() {
            let d = (x - y).abs();
            let slot = if fi < UV_FIELD_START { &mut pos_normal } else { &mut uv };
            if d > slot.max {
                *slot = FieldDiff { max: d, at: format!("triangle[{ti}].corner[{ci}].{}", CORNER_FIELDS[fi]) };
            }
        }
    }
    (pos_normal, uv)
}

/// Worst diff between two triangles, trying every cyclic rotation of `a` and
/// keeping the alignment with the smallest position+normal diff.
///
/// [`canonicalize`] alone is not enough to guarantee `a[i]` and `b[i]` are
/// the SAME physical corner: it starts a triangle at its position-smallest
/// corner, and two corners that are nearly tied on position (a thin sliver
/// triangle, common wherever an extrude/lathe/weld seam runs) can pick a
/// DIFFERENT starting corner on the Rust side than the independently-welded
/// JS side did, even though the two triangles are the same three points in
/// the same winding order. Trying all three rotations (never a free
/// permutation, which would hide a real flipped-winding bug) finds the
/// correct alignment regardless of which corner either side's `canonicalize`
/// happened to start from.
fn corner_max_diff(ti: usize, a: &Triangle, b: &Triangle) -> (FieldDiff, FieldDiff) {
    let mut best: Option<(FieldDiff, FieldDiff)> = None;
    for k in 0..3 {
        let candidate = field_diff_aligned(ti, &rotate(a, k), b);
        let better = match &best {
            Some((bpn, _)) => candidate.0.max < bpn.max,
            None => true,
        };
        if better {
            best = Some(candidate);
        }
    }
    best.unwrap_or_else(|| (FieldDiff::zero(), FieldDiff::zero()))
}

fn triangles_from_slices(pos: &[f32], normal: &[f32], uv: &[f32], index: &[u32]) -> Vec<Triangle> {
    let pos: Vec<f64> = pos.iter().map(|&x| f64::from(x)).collect();
    let normal: Vec<f64> = normal.iter().map(|&x| f64::from(x)).collect();
    let uv: Vec<f64> = uv.iter().map(|&x| f64::from(x)).collect();
    triangles(&pos, &normal, &uv, index)
}

fn golden_triangles(want: &Value) -> Vec<Triangle> {
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
    triangles(&pos, &normal, &uv, &index)
}

/// Triangle-soup comparison, raw-slice entry point: takes plain
/// `pos`/`normal`/`uv`/`index` buffers rather than a `weapons::geometry::Geo`,
/// so it also serves `world::geo::WorldGeo` (`world_port.rs`) and any other
/// vertex-buffer shape with the same layout.
///
/// Triangle count is asserted exactly. Every `got` triangle is then paired
/// with its centroid-matching `want` triangle (see the module doc) and every
/// corner's position/normal/uv is compared within `tol`/`uv_tol`
/// respectively, trying every cyclic corner rotation to find the true
/// correspondence. On failure the panic reports the single worst-deviating
/// field and its magnitude — the number a caller needs to tell a real
/// geometric divergence from noise.
///
/// **Why uv gets its own tolerance.** `extrude()`'s `WorldUVGenerator`-
/// equivalent picks its projection axis via a discrete `<` comparison
/// between two side-length magnitudes: on a contour whose sides are nearly
/// equal, a sub-tolerance POSITION difference can flip that axis choice and
/// produce a `uv` difference far larger than any float-noise budget on an
/// otherwise perfectly correct triangle (correct position, correct normal,
/// correct winding). That is a real, already-diagnosed source quirk
/// (`weapons_parts_magazine_port.rs`, `weapons_parts_controls_port.rs`), not
/// something a position/normal tolerance should have to absorb. Most
/// callers pass the same value for both via [`assert_triangle_soup_matches`];
/// callers that have measured a real axis-tie residual pass a wider
/// `uv_tol` explicitly and record what they measured.
pub fn assert_triangle_soup_matches_raw_uv(name: &str, pos: &[f32], normal: &[f32], uv: &[f32], index: &[u32], want: &Value, tol: f64, uv_tol: f64) {
    let got: Vec<Triangle> = triangles_from_slices(pos, normal, uv, index).into_iter().map(canonicalize).collect();
    let expected: Vec<Triangle> = golden_triangles(want).into_iter().map(canonicalize).collect();
    assert_eq!(
        got.len(),
        expected.len(),
        "{name}: triangle count (triangle-soup) must match exactly (got {} vs golden {})",
        got.len(),
        expected.len()
    );

    let mut want_by_centroid: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for (i, t) in expected.iter().enumerate() {
        want_by_centroid.entry(centroid_key(t)).or_default().push(i);
    }

    let mut worst_pos_normal = FieldDiff::zero();
    let mut worst_uv = FieldDiff::zero();
    let mut fallback_count = 0usize;

    for (gi, gt) in got.iter().enumerate() {
        let exact = want_by_centroid.get(&centroid_key(gt)).filter(|idxs| !idxs.is_empty());
        let (pn, uv_diff) = match exact {
            Some(idxs) => idxs
                .iter()
                .map(|&wi| corner_max_diff(gi, gt, &expected[wi]))
                .min_by(|a, b| a.0.max.total_cmp(&b.0.max))
                .unwrap_or_else(|| (FieldDiff::zero(), FieldDiff::zero())),
            None => {
                fallback_count += 1;
                // No exact-centroid candidate — quantization boundary noise
                // (see the module doc / [`CENTROID_GRID`]). Pair to the
                // nearest triangle by raw centroid distance instead, so a
                // genuinely missing/wrong triangle still fails loudly rather
                // than silently finding no candidate to compare against.
                let nearest = expected
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| centroid_dist(gt, a).total_cmp(&centroid_dist(gt, b)))
                    .map(|(i, _)| i)
                    .unwrap_or_else(|| panic!("{name}: triangle[{gi}] has no candidates at all (empty golden?)"));
                corner_max_diff(gi, gt, &expected[nearest])
            }
        };
        if pn.max > worst_pos_normal.max {
            worst_pos_normal = pn;
        }
        if uv_diff.max > worst_uv.max {
            worst_uv = uv_diff;
        }
    }

    // A triangle that needs the nearest-centroid fallback is expected to be
    // rare (quantization-boundary f32 noise, not a mispairing) — bound how
    // many are allowed so a real regression that produced many more
    // fallbacks (e.g. a shape that moved enough to leave its whole
    // neighbourhood) still fails this test outright, even before the
    // per-corner tolerance check below.
    let budget = (((got.len() as f64) * 0.01).ceil() as usize).max(16);
    assert!(
        fallback_count <= budget,
        "{name}: {fallback_count} of {} triangles had no exact centroid match (budget {budget}) — geometry may have shifted, not just re-welded",
        got.len()
    );

    assert!(
        worst_pos_normal.max < tol,
        "{name}: triangle-soup worst pos/normal deviation {} at {} (tolerance {tol})",
        worst_pos_normal.max,
        worst_pos_normal.at
    );
    assert!(
        worst_uv.max < uv_tol,
        "{name}: triangle-soup worst uv deviation {} at {} (tolerance {uv_tol})",
        worst_uv.max,
        worst_uv.at
    );
}

/// [`assert_triangle_soup_matches_raw_uv`] against a `weapons::geometry::Geo`,
/// with position/normal and uv held to separate tolerances.
pub fn assert_triangle_soup_matches_uv(name: &str, g: &Geo, want: &Value, tol: f64, uv_tol: f64) {
    assert_triangle_soup_matches_raw_uv(name, &g.pos, &g.normal, &g.uv, &g.index, want, tol, uv_tol);
}

/// [`assert_triangle_soup_matches_raw_uv`] against a `weapons::geometry::Geo`,
/// with a single tolerance applied to every field (position, normal, and
/// uv alike). This is the right choice whenever no uv axis-tie residual has
/// been measured for the call site — i.e. most callers; see
/// [`assert_triangle_soup_matches_uv`]'s doc for when to reach for the split
/// form instead.
pub fn assert_triangle_soup_matches(name: &str, g: &Geo, want: &Value, tol: f64) {
    assert_triangle_soup_matches_uv(name, g, want, tol, tol);
}

/// [`assert_triangle_soup_matches_raw_uv`] with a single tolerance applied to
/// every field — the raw-slice analogue of [`assert_triangle_soup_matches`],
/// for callers whose geometry is not a `weapons::geometry::Geo` (e.g.
/// `world::geo::WorldGeo`, which shares the same `pos`/`normal`/`uv`/`index`
/// layout but is a distinct type owned by a different module).
pub fn assert_triangle_soup_matches_raw(name: &str, pos: &[f32], normal: &[f32], uv: &[f32], index: &[u32], want: &Value, tol: f64) {
    assert_triangle_soup_matches_raw_uv(name, pos, normal, uv, index, want, tol, tol);
}
