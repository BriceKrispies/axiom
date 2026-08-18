//! Ported from Claude-of-Duty `src/weapons/geometry.js:154-205` (`extrude`,
//! `roundRect`), plus `THREE.ExtrudeGeometry`
//! (`three/src/geometries/ExtrudeGeometry.js`) and `THREE.ShapeUtils`
//! (`three/src/extras/ShapeUtils.js`), both MIT licensed, Three.js authors.
//!
//! `geometry.js`'s `extrude()` always builds its `THREE.Shape` from straight
//! `moveTo`/`lineTo` segments (never a curve or arc), so `Shape.getPoints()`
//! — normally a curve-subdivision walk — collapses to "the input points,
//! plus one closing point back to the start if the shape isn't already
//! closed" (every `LineCurve` sub-segment samples to exactly its two
//! endpoints, and `curveSegments` never changes that). This port skips
//! porting `Shape`/`Path`/`CurvePath` for that reason: it takes the caller's
//! `pts`/`holes` directly as the closed-loop point lists `ExtrudeGeometry`'s
//! `addShape` would have derived, and never emits the redundant closing
//! duplicate that `Path.closePath()` would have added and
//! `mergeOverlappingPoints` would have promptly deleted again. `curve_segments`
//! stays in [`ExtrudeOpts`] purely for signature fidelity with the source's
//! options object; it has no effect here.
//!
//! ## A real precision boundary, mostly fixed: `f32` points into a
//! division-heavy bevel
//!
//! `get_bevel_vec` (`ExtrudeGeometry.js:234-355`) is provably a bit-exact
//! port: fed the *same* coordinates the JavaScript computes internally, a
//! corner reproduces the source's `getBevelVec` output to the last bit. An
//! earlier version of this contract fixed `pts` at `&[[f32; 2]]`, and
//! `round_rect` (and every other contour producer) truncated to `f32` —
//! about 7 significant decimal digits — *before* `extrude` widened back to
//! `f64`, where the JavaScript keeps its `roundRect`/`Shape.moveTo`/`lineTo`
//! coordinates as plain (`f64`) numbers all the way through. `get_bevel_vec`'s
//! shift-and-intersect construction divides by
//! `v_prev_x*v_next_y - v_prev_y*v_next_x` (`ExtrudeGeometry.js:279`) — near
//! zero at a junction where an arc meets a straight edge it is exactly
//! tangent to (`round_rect`'s corners are built to be exactly that) — which
//! amplified that `f32`-rounding noise from ~`1e-7` relative up past `1e-6`
//! absolute in the emitted bevel vector, on inputs shaped exactly like
//! `round_rect`'s regular, tangent-continuous corners.
//!
//! `03-weapon-geometry-api.md`'s "Corrections to this contract" section
//! fixes this at the boundary rather than widening the tolerance: `pts`,
//! `ExtrudeOpts::holes`, and `round_rect`'s signature are all `f64` now, so
//! no contour value ever round-trips through an `f32` narrowing between
//! being computed and being fed to `get_bevel_vec`'s division. The
//! **triangle count** (`Geo::tri_count`) was never affected either way — it
//! is fixed by `earcut`'s triangulation of `contour`/`holes`, which does not
//! go through this division.
//!
//! ## A smaller residual that is *not* a narrowing bug: independent libm
//!
//! Even at full `f64`, `round_rect`'s corners come from `f64::sin`/`f64::cos`
//! — a different libm than V8's, which can (and measurably does, for the
//! `round_rect`/`picatinny`-shaped goldens) differ by roughly one ULP
//! (`2^-52` relative). Divided by the same near-zero tangent-junction
//! denominator above, that ULP-level noise is still enough, on some inputs,
//! to tip [`weld_vertices`]'s `1e-6` quantization hash to a different bucket
//! than the source's own `mergeVertices`, or (when it doesn't change which
//! bucket a vertex lands in) to land a retained normal component just past
//! the `1e-6` tolerance. `tests/weapons_geometry_primitives_port.rs`'s
//! `assert_geo_topology_matches` documents the exact measurements for the
//! three goldens this still affects (`extrude_normal`, `picatinny_normal`,
//! `mlok_slot_normal`) — this is the honest floor: the algorithm is a
//! faithful port (verified bit-exact when both sides start from identical
//! coordinates), and no amount of additional precision closes a gap that
//! originates in two independent transcendental-function implementations.

