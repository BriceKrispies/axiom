//! Ported from Claude-of-Duty `src/world/util.js` — the sub-primitives
//! `kit.js`'s modular building elements share: `solidSlabs` (`util.js:591-618`),
//! `clothGeometry` (`util.js:760-988`), `tubeY` (`util.js:1094-1100`), and
//! `kit.js`'s own `polyPrism`, `rockGeometry` and `mergeSimple`
//! (`kit.js:1043-1065` docs `polyPrism`'s import from `util.js:622-639`;
//! `rockGeometry` is `util.js:726-743`; `mergeSimple` is `kit.js:456-498`).
//!
//! `new THREE.CylinderGeometry(...)` (`three/src/geometries/CylinderGeometry.js`,
//! MIT licensed, Three.js authors) is ported here too, promoted from
//! `crate::world::ground`'s private copy — see [`cylinder_geometry`]'s doc
//! for why: `tubeY` is the second caller `ground.rs`'s own doc comment
//! anticipated ("if a second caller arrives, promote it there").

use crate::rng::Rng;
use crate::weapons::geometry::primitives::{extrude, ExtrudeOpts};
use crate::world::noise::fbm3;

use super::WallHole;
use crate::world::geo::WorldGeo;

// ============================================================ solidSlabs ==
/// One rectangle of `solidSlabs`' output (`util.js:588-618`'s doc / return
/// shape `{x, y, w, h}`), in panel space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolidSlab {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// `solidSlabs(w, h, holes)` (`util.js:591-618`): the solid rectangles left
/// once every hole is cut out of a `w` x `h` panel — used for collision, so a
/// doorway is a real gap in the collision hull rather than a triangle-soup
/// query. Computed in `f64` (matching the source's JS numbers) and narrowed
/// to `f32` only in the returned [`SolidSlab`]s, consistent with the rest of
/// this port's "compute in f64, store f32" convention.
///
/// `arch`/`ragged` on a [`WallHole`] are ignored here exactly as the source's
/// `solidSlabs` ignores them (`util.js:591-618` only ever reads `x`/`y`/`w`/`h`
/// off each hole) — a ragged or arched cut still reserves its full bounding
/// rectangle for collision purposes.
pub fn solid_slabs(w: f32, h: f32, holes: &[WallHole]) -> Vec<SolidSlab> {
    let (wf, hf) = (f64::from(w), f64::from(h));

    let mut xs: Vec<f64> = Vec::new();
    let push_unique = |xs: &mut Vec<f64>, v: f64| {
        if !xs.iter().any(|&e| e == v) {
            xs.push(v);
        }
    };
    push_unique(&mut xs, -wf / 2.0);
    push_unique(&mut xs, wf / 2.0);
    for o in holes {
        push_unique(&mut xs, (-wf / 2.0).max(f64::from(o.x) - f64::from(o.w) / 2.0));
        push_unique(&mut xs, (wf / 2.0).min(f64::from(o.x) + f64::from(o.w) / 2.0));
    }
    xs.sort_by(|a, b| a.partial_cmp(b).expect("panel-space coordinates are always finite"));

    let mut out = Vec::new();
    for i in 0..xs.len().saturating_sub(1) {
        let (bx0, bx1) = (xs[i], xs[i + 1]);
        if bx1 - bx0 < 1e-4 {
            continue;
        }
        let mid = (bx0 + bx1) / 2.0;
        let mut spans: Vec<(f64, f64)> = holes
            .iter()
            .filter(|o| mid > f64::from(o.x) - f64::from(o.w) / 2.0 && mid < f64::from(o.x) + f64::from(o.w) / 2.0)
            .map(|o| {
                (
                    0.0f64.max(f64::from(o.y) - f64::from(o.h) / 2.0),
                    hf.min(f64::from(o.y) + f64::from(o.h) / 2.0),
                )
            })
            .collect();
        spans.sort_by(|a, b| a.0.partial_cmp(&b.0).expect("hole spans are always finite"));

        let mut y = 0.0f64;
        for (s0, s1) in spans {
            if s0 > y {
                out.push(SolidSlab {
                    x: ((bx0 + bx1) / 2.0) as f32,
                    y: ((y + s0) / 2.0) as f32,
                    w: (bx1 - bx0) as f32,
                    h: (s0 - y) as f32,
                });
            }
            y = y.max(s1);
        }
        if y < hf {
            out.push(SolidSlab {
                x: ((bx0 + bx1) / 2.0) as f32,
                y: ((y + hf) / 2.0) as f32,
                w: (bx1 - bx0) as f32,
                h: (hf - y) as f32,
            });
        }
    }
    out
}

