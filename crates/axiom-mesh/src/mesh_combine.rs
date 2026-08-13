//! Concatenation of several meshes into one.
//!
//! # The attribute-reconciliation policy
//!
//! Combining is only half a concatenation problem. Positions and indices always
//! merge — the second mesh's indices shift up by the first mesh's vertex count
//! and the buffers run end to end. The optional streams do not, because the
//! inputs need not agree on which of them exist: one box may carry normals and
//! UVs while the next carries only positions.
//!
//! A [`crate::Mesh`] forbids a *partially populated* stream — a present stream is
//! exactly one entry per vertex, never a prefix — so the output cannot simply
//! keep whatever it found. Two policies are possible and this module commits,
//! explicitly, to the conservative one:
//!
//! > **An attribute stream appears in the output if, and only if, it is present
//! > in every input mesh.**
//!
//! The rejected alternative is to synthesize filler (zero normals, `(0,0)` UVs,
//! white colours, a null skin binding) for the meshes that lack a stream. That
//! is worse in the way that matters: it produces a mesh that *claims* to carry
//! normals, passes validation, and shades wrong — a silent, plausible-looking
//! defect that surfaces much later as black or inside-out lighting. Dropping the
//! stream instead makes the loss visible at the boundary, where the caller can
//! see it (`has_normals()` is now false) and do something deliberate: run
//! `generate_normals` on the combined result, or fix the input that was missing
//! them. Making a mesh worse is recoverable; making it *convincingly* wrong is
//! not.
//!
//! Skin streams follow the same rule and stay paired automatically: joints and
//! weights are present together on every input, so "present on all" holds for
//! both or neither.

use crate::mesh::Mesh;
use crate::mesh_error::MeshError;
use crate::mesh_error_code::MeshErrorCode;
use crate::mesh_result::MeshResult;
use crate::mesh_streams::MeshStreams;

/// Concatenate meshes into a single mesh, offsetting each one's indices by the
/// vertices already emitted.
///
/// An optional attribute stream survives only if **every** input carries it —
/// see the module documentation for why that policy, and not zero-filling, is
/// the right one. Combining a single mesh therefore reproduces it exactly.
///
/// # Errors
///
/// [`MeshErrorCode::EmptyPositions`] when `meshes` is empty: a `Mesh` always
/// describes at least one vertex, so there is no empty mesh to return.
pub fn combine(meshes: &[Mesh]) -> MeshResult<Mesh> {
    (!meshes.is_empty())
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::EmptyPositions,
                "combining zero meshes has no result: a mesh must have at least one position",
            )
        })
        .and_then(|()| Mesh::from_streams(combined_streams(meshes)))
}

/// Concatenate every stream, applying the presence policy to the optional ones.
fn combined_streams(meshes: &[Mesh]) -> MeshStreams {
    MeshStreams {
        normals: gathered(meshes, Mesh::normals),
        uvs: gathered(meshes, Mesh::uvs),
        tangents: gathered(meshes, Mesh::tangents),
        colors: gathered(meshes, Mesh::colors),
        joints: gathered(meshes, Mesh::joints),
        weights: gathered(meshes, Mesh::weights),
        ..MeshStreams::new(gathered(meshes, Mesh::positions), offset_indices(meshes))
    }
}

/// Concatenate one stream across every mesh — but only when every mesh has it.
///
/// `positions` is never empty, so passing it through here concatenates
/// unconditionally, exactly as required.
fn gathered<T: Copy>(meshes: &[Mesh], pick: fn(&Mesh) -> &[T]) -> Vec<T> {
    let present = meshes.iter().all(|mesh| !pick(mesh).is_empty());
    meshes
        .iter()
        .filter(|_| present)
        .flat_map(|mesh| pick(mesh).iter().copied())
        .collect()
}

