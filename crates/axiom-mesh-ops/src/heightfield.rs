//! Turn a rectangular grid of sampled heights into a surface mesh.
//!
//! This is a **generic scalar-grid surface operator**, not a terrain system. It
//! knows nothing of biomes, materials, chunk streaming, LOD selection, world
//! coordinates, or gameplay: it takes `cols x rows` heights, a grid origin, a
//! spacing on each axis, a quad-split policy, and an optional skirt depth, and
//! returns geometry. Everything semantic — where the samples came from, what the
//! heights mean, when to re-mesh — belongs to the caller.
//!
//! The heights are sampled data for the same reason
//! [`crate::tessellate_surface`] takes samples: a public callback parameter is
//! forbidden across this engine's spine, and passing the samples keeps the whole
//! input visible and replayable.
//!
//! # Layout, winding, and normals
//!
//! Vertex `(col, row)` sits at `origin + (col * spacing_x, height, row * spacing_z)`,
//! stored row-major at `row * cols + col`. Columns run `+X`, rows run `+Z`, so
//! the surface faces `+Y` and its triangles are counter-clockwise seen from
//! above — the layer's CCW-front convention.
//!
//! Normals are the exact gradient normal `normalize(-dh/dx, 1, -dh/dz)`, with
//! each gradient formed by central differences **divided by the true distance
//! between the two sampled columns/rows**. That distance form matters: with
//! unequal `spacing_x` and `spacing_z` the older shortcut of "difference over
//! `2 * spacing`, `y = 2 * spacing`" silently uses the wrong axis scale and tilts
//! the shading. At a border the differencing window is clipped to the grid and
//! the divisor shrinks to match, so a linear ramp yields its analytically exact
//! normal everywhere, edges included.

use axiom_kernel::Meters;
use axiom_math::{Vec2, Vec3};
use axiom_mesh::{Mesh, MeshError, MeshErrorCode, MeshResult, MeshStreams};

/// Which way each grid cell's quad is split into two triangles.
///
/// The choice is visible: a quad has two possible diagonals, and the one you
/// pick decides which pair of opposite corners shares an edge — which changes
/// the interpolated silhouette and the shading of any cell that is not planar.
/// Exposed rather than fixed because for a ridge or a valley one diagonal
/// follows the feature and the other cuts across it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum TriangleDiagonal {
    /// Split along the `(col, row)` -> `(col+1, row+1)` diagonal.
    #[default]
    Forward = 0,
    /// Split along the `(col+1, row)` -> `(col, row+1)` diagonal.
    Backward = 1,
}

/// The two triangles of one cell, as offsets into the cell's corner list
/// `[i0, i1, i2, i3]` = `[(c,r), (c+1,r), (c,r+1), (c+1,r+1)]`. Indexed by the
/// diagonal's discriminant, so choosing a split is a table lookup.
const DIAGONAL_CORNERS: [[usize; 6]; 2] = [
    [0, 2, 3, 0, 3, 1], // Forward: shared edge i0-i3
    [0, 2, 1, 1, 2, 3], // Backward: shared edge i1-i2
];

/// A rectangular grid of sampled heights, row-major: the height at column `c`,
/// row `r` is entry `r * cols + c`.
#[derive(Debug, Clone, PartialEq)]
pub struct HeightfieldSamples {
    heights: Vec<Meters>,
    cols: u32,
    rows: u32,
}

impl HeightfieldSamples {
    /// Validate a row-major height grid: `cols >= 2`, `rows >= 2`, and exactly
    /// `cols * rows` heights ([`MeshErrorCode::InvalidGridDimensions`]).
    ///
    /// Finiteness needs no check here — [`Meters`] is finite by construction.
    pub fn new(heights: Vec<Meters>, cols: u32, rows: u32) -> MeshResult<HeightfieldSamples> {
        ((cols >= 2) & (rows >= 2) & (heights.len() as u64 == u64::from(cols) * u64::from(rows)))
            .then_some(HeightfieldSamples {
                heights,
                cols,
                rows,
            })
            .ok_or_else(|| {
                MeshError::new(
                    MeshErrorCode::InvalidGridDimensions,
                    "a heightfield needs cols >= 2, rows >= 2, and exactly cols * rows heights",
                )
            })
    }

