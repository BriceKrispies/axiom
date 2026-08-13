//! The capsule (stadium of revolution) about `+Y`: a cylindrical mid-section
//! closed by two hemispheres.

use core::f32::consts::{FRAC_PI_2, TAU};

use axiom_kernel::Meters;
use axiom_math::{Vec2, Vec3};
use axiom_mesh::{Mesh, MeshError, MeshErrorCode, MeshResult, MeshStreams};

use crate::tessellation::{Rings, Segments};

/// A capsule about the `+Y` axis, centred on the origin.
///
/// A cylindrical mid-section of length `2 * half_height` and the given `radius`,
/// closed by a hemisphere of the same radius at each end. Total height is
/// therefore `2 * (half_height + radius)`, and every point of the surface is
/// exactly `radius` from the capsule's spine — the segment from `-half_height`
/// to `+half_height` on the `Y` axis. That is the whole reason a capsule exists
/// as a primitive rather than as three glued parts.
///
/// `rings` is the number of latitude bands **per hemisphere**; `segments` is the
/// longitude division count. Vertices form a `(2 * rings + 2) x (segments + 1)`
/// grid: the extra column duplicates the seam so `u` reaches both `0.0` and
/// `1.0`, and the two equator rows are distinct because they sit at opposite
/// ends of the mid-section. `v` runs `0` at the bottom pole to `1` at the top.
///
/// `half_height` may be **zero**, which collapses the mid-section and leaves a
/// sphere of `radius`. The zero-height band is then omitted rather than emitted
/// as a strip of degenerate triangles.
///
/// # Errors
///
/// [`MeshErrorCode::InvalidParameter`] when `radius` is not strictly positive,
/// or when `half_height` is negative.
pub fn capsule(
    radius: Meters,
    half_height: Meters,
    rings: Rings,
    segments: Segments,
) -> MeshResult<Mesh> {
    let (r, h) = (radius.get(), half_height.get());
    ((r > 0.0) & (h >= 0.0))
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "a capsule needs a strictly positive radius and a non-negative half-height",
            )
        })
        .and_then(|()| {
            let (ring_count, seg) = (rings.get(), segments.get());
            let rows = 2 * ring_count + 2;
            let cols = seg + 1;
            let span = 2.0 * (h + r);
            let normals = (0..rows)
                .flat_map(|row| {
                    let (horizontal, vertical) = row_shape(row, ring_count);
                    (0..cols).map(move |j| {
                        let a = TAU * j as f32 / seg as f32;
                        Vec3::new(horizontal * a.cos(), vertical, horizontal * a.sin())
                    })
                })
                .collect();
            let positions = (0..rows)
                .flat_map(|row| {
                    let (horizontal, vertical) = row_shape(row, ring_count);
                    let y = [-h, h][usize::from(row > ring_count)] + r * vertical;
                    (0..cols).map(move |j| {
                        let a = TAU * j as f32 / seg as f32;
                        Vec3::new(r * horizontal * a.cos(), y, r * horizontal * a.sin())
                    })
                })
                .collect();
            let uvs = (0..rows)
                .flat_map(|row| {
                    let (_, vertical) = row_shape(row, ring_count);
                    let v = ([-h, h][usize::from(row > ring_count)] + r * vertical + h + r) / span;
                    (0..cols).map(move |j| Vec2::new(j as f32 / seg as f32, v))
                })
                .collect();
            Mesh::from_streams(MeshStreams {
                normals,
                uvs,
                ..MeshStreams::new(positions, band_indices(ring_count, seg, h))
            })
        })
}

/// The horizontal and vertical components of row `row`'s outward unit normal.
///
/// Rows `0..=rings` sweep the bottom hemisphere from the `-Y` pole to its
/// equator; rows `rings + 1..=2 * rings + 1` sweep the top hemisphere from its
/// equator to the `+Y` pole. Both arms are evaluated and the table indexed, so
/// the row's half is a selection rather than a branch.
fn row_shape(row: u32, rings: u32) -> (f32, f32) {
    let (r, k) = (rings as f32, row as f32);
    let top = usize::from(row > rings);
    let alpha = FRAC_PI_2 * [k / r, (k - r - 1.0) / r][top];
    (
        [alpha.sin(), alpha.cos()][top],
        [-alpha.cos(), alpha.sin()][top],
    )
}

