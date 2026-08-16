//! Type checking: **one forward fold in node id order**, and nothing else.
//!
//! Because a node's inputs may reference only strictly-earlier nodes, every
//! input's derived type is already known when the fold reaches a node. So the
//! whole checker is a `try_fold` that accumulates a `Vec<FieldType>` indexed by
//! node id — no recursion (`engine_no_recursion` is at 0), no second pass, and
//! no worklist.
//!
//! **Cycles are not checked here.** They are structurally impossible:
//! [`RecipeGraph::validate`] already proves every input id is strictly smaller
//! than its node's index, which *is* the complete cycle argument for an
//! id-ordered append graph. [`node_types`] calls it and lifts its diagnostic.
//!
//! **Scalar-broadcasts-to-vector is the language's only implicit conversion.**
//! `Add(Vec3, Scalar)` is legal and yields `Vec3`; `Add(Vec3, Vec2)` is not.

use axiom_recipe::{Node, NodeId, RecipeGraph};

use crate::field_error::{FieldError, FieldErrorCode, FieldResult};
use crate::field_op::FieldOp;
use crate::field_params::FieldParams;
use crate::field_type::FieldType;
use crate::ids::FieldParamSlot;

/// Every rule below is stated once as a constant and located at the offending
/// node on the way out, the way `recipe` states its own rules.
const UNKNOWN_OPERATOR: FieldError = FieldError::at(
    FieldErrorCode::UnknownOperator,
    NodeId::NULL,
    "a node's operator code names no field operator",
);

const WRONG_INPUT_COUNT: FieldError = FieldError::at(
    FieldErrorCode::WrongInputCount,
    NodeId::NULL,
    "a node carries a different number of inputs than its signature declares",
);

const WRONG_PARAM_COUNT: FieldError = FieldError::at(
    FieldErrorCode::WrongParamCount,
    NodeId::NULL,
    "a node carries a different number of parameter words than its signature declares",
);

const TYPE_MISMATCH: FieldError = FieldError::at(
    FieldErrorCode::TypeMismatch,
    NodeId::NULL,
    "a node's input types do not compose",
);

const COMPONENT_OUT_OF_RANGE: FieldError = FieldError::at(
    FieldErrorCode::ComponentOutOfRange,
    NodeId::NULL,
    "a Component node selects a lane its input does not have",
);

const COMPOSE_WIDTH_INVALID: FieldError = FieldError::at(
    FieldErrorCode::ComposeWidthInvalid,
    NodeId::NULL,
    "a Compose node's declared width is not in 2..=4, or its input count is not that width",
);

const UNKNOWN_PARAM_SLOT: FieldError = FieldError::at(
    FieldErrorCode::UnknownParamSlot,
    NodeId::NULL,
    "a Param node reads a slot the parameter table does not have",
);

const NON_FINITE_CONSTANT: FieldError = FieldError::at(
    FieldErrorCode::NonFiniteConstant,
    NodeId::NULL,
    "a Const node's parameter word decodes to NaN or an infinity",
);

const UNKNOWN_TYPE: FieldError = FieldError::at(
    FieldErrorCode::UnknownType,
    NodeId::NULL,
    "a node declares a type code that names no field type",
);

/// The derived type of every node, in id order.
///
/// The container's own structural rules run first, so by the time the fold
/// starts, every input id is known to be strictly smaller than its node's index.
pub(crate) fn node_types(
    recipe: &RecipeGraph,
    params: &FieldParams,
) -> FieldResult<Vec<FieldType>> {
    recipe
        .validate()
        .map_err(FieldError::from_recipe)
        .and_then(|()| fold_types(recipe, params))
}

/// The single forward fold. Each step derives one node's type from the types
/// already accumulated, so the accumulator is the answer.
fn fold_types(recipe: &RecipeGraph, params: &FieldParams) -> FieldResult<Vec<FieldType>> {
    recipe.nodes().iter().enumerate().try_fold(
        Vec::with_capacity(recipe.node_count()),
        |types, (index, node)| {
            check_node(node, NodeId::from_raw(index as u32), &types, params).map(|ty| {
                let mut types = types;
                types.push(ty);
                types
            })
        },
    )
}

