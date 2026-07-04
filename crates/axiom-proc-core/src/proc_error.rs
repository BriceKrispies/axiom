//! Stable, deterministic graph-execution errors.

/// Why executing a recipe graph failed. Fieldless and `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcError {
    /// The recipe is not a valid graph (out of budget, or a cyclic/forward
    /// input link).
    InvalidRecipe,
    /// An operator returned no output for its inputs and parameters (e.g. an
    /// unknown operator code, or the wrong number of inputs).
    OpFailed,
    /// The recipe has no nodes, so there is no result to return.
    EmptyRecipe,
}

impl ProcError {
    /// A stable numeric discriminant, table-indexed by the fieldless variant.
    pub const fn code(self) -> u16 {
        [1_u16, 2, 3][self as usize]
    }
}

/// The result of a fallible graph execution.
pub type ProcResult<T> = Result<T, ProcError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_and_distinct() {
        assert_eq!(ProcError::InvalidRecipe.code(), 1);
        assert_eq!(ProcError::OpFailed.code(), 2);
        assert_eq!(ProcError::EmptyRecipe.code(), 3);
    }
}
