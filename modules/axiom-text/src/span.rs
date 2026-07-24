//! A rich-text span: a run of text plus the style overrides that apply to it.

use crate::style::StyleOverride;

/// One contiguous run of text carrying its own sparse style override. Rich text
/// is a `Vec<TextSpan>` — the *canonical* representation. Plain text is simply a
/// single span with an empty override, so there is one content model, and any
/// markup convenience parser is a thin wrapper that produces spans (never a
/// stored markup string).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextSpan {
    /// The span's text (UTF-8).
    pub text: String,
    /// Style overrides applied on top of the text-level style for this run.
    pub style: StyleOverride,
}

impl TextSpan {
    /// A plain span with no style override.
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: StyleOverride::default(),
        }
    }

    /// A span with an explicit style override.
    pub fn styled(text: impl Into<String>, style: StyleOverride) -> Self {
        Self {
            text: text.into(),
            style,
        }
    }

    /// The number of Unicode scalar values (`char`s) in the span.
    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;

    #[test]
    fn plain_and_styled_construct() {
        let p = TextSpan::plain("hi");
        assert_eq!(p.text, "hi");
        assert_eq!(p.style, StyleOverride::default());
        assert_eq!(p.char_count(), 2);

        let s = TextSpan::styled(
            "x",
            StyleOverride {
                color: Some(Rgba::BLACK),
                ..Default::default()
            },
        );
        assert_eq!(s.style.color, Some(Rgba::BLACK));
    }

    #[test]
    fn char_count_counts_scalars_not_bytes() {
        assert_eq!(TextSpan::plain("café").char_count(), 4);
    }
}
