//! The typed field graph: a [`RecipeGraph`] plus the declared output node and
//! the parameter table.

use axiom_kernel::{BinaryReader, BinaryWriter, SchemaVersion, StableHash};
use axiom_recipe::{NodeId, RecipeGraph, MAX_NODES};

use crate::canonical;
use crate::eval;
use crate::eval_context::EvalContext;
use crate::field_error::{FieldError, FieldErrorCode, FieldResult};
use crate::field_params::FieldParams;
use crate::field_type::FieldType;
use crate::field_value::FieldValue;
use crate::type_check;

/// The wire-format version stamped into every serialized field. Bumping it
/// deliberately changes the bytes (and therefore every digest and golden), so a
/// format change can never be silent.
pub const FIELD_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// Undecodable bytes name no node: decoding stopped before one was formed.
const MALFORMED_DATA: FieldError = FieldError::at(
    FieldErrorCode::MalformedData,
    NodeId::NULL,
    "the serialized field could not be decoded from its bytes",
);

/// A graph too big for the evaluator's register file. The container's own
/// budget is the same number, so a validated graph can never trip this.
const NODE_BUDGET_EXCEEDED: FieldError = FieldError::at(
    FieldErrorCode::NodeLimitExceeded,
    NodeId::NULL,
    "the field has more nodes than the evaluator's register file holds",
);

/// A node id that names no node of the graph. The rule names no node until the
/// caller stamps the offending id on it — the declared output for
/// [`FieldGraph::deserialize`] and [`FieldGraph::validate`], the queried id for
/// [`FieldGraph::type_at`].
pub(crate) const OUTPUT_NODE_MISSING: FieldError = FieldError::at(
    FieldErrorCode::OutputNodeMissing,
    NodeId::NULL,
    "the node id does not reference a node of the graph",
);

/// A field: a pure function from an explicitly supplied [`crate::EvalContext`]
/// to a typed [`crate::FieldValue`], represented as an acyclic expression graph.
///
/// It **wraps** a [`RecipeGraph`] rather than reimplementing one. Acyclicity,
/// the node budget, dense id assignment and the canonical node encoding all come
/// from the container for free. What this layer adds is the three things a
/// container cannot have: the declared `output` node (a `RecipeGraph` has no
/// notion of a *result*), the parameter table, and the meaning of the operator
/// codes.
///
/// **Sharing is free.** A [`NodeId`] may appear in any number of later nodes'
/// inputs — that *is* the DAG's sharing, inherited from the container. There is
/// no separate "reuse" concept, no reference counting and no subgraph type.
///
/// **The bytes are the determinism proof; the digest is the label.** That is the
/// kernel's stated stance for [`StableHash`] and it holds here: compare
/// [`FieldGraph::serialize`] output to prove two fields identical, and use
/// [`FieldGraph::digest`] to index and locate them.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldGraph {
    recipe: RecipeGraph,
    output: NodeId,
    params: FieldParams,
}

impl FieldGraph {
    /// Assemble a graph from its parts. Authoring goes through
    /// [`crate::FieldBuilder`], which is what makes the graph acyclic by
    /// construction.
    pub(crate) fn new(recipe: RecipeGraph, output: NodeId, params: FieldParams) -> Self {
        FieldGraph {
            recipe,
            output,
            params,
        }
    }

    /// The underlying operator DAG.
    pub fn recipe(&self) -> &RecipeGraph {
        &self.recipe
    }

    /// The node whose value is the field's result.
    pub fn output(&self) -> NodeId {
        self.output
    }

    /// The parameter table.
    pub fn params(&self) -> &FieldParams {
        &self.params
    }

    /// How many operator nodes the field has.
    pub fn node_count(&self) -> usize {
        self.recipe.node_count()
    }

    /// The field's canonical bytes: the field schema stamp, the embedded recipe
    /// bytes, the output id (`u32`), then the parameter table.
    pub fn serialize(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        self.write_structure(&mut writer);
        self.params.write_to(&mut writer);
        writer.into_bytes()
    }

