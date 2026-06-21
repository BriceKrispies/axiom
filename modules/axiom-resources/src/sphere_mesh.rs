//! The built-in deterministic unit UV-sphere mesh, as neutral geometry.
//!
//! Like [`crate::cube_mesh`], this is a *generator* producing plain
//! `(position, normal, uv, color)` vertices + a triangle index list. It is a
//! latitude/longitude (UV) sphere of radius 0.5 (diameter 1, matching the unit
//! cube), with smooth per-vertex normals (the unit position). Built branchlessly
//! with `flat_map`/`map` over the ring/segment ranges — no `for`/`if`.

use core::f32::consts::{PI, TAU};

use crate::mesh_data::MeshInputVertex;

/// Latitude bands (poles to poles).
const RINGS: u32 = 12;
/// Longitude segments around the equator.
const SEGMENTS: u32 = 18;
/// Radius (half the unit cube's width, so the sphere matches the cube's size).
const RADIUS: f32 = 0.5;
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// Build the deterministic unit UV sphere: `(RINGS+1)*(SEGMENTS+1)` vertices and
/// `RINGS*SEGMENTS*6` indices.
pub(crate) fn build_sphere_mesh() -> (Vec<MeshInputVertex>, Vec<u32>) {
    let vertices: Vec<MeshInputVertex> = (0..=RINGS)
        .flat_map(|ring| {
            let v = ring as f32 / RINGS as f32;
            let phi = v * PI;
            (0..=SEGMENTS).map(move |seg| {
                let u = seg as f32 / SEGMENTS as f32;
                let theta = u * TAU;
                // Unit direction (also the smooth normal); position is scaled.
                let y = phi.cos();
                let ring_radius = phi.sin();
                let x = ring_radius * theta.cos();
                let z = ring_radius * theta.sin();
                (
                    [x * RADIUS, y * RADIUS, z * RADIUS],
                    [x, y, z],
                    [u, v],
                    WHITE,
                )
            })
        })
        .collect();

    let stride = SEGMENTS + 1;
    let indices: Vec<u32> = (0..RINGS)
        .flat_map(|ring| {
            (0..SEGMENTS).flat_map(move |seg| {
                let top_left = ring * stride + seg;
                let top_right = top_left + 1;
                let bottom_left = top_left + stride;
                let bottom_right = bottom_left + 1;
                // Two triangles per quad. Cull mode is disabled, so winding is
                // not load-bearing for visibility.
                [
                    top_left,
                    bottom_left,
                    top_right,
                    top_right,
                    bottom_left,
                    bottom_right,
                ]
            })
        })
        .collect();

    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sphere_vertex_and_index_counts_match_the_grid() {
        let (vertices, indices) = build_sphere_mesh();
        assert_eq!(vertices.len(), ((RINGS + 1) * (SEGMENTS + 1)) as usize);
        assert_eq!(indices.len(), (RINGS * SEGMENTS * 6) as usize);
    }

    #[test]
    fn sphere_is_deterministic() {
        assert_eq!(build_sphere_mesh(), build_sphere_mesh());
    }

    #[test]
    fn sphere_normals_are_unit_and_positions_on_the_radius() {
        let (vertices, _) = build_sphere_mesh();
        vertices.iter().for_each(|(pos, normal, _, _)| {
            let nlen =
                (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
            assert!((nlen - 1.0).abs() < 1e-4, "normal should be unit length");
            let plen = (pos[0] * pos[0] + pos[1] * pos[1] + pos[2] * pos[2]).sqrt();
            assert!(
                (plen - RADIUS).abs() < 1e-4,
                "position should lie on the radius"
            );
        });
    }

    #[test]
    fn sphere_indices_are_valid() {
        let (vertices, indices) = build_sphere_mesh();
        assert!(indices.iter().all(|&i| (i as usize) < vertices.len()));
    }
}
