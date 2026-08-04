//! Prefiltered reductions of a material texture — the mip chain the sampler
//! needs in order to minify without aliasing.
//!
//! ## Why this module exists
//!
//! A material texture was uploaded with `mip_level_count: 1` and sampled with
//! `FilterMode::Nearest` on every axis. For a texture that is only ever seen at
//! roughly one texel per pixel that is fine, and it is what gives the engine its
//! crunchy un-smoothed look. For a surface that recedes — a road, a floor, a
//! terrain — it is not a look, it is a defect: past the depth where one screen
//! pixel covers more than one texel, a point sample returns *one arbitrary texel
//! out of the many the pixel actually covers*. Which one it returns is decided by
//! the sub-texel phase of the projection, so it changes as the camera advances.
//! The result is the classic pair of artifacts: a static moiré where the texel
//! grid beats against the pixel grid, and a crawl where that interference pattern
//! swims as the phase drifts.
//!
//! Neither is fixable by changing the texture. The information the pixel needs —
//! *the average of everything it covers* — is simply not present in a single
//! level, and no amount of tuning the source amplitude produces it. What produces
//! it is a chain of prefiltered reductions, each level the box-average of the one
//! above, so the sampler can pick (and, trilinearly, blend between) the level
//! whose texel size matches the pixel's footprint. That is what this module
//! builds.
//!
//! ## The averaging must happen in linear light
//!
//! This is the part that is easy to get silently wrong. An albedo is uploaded as
//! `Rgba8UnormSrgb`: the stored byte is an *sRGB-encoded* value, and the sampler
//! decodes it to linear before the shader ever sees it. Averaging the encoded
//! bytes therefore does not average the colours — sRGB is strongly concave, so a
//! byte-space mean of a dark and a light texel lands well below the true mean, and
//! a mip chain built that way darkens progressively with every level. On a road
//! that shows up as the distance getting muddier than the foreground for no
//! lighting reason.
//!
//! So each level decodes to linear, averages there, and re-encodes — but only for
//! the colour channels. **Alpha is linear even in an sRGB texture** (the sRGB
//! transfer function applies to RGB only), so it is averaged directly.
//! [`TexelEncoding`] is what a caller uses to say which of the two a given texture
//! is: an albedo is [`TexelEncoding::Srgb`], a tangent-space normal map is
//! [`TexelEncoding::Linear`] and must never be run through the transfer function.
//!
//! ## What this module deliberately does not do
//!
//! It does not renormalise normal-map vectors after averaging, and it does not
//! implement a wider reconstruction filter (Kaiser, Lanczos). A 2×2 box is the
//! filter the hardware's own mip generation uses, it is exactly the average the
//! trilinear sampler is defined against, and it is cheap enough to run at bind
//! time for every material without a GPU round trip — which is what keeps this
//! logic pure, native-testable, and inside the coverage gate, rather than a
//! render pass hidden behind a `wasm32` arm where nothing measures it.

/// How a texture's stored bytes encode the values the shader will read.
///
/// This is not cosmetic: it decides whether a reduction is allowed to average
/// the bytes directly, and getting it wrong is invisible in the base level and
/// compounds with every level below it. See the module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TexelEncoding {
    /// The bytes are already the values (a normal map, a data texture). Averaged
    /// as stored. Discriminant `0` — it indexes the transfer-function table.
    Linear = 0,
    /// The bytes are sRGB-encoded colour (an albedo). Decoded to linear before
    /// averaging and re-encoded after. Discriminant `1`.
    Srgb = 1,
}

/// One reduction of a texture: its dimensions and its RGBA8 texels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MipLevel {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl MipLevel {
    /// This level's width in texels (never zero).
    pub(crate) const fn width(&self) -> u32 {
        self.width
    }

    /// This level's height in texels (never zero).
    pub(crate) const fn height(&self) -> u32 {
        self.height
    }

    /// This level's RGBA8 texels, row-major, exactly `width * height * 4` bytes.
    pub(crate) fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

/// How many mip levels a `width x height` texture has, counting the base.
///
/// The full chain, down to the 1×1 level that is the whole texture's average —
/// stopping short would leave the most-minified samples with no level small
/// enough to cover their footprint, which is the same aliasing one level up.
///
/// Each level halves both axes with a floor, so the count is set by the *longer*
/// axis: `floor(log2(max)) + 1`, computed as `32 - leading_zeros` because that is
/// exact for every `u32` and has no floating-point rounding to argue about at the
/// power-of-two boundaries. A zero dimension is treated as one, matching the
/// clamp the upload path applies before it ever gets here.
pub(crate) fn level_count(width: u32, height: u32) -> u32 {
    let longest = width.max(height).max(1);
    u32::BITS - longest.leading_zeros()
}

