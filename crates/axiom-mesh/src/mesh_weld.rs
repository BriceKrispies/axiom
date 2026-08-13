//! Deterministic vertex welding and degenerate-triangle removal.
//!
//! # Welding compares positions, and nothing else
//!
//! [`weld`] merges vertices that occupy the same place. It does **not** look at
//! normals, UVs, colours, or skin bindings — only at distance. That has a
//! consequence every caller has to know about:
//!
//! > **Welding destroys seams.** A UV seam, a hard shading edge, and a colour
//! > discontinuity are all encoded the same way: several vertices at the *same
//! > position* carrying *different* attributes. Welding collapses them into one
//! > and the first one encountered keeps its attributes, so the seam disappears
//! > and the texture wraps or the crease smooths.
//!
//! That is exactly what you want after a boolean, a marching-cubes extraction, a
//! naive triangle-soup import, or a per-triangle generator — meshes whose
//! duplicate vertices are noise. It is exactly what you do not want on a
//! UV-unwrapped, hard-edged authored asset. **If a mesh's seams are meaningful,
//! do not weld it**; use [`remove_degenerate_triangles`] alone, which touches no
//! vertices at all.
//!
//! # Why the spatial hash is a `BTreeMap`
//!
//! Welding is O(n²) if written naively, so candidates are found by quantising
//! each position onto an integer lattice of cell side `tolerance` and consulting
//! the cell and its 26 neighbours (a vertex within `tolerance` of another can
//! never be more than one cell away). The lattice map is a [`BTreeMap`] rather
//! than a hash map because hash-map iteration order is unspecified and would
//! make the output depend on it. Determinism is nailed down twice over: the map
//! is ordered, and among all candidates within tolerance the merge always keeps
//! the one with the **lowest original vertex index**, so the surviving vertex —
//! and therefore which attributes survive — is a pure function of the input,
//! independent of any traversal order.

use std::collections::BTreeMap;

use axiom_kernel::Meters;
use axiom_math::Vec3;

use crate::mesh::Mesh;
use crate::mesh_error::MeshError;
use crate::mesh_error_code::MeshErrorCode;
use crate::mesh_result::MeshResult;
use crate::mesh_streams::MeshStreams;

/// The integer lattice cell a position quantises into.
type Cell = [i64; 3];

/// The cell plus its 26 neighbours: the complete candidate set for a merge,
/// since the lattice side equals the merge tolerance.
const NEIGHBORHOOD: i64 = 27;

/// Merge vertices whose positions lie within `tolerance` of one another.
///
/// The surviving vertex of each merged group is the one with the lowest index in
/// the original mesh, and it keeps its own attributes; every index that pointed
/// at a merged vertex is remapped onto it. Triangles that collapse to a line or
/// a point in the process (two or more corners now sharing an index) are
/// dropped. Attribute streams are carried across for exactly the vertices that
/// survive, so a stream that was present stays present and correctly sized.
///
/// **This erases UV seams and hard shading edges** — see the module
/// documentation. Welding an authored, unwrapped asset is usually a mistake.
///
/// # Errors
///
/// [`MeshErrorCode::InvalidParameter`] when `tolerance` is zero or negative: a
/// zero-radius merge has no lattice to quantise onto, and "within a negative
/// distance" is not a relation.
pub fn weld(mesh: &Mesh, tolerance: Meters) -> MeshResult<Mesh> {
    (tolerance.get() > 0.0)
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "a weld tolerance must be greater than zero",
            )
        })
        .and_then(|()| Mesh::from_streams(welded_streams(mesh, tolerance.get())))
}

