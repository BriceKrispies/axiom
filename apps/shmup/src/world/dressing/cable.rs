//! Ported from Claude-of-Duty `src/world/util.js:989-1012` — `catenaryTube`,
//! plus the two Three.js classes it is built out of:
//! `THREE.CatmullRomCurve3` (`three/src/extras/curves/CatmullRomCurve3.js`)
//! and `THREE.TubeGeometry` (`three/src/geometries/TubeGeometry.js`), with
//! the `Curve` base-class machinery they depend on
//! (`three/src/extras/core/Curve.js`: `getLengths`, `getUtoTmapping`,
//! `getPointAt`, `getTangent`, `getTangentAt`, `computeFrenetFrames`) —
//! all MIT licensed, Three.js authors.
//!
//! See [`super::berm`]'s module doc for why a generic `util.js` primitive is
//! sitting in the dressing directory rather than in
//! `crate::world::kit::primitives`.
//!
//! **Everything here is computed in `f64`** — the Three.js originals are JS
//! numbers throughout, and only `TubeGeometry`'s final
//! `Float32BufferAttribute` narrows to `f32`. The arc-length
//! reparameterisation (`getUtoTmapping`) does a binary search over a
//! 201-entry cumulative-length table and then a linear interpolation inside
//! one cell; narrowing anywhere upstream of that would move the sample
//! points, not just round them.

use crate::world::geo::WorldGeo;
use crate::world::noise::fbm3;

/// `Curve.arcLengthDivisions` (`Curve.js:37`).
const ARC_LENGTH_DIVISIONS: usize = 200;

/// `Curve.getTangent`'s finite-difference step (`Curve.js:293`).
const TANGENT_DELTA: f64 = 0.0001;

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: V3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// `Vector3.normalize()` = `divideScalar(this.length() || 1)`: a zero-length
/// vector stays zero rather than becoming `NaN`.
fn normalize(a: V3) -> V3 {
    let l = length(a);
    let d = if l == 0.0 { 1.0 } else { l };
    [a[0] / d, a[1] / d, a[2] / d]
}

fn distance_to_squared(a: V3, b: V3) -> f64 {
    let (dx, dy, dz) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
    // `Vector3.distanceToSquared` — a plain sum of squares, NOT `hypot`.
    dx * dx + dy * dy + dz * dz
}

fn distance_to(a: V3, b: V3) -> f64 {
    distance_to_squared(a, b).sqrt()
}

/// `MathUtils.clamp(value, min, max)`.
fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    v.max(lo).min(hi)
}

// ======================================================== CatmullRomCurve3 ==
/// `CubicPoly` (`CatmullRomCurve3.js:3-76`).
#[derive(Default, Clone, Copy)]
struct CubicPoly {
    c0: f64,
    c1: f64,
    c2: f64,
    c3: f64,
}

impl CubicPoly {
    /// `init(x0, x1, t0, t1)` (`CatmullRomCurve3.js:35-42`).
    fn init(&mut self, x0: f64, x1: f64, t0: f64, t1: f64) {
        self.c0 = x0;
        self.c1 = t0;
        self.c2 = -3.0 * x0 + 3.0 * x1 - 2.0 * t0 - t1;
        self.c3 = 2.0 * x0 - 2.0 * x1 + t0 + t1;
    }

    /// `initNonuniformCatmullRom(x0, x1, x2, x3, dt0, dt1, dt2)`
    /// (`CatmullRomCurve3.js:52-65`). The grouping of the two tangent
    /// expressions is transcribed literally — reassociating them changes the
    /// last bits.
    fn init_nonuniform_catmull_rom(&mut self, x0: f64, x1: f64, x2: f64, x3: f64, dt0: f64, dt1: f64, dt2: f64) {
        let mut t1 = (x1 - x0) / dt0 - (x2 - x0) / (dt0 + dt1) + (x2 - x1) / dt1;
        let mut t2 = (x2 - x1) / dt1 - (x3 - x1) / (dt1 + dt2) + (x3 - x2) / dt2;
        t1 *= dt1;
        t2 *= dt1;
        self.init(x1, x2, t1, t2);
    }

    /// `calc(t)` (`CatmullRomCurve3.js:67-73`).
    fn calc(&self, t: f64) -> f64 {
        let t2 = t * t;
        let t3 = t2 * t;
        self.c0 + self.c1 * t + self.c2 * t2 + self.c3 * t3
    }
}

