//! Ported from Claude-of-Duty `src/materials/masks.js:1-234` — the whole
//! file (`bakeMasks`, `setMask`).
//!
//! Convex edges get *wear* (paint chipped off, corners rubbed back to the
//! substrate); concave creases get *grime* and extra AO. Baking this per-vertex
//! costs nothing at runtime and is what stops modular kit pieces from reading
//! as clean extruded boxes. This is the **automatic, curvature-driven**
//! counterpart to [`crate::world::masks`]'s **analytic** mask painting — see
//! that module's doc for why both exist: curvature detection cannot know that
//! a chamfer strip is where a hand wears paint off (that is a fact about
//! purpose, not local geometry), but it *can* find a hard-edged kit piece's
//! actual convex/concave seams cheaply, which is what this file does for
//! geometry nobody hand-annotated.
//!
//! Writes a 3-component mask: `r = edge wear, g = grime, b = extra AO` —
//! the same convention [`crate::world::masks`] documents, produced a
//! different way.
//!
//! ## Reusing [`Geo`] instead of `plainXYZ`
//!
//! The source's `plainXYZ` helper (`masks.js:26-37`) exists to dodge a
//! per-vertex `getX/getY/getZ` accessor call when the position/normal
//! attribute is already a plain, non-interleaved `Float32Array` — which, for
//! [`crate::weapons::geometry::Geo`], it always is: `Geo::pos`/`Geo::normal`
//! are already flat `f32` arrays, exactly the fast-path shape `plainXYZ`
//! optimizes *toward*. So this port has no `plainXYZ` — every call site is
//! already on the fast path, unconditionally. This is not a precision
//! divergence either: the source's own comment notes that Three.js position
//! attributes are conventionally `Float32Array` under the hood, so JS's
//! "double-precision `Vector3` arithmetic over float32-widened inputs" is the
//! *same* thing this port does (`geo.pos[i] as f64` for the per-triangle
//! unit-offset maths, `f32` accumulators for the running sums — see below).
//!
//! ## Clustering: a `HashMap` bucket, not a hand-rolled chain
//!
//! The source's position-clustering (`masks.js:83-115`) hand-rolls a chained
//! hash bucket (`Int32Array` "previous cluster with this hash" links) instead
//! of `Map<string, cluster[]>`, because building a `${x},${y},${z}` string key
//! per vertex was **50-65 ms of the whole 110-118 ms bake** at 202k vertices —
//! the source's own measured finding. That optimization was about avoiding a
//! **string allocation** per vertex, not about avoiding a hash-map-shaped
//! lookup; `std::collections::HashMap<i32, Vec<usize>>` here needs no string
//! key (the hash is the same `i32` the source computes) and produces
//! byte-identical clusters, in the same first-seen order, without hand-rolling
//! the chain array — Rust's `HashMap` is not the thing the source's
//! measurement indicted.
//!
//! ## `Math.round` vs `f64::round`
//!
//! `Math.round` rounds ties toward `+Infinity` (`Math.round(-0.5) === -0`);
//! Rust's `f64::round` rounds ties away from zero (`(-0.5_f64).round() ==
//! -1.0`). [`js_round`] reproduces the JS behaviour exactly
//! (`(x + 0.5).floor()`), since the quantization step (`masks.js:92-94`,
//! `Math.round(pos * 8192)`) is exactly the kind of "keep the source's exact
//! behaviour" case rule 5 of the port recipe calls out — a position landing
//! precisely on a `k/16384` boundary is a measure-zero event for authored
//! geometry, but the port should not silently pick a different tie-break if
//! one ever does.

use crate::rng::Rng;
use crate::weapons::geometry::Geo;
use std::collections::HashMap;

/// Curvature-bake tuning knobs — the `opts` destructuring defaults
/// (`masks.js:40-51`).
#[derive(Debug, Clone, Copy)]
pub struct BakeMaskOpts {
    pub wear: f32,
    pub grime: f32,
    pub ao: f32,
    /// Vertices whose convexity exceeds this are treated as a hard edge.
    pub edge_threshold: f32,
    /// Extra grime on downward faces (undersides collect dirt).
    pub down_grime: f32,
    /// Extra wear on upward faces (walked on, rained on).
    pub up_wear: f32,
}

