//! Retained per-text state with fine-grained dirty tracking.
//!
//! Text is designed for frequently-changing game values, so each object keeps its
//! resolved layout cached and invalidates only the minimum on a change. The
//! invalidation rules are encoded here and directly tested: changing content,
//! fonts, or a layout-affecting style bit dirties layout; changing colour,
//! opacity, visibility, placement, or effects does not.

use crate::effect::TextEffect;
use crate::font_registry::FontHandle;
use crate::laid_out_text::LaidOutText;
use crate::layout::LayoutConfig;
use crate::placement::TextPlacement;
use crate::span::TextSpan;
use crate::style::TextStyle;

/// A stable, generation-checked handle to a text object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextHandle {
    /// Slot index in the text store.
    pub index: u32,
    /// Generation the slot held when this handle was issued.
    pub generation: u32,
}

/// A hint about how often a text changes, so the runtime can retain more (static)
/// or reuse allocations (dynamic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateMode {
    /// Rarely changes; fully-resolved batches are retained until invalidated.
    #[default]
    Static,
    /// Changes often; layout is recomputed on demand and allocations reused.
    Dynamic,
}

/// Which parts of a text's derived state are stale. The layout-affecting subset
/// (`content|font|glyph|line`) forces a re-layout; the rest only rebuild the
/// glyph batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DirtyFlags {
    /// Text content changed.
    pub content: bool,
    /// Font chain changed.
    pub font: bool,
    /// Glyph mapping changed.
    pub glyph: bool,
    /// Line layout changed.
    pub line: bool,
    /// Visual style (colour/opacity/decoration) changed.
    pub style: bool,
    /// Placement changed.
    pub placement: bool,
    /// Effect configuration changed.
    pub effect: bool,
    /// Final batch changed.
    pub batch: bool,
}

impl DirtyFlags {
    /// All clean.
    pub const CLEAN: DirtyFlags = DirtyFlags {
        content: false,
        font: false,
        glyph: false,
        line: false,
        style: false,
        placement: false,
        effect: false,
        batch: false,
    };

    /// Everything dirty (a freshly created text).
    pub const FRESH: DirtyFlags = DirtyFlags {
        content: true,
        font: true,
        glyph: true,
        line: true,
        style: true,
        placement: true,
        effect: true,
        batch: true,
    };

    /// Whether a re-layout is required.
    pub fn needs_layout(self) -> bool {
        self.content | self.font | self.glyph | self.line
    }
}

/// The retained state of one text object.
#[derive(Debug, Clone)]
pub(crate) struct TextRecord {
    pub fonts: Vec<FontHandle>,
    pub spans: Vec<TextSpan>,
    pub base_style: TextStyle,
    pub config: LayoutConfig,
    pub placement: TextPlacement,
    pub effects: Vec<TextEffect>,
    pub visible: bool,
    pub update_mode: UpdateMode,
    pub dirty: DirtyFlags,
    pub cache: Option<LaidOutText>,
}

impl TextRecord {
    /// A new record, fully dirty.
    pub fn new(fonts: Vec<FontHandle>, spans: Vec<TextSpan>) -> TextRecord {
        TextRecord {
            fonts,
            spans,
            base_style: TextStyle::default(),
            config: LayoutConfig::default(),
            placement: TextPlacement::default(),
            effects: Vec::new(),
            visible: true,
            update_mode: UpdateMode::Static,
            dirty: DirtyFlags::FRESH,
            cache: None,
        }
    }

    /// The total `char` count across all spans.
    pub fn char_count(&self) -> usize {
        self.spans.iter().map(TextSpan::char_count).sum()
    }

    /// Replace content — dirties layout.
    pub fn set_spans(&mut self, spans: Vec<TextSpan>) {
        self.spans = spans;
        self.mark_layout();
    }

    /// Replace the font chain — dirties layout.
    pub fn set_fonts(&mut self, fonts: Vec<FontHandle>) {
        self.fonts = fonts;
        self.dirty.font = true;
        self.dirty.batch = true;
        self.cache = None;
    }