use super::earcut;
use super::xform;
use super::super::Geo;

/// `options` on `extrude(pts, depth, opts = {})` (`geometry.js:154-185`).
/// Defaults match the source: `bevel = 0.0008`, `bevel_segments = 1`,
/// `curve_segments = 6` (unused, see module docs), `steps = 1`, no holes.
#[derive(Clone, Debug)]
pub struct ExtrudeOpts {
    pub bevel: f32,
    pub bevel_segments: u32,
    pub curve_segments: u32,
    pub steps: u32,
    pub holes: Vec<Vec<[f64; 2]>>,
}

impl Default for ExtrudeOpts {
    fn default() -> Self {
        ExtrudeOpts {
            bevel: 0.0008,
            bevel_segments: 1,
            curve_segments: 6,
            steps: 1,
            holes: Vec::new(),
        }
    }
}

/// Extrude a 2-D outline (in XY) along Z with a real bevel on both faces.
/// `pts` is closed automatically (as `THREE.Shape.closePath()` would close
/// it — see module docs for why that step is a no-op here). `pts` and
/// `opts.holes` are `f64` — see the module doc's "A real precision
/// boundary" section: this is a contour that feeds `get_bevel_vec`'s
/// division, and JS numbers (what the source actually computes with) are
/// `f64` throughout.
pub fn extrude(pts: &[[f64; 2]], depth: f32, opts: ExtrudeOpts) -> Geo {
    let bevel = opts.bevel;
    let bevel_enabled = bevel > 1e-6;

    let contour: Vec<(f64, f64)> = pts.iter().map(|p| (p[0], p[1])).collect();
    let holes: Vec<Vec<(f64, f64)>> = opts.holes.iter().map(|h| h.iter().map(|p| (p[0], p[1])).collect()).collect();

    let depth_adjusted = (f64::from(depth) - f64::from(bevel) * 2.0).max(1e-4);

    let (pos, uv) = extrude_shape(
        &contour,
        &holes,
        depth_adjusted,
        bevel_enabled,
        f64::from(bevel),
        opts.bevel_segments,
        opts.steps,
    );

    let mut g = Geo {
        pos,
        normal: Vec::new(),
        uv,
        index: Vec::new(),
    };
    // `this.computeVertexNormals()` (`ExtrudeGeometry.js:75`), run over the
    // non-indexed triangle soup `addShape` built — flat per-triangle
    // normals, exactly what an empty `normal` drives `normalize_attributes`
    // to compute.
    g.normalize_attributes();

    // `g.translate(0, 0, -depth / 2 + bevel)` (`geometry.js:178`) — the
    // *original* `depth`, not `depth_adjusted`.
    xform::translate(&mut g, 0.0, 0.0, -depth / 2.0 + bevel);

    // `mergeVertices(normalizeAttributes(g), 1e-6)` (`geometry.js:182`).
    let mut welded = weld_vertices(&g);
    welded.normalize_attributes();
    welded
}

/// A rounded rectangle outline, for extruded plates that need soft corners.
/// `seg` default `3` (`geometry.js:188`).
///
/// **Contract deviation:** `03-weapon-geometry-api.md` declares this
/// `-> Geo`, but every real caller (`mlokSlot`, `geometry.js:352-353`) feeds
/// its return straight into `extrude(pts, ...)`, whose first parameter is a
/// point list — `roundRect` builds an outline, not a mesh, in the source
/// (`geometry.js:188-205`) exactly as its name says. A `Geo`-returning
/// `round_rect` cannot be `extrude`'s first argument and cannot compile
/// against its only call site, so this returns `Vec<[f64; 2]>`, matching
/// both the source semantics and `extrude`'s actual signature. Noted in
/// `docs/work-manifests/shmup-port/notes/weapons-geometry-primitives.md`.
///
/// `w`/`h`/`r` are `f64`, not `f32`, per `03-weapon-geometry-api.md`'s
/// "Corrections" section: this is a contour producer whose output feeds
/// `extrude`'s division-heavy bevel path, so it stays full-precision from
/// its own inputs onward — never narrowed to `f32` and then widened back.
pub fn round_rect(w: f64, h: f64, r: f64, seg: u32) -> Vec<[f64; 2]> {
    let hw = w / 2.0 - r;
    let hh = h / 2.0 - r;
    let corners = [
        (hw, hh, 0.0),
        (-hw, hh, std::f64::consts::FRAC_PI_2),
        (-hw, -hh, std::f64::consts::PI),
        (hw, -hh, -std::f64::consts::FRAC_PI_2),
    ];
    let mut pts = Vec::new();
    corners.iter().for_each(|&(cx, cy, a0)| {
        (0..=seg).for_each(|i| {
            let a = a0 + (f64::from(i) / f64::from(seg)) * std::f64::consts::FRAC_PI_2;
            pts.push([cx + a.cos() * r, cy + a.sin() * r]);
        });
    });
    pts
}

