//! `Rgba`: a validated colour with straight alpha, in `0.0..=1.0`.

use axiom_kernel::{BinaryWriter, Ratio};

use crate::text_error::{TextError, TextResult};

/// A colour with four channels in `0.0..=1.0` and straight (non-premultiplied)
/// alpha. Channels are stored privately as `f32` and only ever handed out as
/// dimensionless [`Ratio`]s, so the public surface carries no naked float. Text
/// carries colour as neutral data; a backend decides the exact encoding. The TS
/// SDK's `"#rrggbbaa"` strings are parsed to this at the app edge, never stored
/// as strings in the engine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl Rgba {
    /// Opaque white — the clean default text colour.
    pub const WHITE: Rgba = Rgba {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    /// Opaque black.
    pub const BLACK: Rgba = Rgba {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };
    /// Fully transparent.
    pub const TRANSPARENT: Rgba = Rgba {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    /// Construct a colour from 8-bit channels (`0..=255` → `0.0..=1.0`).
    pub const fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Rgba {
        Rgba {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    /// Construct a colour from a packed `0xRRGGBBAA` value (the TS `"#rrggbbaa"`
    /// form).
    pub const fn from_hex(rgba: u32) -> Rgba {
        Rgba::from_rgba8(
            (rgba >> 24) as u8,
            (rgba >> 16) as u8,
            (rgba >> 8) as u8,
            rgba as u8,
        )
    }

    /// Construct a colour from four dimensionless [`Ratio`] channels.
    pub fn from_ratios(r: Ratio, g: Ratio, b: Ratio, a: Ratio) -> Rgba {
        Rgba {
            r: r.get(),
            g: g.get(),
            b: b.get(),
            a: a.get(),
        }
    }

    /// The four channels as dimensionless ratios `[r, g, b, a]`.
    pub fn channels(self) -> [Ratio; 4] {
        [self.r, self.g, self.b, self.a].map(Ratio::finite_or_zero)
    }

    /// The straight-alpha channel.
    pub fn alpha(self) -> Ratio {
        Ratio::finite_or_zero(self.a)
    }

    /// This colour with its alpha multiplied by `opacity`.
    pub fn with_opacity(self, opacity: Ratio) -> Rgba {
        Rgba {
            a: self.a * opacity.get(),
            ..self
        }
    }

    /// Reject any non-finite or out-of-`[0,1]` channel.
    pub fn validate(self) -> TextResult<()> {
        [self.r, self.g, self.b, self.a]
            .into_iter()
            .all(|c| c.is_finite() & (c >= 0.0) & (c <= 1.0))
            .then_some(())
            .ok_or(TextError::InvalidOpacity)
    }

    /// Append the four channels (used by the deterministic snapshot).
    pub(crate) fn write_to(self, writer: &mut BinaryWriter) {
        [self.r, self.g, self.b, self.a]
            .into_iter()
            .for_each(|c| writer.write_f32(c));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_and_constructors() {
        assert_eq!(Rgba::WHITE.validate(), Ok(()));
        assert_eq!(Rgba::from_rgba8(255, 255, 255, 255), Rgba::WHITE);
        assert_eq!(Rgba::from_hex(0xFFFFFFFF), Rgba::WHITE);
        assert_eq!(Rgba::from_hex(0x000000FF), Rgba::BLACK);
        assert_eq!(Rgba::TRANSPARENT.alpha().get(), 0.0);
        assert_eq!(Rgba::WHITE.channels()[0].get(), 1.0);
        let gold = Rgba::from_ratios(
            Ratio::finite_or_zero(1.0),
            Ratio::finite_or_zero(0.5),
            Ratio::finite_or_zero(0.25),
            Ratio::finite_or_zero(1.0),
        );
        assert_eq!(gold.channels()[1].get(), 0.5);
    }

    #[test]
    fn opacity_folds_into_alpha() {
        assert_eq!(
            Rgba::WHITE
                .with_opacity(Ratio::finite_or_zero(0.5))
                .alpha()
                .get(),
            0.5
        );
    }

    #[test]
    fn rejects_out_of_range_and_nan() {
        assert_eq!(
            Rgba::from_ratios(
                Ratio::finite_or_zero(1.5),
                Ratio::finite_or_zero(0.0),
                Ratio::finite_or_zero(0.0),
                Ratio::finite_or_zero(1.0),
            )
            .validate(),
            Err(TextError::InvalidOpacity)
        );
    }

    #[test]
    fn writes_four_channels() {
        let mut w = BinaryWriter::new();
        Rgba::from_hex(0x11223344).write_to(&mut w);
        assert_eq!(w.len(), 16);
    }
}
