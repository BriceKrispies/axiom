//! [`ProcMeshApi`]: bake a mesh recipe into a [`MeshBuffer`].

use axiom_field::FieldGraph;
use axiom_proc_core::{ProcCore, ProcResult};
use axiom_recipe::RecipeGraph;
use axiom_space::SpaceApi;

use crate::dispatch::{mesh_eval, mesh_eval_with_fields};
use crate::mesh_buffer::MeshBuffer;

/// The mesh generation facade. It bakes a validated mesh recipe by running it
/// through the shared executor with the mesh operator evaluator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProcMeshApi;

impl ProcMeshApi {
    /// Construct the facade.
    pub const fn new() -> Self {
        Self
    }

    /// Bake `recipe` at `seed` into the mesh its final node produces. Fails with
    /// the executor's stable error (invalid recipe, operator failure, or an
    /// empty recipe).
    pub fn bake(&self, recipe: &RecipeGraph, seed: u64) -> ProcResult<MeshBuffer> {
        ProcCore::new().execute(recipe, seed, &SpaceApi::root(), mesh_eval)
    }

    /// Bake `recipe` at `seed` with `fields` available to its
    /// [`crate::MeshOp::Displace`] nodes, each of which may name one entry of the
    /// table by index.
    ///
    /// The field table travels **beside** the recipe rather than inside it: a
    /// [`axiom_recipe::Param`] is one `u32` word, so inlining a graph's bytes
    /// would spend the node budget on a single operator. A `Displace` node that
    /// names no field is unaffected and displaces by the value noise it always
    /// did, so [`ProcMeshApi::bake`] and this method agree byte-for-byte on every
    /// recipe written before the field table existed.
    pub fn bake_with_fields(
        &self,
        recipe: &RecipeGraph,
        seed: u64,
        fields: &[FieldGraph],
    ) -> ProcResult<MeshBuffer> {
        ProcCore::new().execute(recipe, seed, &SpaceApi::root(), |ctx| {
            mesh_eval_with_fields(ctx, fields)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_op::MeshOp;
    use axiom_recipe::{NodeId, Param, RecipeId, Scalar};

    fn defaulted<T: Default>() -> T {
        T::default()
    }

    #[test]
    fn new_and_default_agree() {
        assert_eq!(ProcMeshApi::new(), ProcMeshApi);
        assert_eq!(defaulted::<ProcMeshApi>(), ProcMeshApi::new());
    }

    #[test]
    fn bakes_a_cube_crate_recipe_deterministically() {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        let c = g.add(
            MeshOp::Cube as u16,
            vec![Param::scalar(Scalar::new(1.0))],
            vec![],
        );
        let bent = g.add(
            MeshOp::Bevel as u16,
            vec![Param::scalar(Scalar::new(0.1))],
            vec![c],
        );
        g.add(
            MeshOp::UVProject as u16,
            vec![Param::scalar(Scalar::new(1.0))],
            vec![bent],
        );
        let api = ProcMeshApi::new();
        let a = api.bake(&g, 9).unwrap();
        let b = api.bake(&g, 9).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.vertex_count(), 24);
    }

    #[test]
    fn invalid_recipe_bakes_to_an_error() {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        g.add(
            MeshOp::Bevel as u16,
            vec![Param::scalar(Scalar::new(0.1))],
            vec![NodeId::from_raw(7)],
        );
        assert!(ProcMeshApi::new().bake(&g, 0).is_err());
    }
}
