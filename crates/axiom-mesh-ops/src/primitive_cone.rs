//! The right circular cone about `+Y`.

use axiom_kernel::Meters;
use axiom_mesh::{Mesh, MeshResult};

use crate::cap_policy::CapPolicy;
use crate::primitive_frustum::frustum;
use crate::tessellation::Segments;

/// A right circular cone about the `+Y` axis, centred on the origin.
///
/// The base circle of `radius` sits at `-half_height` and the apex at
/// `+half_height`. The apex is a *ring* of coincident positions — one per
/// segment plus the duplicated seam — so every meridian carries its own normal
/// and its own `u`, which is what stops the tip from shading as a single
/// averaged spike.
///
/// # Side normals are slant normals
///
/// This is the classic bug in this family: a cone's outward normal is **not**
/// the horizontal radial direction. It is
///
/// ```text
/// n(theta) ~ ( 2h cos theta,  radius,  2h sin theta )   normalized
/// ```
///
/// which always has a strictly positive `y` component — the surface leans
/// inward as it rises, so its normal leans upward. A cone *is* the frustum whose
/// top radius is zero, so this generator delegates to [`frustum`] and inherits
/// the correct slant normal by construction rather than by re-derivation.
///
/// # Caps
///
/// Only `caps.caps_start()` is meaningful: it selects the base disc.
/// **`caps.caps_end()` is ignored** — the top of a cone is the apex, a point,
/// and a fan of degenerate triangles is not a cap.
///
/// # Errors
///
/// [`axiom_mesh::MeshErrorCode::InvalidParameter`] when `radius` or
/// `half_height` is not strictly positive.
pub fn cone(
    radius: Meters,
    half_height: Meters,
    segments: Segments,
    caps: CapPolicy,
) -> MeshResult<Mesh> {
    frustum(
        radius,
        Meters::finite_or_zero(0.0),
        half_height,
        segments,
        caps,
    )
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
    fn a_capped_cone_has_a_base_fan_and_an_apex_fan() {
        let c = cone(m(2.0), m(3.0), seg(8), CapPolicy::Both).unwrap();
        // 9 base ring + 9 apex ring + (8 rim + 1 centre) for the base disc.
        assert_eq!(c.vertex_count(), 9 + 9 + 9);
        // One side triangle per segment, one base-disc triangle per segment.
        assert_eq!(c.triangle_count(), 8 + 8);
        assert_ccw_outward(&c);
        assert!(c.has_normals() & c.has_uvs());
    }

    #[test]
    fn the_apex_ring_collapses_onto_the_axis() {
        let c = cone(m(2.0), m(3.0), seg(8), CapPolicy::None).unwrap();
        for i in 0..9 {
            let p = c.positions()[i];
            assert!(((p.x * p.x + p.z * p.z).sqrt() - 2.0).abs() < 1.0e-4);
            assert!((p.y + 3.0).abs() < 1.0e-6);
        }
        for i in 9..18 {
            assert_eq!(c.positions()[i], Vec3::new(0.0, 3.0, 0.0));
        }
        // Each apex vertex still carries its own u, so the tip is not a seam.
        assert_eq!(c.uvs()[9], Vec2::new(0.0, 1.0));
        assert_eq!(c.uvs()[17], Vec2::new(1.0, 1.0));
    }

    /// The counterpart of the cylinder's `y == 0` proof: a cone's side normal
    /// leans upward by exactly `radius / hypot(2h, radius)` and is never radial.
    #[test]
    fn side_normals_are_slant_normals_not_radial_ones() {
        let c = cone(m(2.0), m(3.0), seg(16), CapPolicy::None).unwrap();
        let expected_y = 2.0 / (6.0f32 * 6.0 + 2.0 * 2.0).sqrt();
        for (i, n) in c.normals().iter().enumerate() {
            assert!((n.length() - 1.0).abs() < 1.0e-5, "normal {i} not unit");
            assert!(n.y > 0.0, "normal {i} has no upward lean: {}", n.y);
            assert!((n.y - expected_y).abs() < 1.0e-5, "normal {i} y {}", n.y);
        }
        // A radial normal would have y == 0 — assert we did NOT emit that.
        assert!(c.normals().iter().all(|n| n.y != 0.0));
    }

    #[test]
    fn a_flatter_cone_leans_its_normals_further_up() {
        let tall = cone(m(1.0), m(4.0), seg(8), CapPolicy::None).unwrap();
        let flat = cone(m(4.0), m(1.0), seg(8), CapPolicy::None).unwrap();
        assert!(flat.normals()[0].y > tall.normals()[0].y);
    }

    /// `caps_end` names the apex, which cannot be capped, so it changes nothing.
    #[test]
    fn the_end_cap_is_ignored_and_the_start_cap_is_the_base_disc() {
        let open = cone(m(2.0), m(3.0), seg(8), CapPolicy::None).unwrap();
        let end = cone(m(2.0), m(3.0), seg(8), CapPolicy::End).unwrap();
        assert_eq!(open, end);
        let start = cone(m(2.0), m(3.0), seg(8), CapPolicy::Start).unwrap();
        let both = cone(m(2.0), m(3.0), seg(8), CapPolicy::Both).unwrap();
        assert_eq!(start, both);
        assert_eq!(start.triangle_count() - open.triangle_count(), 8);
        // The base disc faces straight down.
        assert_eq!(
            start.normals()[start.vertex_count() - 1],
            Vec3::new(0.0, -1.0, 0.0)
        );
        assert_ccw_outward(&start);
    }

    #[test]
    fn a_non_positive_radius_is_rejected() {
        assert_eq!(
            cone(m(0.0), m(1.0), seg(8), CapPolicy::Both)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            cone(m(-2.0), m(1.0), seg(8), CapPolicy::Both)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn a_non_positive_half_height_is_rejected() {
        assert_eq!(
            cone(m(1.0), m(0.0), seg(8), CapPolicy::Both)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            cone(m(1.0), m(-3.0), seg(8), CapPolicy::Both)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
    }
}
