//! Branchless operator dispatch: a `const` table indexed by the operator code.

use axiom_field::FieldGraph;
use axiom_proc_core::NodeEval;

use crate::combine::{merge, trs};
use crate::implicit::meta_surface;
use crate::mesh_buffer::MeshBuffer;
use crate::primitives::{cube, cylinder, grid, sphere};
use crate::transforms::{bend, bevel, displace, extrude, transform, triangulate, uv_project};

/// A mesh operator: node context and the recipe's field table in, produced mesh
/// out (or `None` on failure).
///
/// Every operator takes the field table, whether or not it reads one, because the
/// dispatch table is a `const` array of one function type — the shape that makes
/// selecting an operator an index rather than a branch. Only `Displace` reads it.
type MeshOpFn =
    for<'a, 'f> fn(NodeEval<'a, MeshBuffer>, &'f [FieldGraph]) -> Option<MeshBuffer>;

/// The dispatch table. Its order mirrors [`crate::MeshOp`] so `op as usize`
/// selects the operator — a table index, never a `match`.
const OPS: [MeshOpFn; 14] = [
    cube,
    cylinder,
    grid,
    transform,
    extrude,
    bevel,
    bend,
    displace,
    uv_project,
    triangulate,
    sphere,
    meta_surface,
    merge,
    trs,
];

/// Evaluate one node against an empty field table — every operator but
/// `Displace` ignores it, and `Displace` falls back to its own value-noise graph.
pub(crate) fn mesh_eval(ctx: NodeEval<'_, MeshBuffer>) -> Option<MeshBuffer> {
    mesh_eval_with_fields(ctx, &[])
}

/// Evaluate one node: select its operator by code and run it against `fields`. An
/// operator code outside the table fails the node (`None`).
pub(crate) fn mesh_eval_with_fields(
    ctx: NodeEval<'_, MeshBuffer>,
    fields: &[FieldGraph],
) -> Option<MeshBuffer> {
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
        g.add(250, vec![], vec![]);
        assert!(ProcCore::new()
            .execute(&g, 0, &SpaceApi::root(), mesh_eval)
            .is_err());
    }
}
