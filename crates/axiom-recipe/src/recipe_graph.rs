//! The recipe graph: a versioned, append-only DAG of operator nodes.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult, SchemaVersion, StableHash};

use crate::ids::{NodeId, RecipeId};
use crate::node::Node;
use crate::recipe_error::{RecipeError, RecipeResult};
use crate::value::Param;

/// The wire-format version stamped into every serialized recipe. Bumping it
/// deliberately changes the bytes (and therefore every digest / golden), so a
/// format change can never be silent.
const SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

/// The maximum number of nodes a recipe may carry. A recipe is meant to be a
/// tiny "how to make it" description; anything larger is rejected as invalid
/// input, keeping evaluation bounded.
pub const MAX_NODES: usize = 256;

/// A procedural recipe: a stable [`RecipeId`], a content `version`, and an
/// append-only list of operator [`Node`]s. A node's inputs reference only
/// strictly-earlier nodes, so the graph is a DAG evaluable in id order. The
/// operator codes are opaque here — a higher generation layer assigns meaning.
#[derive(Debug, Clone, PartialEq)]
pub struct RecipeGraph {
    id: RecipeId,
    version: u32,
    nodes: Vec<Node>,
}

impl RecipeGraph {
    /// A new, empty recipe with the given id and content version.
    pub fn new(id: RecipeId, version: u32) -> Self {
        Self {
            id,
            version,
            nodes: Vec::new(),
        }
    }

    /// Append an operator node and return its [`NodeId`] (its index). Does not
    /// validate wiring — call [`Self::validate`] before evaluating.
    pub fn add(&mut self, op: u16, params: Vec<Param>, inputs: Vec<NodeId>) -> NodeId {
        let id = NodeId::from_raw(self.nodes.len() as u32);
        self.nodes.push(Node::new(op, params, inputs));
        id
    }

    /// The recipe's stable id.
    pub fn id(&self) -> RecipeId {
        self.id
    }

    /// The recipe's content version.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// The number of nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// The nodes, in id order.
    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// The node at `id`, or `None` if out of range.
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.raw() as usize)
    }

    /// Validate the graph: within the node budget, and every node's inputs
    /// reference strictly-earlier nodes (acyclic, in-range). Returns the first
    /// violation.
    pub fn validate(&self) -> RecipeResult<()> {
        let within_budget = (self.nodes.len() <= MAX_NODES)
            .then_some(())
            .ok_or(RecipeError::NodeLimitExceeded);
        let acyclic = self.nodes.iter().enumerate().try_for_each(|(index, node)| {
            node.inputs()
                .iter()
                .all(|input| (input.raw() as usize) < index)
                .then_some(())
                .ok_or(RecipeError::CyclicInput)
        });
        within_budget.and(acyclic)
    }

    /// Append the recipe's canonical bytes: a [`SchemaVersion`] stamp, the id
    /// (`u64`), the version (`u32`), a `u32` node count, then each node.
    pub fn write_to(&self, writer: &mut BinaryWriter) {
        SCHEMA.write_to(writer);
        writer.write_u64(self.id.raw());
        writer.write_u32(self.version);
        writer.write_u32(self.nodes.len() as u32);
        self.nodes.iter().for_each(|node| node.write_to(writer));
    }

    /// Read a recipe written by [`Self::write_to`]. Structural decode only — the
    /// caller validates.
    pub fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<RecipeGraph> {
        SchemaVersion::read_from(reader).and_then(|_schema| {
            reader.read_u64().and_then(|id| {
                reader.read_u32().and_then(|version| {
                    read_nodes(reader).map(|nodes| RecipeGraph {
                        id: RecipeId::from_raw(id),
                        version,
                        nodes,
                    })
                })
            })
        })
    }

    /// The recipe as a portable byte buffer.
    pub fn serialize(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        self.write_to(&mut writer);
        writer.into_bytes()
    }

    /// Decode and validate a recipe produced by [`Self::serialize`]. Fails with
    /// `MalformedData` for undecodable bytes, or the validation error for a
    /// decodable-but-illegal graph.
    pub fn deserialize(bytes: &[u8]) -> RecipeResult<RecipeGraph> {
        RecipeGraph::read_from(&mut BinaryReader::new(bytes))
            .map_err(|_| RecipeError::MalformedData)
            .and_then(|graph| graph.validate().map(|()| graph))
    }

    /// The recipe's stable content digest (a golden index over its canonical
    /// bytes — the bytes are the determinism proof, the digest is the label).
    pub fn digest(&self) -> StableHash {
        StableHash::of_bytes(&self.serialize())
    }
}

/// Read a `u32` node count then that many nodes.
fn read_nodes(reader: &mut BinaryReader<'_>) -> KernelResult<Vec<Node>> {
    reader.read_u32().and_then(|count| {
        (0..count).try_fold(Vec::new(), |mut acc, _| {
            Node::read_from(reader).map(|node| {
                acc.push(node);
                acc
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Scalar;

    fn chain() -> RecipeGraph {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        let a = g.add(0, vec![Param::scalar(Scalar::new(1.0))], vec![]);
        let b = g.add(1, vec![Param::int(2)], vec![a]);
        g.add(2, vec![], vec![a, b]);
        g
    }

    #[test]
    fn builder_assigns_dense_ids_and_reports_parts() {
        let g = chain();
        assert_eq!(g.id(), RecipeId::from_raw(1));
        assert_eq!(g.version(), 1);
        assert_eq!(g.node_count(), 3);
        assert_eq!(g.node(NodeId::from_raw(1)).unwrap().op(), 1);
        assert_eq!(g.node(NodeId::from_raw(9)), None);
        assert_eq!(g.nodes().len(), 3);
    }

    #[test]
    fn valid_chain_validates() {
        assert_eq!(chain().validate(), Ok(()));
    }

    #[test]
    fn forward_reference_is_cyclic() {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        g.add(0, vec![], vec![NodeId::from_raw(1)]); // references a later node
        g.add(1, vec![], vec![]);
        assert_eq!(g.validate(), Err(RecipeError::CyclicInput));
    }

    #[test]
    fn self_reference_is_cyclic() {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        g.add(0, vec![], vec![NodeId::from_raw(0)]); // references itself
        assert_eq!(g.validate(), Err(RecipeError::CyclicInput));
    }

    #[test]
    fn over_budget_is_rejected() {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        (0..=MAX_NODES).for_each(|_| {
            g.add(0, vec![], vec![]);
        });
        assert_eq!(g.validate(), Err(RecipeError::NodeLimitExceeded));
    }

    #[test]
    fn serialize_deserialize_round_trips_and_digests_stably() {
        let g = chain();
        let bytes = g.serialize();
        assert_eq!(RecipeGraph::deserialize(&bytes).unwrap(), g);
        assert_eq!(g.digest(), chain().digest());
    }

    #[test]
    fn deserialize_rejects_garbage_and_illegal_graphs() {
        assert_eq!(
            RecipeGraph::deserialize(&[0xFF]),
            Err(RecipeError::MalformedData)
        );
        // A structurally-decodable but cyclic graph is rejected on validate.
        let mut bad = RecipeGraph::new(RecipeId::from_raw(1), 1);
        bad.add(0, vec![], vec![NodeId::from_raw(5)]);
        let bytes = bad.serialize();
        assert_eq!(
            RecipeGraph::deserialize(&bytes),
            Err(RecipeError::CyclicInput)
        );
    }
}
