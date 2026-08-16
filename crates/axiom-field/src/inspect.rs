//! Reading a graph: what each node is, what it produces, who consumes it, and a
//! schema-stamped description of the whole thing.
//!
//! This is the half of the layer an **agent** uses. Everything here is a pure
//! read — nothing memoises, nothing is stored, and no accessor hands back a
//! handle to internal state that could go stale.
//!
//! ## `dependents_of` is a forward scan, and that is deliberate
//!
//! The obvious implementation is a stored reverse index. It is also the wrong
//! one: an index is retained state, it has to be invalidated on every rewrite,
//! and every rewrite here returns a **new graph** precisely so that nothing has
//! to be invalidated. The scan is `O(nodes × inputs)` per call, bounded by
//! [`MAX_NODES`] = 256 and by an operator arity of at most three, so the worst
//! case is a few hundred comparisons. **It is an authoring-time query. Never
//! call it from a frame path.**
//!
//! ## What a description is, and what `explain` is not
//!
//! [`FieldDescription`] is a **wire form**: schema-stamped, length-prefixed,
//! byte-exact, and decodable back into itself, following the codec shape
//! `crates/axiom-introspect/src/world_tag.rs` established. It is what an agent
//! stores, diffs against, and posts across a process boundary.
//!
//! [`FieldExplanation`] is **not** a wire form. It is deterministic text, one
//! line per node in id order, for a human or a log — it has no reader, there is
//! no textual authoring format in this layer, and nothing downstream may parse
//! it. Do not golden it as though it were a contract.

use axiom_kernel::{BinaryReader, BinaryWriter, SchemaVersion, StableHash};
use axiom_recipe::{NodeId, MAX_NODES};

use crate::field_error::{FieldError, FieldErrorCode, FieldResult};
use crate::field_graph::{FieldGraph, OUTPUT_NODE_MISSING};
use crate::field_op::FieldOp;
use crate::field_type::FieldType;

/// The wire-format version stamped into every serialized [`FieldDescription`].
/// Bumping it deliberately changes the bytes, so a format change can never be
/// silent.
pub const FIELD_DESCRIPTION_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// A node whose operator code names no [`FieldOp`], so there is nothing to
/// describe it as.
const UNKNOWN_OPERATOR: FieldError = FieldError::at(
    FieldErrorCode::UnknownOperator,
    NodeId::NULL,
    "the node carries an operator code that names no field operator",
);

/// A described node whose type code names no [`FieldType`].
const UNKNOWN_TYPE: FieldError = FieldError::at(
    FieldErrorCode::UnknownType,
    NodeId::NULL,
    "the description declares a type code that names no field type",
);

/// Undecodable description bytes name no node: decoding stopped before one was
/// formed.
const MALFORMED_DESCRIPTION: FieldError = FieldError::at(
    FieldErrorCode::MalformedData,
    NodeId::NULL,
    "the field description could not be decoded from its bytes",
);

/// A description claiming more nodes than any graph may hold.
const DESCRIPTION_OVER_BUDGET: FieldError = FieldError::at(
    FieldErrorCode::NodeLimitExceeded,
    NodeId::NULL,
    "the field description declares more nodes than the node budget allows",
);

/// One node of a [`FieldDescription`]: which node, what operator, what it
/// produces, and what it reads.
///
/// The id is carried rather than left implicit so a row read out of a list in
/// isolation still names itself. It is **not** written to the wire — the row's
/// position is its id, and encoding it twice would be two ways for one fact to
/// disagree with itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldNodeDescription {
    node: NodeId,
    op: FieldOp,
    ty: FieldType,
    inputs: Vec<NodeId>,
}

impl FieldNodeDescription {
    /// Which node this row describes.
    pub const fn node(&self) -> NodeId {
        self.node
    }

    /// The operator the node carries.
    pub const fn op(&self) -> FieldOp {
        self.op
    }

    /// The type the node's expression evaluates to.
    pub const fn ty(&self) -> FieldType {
        self.ty
    }

    /// The nodes this one reads, in slot order.
    pub fn inputs(&self) -> &[NodeId] {
        &self.inputs
    }

