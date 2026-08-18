//! Ported from Claude-of-Duty `src/weapons/geometry.js` — the geometry buffer
//! (`THREE.BufferGeometry`, reduced to exactly the three attributes the kit
//! keeps: position, normal, uv) and its per-instance operations:
//! `normalizeAttributes` (`geometry.js:32-45`), `flipWinding`
//! (`geometry.js:82-100`), and the `applyMatrix4`/`getNormalMatrix` pair Three
//! runs inside `Assembly.add` (`geometry.js:387-390`,
//! `BufferGeometry.applyMatrix4`, `Matrix3.getNormalMatrix`,
//! `Vector3.applyNormalMatrix` in `three/src/core/BufferGeometry.js` and
//! `three/src/math/{Matrix3,Vector3}.js`, MIT licensed, Three.js authors).
//!
//! `vert_count`/`tri_count` port `triCount` (`geometry.js:441-444`).
//!
//! This is app code (`apps/`), outside the Branchless Law — the methods below
//! use plain `if`/`for` where that is the clearest way to say what the
//! source says.

use axiom_math::{Mat4, Vec3};

/// A flattened `THREE.BufferGeometry` carrying exactly the three attributes
/// `KEEP_ATTRS` (`geometry.js:30`) keeps: `position`, `normal`, `uv`, plus an
/// optional triangle `index` (empty when the geometry is non-indexed, the
/// state every primitive and `mergeGeometries` output can be in).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Geo {
    /// `xyz` triples, one per vertex.
    pub pos: Vec<f32>,
    /// `xyz` triples, one per vertex.
    pub normal: Vec<f32>,
    /// `uv` pairs, one per vertex.
    pub uv: Vec<f32>,
    /// Triangle indices, three per triangle. Empty means non-indexed.
    pub index: Vec<u32>,
}

impl Geo {
    /// `geo.getAttribute('position').count`.
    pub fn vert_count(&self) -> usize {
        self.pos.len() / 3
    }

    /// `triCount(geo)` (`geometry.js:441-444`): index-triple count when
    /// indexed, else position-triple count.
    pub fn tri_count(&self) -> usize {
        if self.index.is_empty() {
            self.vert_count() / 3
        } else {
            self.index.len() / 3
        }
    }

    /// `BufferGeometry.applyMatrix4(m)` (`three/src/core/BufferGeometry.js`):
    /// transforms every position as a point, and every normal by the
    /// **normal matrix** — `transpose(inverse(upperLeft3x3(m)))`, per
    /// `Matrix3.getNormalMatrix` + `Vector3.applyNormalMatrix` — not the raw
    /// matrix. A uniform-scale-only transform would tolerate the raw 3x3, but
    /// `Assembly.add`'s mirrored geometry uses a *negative, non-uniform*
    /// scale (`sx = -1`), which the raw matrix gets wrong (it would skew the
    /// normals) and the normal matrix gets right.
    ///
    /// `uv` and `index` are untouched, matching `applyMatrix4`, which never
    /// names either attribute.
    pub fn apply(&mut self, m: &Mat4) {
        for p in self.pos.chunks_exact_mut(3) {
            let v = m.transform_point(Vec3::new(p[0], p[1], p[2]));
            p[0] = v.x;
            p[1] = v.y;
            p[2] = v.z;
        }

        let normal_matrix = upper_left_3x3_normal_matrix(m);
        for n in self.normal.chunks_exact_mut(3) {
            let v = apply_3x3(&normal_matrix, [n[0], n[1], n[2]]);
            let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            // `Vector3.normalize()`: `divideScalar(length() || 1)` — a
            // zero-length vector divides by 1, staying zero, rather than
            // dividing by zero.
            let divisor = if len == 0.0 { 1.0 } else { len };
            n[0] = v[0] / divisor;
            n[1] = v[1] / divisor;
            n[2] = v[2] / divisor;
        }
    }

    /// `flipWinding(geo)` (`geometry.js:82-100`): reverses each triangle's
    /// index order (first/last swap) and negates every normal. Ported
    /// exactly, including the source's shape — the index swap is a no-op
    /// when `index` is empty (mirroring the source's `if (idx)` guard, which
    /// in practice is never taken: every geometry this kit produces reaches
    /// `Assembly.add` already indexed, either from a Three geometry
    /// constructor or from `mergeVertices`, which always emits an index).
    pub fn flip_winding(&mut self) {
        for tri in self.index.chunks_exact_mut(3) {
            tri.swap(0, 2);
        }
        for n in self.normal.iter_mut() {
            *n = -*n;
        }
    }

    /// `normalizeAttributes(geo)` (`geometry.js:32-45`), reduced to what
    /// applies once attributes are fixed to exactly `position`/`normal`/`uv`
    /// (the source's `deleteAttribute` loop over an arbitrary attribute set
    /// has nothing left to do): fill a missing `uv` with zeros, and compute a
    /// missing `normal` via [`Geo::compute_vertex_normals`]. `morphAttributes`
    /// and `clearGroups()` have no counterpart — this port carries neither.
    pub fn normalize_attributes(&mut self) {
        if self.uv.is_empty() {
            self.uv = vec![0.0; self.vert_count() * 2];
        }
        if self.normal.is_empty() {
            self.compute_vertex_normals();
        }
    }

