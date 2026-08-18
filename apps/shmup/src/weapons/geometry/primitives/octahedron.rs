//! Ported from `THREE.OctahedronGeometry`/`PolyhedronGeometry`
//! (`three/src/geometries/OctahedronGeometry.js`,
//! `three/src/geometries/PolyhedronGeometry.js`, MIT licensed, Three.js
//! authors), specialized to `detail = 0` — the only value `knurlBand`'s cell
//! ever uses (`geometry.js:251`, `new THREE.OctahedronGeometry(depth*2.2,
//! 0)`).
//!
//! At `detail = 0`, `PolyhedronGeometry`'s generic `subdivideFace` collapses
//! to the identity: each of the octahedron's 8 base triangular faces
//! `(a, b, c)` becomes exactly one output triangle, in the *cyclic
//! rotation* `(b, c, a)` — traced from `subdivideFace(a, b, c, 0)`
//! (`PolyhedronGeometry.js:102-163`) with `cols = 1`: the single row's two
//! `v[0]` samples land on `a` and `b`, and `v[1][0]` lands on `c`; the one
//! `pushVertex` triple that follows pushes `v[0][1]`, `v[1][0]`, `v[0][0]` —
//! i.e. `b, c, a`. That collapse is why this file skips porting the general
//! subdivision — it would only ever run once, producing this fixed result.

use super::super::Geo;

const OCTA_VERTS: [[f64; 3]; 6] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
];

const OCTA_FACES: [[usize; 3]; 8] = [
    [0, 2, 4],
    [0, 4, 3],
    [0, 3, 5],
    [0, 5, 2],
    [1, 2, 5],
    [1, 5, 3],
    [1, 3, 4],
    [1, 4, 2],
];

/// `new THREE.OctahedronGeometry(radius, 0)`.
pub(super) fn octahedron_detail0(radius: f64) -> Geo {
    // `subdivide(0)`, per the module doc: each face `(a, b, c)` -> `(b, c, a)`.
    let tri_verts: Vec<[f64; 3]> = OCTA_FACES
        .iter()
        .flat_map(|&[a, b, c]| [OCTA_VERTS[b], OCTA_VERTS[c], OCTA_VERTS[a]])
        .collect();

    // `applyRadius(radius)`: the base vertices are already unit length, so
    // `.normalize()` is a no-op; only the scale by `radius` has an effect.
    let scaled: Vec<[f64; 3]> = tri_verts.iter().map(|v| [v[0] * radius, v[1] * radius, v[2] * radius]).collect();

    // `generateUVs()`, the raw azimuth/inclination pass (`PolyhedronGeometry.js:187-207`).
    let mut uv: Vec<(f64, f64)> = scaled
        .iter()
        .map(|v| {
            let u = azimuth(v[0], v[2]) / (2.0 * std::f64::consts::PI) + 0.5;
            let vv = inclination(v[0], v[1], v[2]) / std::f64::consts::PI + 0.5;
            (u, 1.0 - vv)
        })
        .collect();

    // `correctUVs()` (`PolyhedronGeometry.js:254-286`) — runs before
    // `correctSeam()` in the source's `generateUVs`.
    (0..8).for_each(|f| {
        let base = f * 3;
        let (a, b, c) = (scaled[base], scaled[base + 1], scaled[base + 2]);
        let centroid = [
            (a[0] + b[0] + c[0]) / 3.0,
            (a[1] + b[1] + c[1]) / 3.0,
            (a[2] + b[2] + c[2]) / 3.0,
        ];
        let azi = azimuth(centroid[0], centroid[2]);
        (0..3).for_each(|k| {
            let idx = base + k;
            correct_uv(&mut uv, idx, scaled[idx], azi);
        });
    });

    // `correctSeam()` (`PolyhedronGeometry.js:209-236`).
    (0..8).for_each(|f| {
        let base = f * 3;
        let (x0, x1, x2) = (uv[base].0, uv[base + 1].0, uv[base + 2].0);
        let max = x0.max(x1).max(x2);
        let min = x0.min(x1).min(x2);
        if max > 0.9 && min < 0.1 {
            if x0 < 0.2 {
                uv[base].0 += 1.0;
            }
            if x1 < 0.2 {
                uv[base + 1].0 += 1.0;
            }
            if x2 < 0.2 {
                uv[base + 2].0 += 1.0;
            }
        }
    });

    let pos: Vec<f32> = scaled.iter().flat_map(|v| [v[0] as f32, v[1] as f32, v[2] as f32]).collect();
    let uv_flat: Vec<f32> = uv.iter().flat_map(|&(u, v)| [u as f32, v as f32]).collect();

    let mut g = Geo {
        pos,
        normal: Vec::new(),
        uv: uv_flat,
        index: Vec::new(),
    };
    // `detail === 0` always takes `this.computeVertexNormals()` (flat
    // per-triangle normals), never `this.normalizeNormals()`
    // (`PolyhedronGeometry.js:66-74`).
    g.normalize_attributes();
    g
}

/// Angle around the Y axis, counter-clockwise when looking from above
/// (`PolyhedronGeometry.js:306-309`).
fn azimuth(x: f64, z: f64) -> f64 {
    z.atan2(-x)
}

/// Angle above the XZ plane (`PolyhedronGeometry.js:315-318`).
fn inclination(x: f64, y: f64, z: f64) -> f64 {
    (-y).atan2((x * x + z * z).sqrt())
}

fn correct_uv(uv: &mut [(f64, f64)], idx: usize, v: [f64; 3], azimuth: f64) {
    if azimuth < 0.0 && uv[idx].0 == 1.0 {
        uv[idx].0 -= 1.0;
    }
    if v[0] == 0.0 && v[2] == 0.0 {
        uv[idx].0 = azimuth / 2.0 / std::f64::consts::PI + 0.5;
    }
}