// ========================================================= cylinderY/tubeY ==
/// `new THREE.CylinderGeometry(radiusTop, radiusBottom, height,
/// radialSegments, heightSegments, openEnded, thetaStart=0,
/// thetaLength=2*PI)` (`three/src/geometries/CylinderGeometry.js`, MIT
/// licensed, Three.js authors), specialized to `thetaStart = 0`,
/// `thetaLength = 2*PI` — no caller in this whole port (the ground manhole
/// ring, `tubeY`) ever narrows the swept angle.
///
/// Promoted from `crate::world::ground`'s private copy of this same function
/// (see this module's doc): that port left it private with the note "if a
/// second caller arrives, promote it there," and [`tube_y`] below is exactly
/// that second caller. The only change from the ground-only version is the
/// added `open_ended` parameter — the manhole ring never opted out of caps,
/// so that port's copy never modelled the gate at all; here it is real,
/// matching `CylinderGeometry.js`'s own `if (!openEnded) { ... }` guard.
pub fn cylinder_geometry(radius_top: f64, radius_bottom: f64, height: f64, radial_segments: u32, height_segments: u32, open_ended: bool) -> WorldGeo {
    let mut vertices: Vec<f32> = Vec::new();
    let mut normals: Vec<f32> = Vec::new();
    let mut uvs: Vec<f32> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let mut index: u32 = 0;
    let half_height = height / 2.0;
    let slope = (radius_bottom - radius_top) / height;

    let mut index_array: Vec<Vec<u32>> = Vec::new();
    for y in 0..=height_segments {
        let mut index_row = Vec::new();
        let v = f64::from(y) / f64::from(height_segments);
        let radius = v * (radius_bottom - radius_top) + radius_top;
        for x in 0..=radial_segments {
            let u = f64::from(x) / f64::from(radial_segments);
            let theta = u * std::f64::consts::TAU;
            let (sin_t, cos_t) = theta.sin_cos();
            vertices.push((radius * sin_t) as f32);
            vertices.push((-v * height + half_height) as f32);
            vertices.push((radius * cos_t) as f32);
            let nlen = (sin_t * sin_t + slope * slope + cos_t * cos_t).sqrt();
            normals.push((sin_t / nlen) as f32);
            normals.push((slope / nlen) as f32);
            normals.push((cos_t / nlen) as f32);
            uvs.push(u as f32);
            uvs.push((1.0 - v) as f32);
            index_row.push(index);
            index += 1;
        }
        index_array.push(index_row);
    }
    for x in 0..radial_segments {
        for y in 0..height_segments {
            let a = index_array[y as usize][x as usize];
            let b = index_array[(y + 1) as usize][x as usize];
            let c = index_array[(y + 1) as usize][(x + 1) as usize];
            let d = index_array[y as usize][(x + 1) as usize];
            if radius_top > 0.0 || y != 0 {
                indices.extend_from_slice(&[a, b, d]);
            }
            if radius_bottom > 0.0 || y != height_segments - 1 {
                indices.extend_from_slice(&[b, c, d]);
            }
        }
    }

    if !open_ended {
        if radius_top > 0.0 {
            cylinder_cap(true, radius_top, radius_bottom, radial_segments, half_height, &mut vertices, &mut normals, &mut uvs, &mut indices, &mut index);
        }
        if radius_bottom > 0.0 {
            cylinder_cap(false, radius_top, radius_bottom, radial_segments, half_height, &mut vertices, &mut normals, &mut uvs, &mut indices, &mut index);
        }
    }

    WorldGeo {
        pos: vertices,
        normal: normals,
        uv: uvs,
        color: Vec::new(),
        index: indices,
    }
}

/// `generateCap(top)` (`CylinderGeometry.js`'s inner function).
#[allow(clippy::too_many_arguments)]
fn cylinder_cap(
    top: bool,
    radius_top: f64,
    radius_bottom: f64,
    radial_segments: u32,
    half_height: f64,
    vertices: &mut Vec<f32>,
    normals: &mut Vec<f32>,
    uvs: &mut Vec<f32>,
    indices: &mut Vec<u32>,
    index: &mut u32,
) {
    let center_index_start = *index;
    let radius = if top { radius_top } else { radius_bottom };
    let sign: f64 = if top { 1.0 } else { -1.0 };

    for _ in 1..=radial_segments {
        vertices.push(0.0);
        vertices.push((half_height * sign) as f32);
        vertices.push(0.0);
        normals.push(0.0);
        normals.push(sign as f32);
        normals.push(0.0);
        uvs.push(0.5);
        uvs.push(0.5);
        *index += 1;
    }
    let center_index_end = *index;

    for x in 0..=radial_segments {
        let u = f64::from(x) / f64::from(radial_segments);
        let theta = u * std::f64::consts::TAU;
        let (sin_t, cos_t) = theta.sin_cos();
        vertices.push((radius * sin_t) as f32);
        vertices.push((half_height * sign) as f32);
        vertices.push((radius * cos_t) as f32);
        normals.push(0.0);
        normals.push(sign as f32);
        normals.push(0.0);
        uvs.push((cos_t * 0.5 + 0.5) as f32);
        uvs.push((sin_t * 0.5 * sign + 0.5) as f32);
        *index += 1;
    }

    for x in 0..radial_segments {
        let c = center_index_start + x;
        let i = center_index_end + x;
        if top {
            indices.extend_from_slice(&[i, i + 1, c]);
        } else {
            indices.extend_from_slice(&[i + 1, i, c]);
        }
    }
}