/// `new THREE.CatmullRomCurve3(points)` — `closed = false`,
/// `curveType = 'centripetal'`, `tension = 0.5` (the constructor defaults,
/// which is what `catenaryTube` uses: it passes only `points`).
struct CatmullRomCurve3 {
    points: Vec<V3>,
    /// `this.cacheArcLengths` (`Curve.js:158-168`), built lazily by
    /// `get_lengths` on first use exactly as the JS does.
    cache_arc_lengths: Option<Vec<f64>>,
}

impl CatmullRomCurve3 {
    fn new(points: Vec<V3>) -> Self {
        CatmullRomCurve3 { points, cache_arc_lengths: None }
    }

    /// `getPoint(t)` (`CatmullRomCurve3.js:166-256`), `closed = false`,
    /// `curveType = 'centripetal'`.
    fn get_point(&self, t: f64) -> V3 {
        let points = &self.points;
        let l = points.len();

        let p = (l - 1) as f64 * t;
        let mut int_point = p.floor();
        let mut weight = p - int_point;

        if weight == 0.0 && int_point == (l - 1) as f64 {
            int_point = (l - 2) as f64;
            weight = 1.0;
        }
        let ip = int_point as usize;

        // extrapolate first point: `(points[0] - points[1]) + points[0]`
        let p0 = if ip > 0 { points[(ip - 1) % l] } else { add(sub(points[0], points[1]), points[0]) };
        let p1 = points[ip % l];
        let p2 = points[(ip + 1) % l];
        // extrapolate last point: `(points[l-1] - points[l-2]) + points[l-1]`
        let p3 = if ip + 2 < l { points[(ip + 2) % l] } else { add(sub(points[l - 1], points[l - 2]), points[l - 1]) };

        // centripetal: pow = 0.25
        let pow = 0.25;
        let mut dt0 = distance_to_squared(p0, p1).powf(pow);
        let mut dt1 = distance_to_squared(p1, p2).powf(pow);
        let mut dt2 = distance_to_squared(p2, p3).powf(pow);
        // safety check for repeated points
        if dt1 < 1e-4 {
            dt1 = 1.0;
        }
        if dt0 < 1e-4 {
            dt0 = dt1;
        }
        if dt2 < 1e-4 {
            dt2 = dt1;
        }

        let mut px = CubicPoly::default();
        let mut py = CubicPoly::default();
        let mut pz = CubicPoly::default();
        px.init_nonuniform_catmull_rom(p0[0], p1[0], p2[0], p3[0], dt0, dt1, dt2);
        py.init_nonuniform_catmull_rom(p0[1], p1[1], p2[1], p3[1], dt0, dt1, dt2);
        pz.init_nonuniform_catmull_rom(p0[2], p1[2], p2[2], p3[2], dt0, dt1, dt2);

        [px.calc(weight), py.calc(weight), pz.calc(weight)]
    }

    /// `getLengths(divisions = this.arcLengthDivisions)` (`Curve.js:152-169`).
    fn get_lengths(&mut self) -> &Vec<f64> {
        if self.cache_arc_lengths.is_none() {
            let divisions = ARC_LENGTH_DIVISIONS;
            let mut cache = Vec::with_capacity(divisions + 1);
            let mut last = self.get_point(0.0);
            let mut sum = 0.0;
            cache.push(0.0);
            for p in 1..=divisions {
                let current = self.get_point(p as f64 / divisions as f64);
                sum += distance_to(current, last);
                cache.push(sum);
                last = current;
            }
            self.cache_arc_lengths = Some(cache);
        }
        self.cache_arc_lengths.as_ref().expect("just populated")
    }

