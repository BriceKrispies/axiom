//! Branchless operator dispatch: a `const` table indexed by the operator code.

use axiom_field::FieldGraph;
use axiom_proc_core::NodeEval;

use crate::field_source::field;
use crate::filters::{blend, blur, color_ramp, height_to_normal};
use crate::generators::{bricks, checker, gradient, noise, solid, spots};
use crate::text::text;
use crate::texture_buffer::TextureBuffer;

/// A texture operator: node context and the recipe's field table in, produced
/// buffer out (or `None` on failure).
///
/// Every operator takes the field table, whether or not it reads one, because the
/// dispatch table is a `const` array of one function type — the shape that makes
/// selecting an operator an index rather than a branch. Only `Field` reads it.
type TexOp =
    for<'a, 'f> fn(NodeEval<'a, TextureBuffer>, &'f [FieldGraph]) -> Option<TextureBuffer>;

/// The dispatch table. Its order mirrors [`crate::TextureOp`] so `op as usize`
/// selects the operator — a table index, never a `match`.
const OPS: [TexOp; 12] = [
    solid,
    gradient,
    noise,
    bricks,
    blur,
    blend,
    color_ramp,
    height_to_normal,
    checker,
    text,
    spots,
    field,
];

/// Evaluate one node against an empty field table — the eleven fixed operators
/// need none, and a `Field` node with nothing to name fails.
pub(crate) fn texture_eval(ctx: NodeEval<'_, TextureBuffer>) -> Option<TextureBuffer> {
    texture_eval_with_fields(ctx, &[])
}

/// Evaluate one node: select its operator by code and run it against `fields`. An
/// operator code outside the table is an unknown operator and fails the node
/// (`None`).
pub(crate) fn texture_eval_with_fields(
    ctx: NodeEval<'_, TextureBuffer>,
    fields: &[FieldGraph],
) -> Option<TextureBuffer> {
    let index = ctx.op() as usize;
    OPS.get(index).copied().and_then(move |op| op(ctx, fields))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_proc_core::ProcCore;
    use axiom_recipe::{RecipeGraph, RecipeId};
    use axiom_space::SpaceApi;

    #[test]
    fn unknown_operator_code_fails_the_node() {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        g.add(200, vec![], vec![]);
        assert!(ProcCore::new()
            .execute(&g, 0, &SpaceApi::root(), texture_eval)
            .is_err());
    }
}
