//! The text module's pure value vocabulary, re-exported as one group.
//!
//! Per Module Law #8, `lib.rs` publishes exactly one behavioral facade
//! ([`crate::TextApi`]) plus the value types that facade traffics in. Those nouns
//! are collected here and re-exported through a single `pub use ids::{…}` so the
//! facade's handles, contracts, and configuration can be named by callers without
//! widening the behavioral surface.

pub use crate::color::Rgba;
pub use crate::compiled_font::CompiledFont;
pub use crate::effect::{EffectKind, TextEffect};
pub use crate::face_metrics::FaceMetrics;
pub use crate::face_slant::FaceSlant;
pub use crate::font_builder::{FontBuild, GlyphInput};
pub use crate::font_registry::FontHandle;
pub use crate::glyph_batch::{GlyphBatch, GlyphInstance};
pub use crate::laid_out_text::{LaidOutText, LineMetrics, TextBounds};
pub use crate::layout::{Align, LayoutConfig, Overflow, VerticalAlign, Wrap};
pub use crate::placement::{Billboard, ScreenPlacement, Space, TextPlacement, WorldPlacement};
pub use crate::span::TextSpan;
pub use crate::style::{StyleOverride, TextStyle};
pub use crate::text_error::{TextError, TextResult};
pub use crate::text_limits::TextLimits;
pub use crate::text_record::{DirtyFlags, TextHandle, UpdateMode};
pub use crate::text_snapshot::TextSnapshot;
