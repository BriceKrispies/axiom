//! The axis-aligned box primitive, and the cube that specializes it.
//!
//! A box is six independent quads, not eight shared corners. Sharing corner
//! vertices between faces would force one averaged normal per corner and shade
//! a box like a rounded blob; hard creases are the whole point of the shape. So
//! the six faces are authored as an explicit table and each contributes its own
//! four vertices, its own outward normal, and its own full `0..1` UV chart.

use axiom_kernel::Meters;
use axiom_math::{Vec2, Vec3};
use axiom_mesh::{Mesh, MeshError, MeshErrorCode, MeshResult, MeshStreams};

/// One face of the box: four corners in counter-clockwise order seen from
/// outside, the outward normal they share, and the UV each corner carries.
///
/// Corners are signs of the half-extents (`-1` / `+1`), so the same table
/// generates any box. The UVs are attached per corner rather than per corner
/// *index* because the `+Y` and `-Y` faces map `u` to `+X` and `v` to `+Z`,
/// which is a different cycle around the quad than the four upright faces use.
struct BoxFace {
    corners: [[f32; 3]; 4],
    normal: [f32; 3],
    uvs: [[f32; 2]; 4],
}

/// The six faces, in `+X, -X, +Y, -Y, +Z, -Z` order.
///
/// On the four upright faces `v` increases with `+Y` and `u` with the face's
/// rightward axis; on the two horizontal faces `u` increases with `+X` and `v`
/// with `+Z`, matching [`crate::quad`].
const FACES: [BoxFace; 6] = [
    BoxFace {
        corners: [
            [1.0, -1.0, 1.0],
            [1.0, -1.0, -1.0],
            [1.0, 1.0, -1.0],
            [1.0, 1.0, 1.0],
        ],
        normal: [1.0, 0.0, 0.0],
        uvs: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    },
    BoxFace {
        corners: [
            [-1.0, -1.0, -1.0],
            [-1.0, -1.0, 1.0],
            [-1.0, 1.0, 1.0],
            [-1.0, 1.0, -1.0],
        ],
        normal: [-1.0, 0.0, 0.0],
        uvs: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    },
    BoxFace {
        corners: [
            [-1.0, 1.0, -1.0],
            [-1.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, -1.0],
        ],
        normal: [0.0, 1.0, 0.0],
        uvs: [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
    },
    BoxFace {
        corners: [
            [-1.0, -1.0, -1.0],
            [1.0, -1.0, -1.0],
            [1.0, -1.0, 1.0],
            [-1.0, -1.0, 1.0],
        ],
        normal: [0.0, -1.0, 0.0],
        uvs: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    },
    BoxFace {
        corners: [
            [-1.0, -1.0, 1.0],
            [1.0, -1.0, 1.0],
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0],
        ],
        normal: [0.0, 0.0, 1.0],
        uvs: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    },
    BoxFace {
        corners: [
            [1.0, -1.0, -1.0],
            [-1.0, -1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [1.0, 1.0, -1.0],
        ],
        normal: [0.0, 0.0, -1.0],
        uvs: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    },
];

/// The vertex count: six faces of four unshared corners.
const VERTEX_COUNT: usize = 24;

/// An axis-aligned box centred on the origin, spanning `±half_extents`.
///
/// Twenty-four vertices (four per face, hard-creased), thirty-six indices,
/// twelve counter-clockwise triangles, one outward normal per face, and a full
/// `0..1` UV chart on every face.
///
/// # Errors
///
/// [`MeshErrorCode::InvalidParameter`] when any half-extent is zero, negative,
/// or non-finite — each of which would collapse or unbound the solid.
pub fn box_mesh(half_extents: Vec3) -> MeshResult<Mesh> {
    let e = half_extents;
    // NaN fails `> 0.0`, and infinity fails `is_finite`, so the two tests
    // together admit exactly the finite positive extents.
    ((e.x > 0.0) & (e.y > 0.0) & (e.z > 0.0) & finite(e))
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "a box needs strictly positive, finite half-extents",
            )
        })
        .and_then(|()| {
            Mesh::from_streams(MeshStreams {
                normals: (0..VERTEX_COUNT)
                    .map(|k| {
                        let n = FACES[k / 4].normal;
                        Vec3::new(n[0], n[1], n[2])
                    })
                    .collect(),
                uvs: (0..VERTEX_COUNT)
                    .map(|k| {
                        let t = FACES[k / 4].uvs[k % 4];
                        Vec2::new(t[0], t[1])
                    })
                    .collect(),
                ..MeshStreams::new(
                    (0..VERTEX_COUNT)
                        .map(|k| {
                            let c = FACES[k / 4].corners[k % 4];
                            Vec3::new(c[0] * e.x, c[1] * e.y, c[2] * e.z)
                        })
                        .collect(),
                    (0..6_u32)
                        .flat_map(|face| [0_u32, 1, 2, 0, 2, 3].map(|corner| face * 4 + corner))
                        .collect(),
                )
            })
        })
}