    /// Replace the layout-affecting base style — dirties layout.
    pub fn set_base_style(&mut self, style: TextStyle) {
        self.base_style = style;
        self.mark_layout();
    }

    /// Change only the fill colour — dirties the batch, not layout.
    pub fn set_color(&mut self, color: crate::color::Rgba) {
        self.base_style.color = color;
        self.dirty.style = true;
        self.dirty.batch = true;
    }

    /// Change only opacity — dirties the batch, not layout.
    pub fn set_opacity(&mut self, opacity: axiom_kernel::Ratio) {
        self.base_style.opacity = opacity;
        self.dirty.style = true;
        self.dirty.batch = true;
    }

    /// Replace layout config — dirties layout.
    pub fn set_config(&mut self, config: LayoutConfig) {
        self.config = config;
        self.mark_layout();
    }

    /// Replace placement — dirties placement only, never layout.
    pub fn set_placement(&mut self, placement: TextPlacement) {
        self.placement = placement;
        self.dirty.placement = true;
        self.dirty.batch = true;
    }

    /// Replace effects — dirties effects only.
    pub fn set_effects(&mut self, effects: Vec<TextEffect>) {
        self.effects = effects;
        self.dirty.effect = true;
        self.dirty.batch = true;
    }

    /// Change visibility — dirties the batch only.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        self.dirty.batch = true;
    }

    /// Change the update-mode hint (static vs dynamic).
    pub fn set_update_mode(&mut self, mode: UpdateMode) {
        self.update_mode = mode;
    }

    /// Mark the layout-affecting flags and drop the cache.
    fn mark_layout(&mut self) {
        self.dirty.content = true;
        self.dirty.glyph = true;
        self.dirty.line = true;
        self.dirty.batch = true;
        self.cache = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;

    fn record() -> TextRecord {
        let mut r = TextRecord::new(
            vec![FontHandle {
                index: 0,
                generation: 0,
            }],
            vec![TextSpan::plain("hi")],
        );
        r.dirty = DirtyFlags::CLEAN;
        r
    }

    #[test]
    fn update_mode_is_settable() {
        let mut r = record();
        r.set_update_mode(UpdateMode::Dynamic);
        assert_eq!(r.update_mode, UpdateMode::Dynamic);
    }

    #[test]
    fn content_and_width_dirty_layout() {
        let mut r = record();
        r.set_spans(vec![TextSpan::plain("new")]);
        assert!(r.dirty.needs_layout());
        let mut r2 = record();
        r2.set_config(LayoutConfig {
            width: Some(axiom_host::Pixels::new(100.0).unwrap()),
            ..LayoutConfig::default()
        });
        assert!(r2.dirty.needs_layout());
    }

    #[test]
    fn color_opacity_placement_effects_do_not_dirty_layout() {
        let mut r = record();
        r.set_color(Rgba::BLACK);
        assert!(!r.dirty.needs_layout());
        assert!(r.dirty.batch);

        let mut r = record();
        r.set_opacity(axiom_kernel::Ratio::finite_or_zero(0.5));
        assert!(!r.dirty.needs_layout());

        let mut r = record();
        r.set_placement(TextPlacement::default());
        assert!(!r.dirty.needs_layout());
        assert!(r.dirty.placement);

        let mut r = record();
        r.set_effects(vec![TextEffect::reveal(
            axiom_kernel::Ratio::finite_or_zero(1.0),
        )]);
        assert!(!r.dirty.needs_layout());
        assert!(r.dirty.effect);
    }

    #[test]
    fn fresh_record_is_fully_dirty_and_counts_chars() {
        let r = TextRecord::new(
            vec![FontHandle {
                index: 0,
                generation: 0,
            }],
            vec![TextSpan::plain("hello")],
        );
        assert!(r.dirty.needs_layout());
        assert_eq!(r.char_count(), 5);
        assert_eq!(r.update_mode, UpdateMode::Static);
    }
}
