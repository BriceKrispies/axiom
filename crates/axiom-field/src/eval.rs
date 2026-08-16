//! The pointwise evaluator: **a flat fold in node id order over a fixed-size
//! register file**.
//!
//! This is the semantic reference implementation of the field language. Every
//! other realisation of a field — a shader emitted for a GPU backend, a
//! per-triangle CPU shading path — is a *mirror* checked against what the
//! operator functions in [`crate::ops`] compute here.
//!
//! ## The shape, and why it is this shape
//!
//! * **A flat fold, never a recursive walk.** Node ids are dense and every input
//!   names a strictly-earlier node, so one forward pass in id order has every
//!   input already computed when it reaches a node. `engine_no_recursion` is at
//!   0 and a recursive evaluator would be worse anyway.
//! * **A fixed-size register array, never a `Vec` grown per call.**
//!   [`axiom_recipe::MAX_NODES`] is 256 and a [`FieldValue`] is five words, so
//!   `[FieldValue; MAX_NODES]` is about 5 KB of stack and **allocates nothing**.
//!   That is the reason this layer does not build on `proc-core`, whose
//!   evaluator allocates a `Vec` and mints an entropy stream per node per call:
//!   fine once per artifact, catastrophic once per texel.
//! * **Inputs are read by index, never cloned.** The per-call cost is
//!   `O(nodes)`, not `O(nodes × inputs)`.
//! * **Every operator is total.** All rejection happened in
//!   [`crate::FieldGraph::validate`]; an evaluator that could fail at a point
//!   would put an error path in the innermost loop of every bake. Every lookup
//!   here is made total against the documented [`FieldValue::ZERO`] default, so
//!   even a graph that was never validated yields a value rather than a panic —
//!   the value is simply only *meaningful* for a graph that type-checks.
//!
//! ## Determinism
//!
//! Same graph, same context → **bit-identical** `f32` on every target including
//! `wasm32`. The algebra excludes transcendentals, `sqrt` is IEEE-754 exact, and
//! the one reciprocal (`Normalize`) has its evaluation order fixed and written
//! down. Nothing here reads a clock, an address or an iteration order.

use axiom_recipe::{Node, NodeId, Param, MAX_NODES};

use crate::dispatch;
use crate::eval_context::EvalContext;
use crate::field_params::FieldParams;
use crate::field_type::FieldType;
use crate::field_value::FieldValue;

/// The four lanes an operator works on. Lanes past the operating width always
/// hold `0.0`, which is what lets every operator run all four unconditionally.
pub(crate) type Lanes = [f32; 4];

/// The most inputs any operator consumes: `Compose` at width four. The gather
/// buffer is this size, so a malformed node carrying more inputs than any
/// operator can consume is truncated rather than trusted.
pub(crate) const MAX_INPUTS: usize = 4;

/// One node under the evaluator's eye: its already-computed input values, its
/// raw parameter words, and the two external tables an operator may read.
///
/// `Copy` and borrow-only — it owns nothing and allocates nothing, so building
/// one per node costs four pointers.
#[derive(Clone, Copy)]
pub(crate) struct FieldEvalStep<'a> {
    inputs: &'a [FieldValue],
    params: &'a [Param],
    context: &'a EvalContext,
    table: &'a FieldParams,
}

impl<'a> FieldEvalStep<'a> {
    /// One step from its four parts.
    pub(crate) fn new(
        inputs: &'a [FieldValue],
        params: &'a [Param],
        context: &'a EvalContext,
        table: &'a FieldParams,
    ) -> Self {
        FieldEvalStep {
            inputs,
            params,
            context,
            table,
        }
    }

    /// Input slot `slot`'s value, or [`FieldValue::ZERO`] when the node has no
    /// such slot.
    pub(crate) fn input(&self, slot: usize) -> FieldValue {
        self.inputs.get(slot).copied().unwrap_or_default()
    }

    /// The node's **operating width**: the widest type among its inputs.
    /// [`FieldType`] orders by width, so this is a plain `max` fold. A node with
    /// no inputs operates at [`FieldType::Scalar`].
    pub(crate) fn width(&self) -> FieldType {
        self.inputs
            .iter()
            .fold(FieldType::Scalar, |widest, value| widest.max(value.ty()))
    }

    /// Input slot `slot`'s lanes, broadened to the node's operating width.
    pub(crate) fn lanes(&self, slot: usize) -> Lanes {
        broadcast(self.input(slot), self.width())
    }

    /// Parameter word `slot`, or `0` when the node carries no such word.
    pub(crate) fn word(&self, slot: usize) -> u32 {
        self.params.get(slot).map_or(0, |param| param.bits())
    }

    /// The node's raw parameter words.
    pub(crate) fn words(&self) -> &'a [Param] {
        self.params
    }

    /// The evaluation context the caller supplied.
    pub(crate) fn context(&self) -> &'a EvalContext {
        self.context
    }

    /// The field's parameter table.
    pub(crate) fn table(&self) -> &'a FieldParams {
        self.table
    }
}

