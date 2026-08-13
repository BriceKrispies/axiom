//! The right circular cylinder about `+Y`.

use axiom_kernel::Meters;
use axiom_mesh::{Mesh, MeshResult};

use crate::cap_policy::CapPolicy;
use crate::primitive_frustum::frustum;
use crate::tessellation::Segments;

/// A right circular cylinder about the `+Y` axis, centred on the origin.
///
/// The side wall runs from `-half_height` to `+half_height` at a constant
/// `radius`, and `caps` selects the bottom (`caps_start`) and top (`caps_end`)
/// discs. Side normals are the **horizontal** radial direction — their `y`
/// component is exactly `0.0`, because a cylinder is the frustum whose two radii
/// are equal and the slant normal's vertical term is `bottom - top`.
///
/// UVs follow the family convention: `u` wraps `0..1` once around the axis with
/// the seam vertex duplicated, `v` is `0` at the bottom ring and `1` at the top.
///
/// A cylinder *is* the equal-radius [`frustum`], so this generator delegates
/// rather than re-deriving the same ring trigonometry. That is why the two can
/// never drift apart.
///
/// # Errors
///
/// [`axiom_mesh::MeshErrorCode::InvalidParameter`] when `radius` or
/// `half_height` is not strictly positive.
pub fn cylinder(
    radius: Meters,
    half_height: Meters,
    segments: Segments,
    caps: CapPolicy,
) -> MeshResult<Mesh> {
    frustum(radius, radius, half_height, segments, caps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Vec2, Vec3};
    use axiom_mesh::MeshErrorCode;

    fn m(v: f32) -> Meters {
        Meters::finite_or_zero(v)
    }

    fn seg(v: u32) -> Segments {
        Segments::new(v).unwrap()
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

    #[test]
    fn a_capped_cylinder_has_the_expected_counts_and_winding() {
        let c = cylinder(m(2.0), m(3.0), seg(8), CapPolicy::Both).unwrap();
        assert_eq!(c.vertex_count(), 2 * 9 + 2 * 9);
        assert_eq!(c.triangle_count(), 2 * 8 + 2 * 8);
        assert_ccw_outward(&c);
        assert!(c.has_normals() & c.has_uvs());
    }

    #[test]
    fn every_side_vertex_is_exactly_radius_from_the_axis() {
        let c = cylinder(m(2.0), m(3.0), seg(16), CapPolicy::None).unwrap();
        for (i, p) in c.positions().iter().enumerate() {
            let radial = (p.x * p.x + p.z * p.z).sqrt();
            assert!((radial - 2.0).abs() < 1.0e-4, "vertex {i} radius {radial}");
            assert!((p.y.abs() - 3.0).abs() < 1.0e-4, "vertex {i} height {}", p.y);
        }
    }

    /// The classic bug this family invites: a cylinder's side normal is purely
    /// horizontal, and a cone's is not (see `primitive_cone`).
    #[test]
    fn side_normals_are_horizontal_with_y_exactly_zero() {
        let c = cylinder(m(2.0), m(3.0), seg(16), CapPolicy::None).unwrap();
        for (i, n) in c.normals().iter().enumerate() {
            assert_eq!(n.y, 0.0, "side normal {i} is not horizontal");
            assert!((n.length() - 1.0).abs() < 1.0e-5);
        }
        // And each normal is the radial direction of its own vertex.
        for (p, n) in c.positions().iter().zip(c.normals()) {
            assert!((n.x - p.x / 2.0).abs() < 1.0e-5);
            assert!((n.z - p.z / 2.0).abs() < 1.0e-5);
        }
    }

    #[test]
    fn uvs_wrap_once_and_rise_with_y() {
        let c = cylinder(m(1.0), m(1.0), seg(8), CapPolicy::None).unwrap();
        assert_eq!(c.uvs()[0], Vec2::new(0.0, 0.0));
        assert_eq!(c.uvs()[8], Vec2::new(1.0, 0.0));
        assert_eq!(c.uvs()[9], Vec2::new(0.0, 1.0));
        assert_eq!(c.uvs()[17], Vec2::new(1.0, 1.0));
        // The duplicated seam vertices share a position but not a u.
        assert!(c.positions()[0].distance(c.positions()[8]) < 1.0e-5);
    }

    /// Capping adds exactly one fan triangle per segment at each end.
    #[test]
    fn caps_add_exactly_two_segments_worth_of_triangles() {
        let open = cylinder(m(1.0), m(1.0), seg(16), CapPolicy::None).unwrap();
        let closed = cylinder(m(1.0), m(1.0), seg(16), CapPolicy::Both).unwrap();
        assert_eq!(closed.triangle_count() - open.triangle_count(), 2 * 16);
        let start = cylinder(m(1.0), m(1.0), seg(16), CapPolicy::Start).unwrap();
        let end = cylinder(m(1.0), m(1.0), seg(16), CapPolicy::End).unwrap();
        assert_eq!(start.triangle_count() - open.triangle_count(), 16);
        assert_eq!(end.triangle_count() - open.triangle_count(), 16);
        // The start cap faces -Y and the end cap +Y.
        assert_eq!(start.normals()[start.vertex_count() - 1], Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(end.normals()[end.vertex_count() - 1], Vec3::new(0.0, 1.0, 0.0));
        assert_ccw_outward(&start);
        assert_ccw_outward(&end);
    }

    #[test]
    fn a_non_positive_radius_is_rejected() {
        assert_eq!(
            cylinder(m(0.0), m(1.0), seg(8), CapPolicy::Both)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            cylinder(m(-1.0), m(1.0), seg(8), CapPolicy::Both)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn a_non_positive_half_height_is_rejected() {
        assert_eq!(
            cylinder(m(1.0), m(0.0), seg(8), CapPolicy::Both)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            cylinder(m(1.0), m(-2.0), seg(8), CapPolicy::Both)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    /// A non-finite length cannot be built: `Meters` rejects it at the kernel
    /// boundary, so no generator here can ever see a `NaN` radius.
    #[test]
    fn a_non_finite_length_is_unrepresentable() {
        assert!(Meters::new(f32::NAN).is_err());
        assert!(Meters::new(f32::INFINITY).is_err());
    }
}