impl Default for BakeMaskOpts {
    fn default() -> Self {
        BakeMaskOpts {
            wear: 1.0,
            grime: 1.0,
            ao: 1.0,
            edge_threshold: 0.06,
            down_grime: 0.35,
            up_wear: 0.15,
        }
    }
}

/// `Math.round(x)`: ties round toward `+Infinity`, unlike [`f64::round`]'s
/// ties-away-from-zero. See the module doc.
use crate::jsmath::round as js_round;

/// `bakeMasks(geometry, opts)` (`masks.js:39-216`). Returns one `[r, g, b]`
/// triple per vertex of `geo`, index-aligned with `geo.pos`/`geo.normal` —
/// the source instead writes a `color` `BufferAttribute` onto `geometry` in
/// place; the caller here attaches the result to whatever geometry
/// representation it is building (there is no shared mutable-geometry-with-
/// attributes type in this crate to write into directly — see
/// [`crate::weapons::geometry::Geo`]'s own doc for the same reasoning).
///
/// `rng` is the source's optional `opts.rng` (`masks.js:50, 204-208`):
/// `None` skips the per-vertex wear/grime jitter entirely, matching the
/// source's `if (rng) { ... }`.
///
/// # Panics
///
/// If `geo.normal.len() != geo.pos.len()`. The source lazily computes vertex
/// normals when the geometry doesn't have them yet (`masks.js:55-59`); `Geo`
/// has no such lazy step (every primitive builder populates `normal`
/// alongside `pos`), so the precondition is asserted rather than silently
/// filled in.
pub fn bake_masks(geo: &Geo, opts: BakeMaskOpts, mut rng: Option<&mut Rng>) -> Vec<[f32; 3]> {
    assert_eq!(
        geo.normal.len(),
        geo.pos.len(),
        "bake_masks: geo must carry one normal per position"
    );

    let count = geo.vert_count();
    if count == 0 {
        return Vec::new();
    }

    // --- position clustering (masks.js:72-115) ---------------------------
    // Hard-edged kit geometry duplicates its vertices per face, so raw vertex
    // adjacency never crosses an edge and every box would come out perfectly
    // clean. Cluster by quantised position first so adjacency (and therefore
    // curvature) spans the seam.
    let mut cluster = vec![0usize; count];
    let mut qx: Vec<i32> = Vec::new();
    let mut qy: Vec<i32> = Vec::new();
    let mut qz: Vec<i32> = Vec::new();
    let mut buckets: HashMap<i32, Vec<usize>> = HashMap::new();
    for i in 0..count {
        let i3 = i * 3;
        let x = js_round(f64::from(geo.pos[i3]) * 8192.0) as i32;
        let y = js_round(f64::from(geo.pos[i3 + 1]) * 8192.0) as i32;
        let z = js_round(f64::from(geo.pos[i3 + 2]) * 8192.0) as i32;
        let h = x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663) ^ z.wrapping_mul(83_492_791);

        let bucket = buckets.entry(h).or_default();
        let found = bucket
            .iter()
            .copied()
            .find(|&c| qx[c] == x && qy[c] == y && qz[c] == z);
        let c = match found {
            Some(c) => c,
            None => {
                let new_c = qx.len();
                qx.push(x);
                qy.push(y);
                qz.push(z);
                bucket.push(new_c);
                new_c
            }
        };
        cluster[i] = c;
    }
    let clusters = qx.len();

    // --- per-cluster accumulation (masks.js:117-171) ----------------------
    // The summed face normal and the summed offset to its neighbours per
    // cluster. dot(avgNormal, avgOffset) < 0 means the surrounding surface
    // folds *away* from the normal => a convex edge; > 0 => a concave crease.
    let mut sum_off = vec![0f32; clusters * 3];
    let mut hits = vec![0f32; clusters];
    let mut cnx = vec![0f32; clusters * 3];
    let mut n_cnt = vec![0f32; clusters];

    let tri_count = geo.tri_count();
    for t in 0..tri_count {
        let t3 = t * 3;
        let (ia, ib, ic) = if geo.index.is_empty() {
            (t3, t3 + 1, t3 + 2)
        } else {
            (
                geo.index[t3] as usize,
                geo.index[t3 + 1] as usize,
                geo.index[t3 + 2] as usize,
            )
        };
        let idx = [ia, ib, ic];
        let tri: [[f64; 3]; 3] = [
            [
                f64::from(geo.pos[ia * 3]),
                f64::from(geo.pos[ia * 3 + 1]),
                f64::from(geo.pos[ia * 3 + 2]),
            ],
            [
                f64::from(geo.pos[ib * 3]),
                f64::from(geo.pos[ib * 3 + 1]),
                f64::from(geo.pos[ib * 3 + 2]),
            ],
            [
                f64::from(geo.pos[ic * 3]),
                f64::from(geo.pos[ic * 3 + 1]),
                f64::from(geo.pos[ic * 3 + 2]),
            ],
        ];

        for k in 0..3usize {
            let i = idx[k];
            let c = cluster[i];
            let p = tri[k];
            let q = tri[(k + 1) % 3];
            let r = tri[(k + 2) % 3];

            // Unit offsets so long thin triangles don't dominate the average.
            let (ux, uy, uz) = (q[0] - p[0], q[1] - p[1], q[2] - p[2]);
            let ulen = (ux * ux + uy * uy + uz * uz).sqrt();
            let us = 1.0 / if ulen == 0.0 { 1.0 } else { ulen };
            let (vx, vy, vz) = (r[0] - p[0], r[1] - p[1], r[2] - p[2]);
            let vlen = (vx * vx + vy * vy + vz * vz).sqrt();
            let vs = 1.0 / if vlen == 0.0 { 1.0 } else { vlen };

            sum_off[c * 3] += (ux * us + vx * vs) as f32;
            sum_off[c * 3 + 1] += (uy * us + vy * vs) as f32;
            sum_off[c * 3 + 2] += (uz * us + vz * vs) as f32;
            hits[c] += 2.0;

            let n3 = i * 3;
            cnx[c * 3] += geo.normal[n3];
            cnx[c * 3 + 1] += geo.normal[n3 + 1];
            cnx[c * 3 + 2] += geo.normal[n3 + 2];
            n_cnt[c] += 1.0;
        }
    }

    // How much the normals at a cluster disagree — 0 on a flat face, high on
    // a crease. This separates "on an edge" from "merely tilted".
    let mut spread = vec![0f32; clusters];
    let mut curve = vec![0f32; clusters];
    for c in 0..clusters {
        let cx = f64::from(cnx[c * 3]);
        let cy = f64::from(cnx[c * 3 + 1]);
        let cz = f64::from(cnx[c * 3 + 2]);
        let nl = (cx * cx + cy * cy + cz * cz).sqrt();
        spread[c] = (1.0 - nl / f64::from(n_cnt[c]).max(1.0)).clamp(0.0, 1.0) as f32;
        if nl > 1e-6 && hits[c] > 0.0 {
            let k = 1.0 / (nl * f64::from(hits[c]));
            curve[c] = ((cx * f64::from(sum_off[c * 3])
                + cy * f64::from(sum_off[c * 3 + 1])
                + cz * f64::from(sum_off[c * 3 + 2]))
                * k) as f32;
        }
    }

    // --- final per-vertex colour (masks.js:188-212) ------------------------
    let mut colors = vec![[0f32; 3]; count];
    for (i, color) in colors.iter_mut().enumerate() {
        let c = cluster[i];
        let mean = f64::from(curve[c]);
        let crease = (f64::from(spread[c]) / 0.18).min(1.0);
        // convex -> mean < 0, concave -> mean > 0.
        let convex = crease * ((-mean - f64::from(opts.edge_threshold)) / 0.22).clamp(0.0, 1.0);
        let concave = crease * ((mean - f64::from(opts.edge_threshold)) / 0.22).clamp(0.0, 1.0);
        let ny = f64::from(geo.normal[i * 3 + 1]);
        let up = ny.max(0.0);
        let down = (-ny).max(0.0);

        let mut w = convex * f64::from(opts.wear) + up * up * f64::from(opts.up_wear) * f64::from(opts.wear);
        let mut g =
            concave * f64::from(opts.grime) + down * f64::from(opts.down_grime) * f64::from(opts.grime);
        let o = concave * f64::from(opts.ao);

        if let Some(r) = rng.as_mut() {
            let j = 0.85 + r.float() * 0.3;
            w *= j;
            g *= 2.0 - j;
        }

        *color = [w.min(1.0) as f32, g.min(1.0) as f32, o.min(1.0) as f32];
    }

    colors
}