/// A value's lanes at `width`.
///
/// A scalar **replicates** across every lane of the width — the language's one
/// implicit conversion — and every lane past the width is zeroed, so an operator
/// that sums lanes (`Dot`, `Length`) can run all four without knowing the width.
pub(crate) fn broadcast(value: FieldValue, width: FieldType) -> Lanes {
    let scalar = usize::from(value.ty() == FieldType::Scalar);
    let single = value.as_scalar().get();
    let vector = value.as_vec4();
    let source: Lanes = [
        [vector.x, vector.y, vector.z, vector.w],
        [single, single, single, single],
    ][scalar];
    let lanes = width.lanes();
    [0_u8, 1, 2, 3].map(|index| [0.0, source[index as usize]][usize::from(index < lanes)])
}

/// Lanes stamped with an output type, which scrubs every lane past that type's
/// width back to the documented default.
pub(crate) fn typed(ty: FieldType, lanes: Lanes) -> FieldValue {
    FieldValue::from_words(
        ty,
        [
            lanes[0].to_bits(),
            lanes[1].to_bits(),
            lanes[2].to_bits(),
            lanes[3].to_bits(),
        ],
    )
}

/// Apply a two-operand lane function across all four lanes. A bare `fn` pointer,
/// never a generic `F: Fn` bound — the Axiom State Law bans the latter, and a
/// function pointer captures no environment.
pub(crate) fn zip2(a: Lanes, b: Lanes, op: fn(f32, f32) -> f32) -> Lanes {
    [
        op(a[0], b[0]),
        op(a[1], b[1]),
        op(a[2], b[2]),
        op(a[3], b[3]),
    ]
}

/// The sum of the four lane products — the dot product, given that lanes past
/// the operating width are zero.
///
/// Written as plain multiply-then-add, never `mul_add`: a fused multiply-add
/// rounds once where a shader rounds twice.
pub(crate) fn dot(a: Lanes, b: Lanes) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}

/// The value of `nodes[node]` under `context` and `table`.
///
/// One forward fold over the nodes `0..=node` — the later nodes cannot
/// contribute, because every input names a strictly-earlier node. The register
/// file starts filled with [`FieldValue::ZERO`], the documented default a node
/// reading an id outside the graph sees.
///
/// The caller has already proved `node` names a node and the graph fits the
/// register file; see [`crate::FieldGraph::evaluate_at`].
pub(crate) fn evaluate(
    nodes: &[Node],
    node: NodeId,
    context: &EvalContext,
    table: &FieldParams,
) -> FieldValue {
    let last = node.raw() as usize;
    let registers = (0..=last).fold([FieldValue::ZERO; MAX_NODES], |mut registers, index| {
        let value = node_value(&nodes[index], &registers, context, table);
        registers[index] = value;
        registers
    });
    registers[last]
}

