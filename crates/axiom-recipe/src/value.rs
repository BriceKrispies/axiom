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

    /// Carry an `f64` across **two** consecutive parameter words: low half
    /// first, then high.
    ///
    /// # Why a pair, and not a wider word
    ///
    /// A recipe carries values a domain wants to *store*, and some domains
    /// compute in `f64` and cannot narrow at the recipe boundary. A ported
    /// simulation is the clear case: 70% of one such corpus's constants are not
    /// representable in `f32`, and for the narrower ranges among them more than
    /// half of the values derived from them change even after being stored back
    /// into an `f32` buffer. Rounding a constant at the boundary is not a
    /// rounding — it computes a different function.
    ///
    /// The alternatives were both worse. Widening [`Param`] to 64 bits moves
    /// every stored recipe and every digest in the tree, breaks three encodings
    /// *defined* in `u32` words, and buys nothing where recipes are used at
    /// scale — `axiom-field` compiles to WGSL, and WGSL has no `f64`. Tagging
    /// the word reintroduces exactly the per-variant read this type's whole
    /// design avoids (see this module's header).
    ///
    /// A slot pair costs neither. An operator already knows the layout of its
    /// own slots — the precedent is in the tree, where a `u64` noise seed is two
    /// consecutive words — so it reads two and recombines. **No tag, no branch,
    /// no wire change, no digest movement.**
    ///
    /// This is also the named widening boundary, in the same spirit as the
    /// single/double vector conversions: "carry this in double precision" is a
    /// symbol you can search for, rather than a bit-shift open-coded at each
    /// call site.
    pub fn pair(value: f64) -> [Self; 2] {
        let bits = value.to_bits();
        [Self(bits as u32), Self((bits >> 32) as u32)]
    }

    /// Recombine the two words [`Param::pair`] wrote.
    pub fn from_pair(pair: [Self; 2]) -> f64 {
        f64::from_bits(u64::from(pair[0].0) | (u64::from(pair[1].0) << 32))
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

    /// The pair carries every `f64` bit pattern, including the ones a narrowing
    /// to `f32` would destroy: a value with mantissa bits below `f32`'s
    /// precision, one whose exponent `f32` cannot hold at either end, and a
    /// negative zero — which `f32` *can* represent but which a sloppy
    /// recombination through an integer would flatten.
    #[test]
    fn a_pair_round_trips_every_double_bit_pattern() {
        [
            0.012_f64,
            -8.0,
            std::f64::consts::PI,
            1e-300,
            1e300,
            -0.0,
            f64::MIN_POSITIVE,
            f64::MAX,
        ]
        .into_iter()
        .for_each(|v| {
            let back = Param::from_pair(Param::pair(v));
            assert_eq!(back.to_bits(), v.to_bits(), "{v}");
        });
    }

    /// NaN survives as the *same* NaN. A recipe is hashed by its bytes, so a
    /// payload that quietly canonicalises one NaN into another would move a
    /// digest without moving any value a caller can observe.
    #[test]
    fn a_pair_preserves_a_nan_payload_exactly() {
        let nan = f64::from_bits(0x7FF8_0000_DEAD_BEEF);
        assert_eq!(Param::from_pair(Param::pair(nan)).to_bits(), nan.to_bits());
    }

    /// Low half first. Stated as a test because it is the wire order two
    /// independently-written operators have to agree on, and nothing else in
    /// the type says which way round it goes.
    #[test]
    fn a_pair_is_low_word_then_high() {
        let p = Param::pair(f64::from_bits(0xAAAA_BBBB_CCCC_DDDD));
        assert_eq!((p[0].bits(), p[1].bits()), (0xCCCC_DDDD, 0xAAAA_BBBB));
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