    /// `BufferGeometry.computeVertexNormals()` in the "no existing normal
    /// attribute" branch — the only branch `normalizeAttributes` ever
    /// reaches, since it only calls this when `normal` is absent
    /// (`three/src/core/BufferGeometry.js:975-1063`). Accumulates the
    /// unnormalized face normal `(pC - pB) x (pA - pB)` at each of a
    /// triangle's three vertices (indexed: shared across every triangle
    /// touching that vertex, for a smooth normal; non-indexed: one triangle
    /// per vertex, for a flat normal), then unit-normalizes every result
    /// (`normalizeNormals`, dividing by `length() || 1`, so a degenerate
    /// zero-area triangle's vertices stay zero rather than becoming NaN).
    fn compute_vertex_normals(&mut self) {
        let vert_count = self.vert_count();
        self.normal = vec![0.0f32; vert_count * 3];

        let triangles: Vec<[usize; 3]> = if self.index.is_empty() {
            (0..vert_count).step_by(3).map(|i| [i, i + 1, i + 2]).collect()
        } else {
            self.index
                .chunks_exact(3)
                .map(|t| [t[0] as usize, t[1] as usize, t[2] as usize])
                .collect()
        };

        for [va, vb, vc] in triangles {
            let pa = read3(&self.pos, va);
            let pb = read3(&self.pos, vb);
            let pc = read3(&self.pos, vc);
            let cb = cross(sub(pc, pb), sub(pa, pb));
            accumulate3(&mut self.normal, va, cb);
            accumulate3(&mut self.normal, vb, cb);
            accumulate3(&mut self.normal, vc, cb);
        }

        for n in self.normal.chunks_exact_mut(3) {
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            let divisor = if len == 0.0 { 1.0 } else { len };
            n[0] /= divisor;
            n[1] /= divisor;
            n[2] /= divisor;
        }
    }
}

fn read3(buf: &[f32], i: usize) -> [f32; 3] {
    [buf[i * 3], buf[i * 3 + 1], buf[i * 3 + 2]]
}

fn accumulate3(buf: &mut [f32], i: usize, v: [f32; 3]) {
    buf[i * 3] += v[0];
    buf[i * 3 + 1] += v[1];
    buf[i * 3 + 2] += v[2];
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// `Matrix3.getNormalMatrix(m)` = `transpose(invert(setFromMatrix4(m)))`,
/// where `setFromMatrix4` takes only `m`'s upper-left 3x3 (translation is
/// irrelevant to a direction transform). For any 3x3 `A`,
/// `transpose(inverse(A)) == cofactor(A) / det(A)` (the adjugate is
/// `transpose(cofactor(A))`, and `inverse(A) = adjugate(A) / det(A)`, so
/// transposing an inverse cancels the adjugate's own transpose and leaves the
/// bare cofactor matrix) — computed directly here rather than through
/// [`axiom_math::Mat4::inverse`], because that returns `None` on a singular
/// matrix where the source silently produces `Infinity`/`NaN` through plain
/// float division; matching the source's total (never-panicking,
/// never-`Option`) behavior takes the direct formula.
fn upper_left_3x3_normal_matrix(m: &Mat4) -> [f32; 9] {
    let c = m.as_cols_array();
    // Column-major upper-left 3x3, row-major-named for readability below.
    let (m00, m10, m20) = (c[0], c[1], c[2]);
    let (m01, m11, m21) = (c[4], c[5], c[6]);
    let (m02, m12, m22) = (c[8], c[9], c[10]);

    let c00 = m11 * m22 - m12 * m21;
    let c01 = -(m10 * m22 - m12 * m20);
    let c02 = m10 * m21 - m11 * m20;
    let c10 = -(m01 * m22 - m02 * m21);
    let c11 = m00 * m22 - m02 * m20;
    let c12 = -(m00 * m21 - m01 * m20);
    let c20 = m01 * m12 - m02 * m11;
    let c21 = -(m00 * m12 - m02 * m10);
    let c22 = m00 * m11 - m01 * m10;

    let det = m00 * c00 + m01 * c01 + m02 * c02;
    let inv_det = 1.0 / det;

    // Row-major: normal_matrix[r][c] = cofactor(r, c) / det.
    [
        c00 * inv_det,
        c01 * inv_det,
        c02 * inv_det,
        c10 * inv_det,
        c11 * inv_det,
        c12 * inv_det,
        c20 * inv_det,
        c21 * inv_det,
        c22 * inv_det,
    ]
}

fn apply_3x3(row_major: &[f32; 9], v: [f32; 3]) -> [f32; 3] {
    [
        row_major[0] * v[0] + row_major[1] * v[1] + row_major[2] * v[2],
        row_major[3] * v[0] + row_major[4] * v[1] + row_major[5] * v[2],
        row_major[6] * v[0] + row_major[7] * v[1] + row_major[8] * v[2],
    ]
}