    /// The field's **structural** digest.
    ///
    /// It folds the schema stamp, the whole recipe, the output id and each
    /// parameter slot's *declared type* — deliberately **not** the parameter
    /// values. Two fields that differ only in what a parameter is set to have the
    /// same digest, because they are the same program; two fields that differ in
    /// structure do not. That is the property a program cache keys on, and it is
    /// what stops a value tweak from forcing a shader recompile.
    ///
    /// Use [`FieldGraph::serialize`] when you need the field's whole state.
    pub fn digest(&self) -> StableHash {
        let mut writer = BinaryWriter::new();
        self.write_structure(&mut writer);
        self.params.write_types_to(&mut writer);
        StableHash::of_bytes(&writer.into_bytes())
    }

    /// The part of the field both the wire form and the digest share.
    fn write_structure(&self, writer: &mut BinaryWriter) {
        FIELD_SCHEMA_VERSION.write_to(writer);
        self.recipe.write_to(writer);
        writer.write_u32(self.output.raw());
    }

    /// Decode and validate a field produced by [`Self::serialize`].
    ///
    /// Bounds-checked throughout and never panics: a buffer truncated at *any*
    /// prefix length fails cleanly, an unrecognised parameter type fails, a
    /// container that is over budget or cyclic fails with the container's own
    /// diagnostic, and an output id that names no node fails naming that id.
    pub fn deserialize(bytes: &[u8]) -> FieldResult<FieldGraph> {
        let mut reader = BinaryReader::new(bytes);
        read_parts(&mut reader)
            .and_then(|(recipe, output)| {
                FieldParams::read_from(&mut reader).map(|params| (recipe, output, params))
            })
            .and_then(|(recipe, output, params)| {
                recipe
                    .validate()
                    .map_err(FieldError::from_recipe)
                    .map(|()| FieldGraph::new(recipe, output, params))
            })
            .and_then(|graph| graph.check_output().map(|()| graph))
    }

    /// The type the field's expression at `node` evaluates to.
    ///
    /// The whole graph is type-checked to answer this: the derived type of a
    /// node is a function of everything before it, so there is no cheaper honest
    /// answer, and a graph that does not type has no types to report. Every
    /// failure names the node that caused it, which is not necessarily `node`.
    ///
    /// Preparation-time only — it is `O(nodes)` per call by construction.
    pub fn type_at(&self, node: NodeId) -> FieldResult<FieldType> {
        self.node_types().and_then(|types| {
            types
                .get(node.raw() as usize)
                .copied()
                .ok_or_else(|| OUTPUT_NODE_MISSING.about(node))
        })
    }

    /// Prove the field is a well-formed, well-typed program.
    ///
    /// The container's structural rules first (budget, and the strictly-earlier
    /// input rule that *is* the acyclicity proof), then one forward fold in id
    /// order that checks every node against its signature row and derives its
    /// type, then the declared output. Every rejection names its node.
    pub fn validate(&self) -> FieldResult<()> {
        self.node_types().and_then(|_types| self.check_output())
    }

    /// The field's canonical form: constants folded, common subexpressions
    /// shared, dead nodes dropped, ids relabelled into a fresh dense `0..n`.
    ///
    /// A pure function of the graph — nothing is memoised, because a cache is
    /// retained state — and idempotent: canonicalising a canonical graph returns
    /// it unchanged. Two graphs that compute the same thing, authored in
    /// different orders, canonicalise to **byte-identical** bytes and therefore
    /// to the same [`FieldGraph::digest`]. That is the property a program cache
    /// and a graph diff both rest on.
    ///
    /// Fails exactly when [`FieldGraph::validate`] fails: there is no canonical
    /// form of a graph that does not type.
    pub fn canonicalize(&self) -> FieldResult<FieldGraph> {
        self.validate().map(|()| canonical::canonicalize(self))
    }

    /// The value of the field's declared output under `context`.
    ///
    /// This call **is the definition of what the field means**. Every other
    /// realisation of the same graph — a shader emitted for a GPU backend, a
    /// per-triangle CPU shading path — is a mirror checked against it.
    ///
    /// **Determinism:** same graph, same context → bit-identical `f32` on every
    /// target including `wasm32`.
    ///
    /// **Allocation:** none. The evaluator folds over a fixed-size register file
    /// on the stack, so this is safe to call once per texel, per lattice node or
    /// per vertex.
    ///
    /// **Contract:** the graph is expected to have been validated once, at
    /// preparation time — [`FieldGraph::validate`] is `O(nodes)` and allocates,
    /// so re-proving it per sample would be the whole cost of a bake. Evaluating
    /// an unvalidated graph is still **total**: every operator returns a value
    /// and every lookup falls back to [`FieldValue::ZERO`], so nothing panics —
    /// the value is simply only *meaningful* for a graph that type-checks.
    pub fn evaluate(&self, context: &EvalContext) -> FieldResult<FieldValue> {
        self.evaluate_at(context, self.output)
    }

