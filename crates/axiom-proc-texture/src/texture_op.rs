//! The texture operator codes, as an authoring-friendly enum.

/// The nine texture operators. The discriminant **is** the operator code stored
/// in a recipe node, and it indexes the dispatch table, so this order is the
/// dispatch order and must not be reshuffled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum TextureOp {
    /// Fill with one color. Params: `[width, height, color]`.
    Solid = 0,
    /// Horizontal ramp `color_a`→`color_b`. Params: `[width, height, a, b]`.
    Gradient = 1,
    /// Value noise remapped `lo`..`hi`. Params: `[width, height, scale, lo, hi]`.
    Noise = 2,
    /// Staggered bricks. Params: `[width, height, rows, cols, mortar, brick, mortar_color]`.
    Bricks = 3,
    /// Box blur of one input. Params: `[radius]`.
    Blur = 4,
    /// Mix two equal-size inputs. Params: `[factor]`.
    Blend = 5,
    /// Remap one input's luminance `lo`..`hi`. Params: `[lo, hi]`.
    ColorRamp = 6,
    /// Normal map from one input's height. Params: `[strength]`.
    HeightToNormal = 7,
    /// Alternating 2-color grid. Params: `[width, height, cell, color_a, color_b]`.
    Checker = 8,
    /// Bitmap-font text (5×7 glyphs, A–Z / 0–9 / space) centred on a background.
    /// Params: `[width, height, fg, bg, scale, char_count, packed_0, …]` where
    /// each `packed_i` word holds up to 4 ASCII chars (one per byte).
    Text = 9,
    /// A base fill stamped with filled circles ("spots"/"dots"). Params:
    /// `[width, height, base_color, spot_color, count, cx0, cy0, r0, …]` — the
    /// 5-word header is followed by `count` `(center_x, center_y, radius)` triples
    /// in texel space; a texel inside any spot's radius is `spot_color`, else
    /// `base_color`. The general dotted/panelled-surface primitive — polka, hide,
    /// and the soccer ball's dark pentagon rosette baked onto the sphere's UVs.
    Spots = 10,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_their_dispatch_indices() {
        assert_eq!(TextureOp::Solid as u16, 0);
        assert_eq!(TextureOp::HeightToNormal as u16, 7);
        assert_eq!(TextureOp::Checker as u16, 8);
        assert_eq!(TextureOp::Text as u16, 9);
        assert_eq!(TextureOp::Spots as u16, 10);
    }
}