/// `addShape(shape)` (`ExtrudeGeometry.js:79-746`), specialized to a single
/// shape (`geometry.js`'s `extrude` never passes an array) and with no
/// `extrudePath` (never used by this kit — bevels are always enabled or the
/// caller passes `bevel: 0.0`, never a spline extrusion). Returns the flat
/// `(position, uv)` component arrays `ExtrudeGeometry`'s
/// `verticesArray`/`uvArray` become; the caller fills in `normal` via
/// `Geo::normalize_attributes` (`computeVertexNormals`, matching
/// `ExtrudeGeometry.js:75`).
fn extrude_shape(
    contour_in: &[(f64, f64)],
    holes_in: &[Vec<(f64, f64)>],
    depth: f64,
    bevel_enabled: bool,
    bevel_in: f64,
    bevel_segments_in: u32,
    steps: u32,
) -> (Vec<f32>, Vec<f32>) {
    // "Safeguards if bevels are not enabled" (`ExtrudeGeometry.js:127-134`).
    let (bevel_segments, bevel_thickness, bevel_size, bevel_offset) = if bevel_enabled {
        (bevel_segments_in, bevel_in, bevel_in, 0.0)
    } else {
        (0u32, 0.0, 0.0, 0.0)
    };

    // `shape.extractPoints(curveSegments)` / `Path.getPoints()`
    // (`ExtrudeGeometry.js:138-142`): for a straight-line-only path this is
    // the input points, PLUS one closing point equal to the first if the
    // path isn't already closed (`Path.closePath()`, `CurvePath.js:56-71`).
    // That duplicate matters — `merge_overlapping_points` below deletes it
    // again, but *which* of the two coincident points survives determines
    // where the final contour starts, i.e. every subsequent vertex index.
    // Skipping this step (as an earlier version of this port did) produced
    // the right point *set* but the wrong *order* whenever `reverse` was
    // `false`, corrupting every downstream index.
    let mut vertices: Vec<(f64, f64)> = close_ring(contour_in);
    let mut holes: Vec<Vec<(f64, f64)>> = holes_in.iter().map(|h| close_ring(h)).collect();

    // `const reverse = !ShapeUtils.isClockWise(vertices);` (`ExtrudeGeometry.js:143-163`).
    if !is_clockwise(&vertices) {
        vertices.reverse();
        holes.iter_mut().for_each(|h| {
            if is_clockwise(h) {
                h.reverse();
            }
        });
    }

    merge_overlapping_points(&mut vertices);
    holes.iter_mut().for_each(|h| merge_overlapping_points(h));

    let contour = vertices.clone();
    holes.iter().for_each(|h| vertices.extend_from_slice(h));
    let vlen = vertices.len();

    let contour_movements = bevel_vec_ring(&contour);
    let holes_movements: Vec<Vec<(f64, f64)>> = holes.iter().map(|h| bevel_vec_ring(h)).collect();

    let mut vertices_movements = contour_movements.clone();
    holes_movements.iter().for_each(|hm| vertices_movements.extend_from_slice(hm));

    let mut ctx = Ctx::default();

    let faces: Vec<[u32; 3]> = if bevel_segments == 0 {
        triangulate_shape(&contour, &holes)
    } else {
        let mut contracted_contour_vertices: Vec<(f64, f64)> = Vec::new();
        let mut expanded_hole_vertices: Vec<Vec<(f64, f64)>> = Vec::new();

        (0..bevel_segments).for_each(|b| {
            let t = f64::from(b) / f64::from(bevel_segments);
            let z = bevel_thickness * (t * std::f64::consts::FRAC_PI_2).cos();
            let bs = bevel_size * (t * std::f64::consts::FRAC_PI_2).sin() + bevel_offset;

            contour.iter().enumerate().for_each(|(i, &pt)| {
                let vert = scale_pt2(pt, contour_movements[i], bs);
                ctx.v(vert.0, vert.1, -z);
                if t == 0.0 {
                    contracted_contour_vertices.push(vert);
                }
            });

            holes.iter().enumerate().for_each(|(h, hole)| {
                let one_hole_movements = &holes_movements[h];
                let mut one_hole_vertices = Vec::new();
                hole.iter().enumerate().for_each(|(i, &pt)| {
                    let vert = scale_pt2(pt, one_hole_movements[i], bs);
                    ctx.v(vert.0, vert.1, -z);
                    if t == 0.0 {
                        one_hole_vertices.push(vert);
                    }
                });
                if t == 0.0 {
                    expanded_hole_vertices.push(one_hole_vertices);
                }
            });
        });

        triangulate_shape(&contracted_contour_vertices, &expanded_hole_vertices)
    };

    let flen = faces.len();
    let bs_full = bevel_size + bevel_offset;

    // Back facing vertices (`ExtrudeGeometry.js:458-481`).
    (0..vlen).for_each(|i| {
        let vert = if bevel_enabled {
            scale_pt2(vertices[i], vertices_movements[i], bs_full)
        } else {
            vertices[i]
        };
        ctx.v(vert.0, vert.1, 0.0);
    });

    // Stepped + front facing vertices (`ExtrudeGeometry.js:486-511`).
    (1..=steps).for_each(|s| {
        (0..vlen).for_each(|i| {
            let vert = if bevel_enabled {
                scale_pt2(vertices[i], vertices_movements[i], bs_full)
            } else {
                vertices[i]
            };
            ctx.v(vert.0, vert.1, depth / f64::from(steps) * f64::from(s));
        });
    });

    // Back bevel segment planes (`ExtrudeGeometry.js:517-557`).
    (0..bevel_segments).rev().for_each(|b| {
        let t = f64::from(b) / f64::from(bevel_segments);
        let z = bevel_thickness * (t * std::f64::consts::FRAC_PI_2).cos();
        let bs = bevel_size * (t * std::f64::consts::FRAC_PI_2).sin() + bevel_offset;

        contour.iter().enumerate().for_each(|(i, &pt)| {
            let vert = scale_pt2(pt, contour_movements[i], bs);
            ctx.v(vert.0, vert.1, depth + z);
        });

        holes.iter().enumerate().for_each(|(h, hole)| {
            let one_hole_movements = &holes_movements[h];
            hole.iter().enumerate().for_each(|(i, &pt)| {
                let vert = scale_pt2(pt, one_hole_movements[i], bs);
                ctx.v(vert.0, vert.1, depth + z);
            });
        });
    });

    build_lid_faces(&mut ctx, &faces, flen, bevel_enabled, vlen, steps, bevel_segments);
    build_side_faces(&mut ctx, contour.len(), &holes, vlen, steps, bevel_segments);

    let pos: Vec<f32> = ctx.out_pos.iter().flat_map(|&(x, y, z)| [x as f32, y as f32, z as f32]).collect();
    let uv: Vec<f32> = ctx.out_uv.iter().flat_map(|&(u, v)| [u as f32, v as f32]).collect();
    (pos, uv)
}

