//! Resolved text style and the sparse overrides that layer into one.
//!
//! Style resolution is a fixed four-level cascade — engine [`TextStyle::default`]
//! → a reusable named style → the text-level style → a span-level override — each
//! expressed as a sparse [`StyleOverride`] applied over the level below. The
//! cascade is pure and order-deterministic, so the same inputs always resolve to
//! the same concrete style. Lengths are [`Pixels`], normalized/multiplier values
//! are [`Ratio`] — the public surface carries no naked `f32`.

use axiom_host::Pixels;
use axiom_kernel::Ratio;
use axiom_math::Vec2;

use crate::color::Rgba;
use crate::face_slant::FaceSlant;
use crate::text_error::{TextError, TextResult};

/// A fully-resolved, concrete text style: every property has a value. Layout and
/// the glyph batch read this; callers build one by layering [`StyleOverride`]s
/// over [`TextStyle::default`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// Font size in pixels (`> 0`).
    pub font_size: Pixels,
    /// OpenType weight class used to pick a face (100..=900).
    pub weight: u16,
    /// Upright/italic/oblique presentation.
    pub slant: FaceSlant,
    /// Fill colour.
    pub color: Rgba,
    /// Extra opacity multiplied into the fill alpha (`0.0..=1.0`).
    pub opacity: Ratio,
    /// Extra advance between glyphs, in pixels.
    pub letter_spacing: Pixels,
    /// Extra advance added at space glyphs, in pixels.
    pub word_spacing: Pixels,
    /// Line advance as a multiple of the font's natural line height (`> 0`).
    pub line_height: Ratio,
    /// Outline stroke width in pixels (`>= 0`; `0` disables).
    pub outline_width: Pixels,
    /// Outline colour.
    pub outline_color: Rgba,
    /// Drop-shadow offset in pixels.
    pub shadow_offset: Vec2,
    /// Drop-shadow colour (transparent disables).
    pub shadow_color: Rgba,
    /// Underline decoration.
    pub underline: bool,
    /// Strikethrough decoration.
    pub strikethrough: bool,
}

/// A pixel length known to be finite (used for the default style's constants).
fn px(value: f32) -> Pixels {
    Pixels::new(value).expect("a style default is a finite pixel length")
}

impl Default for TextStyle {
    /// The clean default: readable opaque-white 16px text, no outline, shadow, or
    /// decoration — safe on a dark or medium background with zero configuration.
    fn default() -> TextStyle {
        TextStyle {
            font_size: px(16.0),
            weight: 400,
            slant: FaceSlant::Upright,
            color: Rgba::WHITE,
            opacity: Ratio::finite_or_zero(1.0),
            letter_spacing: px(0.0),
            word_spacing: px(0.0),
            line_height: Ratio::finite_or_zero(1.2),
            outline_width: px(0.0),
            outline_color: Rgba::BLACK,
            shadow_offset: Vec2::ZERO,
            shadow_color: Rgba::TRANSPARENT,
            underline: false,
            strikethrough: false,
        }
    }
}

impl TextStyle {
    /// Reject out-of-range style numbers with a specific error. (Lengths are
    /// already finite by construction; only their sign/range is checked.)
    pub fn validate(self) -> TextResult<()> {
        (self.font_size.get() > 0.0)
            .then_some(())
            .ok_or(TextError::InvalidFontSize)
            .and_then(|()| {
                ((self.opacity.get() >= 0.0) & (self.opacity.get() <= 1.0))
                    .then_some(())
                    .ok_or(TextError::InvalidOpacity)
            })
            .and_then(|()| {
                (self.line_height.get() > 0.0)
                    .then_some(())
                    .ok_or(TextError::InvalidLineHeight)
            })
            .and_then(|()| {
                ((self.outline_width.get() >= 0.0)
                    & self.shadow_offset.x.is_finite()
                    & self.shadow_offset.y.is_finite())
                .then_some(())
                .ok_or(TextError::InvalidDimensions)
            })
            .and_then(|()| self.color.validate())
            .and_then(|()| self.outline_color.validate())
            .and_then(|()| self.shadow_color.validate())
    }
}

