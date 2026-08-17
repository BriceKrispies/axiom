//! **Exact** algebraic identities: a node that provably *is* one of its own
//! inputs is replaced by that input.
//!
//! This is the one companion to [`crate::const_fold`], and it exists for the
//! case folding cannot reach. Folding needs **every** input constant; a node
//! with one non-constant input can never fold — *even when its value does not
//! depend on that input at all*. `Mix(x, x, t)` is exactly that node, and it is
//! not hypothetical: layer composition emits one `Mix` per channel per layer, so
//! a single **field** mask promotes all seven channels of a layered surface to
//! graphs, including every channel that each surface in the tree binds to the
//! same plain constant. One field mask, seven graphs, and five of them computing
//! a number that never varies.
//!
//! ## The one rule, and why it is allowed to exist
//!
//! `Mix(a, b, t)` is defined by [`crate::ops`] as **`a + (b - a) * t`**,
//! component-wise. When `a` and `b` are the **same node** — not merely equal
//! values, the same node, so the same bits in every lane at every sample — that
//! expression is `a + (a - a) * t = a + 0 * t = a`.
//!
//! The bar this has to clear is the one [`crate::const_fold`] states and the one
//! that keeps `x*1 -> x` and reassociation out of this crate: **a rewrite may not
//! move an `f32` bit.** This one does not, on any lane whose value is finite and
//! non-zero. The two edges where the rewritten and unrewritten graphs are not
//! bit-identical are both cases where the rewrite yields the *better* answer, and
//! both are stated here rather than discovered later:
//!
//! * **A lane that is negative zero.** `-0.0 - -0.0` is `+0.0`, and
//!   `-0.0 + 0.0 * t` is `+0.0`, so the arithmetic flushes the sign; the rewrite
//!   keeps `-0.0`. The two compare equal, and this language has no operator that
//!   can tell them apart (`Smoothstep` keys its degenerate lane on `e0 == e1`,
//!   which `-0.0 == 0.0` satisfies).
//! * **A lane of `a` or `t` that is not finite at evaluation.** `inf - inf` and
//!   `0 * inf` are both `NaN`, so the arithmetic turns an infinite `a`, or a
//!   finite `a` under an infinite `t`, into `NaN`; the rewrite yields `a`. That
//!   is the same choice the language already makes at every other degenerate
//!   point — `Smoothstep` with equal edges is `0`, `Pow` of a negative base is
//!   `0`, `Normalize` of the zero vector is `+Y` — a defined value in place of a
//!   propagated `NaN`. It cannot arise from a constant at all: a non-finite
//!   `Const` lane is rejected by [`crate::FieldGraph::validate`] and a fold whose
//!   result is not finite is refused outright.
//!
//! Both realisations move together, so CPU/GPU parity is untouched: the rewrite
//! runs on the graph, before the evaluator reads it and before a backend lowers
//! it, so there is only ever one expression for both to agree on.
//!
//! ## The two neighbouring rules that look exact and are not
//!
//! `Mix(a, b, 0) -> a` and `Mix(a, b, 1) -> b` are the obvious companions, and
//! **both are rejected**. They are stated here so nobody has to rediscover why.
//!
//! * **`Mix(a, b, 1) -> b` is not even close.** The expression is
//!   `a + (b - a) * 1`, which is `a + (b - a)` — and floating-point subtraction
//!   followed by addition is not the identity on `b`. Take `a = 1e30`,
//!   `b = 1.0`: `b - a` rounds to `-1e30` exactly (one is far below the ulp of
//!   the other), and `1e30 + -1e30` is `0.0`, not `1.0`. Ordinary finite
//!   magnitudes, no infinities, and the rewrite changes the answer outright.
//! * **`Mix(a, b, 0) -> a` fails only at the edges, which is still failing.**
//!   `(b - a) * 0` is `+0.0` or `-0.0` by the sign of `b - a`, so `a + that` is
//!   `a` for every ordinary `a` — but `a = b = -0.0` gives `-0.0 + 0.0 = +0.0`
//!   where the rewrite keeps `-0.0`, and a `b - a` that overflows to an infinity
//!   gives `inf * 0 = NaN` where the rewrite keeps `a`.
//!
//! The distinction from the rule this file *does* carry is not a matter of
//! degree. `Mix(x, x, t)` subtracts a node **from itself**: `x - x` is `+0.0` on
//! every finite lane, it cannot overflow, and it cannot be large relative to
//! `x`. `b - a` for two *different* expressions can be either. That is the whole
//! argument, and it is why one rule is here and two are not.
//!
//! (A constant mask of exactly `1` — which layer composition really does emit
//! for an unmasked layer — is a genuine, separate saving. Its correct home is
//! the composer, which can decline to build a `Mix` at all when the mask is the
//! constant-one *binding*, not an algebraic rewrite of a `Mix` already built.)
//!
//! ## The width guard is not optional
//!
//! `Mix` is width-generic: its output type is its **widest** input, with a scalar
//! broadcasting across a vector. So `Mix(scalar, scalar, vec3_mask)` is a `Vec3`
//! node, and replacing it with its scalar input would change the node's *type*
//! and every type derived from it. The rule therefore fires only when the
//! selector is no wider than the value inputs — i.e. only when the input it
//! selects already has the node's own type.