/// `tubeY(radius, height, opts = {})` (`util.js:1094-1100`): a straight tube
/// along +Y, capped, translated so it spans `y = 0..height` (the source's
/// `g.translate(0, height/2, 0)` after building a Y-centred cylinder).
/// Defaults: `radial=8`, `taper=1`, `open=false`, `seg=1`. No `color`
/// attribute, matching the source (never `paintMasks`/`fillMasks`ed here).
pub fn tube_y(radius: f32, height: f32, radial: u32, taper: f32, open: bool, seg: u32) -> WorldGeo {
    let mut g = cylinder_geometry(f64::from(radius * taper), f64::from(radius), f64::from(height), radial, seg, open);
    g.translate(0.0, height / 2.0, 0.0);
    g
}

// =============================================================== polyPrism ==
/// `polyPrism(pts, height, opts = {})` (`util.js:622-639`): a convex/simple
/// polygon (`pts = [[x, z], ...]`, CCW) extruded along `+Y`.
///
/// **Reuses `weapons::geometry::primitives::extrude`**, exactly the same
/// trade `crate::world::kit::wall_panel` already makes and documents in
/// full (its own doc comment's "Two consequences of that reuse" section) —
/// the same bevelled-extrude-with-holes engine rather than a second
/// hand-written `THREE.ExtrudeGeometry` + `ShapeUtils.triangulateShape`
/// copy. The source builds a raw `THREE.ExtrudeGeometry` directly
/// (`bevelEnabled: !!opts.bevel`, no caller in this port ever passes
/// `opts.bevel`, so `bevel = 0.0` here reproduces "disabled"), which never
/// translates its output (`z` spans `0..height`) and never welds; `extrude`
/// always both translates (`-depth/2 + bevel`) and welds at `1e-6`. The
/// translate is corrected below (add back `+height/2 - bevel`); the weld
/// only ever changes vertex count/order, never triangle count (see
/// `wall_panel`'s doc for why that is the accepted trade here too).
///
/// After that, `pts`'s XZ-authored shape is rotated `rotateX(-PI/2)`
/// (`util.js:635`) to lay the extrusion along `+Y` — `(x, y, z) -> (x, z,
/// -y)` — and vertex normals are recomputed from the rotated positions
/// (`util.js:636`; mathematically idempotent for a pure rotation, but ported
/// verbatim rather than "optimized" away, per the port recipe's "port the
/// behaviour" rule).
pub fn poly_prism(pts: &[[f64; 2]], height: f32, bevel: f32) -> WorldGeo {
    let raw = extrude(
        pts,
        height,
        ExtrudeOpts {
            bevel,
            bevel_segments: 1,
            curve_segments: 6,
            steps: 1,
            holes: Vec::new(),
        },
    );
    let mut g = WorldGeo {
        pos: raw.pos,
        normal: raw.normal,
        uv: raw.uv,
        color: Vec::new(),
        index: raw.index,
    };
    // Undo extrude()'s `-depth/2 + bevel` translate, matching wall_panel's
    // own correction: the source's raw ExtrudeGeometry is never translated.
    g.translate(0.0, 0.0, height / 2.0 - bevel);
    g.rotate_x(-std::f32::consts::FRAC_PI_2);
    g.compute_vertex_normals();
    g
}