/// Build every mip level **below** the base of a `width x height` RGBA8 texture.
///
/// Returns levels `1 ..= level_count - 1`, in order, each the box-average of the
/// one before it. The base is not returned because the caller already has it —
/// copying a full-size level to hand it straight back would be the largest
/// allocation in the whole operation and buy nothing.
///
/// `base` shorter than `width * height * 4` reads as zero past its end rather
/// than panicking. The authoring layer already rejects a malformed buffer
/// (`add_texture_data` validates the length before a texture id is ever issued),
/// so this is a floor, not a policy: this code runs inside GPU bind, where a
/// panic is a blank page rather than a caught error, and a texture is not worth
/// that.
pub(crate) fn build(
    width: u32,
    height: u32,
    base: &[u8],
    encoding: TexelEncoding,
) -> Vec<MipLevel> {
    let root = MipLevel {
        width: width.max(1),
        height: height.max(1),
        pixels: base.to_vec(),
    };
    // `scan` rather than a loop: each level is the reduction of the previous, so
    // the chain is a fold that yields its intermediate states.
    (1..level_count(width, height))
        .scan(root, |previous, _| {
            *previous = reduce(previous, encoding);
            Some(previous.clone())
        })
        .collect()
}

/// One 2×2 box reduction of `src`.
///
/// Every destination texel averages the four source texels it covers, with each
/// source coordinate clamped inside `src`. The clamp is what makes an odd
/// dimension safe: halving 5 with a floor gives 2, whose second texel wants
/// source columns 4 and 5 — column 5 does not exist, so it reads column 4 twice.
/// That is a slight reweighting at the last row/column of an odd level and it is
/// the standard cost of a floor-halved chain; the alternative (a weighted
/// three-tap) buys nothing a material texture can see.
fn reduce(src: &MipLevel, encoding: TexelEncoding) -> MipLevel {
    let width = (src.width >> 1).max(1);
    let height = (src.height >> 1).max(1);
    let pixels = (0..width * height)
        .flat_map(|index| {
            let (x, y) = (index % width, index / width);
            let x0 = (x * 2).min(src.width - 1);
            let x1 = (x * 2 + 1).min(src.width - 1);
            let y0 = (y * 2).min(src.height - 1);
            let y1 = (y * 2 + 1).min(src.height - 1);
            let corners = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)];
            (0..4usize).map(move |channel| average(src, &corners, channel, encoding))
        })
        .collect();
    MipLevel {
        width,
        height,
        pixels,
    }
}

/// The averaged byte for one channel of one destination texel.
///
/// The colour channels average in linear light; alpha averages as stored, because
/// the sRGB transfer function covers RGB only. Selecting between the two by table
/// index rather than by a branch keeps this inside the Branchless Law, and costs
/// one extra transfer-function evaluation per alpha texel — which is why the
/// table is indexed, not the arithmetic.
fn average(src: &MipLevel, corners: &[(u32, u32); 4], channel: usize, encoding: TexelEncoding) -> u8 {
    let encoding = [encoding, TexelEncoding::Linear][usize::from(channel == 3)];
    let sum: f32 = corners
        .iter()
        .map(|(x, y)| to_linear(texel(src, *x, *y, channel), encoding))
        .sum();
    let mean = from_linear(sum * 0.25, encoding);
    (mean * 255.0).round().clamp(0.0, 255.0) as u8
}

/// One channel byte of one texel, or `0` if the buffer is short — see [`build`].
fn texel(src: &MipLevel, x: u32, y: u32, channel: usize) -> u8 {
    let index = (y as usize * src.width as usize + x as usize) * 4 + channel;
    src.pixels.get(index).copied().unwrap_or(0)
}

/// A stored byte as the linear value the shader would receive from it.
fn to_linear(byte: u8, encoding: TexelEncoding) -> f32 {
    let unit = byte as f32 / 255.0;
    [unit, srgb_to_linear(unit)][encoding as usize]
}

/// A linear value back to the byte this texture stores it as.
fn from_linear(value: f32, encoding: TexelEncoding) -> f32 {
    [value, linear_to_srgb(value)][encoding as usize]
}

/// The sRGB electro-optical transfer function, `0..=1` in and out.
///
/// Both arms are always evaluated — the table index is what selects, not a
/// branch — so both must be safe on the whole domain. They are: the argument to
/// `powf` is `(e + 0.055) / 1.055`, which is non-negative for every `e` this is
/// called with.
fn srgb_to_linear(encoded: f32) -> f32 {
    let toe = encoded / 12.92;
    let curve = ((encoded + 0.055) / 1.055).powf(2.4);
    [curve, toe][usize::from(encoded <= 0.040_45)]
}