/// Top/bottom cap faces (`buildLidFaces`, `ExtrudeGeometry.js:572-626`).
fn build_lid_faces(
    ctx: &mut Ctx,
    faces: &[[u32; 3]],
    flen: usize,
    bevel_enabled: bool,
    vlen: usize,
    steps: u32,
    bevel_segments: u32,
) {
    if bevel_enabled {
        let offset = 0usize;
        (0..flen).for_each(|i| {
            let f = faces[i];
            ctx.f3(f[2] as usize + offset, f[1] as usize + offset, f[0] as usize + offset);
        });

        let layer = steps as usize + bevel_segments as usize * 2;
        let offset = vlen * layer;
        (0..flen).for_each(|i| {
            let f = faces[i];
            ctx.f3(f[0] as usize + offset, f[1] as usize + offset, f[2] as usize + offset);
        });
    } else {
        (0..flen).for_each(|i| {
            let f = faces[i];
            ctx.f3(f[2] as usize, f[1] as usize, f[0] as usize);
        });
        let offset = vlen * steps as usize;
        (0..flen).for_each(|i| {
            let f = faces[i];
            ctx.f3(f[0] as usize + offset, f[1] as usize + offset, f[2] as usize + offset);
        });
    }
}

/// Side-wall faces (`buildSideFaces`/`sidewalls`, `ExtrudeGeometry.js:630-681`).
fn build_side_faces(
    ctx: &mut Ctx,
    contour_len: usize,
    holes: &[Vec<(f64, f64)>],
    vlen: usize,
    steps: u32,
    bevel_segments: u32,
) {
    let mut layer_offset = 0usize;
    sidewalls(ctx, contour_len, layer_offset, vlen, steps, bevel_segments);
    layer_offset += contour_len;
    holes.iter().for_each(|h| {
        sidewalls(ctx, h.len(), layer_offset, vlen, steps, bevel_segments);
        layer_offset += h.len();
    });
}

