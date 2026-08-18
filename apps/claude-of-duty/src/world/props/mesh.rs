//! Ported from Claude-of-Duty `src/world/props.js:44-56` (`autoEdgeWear`),
//! `src/world/util.js` (`sackGeometry`: `util.js:1032-1091`; `warpGeometry`:
//! `util.js:1107-1118`), and `src/world/props.js:660-686` (`dustSkirt`).
//!
//! `pockGeometry` (`kit.js:982-1044`) is **not** duplicated here — it landed
//! at `crate::world::kit::pock_geometry` as part of the concurrent `kit.js`
//! port, so `props::registry` calls that directly.
//!
//! `sackGeometry` reuses `crate::weapons::geometry::primitives::sphere_geometry`
//! — the same faithful `THREE.SphereGeometry` port `dome()` already builds on
//! — rather than a third copy of that primitive; see that function's doc for
//! why it was widened from private to `pub(crate)`.
//! `dustSkirt` reuses `crate::world::kit::cylinder_geometry` (the degenerate
//! `radiusTop=radiusBottom=1, height=0` disc the source itself builds it
//! from) the same way.

use crate::rng::Rng;
use crate::weapons::geometry::primitives::sphere_geometry;
use crate::world::geo::WorldGeo;
use crate::world::kit::cylinder_geometry;
use crate::world::noise::fbm3;

