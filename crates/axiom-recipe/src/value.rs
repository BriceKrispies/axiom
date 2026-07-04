//! The raw parameter word and the typed views an operator reads it through.
//!
//! A [`Param`] is a single 32-bit word — the same "generic word" discipline the
//! `proc` recipe uses — deliberately untyped in the graph so the container stays
//! domain-free and branchless (no per-variant `match` to read a value). An
//! operator knows the meaning of each of its parameter slots and reads the word
//! through the matching view ([`Param::int`] / [`Param::scalar`] /
//! [`Param::color`]); no runtime tag check is involved.

/// A recipe scalar parameter — a plain `f32` carried in a parameter word. A
/// single-field quantity newtype, so `new`/`get` are the boundary where a raw
/// scalar enters/leaves a parameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scalar(f32);

impl Scalar {
    /// Wrap a raw scalar.
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    /// The raw scalar.
    pub const fn get(self) -> f32 {
        self.0
    }
}

/// A packed 8-bit-per-channel RGBA color carried in a parameter word (`0xRRGGBBAA`
/// with red in the high byte). A single-field quantity newtype.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(u32);

impl Color {
    /// Wrap a packed `0xRRGGBBAA` word.
    pub const fn from_packed(packed: u32) -> Self {
        Self(packed)
    }

    /// Build from four channels.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | (a as u32))
    }

    /// The packed word.
    pub const fn packed(self) -> u32 {
        self.0
    }

    /// The red channel.
    pub const fn r(self) -> u8 {
        (self.0 >> 24) as u8
    }

    /// The green channel.
    pub const fn g(self) -> u8 {
        (self.0 >> 16) as u8
    }

    /// The blue channel.
    pub const fn b(self) -> u8 {
        (self.0 >> 8) as u8
    }

    /// The alpha channel.
    pub const fn a(self) -> u8 {
        self.0 as u8
    }
}

/// One operator parameter: a raw 32-bit word read through a typed view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Param(u32);

impl Param {
    /// Wrap a raw word.
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// A word carrying an integer.
    pub const fn int(value: u32) -> Self {
        Self(value)
    }

    /// A word carrying a [`Scalar`] (its `f32` bit pattern).
    pub fn scalar(value: Scalar) -> Self {
        Self(value.get().to_bits())
    }

    /// A word carrying a [`Color`].
    pub const fn color(value: Color) -> Self {
        Self(value.packed())
    }

    /// The raw word.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Read the word as an integer.
    pub const fn as_int(self) -> u32 {
        self.0
    }

    /// Read the word as a [`Scalar`].
    pub fn as_scalar(self) -> Scalar {
        Scalar::new(f32::from_bits(self.0))
    }

    /// Read the word as a [`Color`].
    pub const fn as_color(self) -> Color {
        Color::from_packed(self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_round_trips_through_a_word() {
        let p = Param::scalar(Scalar::new(-2.5));
        assert_eq!(p.as_scalar().get(), -2.5);
    }

    #[test]
    fn int_round_trips_through_a_word() {
        let p = Param::int(4200);
        assert_eq!(p.as_int(), 4200);
        assert_eq!(p.bits(), 4200);
        assert_eq!(Param::from_bits(9).bits(), 9);
    }

    #[test]
    fn color_packs_and_unpacks_channels() {
        let c = Color::rgba(0x11, 0x22, 0x33, 0x44);
        assert_eq!(c.packed(), 0x1122_3344);
        assert_eq!((c.r(), c.g(), c.b(), c.a()), (0x11, 0x22, 0x33, 0x44));
        let p = Param::color(c);
        assert_eq!(p.as_color(), c);
        assert_eq!(Color::from_packed(0x1122_3344), c);
    }
}
