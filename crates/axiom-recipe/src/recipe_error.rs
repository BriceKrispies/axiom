//! Stable, deterministic recipe errors.

/// Why a recipe is not a valid, evaluable graph. Deterministic, fieldless, and
/// `Copy`: a recipe is a flat container, so the only failures are an
/// out-of-budget graph, an input link that is not strictly earlier (which would
/// make the graph cyclic), or a byte stream that will not decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecipeError {
    /// The graph has more nodes than the recipe budget allows.
    NodeLimitExceeded,
    /// A node has an input that does not reference a strictly-earlier node (an
    /// out-of-range, self, or forward reference) — the check that keeps the graph
    /// acyclic and evaluable in id order.
    CyclicInput,
    /// A serialized recipe could not be decoded from its bytes.
    MalformedData,
}

impl RecipeError {
    /// A stable numeric discriminant for asserting on *which* failure occurred
    /// without depending on the variant layout. Table-indexed by the fieldless
    /// discriminant, so it is branch-free.
    pub const fn code(self) -> u16 {
        [1_u16, 2, 3][self as usize]
    }
}

/// The result of a fallible recipe operation.
pub type RecipeResult<T> = Result<T, RecipeError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_and_distinct() {
        assert_eq!(RecipeError::NodeLimitExceeded.code(), 1);
        assert_eq!(RecipeError::CyclicInput.code(), 2);
        assert_eq!(RecipeError::MalformedData.code(), 3);
    }
}
