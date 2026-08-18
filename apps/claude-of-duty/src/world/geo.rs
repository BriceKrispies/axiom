//! Ported from Claude-of-Duty `src/world/util.js` — the `THREE.BufferGeometry`
//! shape every world-side geometry builder (`chamferBox`, `patchGeometry`,
//! `wallPanel`, the ground/road patches, …) constructs and [`crate::world::accum::Accum`]
//! merges.
//!
//! This is the world side's counterpart to `weapons::geometry::Geo`, kept as a
//! **separate type** rather than shared: the source itself keeps two
//! independent files (`src/weapons/geometry.js`, `src/world/util.js`) with two
//! independent geometry shapes, and the shapes really do differ — a weapon
//! part never carries a `color` (mask) attribute, and every world geometry
//! does (or is treated as if its `color` attribute is all-zero when absent,
//! matching `Accum.add`'s `ca ? ca.getX(i) : 0` fallback, `util.js:151-153`).
//! Duplicating the small amount of transform/normal-computation logic here
//! mirrors that same file boundary rather than manufacturing a shared
//! dependency between two otherwise-isolated builder kits.

use axiom_math::{Mat4, Vec3};

/// A flattened `THREE.BufferGeometry` carrying `position`, `normal`, `uv` and
/// an *optional* `color` (mask) attribute, plus an optional triangle `index`.
///
/// `color` being `empty()` models the source's `ca === undefined` — most
/// builders here (`chamferBox`, `wallPanel`, anything that finishes with
/// `paintMasks`/`fillMasks`) always populate it, but `patchGeometry` and
/// `plainBox`/`quad`'s underlying primitives do not, and [`crate::world::accum::Accum::add`]
/// treats a missing `color` as an implicit `[0, 0, 0]` per vertex — exactly
/// as `util.js:151-153` does.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorldGeo {
    /// `xyz` triples, one per vertex.
    pub pos: Vec<f32>,
    /// `xyz` triples, one per vertex.
    pub normal: Vec<f32>,
    /// `uv` pairs, one per vertex.
    pub uv: Vec<f32>,
    /// `[wear, grime, ao]` triples, one per vertex. Empty means "no `color`
    /// attribute" (see the struct doc).
    pub color: Vec<f32>,
    /// Triangle indices, three per triangle. Empty means non-indexed (the
    /// state `chamferBox` and the `THREE.PlaneGeometry`/`BoxGeometry`-derived
    /// primitives that never call `setIndex` are always in).
    pub index: Vec<u32>,
}

impl WorldGeo {
    /// `geo.getAttribute('position').count`.
    pub fn vert_count(&self) -> usize {
        self.pos.len() / 3
    }

    /// Index-triple count when indexed, else position-triple count — the same
    /// `triCount` shape `weapons::geometry::Geo::tri_count` ports, needed here
    /// too for the Assembler's `stats.staticTris`/`instTris`/`collideTris`
    /// (`builder.js:333`, `:390`, `:415`, all `geo.index.count / 3` or the
    /// non-indexed fallback).
    pub fn tri_count(&self) -> usize {
        if self.index.is_empty() {
            self.vert_count() / 3
        } else {
            self.index.len() / 3
        }
    }

