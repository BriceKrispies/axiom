//! Symbol-shaped queries: where is this defined, and who references it.
//!
//! These are regexes, not a compiler front end. They are deliberately cheap so
//! that `ax def` stays as fast as `ax q`; the payoff is that an agent asking a
//! *structural* question ("where does this live?") records a structural
//! question in the ledger, instead of an opaque grep.

/// A pattern matching the places `sym` is *defined*, across Rust and TypeScript.
pub fn definition_pattern(sym: &str) -> String {
    let s = regex::escape(sym);
    [
        // Rust items.
        format!(
            r"(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?(?:fn|struct|enum|trait|union|type|const|static|mod)\s+{s}\b"
        ),
        format!(r"macro_rules!\s+{s}\b"),
        // Inherent and trait impls.
        format!(r"impl(?:<[^>]*>)?\s+(?:[^{{]+\s+for\s+)?{s}\b"),
        // TypeScript / JavaScript declarations.
        format!(
            r"(?:export\s+)?(?:default\s+)?(?:async\s+)?(?:abstract\s+)?(?:function|class|interface|enum|namespace|type|const|let|var)\s+{s}\b"
        ),
    ]
    .join("|")
}

/// A pattern matching every mention of `sym` as a whole word.
pub fn reference_pattern(sym: &str) -> String {
    format!(r"\b{}\b", regex::escape(sym))
}