fn sidewalls(ctx: &mut Ctx, ring_len: usize, layer_offset: usize, vlen: usize, steps: u32, bevel_segments: u32) {
    let sl = steps as usize + bevel_segments as usize * 2;
    if ring_len == 0 {
        return;
    }
    let mut i = ring_len - 1;
    loop {
        let j = i;
        let k = if i == 0 { ring_len - 1 } else { i - 1 };
        (0..sl).for_each(|s| {
            let slen1 = vlen * s;
            let slen2 = vlen * (s + 1);
            let a = layer_offset + j + slen1;
            let b = layer_offset + k + slen1;
            let c = layer_offset + k + slen2;
            let d = layer_offset + j + slen2;
            ctx.f4(a, b, c, d);
        });
        if i == 0 {
            break;
        }
        i -= 1;
    }
}

/// The `placeholder`/`verticesArray`/`uvArray` triple `addShape` closes over
/// via `v`/`f3`/`f4`/`addVertex`/`addUV` (`ExtrudeGeometry.js:683-745`).
#[derive(Default)]
struct Ctx {
    placeholder: Vec<(f64, f64, f64)>,
    out_pos: Vec<(f64, f64, f64)>,
    out_uv: Vec<(f64, f64)>,
}

impl Ctx {
    fn v(&mut self, x: f64, y: f64, z: f64) {
        self.placeholder.push((x, y, z));
    }

    fn add_vertex(&mut self, index: usize) {
        self.out_pos.push(self.placeholder[index]);
    }

    /// `f3(a, b, c)` + `WorldUVGenerator.generateTopUV`
    /// (`ExtrudeGeometry.js:692-705`, `:808-822`).
    fn f3(&mut self, a: usize, b: usize, c: usize) {
        self.add_vertex(a);
        self.add_vertex(b);
        self.add_vertex(c);
        let n = self.out_pos.len();
        let (ax, ay, _) = self.out_pos[n - 3];
        let (bx, by, _) = self.out_pos[n - 2];
        let (cx, cy, _) = self.out_pos[n - 1];
        self.out_uv.push((ax, ay));
        self.out_uv.push((bx, by));
        self.out_uv.push((cx, cy));
    }

    /// `f4(a, b, c, d)` + `WorldUVGenerator.generateSideWallUV`
    /// (`ExtrudeGeometry.js:707-729`, `:825-860`).
    fn f4(&mut self, a: usize, b: usize, c: usize, d: usize) {
        self.add_vertex(a);
        self.add_vertex(b);
        self.add_vertex(d);
        self.add_vertex(b);
        self.add_vertex(c);
        self.add_vertex(d);

        let n = self.out_pos.len();
        let (ax, ay, az) = self.out_pos[n - 6];
        let (bx, by, bz) = self.out_pos[n - 3];
        let (cx, cy, cz) = self.out_pos[n - 2];
        let (dx, dy, dz) = self.out_pos[n - 1];

        let quad: [(f64, f64); 4] = if (ay - by).abs() < (ax - bx).abs() {
            [(ax, 1.0 - az), (bx, 1.0 - bz), (cx, 1.0 - cz), (dx, 1.0 - dz)]
        } else {
            [(ay, 1.0 - az), (by, 1.0 - bz), (cy, 1.0 - cz), (dy, 1.0 - dz)]
        };

        self.out_uv.push(quad[0]);
        self.out_uv.push(quad[1]);
        self.out_uv.push(quad[3]);
        self.out_uv.push(quad[1]);
        self.out_uv.push(quad[2]);
        self.out_uv.push(quad[3]);
    }
}

fn scale_pt2(pt: (f64, f64), v: (f64, f64), size: f64) -> (f64, f64) {
    (pt.0 + v.0 * size, pt.1 + v.1 * size)
}