// ============================================================= rockGeometry ==
/// `rockGeometry(rng, size = 0.3, detail = 1, squash = 0.7)`
/// (`util.js:726-743`): a noise-deformed rock / masonry chunk, built from
/// `new THREE.IcosahedronGeometry(size * 0.5, detail)`
/// (`three/src/geometries/IcosahedronGeometry.js` /
/// `PolyhedronGeometry.js`, MIT licensed, Three.js authors).
///
/// **Only `detail = 0` is implemented.** Every real call site across the
/// whole source — `kit.js`'s own `rubbleMound`, and every future caller in
/// `dressing.js`/`props.js` — passes `detail = 0` (grep-verified across
/// `src/world/*.js`); `PolyhedronGeometry`'s general `subdivide(detail)`
/// (barycentric subdivision of each of the 20 base faces, plus its
/// `generateUVs`/`correctUVs`/`correctSeam` azimuth-wrap fixups) would be
/// dead, untested code with no exerciser anywhere in this port. At
/// `detail = 0`, `subdivideFace` algebraically collapses to "emit the base
/// triangle's three corners, in the cyclic order `(b, c, a)` for base face
/// `(a, b, c)`" (worked from `PolyhedronGeometry.js`'s own `subdivideFace`
/// with `cols = 1`), which is what this builds directly: the 20 fixed
/// icosahedron faces, each vertex normalized then scaled to `radius`. `uv`
/// is left empty rather than reproducing `generateUVs`'s azimuth/inclination
/// projection: `rockGeometry` never reads its own `uv` attribute (only
/// `pa`/`na` are touched below and by every caller's `paintMasks`), and
/// downstream `Accum.add` already treats a missing `uv` as `[0, 0]` per
/// vertex (`util.js:149`) — an intentional, documented divergence rather
/// than a silent one, exactly as `patch_geometry`'s own missing `color`
/// column already is in this port.
///
/// Panics if `detail != 0`, for the same reason `crate::rng::Rng::pick`
/// panics on an empty slice rather than silently returning a wrong answer:
/// a future caller that actually needs subdivision should fail loudly at
/// the call site, not receive an unsubdivided rock.
pub fn rock_geometry(rng: &mut Rng, size: f32, detail: u32, squash: f32) -> WorldGeo {
    assert_eq!(detail, 0, "rock_geometry: subdivision (detail > 0) has no caller anywhere in this port and is not implemented");
    let radius = f64::from(size) * 0.5;

    // The base icosahedron: `t = golden ratio`, 12 vertices, 20 triangular
    // faces (`IcosahedronGeometry.js`).
    let t = (1.0 + 5.0f64.sqrt()) / 2.0;
    let base: [[f64; 3]; 12] = [
        [-1.0, t, 0.0],
        [1.0, t, 0.0],
        [-1.0, -t, 0.0],
        [1.0, -t, 0.0],
        [0.0, -1.0, t],
        [0.0, 1.0, t],
        [0.0, -1.0, -t],
        [0.0, 1.0, -t],
        [t, 0.0, -1.0],
        [t, 0.0, 1.0],
        [-t, 0.0, -1.0],
        [-t, 0.0, 1.0],
    ];
    const FACES: [[usize; 3]; 20] = [
        [0, 11, 5], [0, 5, 1], [0, 1, 7], [0, 7, 10], [0, 10, 11],
        [1, 5, 9], [5, 11, 4], [11, 10, 2], [10, 7, 6], [7, 1, 8],
        [3, 9, 4], [3, 4, 2], [3, 2, 6], [3, 6, 8], [3, 8, 9],
        [4, 9, 5], [2, 4, 11], [6, 2, 10], [8, 6, 7], [9, 8, 1],
    ];

    let project = |v: [f64; 3]| -> [f64; 3] {
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / len * radius, v[1] / len * radius, v[2] / len * radius]
    };

    let mut pos: Vec<f64> = Vec::with_capacity(60 * 3);
    for [i0, i1, i2] in FACES {
        // `subdivideFace` at `detail = 0` emits `(b, c, a)` for base face
        // `(a, b, c)` — see this function's doc.
        for v in [project(base[i1]), project(base[i2]), project(base[i0])] {
            pos.extend_from_slice(&v);
        }
    }

    let seed = rng.float() * 40.0;
    let squash = f64::from(squash);
    for i in 0..pos.len() / 3 {
        let (x, y, z) = (pos[i * 3], pos[i * 3 + 1], pos[i * 3 + 2]);
        let n = fbm3(x * 7.0 + seed, y * 7.0 + seed, z * 7.0 + seed, 2);
        // Faceted, not blobby: quantise the radius a little.
        let f = 0.62 + n * 0.72;
        pos[i * 3] = x * f;
        pos[i * 3 + 1] = y * f * squash;
        pos[i * 3 + 2] = z * f;
    }

    let mut g = WorldGeo {
        pos: pos.iter().map(|&v| v as f32).collect(),
        normal: Vec::new(),
        uv: Vec::new(),
        color: Vec::new(),
        index: Vec::new(),
    };
    g.compute_vertex_normals();
    g
}

