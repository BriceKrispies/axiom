//! Branchless operator dispatch: a `const` table of function pointers indexed by
//! the operator code.
//!
//! One row per [`crate::FieldOp`], in discriminant order, so `op as usize`
//! selects the operator — a table index, never a `match` (the Branchless Law) and
//! never a generic closure parameter (the Axiom State Law bans an `F: Fn(..)`
//! bound, which is why `proc-core`'s evaluator cannot be reused here even
//! setting its per-node allocation aside).
//!
//! Every operator is **total**: it returns a [`FieldValue`], never an `Option`
//! and never an error. The whole rejection surface lives in
//! [`crate::FieldGraph::validate`], and putting a per-sample error path in the
//! innermost loop of every bake to re-state it would be indefensible.

use crate::eval::FieldEvalStep;
use crate::field_op::FIELD_OP_COUNT;
use crate::field_value::FieldValue;
use crate::ops::{arith, shape, source, spatial, vector};

/// A field operator: one node's step in, its value out.
type FieldOpFn = fn(&FieldEvalStep<'_>) -> FieldValue;

/// The dispatch table. Its order mirrors [`crate::FieldOp`] exactly, so the
/// operator code **is** the row index.
#[rustfmt::skip]
const OPS: [FieldOpFn; FIELD_OP_COUNT] = [
    source::constant,
    source::point, source::uv, source::normal, source::time,
    source::parameter,
    arith::add, arith::subtract, arith::multiply, arith::minimum, arith::maximum,
    arith::absolute,
    shape::clamp, shape::mix, shape::smoothstep,
    vector::dot, vector::length, vector::normalize,
    vector::compose, vector::component,
    spatial::noise, spatial::fbm, spatial::transform,
];

/// Evaluate one node: select its operator by code and run it.
///
/// An operator code outside the table names no operator. That is impossible on a
/// validated graph — `UnknownOperator` rejects it — so the guard yields the
/// documented [`FieldValue::ZERO`] default rather than inventing an error path
/// the evaluator promises not to have.
pub(crate) fn field_eval(code: u16, step: &FieldEvalStep<'_>) -> FieldValue {
    OPS.get(code as usize)
        .map_or(FieldValue::ZERO, |op| op(step))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_recipe::Scalar;

    use crate::eval_context::EvalContext;
    use crate::field_op::FieldOp;
    use crate::field_params::FieldParams;

    #[test]
    fn every_operator_has_exactly_one_row_and_the_code_is_its_index() {
        assert_eq!(OPS.len(), FIELD_OP_COUNT);
        let table = FieldParams::new();
        let inputs = [FieldValue::scalar(Scalar::new(0.5))];
        // Each row is reachable through its own code: running the table through
        // every operator proves no row is stranded behind another's index.
        FieldOp::ALL.iter().for_each(|op| {
            let step = FieldEvalStep::new(&inputs, &[], &EvalContext::ORIGIN, &table);
            let direct = OPS[op.code() as usize](&step);
            assert_eq!(field_eval(op.code(), &step), direct, "{op:?} must dispatch");
        });
    }

    #[test]
    fn an_unknown_operator_code_yields_the_zero_default() {
        let table = FieldParams::new();
        let step = FieldEvalStep::new(&[], &[], &EvalContext::ORIGIN, &table);
        assert_eq!(
            field_eval(FIELD_OP_COUNT as u16, &step),
            FieldValue::ZERO,
            "code 23 names no operator"
        );
        assert_eq!(field_eval(u16::MAX, &step), FieldValue::ZERO);
    }
}