/// `Math.sign(x)` is three-valued (`sign(0) === 0`); `f64::signum`/`f32::signum`
/// are two-valued (`signum(0.0) == 1.0`). Every call site in this port that
/// leans on a zero sign contributing nothing needs this instead — see the
/// port recipe's "Language traps" list, hit here a third time (`sack_geometry`'s
/// `Math.sign(uy)`, `tyre`'s `Math.sign(y)`).
pub(crate) fn sign3(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// The min/max of one position axis (0=x, 1=y, 2=z) — `computeBoundingBox`'s
/// per-axis half, needed standalone by both [`auto_edge_wear`] and
/// `props::cover::sandbag` (which reads `g.boundingBox.min.y` after
/// `sackGeometry` builds it).
pub(crate) fn bounds_axis(pos: &[f32], axis: usize) -> (f32, f32) {
    let mut lo = f32::INFINITY;
    let mut hi = f32::NEG_INFINITY;
    for &v in pos.iter().skip(axis).step_by(3) {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    (lo, hi)
}

/// `autoEdgeWear(geo, margin = 0.02, amount = 1)` (`props.js:44-56`): a
/// generic convex-edge detector — vertices near two or more bounding faces
/// get `out[0] = max(out[0], amount)`. Analytic edge wear for shapes that
/// don't build their own (a `THREE.CylinderGeometry`, an extrusion, a
/// `polyPrism`), unlike `chamferBox`, which bakes its own wear per bevel
/// strip and never calls this.
pub(crate) fn auto_edge_wear(geo: &mut WorldGeo, margin: f32, amount: f32) {
    let (x0, x1) = bounds_axis(&geo.pos, 0);
    let (y0, y1) = bounds_axis(&geo.pos, 1);
    let (z0, z1) = bounds_axis(&geo.pos, 2);
    let (sx, sy, sz) = (x1 - x0, y1 - y0, z1 - z0);
    geo.paint_masks(|x, y, z, _nx, _ny, _nz, out, _i| {
        let mut near = 0u32;
        if sx > margin * 3.0 && (x - x0 < margin || x1 - x < margin) {
            near += 1;
        }
        if sy > margin * 3.0 && (y - y0 < margin || y1 - y < margin) {
            near += 1;
        }
        if sz > margin * 3.0 && (z - z0 < margin || z1 - z < margin) {
            near += 1;
        }
        if near >= 2 {
            out[0] = out[0].max(amount);
        }
    });
}

/// `warpGeometry(geo, amp = 0.02, freq = 1.1, seed = 0)` (`util.js:1107-1118`):
/// bend a geometry's vertices around Y with two independent `fbm3` samples,
/// so long thin objects (barrels, planks) are never perfectly straight.
/// Computed in `f64` (a JS number), narrowed to `f32` only on write-back.
pub(crate) fn warp_geometry(geo: &mut WorldGeo, amp: f32, freq: f32, seed: f32) {
    let (amp, freq, seed) = (f64::from(amp), f64::from(freq), f64::from(seed));
    for p in geo.pos.chunks_exact_mut(3) {
        let (x, y, z) = (f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
        let t = fbm3(x * freq + seed, y * freq + seed * 1.7, z * freq + seed * 2.3, 2) - 0.5;
        let t2 = fbm3(z * freq + seed * 3.1, y * freq, x * freq, 2) - 0.5;
        p[0] = (x + t * amp) as f32;
        p[1] = (y + t2 * amp * 0.5) as f32;
        p[2] = (z + t2 * amp) as f32;
    }
    geo.compute_vertex_normals();
}

/// `sackGeometry(rng, w, h, d, opts)`'s `{variant, box, lump}` (`util.js:1033`).
/// Defaults match the source: `variant=0`, `box=3.1`, `lump=1`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SackOpts {
    pub variant: u32,
    pub box_p: f64,
    pub lump: f64,
}

impl Default for SackOpts {
    fn default() -> Self {
        SackOpts { variant: 0, box_p: 3.1, lump: 1.0 }
    }
}

/// `sackGeometry(rng, w = 0.5, h = 0.17, d = 0.3, opts = {})` (`util.js:1032-1091`):
/// a filled-bag silhouette carved out of a UV sphere by projecting each
/// vertex's unit direction onto an `Lp` ball (`p = opts.box`) — the boxy,
/// gathered-sack shape a smooth sphere or a plain squashed ellipsoid can't
/// produce — then folding in tied/flattened ends and one of three variant
/// slumps. Built on `new THREE.SphereGeometry(0.5, 20, 12)`
/// (`sphere_geometry(0.5, 20, 12, 0, TAU, 0, PI)`, its full-sphere defaults).
pub(crate) fn sack_geometry(rng: &mut Rng, w: f64, h: f64, d: f64, opts: SackOpts) -> WorldGeo {
    let raw = sphere_geometry(0.5, 20, 12, 0.0, std::f64::consts::TAU, 0.0, std::f64::consts::PI);
    let mut g = WorldGeo {
        pos: raw.pos,
        normal: raw.normal,
        uv: raw.uv,
        color: Vec::new(),
        index: raw.index,
    };

    let seed = rng.float() * 50.0;
    let p = opts.box_p;
    let lump = opts.lump;

    for i in 0..g.vert_count() {
        let vx = f64::from(g.pos[i * 3]);
        let vy = f64::from(g.pos[i * 3 + 1]);
        let vz = f64::from(g.pos[i * 3 + 2]);

        let mut ux = vx * 2.0;
        let mut uy = vy * 2.0;
        let mut uz = vz * 2.0;
        let q = ux.abs().powf(p) + uy.abs().powf(p) + uz.abs().powf(p);
        let f = if q > 1e-6 { 1.0 / q.powf(1.0 / p) } else { 1.0 };
        ux *= f;
        uy *= f;
        uz *= f;

        // The top of a bag under load is flatter than the bottom.
        let flat = if uy > 0.0 { 1.0 - uy * uy * 0.2 } else { 1.0 };
        let n = fbm3(ux * 3.4 + seed, uy * 3.4 + seed, uz * 3.4 + seed, 3) - 0.5;
        let n2 = fbm3(ux * 9.0 + seed * 2.0, uy * 8.0 + seed, uz * 9.0 + seed * 3.0, 2) - 0.5;

        let mut x = ux * w * 0.5 * (1.0 + n * 0.09 * lump);
        let mut z = uz * d * 0.5 * flat * (1.0 + n * 0.26 * lump + n2 * 0.11 * lump);
        let mut y = uy * h * 0.5 * (1.0 + n * 0.24 * lump + n2 * 0.1 * lump);

        let t = x / (w * 0.5); // -1..1 along the bag
        // Tied, folded ends: gathered and flattened, not pinched to a point.
        let neck = (t.abs() - 0.7).max(0.0) / 0.3;
        z *= 1.0 - neck * neck * 0.3;
        y *= 1.0 - neck * neck * 0.55;
        // The sewn end seam stands out as a small flat lip.
        if neck > 0.55 {
            y += sign3(uy) * h * 0.02 * (neck - 0.55) * 2.0;
        }
        // The sewn seam runs the length of the crown on every bag.
        if uy > 0.15 {
            y += h * 0.05 * (-((z / (d * 0.42)).powi(2)) * 6.0).exp() * (1.0 - neck * 0.8);
        }

        match opts.variant {
            0 => {
                z *= 1.0 + 0.06 * (t * 2.6).cos();
            }
            1 => {
                // Slumped: fat at -x, sagging waist, dished top.
                x += w * 0.04 * t;
                let fatter = 1.0 + 0.13 * (0.5 - t);
                z *= fatter;
                y *= fatter * (1.0 - 0.16 * (-((t / 0.32).powi(2))).exp());
                if uy > 0.3 {
                    y -= h * 0.05 * (-((t / 0.45).powi(2))).exp();
                }
            }
            _ => {
                // Half-empty: crease across the waist, flat folded end at +x.
                let crease = (-(((t - 0.1) / 0.16).powi(2))).exp();
                z *= 1.0 - crease * 0.2;
                y *= 1.0 - crease * 0.26;
                if t > 0.5 {
                    y *= 1.0 - (t - 0.5) * 0.5;
                    z *= 1.0 + (t - 0.5) * 0.22;
                }
            }
        }

        g.pos[i * 3] = x as f32;
        g.pos[i * 3 + 1] = y as f32;
        g.pos[i * 3 + 2] = z as f32;
    }

    g.compute_vertex_normals();
    g
}

/// `dustSkirt(rng)` (`props.js:660-686`): the swept fillet of dust and grit
/// that piles against anything left standing on a street. Unit radius
/// (`Assembler::put` scales it per-instance), ~2.5 cm proud at the object
/// and feathering to nothing at the rim, with a jagged outline so it never
/// reads as a disc. Built from the degenerate
/// `radiusTop = radiusBottom = 1, height = 0` cylinder the source itself
/// uses to get an indexed disc topology "for free" (a real centre-to-rim
/// vertex fan only exists in the two end caps; the `RAD` torso rows are
/// coincident duplicates of the same ring — a source quirk carried through
/// unchanged so the vertex/triangle counts match exactly).
///
/// The source's `rng` parameter is never read (grep-verified); kept for
/// call-site parity with `registerProps`.
pub(crate) fn dust_skirt(_rng: &mut Rng) -> WorldGeo {
    const RAD: u32 = 4;
    const SEG: u32 = 26;
    let raw = cylinder_geometry(1.0, 1.0, 0.0, SEG, RAD, false);
    let n = raw.vert_count();
    let mut pos = raw.pos;
    let mut color = vec![0.0f32; n * 3];
    for i in 0..n {
        let x = f64::from(pos[i * 3]);
        let z = f64::from(pos[i * 3 + 2]);
        let d = x.hypot(z).min(1.0);
        let a = z.atan2(x);
        // Ragged outline: the rim wanders +/-22%.
        let wob = 0.86 + 0.28 * fbm3(a.cos() * 2.2, a.sin() * 2.2, 3.1, 3);
        let dd = d * wob;
        pos[i * 3] = (x * wob) as f32;
        pos[i * 3 + 2] = (z * wob) as f32;
        // (1-d)^2 profile: steep against the object, flat at the edge.
        let t = (1.0 - dd).max(0.0);
        pos[i * 3 + 1] = (t * t * 0.021 + (fbm3(x * 6.0, z * 6.0, 9.4, 3) - 0.5) * 0.004 * (1.0 - dd)) as f32;
        color[i * 3] = 0.05;
        color[i * 3 + 1] = (0.35 + 0.6 * t) as f32;
        color[i * 3 + 2] = (0.3 + 0.55 * t) as f32;
    }
    let mut g = WorldGeo { pos, normal: raw.normal, uv: raw.uv, color, index: raw.index };
    g.compute_vertex_normals();
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign3_is_three_valued_unlike_signum() {
        assert_eq!(sign3(0.0), 0.0);
        assert_eq!(sign3(2.5), 1.0);
        assert_eq!(sign3(-2.5), -1.0);
    }

    #[test]
    fn bounds_axis_finds_min_and_max_of_one_column() {
        let pos = [0.0, -1.0, 5.0, 2.0, 3.0, -5.0];
        assert_eq!(bounds_axis(&pos, 0), (0.0, 2.0));
        assert_eq!(bounds_axis(&pos, 1), (-1.0, 3.0));
        assert_eq!(bounds_axis(&pos, 2), (-5.0, 5.0));
    }

    #[test]
    fn auto_edge_wear_marks_only_corners_near_two_or_more_faces() {
        // A real 1x1x1 box (all 8 corners present, so the bounding box is
        // genuinely 1x1x1), plus one face-centre vertex (near only the -Z
        // face) and one edge-midpoint vertex (near two faces, +X and -Y).
        let mut pos = Vec::new();
        for &sx in &[-0.5f32, 0.5] {
            for &sy in &[-0.5f32, 0.5] {
                for &sz in &[-0.5f32, 0.5] {
                    pos.extend_from_slice(&[sx, sy, sz]);
                }
            }
        }
        let face_centre_index = pos.len() / 3;
        pos.extend_from_slice(&[0.0, 0.0, -0.5]); // -Z face centre: near 1 face.
        let edge_index = pos.len() / 3;
        pos.extend_from_slice(&[0.5, -0.5, 0.0]); // +X/-Y edge: near 2 faces.

        let n = pos.len() / 3;
        let mut g = WorldGeo {
            pos,
            normal: vec![0.0; n * 3],
            uv: Vec::new(),
            color: Vec::new(),
            index: Vec::new(),
        };
        auto_edge_wear(&mut g, 0.02, 1.0);
        assert_eq!(g.color[0], 1.0, "a genuine corner should be marked");
        assert_eq!(g.color[face_centre_index * 3], 0.0, "a face centre should not be marked");
        assert_eq!(g.color[edge_index * 3], 1.0, "an edge midpoint (near 2 faces) should be marked");
    }

    #[test]
    fn warp_geometry_perturbs_positions_and_recomputes_normals() {
        let mut g = WorldGeo {
            pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normal: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            uv: Vec::new(),
            color: Vec::new(),
            index: Vec::new(),
        };
        let before = g.pos.clone();
        warp_geometry(&mut g, 0.05, 1.1, 3.0);
        assert_ne!(g.pos, before);
        assert_eq!(g.normal.len(), 9);
    }

    #[test]
    fn sack_geometry_produces_a_bag_sized_within_its_own_half_extents() {
        let mut rng = Rng::new(1);
        let g = sack_geometry(&mut rng, 0.5, 0.17, 0.3, SackOpts { variant: 0, box_p: 4.6, lump: 1.2 });
        let (x0, x1) = bounds_axis(&g.pos, 0);
        // The Lp-ball projection keeps every vertex within a modest margin of
        // the nominal half-width (folding/creasing can only shrink, plus a
        // small noise wobble).
        assert!(x1 <= 0.5 * 0.5 + 0.05 && x0 >= -0.5 * 0.5 - 0.05, "x range [{x0}, {x1}]");
    }

    #[test]
    fn sack_geometry_variants_produce_different_geometry() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(1);
        let ga = sack_geometry(&mut a, 0.5, 0.17, 0.3, SackOpts { variant: 0, box_p: 4.6, lump: 1.2 });
        let gb = sack_geometry(&mut b, 0.5, 0.17, 0.3, SackOpts { variant: 1, box_p: 4.6, lump: 1.2 });
        assert_ne!(ga.pos, gb.pos);
    }

    #[test]
    fn dust_skirt_has_the_expected_topology() {
        let mut rng = Rng::new(1);
        let g = dust_skirt(&mut rng);
        // Torso: (RAD+1)*(SEG+1) = 5*27 = 135. Two caps: (SEG+(SEG+1))*2 = 106.
        assert_eq!(g.vert_count(), 135 + 106);
        // Torso: SEG*RAD*2 = 208. Two caps: SEG*2 = 52.
        assert_eq!(g.tri_count(), 208 + 52);
    }

    #[test]
    fn dust_skirt_is_deterministic_and_feathers_to_nothing_at_the_rim() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        let ga = dust_skirt(&mut a);
        let gb = dust_skirt(&mut b);
        assert_eq!(ga.pos, gb.pos);
        // Every vertex's radial distance from the axis should be at most
        // ~1.28 (unit radius, wobbled by at most +28%).
        for p in ga.pos.chunks_exact(3) {
            let d = (f64::from(p[0]).powi(2) + f64::from(p[2]).powi(2)).sqrt();
            assert!(d < 1.3, "dust_skirt vertex strayed past the wobbled rim: d={d}");
        }
    }
}
