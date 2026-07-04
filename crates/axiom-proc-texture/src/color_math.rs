//! Small, explicit color arithmetic the texture operators share.

use axiom_recipe::Color;

/// A packed recipe [`Color`] as an `[r, g, b, a]` pixel.
pub(crate) fn rgba(c: Color) -> [u8; 4] {
    [c.r(), c.g(), c.b(), c.a()]
}

/// Linear interpolation of one channel: `a` at `t = 0`, `b` at `t = 1`, rounded
/// to the nearest byte with `t` clamped into `[0, 1]`.
pub(crate) fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    let ft = t.clamp(0.0, 1.0);
    (f32::from(a) + (f32::from(b) - f32::from(a)) * ft)
        .round()
        .clamp(0.0, 255.0) as u8
}

/// Per-channel linear interpolation between two RGBA pixels.
pub(crate) fn lerp_rgba(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    [
        lerp_u8(a[0], b[0], t),
        lerp_u8(a[1], b[1], t),
        lerp_u8(a[2], b[2], t),
        lerp_u8(a[3], b[3], t),
    ]
}

/// Perceptual luminance of an RGB pixel in `[0, 1]` (Rec. 601 weights). Alpha is
/// ignored — luminance drives height/ramp lookups.
pub(crate) fn luminance(px: [u8; 4]) -> f32 {
    (0.299 * f32::from(px[0]) + 0.587 * f32::from(px[1]) + 0.114 * f32::from(px[2])) / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_u8_hits_endpoints_and_midpoint_and_clamps_t() {
        assert_eq!(lerp_u8(0, 100, 0.0), 0);
        assert_eq!(lerp_u8(0, 100, 1.0), 100);
        assert_eq!(lerp_u8(0, 100, 0.5), 50);
        assert_eq!(lerp_u8(0, 100, -1.0), 0);
        assert_eq!(lerp_u8(0, 100, 2.0), 100);
    }

    #[test]
    fn lerp_rgba_blends_each_channel() {
        assert_eq!(lerp_rgba([0, 0, 0, 0], [10, 20, 30, 40], 0.5), [5, 10, 15, 20]);
    }

    #[test]
    fn luminance_is_zero_for_black_and_one_for_white() {
        assert_eq!(luminance([0, 0, 0, 255]), 0.0);
        assert!((luminance([255, 255, 255, 255]) - 1.0).abs() < 1.0e-6);
    }
}