    /// `BufferGeometry.applyMatrix4(m)`: transforms every position as a
    /// point, and every normal by the normal matrix
    /// (`transpose(inverse(upperLeft3x3(m)))`), matching
    /// `weapons::geometry::Geo::apply`'s reasoning exactly — `Accum.add`
    /// (`util.js:140,144,146`) does the identical two-transform dance via
    /// `_nm.getNormalMatrix(matrix)` + `Vector3.applyMatrix3(_nm).normalize()`
    /// before it ever writes to `this.pos`/`this.nrm`.
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
            let divisor = if len == 0.0 { 1.0 } else { len };
            n[0] = v[0] / divisor;
            n[1] = v[1] / divisor;
            n[2] = v[2] / divisor;
        }
    }

    /// `BufferGeometry.computeVertexNormals()`, the same accumulate-then-
    /// normalize algorithm as `weapons::geometry::Geo`'s private copy —
    /// needed independently here because several world builders
    /// (`wallPanel`, the ground/road patches) call it directly rather than
    /// going through a shared normalize step.
    pub fn compute_vertex_normals(&mut self) {
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

    /// `paintMasks(geo, fn)` (`util.js:205-227`), the [`WorldGeo`]-native
    /// counterpart to [`crate::world::masks::paint_masks`] (which operates on
    /// the minimal [`crate::world::masks::MaskGeometry`] carrier — see that
    /// module's doc for why both exist). Lazily allocates a zeroed `color`
    /// column when this geometry doesn't have one yet, exactly as the
    /// source's `ca = new Float32BufferAttribute(...)` fallback does, then
    /// hands the callback each vertex's position/normal plus its *current*
    /// mask (read into `out`, written back after the callback runs).
    pub fn paint_masks<F>(&mut self, mut paint: F)
    where
        F: FnMut(f32, f32, f32, f32, f32, f32, &mut [f32; 3], usize),
    {
        if self.color.is_empty() {
            self.color = vec![0.0; self.vert_count() * 3];
        }
        for i in 0..self.vert_count() {
            let px = self.pos[i * 3];
            let py = self.pos[i * 3 + 1];
            let pz = self.pos[i * 3 + 2];
            let nx = self.normal[i * 3];
            let ny = self.normal[i * 3 + 1];
            let nz = self.normal[i * 3 + 2];
            let mut out = [self.color[i * 3], self.color[i * 3 + 1], self.color[i * 3 + 2]];
            paint(px, py, pz, nx, ny, nz, &mut out, i);
            self.color[i * 3] = out[0];
            self.color[i * 3 + 1] = out[1];
            self.color[i * 3 + 2] = out[2];
        }
    }

    /// `fillMasks(geo, w = 0, g = 0, a = 0)` (`util.js:230-240`), the
    /// [`WorldGeo`]-native counterpart to
    /// [`crate::world::masks::fill_masks`]. Overwrites (not blends with)
    /// every vertex's mask, allocating the `color` column if absent.
    pub fn fill_masks(&mut self, w: f32, g: f32, a: f32) {
        let n = self.vert_count();
        self.color = vec![0.0; n * 3];
        for c in self.color.chunks_exact_mut(3) {
            c[0] = w;
            c[1] = g;
            c[2] = a;
        }
    }

    /// Translate every position by `(dx, dy, dz)` — `BufferGeometry.translate`,
    /// used directly (not through [`WorldGeo::apply`]) by a couple of the
    /// source's builders (e.g. `wallPanel`'s bevel-offset translate,
    /// `geometry.js`-style `g.translate(0,0,z)` calls that appear throughout
    /// `util.js`/`kit.js`). Normals are direction-only and untouched.
    pub fn translate(&mut self, dx: f32, dy: f32, dz: f32) {
        for p in self.pos.chunks_exact_mut(3) {
            p[0] += dx;
            p[1] += dy;
            p[2] += dz;
        }
    }

    /// `BufferGeometry.rotateX(angle)`: a rigid rotation about the X axis,
    /// used by `buildGround` (`ground.js:24,45`) to lay a `PlaneGeometry`
    /// (authored flat in XY, normal `+Z`) down into the XZ ground plane.
    /// Three implements every `rotate*` as `applyMatrix4` with a pure
    /// rotation matrix; since a rotation matrix is orthogonal its own normal
    /// matrix equals itself, so this is exactly [`WorldGeo::apply`] with a
    /// rotation-only [`Mat4`].
    pub fn rotate_x(&mut self, angle: f32) {
        let (s, c) = angle.sin_cos();
        // Column-major rotation-about-X matrix.
        let m = Mat4::from_cols_array([
            1.0, 0.0, 0.0, 0.0, //
            0.0, c, s, 0.0, //
            0.0, -s, c, 0.0, //
            0.0, 0.0, 0.0, 1.0, //
        ]);
        self.apply(&m);
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

/// `Matrix3.getNormalMatrix(m)` = cofactor(upperLeft3x3(m)) / det — see
/// `weapons::geometry::geo`'s copy of this derivation for the full algebra;
/// duplicated here for the same file-boundary reason as the rest of this
/// module.
fn upper_left_3x3_normal_matrix(m: &Mat4) -> [f32; 9] {
    let c = m.as_cols_array();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_triangle() -> WorldGeo {
        WorldGeo {
            pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normal: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            uv: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            color: vec![0.1, 0.2, 0.3, 0.1, 0.2, 0.3, 0.1, 0.2, 0.3],
            index: Vec::new(),
        }
    }

    #[test]
    fn vert_and_tri_count_non_indexed() {
        let g = unit_triangle();
        assert_eq!(g.vert_count(), 3);
        assert_eq!(g.tri_count(), 1);
    }

    #[test]
    fn vert_and_tri_count_indexed() {
        let mut g = unit_triangle();
        g.index = vec![0, 1, 2, 0, 2, 1];
        assert_eq!(g.tri_count(), 2);
    }

    #[test]
    fn translate_moves_positions_only() {
        let mut g = unit_triangle();
        let before_normal = g.normal.clone();
        g.translate(1.0, 2.0, 3.0);
        assert_eq!(g.pos, vec![1.0, 2.0, 3.0, 2.0, 2.0, 3.0, 1.0, 3.0, 3.0]);
        assert_eq!(g.normal, before_normal);
    }

    #[test]
    fn apply_identity_is_a_no_op() {
        let mut g = unit_triangle();
        let before = g.clone();
        g.apply(&Mat4::IDENTITY);
        for (a, b) in g.pos.iter().zip(before.pos.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
        for (a, b) in g.normal.iter().zip(before.normal.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn apply_translates_points_but_not_normals() {
        let mut g = unit_triangle();
        let m = Mat4::translation(Vec3::new(5.0, 0.0, 0.0));
        g.apply(&m);
        assert!((g.pos[0] - 5.0).abs() < 1e-6);
        assert!((g.pos[3] - 6.0).abs() < 1e-6);
        assert!((g.normal[2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn compute_vertex_normals_from_index() {
        let mut g = WorldGeo {
            pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normal: Vec::new(),
            uv: Vec::new(),
            color: Vec::new(),
            index: vec![0, 1, 2],
        };
        g.compute_vertex_normals();
        for n in g.normal.chunks_exact(3) {
            assert!((n[0]).abs() < 1e-6);
            assert!((n[1]).abs() < 1e-6);
            assert!((n[2] - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn paint_masks_lazily_allocates_a_zeroed_color_column() {
        let mut g = unit_triangle();
        g.color.clear();
        g.paint_masks(|_x, _y, _z, _nx, _ny, _nz, out, _i| {
            out[0] += 1.0;
        });
        assert_eq!(g.color, vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn paint_masks_reads_the_existing_mask_before_writing() {
        let mut g = unit_triangle();
        g.paint_masks(|_x, _y, _z, _nx, _ny, _nz, out, _i| {
            out[1] += 1.0;
        });
        assert_eq!(&g.color[0..3], &[0.1, 1.2, 0.3]);
    }

    #[test]
    fn fill_masks_overwrites_every_vertex() {
        let mut g = unit_triangle();
        g.fill_masks(0.4, 0.5, 0.6);
        assert_eq!(g.color, vec![0.4, 0.5, 0.6, 0.4, 0.5, 0.6, 0.4, 0.5, 0.6]);
    }

    #[test]
    fn compute_vertex_normals_degenerate_triangle_stays_zero() {
        let mut g = WorldGeo {
            pos: vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            normal: Vec::new(),
            uv: Vec::new(),
            color: Vec::new(),
            index: vec![0, 1, 2],
        };
        g.compute_vertex_normals();
        assert_eq!(g.normal, vec![0.0; 9]);
    }
}
