//! Composing bound graphs into one graph.
//!
//! Everything a surface does that produces a *new* field — blending a layer
//! into what is under it, deriving a normal from a height — goes through the
//! [`Composer`] here. It inlines a whole [`FieldGraph`] into a builder,
//! remapping node ids and parameter slots, and optionally **substituting** what
//! the inlined graph reads as its sample point. That substitution is the whole
//! mechanism behind finite differences: sampling a height field at
//! `point + offset` is the same graph read against a different point node, not a
//! new operator.

use axiom_field::{EvalContext, FieldBuilder, FieldGraph, FieldId, FieldOp, FieldValue};
use axiom_recipe::{NodeId, Param};

use crate::binding::ChannelBinding;
use crate::channel::SurfaceChannel;
use crate::layer::LayerBlend;
use crate::surface_error::{SurfaceError, SurfaceErrorCode, SurfaceResult};

/// A node of a graph being inlined carries an operator code that names no field
/// operator, so there is no meaning to copy.
const UNKNOWN_OPERATOR: SurfaceError = SurfaceError::at(
    SurfaceErrorCode::UnknownOperator,
    "a node of a bound field graph carries an operator code that names no field operator",
);

/// Builds one field graph out of several.
///
/// By value, like [`FieldBuilder`] itself: the Axiom State Law forbids `&mut
/// self` on a boundary, so a composition step is a value-in / value-out
/// transform. `slots` tracks how many parameter slots the merged table already
/// holds, which is the base every further inlined graph's `Param` nodes are
/// rebased onto.
#[derive(Debug, Clone)]
pub(crate) struct Composer {
    builder: FieldBuilder,
    slots: u16,
}

impl Composer {
    /// A composer for a fresh graph named `id`.
    pub(crate) fn new(id: FieldId) -> Self {
        Composer {
            builder: FieldBuilder::new(id, 1),
            slots: 0,
        }
    }

    /// Append one operator node.
    pub(crate) fn push(
        self,
        op: FieldOp,
        params: Vec<Param>,
        inputs: Vec<NodeId>,
    ) -> (Composer, NodeId) {
        let Composer { builder, slots } = self;
        let (builder, node) = builder.push(op, params, inputs);
        (Composer { builder, slots }, node)
    }

    /// Append a literal node.
    pub(crate) fn push_const(self, value: FieldValue) -> (Composer, NodeId) {
        let Composer { builder, slots } = self;
        let (builder, node) = builder.push_const(value);
        (Composer { builder, slots }, node)
    }

    /// Finish, declaring `output` as the composed graph's result.
    pub(crate) fn build(self, output: NodeId) -> FieldGraph {
        self.builder.build(output)
    }

    /// Copy every node of `graph` into this composer, returning the id the
    /// graph's own output now has.
    ///
    /// Three things are remapped. Node inputs are looked up through a per-graph
    /// id map rather than shifted by a constant, so a substituted node needs no
    /// placeholder. `Param` nodes have their slot word rebased onto the merged
    /// table, whose values are declared here in slot order. And when
    /// `substitute` is given, every `Point` node's *result* is redirected to
    /// that node — the inlined graph is read against a different sample point.
    ///
    /// An id that names no node maps to [`NodeId::NULL`], and an output that
    /// names no node makes the composed graph's output null: both make the
    /// composed graph fail validation with the field layer's own diagnostic
    /// rather than panicking on a hostile graph.
    pub(crate) fn inline(
        self,
        graph: &FieldGraph,
        substitute: Option<NodeId>,
    ) -> SurfaceResult<(Composer, NodeId)> {
        let base = self.slots;
        let seeded = graph
            .params()
            .values()
            .iter()
            .enumerate()
            .fold(self, |composer, (index, value)| {
                let name = format!("surface/slot/{}", usize::from(base) + index);
                let Composer { builder, slots } = composer;
                let (builder, _slot) = builder.declare(&name, *value);
                Composer {
                    builder,
                    slots: slots.saturating_add(1),
                }
            });
        let point_code = FieldOp::Point.code();
        let param_code = FieldOp::Param.code();
        graph
            .recipe()
            .nodes()
            .iter()
            .enumerate()
            .try_fold(
                (seeded, Vec::<NodeId>::new()),
                |(composer, mut map), (index, node)| {
                    let inputs: Vec<NodeId> = node
                        .inputs()
                        .iter()
                        .map(|input| lookup(&map, input.raw()))
                        .collect();
                    let rebased = node.op() == param_code;
                    let params: Vec<Param> = node
                        .params()
                        .iter()
                        .enumerate()
                        .map(|(word, value)| {
                            [
                                *value,
                                Param::int(value.as_int().wrapping_add(u32::from(base))),
                            ][usize::from(rebased & (word == 0))]
                        })
                        .collect();
                    FieldOp::from_code(node.op())
                        .ok_or_else(|| UNKNOWN_OPERATOR.about_node(NodeId::from_raw(index as u32)))
                        .map(|op| {
                            let (composer, pushed) = composer.push(op, params, inputs);
                            map.push(
                                substitute
                                    .filter(|_| node.op() == point_code)
                                    .unwrap_or_else(|| pushed),
                            );
                            (composer, map)
                        })
                },
            )
            .map(|(composer, map)| {
                let output = lookup(&map, graph.output().raw());
                (composer, output)
            })
    }
}

