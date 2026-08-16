//! The append-only authoring surface for a field graph.

use axiom_kernel::StableHash;
use axiom_recipe::{NodeId, Param, RecipeGraph, RecipeId};

use crate::field_graph::FieldGraph;
use crate::field_op::FieldOp;
use crate::field_params::FieldParams;
use crate::field_type::FieldType;
use crate::field_value::FieldValue;
use crate::ids::{FieldId, FieldParamSlot};

/// Builds a [`FieldGraph`] by appending operator nodes.
///
/// Every `push_*` returns the builder **by value** together with the
/// [`NodeId`] it just minted, and a node's inputs must be ids the builder has
/// already returned. That is the same shape as [`RecipeGraph::add`], and it is
/// what makes the graph acyclic *by construction* rather than by search.
///
/// The by-value threading is not stylistic: the Axiom State Law forbids `&mut
/// self` on a public boundary, so a builder step is a value-in / value-out
/// transform like every other engine computation.
///
/// The builder also owns the **name → slot** map for the parameter table. Slot
/// names are minted deterministically from a string through the kernel's
/// [`StableHash`], the `StateId::of_path` pattern, so an agent can address a
/// parameter by name — but the name never enters the wire format, which carries
/// the dense slot index only.
#[derive(Debug, Clone)]
pub struct FieldBuilder {
    recipe: RecipeGraph,
    params: FieldParams,
    names: Vec<StableHash>,
}

impl FieldBuilder {
    /// A new, empty builder for the field named by `id`, at content `version`.
    pub fn new(id: FieldId, version: u32) -> Self {
        FieldBuilder {
            recipe: RecipeGraph::new(RecipeId::from_raw(id.raw()), version),
            params: FieldParams::new(),
            names: Vec::new(),
        }
    }

    /// How many nodes have been appended so far. The next id is this value.
    pub fn node_count(&self) -> usize {
        self.recipe.node_count()
    }

    /// Append one operator node with raw parameter words and input links.
    ///
    /// The general form. `params` must be the words the operator's
    /// [`crate::FieldSignature`] describes, and `inputs` must be ids this
    /// builder already returned.
    pub fn push(self, op: FieldOp, params: Vec<Param>, inputs: Vec<NodeId>) -> (Self, NodeId) {
        let FieldBuilder {
            mut recipe,
            params: table,
            names,
        } = self;
        let id = recipe.add(op.code(), params, inputs);
        (
            FieldBuilder {
                recipe,
                params: table,
                names,
            },
            id,
        )
    }

    /// Append a `Const` node carrying `value`, encoding the canonical five
    /// parameter words: the declared type, then the four lanes.
    pub fn push_const(self, value: FieldValue) -> (Self, NodeId) {
        let mut params = Vec::with_capacity(5);
        params.push(Param::int(u32::from(value.ty().code())));
        params.extend(value.words().iter().map(|word| Param::from_bits(*word)));
        self.push(FieldOp::Const, params, Vec::new())
    }

    /// Append a `Param` node reading `slot`, declared to carry `ty`.
    pub fn push_param(self, slot: FieldParamSlot, ty: FieldType) -> (Self, NodeId) {
        self.push(
            FieldOp::Param,
            vec![Param::int(u32::from(slot.raw())), Param::int(u32::from(ty.code()))],
            Vec::new(),
        )
    }

    /// Declare the parameter named `name` with an initial `value`, returning its
    /// slot.
    ///
    /// Re-declaring a name returns the **same slot** and replaces the value, so
    /// retuning a field never renumbers its slots — the property that keeps a
    /// value change out of [`FieldGraph::digest`].
    pub fn declare(self, name: &str, value: FieldValue) -> (Self, FieldParamSlot) {
        let key = StableHash::of_bytes(name.as_bytes());
        let index = self
            .names
            .iter()
            .position(|known| *known == key)
            .unwrap_or_else(|| self.names.len());
        let slot = FieldParamSlot::from_raw(index as u16);
        let FieldBuilder {
            recipe,
            params,
            mut names,
        } = self;
        names.resize(names.len().max(index + 1), key);
        (
            FieldBuilder {
                recipe,
                params: params.with(slot, value),
                names,
            },
            slot,
        )
    }

