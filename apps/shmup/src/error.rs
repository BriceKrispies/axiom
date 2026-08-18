//! The one error type the ported core reports with.
//!
//! Not a file in the source: JavaScript has exactly one `Error` class and every
//! failure in `core/` is `throw new Error("…")`. The faithful Rust shape is a
//! single error type carrying the same message text, returned rather than
//! thrown — so a caller that ignores a failure cannot compile, and the messages
//! stay diffable against the source (`duplicate subsystem id "x"`,
//! `dependency cycle at "x" (via y)`, and the rest are verbatim).

use std::fmt;

/// A failure with a message, mirroring the source's `new Error(message)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreError {
    message: String,
}

impl CoreError {
    pub fn new(message: impl Into<String>) -> Self {
        CoreError {
            message: message.into(),
        }
    }

    /// The message text, byte-for-byte what the source throws.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CoreError {}