/// The composed id of an inlined node, or [`NodeId::NULL`] when the original id
/// named no node of the inlined graph.
fn lookup(map: &[NodeId], original: u32) -> NodeId {
    map.get(original as usize)
        .copied()
        .unwrap_or_else(|| NodeId::NULL)
}

/// Compose one channel of a layer into the channel under it.
///
/// The three expressions are stated on [`LayerBlend`] and built here; all three
/// are emitted and the blend selects the output node by table index, so the
/// dead ones are dropped by the field layer's own dead-node elimination rather
/// than by a branch here.
///
/// A composition whose inputs are all constants folds back to a **constant**
/// binding, so flattening a fully-constant surface does not manufacture graphs a
/// backend would then have to lower.
pub(crate) fn blend(
    under: &ChannelBinding,
    over: &ChannelBinding,
    mask: &ChannelBinding,
    layer_blend: LayerBlend,
    channel: SurfaceChannel,
) -> SurfaceResult<ChannelBinding> {
    Composer::new(FieldId::from_raw(u64::from(channel.code())))
        .inline(&under.as_graph(), None)
        .and_then(|(composer, base)| {
            composer
                .inline(&over.as_graph(), None)
                .map(|(composer, top)| (composer, base, top))
        })
        .and_then(|(composer, base, top)| {
            composer
                .inline(&mask.as_graph(), None)
                .map(|(composer, selector)| (composer, base, top, selector))
        })
        .and_then(|(composer, base, top, selector)| {
            let (composer, masked) = composer.push(FieldOp::Mul, Vec::new(), vec![top, selector]);
            let (composer, added) = composer.push(FieldOp::Add, Vec::new(), vec![base, masked]);
            let (composer, product) = composer.push(FieldOp::Mul, Vec::new(), vec![base, top]);
            let (composer, multiplied) =
                composer.push(FieldOp::Mix, Vec::new(), vec![base, product, selector]);
            let (composer, mixed) =
                composer.push(FieldOp::Mix, Vec::new(), vec![base, top, selector]);
            composer
                .build([mixed, added, multiplied][layer_blend.index()])
                .canonicalize()
                .map_err(SurfaceError::from_field)
        })
        .map(fold_constant)
        .map_err(|error| error.about_channel(channel))
}

/// A graph that reduced to one literal becomes a constant binding again.
fn fold_constant(graph: FieldGraph) -> ChannelBinding {
    let folded = constant_of(&graph);
    folded.map_or_else(|| ChannelBinding::field(graph), ChannelBinding::constant)
}