    /// The value of one node of the field under `context` — the same evaluation
    /// as [`FieldGraph::evaluate`], read at `node` instead of at the declared
    /// output. Nodes after `node` are not evaluated, because no node can depend
    /// on a later one.
    pub fn evaluate_at(&self, context: &EvalContext, node: NodeId) -> FieldResult<FieldValue> {
        self.check_budget()
            .and_then(|()| self.check_node(node))
            .map(|()| eval::evaluate(self.recipe.nodes(), node, context, &self.params))
    }

    /// Whether the field already **is** its canonical form.
    ///
    /// Answered by canonicalising and comparing bytes rather than by a second,
    /// drift-prone description of what canonical means. A graph that does not
    /// validate is not canonical.
    pub fn is_canonical(&self) -> bool {
        self.canonicalize()
            .is_ok_and(|canonical| canonical.serialize() == self.serialize())
    }

    /// The derived type of every node, in id order.
    pub(crate) fn node_types(&self) -> FieldResult<Vec<FieldType>> {
        type_check::node_types(&self.recipe, &self.params)
    }

    /// The declared output must name a node of the graph.
    fn check_output(&self) -> FieldResult<()> {
        self.check_node(self.output)
    }

    /// `node` must name a node of the graph.
    pub(crate) fn check_node(&self, node: NodeId) -> FieldResult<()> {
        ((node.raw() as usize) < self.node_count())
            .then_some(())
            .ok_or_else(|| OUTPUT_NODE_MISSING.about(node))
    }

    /// The graph must fit the evaluator's register file. The container's budget
    /// is the same number, so this can only fail for a graph
    /// [`FieldGraph::validate`] would already reject.
    fn check_budget(&self) -> FieldResult<()> {
        (self.node_count() <= MAX_NODES)
            .then_some(())
            .ok_or(NODE_BUDGET_EXCEEDED)
    }
}

