//! The recipe's node model and its branchless evaluation step.
//!
//! A node is a generic, **domain-free** operation: a literal, an entropy draw, or
//! a two-input combine. Node dispatch is a table index over the fieldless
//! [`NodeOp`] discriminant — never a `match` over kinds — so evaluation stays
//! branchless. Inputs reference earlier nodes by index, so a recipe is a DAG by
//! construction: a forward, self, or back reference is an invalid recipe, rejected
//! as data by [`nodes_form_dag`] (never a panic, and evaluation defaults a missing
//! input to `0` so it cannot panic either).

use axiom_entropy::EntropyStream;

/// A generic, domain-free node operation. Discriminants are explicit so the value
/// indexes the op + arity tables (the sanctioned branchless form for a
/// fieldless-enum match). Domain meaning never lives here — an op transforms
/// neutral `u64` words; what they encode is a domain module's job.
#[derive(Debug, Clone, Copy)]
pub(crate) enum NodeOp {
    /// A literal value (the node's `immediate`).
    Const = 0,
    /// Draw the next value from the entropy stream.
    Draw = 1,
    /// Wrapping add of two earlier nodes.
    Add = 2,
    /// Xor of two earlier nodes.
    Xor = 3,
}

/// One recipe node: an op, an immediate (used by `Const`), and up to two input
/// node indices (used by the two-input ops).
#[derive(Debug, Clone, Copy)]
pub(crate) struct RecipeNode {
    pub(crate) op: NodeOp,
    pub(crate) immediate: u64,
    pub(crate) inputs: [usize; 2],
}

/// How many of a node's `inputs` each op consumes (indexed by `op as usize`).
const ARITY: [usize; 4] = [0, 0, 2, 2];

/// The op implementations, indexed by `op as usize` — a branchless dispatch table.
const OPS: [fn(&RecipeNode, &[u64], &mut EntropyStream) -> u64; 4] =
    [op_const, op_draw, op_add, op_xor];

fn op_const(node: &RecipeNode, _values: &[u64], _stream: &mut EntropyStream) -> u64 {
    node.immediate
}

fn op_draw(_node: &RecipeNode, _values: &[u64], stream: &mut EntropyStream) -> u64 {
    stream.next_u64()
}

fn op_add(node: &RecipeNode, values: &[u64], _stream: &mut EntropyStream) -> u64 {
    input(values, node, 0).wrapping_add(input(values, node, 1))
}

fn op_xor(node: &RecipeNode, values: &[u64], _stream: &mut EntropyStream) -> u64 {
    input(values, node, 0) ^ input(values, node, 1)
}

/// Read input `k`'s already-computed value, defaulting a missing index to `0` so
/// evaluation can never panic (validity is enforced separately by
/// [`nodes_form_dag`]). Branchless.
fn input(values: &[u64], node: &RecipeNode, k: usize) -> u64 {
    values.get(node.inputs[k]).copied().unwrap_or(0)
}

/// Apply a node, producing its value. Branchless table dispatch over the op.
pub(crate) fn apply(node: &RecipeNode, values: &[u64], stream: &mut EntropyStream) -> u64 {
    OPS[node.op as usize](node, values, stream)
}

/// The trace discriminant for a node's op.
pub(crate) fn op_code(node: &RecipeNode) -> u32 {
    node.op as u32
}

/// Whether `nodes` form a valid DAG: every node's *used* inputs reference a
/// strictly-earlier node. A forward, self, or back reference makes the recipe
/// invalid (rejected as data, not a panic). Branchless.
pub(crate) fn nodes_form_dag(nodes: &[RecipeNode]) -> bool {
    nodes.iter().enumerate().all(|(index, node)| {
        node.inputs
            .iter()
            .take(ARITY[node.op as usize])
            .all(|&input_index| input_index < index)
    })
}
