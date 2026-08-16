//! Stable, deterministic recipe errors that name the node they concern.

use crate::ids::NodeId;

/// Why a recipe is not a valid, evaluable graph. Deterministic, fieldless, and
/// `Copy`: a recipe is a flat container, so the only failures are an
/// out-of-budget graph, an input link that is not strictly earlier (which would
/// make the graph cyclic), or a byte stream that will not decode.
///
/// This is the *kind* only. The node a failure concerns is a field on
/// [`RecipeError`], never an enum payload — a data-carrying variant would force
/// a `match` on read and violate the Branchless Law.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeErrorCode {
    /// The graph has more nodes than the recipe budget allows.
    NodeLimitExceeded,
    /// A node has an input that does not reference a strictly-earlier node (an
    /// out-of-range, self, or forward reference) — the check that keeps the graph
    /// acyclic and evaluable in id order.
    CyclicInput,
    /// A serialized recipe could not be decoded from its bytes.
    MalformedData,
}

impl RecipeErrorCode {
    /// A stable numeric discriminant for asserting on *which* failure occurred
    /// without depending on the variant layout. Table-indexed by the fieldless
    /// discriminant, so it is branch-free.
    pub const fn code(self) -> u16 {
        [1_u16, 2, 3][self as usize]
    }
}

/// One recipe failure: what went wrong, **which node** it went wrong at, and a
/// human-readable explanation.
///
/// Following the kernel's rule, the **identity** of an error is its machine data
/// — the `(code, node)` pair. The `&'static str` message is for humans and never
/// participates in equality, so a test asserts on what the failure *is* rather
/// than on how it is worded. A whole-graph failure that concerns no single node
/// reports [`NodeId::NULL`].
#[derive(Debug, Clone, Copy)]
pub struct RecipeError {
    code: RecipeErrorCode,
    node: NodeId,
    message: &'static str,
}

impl RecipeError {
    /// A failure concerning one particular node. Pass [`NodeId::NULL`] for a
    /// whole-graph property that no single node owns.
    pub const fn at(code: RecipeErrorCode, node: NodeId, message: &'static str) -> Self {
        RecipeError {
            code,
            node,
            message,
        }
    }

    /// Locate an existing failure at a node.
    ///
    /// A rule stated once as a constant does not know which node broke it; the
    /// scan that walks the graph does, and stamps it on the way out so the
    /// diagnostic names the node instead of just the rule.
    pub const fn about(self, node: NodeId) -> Self {
        RecipeError { node, ..self }
    }

    /// Which of the three failures this is.
    pub const fn kind(self) -> RecipeErrorCode {
        self.code
    }

    /// The stable numeric discriminant — `1`, `2`, or `3`. Existing callers and
    /// any serialized verdict depend on these values.
    pub const fn code(self) -> u16 {
        self.code.code()
    }

    /// The node this failure concerns, or [`NodeId::NULL`].
    pub const fn node(self) -> NodeId {
        self.node
    }

    /// The human-readable explanation. Never part of identity.
    pub const fn message(self) -> &'static str {
        self.message
    }
}

/// Identity is `(code, node)` — never the message. `&` rather than `&&` because
/// the Branchless Law forbids the short-circuiting form and both sides are pure
/// comparisons that are always safe to evaluate.
impl PartialEq for RecipeError {
    fn eq(&self, other: &Self) -> bool {
        (self.code == other.code) & (self.node == other.node)
    }
}

impl Eq for RecipeError {}

/// The result of a fallible recipe operation.
pub type RecipeResult<T> = Result<T, RecipeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_and_distinct() {
        assert_eq!(RecipeErrorCode::NodeLimitExceeded.code(), 1);
        assert_eq!(RecipeErrorCode::CyclicInput.code(), 2);
        assert_eq!(RecipeErrorCode::MalformedData.code(), 3);
    }

    #[test]
    fn an_error_carries_its_code_node_and_message() {
        let error = RecipeError::at(
            RecipeErrorCode::CyclicInput,
            NodeId::from_raw(4),
            "input is not strictly earlier",
        );
        assert_eq!(error.kind(), RecipeErrorCode::CyclicInput);
        assert_eq!(error.code(), 2);
        assert_eq!(error.node(), NodeId::from_raw(4));
        assert_eq!(error.message(), "input is not strictly earlier");
    }

    #[test]
    fn a_whole_graph_failure_names_no_node() {
        let error = RecipeError::at(
            RecipeErrorCode::NodeLimitExceeded,
            NodeId::NULL,
            "too many nodes",
        );
        assert_eq!(error.node(), NodeId::NULL);
        assert_eq!(error.code(), 1);
    }

    #[test]
    fn a_failure_can_be_located_at_a_node_after_the_fact() {
        let rule = RecipeError::at(RecipeErrorCode::CyclicInput, NodeId::NULL, "cyclic");
        let located = rule.about(NodeId::from_raw(7));
        assert_eq!(located.node(), NodeId::from_raw(7));
        assert_eq!(located.kind(), RecipeErrorCode::CyclicInput);
        assert_eq!(located.message(), "cyclic");
    }

    #[test]
    fn the_message_is_not_part_of_identity() {
        let one = RecipeError::at(RecipeErrorCode::MalformedData, NodeId::NULL, "one wording");
        let other = RecipeError::at(
            RecipeErrorCode::MalformedData,
            NodeId::NULL,
            "a completely different wording",
        );
        assert_eq!(one, other);
    }

    #[test]
    fn both_the_code_and_the_node_are_part_of_identity() {
        let base = RecipeError::at(RecipeErrorCode::CyclicInput, NodeId::from_raw(1), "m");
        assert_ne!(
            base,
            RecipeError::at(RecipeErrorCode::MalformedData, NodeId::from_raw(1), "m")
        );
        assert_ne!(base, base.about(NodeId::from_raw(2)));
    }
}