/// `getBevelVec(inPt, inPrev, inNext)` (`ExtrudeGeometry.js:234-355`).
fn get_bevel_vec(in_pt: (f64, f64), in_prev: (f64, f64), in_next: (f64, f64)) -> (f64, f64) {
    let v_prev = (in_pt.0 - in_prev.0, in_pt.1 - in_prev.1);
    let v_next = (in_next.0 - in_pt.0, in_next.1 - in_pt.1);
    let v_prev_lensq = v_prev.0 * v_prev.0 + v_prev.1 * v_prev.1;
    let collinear0 = v_prev.0 * v_next.1 - v_prev.1 * v_next.0;

    if collinear0.abs() > f64::EPSILON {
        let v_prev_len = v_prev_lensq.sqrt();
        let v_next_len = (v_next.0 * v_next.0 + v_next.1 * v_next.1).sqrt();

        let pt_prev_shift = (in_prev.0 - v_prev.1 / v_prev_len, in_prev.1 + v_prev.0 / v_prev_len);
        let pt_next_shift = (in_next.0 - v_next.1 / v_next_len, in_next.1 + v_next.0 / v_next_len);

        let sf = ((pt_next_shift.0 - pt_prev_shift.0) * v_next.1 - (pt_next_shift.1 - pt_prev_shift.1) * v_next.0)
            / (v_prev.0 * v_next.1 - v_prev.1 * v_next.0);

        let v_trans = (
            pt_prev_shift.0 + v_prev.0 * sf - in_pt.0,
            pt_prev_shift.1 + v_prev.1 * sf - in_pt.1,
        );
        let v_trans_lensq = v_trans.0 * v_trans.0 + v_trans.1 * v_trans.1;
        if v_trans_lensq <= 2.0 {
            return v_trans;
        }
        let shrink_by = (v_trans_lensq / 2.0).sqrt();
        (v_trans.0 / shrink_by, v_trans.1 / shrink_by)
    } else {
        let direction_eq = if v_prev.0 > f64::EPSILON {
            v_next.0 > f64::EPSILON
        } else if v_prev.0 < -f64::EPSILON {
            v_next.0 < -f64::EPSILON
        } else {
            math_sign(v_prev.1) == math_sign(v_next.1)
        };
        let (v_trans, shrink_by) = if direction_eq {
            ((-v_prev.1, v_prev.0), v_prev_lensq.sqrt())
        } else {
            (v_prev, (v_prev_lensq / 2.0).sqrt())
        };
        (v_trans.0 / shrink_by, v_trans.1 / shrink_by)
    }
}

fn bevel_vec_ring(ring: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let j = (i + n - 1) % n;
            let k = (i + 1) % n;
            get_bevel_vec(ring[i], ring[j], ring[k])
        })
        .collect()
}

fn math_sign(x: f64) -> i32 {
    let positive = i32::from(x > 0.0);
    let negative = i32::from(x < 0.0);
    positive - negative
}

/// `ShapeUtils.area`/`isClockWise` (`ShapeUtils.js:16-41`).
fn area2(pts: &[(f64, f64)]) -> f64 {
    let n = pts.len();
    let mut a = 0.0;
    (0..n).for_each(|q| {
        let p = if q == 0 { n - 1 } else { q - 1 };
        a += pts[p].0 * pts[q].1 - pts[q].0 * pts[p].1;
    });
    a * 0.5
}

fn is_clockwise(pts: &[(f64, f64)]) -> bool {
    area2(pts) < 0.0
}

/// `Path.closePath()` (`CurvePath.js:56-71`): append a copy of the first
/// point if the ring isn't already closed. `getPoints()` on a straight-line
/// path always samples this closing `LineCurve`'s two endpoints, so the
/// duplicate always survives into `shapePoints.shape`/`holesPts[i]`
/// (`ExtrudeGeometry.js:138`) for `extrude()`'s never-pre-closed inputs.
fn close_ring(pts: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut out = pts.to_vec();
    let already_closed = matches!((pts.first(), pts.last()), (Some(a), Some(b)) if a == b);
    if !pts.is_empty() && !already_closed {
        out.push(pts[0]);
    }
    out
}

