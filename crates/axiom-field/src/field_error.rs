//! Stable, deterministic field failures that name the node they concern.

use axiom_recipe::{NodeId, RecipeError};

/// Why a byte stream is not a valid field graph.
///
/// Deterministic, fieldless and `Copy`. The first three are the container's own
/// failures, inherited from [`axiom_recipe::RecipeErrorCode`] one-for-one so a
/// caller never has to unwrap two error vocabularies; the last two are the
/// field-level structure `recipe` cannot know about.
///
/// This is the *kind* only. The node a failure concerns is a field on
/// [`FieldError`], never an enum payload — a data-carrying variant would force a
/// `match` on read and violate the Branchless Law.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldErrorCode {
    /// The graph has more nodes than the recipe budget allows.
    NodeLimitExceeded,
    /// A node input does not reference a strictly-earlier node.
    CyclicInput,
    /// The bytes could not be decoded.
    MalformedData,
    /// A type code in the parameter table names no [`crate::FieldType`].
    UnknownType,
    /// The declared output node is not a node of the graph.
    OutputOutOfRange,
}

impl FieldErrorCode {
    /// A stable numeric discriminant for asserting on *which* failure occurred
    /// without depending on the variant layout. Table-indexed by the fieldless
    /// discriminant, so it is branch-free.
    pub const fn code(self) -> u16 {
        [1_u16, 2, 3, 4, 5][self as usize]
    }
}

/// The three recipe failures, indexed by `RecipeError::code() - 1`. Recipe's
/// codes are `1..=3` and documented as stable, so this is a total mapping.
const FROM_RECIPE: [FieldErrorCode; 3] = [
    FieldErrorCode::NodeLimitExceeded,
    FieldErrorCode::CyclicInput,
    FieldErrorCode::MalformedData,
];

/// One field failure: what went wrong, **which node** it went wrong at, and a
/// human-readable explanation.
///
/// Identity is the machine data — the `(code, node)` pair. The `&'static str`
/// message is for humans and never participates in equality, so a test asserts
/// on what the failure *is* rather than on how it is worded. A whole-graph
/// failure that concerns no single node reports [`NodeId::NULL`].
#[derive(Debug, Clone, Copy)]
pub struct FieldError {
    code: FieldErrorCode,
    node: NodeId,
    message: &'static str,
}

impl FieldError {
    /// A failure concerning one particular node. Pass [`NodeId::NULL`] for a
    /// whole-graph property that no single node owns.
    pub const fn at(code: FieldErrorCode, node: NodeId, message: &'static str) -> Self {
        FieldError {
            code,
            node,
            message,
        }
    }

    /// Locate an existing failure at a node — a rule stated once as a constant
    /// does not know which node broke it; the scan that walks the graph does.
    pub const fn about(self, node: NodeId) -> Self {
        FieldError { node, ..self }
    }

    /// Wrap a container failure, preserving the node it named and its wording.
    /// The two vocabularies stay one-for-one, so nothing is lost in the lift.
    pub fn from_recipe(error: RecipeError) -> Self {
        FieldError {
            code: FROM_RECIPE[(error.code() - 1) as usize],
            node: error.node(),
            message: error.message(),
        }
    }

    /// Which failure this is.
    pub const fn kind(self) -> FieldErrorCode {
        self.code
    }

    /// The stable numeric discriminant.
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
impl PartialEq for FieldError {
    fn eq(&self, other: &Self) -> bool {
        (self.code == other.code) & (self.node == other.node)
    }
}

impl Eq for FieldError {}

/// The result of a fallible field operation.
pub type FieldResult<T> = Result<T, FieldError>;

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_recipe::RecipeErrorCode;

    #[test]
    fn codes_are_stable_and_distinct() {
        assert_eq!(FieldErrorCode::NodeLimitExceeded.code(), 1);
        assert_eq!(FieldErrorCode::CyclicInput.code(), 2);
        assert_eq!(FieldErrorCode::MalformedData.code(), 3);
        assert_eq!(FieldErrorCode::UnknownType.code(), 4);
        assert_eq!(FieldErrorCode::OutputOutOfRange.code(), 5);
    }

    #[test]
    fn an_error_carries_its_code_node_and_message() {
        let error = FieldError::at(
            FieldErrorCode::UnknownType,
            NodeId::from_raw(4),
            "unknown type code",
        );
        assert_eq!(error.kind(), FieldErrorCode::UnknownType);
        assert_eq!(error.code(), 4);
        assert_eq!(error.node(), NodeId::from_raw(4));
        assert_eq!(error.message(), "unknown type code");
    }

    #[test]
    fn a_failure_can_be_located_at_a_node_after_the_fact() {
        let rule = FieldError::at(
            FieldErrorCode::OutputOutOfRange,
            NodeId::NULL,
            "no such output",
        );
        assert_eq!(rule.node(), NodeId::NULL);
        let located = rule.about(NodeId::from_raw(7));
        assert_eq!(located.node(), NodeId::from_raw(7));
        assert_eq!(located.kind(), FieldErrorCode::OutputOutOfRange);
        assert_eq!(located.message(), "no such output");
    }

    #[test]
    fn every_recipe_failure_lifts_one_for_one() {
        let lifted = [
            (RecipeErrorCode::NodeLimitExceeded, FieldErrorCode::NodeLimitExceeded),
            (RecipeErrorCode::CyclicInput, FieldErrorCode::CyclicInput),
            (RecipeErrorCode::MalformedData, FieldErrorCode::MalformedData),
        ];
        lifted.iter().for_each(|(recipe_code, field_code)| {
            let recipe = RecipeError::at(*recipe_code, NodeId::from_raw(3), "container said so");
            let field = FieldError::from_recipe(recipe);
            assert_eq!(field.kind(), *field_code);
            assert_eq!(field.node(), NodeId::from_raw(3));
            assert_eq!(field.message(), "container said so");
        });
    }

    #[test]
    fn the_message_is_not_part_of_identity_but_the_code_and_node_are() {
        let one = FieldError::at(FieldErrorCode::MalformedData, NodeId::NULL, "one wording");
        let other = FieldError::at(FieldErrorCode::MalformedData, NodeId::NULL, "another");
        assert_eq!(one, other);
        assert_ne!(
            one,
            FieldError::at(FieldErrorCode::UnknownType, NodeId::NULL, "one wording")
        );
        assert_ne!(one, one.about(NodeId::from_raw(2)));
    }
}