/// One node's value: gather its inputs out of the register file into a
/// fixed-size buffer, then dispatch on its operator code.
fn node_value(
    node: &Node,
    registers: &[FieldValue; MAX_NODES],
    context: &EvalContext,
    table: &FieldParams,
) -> FieldValue {
    let mut inputs = [FieldValue::ZERO; MAX_INPUTS];
    node.inputs()
        .iter()
        .take(MAX_INPUTS)
        .enumerate()
        .for_each(|(slot, input)| {
            inputs[slot] = registers
                .get(input.raw() as usize)
                .copied()
                .unwrap_or_default();
        });
    let count = node.inputs().len().min(MAX_INPUTS);
    dispatch::field_eval(
        node.op(),
        &FieldEvalStep::new(&inputs[..count], node.params(), context, table),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::Seconds;
    use axiom_math::{Vec2, Vec3};
    use axiom_recipe::Scalar;

    use crate::field_op::FieldOp;

    fn value(x: f32) -> FieldValue {
        FieldValue::scalar(Scalar::new(x))
    }

    fn step<'a>(
        inputs: &'a [FieldValue],
        params: &'a [Param],
        context: &'a EvalContext,
        table: &'a FieldParams,
    ) -> FieldEvalStep<'a> {
        FieldEvalStep::new(inputs, params, context, table)
    }

    #[test]
    fn a_step_reads_its_inputs_words_and_tables_totally() {
        let inputs = [value(1.5), FieldValue::vec3(Vec3::new(1.0, 2.0, 3.0))];
        let params = [Param::int(7)];
        let context = EvalContext::new(
            Vec3::UNIT_X,
            Vec2::ZERO,
            Vec3::UNIT_Y,
            Seconds::finite_or_zero(2.0),
        );
        let table = FieldParams::new();
        let cx = step(&inputs, &params, &context, &table);
        assert_eq!(cx.input(0), value(1.5));
        assert_eq!(cx.input(9), FieldValue::ZERO);
        assert_eq!(cx.word(0), 7);
        assert_eq!(cx.word(9), 0);
        assert_eq!(cx.words().len(), 1);
        assert_eq!(cx.context().point(), Vec3::UNIT_X);
        assert_eq!(cx.table().len(), 0);
        assert_eq!(cx.width(), FieldType::Vec3);
        // The scalar broadcasts across the Vec3 width; the fourth lane is zero.
        assert_eq!(cx.lanes(0), [1.5, 1.5, 1.5, 0.0]);
        assert_eq!(cx.lanes(1), [1.0, 2.0, 3.0, 0.0]);
    }

    #[test]
    fn a_step_with_no_inputs_operates_at_scalar_width() {
        let table = FieldParams::new();
        let cx = step(&[], &[], &EvalContext::ORIGIN, &table);
        assert_eq!(cx.width(), FieldType::Scalar);
        assert_eq!(cx.lanes(0), [0.0; 4]);
    }

    #[test]
    fn broadcast_masks_every_lane_past_the_width() {
        let vector = FieldValue::vec4(axiom_math::Vec4::new(1.0, 2.0, 3.0, 4.0));
        assert_eq!(broadcast(vector, FieldType::Vec4), [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(broadcast(vector, FieldType::Vec2), [1.0, 2.0, 0.0, 0.0]);
        assert_eq!(broadcast(value(3.0), FieldType::Vec3), [3.0, 3.0, 3.0, 0.0]);
        assert_eq!(broadcast(value(3.0), FieldType::Scalar), [3.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn typed_scrubs_the_lanes_the_output_type_does_not_carry() {
        assert_eq!(
            typed(FieldType::Vec2, [1.0, 2.0, 3.0, 4.0]),
            FieldValue::vec2(Vec2::new(1.0, 2.0))
        );
        assert_eq!(typed(FieldType::Scalar, [5.0, 9.0, 9.0, 9.0]), value(5.0));
    }

    #[test]
    fn the_lane_helpers_run_every_lane() {
        assert_eq!(
            zip2([1.0, 2.0, 3.0, 4.0], [10.0, 20.0, 30.0, 40.0], |a, b| a + b),
            [11.0, 22.0, 33.0, 44.0]
        );
        assert_eq!(dot([1.0, 2.0, 3.0, 4.0], [1.0, 1.0, 1.0, 1.0]), 10.0);
    }

    /// Two nodes: a `Point` source and the `Length` of it. The fold must reach
    /// the second through the first's register.
    fn nodes() -> Vec<Node> {
        vec![
            Node::new(FieldOp::Point.code(), Vec::new(), Vec::new()),
            Node::new(
                FieldOp::Length.code(),
                Vec::new(),
                vec![NodeId::from_raw(0)],
            ),
        ]
    }

    #[test]
    fn the_fold_reaches_a_node_through_its_inputs_registers() {
        let context = EvalContext::new(
            Vec3::new(3.0, 4.0, 0.0),
            Vec2::ZERO,
            Vec3::UNIT_Y,
            Seconds::finite_or_zero(0.0),
        );
        let table = FieldParams::new();
        assert_eq!(
            evaluate(&nodes(), NodeId::from_raw(1), &context, &table),
            value(5.0)
        );
        assert_eq!(
            evaluate(&nodes(), NodeId::from_raw(0), &context, &table),
            FieldValue::vec3(Vec3::new(3.0, 4.0, 0.0))
        );
    }

    #[test]
    fn an_input_naming_no_register_reads_as_the_zero_default() {
        // A hostile node whose input id is past the register file entirely.
        let hostile = vec![Node::new(
            FieldOp::Abs.code(),
            Vec::new(),
            vec![NodeId::from_raw(9_000)],
        )];
        let table = FieldParams::new();
        assert_eq!(
            evaluate(&hostile, NodeId::from_raw(0), &EvalContext::ORIGIN, &table),
            FieldValue::ZERO
        );
    }

    #[test]
    fn a_node_carrying_more_inputs_than_any_operator_consumes_is_truncated() {
        let ids: Vec<NodeId> = (0..6).map(|_| NodeId::from_raw(0)).collect();
        let flooded = vec![
            Node::new(FieldOp::Point.code(), Vec::new(), Vec::new()),
            Node::new(FieldOp::Compose.code(), vec![Param::int(4)], ids),
        ];
        let table = FieldParams::new();
        let context = EvalContext::new(
            Vec3::new(2.0, 0.0, 0.0),
            Vec2::ZERO,
            Vec3::UNIT_Y,
            Seconds::finite_or_zero(0.0),
        );
        assert_eq!(
            evaluate(&flooded, NodeId::from_raw(1), &context, &table),
            FieldValue::vec4(axiom_math::Vec4::new(2.0, 2.0, 2.0, 2.0)),
            "only the first {MAX_INPUTS} inputs can be read"
        );
    }
}
