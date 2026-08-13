//! Tessellate a **pre-sampled** rectangular grid of surface points into a mesh.
//!
//! This is the layer's general parametric-surface operator. It does *not* take a
//! function `f(u, v) -> Vec3` to evaluate: the caller samples whatever surface it
//! likes — a Bezier patch, a NURBS evaluation, a torus, a cloth simulation's
//! current state, a scanned point grid — into a row-major array of positions, and
//! hands the array over. The operator owns only the topology, the smooth normals,
//! and the parameter-space UVs.
//!
//! **Why sampled data rather than a callback.** A public `impl Fn` parameter is
//! forbidden across this engine's spine by the State Law:
//! a callback is an opaque, unreplayable capability that can read a clock, a
//! global, or an RNG, which would make an operator's output depend on something
//! the caller cannot see in its inputs. Passing the samples instead makes the
//! whole input visible, hashable, and replayable — the same reason
//! [`crate::heightfield_mesh`] takes heights and [`crate::implicit_surface_mesh`]
//! takes a sampled field. Evaluation policy is the caller's; geometry is ours.
//!
//! # Winding
//!
//! Columns advance the `u` parameter, rows advance `v`. Triangles are wound so
//! the front face normal is `dP/dv x dP/du`. Concretely: for a grid laid out in
//! the XZ plane with columns running along `+X` and rows along `+Z`, the front
//! face points `+Y` — the same convention every generator in this layer uses
//! (CCW is front-facing, right-handed, Y-up).

use axiom_math::{Vec2, Vec3};
use axiom_mesh::{Mesh, MeshError, MeshErrorCode, MeshResult, MeshStreams};

/// A rectangular lattice of already-sampled surface points, row-major:
/// the point at column `c`, row `r` is `positions()[r * cols + c]`.
///
/// Validated on construction, so a tessellation of one cannot fail on shape.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceGrid {
    positions: Vec<Vec3>,
    cols: u32,
    rows: u32,
}

impl SurfaceGrid {
    /// Validate a row-major sample lattice.
    ///
    /// Requires `cols >= 2`, `rows >= 2`, and exactly `cols * rows` positions
    /// ([`MeshErrorCode::InvalidGridDimensions`]), every component finite
    /// ([`MeshErrorCode::NonFinitePosition`]).
    pub fn new(positions: Vec<Vec3>, cols: u32, rows: u32) -> MeshResult<SurfaceGrid> {
        let shaped = (cols >= 2)
            & (rows >= 2)
            & (positions.len() as u64 == u64::from(cols) * u64::from(rows));
        let finite = positions
            .iter()
            .all(|p| p.x.is_finite() & p.y.is_finite() & p.z.is_finite());
        shaped
            .then_some(())
            .ok_or_else(|| {
                MeshError::new(
                    MeshErrorCode::InvalidGridDimensions,
                    "a surface grid needs cols >= 2, rows >= 2, and exactly cols * rows positions",
                )
            })
            .and_then(|()| {
                finite.then_some(()).ok_or_else(|| {
                    MeshError::new(
                        MeshErrorCode::NonFinitePosition,
                        "every sampled surface position component must be finite",
                    )
                })
            })
            .map(|()| SurfaceGrid {
                positions,
                cols,
                rows,
            })
    }

    /// The number of samples along the `u` parameter.
    pub const fn cols(&self) -> u32 {
        self.cols
    }

    /// The number of samples along the `v` parameter.
    pub const fn rows(&self) -> u32 {
        self.rows
    }

    /// The row-major sample lattice.
    pub fn positions(&self) -> &[Vec3] {
        &self.positions
    }
}

/// Area-weighted smooth normals: every triangle contributes its un-normalized
/// cross product (whose length is twice the triangle area) to each of its three
/// corners, and each accumulated vector is then normalized.
///
/// A vertex whose contributions cancel exactly — only reachable on a degenerate
/// lattice — falls back to `+Y`, the layer's documented deterministic default.
fn smooth_normals(positions: &[Vec3], indices: &[u32]) -> Vec<Vec3> {
    let mut accumulated = vec![Vec3::ZERO; positions.len()];
    indices.chunks_exact(3).for_each(|t| {
        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let face = positions[b]
            .subtract(positions[a])
            .cross(positions[c].subtract(positions[a]));
        accumulated[a] = accumulated[a].add(face);
        accumulated[b] = accumulated[b].add(face);
        accumulated[c] = accumulated[c].add(face);
    });
    accumulated
        .into_iter()
        .map(|n| n.normalize().unwrap_or(Vec3::UNIT_Y))
        .collect()
}