/// The value of a graph that is exactly one literal node, or `None`.
///
/// The value is read by **evaluating** that node rather than by decoding its
/// parameter words: the field layer's evaluator is the one definition of what a
/// literal means, and a second decoder here would be a second definition.
fn constant_of(graph: &FieldGraph) -> Option<FieldValue> {
    graph
        .recipe()
        .node(graph.output())
        .filter(|node| (graph.node_count() == 1) & (node.op() == FieldOp::Const.code()))
        .and_then(|_literal| graph.evaluate(&EvalContext::ORIGIN).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::BinaryWriter;
    use axiom_recipe::Scalar;

    fn constant(value: f32) -> ChannelBinding {
        ChannelBinding::constant(FieldValue::scalar(Scalar::new(value)))
    }

    fn blended(kind: LayerBlend) -> f32 {
        blend(
            &constant(0.25),
            &constant(0.75),
            &constant(0.5),
            kind,
            SurfaceChannel::Roughness,
        )
        .expect("three scalar constants compose")
        .as_constant()
        .expect("a fully constant composition folds back to a constant")
        .as_scalar()
        .get()
    }

    /// A graph carrying one node with the raw operator `code`, built from bytes
    /// because the authoring surface will not mint an unknown operator.
    fn graph_with_raw_op(code: u16) -> FieldGraph {
        let mut writer = BinaryWriter::new();
        writer.write_u16(1);
        writer.write_u16(0);
        writer.write_u16(1);
        writer.write_u16(0);
        writer.write_u64(7);
        writer.write_u32(1);
        writer.write_u32(1);
        writer.write_u16(code);
        writer.write_u32(0);
        writer.write_u32(0);
        writer.write_u32(0);
        writer.write_u32(0);
        FieldGraph::deserialize(&writer.into_bytes()).expect("one parameterless node decodes")
    }

    #[test]
    fn every_blend_computes_its_documented_expression() {
        // Mix(0.25, 0.75, 0.5) = 0.25 + (0.75 - 0.25) * 0.5
        assert_eq!(blended(LayerBlend::Over), 0.5);
        // 0.25 + 0.75 * 0.5
        assert_eq!(blended(LayerBlend::Add), 0.625);
        // Mix(0.25, 0.25 * 0.75, 0.5)
        assert_eq!(blended(LayerBlend::Multiply), 0.21875);
    }

    #[test]
    fn a_composition_over_a_field_stays_a_field() {
        let (builder, uv) = FieldBuilder::new(FieldId::of_name("surface/compose/uv"), 1).push(
            FieldOp::Uv,
            Vec::new(),
            Vec::new(),
        );
        let (builder, lane) = builder.push(FieldOp::Component, vec![Param::int(0)], vec![uv]);
        let composed = blend(
            &constant(0.25),
            &ChannelBinding::field(builder.build(lane)),
            &constant(1.0),
            LayerBlend::Over,
            SurfaceChannel::Roughness,
        )
        .expect("a scalar field composes with a scalar constant");
        assert!(composed.as_field().is_some());
        assert_eq!(composed.ty(), Ok(axiom_field::FieldType::Scalar));
    }

    #[test]
    fn parameter_slots_are_rebased_so_two_graphs_keep_their_own_values() {
        let tuned = |name: &str, value: f32| {
            let (builder, slot) = FieldBuilder::new(FieldId::of_name(name), 1)
                .declare(name, FieldValue::scalar(Scalar::new(value)));
            let (builder, node) = builder.push_param(slot, axiom_field::FieldType::Scalar);
            ChannelBinding::field(builder.build(node))
        };
        let composed = blend(
            &tuned("surface/compose/a", 0.25),
            &tuned("surface/compose/b", 0.75),
            &constant(0.5),
            LayerBlend::Over,
            SurfaceChannel::Opacity,
        )
        .expect("two parameterised graphs compose");
        let graph = composed.as_field().expect("parameters keep it a field");
        assert_eq!(graph.params().len(), 2);
        assert_eq!(
            graph.evaluate(&EvalContext::ORIGIN),
            Ok(FieldValue::scalar(Scalar::new(0.5)))
        );
    }

    #[test]
    fn an_unknown_operator_code_names_the_node_and_the_channel() {
        let error = blend(
            &ChannelBinding::field(graph_with_raw_op(999)),
            &constant(0.5),
            &constant(0.5),
            LayerBlend::Over,
            SurfaceChannel::Emission,
        )
        .expect_err("operator code 999 names no field operator");
        assert_eq!(error.kind(), SurfaceErrorCode::UnknownOperator);
        assert_eq!(error.node(), NodeId::from_raw(0));
        assert_eq!(error.channel(), Some(SurfaceChannel::Emission));
    }

    #[test]
    fn an_input_naming_no_node_composes_to_a_graph_that_is_rejected() {
        let (builder, _node) = FieldBuilder::new(FieldId::from_raw(1), 1).push(
            FieldOp::Abs,
            Vec::new(),
            vec![NodeId::from_raw(5)],
        );
        let error = blend(
            &ChannelBinding::field(builder.build(NodeId::from_raw(0))),
            &constant(0.5),
            &constant(0.5),
            LayerBlend::Over,
            SurfaceChannel::Metallic,
        )
        .expect_err("node 5 does not exist");
        assert_eq!(error.kind(), SurfaceErrorCode::InvalidField);
    }

    #[test]
    fn an_output_naming_no_node_composes_to_a_graph_that_is_rejected() {
        let (builder, _node) =
            FieldBuilder::new(FieldId::from_raw(1), 1).push(FieldOp::Uv, Vec::new(), Vec::new());
        let error = blend(
            &ChannelBinding::field(builder.build(NodeId::from_raw(9))),
            &constant(0.5),
            &constant(0.5),
            LayerBlend::Over,
            SurfaceChannel::Metallic,
        )
        .expect_err("node 9 does not exist");
        assert_eq!(error.kind(), SurfaceErrorCode::InvalidField);
    }

    #[test]
    fn a_composition_that_will_not_fit_the_node_budget_is_rejected_not_truncated() {
        let wide = || {
            let (builder, last) = (0..130).fold(
                (FieldBuilder::new(FieldId::from_raw(2), 1), NodeId::NULL),
                |(builder, _last), _| builder.push(FieldOp::Time, Vec::new(), Vec::new()),
            );
            ChannelBinding::field(builder.build(last))
        };
        let error = blend(
            &wide(),
            &wide(),
            &constant(0.5),
            LayerBlend::Add,
            SurfaceChannel::Roughness,
        )
        .expect_err("260 nodes do not fit the field node budget");
        assert_eq!(error.kind(), SurfaceErrorCode::InvalidField);
        assert_eq!(error.field_code(), 1);
    }
}