    /// This row as one line of an explanation: `n7: Mul(Scalar) <- n5, n6`.
    fn line(&self) -> String {
        let inputs: Vec<String> = self
            .inputs
            .iter()
            .map(|input| format!("n{}", input.raw()))
            .collect();
        let arrow = ["", " <- "][usize::from(!inputs.is_empty())];
        format!(
            "n{}: {:?}({:?}){arrow}{}",
            self.node.raw(),
            self.op,
            self.ty,
            inputs.join(", ")
        )
    }
}

/// A whole graph, described: its structural digest, its declared output, and one
/// row per node in id order.
///
/// Deterministic and byte-serializable. Two graphs that describe identically
/// **are** the same program up to parameter values, which is what makes a
/// description safe to store in a golden, post across a process boundary, or
/// diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDescription {
    digest: StableHash,
    output: NodeId,
    nodes: Vec<FieldNodeDescription>,
}

impl FieldDescription {
    /// The described graph's structural digest — the label, never the proof.
    pub const fn digest(&self) -> StableHash {
        self.digest
    }

    /// The node whose value is the described graph's result.
    pub const fn output(&self) -> NodeId {
        self.output
    }

    /// How many nodes the described graph has.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Every node's row, in id order.
    pub fn nodes(&self) -> &[FieldNodeDescription] {
        &self.nodes
    }

    /// The description's canonical bytes: the schema stamp, the digest, the
    /// output id, the node count, then per node its operator code, its type
    /// code, and its length-prefixed input ids.
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        FIELD_DESCRIPTION_SCHEMA_VERSION.write_to(&mut writer);
        writer.write_u64(self.digest.raw());
        writer.write_u32(self.output.raw());
        writer.write_u32(self.nodes.len() as u32);
        self.nodes.iter().for_each(|row| {
            writer.write_u16(row.op.code());
            writer.write_u16(row.ty.code());
            writer.write_u32(row.inputs.len() as u32);
            row.inputs
                .iter()
                .for_each(|input| writer.write_u32(input.raw()));
        });
        writer.into_bytes()
    }

    /// Decode a description produced by [`Self::encode`].
    ///
    /// Bounds-checked throughout and never panics: a buffer truncated at *any*
    /// prefix length fails cleanly, an incompatible schema major fails, an
    /// operator or type code that names nothing fails, and a claimed node count
    /// past the budget fails rather than being allocated.
    pub fn decode(bytes: &[u8]) -> FieldResult<FieldDescription> {
        let mut reader = BinaryReader::new(bytes);
        read_head(&mut reader).and_then(|(digest, output, count)| {
            (0..count)
                .try_fold(Vec::new(), |mut rows, index| {
                    read_row(&mut reader, NodeId::from_raw(index)).map(|row| {
                        rows.push(row);
                        rows
                    })
                })
                .map(|nodes| FieldDescription {
                    digest,
                    output,
                    nodes,
                })
        })
    }

    /// One deterministic line per node, in id order.
    pub fn explain(&self) -> FieldExplanation {
        FieldExplanation {
            lines: self.nodes.iter().map(FieldNodeDescription::line).collect(),
        }
    }
}

/// A graph explained: one line per node, in id order.
///
/// **Output only.** There is no reader for this text and there is deliberately
/// no textual authoring format in this layer — introducing one is a separate
/// decision with a wire-compatibility cost. The bytes of
/// [`FieldDescription::encode`] are the machine-readable form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldExplanation {
    lines: Vec<String>,
}

impl FieldExplanation {
    /// One line per node, in id order.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// Every line, newline-joined.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }
}

impl FieldGraph {
    /// The operator at `node`.
    pub fn op_at(&self, node: NodeId) -> FieldResult<FieldOp> {
        self.recipe()
            .node(node)
            .ok_or_else(|| OUTPUT_NODE_MISSING.about(node))
            .and_then(|found| {
                FieldOp::from_code(found.op()).ok_or_else(|| UNKNOWN_OPERATOR.about(node))
            })
    }

    /// The nodes `node` reads, in slot order.
    pub fn inputs_at(&self, node: NodeId) -> FieldResult<&[NodeId]> {
        self.recipe()
            .node(node)
            .ok_or_else(|| OUTPUT_NODE_MISSING.about(node))
            .map(|found| found.inputs())
    }