/// Decode the operator, check its shape, then derive its output type.
fn check_node(
    node: &Node,
    id: NodeId,
    types: &[FieldType],
    params: &FieldParams,
) -> FieldResult<FieldType> {
    FieldOp::from_code(node.op())
        .ok_or_else(|| UNKNOWN_OPERATOR.about(id))
        .map(|op| NodeCheck {
            op,
            node,
            id,
            types,
            params,
        })
        .and_then(|cx| cx.check_param_count().map(|()| cx))
        .and_then(|cx| cx.check_input_count().map(|()| cx))
        .and_then(NodeCheck::derive_type)
}

/// One node under the checker's eye, with every lookup it needs made total.
///
/// `Copy`, so it threads through a combinator chain without a clone and without
/// a borrow dance.
#[derive(Clone, Copy)]
struct NodeCheck<'a> {
    op: FieldOp,
    node: &'a Node,
    id: NodeId,
    /// The derived types of nodes `0..id`, which is every node this one may
    /// reference.
    types: &'a [FieldType],
    params: &'a FieldParams,
}

impl NodeCheck<'_> {
    /// The derived type of input slot `slot`. Total: a slot or an id the graph
    /// does not have reads as [`FieldType::Scalar`], which cannot happen once
    /// the container's structural rules have passed but keeps every rule
    /// function free of a panic path.
    fn input_type(self, slot: usize) -> FieldType {
        self.node
            .inputs()
            .get(slot)
            .and_then(|input| self.types.get(input.raw() as usize))
            .map_or(FieldType::Scalar, |ty| *ty)
    }

    /// Parameter word `slot`, or `0` when the node has no such word.
    fn word(self, slot: usize) -> u32 {
        self.node.params().get(slot).map_or(0, |param| param.bits())
    }

    /// The widest input type. [`FieldType`] orders by width, so this is a plain
    /// `max` fold.
    fn widest_input(self) -> FieldType {
        (0..self.node.inputs().len()).fold(FieldType::Scalar, |widest, slot| {
            widest.max(self.input_type(slot))
        })
    }

    /// Whether every input is either a scalar (which broadcasts) or exactly
    /// `widest`.
    fn inputs_agree(self, widest: FieldType) -> bool {
        (0..self.node.inputs().len()).all(|slot| {
            let ty = self.input_type(slot);
            (ty == FieldType::Scalar) | (ty == widest)
        })
    }

    /// The node's parameter-word count must equal its signature's.
    fn check_param_count(self) -> FieldResult<()> {
        (self.node.params().len() == usize::from(self.op.signature().params()))
            .then_some(())
            .ok_or_else(|| WRONG_PARAM_COUNT.about(self.id))
    }

    /// The node's input count must equal its signature's — except for the one
    /// operator whose arity a parameter decides (`Compose`), where the whole
    /// width rule is a single `ComposeWidthInvalid`.
    fn check_input_count(self) -> FieldResult<()> {
        let signature = self.op.signature();
        let variadic = usize::from(signature.has_param_decided_inputs());
        let count = self.node.inputs().len();
        let width = self.word(0) as usize;
        let ok = [
            count == usize::from(signature.inputs()),
            (2..=4).contains(&width) & (count == width),
        ][variadic];
        let rule = [WRONG_INPUT_COUNT, COMPOSE_WIDTH_INVALID][variadic];
        ok.then_some(()).ok_or_else(|| rule.about(self.id))
    }

    /// The output type, by the rule the signature table names.
    fn derive_type(self) -> FieldResult<FieldType> {
        RULES[self.op.signature().kind() as usize](self)
    }
}