/// `mergeOverlappingPoints(points)` (`ExtrudeGeometry.js:165-200`).
fn merge_overlapping_points(points: &mut Vec<(f64, f64)>) {
    const THRESHOLD: f64 = 1e-10;
    const THRESHOLD_SQ: f64 = THRESHOLD * THRESHOLD;
    if points.is_empty() {
        return;
    }
    let mut prev_pos = points[0];
    let mut i: usize = 1;
    while !points.is_empty() && i <= points.len() {
        let current_index = i % points.len();
        let current_pos = points[current_index];
        let dx = current_pos.0 - prev_pos.0;
        let dy = current_pos.1 - prev_pos.1;
        let dist_sq = dx * dx + dy * dy;
        let scaling = current_pos
            .0
            .abs()
            .max(current_pos.1.abs())
            .max(prev_pos.0.abs())
            .max(prev_pos.1.abs());
        let threshold_sq_scaled = THRESHOLD_SQ * scaling * scaling;
        if dist_sq <= threshold_sq_scaled {
            points.remove(current_index);
            // `i--; continue;` in the source, where `continue` still runs
            // the `for` loop's `i++` — net effect: `i` is unchanged.
        } else {
            prev_pos = current_pos;
            i += 1;
        }
    }
}

/// `removeDupEndPts(points)` (`ShapeUtils.js:91-101`).
fn remove_dup_end_pts(points: &mut Vec<(f64, f64)>) {
    let l = points.len();
    if l > 2 && points[l - 1] == points[0] {
        points.pop();
    }
}

/// `ShapeUtils.triangulateShape(contour, holes)` (`ShapeUtils.js:50-87`).
fn triangulate_shape(contour: &[(f64, f64)], holes: &[Vec<(f64, f64)>]) -> Vec<[u32; 3]> {
    let mut contour = contour.to_vec();
    remove_dup_end_pts(&mut contour);

    let mut holes: Vec<Vec<(f64, f64)>> = holes.to_vec();
    holes.iter_mut().for_each(|h| remove_dup_end_pts(h));

    let mut combined: Vec<(f64, f64)> = contour.clone();
    let mut hole_indices: Vec<usize> = Vec::with_capacity(holes.len());
    let mut hole_index = contour.len();
    holes.iter().for_each(|h| {
        hole_indices.push(hole_index);
        hole_index += h.len();
        combined.extend_from_slice(h);
    });

    let triangles = earcut::earcut(&combined, &hole_indices);
    triangles.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect()
}

/// `mergeVertices(geometry, 1e-6)` (`BufferGeometryUtils.js:644-800`),
/// specialized to `tolerance = 1e-6` and `Geo`'s fixed position/normal/uv
/// attribute order — a duplicate of `merge::merge_vertices` (private to that
/// module) kept local because `extrude()` welds inline
/// (`geometry.js:182`), independent of `mergeAll`. Same algorithm, same
/// tolerance, two call sites, as in the source.
fn weld_vertices(g: &Geo) -> Geo {
    const TOLERANCE: f64 = 1e-6;
    let half_tolerance = TOLERANCE * 0.5;
    let hash_multiplier = 10f64.powf((1.0 / TOLERANCE).log10());
    let hash_additive = half_tolerance * hash_multiplier;

    let mut hash_to_index: std::collections::HashMap<[i64; 8], u32> = std::collections::HashMap::new();
    let mut new_pos = Vec::new();
    let mut new_normal = Vec::new();
    let mut new_uv = Vec::new();
    let mut new_index = Vec::with_capacity(g.vert_count());
    let mut next_index: u32 = 0;

    (0..g.vert_count()).for_each(|i| {
        let components = [
            g.pos[i * 3],
            g.pos[i * 3 + 1],
            g.pos[i * 3 + 2],
            g.normal[i * 3],
            g.normal[i * 3 + 1],
            g.normal[i * 3 + 2],
            g.uv[i * 2],
            g.uv[i * 2 + 1],
        ];
        let hash: [i64; 8] = components.map(|v| (f64::from(v) * hash_multiplier + hash_additive).trunc() as i64);

        let existing = hash_to_index.get(&hash).copied();
        match existing {
            Some(idx) => new_index.push(idx),
            None => {
                new_pos.extend_from_slice(&g.pos[i * 3..i * 3 + 3]);
                new_normal.extend_from_slice(&g.normal[i * 3..i * 3 + 3]);
                new_uv.extend_from_slice(&g.uv[i * 2..i * 2 + 2]);
                hash_to_index.insert(hash, next_index);
                new_index.push(next_index);
                next_index += 1;
            }
        }
    });

    Geo {
        pos: new_pos,
        normal: new_normal,
        uv: new_uv,
        index: new_index,
    }
}