    /// `getUtoTmapping(u)` (`Curve.js:207-262`), `distance = null`.
    fn u_to_t_mapping(&mut self, u: f64) -> f64 {
        let arc_lengths = self.get_lengths().clone();
        let il = arc_lengths.len();
        let target_arc_length = u * arc_lengths[il - 1];

        // binary search for the index with largest value smaller than the
        // target distance
        let mut low: i64 = 0;
        let mut high: i64 = il as i64 - 1;
        let mut i: i64 = 0;
        while low <= high {
            i = low + (high - low) / 2;
            let comparison = arc_lengths[i as usize] - target_arc_length;
            if comparison < 0.0 {
                low = i + 1;
            } else if comparison > 0.0 {
                high = i - 1;
            } else {
                high = i;
                break;
            }
        }
        let _ = i;
        let idx = high;
        // `arcLengths[-1]` is `undefined` in JS and `undefined === x` is
        // false for any number, so a negative index falls through to the
        // interpolation below — where `arcLengths[-1]` is `undefined` and the
        // arithmetic yields `NaN`. That never happens for a real curve
        // (`arcLengths[0] === 0` and `targetArcLength >= 0`, so the search
        // always leaves `high >= 0` unless `u < 0`), and this port asserts
        // rather than silently producing `NaN`.
        assert!(idx >= 0, "catenary_tube: u_to_t_mapping called with u < 0");
        let i = idx as usize;

        if arc_lengths[i] == target_arc_length {
            return i as f64 / (il - 1) as f64;
        }

        let length_before = arc_lengths[i];
        let length_after = arc_lengths[i + 1];
        let segment_length = length_after - length_before;
        let segment_fraction = (target_arc_length - length_before) / segment_length;
        (i as f64 + segment_fraction) / (il - 1) as f64
    }

    /// `getPointAt(u)` (`Curve.js:82-86`).
    fn get_point_at(&mut self, u: f64) -> V3 {
        let t = self.u_to_t_mapping(u);
        self.get_point(t)
    }

    /// `getTangent(t)` (`Curve.js:292-306`) — the numeric-differentiation
    /// fallback, which `CatmullRomCurve3` does not override.
    fn get_tangent(&self, t: f64) -> V3 {
        let mut t1 = t - TANGENT_DELTA;
        let mut t2 = t + TANGENT_DELTA;
        if t1 < 0.0 {
            t1 = 0.0;
        }
        if t2 > 1.0 {
            t2 = 1.0;
        }
        let pt1 = self.get_point(t1);
        let pt2 = self.get_point(t2);
        normalize(sub(pt2, pt1))
    }

    /// `getTangentAt(u)` (`Curve.js:322-326`).
    fn get_tangent_at(&mut self, u: f64) -> V3 {
        let t = self.u_to_t_mapping(u);
        self.get_tangent(t)
    }

    /// `computeFrenetFrames(segments, closed = false)` (`Curve.js:337-...`),
    /// specialised to `closed = false` — `catenaryTube` never builds a closed
    /// tube, so the closed-curve post-pass is not ported (it would be dead
    /// code with no exerciser, the same call the rest of this port makes for
    /// `rock_geometry`'s unused subdivision arm).
    fn compute_frenet_frames(&mut self, segments: usize) -> (Vec<V3>, Vec<V3>) {
        let tangents: Vec<V3> = (0..=segments).map(|i| self.get_tangent_at(i as f64 / segments as f64)).collect();

        // Initial normal perpendicular to the first tangent, in the direction
        // of that tangent's minimum component. Note the source's `<=`
        // comparisons and the fall-through `if (tz <= min)` with no `min`
        // update — transcribed exactly.
        let mut normal: V3 = [0.0, 0.0, 0.0];
        let mut min = f64::MAX;
        let tx = tangents[0][0].abs();
        let ty = tangents[0][1].abs();
        let tz = tangents[0][2].abs();
        if tx <= min {
            min = tx;
            normal = [1.0, 0.0, 0.0];
        }
        if ty <= min {
            min = ty;
            normal = [0.0, 1.0, 0.0];
        }
        if tz <= min {
            normal = [0.0, 0.0, 1.0];
        }

        let mut normals: Vec<V3> = vec![[0.0; 3]; segments + 1];
        let mut binormals: Vec<V3> = vec![[0.0; 3]; segments + 1];
        let vec0 = normalize(cross(tangents[0], normal));
        normals[0] = cross(tangents[0], vec0);
        binormals[0] = cross(tangents[0], normals[0]);

        for i in 1..=segments {
            normals[i] = normals[i - 1];
            binormals[i] = binormals[i - 1];
            let v = cross(tangents[i - 1], tangents[i]);
            if length(v) > f64::EPSILON {
                let axis = normalize(v);
                let theta = clamp(dot(tangents[i - 1], tangents[i]), -1.0, 1.0).acos();
                normals[i] = apply_rotation_axis(normals[i], axis, theta);
            }
            binormals[i] = cross(tangents[i], normals[i]);
        }

        (normals, binormals)
    }
}

