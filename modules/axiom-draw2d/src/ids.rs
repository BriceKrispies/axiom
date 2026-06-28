//! The draw2d module's value-type vocabulary — the pure data nouns the
//! [`crate::Draw2dApi`] facade returns and accepts.
//!
//! Kept in an `ids` module so the single `pub use ids::{…}` line in `lib.rs` is
//! published as identity/value vocabulary (Module Law #8), not counted as a
//! second behavioral facade. None of these carry behaviour beyond construction
//! and field access; all engine behaviour lives behind `Draw2dApi`. The type
//! *definitions* live in their own files; this module only re-exports them as
//! the crate's public noun vocabulary.

pub use crate::camera2d::Camera2d;
pub use crate::common2d::{Common2d, Shadow2d};
pub use crate::draw2d_command::Draw2dCommand;
pub use crate::draw2d_list::Draw2dList;
pub use crate::fill2d::{Fill2d, Stroke2d};
pub use crate::handles::{FontHandle, PaintId, TextureId, TransformDepth};
pub use crate::paint::GradientStop;
pub use crate::rect::Rect;
pub use crate::rgba::Rgba;
pub use crate::sprite_draw2d::SpriteDraw2d;
pub use crate::text2d::{Glyph2d, GlyphRun, TextAlign, TextDraw2d, TextMetrics};