/// A sparse set of style overrides: every field is `Some` only where this level
/// of the cascade sets it. An empty override is the identity.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StyleOverride {
    /// Overrides [`TextStyle::font_size`].
    pub font_size: Option<Pixels>,
    /// Overrides [`TextStyle::weight`].
    pub weight: Option<u16>,
    /// Overrides [`TextStyle::slant`].
    pub slant: Option<FaceSlant>,
    /// Overrides [`TextStyle::color`].
    pub color: Option<Rgba>,
    /// Overrides [`TextStyle::opacity`].
    pub opacity: Option<Ratio>,
    /// Overrides [`TextStyle::letter_spacing`].
    pub letter_spacing: Option<Pixels>,
    /// Overrides [`TextStyle::word_spacing`].
    pub word_spacing: Option<Pixels>,
    /// Overrides [`TextStyle::line_height`].
    pub line_height: Option<Ratio>,
    /// Overrides [`TextStyle::outline_width`].
    pub outline_width: Option<Pixels>,
    /// Overrides [`TextStyle::outline_color`].
    pub outline_color: Option<Rgba>,
    /// Overrides [`TextStyle::shadow_offset`].
    pub shadow_offset: Option<Vec2>,
    /// Overrides [`TextStyle::shadow_color`].
    pub shadow_color: Option<Rgba>,
    /// Overrides [`TextStyle::underline`].
    pub underline: Option<bool>,
    /// Overrides [`TextStyle::strikethrough`].
    pub strikethrough: Option<bool>,
}

impl StyleOverride {
    /// Apply this override on top of `base`, keeping `base` where a field is
    /// `None`. This is the single merge step the cascade repeats.
    pub fn apply(self, base: TextStyle) -> TextStyle {
        TextStyle {
            font_size: self.font_size.unwrap_or(base.font_size),
            weight: self.weight.unwrap_or(base.weight),
            slant: self.slant.unwrap_or(base.slant),
            color: self.color.unwrap_or(base.color),
            opacity: self.opacity.unwrap_or(base.opacity),
            letter_spacing: self.letter_spacing.unwrap_or(base.letter_spacing),
            word_spacing: self.word_spacing.unwrap_or(base.word_spacing),
            line_height: self.line_height.unwrap_or(base.line_height),
            outline_width: self.outline_width.unwrap_or(base.outline_width),
            outline_color: self.outline_color.unwrap_or(base.outline_color),
            shadow_offset: self.shadow_offset.unwrap_or(base.shadow_offset),
            shadow_color: self.shadow_color.unwrap_or(base.shadow_color),
            underline: self.underline.unwrap_or(base.underline),
            strikethrough: self.strikethrough.unwrap_or(base.strikethrough),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(v: f32) -> Pixels {
        Pixels::new(v).unwrap()
    }

    #[test]
    fn default_is_clean_and_valid() {
        let d = TextStyle::default();
        assert_eq!(d.validate(), Ok(()));
        assert_eq!(d.color, Rgba::WHITE);
        assert_eq!(d.font_size.get(), 16.0);
        assert!(!d.underline);
    }

    #[test]
    fn cascade_layers_named_text_and_span() {
        let named = StyleOverride {
            font_size: Some(px(32.0)),
            ..Default::default()
        };
        let text = StyleOverride {
            color: Some(Rgba::BLACK),
            ..Default::default()
        };
        let span = StyleOverride {
            weight: Some(700),
            ..Default::default()
        };
        let resolved = span.apply(text.apply(named.apply(TextStyle::default())));
        assert_eq!(resolved.font_size.get(), 32.0, "named level survives");
        assert_eq!(resolved.color, Rgba::BLACK, "text level survives");
        assert_eq!(resolved.weight, 700, "span level survives");
        assert_eq!(resolved.line_height.get(), 1.2, "unset stays default");
    }

    #[test]
    fn empty_override_is_identity() {
        assert_eq!(
            StyleOverride::default().apply(TextStyle::default()),
            TextStyle::default()
        );
    }

    #[test]
    fn rejects_bad_numbers() {
        assert_eq!(
            TextStyle {
                font_size: px(0.0),
                ..TextStyle::default()
            }
            .validate(),
            Err(TextError::InvalidFontSize)
        );
        assert_eq!(
            TextStyle {
                opacity: Ratio::finite_or_zero(2.0),
                ..TextStyle::default()
            }
            .validate(),
            Err(TextError::InvalidOpacity)
        );
        assert_eq!(
            TextStyle {
                line_height: Ratio::finite_or_zero(-1.0),
                ..TextStyle::default()
            }
            .validate(),
            Err(TextError::InvalidLineHeight)
        );
        assert_eq!(
            TextStyle {
                outline_width: px(-1.0),
                ..TextStyle::default()
            }
            .validate(),
            Err(TextError::InvalidDimensions)
        );
        assert_eq!(
            TextStyle {
                shadow_offset: Vec2::new(f32::NAN, 0.0),
                ..TextStyle::default()
            }
            .validate(),
            Err(TextError::InvalidDimensions)
        );
    }
}