/// `Vector3.applyMatrix4(new Matrix4().makeRotationAxis(axis, angle))`
/// (`Matrix4.js:920-941` + `Vector3.applyMatrix4`). `makeRotationAxis`
/// produces a pure rotation, so the perspective divide is exactly `1` and
/// only the upper-left 3x3 matters; the element grouping below matches
/// `Matrix4.set`'s row-major argument order fed through `applyMatrix4`'s
/// `e[0]*x + e[4]*y + e[8]*z` column reads.
fn apply_rotation_axis(v: V3, axis: V3, angle: f64) -> V3 {
    let c = angle.cos();
    let s = angle.sin();
    let t = 1.0 - c;
    let (x, y, z) = (axis[0], axis[1], axis[2]);
    let tx = t * x;
    let ty = t * y;
    // Row 0 / row 1 / row 2 of `makeRotationAxis`.
    let r = [
        [tx * x + c, tx * y - s * z, tx * z + s * y],
        [tx * y + s * z, ty * y + c, ty * z - s * x],
        [tx * z - s * y, ty * z + s * x, t * z * z + c],
    ];
    [
        r[0][0] * v[0] + r[0][1] * v[1] + r[0][2] * v[2],
        r[1][0] * v[0] + r[1][1] * v[1] + r[1][2] * v[2],
        r[2][0] * v[0] + r[2][1] * v[1] + r[2][2] * v[2],
    ]
}