/// A box with all three half-extents equal.
///
/// # Errors
///
/// [`MeshErrorCode::InvalidParameter`] when `half_extent` is zero or negative.
pub fn cube(half_extent: Meters) -> MeshResult<Mesh> {
    let h = half_extent.get();
    box_mesh(Vec3::new(h, h, h))
}

const fn finite(v: Vec3) -> bool {
    v.x.is_finite() & v.y.is_finite() & v.z.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_cube() -> Mesh {
        cube(Meters::new(1.0).unwrap()).unwrap()
    }

    #[test]
    fn a_cube_is_twenty_four_hard_creased_vertices_and_twelve_triangles() {
        let m = unit_cube();
        assert_eq!(m.vertex_count(), 24);
        assert_eq!(m.triangle_count(), 12);
        assert_eq!(m.indices().len(), 36);
        assert_eq!(m.normals().len(), 24);
        assert_eq!(m.uvs().len(), 24);
        assert!(m.tangents().is_empty());
        assert!(m.colors().is_empty());
        // Every corner is a signed unit corner, and all eight are present.
        assert!(m
            .positions()
            .iter()
            .all(|p| (p.x.abs() == 1.0) & (p.y.abs() == 1.0) & (p.z.abs() == 1.0)));
    }

    #[test]
    fn the_six_face_normals_are_exactly_the_signed_axes() {
        let m = unit_cube();
        let mut seen: Vec<Vec3> = Vec::new();
        for n in m.normals() {
            if !seen.contains(n) {
                seen.push(*n);
            }
        }
        assert_eq!(seen.len(), 6);
        for axis in [
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 0.0, -1.0),
        ] {
            assert!(seen.contains(&axis), "missing face normal {axis:?}");
        }
        // Four vertices share each face normal.
        for axis in &seen {
            assert_eq!(m.normals().iter().filter(|n| *n == axis).count(), 4);
        }
    }

    #[test]
    fn every_triangle_winds_counter_clockwise_outward() {
        // The CCW proof: the geometric normal of each triangle points away from
        // the (interior) origin, and agrees with the stored vertex normal.
        let m = box_mesh(Vec3::new(2.0, 0.5, 3.0)).unwrap();
        let p = m.positions();
        for t in m.indices().chunks(3) {
            let (a, b, c) = (p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]);
            let geometric = b.subtract(a).cross(c.subtract(a));
            let centroid = a.add(b).add(c).mul_scalar(1.0 / 3.0);
            assert!(
                geometric.dot(centroid) > 0.0,
                "triangle {t:?} faces the origin"
            );
            assert!(geometric.dot(m.normals()[t[0] as usize]) > 0.0);
        }
    }

    #[test]
    fn every_face_carries_a_full_unit_uv_chart() {
        let m = unit_cube();
        for face in 0..6 {
            let mut chart: Vec<Vec2> = m.uvs()[face * 4..face * 4 + 4].to_vec();
            chart.sort_by(|a, b| (a.x, a.y).partial_cmp(&(b.x, b.y)).unwrap());
            assert_eq!(
                chart,
                vec![
                    Vec2::new(0.0, 0.0),
                    Vec2::new(0.0, 1.0),
                    Vec2::new(1.0, 0.0),
                    Vec2::new(1.0, 1.0),
                ],
                "face {face} is not a unit chart"
            );
        }
    }

    #[test]
    fn a_box_scales_each_axis_independently() {
        let m = box_mesh(Vec3::new(2.0, 0.5, 3.0)).unwrap();
        assert!(m
            .positions()
            .iter()
            .all(|p| (p.x.abs() == 2.0) & (p.y.abs() == 0.5) & (p.z.abs() == 3.0)));
    }

    #[test]
    fn a_cube_is_the_box_with_three_equal_extents() {
        assert_eq!(
            cube(Meters::new(1.5).unwrap()).unwrap(),
            box_mesh(Vec3::new(1.5, 1.5, 1.5)).unwrap()
        );
    }

    #[test]
    fn zero_negative_or_non_finite_half_extents_are_rejected() {
        for bad in [
            Vec3::new(0.0, 1.0, 1.0),
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(-1.0, 1.0, 1.0),
            Vec3::new(f32::NAN, 1.0, 1.0),
            Vec3::new(1.0, f32::INFINITY, 1.0),
        ] {
            assert_eq!(
                box_mesh(bad).unwrap_err().code(),
                MeshErrorCode::InvalidParameter,
                "accepted {bad:?}"
            );
        }
    }

    #[test]
    fn a_non_positive_cube_extent_is_rejected() {
        assert_eq!(
            cube(Meters::new(0.0).unwrap()).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            cube(Meters::new(-2.0).unwrap()).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
    }
}