    /// The number of samples along `+X`.
    pub const fn cols(&self) -> u32 {
        self.cols
    }

    /// The number of samples along `+Z`.
    pub const fn rows(&self) -> u32 {
        self.rows
    }

    /// The height at `(col, row)`, in metres.
    fn at(&self, col: u32, row: u32) -> f32 {
        self.heights[(row * self.cols + col) as usize].get()
    }
}

/// How a [`HeightfieldSamples`] grid is placed and closed off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeightfieldOptions {
    /// World position of sample `(0, 0)`.
    pub origin: Vec3,
    /// Distance between adjacent columns. Must be greater than zero.
    pub spacing_x: Meters,
    /// Distance between adjacent rows. Must be greater than zero.
    pub spacing_z: Meters,
    /// Which diagonal splits each quad.
    pub diagonal: TriangleDiagonal,
    /// When present, how far the border is extended straight down as a skirt.
    /// Must be greater than zero.
    pub skirt_depth: Option<Meters>,
}

impl Default for HeightfieldOptions {
    /// A unit-spaced grid at the origin, forward diagonals, no skirt.
    fn default() -> Self {
        HeightfieldOptions {
            origin: Vec3::ZERO,
            spacing_x: Meters::finite_or_zero(1.0),
            spacing_z: Meters::finite_or_zero(1.0),
            diagonal: TriangleDiagonal::Forward,
            skirt_depth: None,
        }
    }
}

fn invalid_parameter(message: &'static str) -> MeshError {
    MeshError::new(MeshErrorCode::InvalidParameter, message)
}

/// The vertex ring around the border, as grid indices, walked so that the
/// interior stays on the left: `+X` along row 0, `+Z` up the last column, `-X`
/// back along the last row, `-Z` down column 0. Length `2*(cols-1) + 2*(rows-1)`,
/// closing on itself.
fn border_ring(cols: u32, rows: u32) -> Vec<u32> {
    (0..cols - 1)
        .chain((0..rows - 1).map(|r| (cols - 1) + r * cols))
        .chain((0..cols - 1).map(|c| (rows - 1) * cols + (cols - 1 - c)))
        .chain((0..rows - 1).map(|r| (rows - 1 - r) * cols))
        .collect()
}

/// The skirt: a duplicated copy of the border ring plus a copy dropped `depth`
/// metres, joined by an outward-facing wall.
///
/// The ring vertices are duplicated rather than shared with the surface so the
/// border keeps a hard crease — a skirt is a curtain hiding the crack where a
/// neighbouring patch meets this one at a different resolution, and smoothing it
/// into the surface would pull the surface shading down over the edge. Each wall
/// vertex's normal is the average of its two adjacent edge normals, so the
/// curtain shades as one surface and the corners are mitred.
///
/// Returns `(positions, normals, uvs, indices)`; indices are already offset by
/// `base`, the surface's vertex count.
fn skirt_geometry(
    positions: &[Vec3],
    uvs: &[Vec2],
    cols: u32,
    rows: u32,
    depth: f32,
    base: u32,
) -> (Vec<Vec3>, Vec<Vec3>, Vec<Vec2>, Vec<u32>) {
    let ring = border_ring(cols, rows);
    let count = ring.len();
    let top: Vec<Vec3> = ring.iter().map(|&i| positions[i as usize]).collect();
    let ring_uvs: Vec<Vec2> = ring.iter().map(|&i| uvs[i as usize]).collect();
    // Outward horizontal normal of edge `e` (from ring vertex `e` to `e+1`):
    // `up x edge` points away from the interior for this ring direction. Both
    // normalizations below are total for any grid this operator accepts —
    // validated spacing makes every ring edge non-degenerate and horizontal, and
    // consecutive rectangle edges are collinear or perpendicular, never opposed —
    // so `unwrap_or` names the layer's deterministic default without adding a
    // reachable arm.
    let edge_out: Vec<Vec3> = (0..count)
        .map(|e| {
            let (a, b) = (top[e], top[(e + 1) % count]);
            Vec3::UNIT_Y
                .cross(Vec3::new(b.x - a.x, 0.0, b.z - a.z))
                .normalize()
                .unwrap_or(Vec3::UNIT_Y)
        })
        .collect();
    let ring_normals: Vec<Vec3> = (0..count)
        .map(|v| {
            edge_out[(v + count - 1) % count]
                .add(edge_out[v])
                .normalize()
                .unwrap_or(Vec3::UNIT_Y)
        })
        .collect();
    let drop = Vec3::new(0.0, -depth, 0.0);
    let skirt_positions: Vec<Vec3> = top
        .iter()
        .copied()
        .chain(top.iter().map(|p| p.add(drop)))
        .collect();
    let skirt_normals: Vec<Vec3> =
        ring_normals.iter().chain(ring_normals.iter()).copied().collect();
    let skirt_uvs: Vec<Vec2> = ring_uvs.iter().chain(ring_uvs.iter()).copied().collect();
    let n = count as u32;
    let indices: Vec<u32> = (0..n)
        .flat_map(|e| {
            let next = (e + 1) % n;
            let (a, b) = (base + e, base + next);
            let (a_low, b_low) = (base + n + e, base + n + next);
            [a, b, b_low, a, b_low, a_low]
        })
        .collect();
    (skirt_positions, skirt_normals, skirt_uvs, indices)
}

