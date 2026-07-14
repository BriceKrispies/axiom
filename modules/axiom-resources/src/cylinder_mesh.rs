//! The built-in deterministic unit cylinder mesh, as neutral geometry.
//!
//! Like [`crate::sphere_mesh`], this is a *generator* producing plain
//! `(position, normal, uv, color)` vertices + a triangle index list. A radius-0.5
//! cylinder of height 1 (±0.5 on Y, so it matches the unit cube/sphere extent),
//! with a radially-normalled side wall and two axially-normalled end caps. Built
//! branchlessly with `map`/`flat_map`/`chain` over the segment ranges — no
//! `for`/`if`. A cap's flat normal vs. the wall's radial normal is selected by
//! `1 - |ny|` arithmetic, not a branch.

use core::f32::consts::TAU;

use crate::mesh_data::MeshInputVertex;

/// Radial segments around the axis.
const SEGMENTS: u32 = 16;
/// Radius (half the unit cube's width).
const RADIUS: f32 = 0.5;
/// Half height (the cylinder spans ±0.5 on Y).
const HALF_H: f32 = 0.5;
const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

/// One ring of `SEGMENTS+1` vertices at height `y`. `ny` is the Y component of
/// the normal: `0` for the side wall (normal is radial `(cos, 0, sin)`), `±1` for
/// a cap (normal is axial `(0, ±1, 0)`). The `1 - |ny|` factor zeroes the radial
/// part on a cap branchlessly.
fn ring(y: f32, ny: f32, v: f32) -> Vec<MeshInputVertex> {
    let radial = 1.0 - ny.abs();
    (0..=SEGMENTS)
        .map(|seg| {
            let u = seg as f32 / SEGMENTS as f32;
            let theta = u * TAU;
            let (c, s) = (theta.cos(), theta.sin());
            (
                [c * RADIUS, y, s * RADIUS],
                [c * radial, ny, s * radial],
                [u, v],
                WHITE,
            )
        })
        .collect()
}

/// Build the deterministic unit cylinder: a side wall plus two cap fans.
/// `4*(SEGMENTS+1) + 2` vertices and `SEGMENTS*12` indices.
pub(crate) fn build_cylinder_mesh() -> (Vec<MeshInputVertex>, Vec<u32>) {
    let stride = SEGMENTS + 1;
    let top_side = ring(HALF_H, 0.0, 0.0);
    let bottom_side = ring(-HALF_H, 0.0, 1.0);
    let top_cap = ring(HALF_H, 1.0, 0.0);
    let bottom_cap = ring(-HALF_H, -1.0, 1.0);
    let top_center: MeshInputVertex = ([0.0, HALF_H, 0.0], [0.0, 1.0, 0.0], [0.5, 0.5], WHITE);
    let bottom_center: MeshInputVertex = ([0.0, -HALF_H, 0.0], [0.0, -1.0, 0.0], [0.5, 0.5], WHITE);

    let vertices: Vec<MeshInputVertex> = top_side
        .into_iter()
        .chain(bottom_side)
        .chain(top_cap)
        .chain(bottom_cap)
        .chain([top_center, bottom_center])
        .collect();

    // Vertex-block base offsets in the concatenated list above.
    let off_top = 0;
    let off_bottom = stride;
    let off_top_cap = 2 * stride;
    let off_bottom_cap = 3 * stride;
    let top_center_idx = 4 * stride;
    let bottom_center_idx = 4 * stride + 1;

    // Side wall: two triangles per segment between the top and bottom rings.
    let wall = (0..SEGMENTS).flat_map(move |seg| {
        let (t0, t1) = (off_top + seg, off_top + seg + 1);
        let (b0, b1) = (off_bottom + seg, off_bottom + seg + 1);
        [t0, b0, t1, t1, b0, b1]
    });
    // Top cap fan (center → ring), and bottom cap fan with reversed winding.
    let top_fan = (0..SEGMENTS)
        .flat_map(move |seg| [top_center_idx, off_top_cap + seg, off_top_cap + seg + 1]);
    let bottom_fan = (0..SEGMENTS).flat_map(move |seg| {
        [
            bottom_center_idx,
            off_bottom_cap + seg + 1,
            off_bottom_cap + seg,
        ]
    });

    let indices: Vec<u32> = wall.chain(top_fan).chain(bottom_fan).collect();
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_and_index_counts_match_the_topology() {
        let (vertices, indices) = build_cylinder_mesh();
        assert_eq!(vertices.len() as u32, 4 * (SEGMENTS + 1) + 2);
        assert_eq!(indices.len() as u32, SEGMENTS * 12);
    }

    #[test]
    fn cylinder_is_deterministic() {
        assert_eq!(build_cylinder_mesh(), build_cylinder_mesh());
    }

    #[test]
    fn indices_are_all_in_range() {
        let (vertices, indices) = build_cylinder_mesh();
        let n = vertices.len() as u32;
        assert!(indices.iter().all(|&i| i < n));
        assert!(!indices.is_empty());
    }

    #[test]
    fn side_normals_are_unit_and_radial_caps_are_axial() {
        let (vertices, _) = build_cylinder_mesh();
        // Every normal is unit length.
        assert!(vertices.iter().all(|(_, n, _, _)| {
            let len2 = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
            (len2 - 1.0).abs() < 1e-4
        }));
        // The two cap centers carry purely axial normals.
        let top_center = vertices[vertices.len() - 2];
        let bottom_center = vertices[vertices.len() - 1];
        assert_eq!(top_center.1, [0.0, 1.0, 0.0]);
        assert_eq!(bottom_center.1, [0.0, -1.0, 0.0]);
    }
}
