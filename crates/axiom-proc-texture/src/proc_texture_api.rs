//! [`ProcTextureApi`]: bake a texture recipe into a [`TextureBuffer`].

use axiom_field::FieldGraph;
use axiom_proc_core::{ProcCore, ProcResult};
use axiom_recipe::RecipeGraph;
use axiom_space::SpaceApi;

use crate::dispatch::{texture_eval, texture_eval_with_fields};
use crate::texture_buffer::TextureBuffer;

/// The texture generation facade. It bakes a validated texture recipe by running
/// it through the shared executor with the texture operator evaluator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProcTextureApi;

impl ProcTextureApi {
    /// Construct the facade.
    pub const fn new() -> Self {
        Self
    }

    /// Bake `recipe` at `seed` into the texture its final node produces. Fails
    /// with the executor's stable error (invalid recipe, operator failure, or
    /// an empty recipe).
    ///
    /// The field table is empty, so a recipe containing a
    /// [`crate::TextureOp::Field`] node fails here; bake it with
    /// [`ProcTextureApi::bake_with_fields`] instead.
    pub fn bake(&self, recipe: &RecipeGraph, seed: u64) -> ProcResult<TextureBuffer> {
        ProcCore::new().execute(recipe, seed, &SpaceApi::root(), texture_eval)
    }

    /// Bake `recipe` at `seed` with `fields` available to its
    /// [`crate::TextureOp::Field`] nodes, each of which names one entry of the
    /// table by index.
    ///
    /// The field table travels **beside** the recipe rather than inside it: a
    /// [`axiom_recipe::Param`] is one `u32` word, so inlining a graph's bytes
    /// would spend the node budget on a single operator. Everything else is
    /// exactly [`ProcTextureApi::bake`] — same executor, same seed, same
    /// operator table, and the same byte-identical output for the same inputs.
    pub fn bake_with_fields(
        &self,
        recipe: &RecipeGraph,
        seed: u64,
        fields: &[FieldGraph],
    ) -> ProcResult<TextureBuffer> {
        ProcCore::new().execute(recipe, seed, &SpaceApi::root(), |ctx| {
            texture_eval_with_fields(ctx, fields)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::texture_op::TextureOp;
    use axiom_recipe::{Color, NodeId, Param, RecipeId};

    fn defaulted<T: Default>() -> T {
        T::default()
    }

    #[test]
    fn new_and_default_agree() {
        assert_eq!(ProcTextureApi::new(), ProcTextureApi);
        assert_eq!(defaulted::<ProcTextureApi>(), ProcTextureApi::new());
    }

    #[test]
    fn bakes_a_brick_over_noise_recipe_deterministically() {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        let bricks = g.add(
            TextureOp::Bricks as u16,
            vec![
                Param::int(16),
                Param::int(16),
                Param::int(4),
                Param::int(2),
                Param::int(1),
                Param::color(Color::rgba(0xB0, 0x50, 0x40, 0xFF)),
                Param::color(Color::rgba(0x30, 0x30, 0x30, 0xFF)),
            ],
            vec![],
        );
        g.add(TextureOp::Blur as u16, vec![Param::int(1)], vec![bricks]);
        let api = ProcTextureApi::new();
        let a = api.bake(&g, 5).unwrap();
        let b = api.bake(&g, 5).unwrap();
        assert_eq!(a, b);
        assert_eq!((a.width(), a.height()), (16, 16));
    }

    #[test]
    fn invalid_recipe_bakes_to_an_error() {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        g.add(
            TextureOp::Blur as u16,
            vec![Param::int(1)],
            vec![NodeId::from_raw(9)],
        );
        assert!(ProcTextureApi::new().bake(&g, 0).is_err());
    }
}