/// Mesh a sampled height grid.
///
/// Emits positions, gradient normals, and UVs normalized across the grid, two
/// triangles per cell in row-major order, split by `options.diagonal`. When
/// `options.skirt_depth` is present, a wall is added around the whole perimeter,
/// dropping the border straight down by that depth and facing outward.
///
/// Fails with [`MeshErrorCode::InvalidParameter`] if either spacing, or a
/// present skirt depth, is not greater than zero.
pub fn heightfield_mesh(
    samples: &HeightfieldSamples,
    options: HeightfieldOptions,
) -> MeshResult<Mesh> {
    let (sx, sz) = (options.spacing_x.get(), options.spacing_z.get());
    ((sx > 0.0) & (sz > 0.0))
        .then_some(())
        .ok_or_else(|| invalid_parameter("heightfield spacing must be greater than zero on both axes"))
        .and_then(|()| {
            options
                .skirt_depth
                .is_none_or(|d| d.get() > 0.0)
                .then_some(())
                .ok_or_else(|| invalid_parameter("a heightfield skirt depth must be greater than zero"))
        })
        .and_then(|()| build(samples, options, sx, sz))
}

/// Build the validated mesh: surface streams, then the optional skirt appended.
fn build(
    samples: &HeightfieldSamples,
    options: HeightfieldOptions,
    sx: f32,
    sz: f32,
) -> MeshResult<Mesh> {
    let (cols, rows) = (samples.cols, samples.rows);
    let vertex_count = cols * rows;
    let mut positions: Vec<Vec3> = (0..vertex_count)
        .map(|k| {
            let (c, r) = (k % cols, k / cols);
            options
                .origin
                .add(Vec3::new(c as f32 * sx, samples.at(c, r), r as f32 * sz))
        })
        .collect();
    let mut normals: Vec<Vec3> = (0..vertex_count)
        .map(|k| gradient_normal(samples, k % cols, k / cols, sx, sz))
        .collect();
    let mut uvs: Vec<Vec2> = (0..vertex_count)
        .map(|k| {
            Vec2::new(
                (k % cols) as f32 / (cols - 1) as f32,
                (k / cols) as f32 / (rows - 1) as f32,
            )
        })
        .collect();
    let corners = DIAGONAL_CORNERS[options.diagonal as usize];
    let mut indices: Vec<u32> = (0..(cols - 1) * (rows - 1))
        .flat_map(|cell| {
            let (c, r) = (cell % (cols - 1), cell / (cols - 1));
            let i0 = r * cols + c;
            let quad = [i0, i0 + 1, i0 + cols, i0 + cols + 1];
            corners.map(|k| quad[k])
        })
        .collect();

    let (sp, sn, su, si) = options
        .skirt_depth
        .map(|d| skirt_geometry(&positions, &uvs, cols, rows, d.get(), vertex_count))
        .unwrap_or_default();
    positions.extend(sp);
    normals.extend(sn);
    uvs.extend(su);
    indices.extend(si);

    Mesh::from_streams(MeshStreams {
        normals,
        uvs,
        ..MeshStreams::new(positions, indices)
    })
}