    /// Every node that reads `node`, in id order.
    ///
    /// A **forward scan**, not a stored reverse index — an index would be
    /// retained state needing invalidation on every rewrite, and every rewrite
    /// here returns a new graph so that nothing needs invalidating. `O(nodes ×
    /// inputs)` per call, bounded by [`MAX_NODES`]. **Authoring-time only; never
    /// call it from a frame path.**
    ///
    /// A node listed twice as an input (`Add(n1, n1)`) yields its consumer
    /// **once**: this answers *who depends on this*, not *how many edges arrive*.
    pub fn dependents_of(&self, node: NodeId) -> FieldResult<Vec<NodeId>> {
        self.recipe()
            .node(node)
            .ok_or_else(|| OUTPUT_NODE_MISSING.about(node))
            .map(|_present| {
                self.recipe()
                    .nodes()
                    .iter()
                    .enumerate()
                    .filter(|(_index, other)| other.inputs().contains(&node))
                    .map(|(index, _other)| NodeId::from_raw(index as u32))
                    .collect()
            })
    }

    /// The whole graph, described: one row per node with its operator, its
    /// derived type and its inputs, plus the declared output and the structural
    /// digest.
    ///
    /// Fails exactly where [`FieldGraph::validate`] fails, and for the same
    /// reason: **a graph that does not type has no types to report.** The
    /// operator decode runs first, so an unknown operator code is named as one
    /// rather than surfacing as whatever the type checker happened to notice.
    pub fn describe(&self) -> FieldResult<FieldDescription> {
        self.decode_ops()
            .and_then(|ops| self.node_types().map(|types| (ops, types)))
            .and_then(|(ops, types)| {
                self.check_node(self.output()).map(|()| (ops, types))
            })
            .map(|(ops, types)| FieldDescription {
                digest: self.digest(),
                output: self.output(),
                nodes: self
                    .recipe()
                    .nodes()
                    .iter()
                    .enumerate()
                    .map(|(index, node)| FieldNodeDescription {
                        node: NodeId::from_raw(index as u32),
                        op: ops[index],
                        ty: types[index],
                        inputs: node.inputs().to_vec(),
                    })
                    .collect(),
            })
    }

    /// The graph as deterministic text, one line per node in id order:
    /// `n7: Mul(Scalar) <- n5, n6`.
    ///
    /// An operator's name is its own `Debug` spelling, so there is exactly one
    /// name for an operator in this crate and a rename cannot leave two. Output
    /// only — see [`FieldExplanation`].
    pub fn explain(&self) -> FieldResult<FieldExplanation> {
        self.describe().map(|described| described.explain())
    }

    /// Every node's operator, in id order.
    fn decode_ops(&self) -> FieldResult<Vec<FieldOp>> {
        self.recipe()
            .nodes()
            .iter()
            .enumerate()
            .try_fold(Vec::new(), |mut ops, (index, node)| {
                FieldOp::from_code(node.op())
                    .ok_or_else(|| UNKNOWN_OPERATOR.about(NodeId::from_raw(index as u32)))
                    .map(|op| {
                        ops.push(op);
                        ops
                    })
            })
    }
}

/// Read the schema stamp, the digest, the output id and the node count.
fn read_head(reader: &mut BinaryReader<'_>) -> FieldResult<(StableHash, NodeId, u32)> {
    SchemaVersion::read_from(reader)
        .and_then(|stamp| {
            reader
                .read_u64()
                .and_then(|digest| reader.read_u32().map(|output| (stamp, digest, output)))
        })
        .and_then(|(stamp, digest, output)| {
            reader.read_u32().map(|count| (stamp, digest, output, count))
        })
        .map_err(|_| MALFORMED_DESCRIPTION)
        .and_then(|(stamp, digest, output, count)| {
            FIELD_DESCRIPTION_SCHEMA_VERSION
                .is_compatible_with(stamp)
                .then_some(())
                .ok_or(MALFORMED_DESCRIPTION)
                .map(|()| (digest, output, count))
        })
        .and_then(|(digest, output, count)| {
            (count as usize <= MAX_NODES)
                .then_some(())
                .ok_or(DESCRIPTION_OVER_BUDGET)
                .map(|()| (StableHash::from_raw(digest), NodeId::from_raw(output), count))
        })
}