    /// Finish, declaring `output` as the node whose value is the field's result.
    pub fn build(self, output: NodeId) -> FieldGraph {
        FieldGraph::new(self.recipe, output, self.params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Vec2, Vec3};
    use axiom_recipe::Scalar;

    #[test]
    fn ids_are_dense_insertion_indices() {
        let builder = FieldBuilder::new(FieldId::of_name("field/dense"), 1);
        assert_eq!(builder.node_count(), 0);
        let (builder, a) = builder.push(FieldOp::Point, Vec::new(), Vec::new());
        let (builder, b) = builder.push(FieldOp::Uv, Vec::new(), Vec::new());
        let (builder, c) = builder.push(FieldOp::Length, Vec::new(), vec![a]);
        assert_eq!((a.raw(), b.raw(), c.raw()), (0, 1, 2));
        assert_eq!(builder.node_count(), 3);
    }

    #[test]
    fn the_built_graph_carries_the_id_output_and_params() {
        let id = FieldId::of_name("field/built");
        let (builder, slot) =
            FieldBuilder::new(id, 4).declare("tint", FieldValue::vec3(Vec3::new(1.0, 0.0, 0.0)));
        let (builder, node) = builder.push_param(slot, FieldType::Vec3);
        let field = builder.build(node);
        assert_eq!(field.recipe().id(), RecipeId::from_raw(id.raw()));
        assert_eq!(field.recipe().version(), 4);
        assert_eq!(field.output(), node);
        assert_eq!(
            field.params().get(slot),
            Some(FieldValue::vec3(Vec3::new(1.0, 0.0, 0.0)))
        );
    }

    #[test]
    fn a_const_node_encodes_its_type_then_its_four_lanes() {
        let (builder, node) = FieldBuilder::new(FieldId::from_raw(1), 1)
            .push_const(FieldValue::vec2(Vec2::new(0.5, 1.5)));
        let field = builder.build(node);
        let words: Vec<u32> = field
            .recipe()
            .node(node)
            .expect("the const node exists")
            .params()
            .iter()
            .map(|p| p.bits())
            .collect();
        assert_eq!(
            words,
            vec![
                u32::from(FieldType::Vec2.code()),
                0.5_f32.to_bits(),
                1.5_f32.to_bits(),
                0,
                0,
            ]
        );
        assert_eq!(words.len() as u8, FieldOp::Const.signature().params());
    }

    #[test]
    fn a_param_node_encodes_its_slot_then_its_type() {
        let (builder, node) = FieldBuilder::new(FieldId::from_raw(1), 1)
            .push_param(FieldParamSlot::from_raw(3), FieldType::Vec4);
        let field = builder.build(node);
        let words: Vec<u32> = field
            .recipe()
            .node(node)
            .expect("the param node exists")
            .params()
            .iter()
            .map(|p| p.bits())
            .collect();
        assert_eq!(words, vec![3, u32::from(FieldType::Vec4.code())]);
        assert_eq!(words.len() as u8, FieldOp::Param.signature().params());
    }

    #[test]
    fn distinct_names_get_distinct_dense_slots() {
        let (builder, first) = FieldBuilder::new(FieldId::from_raw(1), 1)
            .declare("roughness", FieldValue::scalar(Scalar::new(0.2)));
        let (builder, second) = builder.declare("metallic", FieldValue::scalar(Scalar::new(0.0)));
        assert_eq!((first.raw(), second.raw()), (0, 1));
        let field = builder.build(NodeId::from_raw(0));
        assert_eq!(field.params().len(), 2);
    }

    #[test]
    fn redeclaring_a_name_reuses_its_slot_and_replaces_the_value() {
        let (builder, first) = FieldBuilder::new(FieldId::from_raw(1), 1)
            .declare("roughness", FieldValue::scalar(Scalar::new(0.2)));
        let (builder, other) = builder.declare("metallic", FieldValue::scalar(Scalar::new(0.0)));
        let (builder, again) = builder.declare("roughness", FieldValue::scalar(Scalar::new(0.9)));
        assert_eq!(again, first);
        assert_ne!(again, other);
        let field = builder.build(NodeId::from_raw(0));
        assert_eq!(field.params().len(), 2);
        assert_eq!(
            field.params().get(first),
            Some(FieldValue::scalar(Scalar::new(0.9)))
        );
    }

    #[test]
    fn a_graph_exercising_every_operator_round_trips() {
        let start = (
            FieldBuilder::new(FieldId::of_name("field/every-op"), 1),
            Vec::<NodeId>::new(),
        );
        let (builder, ids) = FieldOp::ALL.iter().fold(start, |(builder, mut ids), op| {
            let signature = op.signature();
            // A parameter-decided arity (`Compose`) is exercised with one input.
            let variadic = usize::from(signature.has_param_decided_inputs());
            let arity = usize::from([signature.inputs(), 1][variadic]);
            let inputs: Vec<NodeId> = ids.iter().rev().take(arity).copied().collect();
            let params: Vec<Param> = (0..signature.params())
                .map(|word| Param::int(u32::from(word)))
                .collect();
            let (builder, id) = builder.push(*op, params, inputs);
            ids.push(id);
            (builder, ids)
        });
        let field = builder.build(*ids.last().expect("every operator was appended"));
        assert_eq!(field.node_count(), crate::field_op::FIELD_OP_COUNT);
        assert_eq!(
            field
                .recipe()
                .nodes()
                .iter()
                .map(|node| node.op())
                .collect::<Vec<u16>>(),
            FieldOp::ALL.iter().map(|op| op.code()).collect::<Vec<u16>>()
        );
        assert_eq!(
            crate::field_graph::FieldGraph::deserialize(&field.serialize()),
            Ok(field.clone())
        );
        let empty = FieldBuilder::new(FieldId::from_raw(1), 1).build(NodeId::NULL);
        assert_ne!(field.digest(), empty.digest());
    }

    #[test]
    fn retuning_a_parameter_leaves_the_digest_alone() {
        let build_with = |roughness: f32| {
            let (builder, slot) = FieldBuilder::new(FieldId::of_name("field/tuned"), 1)
                .declare("roughness", FieldValue::scalar(Scalar::new(roughness)));
            let (builder, node) = builder.push_param(slot, FieldType::Scalar);
            builder.build(node)
        };
        let low = build_with(0.1);
        let high = build_with(0.9);
        assert_ne!(low.serialize(), high.serialize());
        assert_eq!(low.digest(), high.digest());
    }
}