// ============================================================= mergeSimple ==
/// `mergeSimple(list)` (`kit.js:456-498`): a minimal merge for kit sub-parts
/// that are all `plainBox()`/`chamferBox()`-shaped (same fixed
/// position/normal/uv/color attribute set). Unlike
/// [`crate::world::accum::Accum::add`], this never transforms by a matrix
/// (every part is already baked into its own local space by the caller
/// before merging — `sashLeaf`/`shutterLeaf`/`doorLeaf`, all below), never
/// widens masks, and never welds: it is exactly index-offset concatenation.
/// A missing `normal`/`uv`/`color` on any input is left as the zero-filled
/// slot the source's pre-sized `Float32Array`s default to (`kit.js:463-466`)
/// — **not** the same fallback as `Accum::add`, which computes real vertex
/// normals for a normal-less input; every real caller here only ever merges
/// `plainBox()`-derived parts, which always carry all four attributes, so
/// the distinction is inert in practice but kept faithful to the source's
/// actual (simpler, zero-filling) code path rather than the busier one nearby.
pub fn merge_simple(parts: &[WorldGeo]) -> WorldGeo {
    let vert_count: usize = parts.iter().map(WorldGeo::vert_count).sum();
    let index_count: usize = parts
        .iter()
        .map(|g| if g.index.is_empty() { g.vert_count() } else { g.index.len() })
        .sum();

    let mut pos = vec![0.0f32; vert_count * 3];
    let mut normal = vec![0.0f32; vert_count * 3];
    let mut uv = vec![0.0f32; vert_count * 2];
    let mut color = vec![0.0f32; vert_count * 3];
    let mut index = Vec::with_capacity(index_count);

    let mut vo = 0usize;
    for g in parts {
        let vc = g.vert_count();
        pos[vo * 3..(vo + vc) * 3].copy_from_slice(&g.pos);
        if !g.normal.is_empty() {
            normal[vo * 3..(vo + vc) * 3].copy_from_slice(&g.normal);
        }
        if !g.uv.is_empty() {
            uv[vo * 2..(vo + vc) * 2].copy_from_slice(&g.uv);
        }
        if !g.color.is_empty() {
            color[vo * 3..(vo + vc) * 3].copy_from_slice(&g.color);
        }
        if g.index.is_empty() {
            index.extend((0..vc as u32).map(|i| vo as u32 + i));
        } else {
            index.extend(g.index.iter().map(|&i| vo as u32 + i));
        }
        vo += vc;
    }

    WorldGeo { pos, normal, uv, color, index }
}

// ============================================================ clothGeometry ==
/// `clothGeometry(w, h, opts = {})`'s options (`util.js:760-788`). Defaults
/// match the source: `seg_x=10`, `seg_y=8`, `sag=0.12`, `wrinkle=0.03`,
/// `twist=0.0`, `bulge=0.0`, `u_range=None` (full `0..1`), `seed=None` (drawn
/// from `rng` when absent), `thickness=0.0022`, `hem=1.0`, `fray=0.0`,
/// `bow=1.0`.
#[derive(Debug, Clone, Copy)]
pub struct ClothOpts {
    pub seg_x: u32,
    pub seg_y: u32,
    pub sag: f32,
    pub wrinkle: f32,
    pub twist: f32,
    pub bulge: f32,
    pub u_range: Option<(f32, f32)>,
    pub seed: Option<f32>,
    pub thickness: f32,
    pub hem: f32,
    pub fray: f32,
    pub bow: f32,
}

impl Default for ClothOpts {
    fn default() -> Self {
        ClothOpts {
            seg_x: 10,
            seg_y: 8,
            sag: 0.12,
            wrinkle: 0.03,
            twist: 0.0,
            bulge: 0.0,
            u_range: None,
            seed: None,
            thickness: 0.0022,
            hem: 1.0,
            fray: 0.0,
            bow: 1.0,
        }
    }
}

/// Triangle wave in `-1..1` (`util.js:804-807`): creases have corners, sines
/// do not.
fn tri_wave(t: f32) -> f32 {
    let f = t.rem_euclid(1.0);
    if f < 0.5 {
        f * 4.0 - 1.0
    } else {
        3.0 - f * 4.0
    }
}