/// Read one node row: an operator code, a type code, and length-prefixed inputs.
fn read_row(reader: &mut BinaryReader<'_>, node: NodeId) -> FieldResult<FieldNodeDescription> {
    reader
        .read_u16()
        .map_err(|_| MALFORMED_DESCRIPTION)
        .and_then(|code| FieldOp::from_code(code).ok_or_else(|| UNKNOWN_OPERATOR.about(node)))
        .and_then(|op| {
            reader
                .read_u16()
                .map_err(|_| MALFORMED_DESCRIPTION)
                .and_then(|code| {
                    FieldType::from_code(code).ok_or_else(|| UNKNOWN_TYPE.about(node))
                })
                .map(|ty| (op, ty))
        })
        .and_then(|(op, ty)| {
            read_inputs(reader).map(|inputs| FieldNodeDescription {
                node,
                op,
                ty,
                inputs,
            })
        })
}

/// Read one row's length-prefixed input ids.
fn read_inputs(reader: &mut BinaryReader<'_>) -> FieldResult<Vec<NodeId>> {
    reader
        .read_u32()
        .map_err(|_| MALFORMED_DESCRIPTION)
        .and_then(|count| {
            (0..count).try_fold(Vec::new(), |mut inputs, _| {
                reader
                    .read_u32()
                    .map_err(|_| MALFORMED_DESCRIPTION)
                    .map(|raw| {
                        inputs.push(NodeId::from_raw(raw));
                        inputs
                    })
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_recipe::{Param, RecipeGraph, RecipeId, Scalar};

    use crate::field_builder::FieldBuilder;
    use crate::field_params::FieldParams;
    use crate::field_value::FieldValue;
    use crate::ids::FieldId;

    /// `length(point + point)` with the sum shared by two consumers — enough to
    /// exercise a shared node, a no-input node and a two-input node.
    fn shared() -> FieldGraph {
        let (build, point) = FieldBuilder::new(FieldId::of_name("field/inspect"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        let (build, sum) = build.push(FieldOp::Add, Vec::new(), vec![point, point]);
        let (build, length) = build.push(FieldOp::Length, Vec::new(), vec![sum]);
        let (build, doubled) = build.push(FieldOp::Add, Vec::new(), vec![length, length]);
        build.build(doubled)
    }

    /// A one-node graph carrying a raw operator code, built by hand because the
    /// authoring surface will not mint an unknown operator.
    fn raw_op(code: u16) -> FieldGraph {
        let mut recipe = RecipeGraph::new(RecipeId::from_raw(1), 1);
        recipe.add(code, Vec::new(), Vec::new());
        FieldGraph::new(recipe, NodeId::from_raw(0), FieldParams::new())
    }

    #[test]
    fn every_accessor_reports_the_node_it_was_asked_about() {
        let field = shared();
        assert_eq!(field.node_count(), 4);
        assert_eq!(field.output(), NodeId::from_raw(3));
        assert_eq!(field.op_at(NodeId::from_raw(0)), Ok(FieldOp::Point));
        assert_eq!(field.op_at(NodeId::from_raw(2)), Ok(FieldOp::Length));
        assert_eq!(field.type_at(NodeId::from_raw(0)), Ok(FieldType::Vec3));
        assert_eq!(field.type_at(NodeId::from_raw(2)), Ok(FieldType::Scalar));
        assert_eq!(field.inputs_at(NodeId::from_raw(0)), Ok(&[][..]));
        assert_eq!(
            field.inputs_at(NodeId::from_raw(1)),
            Ok(&[NodeId::from_raw(0), NodeId::from_raw(0)][..])
        );
    }

    #[test]
    fn every_accessor_rejects_a_node_the_graph_does_not_have_and_names_it() {
        let field = shared();
        let missing = NodeId::from_raw(9);
        [
            field.op_at(missing).err(),
            field.type_at(missing).err(),
            field.inputs_at(missing).err(),
            field.dependents_of(missing).err(),
        ]
        .iter()
        .for_each(|error| {
            let error = error.expect("node 9 does not exist");
            assert_eq!(error.kind(), FieldErrorCode::OutputNodeMissing);
            assert_eq!(error.node(), missing);
        });
    }

    #[test]
    fn an_operator_code_that_names_nothing_is_reported_at_its_node() {
        let field = raw_op(999);
        let error = field
            .op_at(NodeId::from_raw(0))
            .expect_err("operator code 999 names no field operator");
        assert_eq!(error.kind(), FieldErrorCode::UnknownOperator);
        assert_eq!(error.node(), NodeId::from_raw(0));
        assert_eq!(
            field.describe().expect_err("nor can it be described").kind(),
            FieldErrorCode::UnknownOperator
        );
    }

    #[test]
    fn dependents_of_a_shared_node_names_every_consumer_once() {
        let field = shared();
        // The point feeds the sum twice; the sum is still named once.
        assert_eq!(
            field.dependents_of(NodeId::from_raw(0)),
            Ok(vec![NodeId::from_raw(1)])
        );
        assert_eq!(
            field.dependents_of(NodeId::from_raw(2)),
            Ok(vec![NodeId::from_raw(3)])
        );
        // The output is read by nobody.
        assert_eq!(field.dependents_of(NodeId::from_raw(3)), Ok(Vec::new()));
    }

    #[test]
    fn a_node_read_by_several_later_nodes_names_them_all_in_id_order() {
        let (build, point) = FieldBuilder::new(FieldId::of_name("field/inspect/fan"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        let (build, length) = build.push(FieldOp::Length, Vec::new(), vec![point]);
        let (build, absolute) = build.push(FieldOp::Abs, Vec::new(), vec![point]);
        let (build, normalized) = build.push(FieldOp::Normalize, Vec::new(), vec![point]);
        let (build, total) = build.push(FieldOp::Dot, Vec::new(), vec![absolute, normalized]);
        let field = build.push(FieldOp::Add, Vec::new(), vec![length, total]).0.build(total);
        assert_eq!(
            field.dependents_of(point),
            Ok(vec![
                NodeId::from_raw(1),
                NodeId::from_raw(2),
                NodeId::from_raw(3)
            ])
        );
    }

    #[test]
    fn a_description_names_every_node_its_operator_its_type_and_its_inputs() {
        let described = shared().describe().expect("the graph types");
        assert_eq!(described.node_count(), 4);
        assert_eq!(described.output(), NodeId::from_raw(3));
        assert_eq!(described.digest(), shared().digest());
        let ops: Vec<FieldOp> = described.nodes().iter().map(|row| row.op()).collect();
        assert_eq!(
            ops,
            vec![FieldOp::Point, FieldOp::Add, FieldOp::Length, FieldOp::Add]
        );
        let types: Vec<FieldType> = described.nodes().iter().map(|row| row.ty()).collect();
        assert_eq!(
            types,
            vec![
                FieldType::Vec3,
                FieldType::Vec3,
                FieldType::Scalar,
                FieldType::Scalar
            ]
        );
        assert_eq!(described.nodes()[0].node(), NodeId::from_raw(0));
        assert_eq!(described.nodes()[0].inputs(), &[]);
        assert_eq!(
            described.nodes()[2].inputs(),
            &[NodeId::from_raw(1)]
        );
    }

    #[test]
    fn a_graph_that_does_not_type_has_no_description_and_no_explanation() {
        let (build, point) = FieldBuilder::new(FieldId::of_name("field/inspect/bad"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        let (build, uv) = build.push(FieldOp::Uv, Vec::new(), Vec::new());
        let (build, bad) = build.push(FieldOp::Add, Vec::new(), vec![point, uv]);
        let field = build.build(bad);
        assert_eq!(
            field.describe().expect_err("Vec3 and Vec2 do not meet").kind(),
            FieldErrorCode::TypeMismatch
        );
        assert_eq!(
            field.explain().expect_err("nor can it be explained").kind(),
            FieldErrorCode::TypeMismatch
        );

        let orphaned = FieldBuilder::new(FieldId::from_raw(2), 1)
            .push(FieldOp::Point, Vec::new(), Vec::new())
            .0
            .build(NodeId::from_raw(9));
        assert_eq!(
            orphaned
                .describe()
                .expect_err("the declared output names no node")
                .kind(),
            FieldErrorCode::OutputNodeMissing
        );
    }

    #[test]
    fn an_explanation_is_one_line_per_node_in_id_order() {
        let explained = shared().explain().expect("the graph types");
        assert_eq!(
            explained.lines(),
            &[
                String::from("n0: Point(Vec3)"),
                String::from("n1: Add(Vec3) <- n0, n0"),
                String::from("n2: Length(Scalar) <- n1"),
                String::from("n3: Add(Scalar) <- n2, n2"),
            ]
        );
        assert_eq!(
            explained.text(),
            "n0: Point(Vec3)\nn1: Add(Vec3) <- n0, n0\nn2: Length(Scalar) <- n1\nn3: Add(Scalar) <- n2, n2"
        );
        // Deterministic: the same graph always explains the same way.
        assert_eq!(shared().explain(), Ok(explained));
    }

    #[test]
    fn a_description_round_trips_through_its_bytes() {
        let described = shared().describe().expect("the graph types");
        assert_eq!(
            FieldDescription::decode(&described.encode()),
            Ok(described.clone())
        );
        // The same description always produces the same bytes.
        assert_eq!(described.encode(), described.encode());
        assert_eq!(
            FIELD_DESCRIPTION_SCHEMA_VERSION,
            SchemaVersion::new(1, 0)
        );
    }

    #[test]
    fn a_description_carrying_a_parameterised_node_round_trips_too() {
        let (build, slot) = FieldBuilder::new(FieldId::of_name("field/inspect/param"), 1)
            .declare("tint", FieldValue::scalar(Scalar::new(0.5)));
        let (build, knob) = build.push_param(slot, FieldType::Scalar);
        let (build, lane) = build.push(FieldOp::Compose, vec![Param::int(2)], vec![knob, knob]);
        let described = build.build(lane).describe().expect("the graph types");
        assert_eq!(described.nodes()[1].ty(), FieldType::Vec2);
        assert_eq!(
            FieldDescription::decode(&described.encode()),
            Ok(described)
        );
    }

    #[test]
    fn every_truncation_of_a_description_fails_cleanly() {
        let bytes = shared().describe().expect("types").encode();
        (0..bytes.len()).for_each(|n| {
            assert!(
                FieldDescription::decode(&bytes[..n]).is_err(),
                "prefix of length {n} must not decode"
            );
        });
    }

    #[test]
    fn hostile_description_bytes_are_rejected_rather_than_believed() {
        let good = shared().describe().expect("types").encode();

        // An incompatible schema major.
        let mut wrong_schema = good.clone();
        wrong_schema[0] = 9;
        assert_eq!(
            FieldDescription::decode(&wrong_schema)
                .expect_err("schema major 9 is not this format")
                .kind(),
            FieldErrorCode::MalformedData
        );

        // An operator code that names nothing, at the first row.
        let head = 4 + 8 + 4 + 4;
        let mut bad_op = good.clone();
        bad_op[head] = 0xFF;
        let error = FieldDescription::decode(&bad_op)
            .expect_err("operator code 255 names no field operator");
        assert_eq!(error.kind(), FieldErrorCode::UnknownOperator);
        assert_eq!(error.node(), NodeId::from_raw(0));

        // A type code that names nothing, at the first row.
        let mut bad_type = good.clone();
        bad_type[head + 2] = 0xFF;
        let error = FieldDescription::decode(&bad_type)
            .expect_err("type code 255 names no field type");
        assert_eq!(error.kind(), FieldErrorCode::UnknownType);
        assert_eq!(error.node(), NodeId::from_raw(0));

        // A node count past the budget is refused rather than allocated.
        let mut huge = good;
        huge[16..20].copy_from_slice(&((MAX_NODES + 1) as u32).to_le_bytes());
        assert_eq!(
            FieldDescription::decode(&huge)
                .expect_err("257 nodes exceed the budget")
                .kind(),
            FieldErrorCode::NodeLimitExceeded
        );
    }
}
