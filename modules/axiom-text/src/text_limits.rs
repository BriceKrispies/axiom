//! Configurable safety limits: every unbounded quantity has a checked ceiling.

/// The caps the [`crate::TextApi`] enforces so no input can drive unbounded
/// allocation. Every breach returns [`crate::TextError::CapacityExceeded`] (or
/// [`crate::TextError::AtlasPackingOverflow`] for atlas pages) rather than
/// panicking or allocating without bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextLimits {
    /// Maximum number of live text objects.
    pub max_text_objects: u32,
    /// Maximum `char`s across all spans of one text object.
    pub max_chars_per_text: u32,
    /// Maximum spans in one text object.
    pub max_spans: u32,
    /// Maximum laid-out lines in one text object.
    pub max_lines: u32,
    /// Maximum glyph instances in one snapshot/batch.
    pub max_glyphs: u32,
    /// Maximum registered fonts.
    pub max_fonts: u32,
    /// Maximum atlas pages a single font may declare.
    pub max_atlas_pages: u32,
}

impl TextLimits {
    /// Generous defaults suited to a game HUD/UI: thousands of objects, long
    /// strings, and plenty of glyphs, while still bounded.
    pub const DEFAULT: TextLimits = TextLimits {
        max_text_objects: 8192,
        max_chars_per_text: 16384,
        max_spans: 1024,
        max_lines: 4096,
        max_glyphs: 262144,
        max_fonts: 256,
        max_atlas_pages: 64,
    };
}

impl Default for TextLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limits_are_bounded_and_nonzero() {
        let l = TextLimits::default();
        assert_eq!(l, TextLimits::DEFAULT);
        assert!(l.max_text_objects > 0);
        assert!(l.max_glyphs > 0);
        assert!(l.max_fonts > 0);
    }
}