/// The exact gradient normal at `(col, row)`.
///
/// Central differences clipped to the grid: the divisor is the real distance
/// spanned by the two samples actually read, so an interior sample differences
/// over `2 * spacing` and a border sample over `1 * spacing`. A linear ramp
/// therefore reports its true normal at every vertex, including the edges.
fn gradient_normal(samples: &HeightfieldSamples, col: u32, row: u32, sx: f32, sz: f32) -> Vec3 {
    let c0 = col.saturating_sub(1);
    let c1 = (col + 1).min(samples.cols - 1);
    let r0 = row.saturating_sub(1);
    let r1 = (row + 1).min(samples.rows - 1);
    let dh_dx = (samples.at(c1, row) - samples.at(c0, row)) / ((c1 - c0) as f32 * sx);
    let dh_dz = (samples.at(col, r1) - samples.at(col, r0)) / ((r1 - r0) as f32 * sz);
    // The `y` component is 1, so this vector is never zero-length; the fallback
    // is the layer's documented default and is not reachable here.
    Vec3::new(-dh_dx, 1.0, -dh_dz)
        .normalize()
        .unwrap_or(Vec3::UNIT_Y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    fn samples(heights: &[f32], cols: u32, rows: u32) -> HeightfieldSamples {
        HeightfieldSamples::new(heights.iter().copied().map(m).collect(), cols, rows).unwrap()
    }

    fn flat(cols: u32, rows: u32) -> HeightfieldSamples {
        samples(&vec![0.0; (cols * rows) as usize], cols, rows)
    }

    fn face_normal(mesh: &Mesh, tri: usize) -> Vec3 {
        let (p, i) = (mesh.positions(), mesh.indices());
        let (a, b, c) = (
            p[i[tri * 3] as usize],
            p[i[tri * 3 + 1] as usize],
            p[i[tri * 3 + 2] as usize],
        );
        b.subtract(a).cross(c.subtract(a)).normalize().unwrap()
    }

    #[test]
    fn a_height_grid_reports_its_shape() {
        let s = flat(4, 3);
        assert_eq!(s.cols(), 4);
        assert_eq!(s.rows(), 3);
    }

    #[test]
    fn a_short_or_mismatched_height_grid_is_rejected() {
        for (n, cols, rows) in [(2_usize, 1_u32, 2_u32), (2, 2, 1), (5, 2, 3)] {
            assert_eq!(
                HeightfieldSamples::new(vec![m(0.0); n], cols, rows)
                    .unwrap_err()
                    .code(),
                MeshErrorCode::InvalidGridDimensions
            );
        }
    }

    #[test]
    fn a_flat_field_is_exactly_flat_and_faces_up() {
        const FLAT: Vec3 = Vec3::new(0.0, 1.0, 0.0);
        let mesh = heightfield_mesh(&flat(3, 3), HeightfieldOptions::default()).unwrap();
        assert_eq!(mesh.vertex_count(), 9);
        assert_eq!(mesh.triangle_count(), 8);
        let normals = mesh.normals().to_vec();
        assert!(
            normals.iter().all(|n| *n == FLAT),
            "a flat field's normals are exactly +Y: {normals:?}"
        );
        (0..mesh.triangle_count()).for_each(|t| {
            assert_eq!(face_normal(&mesh, t), FLAT, "triangle {t} winds CCW from above");
        });
    }

    #[test]
    fn positions_are_the_origin_plus_the_spaced_sample_lattice() {
        let options = HeightfieldOptions {
            origin: Vec3::new(10.0, 0.0, -4.0),
            spacing_x: m(2.0),
            spacing_z: m(0.5),
            ..HeightfieldOptions::default()
        };
        let s = samples(&[0.0, 1.0, 2.0, 3.0], 2, 2);
        let mesh = heightfield_mesh(&s, options).unwrap();
        assert_eq!(mesh.positions()[0], Vec3::new(10.0, 0.0, -4.0));
        assert_eq!(mesh.positions()[1], Vec3::new(12.0, 1.0, -4.0));
        assert_eq!(mesh.positions()[2], Vec3::new(10.0, 2.0, -3.5));
        assert_eq!(mesh.positions()[3], Vec3::new(12.0, 3.0, -3.5));
        assert_eq!(mesh.uvs()[0], Vec2::new(0.0, 0.0));
        assert_eq!(mesh.uvs()[3], Vec2::new(1.0, 1.0));
    }

    #[test]
    fn a_linear_ramp_reports_its_analytic_normal_everywhere() {
        // h = 2*x with spacing_x = 0.5 -> dh/dx = 2, dh/dz = 0.
        let heights: Vec<f32> = (0..12).map(|k| 2.0 * (k % 4) as f32 * 0.5).collect();
        let s = samples(&heights, 4, 3);
        let options = HeightfieldOptions {
            spacing_x: m(0.5),
            spacing_z: m(3.0),
            ..HeightfieldOptions::default()
        };
        let mesh = heightfield_mesh(&s, options).unwrap();
        let expected = Vec3::new(-2.0, 1.0, 0.0).normalize().unwrap();
        mesh.normals().iter().enumerate().for_each(|(i, n)| {
            assert!(
                n.subtract(expected).length() < 1.0e-6,
                "vertex {i}: {n:?} != {expected:?}"
            );
        });
    }

    #[test]
    fn an_unequal_spacing_ramp_scales_each_axis_independently() {
        // h = z, with a much wider x spacing. Only the z gradient is non-zero,
        // and it must be divided by spacing_z (4), not by spacing_x.
        let heights: Vec<f32> = (0..9).map(|k| (k / 3) as f32 * 4.0).collect();
        let s = samples(&heights, 3, 3);
        let options = HeightfieldOptions {
            spacing_x: m(100.0),
            spacing_z: m(4.0),
            ..HeightfieldOptions::default()
        };
        let mesh = heightfield_mesh(&s, options).unwrap();
        let expected = Vec3::new(0.0, 1.0, -1.0).normalize().unwrap();
        assert!(mesh.normals()[4].subtract(expected).length() < 1.0e-6);
    }

    #[test]
    fn the_diagonal_changes_the_index_buffer_but_not_the_vertices() {
        let s = samples(&[0.0, 1.0, 1.0, 0.0], 2, 2);
        let forward = heightfield_mesh(
            &s,
            HeightfieldOptions {
                diagonal: TriangleDiagonal::Forward,
                ..HeightfieldOptions::default()
            },
        )
        .unwrap();
        let backward = heightfield_mesh(
            &s,
            HeightfieldOptions {
                diagonal: TriangleDiagonal::Backward,
                ..HeightfieldOptions::default()
            },
        )
        .unwrap();
        assert_eq!(forward.vertex_count(), backward.vertex_count());
        assert_eq!(forward.triangle_count(), backward.triangle_count());
        assert_eq!(forward.positions(), backward.positions());
        assert_ne!(forward.indices(), backward.indices());
        assert_eq!(forward.indices(), &[0, 2, 3, 0, 3, 1]);
        assert_eq!(backward.indices(), &[0, 2, 1, 1, 2, 3]);
        assert_eq!(TriangleDiagonal::default(), TriangleDiagonal::Forward);
        // Both splits still wind CCW-from-above on a saddle.
        (0..2).for_each(|t| {
            assert!(face_normal(&forward, t).y > 0.0);
            assert!(face_normal(&backward, t).y > 0.0);
        });
    }

    #[test]
    fn a_skirt_adds_an_outward_curtain_hanging_exactly_below_the_border() {
        let bare = heightfield_mesh(&flat(3, 3), HeightfieldOptions::default()).unwrap();
        let options = HeightfieldOptions {
            skirt_depth: Some(m(2.5)),
            ..HeightfieldOptions::default()
        };
        let skirted = heightfield_mesh(&flat(3, 3), options).unwrap();

        // The border ring of a 3x3 grid is 2*(2) + 2*(2) = 8 vertices, duplicated
        // top and bottom, walled by 8 quads.
        assert_eq!(skirted.vertex_count(), bare.vertex_count() + 16);
        assert_eq!(skirted.triangle_count(), bare.triangle_count() + 16);
        assert!(skirted.vertex_count() > bare.vertex_count());
        assert!(skirted.triangle_count() > bare.triangle_count());

        let lowest = skirted
            .positions()
            .iter()
            .map(|p| p.y)
            .fold(f32::INFINITY, f32::min);
        assert_eq!(lowest, -2.5, "the curtain hangs exactly skirt_depth down");
        assert_eq!(bare.positions().iter().map(|p| p.y).fold(f32::INFINITY, f32::min), 0.0);

        // Every wall triangle faces away from the patch centre.
        let centre = Vec3::new(1.0, 0.0, 1.0);
        (bare.triangle_count()..skirted.triangle_count()).for_each(|t| {
            let i = skirted.indices()[t * 3] as usize;
            let p = skirted.positions()[i];
            let outward = Vec3::new(p.x - centre.x, 0.0, p.z - centre.z);
            assert!(
                face_normal(&skirted, t).dot(outward) > 0.0,
                "skirt triangle {t} faces inward"
            );
        });
        // And every wall vertex normal is horizontal and outward.
        skirted.normals()[bare.vertex_count()..]
            .iter()
            .zip(&skirted.positions()[bare.vertex_count()..])
            .for_each(|(n, p)| {
                assert!(n.y.abs() < 1.0e-6, "a curtain normal is horizontal: {n:?}");
                let outward = Vec3::new(p.x - centre.x, 0.0, p.z - centre.z);
                assert!(n.dot(outward) > 0.0, "normal {n:?} at {p:?}");
            });
    }

    #[test]
    fn a_skirt_on_a_non_square_grid_rings_the_whole_border() {
        let options = HeightfieldOptions {
            skirt_depth: Some(m(1.0)),
            ..HeightfieldOptions::default()
        };
        let mesh = heightfield_mesh(&flat(5, 2), options).unwrap();
        // ring = 2*(5-1) + 2*(2-1) = 10
        assert_eq!(mesh.vertex_count(), 10 + 20);
        assert_eq!(mesh.triangle_count(), 4 * 2 + 10 * 2);
        assert_eq!(border_ring(5, 2), vec![0, 1, 2, 3, 4, 9, 8, 7, 6, 5]);
    }

    #[test]
    fn a_non_positive_spacing_is_rejected() {
        let bad_x = HeightfieldOptions {
            spacing_x: m(0.0),
            ..HeightfieldOptions::default()
        };
        assert_eq!(
            heightfield_mesh(&flat(2, 2), bad_x).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
        let bad_z = HeightfieldOptions {
            spacing_z: m(-1.0),
            ..HeightfieldOptions::default()
        };
        assert_eq!(
            heightfield_mesh(&flat(2, 2), bad_z).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn a_non_positive_skirt_depth_is_rejected() {
        let options = HeightfieldOptions {
            skirt_depth: Some(m(0.0)),
            ..HeightfieldOptions::default()
        };
        assert_eq!(
            heightfield_mesh(&flat(2, 2), options).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn the_default_options_are_a_unit_grid_at_the_origin() {
        let d = HeightfieldOptions::default();
        assert_eq!(d.origin, Vec3::ZERO);
        assert_eq!(d.spacing_x, m(1.0));
        assert_eq!(d.spacing_z, m(1.0));
        assert_eq!(d.diagonal, TriangleDiagonal::Forward);
        assert_eq!(d.skirt_depth, None);
    }
}