/// Read the schema stamp, the embedded recipe, and the output id.
fn read_parts(reader: &mut BinaryReader<'_>) -> FieldResult<(RecipeGraph, NodeId)> {
    SchemaVersion::read_from(reader)
        .and_then(|_stamp| {
            RecipeGraph::read_from(reader).and_then(|recipe| {
                reader
                    .read_u32()
                    .map(|output| (recipe, NodeId::from_raw(output)))
            })
        })
        .map_err(|_| MALFORMED_DATA)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_op::FieldOp;
    use crate::field_type::FieldType;
    use crate::field_value::FieldValue;
    use crate::ids::FieldParamSlot;
    use axiom_recipe::{Param, RecipeId, Scalar};

    fn graph_with(output_raw: u32, tint: f32) -> FieldGraph {
        let mut recipe = RecipeGraph::new(RecipeId::from_raw(9), 1);
        let point = recipe.add(FieldOp::Point.code(), vec![], vec![]);
        recipe.add(FieldOp::Length.code(), vec![], vec![point]);
        let params = FieldParams::new().with(
            FieldParamSlot::from_raw(0),
            FieldValue::scalar(Scalar::new(tint)),
        );
        FieldGraph::new(recipe, NodeId::from_raw(output_raw), params)
    }

    fn sample() -> FieldGraph {
        graph_with(1, 0.5)
    }

    #[test]
    fn a_graph_reports_its_parts() {
        let field = sample();
        assert_eq!(field.node_count(), 2);
        assert_eq!(field.output(), NodeId::from_raw(1));
        assert_eq!(field.params().len(), 1);
        assert_eq!(field.recipe().id(), RecipeId::from_raw(9));
        assert_eq!(
            field.recipe().node(NodeId::from_raw(0)).map(|n| n.op()),
            Some(FieldOp::Point.code())
        );
    }

    #[test]
    fn a_graph_round_trips_through_its_canonical_bytes() {
        let field = sample();
        let bytes = field.serialize();
        assert_eq!(FieldGraph::deserialize(&bytes), Ok(field));
    }

    #[test]
    fn every_truncation_fails_cleanly() {
        let bytes = sample().serialize();
        (0..bytes.len()).for_each(|n| {
            assert!(
                FieldGraph::deserialize(&bytes[..n]).is_err(),
                "prefix of length {n} must not decode"
            );
        });
    }

    #[test]
    fn garbage_bytes_are_malformed() {
        let error = FieldGraph::deserialize(&[0xFF]).expect_err("one byte cannot decode");
        assert_eq!(error.kind(), FieldErrorCode::MalformedData);
        assert_eq!(error.node(), NodeId::NULL);
        assert_eq!(
            error.message(),
            "the serialized field could not be decoded from its bytes"
        );
    }

    #[test]
    fn an_output_naming_no_node_is_rejected_and_names_the_id() {
        let bytes = graph_with(7, 0.5).serialize();
        let error = FieldGraph::deserialize(&bytes).expect_err("node 7 does not exist");
        assert_eq!(error.kind(), FieldErrorCode::OutputNodeMissing);
        assert_eq!(error.code(), 5);
        assert_eq!(error.node(), NodeId::from_raw(7));
    }

    #[test]
    fn a_cyclic_container_is_rejected_with_the_containers_own_diagnostic() {
        let mut recipe = RecipeGraph::new(RecipeId::from_raw(1), 1);
        recipe.add(FieldOp::Abs.code(), vec![], vec![NodeId::from_raw(0)]);
        let bytes = FieldGraph::new(recipe, NodeId::from_raw(0), FieldParams::new()).serialize();
        let error = FieldGraph::deserialize(&bytes).expect_err("node 0 references itself");
        assert_eq!(error.kind(), FieldErrorCode::CyclicInput);
        assert_eq!(error.node(), NodeId::from_raw(0));
    }

    #[test]
    fn an_unknown_parameter_type_is_rejected() {
        let mut bytes = sample().serialize();
        // The parameter table's single slot ends the buffer: `u16` type code then
        // four `u32` lane words. Corrupt the type code in place.
        let type_at = bytes.len() - (2 + 16);
        bytes[type_at] = 0x77;
        let error = FieldGraph::deserialize(&bytes).expect_err("type code 0x77 names no type");
        assert_eq!(error.kind(), FieldErrorCode::UnknownType);
    }

    #[test]
    fn the_digest_ignores_parameter_values_but_not_structure() {
        let one = graph_with(1, 0.5);
        let other = graph_with(1, -12.75);
        assert_ne!(one.serialize(), other.serialize());
        assert_eq!(one.digest(), other.digest());

        let restructured = graph_with(0, 0.5);
        assert_ne!(one.digest(), restructured.digest());
    }

    #[test]
    fn the_digest_moves_when_a_parameter_slot_changes_type() {
        let one = sample();
        let retyped = FieldGraph::new(
            one.recipe().clone(),
            one.output(),
            FieldParams::new().with(
                FieldParamSlot::from_raw(0),
                FieldValue::vec3(axiom_math::Vec3::ZERO),
            ),
        );
        assert_eq!(retyped.params().values()[0].ty(), FieldType::Vec3);
        assert_ne!(one.digest(), retyped.digest());
    }

    #[test]
    fn the_same_graph_always_produces_the_same_bytes_and_digest() {
        assert_eq!(sample().serialize(), sample().serialize());
        assert_eq!(sample().digest(), sample().digest());
        assert_eq!(FIELD_SCHEMA_VERSION, SchemaVersion::new(1, 0));
    }

    /// The committed golden bytes of [`sample`]: field schema stamp `1.0`, the
    /// embedded recipe (its own `1.0` stamp, id `9`, version `1`, two nodes),
    /// the output id `1`, then a one-slot parameter table holding `0.5`.
    ///
    /// A format change now costs a deliberate edit here. It can never be silent.
    #[rustfmt::skip]
    const GOLDEN_BYTES: [u8; 74] = [
        1, 0, 0, 0,                             // field schema major 1, minor 0
        1, 0, 0, 0,                             // recipe schema major 1, minor 0
        9, 0, 0, 0, 0, 0, 0, 0,                 // recipe id 9
        1, 0, 0, 0,                             // recipe version 1
        2, 0, 0, 0,                             // node count 2
        1, 0, 0, 0, 0, 0, 0, 0, 0, 0,           // node 0: Point, 0 params, 0 inputs
        16, 0, 0, 0, 0, 0, 1, 0, 0, 0,          // node 1: Length, 0 params, 1 input...
        0, 0, 0, 0,                             // ...node 0
        1, 0, 0, 0,                             // output = node 1
        1, 0, 0, 0,                             // 1 parameter slot
        0, 0,                                   // slot 0 is a Scalar
        0, 0, 0, 63,                            // lane x = 0.5
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,     // lanes y, z, w unused
    ];

    /// The digest of [`sample`]'s **structural** bytes (the golden above with the
    /// parameter values replaced by their type codes).
    const GOLDEN_DIGEST: u64 = 6_235_930_630_919_881_367;

    #[test]
    fn the_golden_bytes_and_digest_are_unchanged() {
        assert_eq!(sample().serialize(), GOLDEN_BYTES);
        assert_eq!(sample().digest(), StableHash::from_raw(GOLDEN_DIGEST));
        assert_eq!(
            FieldGraph::deserialize(&GOLDEN_BYTES),
            Ok(sample()),
            "the golden bytes must still decode to the graph that produced them"
        );
    }

    #[test]
    fn a_graph_evaluates_its_declared_output_and_any_node_of_it() {
        let field = sample();
        let context = EvalContext::new(
            axiom_math::Vec3::new(3.0, 4.0, 0.0),
            axiom_math::Vec2::ZERO,
            axiom_math::Vec3::UNIT_Y,
            axiom_kernel::Seconds::finite_or_zero(0.0),
        );
        assert_eq!(
            field.evaluate(&context),
            Ok(FieldValue::scalar(Scalar::new(5.0)))
        );
        assert_eq!(
            field.evaluate_at(&context, NodeId::from_raw(0)),
            Ok(FieldValue::vec3(axiom_math::Vec3::new(3.0, 4.0, 0.0)))
        );
    }

    #[test]
    fn evaluating_a_node_the_graph_does_not_have_names_that_id() {
        let error = sample()
            .evaluate_at(&EvalContext::ORIGIN, NodeId::from_raw(9))
            .expect_err("node 9 does not exist");
        assert_eq!(error.kind(), FieldErrorCode::OutputNodeMissing);
        assert_eq!(error.node(), NodeId::from_raw(9));

        let orphaned = graph_with(7, 0.5);
        assert_eq!(
            orphaned
                .evaluate(&EvalContext::ORIGIN)
                .expect_err("the declared output does not exist")
                .node(),
            NodeId::from_raw(7)
        );
    }

    #[test]
    fn a_graph_too_big_for_the_register_file_is_refused_rather_than_evaluated() {
        let mut recipe = RecipeGraph::new(RecipeId::from_raw(1), 1);
        (0..=axiom_recipe::MAX_NODES).for_each(|_| {
            recipe.add(FieldOp::Point.code(), Vec::new(), Vec::new());
        });
        let field = FieldGraph::new(recipe, NodeId::from_raw(0), FieldParams::new());
        assert_eq!(field.node_count(), axiom_recipe::MAX_NODES + 1);
        let error = field
            .evaluate(&EvalContext::ORIGIN)
            .expect_err("the graph is over the container's budget");
        assert_eq!(error.kind(), FieldErrorCode::NodeLimitExceeded);
        assert_eq!(error.node(), NodeId::NULL);
        // The same graph is already illegal by the container's own rule, so this
        // guard can only ever fire for a graph `validate` would reject too.
        assert_eq!(
            field.validate().expect_err("over budget").kind(),
            FieldErrorCode::NodeLimitExceeded
        );
    }

    #[test]
    fn a_const_nodes_parameter_words_survive_the_round_trip() {
        let mut recipe = RecipeGraph::new(RecipeId::from_raw(2), 1);
        let words = FieldValue::vec3(axiom_math::Vec3::new(1.0, 2.0, 3.0));
        let mut params: Vec<Param> = vec![Param::int(u32::from(FieldType::Vec3.code()))];
        params.extend(words.words().iter().map(|w| Param::from_bits(*w)));
        recipe.add(FieldOp::Const.code(), params, vec![]);
        let field = FieldGraph::new(recipe, NodeId::from_raw(0), FieldParams::new());
        let decoded =
            FieldGraph::deserialize(&field.serialize()).expect("a const node round trips");
        assert_eq!(decoded, field);
        assert_eq!(decoded.recipe().node(NodeId::from_raw(0)).map(|n| n.params().len()), Some(5));
    }
}