/// The inverse of [`srgb_to_linear`].
fn linear_to_srgb(linear: f32) -> f32 {
    let toe = linear * 12.92;
    let curve = 1.055 * linear.max(0.0).powf(1.0 / 2.4) - 0.055;
    [curve, toe][usize::from(linear <= 0.003_130_8)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A level's texel as `(r, g, b, a)`.
    fn at(level: &MipLevel, x: u32, y: u32) -> (u8, u8, u8, u8) {
        let i = (y as usize * level.width() as usize + x as usize) * 4;
        let p = level.pixels();
        (p[i], p[i + 1], p[i + 2], p[i + 3])
    }

    /// A `w x h` texture whose every channel is `value(x, y)`.
    fn solid(width: u32, height: u32, value: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        (0..width * height)
            .flat_map(|i| {
                let v = value(i % width, i / width);
                [v, v, v, 255]
            })
            .collect()
    }

    #[test]
    fn the_chain_runs_all_the_way_down_to_a_single_texel() {
        // The full chain, counting the base: 32,16,8,4,2,1.
        assert_eq!(level_count(32, 32), 6);
        // A 1x1 texture is already its own smallest level.
        assert_eq!(level_count(1, 1), 1);
        // The longer axis sets the count: 8x1 still needs 8,4,2,1.
        assert_eq!(level_count(8, 1), 4);
        assert_eq!(level_count(1, 8), 4);
        // Non-power-of-two floors: 5 -> 2 -> 1.
        assert_eq!(level_count(5, 3), 3);
        // The clamp the upload path also applies, so a degenerate size cannot
        // produce a zero-level texture the GPU would reject.
        assert_eq!(level_count(0, 0), 1);
    }

    #[test]
    fn build_returns_every_level_below_the_base_at_the_expected_size() {
        let base = solid(8, 8, |_, _| 128);
        let chain = build(8, 8, &base, TexelEncoding::Linear);
        // Levels 1..=3 — the base is the caller's, and is not handed back.
        assert_eq!(chain.len(), 3);
        assert_eq!(
            chain.iter().map(|l| (l.width(), l.height())).collect::<Vec<_>>(),
            vec![(4, 4), (2, 2), (1, 1)]
        );
        // And every level carries exactly the bytes its extent implies, which is
        // what `write_texture`'s `bytes_per_row`/`rows_per_image` assume.
        for level in &chain {
            assert_eq!(
                level.pixels().len(),
                (level.width() * level.height() * 4) as usize
            );
        }
    }

    #[test]
    fn a_one_texel_texture_has_no_levels_below_it() {
        let chain = build(1, 1, &[10, 20, 30, 40], TexelEncoding::Srgb);
        assert!(chain.is_empty(), "a 1x1 texture is already fully reduced");
    }

    /// A flat texture must survive the whole chain unchanged. This is the test
    /// that catches a transfer-function mistake immediately: average a constant
    /// and you must get that constant back, in either encoding.
    #[test]
    fn a_flat_texture_reduces_to_itself_in_both_encodings() {
        for encoding in [TexelEncoding::Linear, TexelEncoding::Srgb] {
            let base = solid(16, 16, |_, _| 73);
            for level in build(16, 16, &base, encoding) {
                // Bound before the assert rather than passed as message
                // arguments: an `assert!` message is only evaluated on failure,
                // so inline `level.width()` calls would be regions no passing
                // run ever executes.
                let (w, h) = (level.width(), level.height());
                assert!(
                    level.pixels().chunks(4).all(|t| t[0] == 73),
                    "{encoding:?} level {w}x{h} drifted off a flat 73"
                );
            }
        }
    }

    /// The reduction is the box average, on a case where the answer is exact in
    /// both directions: a 2x2 of 0/0/0/255 in *linear* averages to 64 (255/4
    /// rounded).
    #[test]
    fn a_linear_reduction_is_the_plain_mean_of_its_four_texels() {
        let base = vec![
            0, 0, 0, 255, // (0,0)
            0, 0, 0, 255, // (1,0)
            0, 0, 0, 255, // (0,1)
            255, 255, 255, 255, // (1,1)
        ];
        let chain = build(2, 2, &base, TexelEncoding::Linear);
        assert_eq!(at(&chain[0], 0, 0), (64, 64, 64, 255));
    }

    /// The same four texels in an **sRGB** texture must average to a much
    /// brighter byte, because the mean is taken in linear light where the single
    /// white texel carries far more weight than its encoded byte suggests.
    ///
    /// This is the regression that a byte-space average would silently pass in
    /// every other test: `0,0,0,255` averages to 64 in byte space, but one part
    /// white in linear is 0.25 linear, which encodes to 137.
    #[test]
    fn an_srgb_reduction_averages_in_linear_light_not_in_byte_space() {
        let base = vec![
            0, 0, 0, 255, //
            0, 0, 0, 255, //
            0, 0, 0, 255, //
            255, 255, 255, 255,
        ];
        let chain = build(2, 2, &base, TexelEncoding::Srgb);
        let (r, ..) = at(&chain[0], 0, 0);
        assert_eq!(r, 137, "an sRGB mip must average in linear light, not bytes");
        assert!(
            r > 64,
            "byte-space averaging darkens every level; that is the bug this pins"
        );
    }

    /// Alpha is linear even inside an sRGB texture, so it must be averaged as
    /// stored. Running it through the transfer function would push a half-covered
    /// texel from 128 to 188 and make every cutout material's edges swell as it
    /// minifies.
    #[test]
    fn alpha_is_averaged_as_stored_even_in_an_srgb_texture() {
        let base = vec![
            9, 9, 9, 0, //
            9, 9, 9, 0, //
            9, 9, 9, 255, //
            9, 9, 9, 255,
        ];
        let chain = build(2, 2, &base, TexelEncoding::Srgb);
        let (.., a) = at(&chain[0], 0, 0);
        assert_eq!(a, 128, "alpha must average in byte space: (0+0+255+255)/4");
    }

    /// The clamp that makes an odd dimension safe: 3 halves to 1, and that one
    /// destination texel reads column/row 2 twice rather than reading off the end.
    #[test]
    fn an_odd_dimension_clamps_its_footprint_inside_the_source() {
        // A 3x1 row of 0, 0, 255. The single destination texel covers columns 0
        // and 1 (both zero), so it is 0 — column 2 is dropped by the floor, which
        // is the standard convention.
        let base = vec![0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255];
        let chain = build(3, 1, &base, TexelEncoding::Linear);
        assert_eq!(chain[0].width(), 1);
        assert_eq!(chain[0].height(), 1);
        assert_eq!(at(&chain[0], 0, 0), (0, 0, 0, 255));

        // And a 1x3 column, which exercises the *row* clamp rather than the
        // column clamp.
        let tall = build(1, 3, &base, TexelEncoding::Linear);
        assert_eq!((tall[0].width(), tall[0].height()), (1, 1));
    }

    /// A short buffer reads as zero rather than panicking — the floor described
    /// on [`build`]. The authoring layer rejects this case, so what is asserted
    /// here is only that GPU bind cannot be taken down by it.
    #[test]
    fn a_short_buffer_reads_as_zero_instead_of_panicking() {
        // Claims 4x4 (256 bytes) but supplies one texel.
        let chain = build(4, 4, &[255, 255, 255, 255], TexelEncoding::Srgb);
        assert_eq!(chain.len(), 2);
        // The one real texel still contributes to the level-1 texel that covers
        // it; every other texel is the zero the missing bytes read as.
        assert_eq!(at(&chain[0], 0, 0).0, 137);
        assert_eq!(at(&chain[0], 1, 1), (0, 0, 0, 0));
    }

    /// The transfer function and its inverse, pinned at both ends, across the
    /// toe/curve join, and as a round trip. Both arms of both tables are hit.
    #[test]
    fn the_srgb_transfer_function_round_trips() {
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1.0e-6);
        assert_eq!(linear_to_srgb(0.0), 0.0);
        assert!((linear_to_srgb(1.0) - 1.0).abs() < 1.0e-6);
        // The linear toe, below the join on each side.
        assert!((srgb_to_linear(0.02) - 0.02 / 12.92).abs() < 1.0e-9);
        assert!((linear_to_srgb(0.001) - 0.001 * 12.92).abs() < 1.0e-9);
        // And the curve above it, round-tripped.
        for encoded in [0.05f32, 0.25, 0.5, 0.75] {
            let back = linear_to_srgb(srgb_to_linear(encoded));
            assert!((back - encoded).abs() < 1.0e-5, "{encoded} round-tripped to {back}");
        }
        // A negative linear value cannot come out of an average of non-negative
        // texels, but `powf` of a negative is NaN, so the guard is asserted
        // rather than assumed.
        assert!(linear_to_srgb(-1.0).is_finite());
    }

    /// The encodings select different arithmetic — a table-index mistake that
    /// made them identical would pass every flat-texture test above.
    #[test]
    fn the_two_encodings_are_not_the_same_transform() {
        assert_eq!(to_linear(128, TexelEncoding::Linear), 128.0 / 255.0);
        assert!(to_linear(128, TexelEncoding::Srgb) < 0.25, "sRGB decode darkens a mid byte");
        assert_eq!(from_linear(0.5, TexelEncoding::Linear), 0.5);
        assert!(from_linear(0.5, TexelEncoding::Srgb) > 0.7, "sRGB encode lifts a mid value");
    }

    #[test]
    fn building_the_same_texture_twice_produces_the_same_chain() {
        let base = solid(16, 16, |x, y| ((x * 37 + y * 11) % 256) as u8);
        assert_eq!(
            build(16, 16, &base, TexelEncoding::Srgb),
            build(16, 16, &base, TexelEncoding::Srgb)
        );
    }
}