/// Drop every triangle whose area is at most `tolerance` squared, or whose
/// corners are not three distinct vertices.
///
/// A degenerate triangle is invisible but not harmless: it produces `NaN`
/// geometric normals, breaks tangent generation, and wastes an index triple in
/// every buffer downstream. This removes them from the index buffer and
/// **leaves the vertex streams completely untouched** — no vertex is deleted and
/// no vertex is renumbered, so any index a caller is holding, and any parallel
/// per-vertex data the caller keeps outside the mesh, stays valid. (Removing the
/// now-unreferenced vertices as well would be a compaction, which is [`weld`]'s
/// job, not this one's.)
///
/// Removing every triangle is a legal outcome: a mesh with vertices and an empty
/// index buffer satisfies the mesh contract.
///
/// # Errors
///
/// [`MeshErrorCode::InvalidParameter`] when `tolerance` is negative. Zero is
/// allowed and means "drop only exactly-zero-area triangles"; a negative value
/// is not a threshold at all, and squaring it would silently turn it into a
/// positive one.
pub fn remove_degenerate_triangles(mesh: &Mesh, tolerance: Meters) -> MeshResult<Mesh> {
    (tolerance.get() >= 0.0)
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidParameter,
                "a degenerate-area tolerance must not be negative",
            )
        })
        .and_then(|()| {
            Mesh::from_streams(MeshStreams {
                normals: mesh.normals().to_vec(),
                uvs: mesh.uvs().to_vec(),
                tangents: mesh.tangents().to_vec(),
                colors: mesh.colors().to_vec(),
                joints: mesh.joints().to_vec(),
                weights: mesh.weights().to_vec(),
                ..MeshStreams::new(
                    mesh.positions().to_vec(),
                    substantial_triangles(mesh, tolerance.get()),
                )
            })
        })
}

/// The index buffer with degenerate triangles removed.
fn substantial_triangles(mesh: &Mesh, tolerance: f32) -> Vec<u32> {
    let positions = mesh.positions();
    let threshold = tolerance * tolerance;
    mesh.indices()
        .chunks_exact(3)
        .filter(|corners| distinct(corners))
        .filter(|corners| triangle_area(positions, corners) > threshold)
        .flatten()
        .copied()
        .collect()
}

/// Twice-the-cross-product area of one triangle, halved.
fn triangle_area(positions: &[Vec3], corners: &[u32]) -> f32 {
    let a = positions[corners[0] as usize];
    let edge_1 = positions[corners[1] as usize].subtract(a);
    let edge_2 = positions[corners[2] as usize].subtract(a);
    edge_1.cross(edge_2).length() * 0.5
}

/// Whether a triangle's three corners address three different vertices.
fn distinct(corners: &[u32]) -> bool {
    (corners[0] != corners[1]) & (corners[1] != corners[2]) & (corners[0] != corners[2])
}

/// Every stream of the welded mesh.
fn welded_streams(mesh: &Mesh, tolerance: f32) -> MeshStreams {
    let (remap, keep) = weld_scan(mesh.positions(), tolerance);
    let indices: Vec<u32> = mesh
        .indices()
        .iter()
        .map(|&index| remap[index as usize])
        .collect();
    MeshStreams {
        normals: picked(mesh.normals(), &keep),
        uvs: picked(mesh.uvs(), &keep),
        tangents: picked(mesh.tangents(), &keep),
        colors: picked(mesh.colors(), &keep),
        joints: picked(mesh.joints(), &keep),
        weights: picked(mesh.weights(), &keep),
        ..MeshStreams::new(
            picked(mesh.positions(), &keep),
            indices
                .chunks_exact(3)
                .filter(|corners| distinct(corners))
                .flatten()
                .copied()
                .collect(),
        )
    }
}

/// Walk the vertices in index order, merging each into the lowest-indexed
/// surviving vertex within `tolerance`.
///
/// Returns `(remap, keep)`: `remap[old]` is the new index of every original
/// vertex, and `keep[new]` is the original vertex that new index came from.
fn weld_scan(positions: &[Vec3], tolerance: f32) -> (Vec<u32>, Vec<u32>) {
    let mut buckets: BTreeMap<Cell, Vec<u32>> = BTreeMap::new();
    let mut remap: Vec<u32> = Vec::with_capacity(positions.len());
    let mut keep: Vec<u32> = Vec::new();
    positions.iter().enumerate().for_each(|(index, &position)| {
        let cell = cell_of(position, tolerance);
        let merged_into = (0..NEIGHBORHOOD)
            .filter_map(|step| buckets.get(&neighbor(cell, step)))
            .flatten()
            .copied()
            .filter(|&candidate| positions[candidate as usize].distance(position) <= tolerance)
            .min();
        let new_index = merged_into.map_or_else(
            || {
                let fresh = keep.len() as u32;
                keep.push(index as u32);
                buckets.entry(cell).or_default().push(index as u32);
                fresh
            },
            |candidate| remap[candidate as usize],
        );
        remap.push(new_index);
    });
    (remap, keep)
}