/// Stitch the row grid into quads.
///
/// Three bands would otherwise be degenerate and are dropped: the half-quad that
/// spans the `-Y` pole row, the half-quad that spans the `+Y` pole row, and —
/// when `half_height` is zero — the entire mid-section band, whose two rows then
/// coincide.
fn band_indices(rings: u32, segments: u32, half_height: f32) -> Vec<u32> {
    let cols = segments + 1;
    let bands = 2 * rings + 1;
    (0..bands)
        .flat_map(move |band| {
            let spans_mid_section = (band != rings) | (half_height > 0.0);
            let lower = spans_mid_section & (row_shape(band, rings).0 > 0.0);
            let upper = spans_mid_section & (row_shape(band + 1, rings).0 > 0.0);
            (0..segments).flat_map(move |j| {
                let (a, b) = (band * cols + j, band * cols + j + 1);
                let (c, d) = (a + cols, b + cols);
                lower
                    .then_some([a, c, b])
                    .into_iter()
                    .chain(upper.then_some([b, c, d]))
            })
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(v: f32) -> Meters {
        Meters::finite_or_zero(v)
    }

    fn build(radius: f32, half_height: f32, rings: u32, segments: u32) -> Mesh {
        capsule(
            m(radius),
            m(half_height),
            Rings::new(rings).unwrap(),
            Segments::new(segments).unwrap(),
        )
        .unwrap()
    }

    fn assert_ccw_outward(mesh: &Mesh) {
        let p = mesh.positions();
        let n = mesh.normals();
        for t in mesh.indices().chunks(3) {
            let (i, j, k) = (t[0] as usize, t[1] as usize, t[2] as usize);
            let geometric = p[j].subtract(p[i]).cross(p[k].subtract(p[i]));
            let outward = n[i].add(n[j]).add(n[k]);
            assert!(
                geometric.dot(outward) > 0.0,
                "triangle {t:?} is not CCW-outward"
            );
        }
    }

    /// The distance from a point to the capsule's spine: the segment from
    /// `-half_height` to `+half_height` on the `Y` axis.
    fn spine_distance(p: Vec3, half_height: f32) -> f32 {
        let y = p.y.clamp(-half_height, half_height);
        p.subtract(Vec3::new(0.0, y, 0.0)).length()
    }

    #[test]
    fn a_two_ring_capsule_has_exact_counts() {
        let c = build(1.0, 2.0, 2, 8);
        // (2 * rings + 2) rows x (segments + 1) columns.
        assert_eq!(c.vertex_count(), 6 * 9);
        // 5 bands x 2 triangles x 8 segments, less one half-band at each pole.
        assert_eq!(c.triangle_count(), 4 * 2 * 8);
        assert!(c.has_normals() & c.has_uvs());
        assert!(c.tangents().is_empty() & c.colors().is_empty());
        assert_ccw_outward(&c);
    }

    #[test]
    fn every_vertex_is_exactly_radius_from_the_spine() {
        let c = build(1.5, 3.0, 4, 12);
        for (i, p) in c.positions().iter().enumerate() {
            let d = spine_distance(*p, 3.0);
            assert!((d - 1.5).abs() < 1.0e-4, "vertex {i} is {d} from the spine");
        }
    }

    #[test]
    fn the_total_height_is_twice_half_height_plus_radius() {
        let c = build(1.5, 3.0, 4, 12);
        let lowest = c.positions().iter().map(|p| p.y).fold(f32::MAX, f32::min);
        let highest = c.positions().iter().map(|p| p.y).fold(f32::MIN, f32::max);
        assert!((lowest + 4.5).abs() < 1.0e-4);
        assert!((highest - 4.5).abs() < 1.0e-4);
        // v spans the full height, 0 at the bottom pole and 1 at the top.
        assert!(c.uvs().iter().all(|uv| (-1.0e-5..=1.000_01).contains(&uv.y)));
        assert!(c.uvs()[0].y.abs() < 1.0e-5);
        assert!((c.uvs()[c.vertex_count() - 1].y - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn the_mid_section_is_a_cylinder_with_horizontal_normals() {
        let c = build(2.0, 5.0, 3, 8);
        let cols = 9usize;
        // Rows `rings` and `rings + 1` are the two equator rows.
        for k in 3 * cols..5 * cols {
            let n = c.normals()[k];
            assert!(n.y.abs() < 1.0e-6, "equator normal {k} is not horizontal");
            let p = c.positions()[k];
            assert!((p.y.abs() - 5.0).abs() < 1.0e-4, "equator y {}", p.y);
        }
    }

    #[test]
    fn hemisphere_normals_are_radial_about_their_own_centre() {
        let c = build(2.0, 5.0, 3, 8);
        for (p, n) in c.positions().iter().zip(c.normals()) {
            let centre = Vec3::new(0.0, p.y.clamp(-5.0, 5.0), 0.0);
            let radial = p.subtract(centre).normalize().unwrap();
            assert!(n.subtract(radial).length() < 1.0e-5);
            assert!((n.length() - 1.0).abs() < 1.0e-5);
        }
    }

    /// A capsule with no mid-section is a sphere — and its degenerate
    /// zero-height band is dropped, not emitted.
    #[test]
    fn a_zero_half_height_capsule_is_a_sphere() {
        let c = build(2.0, 0.0, 2, 8);
        assert_eq!(c.vertex_count(), 6 * 9);
        for (i, p) in c.positions().iter().enumerate() {
            let d = p.length();
            assert!((d - 2.0).abs() < 1.0e-4, "vertex {i} at {d} is off the sphere");
        }
        // The mid band is gone: exactly one band's worth of quads fewer than the
        // same capsule with height, and no degenerate triangle survives.
        let tall = build(2.0, 1.0, 2, 8);
        assert_eq!(tall.triangle_count() - c.triangle_count(), 2 * 8);
        assert_ccw_outward(&c);
        let p = c.positions();
        for t in c.indices().chunks(3) {
            let (a, b, d) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
            assert!(b.subtract(a).cross(d.subtract(a)).length() > 1.0e-6);
        }
    }

    #[test]
    fn the_seam_column_is_duplicated() {
        let c = build(1.0, 1.0, 2, 8);
        let equator = 2 * 9;
        assert!(c.positions()[equator].distance(c.positions()[equator + 8]) < 1.0e-5);
        assert_eq!(c.uvs()[equator].x, 0.0);
        assert_eq!(c.uvs()[equator + 8].x, 1.0);
    }

    #[test]
    fn a_non_positive_radius_is_rejected() {
        let (r, s) = (Rings::new(2).unwrap(), Segments::new(8).unwrap());
        assert_eq!(
            capsule(m(0.0), m(1.0), r, s).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            capsule(m(-1.0), m(1.0), r, s).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn a_negative_half_height_is_rejected() {
        let (r, s) = (Rings::new(2).unwrap(), Segments::new(8).unwrap());
        assert_eq!(
            capsule(m(1.0), m(-0.5), r, s).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn generation_is_deterministic() {
        assert_eq!(build(1.0, 2.0, 3, 10), build(1.0, 2.0, 3, 10));
    }
}