// ============================================================ TubeGeometry ==
/// `new THREE.TubeGeometry(path, tubularSegments, radius, radialSegments,
/// closed = false)` (`TubeGeometry.js:60-190`). No `color` attribute —
/// matching the source, and `Accum::add` treats a missing one as `[0, 0, 0]`.
fn tube_geometry(path: &mut CatmullRomCurve3, tubular_segments: usize, radius: f64, radial_segments: usize) -> WorldGeo {
    let (frame_normals, frame_binormals) = path.compute_frenet_frames(tubular_segments);

    let mut vertices: Vec<f32> = Vec::new();
    let mut normals: Vec<f32> = Vec::new();
    let mut uvs: Vec<f32> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    // `generateSegment(i)` (`TubeGeometry.js:112-140`).
    #[allow(clippy::too_many_arguments)]
    fn generate_segment(
        i: usize,
        path: &mut CatmullRomCurve3,
        frame_normals: &[V3],
        frame_binormals: &[V3],
        tubular_segments: usize,
        radial_segments: usize,
        radius: f64,
        vertices: &mut Vec<f32>,
        normals: &mut Vec<f32>,
    ) {
        let p = path.get_point_at(i as f64 / tubular_segments as f64);
        let n = frame_normals[i];
        let b = frame_binormals[i];
        for j in 0..=radial_segments {
            let v = j as f64 / radial_segments as f64 * std::f64::consts::PI * 2.0;
            let sin = v.sin();
            let cos = -v.cos();
            let normal = normalize([cos * n[0] + sin * b[0], cos * n[1] + sin * b[1], cos * n[2] + sin * b[2]]);
            normals.push(normal[0] as f32);
            normals.push(normal[1] as f32);
            normals.push(normal[2] as f32);
            vertices.push((p[0] + radius * normal[0]) as f32);
            vertices.push((p[1] + radius * normal[1]) as f32);
            vertices.push((p[2] + radius * normal[2]) as f32);
        }
    }

    for i in 0..tubular_segments {
        generate_segment(i, path, &frame_normals, &frame_binormals, tubular_segments, radial_segments, radius, &mut vertices, &mut normals);
    }
    // not closed: the last row sits at the regular position on the path
    generate_segment(
        tubular_segments,
        path,
        &frame_normals,
        &frame_binormals,
        tubular_segments,
        radial_segments,
        radius,
        &mut vertices,
        &mut normals,
    );

    // `generateUVs`
    for i in 0..=tubular_segments {
        for j in 0..=radial_segments {
            uvs.push((i as f64 / tubular_segments as f64) as f32);
            uvs.push((j as f64 / radial_segments as f64) as f32);
        }
    }

    // `generateIndices`
    for j in 1..=tubular_segments {
        for i in 1..=radial_segments {
            let a = ((radial_segments + 1) * (j - 1) + (i - 1)) as u32;
            let b = ((radial_segments + 1) * j + (i - 1)) as u32;
            let c = ((radial_segments + 1) * j + i) as u32;
            let d = ((radial_segments + 1) * (j - 1) + i) as u32;
            indices.extend_from_slice(&[a, b, d, b, c, d]);
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

// ============================================================ catenaryTube ==
/// `catenaryTube`'s `opts` (`util.js:991`). Defaults: `seg=12`, `radial=4`,
/// `jitter=0`.
#[derive(Debug, Clone, Copy)]
pub struct CatenaryOpts {
    pub seg: usize,
    pub radial: usize,
    pub jitter: f64,
}

impl Default for CatenaryOpts {
    fn default() -> Self {
        CatenaryOpts { seg: 12, radial: 4, jitter: 0.0 }
    }
}

/// `catenaryTube(from, to, sagAmt, radius, opts = {})` (`util.js:991-1012`):
/// a sagging cable / rope / wire between two points, as a thin tube.
///
/// Draws nothing from any `rng` — the jitter comes from `fbm3` keyed on the
/// sample index. Note the source's `jitter ? … : 0` guard: a zero jitter
/// skips the `fbm3` calls entirely rather than multiplying their result by
/// zero, which matters because `fbm3(…) - 0.5` is not zero.
pub fn catenary_tube(from: [f64; 3], to: [f64; 3], sag_amt: f64, radius: f64, opts: CatenaryOpts) -> WorldGeo {
    let seg = opts.seg;
    let jitter = opts.jitter;
    let mut pts: Vec<V3> = Vec::with_capacity(seg + 1);
    let k = 1.5f64.cosh() - 1.0;
    for i in 0..=seg {
        let t = i as f64 / seg as f64;
        // normalised catenary droop: 0 at the ends, 1 at mid-span
        let droop = (1.5f64.cosh() - ((t - 0.5) * 3.0).cosh()) / k;
        let jx = if jitter != 0.0 { (fbm3(i as f64 * 3.1, 1.2, 4.4, 2) - 0.5) * jitter } else { 0.0 };
        let jz = if jitter != 0.0 { (fbm3(i as f64 * 2.7, 8.2, 1.4, 2) - 0.5) * jitter } else { 0.0 };
        pts.push([
            from[0] + (to[0] - from[0]) * t + jx,
            from[1] + (to[1] - from[1]) * t - sag_amt * droop,
            from[2] + (to[2] - from[2]) * t + jz,
        ]);
    }
    let mut curve = CatmullRomCurve3::new(pts);
    tube_geometry(&mut curve, seg, radius, opts.radial)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catenary_tube_has_the_tube_grid_topology() {
        let g = catenary_tube([0.0, 3.0, 0.0], [4.0, 3.0, 0.0], 0.5, 0.02, CatenaryOpts { seg: 6, radial: 4, jitter: 0.0 });
        // (seg + 1) rows of (radial + 1) vertices.
        assert_eq!(g.vert_count(), 7 * 5);
        // seg * radial quads, 2 triangles each.
        assert_eq!(g.tri_count(), 6 * 4 * 2);
        assert_eq!(g.uv.len(), 7 * 5 * 2);
    }

    #[test]
    fn catenary_tube_sags_below_the_chord_at_mid_span() {
        let g = catenary_tube([0.0, 3.0, 0.0], [4.0, 3.0, 0.0], 0.5, 0.01, CatenaryOpts::default());
        let lowest = g.pos.iter().skip(1).step_by(3).copied().fold(f32::INFINITY, f32::min);
        assert!(lowest < 2.6, "mid-span should droop by ~sag: {lowest}");
    }

    #[test]
    fn catenary_tube_endpoints_sit_on_the_given_points() {
        let g = catenary_tube([-1.0, 2.0, 5.0], [3.0, 2.5, 5.0], 0.4, 0.02, CatenaryOpts { seg: 8, radial: 4, jitter: 0.0 });
        // First ring is centred on `from`, last ring on `to` (radius 0.02
        // off-axis at most).
        assert!((g.pos[0] - -1.0).abs() < 0.05, "{}", g.pos[0]);
        let n = g.vert_count();
        assert!((g.pos[(n - 1) * 3] - 3.0).abs() < 0.05, "{}", g.pos[(n - 1) * 3]);
    }

    #[test]
    fn jitter_moves_the_control_points() {
        let a = catenary_tube([0.0, 3.0, 0.0], [4.0, 3.0, 0.0], 0.5, 0.02, CatenaryOpts { seg: 6, radial: 4, jitter: 0.0 });
        let b = catenary_tube([0.0, 3.0, 0.0], [4.0, 3.0, 0.0], 0.5, 0.02, CatenaryOpts { seg: 6, radial: 4, jitter: 0.2 });
        assert_ne!(a.pos, b.pos);
    }
}