/// The lattice cell a position falls in, for a lattice of side `tolerance`.
///
/// The float-to-integer cast saturates rather than wrapping, so a coordinate far
/// beyond the lattice's integer range lands in an extreme cell instead of
/// aliasing back onto the origin.
fn cell_of(position: Vec3, tolerance: f32) -> Cell {
    [
        (position.x / tolerance).floor() as i64,
        (position.y / tolerance).floor() as i64,
        (position.z / tolerance).floor() as i64,
    ]
}

/// The `step`-th of the 27 cells in `cell`'s closed neighbourhood.
fn neighbor(cell: Cell, step: i64) -> Cell {
    [
        cell[0] + step % 3 - 1,
        cell[1] + (step / 3) % 3 - 1,
        cell[2] + step / 9 - 1,
    ]
}

/// Carry one attribute stream across the surviving vertices, or leave it absent.
fn picked<T: Copy>(stream: &[T], keep: &[u32]) -> Vec<T> {
    keep.iter()
        .filter(|_| !stream.is_empty())
        .map(|&index| stream[index as usize])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Vec2, Vec4};

    fn meters(value: f32) -> Meters {
        Meters::new(value).unwrap()
    }

    /// Two triangles sharing an edge, but with the shared edge duplicated: six
    /// vertices where four distinct positions exist.
    fn seamed_quad() -> Mesh {
        Mesh::from_streams(MeshStreams {
            // The duplicated pair (1,3) and (2,4) carry DIFFERENT uvs: this is
            // exactly the seam that welding erases.
            uvs: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 0.0),
                Vec2::new(0.0, 1.0),
                Vec2::new(0.0, 0.0),
                Vec2::new(1.0, 1.0),
                Vec2::new(1.0, 1.0),
            ],
            ..MeshStreams::new(
                vec![
                    Vec3::new(0.0, 0.0, 0.0),
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.0, 0.0, 1.0),
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.0, 0.0, 1.0),
                    Vec3::new(1.0, 0.0, 1.0),
                ],
                vec![0, 1, 2, 3, 5, 4],
            )
        })
        .unwrap()
    }

    /// A triangle with every optional stream populated, plus a coincident
    /// duplicate of its first vertex.
    fn decorated_with_duplicate() -> Mesh {
        Mesh::from_streams(MeshStreams {
            normals: vec![Vec3::UNIT_Y; 4],
            uvs: vec![Vec2::new(0.5, 0.5); 4],
            tangents: vec![Vec4::new(1.0, 0.0, 0.0, 1.0); 4],
            colors: vec![Vec4::ONE; 4],
            joints: vec![[3, 0, 0, 0]; 4],
            weights: vec![[1.0, 0.0, 0.0, 0.0]; 4],
            ..MeshStreams::new(
                vec![
                    Vec3::ZERO,
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.0, 0.0, 1.0),
                    Vec3::new(1.0e-7, 0.0, 0.0),
                ],
                vec![0, 1, 2],
            )
        })
        .unwrap()
    }

    #[test]
    fn two_coincident_vertices_become_one() {
        let m = decorated_with_duplicate();
        assert_eq!(m.vertex_count(), 4);
        let out = weld(&m, meters(1.0e-4)).unwrap();
        assert_eq!(out.vertex_count(), 3);
        // The lowest-indexed vertex of the pair is the one that survived.
        assert_eq!(out.positions()[0], Vec3::ZERO);
        assert_eq!(out.positions()[1], Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(out.indices(), &[0, 1, 2]);
    }

    #[test]
    fn every_attribute_stream_follows_the_surviving_vertices() {
        let out = weld(&decorated_with_duplicate(), meters(1.0e-4)).unwrap();
        assert_eq!(out.normals(), &[Vec3::UNIT_Y; 3]);
        assert_eq!(out.uvs(), &[Vec2::new(0.5, 0.5); 3]);
        assert_eq!(out.tangents(), &[Vec4::new(1.0, 0.0, 0.0, 1.0); 3]);
        assert_eq!(out.colors(), &[Vec4::ONE; 3]);
        assert_eq!(out.joints(), &[[3, 0, 0, 0]; 3]);
        assert_eq!(out.weights(), &[[1.0, 0.0, 0.0, 0.0]; 3]);
    }

    #[test]
    fn a_mesh_with_no_optional_streams_welds_to_one_without_them() {
        let m = Mesh::from_streams(MeshStreams::new(
            vec![Vec3::ZERO, Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Z],
            vec![0, 2, 3, 1, 2, 3],
        ))
        .unwrap();
        let out = weld(&m, meters(1.0e-4)).unwrap();
        assert_eq!(out.vertex_count(), 3);
        assert!(!out.has_normals());
        assert!(!out.has_uvs());
        assert!(!out.has_tangents());
        assert!(!out.has_colors());
        assert!(!out.is_skinned());
        // Both triangles now name the same three vertices.
        assert_eq!(out.indices(), &[0, 1, 2, 0, 1, 2]);
    }

    #[test]
    fn welding_erases_a_uv_seam_and_the_first_vertex_wins() {
        let out = weld(&seamed_quad(), meters(1.0e-4)).unwrap();
        assert_eq!(seamed_quad().vertex_count(), 6);
        assert_eq!(out.vertex_count(), 4);
        // Vertex 3 (uv 0,0) merged into vertex 1 (uv 1,0) — the LOWER index kept
        // its attributes, so the seam's second uv is gone.
        assert_eq!(out.uvs()[1], Vec2::new(1.0, 0.0));
        assert_eq!(out.uvs()[2], Vec2::new(0.0, 1.0));
        assert_eq!(out.indices(), &[0, 1, 2, 1, 3, 2]);
    }

    #[test]
    fn a_tolerance_smaller_than_the_gap_merges_nothing() {
        let m = decorated_with_duplicate();
        let out = weld(&m, meters(1.0e-9)).unwrap();
        assert_eq!(out.vertex_count(), 4);
        assert_eq!(out, m);
    }

    #[test]
    fn welding_across_a_lattice_boundary_still_merges() {
        // 0.999999 and 1.000001 straddle the cell boundary at 1.0 for a
        // tolerance of 1.0e-3: only the neighbour scan finds this pair.
        let m = Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::new(0.999_999, 0.0, 0.0),
                Vec3::new(1.000_001, 0.0, 0.0),
                Vec3::new(5.0, 0.0, 0.0),
            ],
            vec![0, 1, 2],
        ))
        .unwrap();
        let out = weld(&m, meters(1.0e-3)).unwrap();
        assert_eq!(out.vertex_count(), 2);
        assert_eq!(out.positions()[0], Vec3::new(0.999_999, 0.0, 0.0));
    }

    #[test]
    fn a_triangle_that_collapses_while_welding_is_dropped() {
        // Vertices 1 and 2 are coincident, so triangle (0,1,2) becomes a line.
        let m = Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::ZERO,
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ],
            vec![0, 1, 2, 0, 2, 3],
        ))
        .unwrap();
        let out = weld(&m, meters(1.0e-4)).unwrap();
        assert_eq!(out.vertex_count(), 3);
        assert_eq!(out.triangle_count(), 1);
        assert_eq!(out.indices(), &[0, 1, 2]);
    }

    #[test]
    fn welding_may_legally_remove_every_triangle() {
        let m = Mesh::from_streams(MeshStreams::new(
            vec![Vec3::ZERO, Vec3::ZERO, Vec3::ZERO],
            vec![0, 1, 2],
        ))
        .unwrap();
        let out = weld(&m, meters(1.0e-4)).unwrap();
        assert_eq!(out.vertex_count(), 1);
        assert_eq!(out.triangle_count(), 0);
        assert!(out.indices().is_empty());
    }

    #[test]
    fn welding_is_deterministic_across_repeated_runs() {
        let a = weld(&seamed_quad(), meters(1.0e-4)).unwrap();
        let b = weld(&seamed_quad(), meters(1.0e-4)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn a_non_positive_weld_tolerance_is_rejected() {
        assert_eq!(
            weld(&seamed_quad(), meters(0.0)).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
        assert_eq!(
            weld(&seamed_quad(), meters(-1.0)).unwrap_err().code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn a_zero_area_triangle_is_removed_and_every_vertex_is_kept() {
        // Triangle 1 is collinear (zero area); triangle 0 is real.
        let m = Mesh::from_streams(MeshStreams {
            normals: vec![Vec3::UNIT_Y; 5],
            uvs: vec![Vec2::ZERO; 5],
            tangents: vec![Vec4::new(1.0, 0.0, 0.0, 1.0); 5],
            colors: vec![Vec4::ONE; 5],
            joints: vec![[0, 0, 0, 0]; 5],
            weights: vec![[1.0, 0.0, 0.0, 0.0]; 5],
            ..MeshStreams::new(
                vec![
                    Vec3::ZERO,
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.0, 0.0, 1.0),
                    Vec3::new(2.0, 0.0, 0.0),
                    Vec3::new(3.0, 0.0, 0.0),
                ],
                vec![0, 1, 2, 1, 3, 4],
            )
        })
        .unwrap();
        let out = remove_degenerate_triangles(&m, meters(0.0)).unwrap();
        assert_eq!(out.triangle_count(), 1);
        assert_eq!(out.indices(), &[0, 1, 2]);
        // No compaction: the orphaned vertices 3 and 4 are still addressable.
        assert_eq!(out.vertex_count(), 5);
        assert_eq!(out.positions()[4], Vec3::new(3.0, 0.0, 0.0));
        assert_eq!(out.normals().len(), 5);
        assert_eq!(out.uvs().len(), 5);
        assert_eq!(out.tangents().len(), 5);
        assert_eq!(out.colors().len(), 5);
        assert_eq!(out.joints().len(), 5);
        assert_eq!(out.weights().len(), 5);
    }

    #[test]
    fn a_repeated_corner_is_removed_whatever_the_tolerance() {
        let m = Mesh::from_streams(MeshStreams::new(
            vec![Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0)],
            vec![0, 1, 1, 0, 1, 2],
        ))
        .unwrap();
        let out = remove_degenerate_triangles(&m, meters(0.0)).unwrap();
        assert_eq!(out.indices(), &[0, 1, 2]);
    }

    #[test]
    fn the_tolerance_is_an_area_threshold_of_tolerance_squared() {
        // A right triangle with legs of 0.1 has area 0.005.
        let m = Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::ZERO,
                Vec3::new(0.1, 0.0, 0.0),
                Vec3::new(0.0, 0.0, 0.1),
            ],
            vec![0, 1, 2],
        ))
        .unwrap();
        // 0.07^2 = 0.0049 < 0.005 → kept.
        assert_eq!(
            remove_degenerate_triangles(&m, meters(0.07))
                .unwrap()
                .triangle_count(),
            1
        );
        // 0.08^2 = 0.0064 > 0.005 → dropped.
        assert_eq!(
            remove_degenerate_triangles(&m, meters(0.08))
                .unwrap()
                .triangle_count(),
            0
        );
    }

    #[test]
    fn removing_every_triangle_is_legal() {
        let m = Mesh::from_streams(MeshStreams::new(
            vec![Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0)],
            vec![0, 1, 2],
        ))
        .unwrap();
        let out = remove_degenerate_triangles(&m, meters(0.0)).unwrap();
        assert_eq!(out.triangle_count(), 0);
        assert!(out.indices().is_empty());
        assert_eq!(out.vertex_count(), 3);
    }

    #[test]
    fn a_negative_area_tolerance_is_rejected() {
        assert_eq!(
            remove_degenerate_triangles(&seamed_quad(), meters(-0.5))
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidParameter
        );
    }

    #[test]
    fn lattice_cells_and_their_neighbourhood_are_addressed_consistently() {
        assert_eq!(cell_of(Vec3::new(0.0, 0.0, 0.0), 1.0), [0, 0, 0]);
        assert_eq!(cell_of(Vec3::new(-0.5, 1.5, 2.999), 1.0), [-1, 1, 2]);
        // Step 13 is the centre of the 3x3x3 neighbourhood.
        assert_eq!(neighbor([4, 5, 6], 13), [4, 5, 6]);
        assert_eq!(neighbor([4, 5, 6], 0), [3, 4, 5]);
        assert_eq!(neighbor([4, 5, 6], 26), [5, 6, 7]);
    }
}
