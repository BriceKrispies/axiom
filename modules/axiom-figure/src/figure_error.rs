//! The figure module's deterministic error type.

/// Why a figure operation failed. Small and `Copy`: a figure is a flat data
/// structure, so the only failures are a malformed byte stream, an illegal
/// parent link, or a posing call whose world-transform count does not match the
/// figure's part count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FigureError {
    /// A part names a parent that is not strictly earlier in the part list (out
    /// of range, forward, or self reference) — the rule that keeps a figure a
    /// single-pass, acyclic hierarchy.
    BadParent,
    /// A serialized figure could not be decoded from its bytes.
    MalformedData,
    /// Posing was given a world-transform slice whose length differs from the
    /// figure's part count.
    TransformCountMismatch,
}

/// The result of a fallible figure operation.
pub type FigureResult<T> = Result<T, FigureError>;
