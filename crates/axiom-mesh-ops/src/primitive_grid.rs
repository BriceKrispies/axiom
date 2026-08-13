//! The subdivided flat grid primitive: a tessellated rectangle in the XZ plane,
//! facing `+Y`.
//!
//! A grid is the input every vertex-displacing operator wants — terrain, water,
//! a cloth patch, a lightmapped floor — so it carries interior vertices that
//! [`crate::quad`] does not. The division counts are [`Segments`], so the
//! layer's tessellation vocabulary (and its `3..=4096` bound) applies here
//! exactly as it does to a cylinder's radial count.

use axiom_kernel::Meters;
use axiom_math::{Vec2, Vec3};
use axiom_mesh::{Mesh, MeshError, MeshErrorCode, MeshResult, MeshStreams};

use crate::tessellation::Segments;

/// A `cols` x `rows` tessellated rectangle centred on the origin in the XZ
/// plane, with `+Y` normals.
///
/// Vertices are row-major: `(cols + 1) * (rows + 1)` of them, index
/// `row * (cols + 1) + col`, marching `+X` along a row and `+Z` between rows.
/// Each of the `cols * rows` cells is split into two counter-clockwise
/// triangles. UVs span the whole grid — `u = col / cols`, `v = row / rows` — so
/// a texture stretches once across the sheet rather than repeating per cell.
///
/// # Errors
///
/// [`MeshErrorCode::InvalidParameter`] when either half-extent is zero or
/// negative.
pub fn grid(
    half_width: Meters,
    half_depth: Meters,
    cols: Segments,
    rows: Segments,
) -> MeshResult<Mesh> {
    let (w, d) = (half_width.get(), half_depth.get());
    ((w > 0.0) & (d > 0.0))
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "a grid needs strictly positive half-extents",
            )
        })
        .and_then(|()| build(w, d, cols.get(), rows.get()))
}

fn build(w: f32, d: f32, cols: u32, rows: u32) -> MeshResult<Mesh> {
    let stride = cols + 1;
    let count = stride * (rows + 1);
    let fraction = move |k: u32| {
        (
            (k % stride) as f32 / cols as f32,
            (k / stride) as f32 / rows as f32,
        )
    };
    Mesh::from_streams(MeshStreams {
        normals: vec![Vec3::UNIT_Y; count as usize],
        uvs: (0..count)
            .map(|k| {
                let (u, v) = fraction(k);
                Vec2::new(u, v)
            })
            .collect(),
        ..MeshStreams::new(
            (0..count)
                .map(|k| {
                    let (u, v) = fraction(k);
                    Vec3::new(w * (2.0 * u - 1.0), 0.0, d * (2.0 * v - 1.0))
                })
                .collect(),
            (0..cols * rows)
                .flat_map(|cell| {
                    let base = (cell / cols) * stride + (cell % cols);
                    [
                        base,
                        base + stride,
                        base + stride + 1,
                        base,
                        base + stride + 1,
                        base + 1,
                    ]
                })
                .collect(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meters(v: f32) -> Meters {
        Meters::new(v).unwrap()
    }

    fn segments(v: u32) -> Segments {
        Segments::new(v).unwrap()
    }

    fn built(cols: u32, rows: u32) -> Mesh {
        grid(meters(1.0), meters(2.0), segments(cols), segments(rows)).unwrap()
    }

    #[test]
    fn a_grid_has_one_more_vertex_than_cell_per_axis() {
        // The smallest grid the Segments vocabulary permits is 3x3.
        let m = built(3, 3);
        assert_eq!(m.vertex_count(), 4 * 4);
        assert_eq!(m.triangle_count(), 3 * 3 * 2);

        let m = built(3, 4);
        assert_eq!(m.vertex_count(), 4 * 5);
        assert_eq!(m.triangle_count(), 3 * 4 * 2);
    }

    #[test]
    fn the_corners_sit_on_the_requested_half_extents() {
        let m = built(3, 4);
        let p = m.positions();
        assert_eq!(p[0], Vec3::new(-1.0, 0.0, -2.0));
        assert_eq!(p[3], Vec3::new(1.0, 0.0, -2.0));
        assert_eq!(p[16], Vec3::new(-1.0, 0.0, 2.0));
        assert_eq!(p[19], Vec3::new(1.0, 0.0, 2.0));
        assert!(p.iter().all(|q| q.y == 0.0));
    }

    #[test]
    fn the_uvs_span_zero_to_one_across_the_whole_sheet() {
        let m = built(3, 4);
        let uv = m.uvs();
        assert_eq!(uv[0], Vec2::new(0.0, 0.0));
        assert_eq!(uv[3], Vec2::new(1.0, 0.0));
        assert_eq!(uv[16], Vec2::new(0.0, 1.0));
        assert_eq!(uv[19], Vec2::new(1.0, 1.0));
        // The interior really is interior: a mid-row vertex sits at 1/3.
        assert!((uv[1].x - 1.0 / 3.0).abs() < 1.0e-6);
    }

    #[test]
    fn every_normal_is_exactly_up() {
        let m = built(3, 3);
        assert!(m.normals().iter().all(|n| *n == Vec3::UNIT_Y));
        assert_eq!(m.normals().len(), m.vertex_count());
    }

    #[test]
    fn every_triangle_winds_counter_clockwise_about_the_normal() {
        let m = built(4, 3);
        let p = m.positions();
        for t in m.indices().chunks(3) {
            let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
            assert!(b.subtract(a).cross(c.subtract(a)).dot(Vec3::UNIT_Y) > 0.0);
        }
    }

    #[test]
    fn non_positive_half_extents_are_rejected() {
        for (w, d) in [(0.0, 1.0), (1.0, 0.0), (-2.0, 1.0), (1.0, -2.0)] {
            assert_eq!(
                grid(meters(w), meters(d), segments(3), segments(3))
                    .unwrap_err()
                    .code(),
                MeshErrorCode::InvalidParameter
            );
        }
    }
}
