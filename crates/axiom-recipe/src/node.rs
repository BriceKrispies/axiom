//! One operator node in a recipe: an opaque operator code, its parameter words,
//! and its input links.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

use crate::ids::NodeId;
use crate::value::Param;

/// A single node of a [`crate::RecipeGraph`]. The `op` code is **opaque** to this
/// layer — a higher generation layer assigns and interprets it. `params` are the
/// raw parameter words the operator reads through typed views; `inputs` link to
/// the nodes whose outputs this operator consumes (each strictly earlier in the
/// graph).
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    op: u16,
    params: Vec<Param>,
    inputs: Vec<NodeId>,
}

impl Node {
    /// Build a node from its operator code, parameter words, and input links.
    pub fn new(op: u16, params: Vec<Param>, inputs: Vec<NodeId>) -> Self {
        Self { op, params, inputs }
    }

    /// The opaque operator code.
    pub fn op(&self) -> u16 {
        self.op
    }

    /// The parameter words, in slot order.
    pub fn params(&self) -> &[Param] {
        &self.params
    }

    /// The input links, in slot order.
    pub fn inputs(&self) -> &[NodeId] {
        &self.inputs
    }

    /// Append the node's bytes: `op` (`u16`), a `u32` param count then each param
    /// word, a `u32` input count then each input id.
    pub(crate) fn write_to(&self, writer: &mut BinaryWriter) {
        writer.write_u16(self.op);
        writer.write_u32(self.params.len() as u32);
        self.params.iter().for_each(|p| writer.write_u32(p.bits()));
        writer.write_u32(self.inputs.len() as u32);
        self.inputs.iter().for_each(|i| writer.write_u32(i.raw()));
    }

    /// Read a node written by [`Node::write_to`].
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Node> {
        reader.read_u16().and_then(|op| {
            read_words(reader).and_then(|params| {
                read_ids(reader).map(|inputs| Node {
                    op,
                    params: params.into_iter().map(Param::from_bits).collect(),
                    inputs: inputs.into_iter().map(NodeId::from_raw).collect(),
                })
            })
        })
    }
}

/// Read a `u32` length prefix then that many `u32` words.
fn read_words(reader: &mut BinaryReader<'_>) -> KernelResult<Vec<u32>> {
    reader.read_u32().and_then(|count| {
        (0..count).try_fold(Vec::new(), |mut acc, _| {
            reader.read_u32().map(|w| {
                acc.push(w);
                acc
            })
        })
    })
}

/// Read a `u32` length prefix then that many `u32` ids (same wire shape as
/// [`read_words`], kept separate for readability at the call site).
fn read_ids(reader: &mut BinaryReader<'_>) -> KernelResult<Vec<u32>> {
    read_words(reader)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Scalar;

    fn sample() -> Node {
        Node::new(
            5,
            vec![Param::int(2), Param::scalar(Scalar::new(0.5))],
            vec![NodeId::from_raw(0), NodeId::from_raw(1)],
        )
    }

    #[test]
    fn node_exposes_its_parts() {
        let n = sample();
        assert_eq!(n.op(), 5);
        assert_eq!(n.params().len(), 2);
        assert_eq!(n.inputs(), &[NodeId::from_raw(0), NodeId::from_raw(1)]);
    }

    #[test]
    fn node_round_trips_through_bytes() {
        let n = sample();
        let mut w = BinaryWriter::new();
        n.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(Node::read_from(&mut BinaryReader::new(&bytes)).unwrap(), n);
    }

    #[test]
    fn truncated_node_bytes_fail() {
        let n = sample();
        let mut w = BinaryWriter::new();
        n.write_to(&mut w);
        let bytes = w.into_bytes();
        assert!(Node::read_from(&mut BinaryReader::new(&bytes[..3])).is_err());
    }
}