/// Concatenate the index buffers, shifting each mesh's indices by the number of
/// vertices contributed by the meshes before it.
fn offset_indices(meshes: &[Mesh]) -> Vec<u32> {
    meshes
        .iter()
        .scan(0_u32, |base, mesh| {
            let start = *base;
            *base = start + mesh.vertex_count() as u32;
            Some(mesh.indices().iter().map(move |index| index + start))
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Vec2, Vec3, Vec4};

    /// A triangle at `offset`, with every optional stream populated.
    fn full(offset: f32) -> Mesh {
        Mesh::from_streams(MeshStreams {
            normals: vec![Vec3::UNIT_Y; 3],
            uvs: vec![Vec2::new(offset, 0.5); 3],
            tangents: vec![Vec4::new(1.0, 0.0, 0.0, 1.0); 3],
            colors: vec![Vec4::new(offset, 0.0, 0.0, 1.0); 3],
            joints: vec![[1, 0, 0, 0]; 3],
            weights: vec![[1.0, 0.0, 0.0, 0.0]; 3],
            ..MeshStreams::new(
                vec![
                    Vec3::new(offset, 0.0, 0.0),
                    Vec3::new(offset + 1.0, 0.0, 0.0),
                    Vec3::new(offset, 0.0, 1.0),
                ],
                vec![0, 1, 2],
            )
        })
        .unwrap()
    }

    /// A triangle carrying positions and indices only.
    fn bare(offset: f32) -> Mesh {
        Mesh::from_streams(MeshStreams::new(
            vec![
                Vec3::new(offset, 5.0, 0.0),
                Vec3::new(offset + 1.0, 5.0, 0.0),
                Vec3::new(offset, 5.0, 1.0),
            ],
            vec![0, 1, 2],
        ))
        .unwrap()
    }

    #[test]
    fn combining_nothing_is_an_error() {
        assert_eq!(
            combine(&[]).unwrap_err().code(),
            MeshErrorCode::EmptyPositions
        );
    }

    #[test]
    fn combining_one_mesh_reproduces_it() {
        assert_eq!(combine(&[full(0.0)]).unwrap(), full(0.0));
        assert_eq!(combine(&[bare(0.0)]).unwrap(), bare(0.0));
    }

    #[test]
    fn positions_concatenate_and_indices_shift() {
        let out = combine(&[bare(0.0), bare(10.0), bare(20.0)]).unwrap();
        assert_eq!(out.vertex_count(), 9);
        assert_eq!(out.triangle_count(), 3);
        assert_eq!(out.indices(), &[0, 1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(out.positions()[3], Vec3::new(10.0, 5.0, 0.0));
        assert_eq!(out.positions()[8], Vec3::new(20.0, 5.0, 1.0));
    }

    #[test]
    fn every_attribute_survives_when_every_input_has_it() {
        let out = combine(&[full(0.0), full(2.0)]).unwrap();
        assert_eq!(out.vertex_count(), 6);
        assert!(out.has_normals() & out.has_uvs() & out.has_tangents() & out.has_colors());
        assert!(out.is_skinned());
        assert_eq!(out.normals().len(), 6);
        assert_eq!(out.uvs()[0], Vec2::new(0.0, 0.5));
        assert_eq!(out.uvs()[3], Vec2::new(2.0, 0.5));
        assert_eq!(out.tangents()[5], Vec4::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(out.colors()[3], Vec4::new(2.0, 0.0, 0.0, 1.0));
        assert_eq!(out.joints()[5], [1, 0, 0, 0]);
        assert_eq!(out.weights()[5], [1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn an_attribute_missing_from_one_input_is_dropped_from_the_result() {
        // The policy under test: normals present on one mesh and absent on the
        // other yield NO normals, rather than a zero-filled half-truth.
        let out = combine(&[full(0.0), bare(4.0)]).unwrap();
        assert_eq!(out.vertex_count(), 6);
        assert!(!out.has_normals());
        assert!(!out.has_uvs());
        assert!(!out.has_tangents());
        assert!(!out.has_colors());
        assert!(!out.is_skinned());
        // The geometry itself is unaffected by the policy.
        assert_eq!(out.indices(), &[0, 1, 2, 3, 4, 5]);
        assert_eq!(out.positions()[4], Vec3::new(5.0, 5.0, 0.0));
    }

    #[test]
    fn the_policy_is_order_independent() {
        let a = combine(&[full(0.0), bare(4.0)]).unwrap();
        let b = combine(&[bare(4.0), full(0.0)]).unwrap();
        assert!(!a.has_normals());
        assert!(!b.has_normals());
        assert_eq!(a.vertex_count(), b.vertex_count());
    }

    #[test]
    fn a_mesh_with_no_triangles_still_contributes_its_vertices() {
        let points = Mesh::from_streams(MeshStreams::new(vec![Vec3::ZERO; 2], Vec::new())).unwrap();
        let out = combine(&[points, bare(0.0)]).unwrap();
        assert_eq!(out.vertex_count(), 5);
        assert_eq!(out.indices(), &[2, 3, 4]);
    }
}