use axiom_recipe::NodeId;

use crate::field_op::{FieldOp, FIELD_OP_COUNT};
use crate::field_type::FieldType;

/// Which operators carry an exact identity rule, indexed by the operator code,
/// in code order.
///
/// One entry is true, and adding another is a deliberate amendment to the
/// argument above — not a default. An operator here must also be
/// non-commutative, because the ids handed to [`identity_input`] are in
/// canonical order and canonicalisation sorts a commutative operator's inputs;
/// `canonical::the_two_operator_tables_do_not_overlap` holds that line.
#[rustfmt::skip]
pub(crate) const SELECTS_AN_INPUT: [bool; FIELD_OP_COUNT] = [
    false,                              // Const
    false, false, false, false,         // Point / Uv / Normal / Time
    false,                              // Param
    false, false, false, false, false,  // Add / Sub / Mul / Min / Max
    false,                              // Abs
    false, true,  false,                // Clamp / Mix / Smoothstep
    false, false, false,                // Dot / Length / Normalize
    false, false,                       // Compose / Component
    false, false,                       // Noise / Fbm
    false,                              // Transform
    false, false, false, false,         // Sin / Cos / Pow / Exp
];

/// The input this node **is**, when an exact identity makes it one of them.
///
/// `inputs` are the node's inputs as ids in the graph being emitted, in slot
/// order; `widths` are the derived types of those same slots. Both are total: a
/// slot either slice does not have simply refuses the rewrite.
pub(crate) fn identity_input(
    op: FieldOp,
    inputs: &[NodeId],
    widths: &[FieldType],
) -> Option<NodeId> {
    SELECTS_AN_INPUT[op.code() as usize]
        .then(|| equal_endpoints(inputs, widths))
        .flatten()
}

/// `Mix(x, x, t) -> x`, guarded on the selector being no wider than `x`.
///
/// The two value inputs are compared by **id**, not by value: two nodes that
/// canonicalisation has already shared are one node, so this sees the equality
/// that folding could not.
fn equal_endpoints(inputs: &[NodeId], widths: &[FieldType]) -> Option<NodeId> {
    inputs
        .first()
        .zip(inputs.get(1))
        .zip(widths.first().zip(widths.get(2)))
        .filter(|((under, over), (value, selector))| (under == over) & (selector <= value))
        .map(|((under, _over), _widths)| *under)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(raw: [u32; 3]) -> Vec<NodeId> {
        raw.iter().copied().map(NodeId::from_raw).collect()
    }

    #[test]
    fn a_mix_of_one_node_with_itself_is_that_node() {
        assert_eq!(
            identity_input(
                FieldOp::Mix,
                &ids([4, 4, 9]),
                &[FieldType::Scalar, FieldType::Scalar, FieldType::Scalar]
            ),
            Some(NodeId::from_raw(4))
        );
    }

    #[test]
    fn a_mix_of_two_different_nodes_is_left_alone() {
        assert_eq!(
            identity_input(
                FieldOp::Mix,
                &ids([4, 5, 9]),
                &[FieldType::Vec3, FieldType::Vec3, FieldType::Scalar]
            ),
            None
        );
    }

    #[test]
    fn a_selector_wider_than_its_endpoints_refuses_the_rewrite() {
        // `Mix(scalar, scalar, vec3)` is a `Vec3` node. Replacing it with its
        // scalar input would narrow the node's type.
        assert_eq!(
            identity_input(
                FieldOp::Mix,
                &ids([4, 4, 9]),
                &[FieldType::Scalar, FieldType::Scalar, FieldType::Vec3]
            ),
            None
        );
        // A selector narrower than the endpoints broadcasts and changes nothing.
        assert_eq!(
            identity_input(
                FieldOp::Mix,
                &ids([4, 4, 9]),
                &[FieldType::Vec4, FieldType::Vec4, FieldType::Scalar]
            ),
            Some(NodeId::from_raw(4))
        );
    }

    #[test]
    fn no_other_operator_carries_an_identity_rule() {
        let every = (0..FIELD_OP_COUNT as u16).filter_map(FieldOp::from_code);
        let selecting: Vec<FieldOp> = every
            .filter(|op| {
                identity_input(
                    *op,
                    &ids([4, 4, 4]),
                    &[FieldType::Scalar, FieldType::Scalar, FieldType::Scalar],
                )
                .is_some()
            })
            .collect();
        assert_eq!(selecting, vec![FieldOp::Mix]);
    }

    #[test]
    fn a_node_missing_the_slots_the_rule_reads_refuses_it() {
        // Only reachable through a graph the type checker has already rejected;
        // the rule still answers rather than indexing past its inputs.
        assert_eq!(
            identity_input(FieldOp::Mix, &ids([4, 4, 9])[..1], &[FieldType::Scalar; 1]),
            None
        );
        assert_eq!(
            identity_input(FieldOp::Mix, &ids([4, 4, 9]), &[FieldType::Scalar; 2]),
            None
        );
    }
}