/// Tessellate a sampled surface lattice into two triangles per cell, in
/// row-major cell order.
///
/// `wrap_u` adds the closing ring of cells that joins the last column back to
/// the first (a cylinder / tube parameterization); `wrap_v` does the same for
/// rows. Both together close a torus. A wrapped axis reuses the *existing*
/// column-0 / row-0 vertices rather than duplicating a seam, so a wrapped
/// parameter is genuinely periodic: `u` is normalized by the number of cells
/// (`cols` when wrapped, `cols - 1` when open), which is exactly the periodic
/// parameterization. A caller that wants a texture seam with `u` reaching both
/// `0` and `1` samples the duplicate column itself and leaves `wrap_u` false.
///
/// Normals are area-weighted smooth normals of the surrounding faces; UVs are
/// the normalized `(column, row)` parameter position.
pub fn tessellate_surface(grid: &SurfaceGrid, wrap_u: bool, wrap_v: bool) -> MeshResult<Mesh> {
    let (cols, rows) = (grid.cols, grid.rows);
    let cells_u = cols - 1 + u32::from(wrap_u);
    let cells_v = rows - 1 + u32::from(wrap_v);
    let indices: Vec<u32> = (0..cells_u * cells_v)
        .flat_map(|cell| {
            let (cu, cv) = (cell % cells_u, cell / cells_u);
            let (c0, c1) = (cu, (cu + 1) % cols);
            let (r0, r1) = (cv, (cv + 1) % rows);
            let i0 = r0 * cols + c0;
            let i1 = r0 * cols + c1;
            let i2 = r1 * cols + c0;
            let i3 = r1 * cols + c1;
            [i0, i2, i3, i0, i3, i1]
        })
        .collect();
    let uvs: Vec<Vec2> = (0..cols * rows)
        .map(|k| {
            Vec2::new(
                (k % cols) as f32 / cells_u as f32,
                (k / cols) as f32 / cells_v as f32,
            )
        })
        .collect();
    let normals = smooth_normals(&grid.positions, &indices);
    Mesh::from_streams(MeshStreams {
        normals,
        uvs,
        ..MeshStreams::new(grid.positions.clone(), indices)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat `cols x rows` lattice in the XZ plane, one metre apart.
    fn flat(cols: u32, rows: u32) -> SurfaceGrid {
        let positions = (0..cols * rows)
            .map(|k| Vec3::new((k % cols) as f32, 0.0, (k / cols) as f32))
            .collect();
        SurfaceGrid::new(positions, cols, rows).unwrap()
    }

    /// A tube of `cols` samples around `+Y`, `rows` samples up it. Column `c`
    /// sits at angle `2*pi*c/cols`, which makes `dP/du` the counter-clockwise
    /// tangent and therefore the front face outward.
    fn tube(cols: u32, rows: u32) -> SurfaceGrid {
        let positions = (0..cols * rows)
            .map(|k| {
                let theta = (k % cols) as f32 / cols as f32 * core::f32::consts::TAU;
                Vec3::new(theta.cos(), (k / cols) as f32, theta.sin())
            })
            .collect();
        SurfaceGrid::new(positions, cols, rows).unwrap()
    }

    fn face_normal(mesh: &Mesh, tri: usize) -> Vec3 {
        let p = mesh.positions();
        let i = mesh.indices();
        let (a, b, c) = (
            p[i[tri * 3] as usize],
            p[i[tri * 3 + 1] as usize],
            p[i[tri * 3 + 2] as usize],
        );
        b.subtract(a).cross(c.subtract(a)).normalize().unwrap()
    }

    #[test]
    fn a_grid_reports_its_shape_and_samples() {
        let g = flat(4, 3);
        assert_eq!(g.cols(), 4);
        assert_eq!(g.rows(), 3);
        assert_eq!(g.positions().len(), 12);
        assert_eq!(g.positions()[5], Vec3::new(1.0, 0.0, 1.0));
    }

    #[test]
    fn a_short_or_mismatched_lattice_is_rejected() {
        for (n, cols, rows) in [(2_u32, 1_u32, 2_u32), (2, 2, 1), (5, 2, 3)] {
            let positions = vec![Vec3::ZERO; n as usize];
            assert_eq!(
                SurfaceGrid::new(positions, cols, rows).unwrap_err().code(),
                MeshErrorCode::InvalidGridDimensions
            );
        }
    }

    #[test]
    fn a_non_finite_sample_is_rejected() {
        let mut positions = vec![Vec3::ZERO; 4];
        positions[3] = Vec3::new(0.0, f32::NAN, 0.0);
        assert_eq!(
            SurfaceGrid::new(positions, 2, 2).unwrap_err().code(),
            MeshErrorCode::NonFinitePosition
        );
    }

    #[test]
    fn an_open_grid_emits_two_triangles_per_cell_facing_up() {
        let m = tessellate_surface(&flat(3, 3), false, false).unwrap();
        assert_eq!(m.vertex_count(), 9);
        assert_eq!(m.triangle_count(), 8);
        // Columns run +X and rows run +Z, so the front face is +Y.
        (0..m.triangle_count()).for_each(|t| {
            assert_eq!(face_normal(&m, t), Vec3::UNIT_Y, "triangle {t}");
        });
        assert!(m.normals().iter().all(|n| *n == Vec3::UNIT_Y));
    }

    #[test]
    fn an_open_grid_parameterizes_uvs_across_the_whole_lattice() {
        let m = tessellate_surface(&flat(3, 2), false, false).unwrap();
        assert_eq!(m.uvs()[0], Vec2::new(0.0, 0.0));
        assert_eq!(m.uvs()[2], Vec2::new(1.0, 0.0));
        assert_eq!(m.uvs()[3], Vec2::new(0.0, 1.0));
        assert_eq!(m.uvs()[5], Vec2::new(1.0, 1.0));
    }

    #[test]
    fn wrapping_u_closes_the_ring_and_adds_one_cell_column() {
        let open = tessellate_surface(&tube(8, 2), false, false).unwrap();
        let closed = tessellate_surface(&tube(8, 2), true, false).unwrap();
        assert_eq!(open.triangle_count(), 7 * 2);
        assert_eq!(closed.triangle_count(), 8 * 2);
        // No extra vertices: the closing cells reuse column 0.
        assert_eq!(closed.vertex_count(), open.vertex_count());
        // The wrapped parameter is periodic, so u is divided by the cell count.
        assert_eq!(closed.uvs()[7].x, 7.0 / 8.0);
        assert_eq!(open.uvs()[7].x, 1.0);
    }

    #[test]
    fn a_wrapped_tube_faces_outward_everywhere() {
        let m = tessellate_surface(&tube(16, 3), true, false).unwrap();
        // Every smooth normal is radial-outward (the y component is ~0 on a tube).
        m.positions()
            .iter()
            .zip(m.normals())
            .for_each(|(p, n)| {
                let radial = Vec3::new(p.x, 0.0, p.z).normalize().unwrap();
                assert!(n.dot(radial) > 0.9, "normal {n:?} at {p:?}");
            });
        // And every face agrees with its vertices.
        (0..m.triangle_count()).for_each(|t| {
            let i = m.indices()[t * 3] as usize;
            let p = m.positions()[i];
            let radial = Vec3::new(p.x, 0.0, p.z).normalize().unwrap();
            assert!(face_normal(&m, t).dot(radial) > 0.5, "triangle {t}");
        });
    }

    #[test]
    fn wrapping_v_closes_the_other_axis() {
        let open = tessellate_surface(&flat(3, 4), false, false).unwrap();
        let closed = tessellate_surface(&flat(3, 4), false, true).unwrap();
        assert_eq!(open.triangle_count(), 2 * 3 * 2);
        assert_eq!(closed.triangle_count(), 2 * 4 * 2);
        assert_eq!(closed.uvs()[3].y, 0.25);
    }

    #[test]
    fn wrapping_both_axes_closes_a_torus() {
        let m = tessellate_surface(&flat(4, 5), true, true).unwrap();
        assert_eq!(m.triangle_count(), 4 * 5 * 2);
        assert_eq!(m.vertex_count(), 20);
        // A closed torus is edge-manifold: every vertex is touched by six
        // triangle corners (two triangles per cell, three cells meeting).
        let touches = m.indices().iter().filter(|&&i| i == 0).count();
        assert_eq!(touches, 6);
    }

    #[test]
    fn a_degenerate_lattice_falls_back_to_the_documented_normal() {
        // Every sample coincident: every face normal is zero, so every
        // accumulated normal cancels and the +Y fallback is used.
        let g = SurfaceGrid::new(vec![Vec3::ZERO; 4], 2, 2).unwrap();
        let m = tessellate_surface(&g, false, false).unwrap();
        assert!(m.normals().iter().all(|n| *n == Vec3::UNIT_Y));
    }

    #[test]
    fn a_curved_surface_gets_smoothly_varying_normals() {
        // A cylinder-like ridge: y = 1 - x^2 over a 5x2 lattice.
        let positions: Vec<Vec3> = (0..10)
            .map(|k| {
                let x = (k % 5) as f32 * 0.5 - 1.0;
                Vec3::new(x, 1.0 - x * x, (k / 5) as f32)
            })
            .collect();
        let g = SurfaceGrid::new(positions, 5, 2).unwrap();
        let m = tessellate_surface(&g, false, false).unwrap();
        // The crest normal is the most up-facing of its row; each flank of the
        // dome leans outward (the up-facing normal of `y = 1 - x^2` is
        // proportional to `(2x, 1, 0)`).
        let crest = m.normals()[2];
        assert!(crest.dot(Vec3::UNIT_Y) > 0.98, "the crest is {crest:?}");
        assert!(m.normals()[2].y > m.normals()[1].y);
        assert!(m.normals()[2].y > m.normals()[3].y);
        assert!(m.normals()[0].x < -0.5, "the -x flank leans -x");
        assert!(m.normals()[4].x > 0.5, "the +x flank leans +x");
        assert!(m.normals().iter().all(|n| (n.length() - 1.0).abs() < 1.0e-5));
    }
}