/// `clothGeometry(w, h, opts = {})` (`util.js:760-988`): hanging/draped
/// cloth, built as a real double shell (a deformed mid-surface offset by a
/// per-vertex half-thickness along its own normal, in both directions)
/// closed by a rim strip that thickens into a rolled hem at the free edges —
/// see the source's own doc comment for why a cloth is never a zero-thickness
/// quad.
pub fn cloth_geometry(w: f32, h: f32, opts: ClothOpts, rng: Option<&mut Rng>) -> WorldGeo {
    let (u0, u1) = opts.u_range.unwrap_or((0.0, 1.0));
    let sw = w * (u1 - u0);
    let nx = opts.seg_x + 1;
    let ny = opts.seg_y + 1;
    let nv = (nx * ny) as usize;
    let seed = opts.seed.unwrap_or_else(|| rng.map_or(0.0, |r| r.float() as f32 * 30.0));

    let mut p = vec![0.0f32; nv * 3];
    let mut n = vec![0.0f32; nv * 3];
    let mut uv = vec![0.0f32; nv * 2];
    let mut ht = vec![0.0f32; nv]; // half thickness per vertex
    let mut ao = vec![0.0f32; nv]; // crease occlusion per vertex

    for j in 0..ny {
        for i in 0..nx {
            let k = (j * nx + i) as usize;
            let uu = i as f32 / opts.seg_x as f32;
            let u = u0 + uu * (u1 - u0);
            let v = j as f32 / opts.seg_y as f32;
            let mut x = (u - 0.5) * w;
            let mut y = (v - 0.5) * h;
            let cat = ((u - 0.5) * 2.2).cosh() - 1.0;
            let mut z = -cat * opts.sag * (1.0 - v * 0.35);
            z += (v * 7.1 + u * 3.3 + seed).sin() * opts.wrinkle * (0.4 + u * (1.0 - u) * 3.0);
            z += (u * 11.3 + seed * 2.0).sin() * opts.wrinkle * 0.5 * v;
            let cr = tri_wave(u * 2.6 + v * 1.15 + seed * 0.37);
            z += cr * opts.wrinkle * 0.85 * (0.4 + 0.6 * (1.0 - v));
            z -= opts.bulge * (u * std::f32::consts::PI).sin() * (v * std::f32::consts::PI).sin();
            z *= opts.bow;
            y -= cat * opts.sag * 0.5;
            x += opts.twist * (v - 0.5) * (u * 4.0 + seed).sin();
            if j == 0 && opts.fray > 0.0 {
                y -= opts.fray * (0.3 + 0.7 * (u * 8.7 + seed * 1.7).sin().abs());
                x += opts.fray * 0.35 * (u * 15.3 + seed).sin();
            }
            p[k * 3] = x;
            p[k * 3 + 1] = y;
            p[k * 3 + 2] = z;
            uv[k * 2] = uu;
            uv[k * 2 + 1] = v;

            let d_bottom = v * h;
            let d_top = (1.0 - v) * h;
            let d_side = uu.min(1.0 - uu) * sw;
            let band = (1.0 - d_bottom / 0.045).max(0.0).max(0.55 * (1.0 - d_top.min(d_side) / 0.03).max(0.0));
            ht[k] = opts.thickness * 0.5 * (1.0 + opts.hem * 2.8 * band * band);
            ao[k] = (-cr).max(0.0) * 0.4 + band * 0.3;
        }
    }

    // Mid-surface normals from grid tangents (`util.js:852-874`).
    for j in 0..ny {
        for i in 0..nx {
            let k = (j * nx + i) as usize;
            let i0 = if i > 0 { i - 1 } else { i };
            let i1 = if i < nx - 1 { i + 1 } else { i };
            let j0 = if j > 0 { j - 1 } else { j };
            let j1 = if j < ny - 1 { j + 1 } else { j };
            let tu = [
                p[((j * nx + i1) as usize) * 3] - p[((j * nx + i0) as usize) * 3],
                p[((j * nx + i1) as usize) * 3 + 1] - p[((j * nx + i0) as usize) * 3 + 1],
                p[((j * nx + i1) as usize) * 3 + 2] - p[((j * nx + i0) as usize) * 3 + 2],
            ];
            let tv = [
                p[((j1 * nx + i) as usize) * 3] - p[((j0 * nx + i) as usize) * 3],
                p[((j1 * nx + i) as usize) * 3 + 1] - p[((j0 * nx + i) as usize) * 3 + 1],
                p[((j1 * nx + i) as usize) * 3 + 2] - p[((j0 * nx + i) as usize) * 3 + 2],
            ];
            let nxv = tu[1] * tv[2] - tu[2] * tv[1];
            let nyv = tu[2] * tv[0] - tu[0] * tv[2];
            let nzv = tu[0] * tv[1] - tu[1] * tv[0];
            let l = (nxv * nxv + nyv * nyv + nzv * nzv).sqrt();
            let l = if l == 0.0 { 1.0 } else { l };
            n[k * 3] = nxv / l;
            n[k * 3 + 1] = nyv / l;
            n[k * 3 + 2] = nzv / l;
        }
    }

    let mut pos = Vec::new();
    let mut normal = Vec::new();
    let mut out_uv = Vec::new();
    let mut col = Vec::new();
    let mut idx = Vec::new();
    let push = |pos: &mut Vec<f32>, normal: &mut Vec<f32>, out_uv: &mut Vec<f32>, col: &mut Vec<f32>, px: f32, py: f32, pz: f32, nx2: f32, ny2: f32, nz2: f32, u2: f32, v2: f32, wear: f32, grime: f32, ambient: f32| -> u32 {
        pos.extend_from_slice(&[px, py, pz]);
        normal.extend_from_slice(&[nx2, ny2, nz2]);
        out_uv.extend_from_slice(&[u2, v2]);
        col.extend_from_slice(&[wear, grime, ambient]);
        (pos.len() / 3 - 1) as u32
    };

    // The two shells (`util.js:889-921`).
    for s in 0..2 {
        let sign: f32 = if s == 0 { 1.0 } else { -1.0 };
        let base = (pos.len() / 3) as u32;
        for k in 0..nv {
            let o = ht[k] * sign;
            // The back of a hanging cloth is dustier and never gets sun.
            let grime = if s == 0 { ao[k] * 0.5 } else { 0.28 + ao[k] * 0.5 };
            push(
                &mut pos, &mut normal, &mut out_uv, &mut col,
                p[k * 3] + n[k * 3] * o,
                p[k * 3 + 1] + n[k * 3 + 1] * o,
                p[k * 3 + 2] + n[k * 3 + 2] * o,
                n[k * 3] * sign,
                n[k * 3 + 1] * sign,
                n[k * 3 + 2] * sign,
                uv[k * 2],
                uv[k * 2 + 1],
                ao[k] * 0.5,
                grime,
                ao[k] * (if s == 0 { 1.0 } else { 1.3 }),
            );
        }
        for j in 0..opts.seg_y {
            for i in 0..opts.seg_x {
                let a = base + j * nx + i;
                let b = a + 1;
                let c = a + nx;
                let d = c + 1;
                if sign > 0.0 {
                    idx.extend_from_slice(&[a, b, d, a, d, c]);
                } else {
                    idx.extend_from_slice(&[a, d, b, a, c, d]);
                }
            }
        }
    }

    // The rim strip: the hem, which gives the edge a silhouette
    // (`util.js:923-977`).
    let mut loop_indices: Vec<u32> = Vec::new();
    for i in 0..nx - 1 {
        loop_indices.push(i);
    }
    for j in 0..ny - 1 {
        loop_indices.push(j * nx + (nx - 1));
    }
    for i in (1..nx).rev() {
        loop_indices.push((ny - 1) * nx + i);
    }
    for j in (1..ny).rev() {
        loop_indices.push(j * nx);
    }
    let loop_len = loop_indices.len() as u32;
    for e in 0..loop_len {
        let ka = loop_indices[e as usize] as usize;
        let kb = loop_indices[((e + 1) % loop_len) as usize] as usize;
        let (ax, ay, az) = (p[ka * 3], p[ka * 3 + 1], p[ka * 3 + 2]);
        let (bx, by, bz) = (p[kb * 3], p[kb * 3 + 1], p[kb * 3 + 2]);
        let (ex, ey, ez) = (bx - ax, by - ay, bz - az);
        let mnx = (n[ka * 3] + n[kb * 3]) * 0.5;
        let mny = (n[ka * 3 + 1] + n[kb * 3 + 1]) * 0.5;
        let mnz = (n[ka * 3 + 2] + n[kb * 3 + 2]) * 0.5;
        let mut ox = ey * mnz - ez * mny;
        let mut oy = ez * mnx - ex * mnz;
        let mut oz = ex * mny - ey * mnx;
        let ol = (ox * ox + oy * oy + oz * oz).sqrt();
        let ol = if ol == 0.0 { 1.0 } else { ol };
        ox /= ol;
        oy /= ol;
        oz /= ol;

        let mut q = [0u32; 4];
        for (slot, &(k, sgn)) in [(ka, 1.0f32), (ka, -1.0), (kb, -1.0), (kb, 1.0)].iter().enumerate() {
            let o = ht[k] * sgn;
            q[slot] = push(
                &mut pos, &mut normal, &mut out_uv, &mut col,
                p[k * 3] + n[k * 3] * o,
                p[k * 3 + 1] + n[k * 3 + 1] * o,
                p[k * 3 + 2] + n[k * 3 + 2] * o,
                ox, oy, oz,
                uv[k * 2], uv[k * 2 + 1],
                0.45,
                0.3 + ao[k] * 0.4,
                (ao[k] + 0.2).min(1.0),
            );
        }
        idx.extend_from_slice(&[q[0], q[1], q[2], q[0], q[2], q[3]]);
    }

    WorldGeo { pos, normal, uv: out_uv, color: col, index: idx }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_slabs_no_holes_is_one_full_rect() {
        let out = solid_slabs(2.0, 3.0, &[]);
        assert_eq!(out, vec![SolidSlab { x: 0.0, y: 1.5, w: 2.0, h: 3.0 }]);
    }

    #[test]
    fn solid_slabs_one_centered_hole_splits_into_three_bands() {
        let hole = WallHole { x: 0.0, y: 1.5, w: 0.6, h: 0.8, arch: 0.0, ragged: 0.0 };
        let out = solid_slabs(2.0, 3.0, &[hole]);
        // Middle band: below + above the hole. Side bands: full height each.
        assert_eq!(out.len(), 4);
        let total_area: f32 = out.iter().map(|s| s.w * s.h).sum();
        assert!((total_area - (2.0 * 3.0 - 0.6 * 0.8)).abs() < 1e-4);
    }

    #[test]
    fn solid_slabs_ignores_arch_and_ragged_and_uses_the_bounding_rect() {
        let plain = WallHole { x: 0.0, y: 1.0, w: 0.5, h: 0.5, arch: 0.0, ragged: 0.0 };
        let arched = WallHole { x: 0.0, y: 1.0, w: 0.5, h: 0.5, arch: 0.9, ragged: 0.0 };
        assert_eq!(solid_slabs(2.0, 2.0, &[plain]), solid_slabs(2.0, 2.0, &[arched]));
    }

    #[test]
    fn cylinder_geometry_matches_three_cylindergeometry_dimensions() {
        let g = cylinder_geometry(0.36, 0.36, 0.04, 18, 1, false);
        let torso_verts = 19 * 2;
        let cap_verts = (18 + 19) * 2;
        assert_eq!(g.vert_count(), torso_verts + cap_verts);
        assert_eq!(g.tri_count(), 18 * 1 * 2 + 18 + 18);
    }

    #[test]
    fn cylinder_geometry_open_ended_skips_both_caps() {
        let closed = cylinder_geometry(0.36, 0.36, 0.04, 18, 1, false);
        let open = cylinder_geometry(0.36, 0.36, 0.04, 18, 1, true);
        // Torso-only: same vertex count as the closed case minus both caps.
        assert_eq!(open.vert_count(), closed.vert_count() - (18 + 19) * 2);
        assert_eq!(open.tri_count(), 18 * 1 * 2);
    }

    #[test]
    fn tube_y_spans_zero_to_height() {
        let g = tube_y(0.05, 2.0, 8, 1.0, false, 1);
        let y_min = g.pos.iter().skip(1).step_by(3).copied().fold(f32::INFINITY, f32::min);
        let y_max = g.pos.iter().skip(1).step_by(3).copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(y_min > -0.05 && y_min < 0.05, "y_min={y_min}");
        assert!(y_max > 1.95 && y_max < 2.05, "y_max={y_max}");
    }

    #[test]
    fn poly_prism_extrudes_along_y() {
        let pts = [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]];
        let g = poly_prism(&pts, 1.0, 0.0);
        assert!(g.vert_count() > 0);
        let y_min = g.pos.iter().skip(1).step_by(3).copied().fold(f32::INFINITY, f32::min);
        let y_max = g.pos.iter().skip(1).step_by(3).copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(y_min < 0.05 && y_min > -0.05, "y_min={y_min}");
        assert!(y_max < 1.05 && y_max > 0.95, "y_max={y_max}");
    }

    #[test]
    fn rock_geometry_at_detail_zero_is_the_base_icosahedron_triangle_soup() {
        let mut rng = Rng::new(1);
        let g = rock_geometry(&mut rng, 0.3, 0, 0.75);
        assert_eq!(g.vert_count(), 60);
        assert_eq!(g.tri_count(), 20);
        assert!(g.index.is_empty());
    }

    #[test]
    #[should_panic(expected = "not implemented")]
    fn rock_geometry_panics_on_nonzero_detail() {
        let mut rng = Rng::new(1);
        rock_geometry(&mut rng, 0.3, 1, 0.75);
    }

    fn tiny_box() -> WorldGeo {
        WorldGeo {
            pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normal: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            uv: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            color: vec![0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1],
            index: Vec::new(),
        }
    }

    #[test]
    fn merge_simple_concatenates_without_welding_or_transforming() {
        let merged = merge_simple(&[tiny_box(), tiny_box()]);
        assert_eq!(merged.vert_count(), 6);
        assert_eq!(merged.tri_count(), 2);
        assert_eq!(merged.index, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn merge_simple_zero_fills_a_missing_attribute() {
        let mut no_uv = tiny_box();
        no_uv.uv.clear();
        let merged = merge_simple(&[no_uv, tiny_box()]);
        assert_eq!(&merged.uv[0..6], &[0.0; 6]);
        assert_eq!(&merged.uv[6..12], &[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn cloth_geometry_double_shell_plus_hem_has_the_expected_triangle_count() {
        let mut rng = Rng::new(3);
        let g = cloth_geometry(1.0, 1.5, ClothOpts { seg_x: 4, seg_y: 3, ..ClothOpts::default() }, Some(&mut rng));
        // Two shells of segX*segY*2 tris each, plus a rim quad (2 tris) per
        // boundary edge: 2*(nx-1) + 2*(ny-1) edges.
        let shell_tris = 4 * 3 * 2 * 2;
        let rim_edges = 2 * 4 + 2 * 3;
        assert_eq!(g.tri_count(), shell_tris + rim_edges * 2);
    }

    #[test]
    fn cloth_geometry_deterministic_seed_reproduces_the_same_geometry() {
        let mut a = Rng::new(9);
        let mut b = Rng::new(9);
        let ga = cloth_geometry(1.0, 1.0, ClothOpts { seg_x: 3, seg_y: 3, ..ClothOpts::default() }, Some(&mut a));
        let gb = cloth_geometry(1.0, 1.0, ClothOpts { seg_x: 3, seg_y: 3, ..ClothOpts::default() }, Some(&mut b));
        assert_eq!(ga.pos, gb.pos);
    }

    #[test]
    fn cloth_geometry_explicit_seed_ignores_rng() {
        let ga = cloth_geometry(1.0, 1.0, ClothOpts { seg_x: 3, seg_y: 3, seed: Some(5.0), ..ClothOpts::default() }, None);
        let gb = cloth_geometry(1.0, 1.0, ClothOpts { seg_x: 3, seg_y: 3, seed: Some(5.0), ..ClothOpts::default() }, None);
        assert_eq!(ga.pos, gb.pos);
    }
}
