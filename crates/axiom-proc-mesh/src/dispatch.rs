//! Branchless operator dispatch: a `const` table indexed by the operator code.

use axiom_proc_core::NodeEval;

use crate::implicit::meta_surface;
use crate::mesh_buffer::MeshBuffer;
use crate::primitives::{cube, cylinder, grid, sphere};
use crate::transforms::{bend, bevel, displace, extrude, transform, triangulate, uv_project};

/// A mesh operator: node context in, produced mesh out (or `None` on failure).
type MeshOpFn = for<'a> fn(NodeEval<'a, MeshBuffer>) -> Option<MeshBuffer>;

/// The dispatch table. Its order mirrors [`crate::MeshOp`] so `op as usize`
/// selects the operator — a table index, never a `match`.
const OPS: [MeshOpFn; 12] = [
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
];

/// Evaluate one node: select its operator by code and run it. An operator code
/// outside the table fails the node (`None`).
pub(crate) fn mesh_eval(ctx: NodeEval<'_, MeshBuffer>) -> Option<MeshBuffer> {
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
        g.add(250, vec![], vec![]);
        assert!(ProcCore::new()
            .execute(&g, 0, &SpaceApi::root(), mesh_eval)
            .is_err());
    }
}