/// One rule per [`crate::SignatureKind`], indexed by the fieldless discriminant
/// — the `const [fn; N]` dispatch table the algebra uses everywhere, in place of
/// a `match` the Branchless Law forbids.
///
/// `ScalarOut` and `Vec3Out` share [`fixed_type`] because the concrete type they
/// yield already rides in the signature row.
type Rule = fn(NodeCheck<'_>) -> FieldResult<FieldType>;

const RULES: [Rule; 6] = [
    fixed_type,
    declared_type,
    width_generic,
    fixed_type,
    fixed_type,
    explicit,
];

/// `Fixed` / `ScalarOut` / `Vec3Out`: the type is in the signature row.
fn fixed_type(cx: NodeCheck<'_>) -> FieldResult<FieldType> {
    Ok(cx.op.signature().fixed_type())
}

/// `WidthGeneric`: the widest input wins, and every non-scalar input must agree
/// with it. A scalar broadcasting to a vector is the only implicit conversion in
/// the language.
fn width_generic(cx: NodeCheck<'_>) -> FieldResult<FieldType> {
    let widest = cx.widest_input();
    cx.inputs_agree(widest)
        .then_some(widest)
        .ok_or_else(|| TYPE_MISMATCH.about(cx.id))
}

/// `FromParams`: `Const` and `Param` declare their type in a parameter word, but
/// in different slots and with different companion checks. Both arms are
/// evaluated — each is pure, total and cheap — and the operator selects.
fn declared_type(cx: NodeCheck<'_>) -> FieldResult<FieldType> {
    [const_type(cx), param_type(cx)][usize::from(cx.op == FieldOp::Param)]
}

/// A `Const` declares its type in word 0 and its four lanes in words 1..5. A
/// lane that decodes to NaN or an infinity is rejected **at the door**: a
/// non-finite constant propagates silently to every consumer, and
/// `ScalarField::new` already refuses such a value downstream.
fn const_type(cx: NodeCheck<'_>) -> FieldResult<FieldType> {
    let finite = (1..5).all(|slot| f32::from_bits(cx.word(slot)).is_finite());
    decoded_type(cx.word(0))
        .ok_or_else(|| UNKNOWN_TYPE.about(cx.id))
        .and_then(|ty| {
            finite
                .then_some(ty)
                .ok_or_else(|| NON_FINITE_CONSTANT.about(cx.id))
        })
}

/// A `Param` names its slot in word 0 and its declared type in word 1. The slot
/// must exist, and the type it declares must be the type that slot holds —
/// otherwise a value change could not be type-preserving, which is the one
/// property the parameter table exists to guarantee.
fn param_type(cx: NodeCheck<'_>) -> FieldResult<FieldType> {
    let held = u16::try_from(cx.word(0))
        .ok()
        .map(FieldParamSlot::from_raw)
        .and_then(|slot| cx.params.get(slot))
        .map(|value| value.ty());
    decoded_type(cx.word(1))
        .ok_or_else(|| UNKNOWN_TYPE.about(cx.id))
        .and_then(|declared| {
            held.ok_or_else(|| UNKNOWN_PARAM_SLOT.about(cx.id))
                .map(|actual| (declared, actual))
        })
        .and_then(|(declared, actual)| {
            (declared == actual)
                .then_some(declared)
                .ok_or_else(|| TYPE_MISMATCH.about(cx.id))
        })
}

/// The [`FieldType`] a raw parameter word names, or `None`.
fn decoded_type(word: u32) -> Option<FieldType> {
    u16::try_from(word).ok().and_then(FieldType::from_code)
}

/// `Explicit`: `Compose` reads its width from a parameter, `Component` always
/// yields a scalar but must name a lane its input has.
fn explicit(cx: NodeCheck<'_>) -> FieldResult<FieldType> {
    [component_type(cx), Ok(compose_type(cx))][usize::from(cx.op == FieldOp::Compose)]
}

/// The type a declared `Compose` width names — [`FieldType::of_width`], the one
/// statement of that rule, which the evaluator's `Compose` reads too.
fn compose_type(cx: NodeCheck<'_>) -> FieldType {
    FieldType::of_width(cx.word(0))
}

/// A `Component` selects one lane of its input and yields a scalar.
fn component_type(cx: NodeCheck<'_>) -> FieldResult<FieldType> {
    (cx.word(0) < u32::from(cx.input_type(0).lanes()))
        .then_some(FieldType::Scalar)
        .ok_or_else(|| COMPONENT_OUT_OF_RANGE.about(cx.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_recipe::{Param, RecipeGraph, RecipeId};

    use crate::field_graph::FieldGraph;
    use crate::field_value::FieldValue;

    /// One node, spelled as raw words so a test can build shapes the typed
    /// authoring surface deliberately cannot express — an operator code that
    /// names nothing, a wrong arity, a hostile type code.
    struct Raw {
        op: u16,
        words: Vec<u32>,
        inputs: Vec<u32>,
    }

    fn node(op: FieldOp, words: &[u32], inputs: &[u32]) -> Raw {
        Raw {
            op: op.code(),
            words: words.to_vec(),
            inputs: inputs.to_vec(),
        }
    }

    fn graph_of(nodes: Vec<Raw>, output: u32, params: FieldParams) -> FieldGraph {
        let mut recipe = RecipeGraph::new(RecipeId::from_raw(1), 1);
        nodes.iter().for_each(|raw| {
            recipe.add(
                raw.op,
                raw.words.iter().map(|word| Param::int(*word)).collect(),
                raw.inputs
                    .iter()
                    .map(|input| NodeId::from_raw(*input))
                    .collect(),
            );
        });
        FieldGraph::new(recipe, NodeId::from_raw(output), params)
    }

    fn graph(nodes: Vec<Raw>, output: u32) -> FieldGraph {
        graph_of(nodes, output, FieldParams::new())
    }

    /// The five parameter words of a `Const` carrying a scalar.
    fn scalar_const(value: f32) -> Raw {
        Raw {
            op: FieldOp::Const.code(),
            words: vec![
                u32::from(FieldType::Scalar.code()),
                value.to_bits(),
                0,
                0,
                0,
            ],
            inputs: Vec::new(),
        }
    }

    fn rejection(field: &FieldGraph) -> (FieldErrorCode, NodeId) {
        let error = field.validate().expect_err("the graph is invalid");
        (error.kind(), error.node())
    }

    // ----- one rejection test per code, each naming its node -----------------

    #[test]
    fn an_operator_code_that_names_nothing_is_rejected_at_its_node() {
        let field = graph(
            vec![
                node(FieldOp::Point, &[], &[]),
                Raw {
                    op: 23,
                    words: Vec::new(),
                    inputs: Vec::new(),
                },
            ],
            1,
        );
        assert_eq!(
            rejection(&field),
            (FieldErrorCode::UnknownOperator, NodeId::from_raw(1))
        );
    }

    #[test]
    fn a_wrong_input_count_is_rejected_at_its_node() {
        let field = graph(
            vec![
                node(FieldOp::Point, &[], &[]),
                node(FieldOp::Add, &[], &[0]),
            ],
            1,
        );
        assert_eq!(
            rejection(&field),
            (FieldErrorCode::WrongInputCount, NodeId::from_raw(1))
        );
    }

    #[test]
    fn a_wrong_parameter_word_count_is_rejected_at_its_node() {
        let field = graph(vec![node(FieldOp::Const, &[0, 0, 0], &[])], 0);
        assert_eq!(
            rejection(&field),
            (FieldErrorCode::WrongParamCount, NodeId::from_raw(0))
        );
    }

    #[test]
    fn disagreeing_non_scalar_widths_are_a_type_mismatch_at_the_consuming_node() {
        let field = graph(
            vec![
                node(FieldOp::Point, &[], &[]),
                node(FieldOp::Uv, &[], &[]),
                node(FieldOp::Add, &[], &[0, 1]),
            ],
            2,
        );
        assert_eq!(
            rejection(&field),
            (FieldErrorCode::TypeMismatch, NodeId::from_raw(2))
        );
    }

    #[test]
    fn a_param_node_that_disagrees_with_its_slot_is_a_type_mismatch_at_its_node() {
        let table = FieldParams::new().with(
            FieldParamSlot::from_raw(0),
            FieldValue::scalar(axiom_recipe::Scalar::new(0.5)),
        );
        let field = graph_of(
            vec![node(
                FieldOp::Param,
                &[0, u32::from(FieldType::Vec3.code())],
                &[],
            )],
            0,
            table,
        );
        assert_eq!(
            rejection(&field),
            (FieldErrorCode::TypeMismatch, NodeId::from_raw(0))
        );
    }

    #[test]
    fn a_component_past_its_inputs_width_is_rejected_at_its_node() {
        let field = graph(
            vec![
                node(FieldOp::Uv, &[], &[]),
                node(FieldOp::Component, &[2], &[0]),
            ],
            1,
        );
        assert_eq!(
            rejection(&field),
            (FieldErrorCode::ComponentOutOfRange, NodeId::from_raw(1))
        );
    }

    #[test]
    fn a_compose_width_outside_two_to_four_is_rejected_at_its_node() {
        let widths = [(1_u32, 1_usize), (5, 5), (0, 0)];
        widths.iter().for_each(|(width, arity)| {
            let inputs: Vec<u32> = (0..*arity as u32).map(|_| 0).collect();
            let field = graph(
                vec![
                    scalar_const(1.0),
                    node(FieldOp::Compose, &[*width], &inputs),
                ],
                1,
            );
            assert_eq!(
                rejection(&field),
                (FieldErrorCode::ComposeWidthInvalid, NodeId::from_raw(1)),
                "width {width} must be rejected"
            );
        });
    }

    #[test]
    fn a_compose_whose_input_count_is_not_its_width_is_rejected_at_its_node() {
        let field = graph(
            vec![
                scalar_const(1.0),
                node(FieldOp::Compose, &[3], &[0, 0]),
            ],
            1,
        );
        assert_eq!(
            rejection(&field),
            (FieldErrorCode::ComposeWidthInvalid, NodeId::from_raw(1))
        );
    }

    #[test]
    fn a_param_reading_a_slot_the_table_lacks_is_rejected_at_its_node() {
        let field = graph(
            vec![node(
                FieldOp::Param,
                &[5, u32::from(FieldType::Scalar.code())],
                &[],
            )],
            0,
        );
        assert_eq!(
            rejection(&field),
            (FieldErrorCode::UnknownParamSlot, NodeId::from_raw(0))
        );
    }

    #[test]
    fn a_non_finite_constant_is_rejected_at_its_node() {
        let hostile = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
        hostile.iter().for_each(|value| {
            let field = graph(vec![node(FieldOp::Point, &[], &[]), scalar_const(*value)], 1);
            assert_eq!(
                rejection(&field),
                (FieldErrorCode::NonFiniteConstant, NodeId::from_raw(1)),
                "{value} must not enter a graph"
            );
        });
    }

    #[test]
    fn a_declared_type_code_that_names_nothing_is_rejected_at_its_node() {
        let bad_const = graph(
            vec![node(FieldOp::Const, &[9, 0, 0, 0, 0], &[])],
            0,
        );
        assert_eq!(
            rejection(&bad_const),
            (FieldErrorCode::UnknownType, NodeId::from_raw(0))
        );
        let bad_param = graph(vec![node(FieldOp::Param, &[0, 9], &[])], 0);
        assert_eq!(
            rejection(&bad_param),
            (FieldErrorCode::UnknownType, NodeId::from_raw(0))
        );
    }

    #[test]
    fn an_output_naming_no_node_is_rejected_naming_that_id() {
        let field = graph(vec![node(FieldOp::Point, &[], &[])], 4);
        assert_eq!(
            rejection(&field),
            (FieldErrorCode::OutputNodeMissing, NodeId::from_raw(4))
        );
    }

    #[test]
    fn the_containers_own_diagnostic_is_lifted_unchanged() {
        let mut recipe = RecipeGraph::new(RecipeId::from_raw(1), 1);
        recipe.add(FieldOp::Abs.code(), Vec::new(), vec![NodeId::from_raw(0)]);
        let field = FieldGraph::new(recipe, NodeId::from_raw(0), FieldParams::new());
        assert_eq!(
            rejection(&field),
            (FieldErrorCode::CyclicInput, NodeId::from_raw(0))
        );
    }

    // ----- acceptance --------------------------------------------------------

    #[test]
    fn a_scalar_broadcasts_to_a_vector_but_two_widths_do_not_meet() {
        let broadcast = graph(
            vec![
                node(FieldOp::Point, &[], &[]),
                scalar_const(2.0),
                node(FieldOp::Add, &[], &[0, 1]),
            ],
            2,
        );
        assert_eq!(broadcast.validate(), Ok(()));
        assert_eq!(broadcast.type_of(NodeId::from_raw(2)), Ok(FieldType::Vec3));
        assert_eq!(broadcast.type_of(NodeId::from_raw(1)), Ok(FieldType::Scalar));
    }

    #[test]
    fn every_signature_rule_derives_the_type_its_row_promises() {
        let field = graph_of(
            vec![
                node(FieldOp::Point, &[], &[]),                                   // 0 Vec3
                node(FieldOp::Uv, &[], &[]),                                      // 1 Vec2
                node(FieldOp::Time, &[], &[]),                                    // 2 Scalar
                node(FieldOp::Param, &[0, u32::from(FieldType::Scalar.code())], &[]), // 3 Scalar
                node(FieldOp::Add, &[], &[0, 2]),                                 // 4 Vec3
                node(FieldOp::Length, &[], &[0]),                                 // 5 Scalar
                node(FieldOp::Normalize, &[], &[0]),                              // 6 Vec3
                node(FieldOp::Component, &[1], &[1]),                             // 7 Scalar
                node(FieldOp::Compose, &[4], &[2, 2, 2, 2]),                      // 8 Vec4
                scalar_const(0.25),                                               // 9 Scalar
            ],
            8,
            FieldParams::new().with(
                FieldParamSlot::from_raw(0),
                FieldValue::scalar(axiom_recipe::Scalar::new(1.0)),
            ),
        );
        assert_eq!(field.validate(), Ok(()));
        let derived: Vec<FieldType> = (0..10)
            .map(|id| {
                field
                    .type_of(NodeId::from_raw(id))
                    .expect("the graph type-checks")
            })
            .collect();
        assert_eq!(
            derived,
            vec![
                FieldType::Vec3,
                FieldType::Vec2,
                FieldType::Scalar,
                FieldType::Scalar,
                FieldType::Vec3,
                FieldType::Scalar,
                FieldType::Vec3,
                FieldType::Scalar,
                FieldType::Vec4,
                FieldType::Scalar,
            ]
        );
    }

    #[test]
    fn a_compose_width_names_the_vector_type_it_builds() {
        let widths = [
            (2_u32, FieldType::Vec2),
            (3, FieldType::Vec3),
            (4, FieldType::Vec4),
        ];
        widths.iter().for_each(|(width, ty)| {
            let inputs: Vec<u32> = (0..*width).map(|_| 0).collect();
            let field = graph(
                vec![
                    node(FieldOp::Time, &[], &[]),
                    node(FieldOp::Compose, &[*width], &inputs),
                ],
                1,
            );
            assert_eq!(field.type_of(NodeId::from_raw(1)), Ok(*ty));
        });
    }

    #[test]
    fn asking_about_a_node_the_graph_does_not_have_names_that_id() {
        let field = graph(vec![node(FieldOp::Point, &[], &[])], 0);
        let error = field
            .type_of(NodeId::from_raw(9))
            .expect_err("node 9 does not exist");
        assert_eq!(error.kind(), FieldErrorCode::OutputNodeMissing);
        assert_eq!(error.node(), NodeId::from_raw(9));
    }

    #[test]
    fn a_type_error_anywhere_denies_every_type_query() {
        let field = graph(
            vec![
                node(FieldOp::Point, &[], &[]),
                node(FieldOp::Uv, &[], &[]),
                node(FieldOp::Add, &[], &[0, 1]),
            ],
            2,
        );
        let error = field
            .type_of(NodeId::from_raw(0))
            .expect_err("a graph that does not type has no types to report");
        assert_eq!(error.kind(), FieldErrorCode::TypeMismatch);
        assert_eq!(error.node(), NodeId::from_raw(2));
    }
}
