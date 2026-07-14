//! Branchless operator dispatch: a `const` table indexed by the operator code.

use axiom_proc_core::NodeEval;

use crate::filters::{blend, blur, color_ramp, height_to_normal};
use crate::generators::{bricks, checker, gradient, noise, solid, spots};
use crate::text::text;
use crate::texture_buffer::TextureBuffer;

/// A texture operator: node context in, produced buffer out (or `None` on
/// failure).
type TexOp = for<'a> fn(NodeEval<'a, TextureBuffer>) -> Option<TextureBuffer>;

/// The dispatch table. Its order mirrors [`crate::TextureOp`] so `op as usize`
/// selects the operator — a table index, never a `match`.
const OPS: [TexOp; 11] = [
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
];

/// Evaluate one node: select its operator by code and run it. An operator code
/// outside the table is an unknown operator and fails the node (`None`).
pub(crate) fn texture_eval(ctx: NodeEval<'_, TextureBuffer>) -> Option<TextureBuffer> {
    let index = ctx.op() as usize;
    OPS.get(index).copied().and_then(move |op| op(ctx))
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