/// `setMask(geometry, { wear, grime, ao })` (`masks.js:222-233`): push a
/// uniform triple onto every vertex without recomputing curvature — used by
/// callers that already know their own topology. Rust has no default
/// arguments; the source's defaulted `{ wear = 0, grime = 0, ao = 0 }` is
/// `set_mask(geo, 0.0, 0.0, 0.0)` here.
pub fn set_mask(geo: &Geo, wear: f32, grime: f32, ao: f32) -> Vec<[f32; 3]> {
    vec![[wear, grime, ao]; geo.vert_count()]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three flat, non-indexed triangles meeting at a shared corner —
    /// three faces of a cube's outward corner, each duplicating its own
    /// vertices (flat shading): the exact "hard-edged kit geometry" shape
    /// the module doc calls out. `outward` picks the +X/+Y/+Z (convex) or
    /// -X/-Y/-Z (concave, an interior corner) normals against the same
    /// positions.
    fn corner_geo(outward: bool) -> Geo {
        let corner = [1.0f32, 1.0, 1.0];
        #[rustfmt::skip]
        let pos: Vec<f32> = vec![
            corner[0], corner[1], corner[2], 1.0, 0.0, 1.0, 1.0, 1.0, 0.0, // +X face
            corner[0], corner[1], corner[2], 1.0, 1.0, 0.0, 0.0, 1.0, 1.0, // +Y face
            corner[0], corner[1], corner[2], 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, // +Z face
        ];
        #[rustfmt::skip]
        let mut normal: Vec<f32> = vec![
            1.0, 0.0, 0.0,  1.0, 0.0, 0.0,  1.0, 0.0, 0.0,
            0.0, 1.0, 0.0,  0.0, 1.0, 0.0,  0.0, 1.0, 0.0,
            0.0, 0.0, 1.0,  0.0, 0.0, 1.0,  0.0, 0.0, 1.0,
        ];
        if !outward {
            normal.iter_mut().for_each(|n| *n = -*n);
        }
        Geo {
            pos,
            normal,
            uv: Vec::new(),
            index: Vec::new(),
        }
    }

    fn default_opts() -> BakeMaskOpts {
        BakeMaskOpts::default()
    }

    /// Golden-captured from the real `bakeMasks` in
    /// `C:/dev/Claude-of-Duty/src/materials/masks.js`, run against a
    /// `THREE.BufferGeometry` built from exactly `corner_geo(true)`'s
    /// position/normal arrays (see
    /// `docs/work-manifests/shmup-port/notes/materials-bake-and-masks.md`
    /// for the capture script). Every value here is built only from `+ - *
    /// / sqrt / min / max / clamp` — no `sin`/`cos`/`pow` — so it is pinned
    /// exactly, matching `tests/materials_noise_port.rs`'s convention of
    /// exact equality for anything without a transcendental in its path
    /// (`sqrt` alone does not get the `1e-12` tolerance there either, e.g.
    /// `owWorley`'s `f1`/`f2` — the tolerance is for `sin`/`cos`/`pow`
    /// specifically, and this bake never calls any of those).
    #[test]
    fn convex_corner_matches_the_javascript_capture() {
        let geo = corner_geo(true);
        let colors = bake_masks(&geo, default_opts(), None);
        let expected: [[f32; 3]; 9] = [
            [1.0, 0.0, 0.0],
            [0.8636364, 0.0, 0.0],
            [0.8636364, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.8636364, 0.0, 0.0],
            [0.8636364, 0.0, 0.0],
        ];
        for (i, (got, want)) in colors.iter().zip(expected).enumerate() {
            for ch in 0..3 {
                assert!(
                    (got[ch] - want[ch]).abs() < 1e-6,
                    "i={i} got {got:?}, want {want:?}"
                );
            }
        }
    }

    /// Same geometry, normals negated — an interior/concave corner (e.g.
    /// where three walls of a room meet on the inside). `bakeMasks` derives
    /// convex/concave purely from the *stored* normal array against the
    /// triangle positions, so this is a legitimate second real fixture, not
    /// a reuse of the first with a sign flipped after the fact.
    #[test]
    fn concave_corner_matches_the_javascript_capture() {
        let geo = corner_geo(false);
        let colors = bake_masks(&geo, default_opts(), None);
        let expected: [[f32; 3]; 9] = [
            [0.0, 1.0, 1.0],
            [0.0, 0.8636364, 0.8636364],
            [0.0, 0.8636364, 0.8636364],
            [0.0, 1.0, 1.0],
            [0.0, 1.0, 0.8636364],
            [0.0, 1.0, 0.8636364],
            [0.0, 1.0, 1.0],
            [0.0, 0.8636364, 0.8636364],
            [0.0, 0.8636364, 0.8636364],
        ];
        for (got, want) in colors.iter().zip(expected) {
            for ch in 0..3 {
                assert!(
                    (got[ch] - want[ch]).abs() < 1e-6,
                    "got {got:?}, want {want:?}"
                );
            }
        }
    }

    /// A single flat, unshared triangle (its own two-triangle-per-vertex
    /// "self" adjacency only): curve is exactly 0 (the triangle's own
    /// normal is perpendicular to both its own edge-offset vectors by
    /// definition, so `dot(normal, sumOffset) == 0`), and spread is exactly
    /// 0 (only one normal contributes). The only nonzero channel is
    /// `up * up * upWear * wear` on the one upward-facing normal.
    #[test]
    fn flat_lone_triangle_has_zero_curve_and_only_up_wear() {
        let geo = Geo {
            pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0],
            normal: vec![0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0],
            uv: Vec::new(),
            index: Vec::new(),
        };
        let colors = bake_masks(&geo, default_opts(), None);
        for c in colors {
            // up=1, upWear=0.15, wear=1.0 -> 1*1*0.15*1 = 0.15.
            assert!((c[0] - 0.15).abs() < 1e-6, "got {c:?}");
            assert_eq!(c[1], 0.0);
            assert_eq!(c[2], 0.0);
        }
    }

    #[test]
    fn empty_geo_bakes_no_colors() {
        let geo = Geo::default();
        assert_eq!(bake_masks(&geo, default_opts(), None), Vec::<[f32; 3]>::new());
    }

    #[test]
    #[should_panic(expected = "one normal per position")]
    fn mismatched_normal_count_panics() {
        let geo = Geo {
            pos: vec![0.0, 0.0, 0.0],
            normal: Vec::new(),
            uv: Vec::new(),
            index: Vec::new(),
        };
        bake_masks(&geo, default_opts(), None);
    }

    /// The `rng` jitter branch (`masks.js:204-208`): `Rng::new(1234)`,
    /// applied per-*vertex* (not per-cluster) inside the final loop, matching
    /// the source's `for (i < count)` — golden-captured from the real
    /// `bakeMasks` driven by a JS transcription of `apps/shmup/src/
    /// rng.rs`'s exact xoshiro128**/SplitMix32 sequence (see the notes file).
    #[test]
    fn rng_jitter_matches_the_javascript_capture() {
        let geo = corner_geo(true);
        let mut rng = Rng::new(1234);
        let colors = bake_masks(&geo, default_opts(), Some(&mut rng));
        let expected_w: [f32; 9] = [
            0.9541765, 0.9248297, 0.7594154, 1.0, 0.8664029, 1.0, 1.0, 0.7427554, 0.9096895,
        ];
        for (got, want) in colors.iter().zip(expected_w) {
            assert!((got[0] - want).abs() < 1e-6, "got {}, want {want}", got[0]);
            assert_eq!(got[1], 0.0);
            assert_eq!(got[2], 0.0);
        }
    }

    #[test]
    fn set_mask_writes_the_same_triple_into_every_vertex() {
        let geo = corner_geo(true);
        let colors = set_mask(&geo, 0.4, 0.6, 0.2);
        assert_eq!(colors.len(), geo.vert_count());
        for c in colors {
            assert_eq!(c, [0.4, 0.6, 0.2]);
        }

        // The source's defaulted `setMask(geo, {})` call, named explicitly.
        let zeroed = set_mask(&geo, 0.0, 0.0, 0.0);
        assert!(zeroed.iter().all(|c| *c == [0.0, 0.0, 0.0]));
    }

    #[test]
    fn set_mask_on_empty_geo_is_empty() {
        let geo = Geo::default();
        assert_eq!(set_mask(&geo, 0.1, 0.2, 0.3), Vec::<[f32; 3]>::new());
    }
}
