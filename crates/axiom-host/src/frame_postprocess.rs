//! Backend-neutral **color grade** post-process for a frame: an exposure scale, a
//! contrast S-curve, and a saturation adjustment applied to a finished RGBA
//! framebuffer, expressed as neutral frame data and realized by a single pure
//! post-process.
//!
//! This lives in `host` — not in any one backend — for the same reason as
//! [`crate::FrameVolumetrics`] and [`crate::FrameAmbient`]: the engine's contract is
//! *neutral frame in, pixels out, through **any** renderer*. A [`FramePacket`] carries
//! an optional [`FramePostProcess`]; every backend (Canvas 2D software raster,
//! WebGPU/WebGL via wgpu, …) calls [`apply_frame_postprocess`] on its output, so the
//! graded look is identical no matter which renderer produced the frame.
//!
//! The grade is the standard LDR "filmic look" chain, in order, per pixel:
//! 1. **exposure** — scale each channel (below 1 darkens a washed-out frame);
//! 2. **contrast** — an S-curve around a mid pivot, `(v - 0.5) * contrast + 0.5`, which
//!    deepens shadows and separates the flat midtones a raster with strong ambient/fog
//!    tends to produce;
//! 3. **saturation** — push each channel away from the pixel's Rec.709 luma, enriching
//!    the palette (a neutral-grey pixel is unchanged).
//!
//! It is **not** an HDR tonemap: the input is an already-LDR sRGB framebuffer, so a
//! highlight-compressing curve would only lift the mids into a milky wash. Deterministic,
//! no feedback, no browser types.

use crate::frame_packet::FramePacket;

/// Tuning for the color-grade post-process, carried as neutral frame data: `exposure`
/// scales each channel, `contrast` is the S-curve strength around the 0.5 pivot
/// (`1.0` = unchanged, `>1` deepens), and `saturation` scales the distance of each
/// channel from the pixel's luma (`1.0` = unchanged, `>1` richer). Presence of a
/// `FramePostProcess` on a [`FramePacket`] *is* the enable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FramePostProcess {
    exposure: f32,
    contrast: f32,
    saturation: f32,
}

impl FramePostProcess {
    /// Assemble grade parameters. Crate-internal: the public constructor is
    /// [`FramePostProcess::cinematic`] (a preset), so no naked tuning scalar crosses the
    /// module facade.
    pub(crate) const fn new(exposure: f32, contrast: f32, saturation: f32) -> Self {
        FramePostProcess { exposure, contrast, saturation }
    }

    /// The public constructor: a tuned filmic preset that counters a washed-out,
    /// flat-midtone raster — a near-neutral exposure, gentle contrast to give the
    /// midtones punch without crushing shadows to black, and a saturation boost to
    /// enrich the palette. Presets keep the raw tuning scalars off the public surface.
    ///
    /// Retuned from the earlier heavy `(0.88, 1.32, 1.35)` grade: that combination
    /// dimmed the whole frame (0.88), crushed the warm crowd/backdrop into near-black
    /// (1.32 around the 0.5 pivot), and pushed the turf into a neon green (1.35) — the
    /// opposite of a bright, warm, punchy-not-crushed reference. Exposure is lifted
    /// toward neutral so the backdrop reads, contrast eased so shadows deepen without
    /// clipping to black, and saturation tamed so the vivid albedo stays vivid rather
    /// than radioactive.
    pub const fn cinematic() -> Self {
        FramePostProcess::new(0.96, 1.14, 1.18)
    }
}

/// Grade one RGBA8 pixel's R/G/B in place (alpha untouched): exposure → contrast
/// S-curve → saturation toward Rec.709 luma → clamp + re-quantize. Pure arithmetic.
fn grade_pixel(px: &mut [u8], pp: &FramePostProcess) {
    let lin = |b: u8| f32::from(b) / 255.0 * pp.exposure;
    let contrast = |v: f32| (v - 0.5) * pp.contrast + 0.5;
    let (r, g, b) = (contrast(lin(px[0])), contrast(lin(px[1])), contrast(lin(px[2])));
    let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let sat = |v: f32| luma + (v - luma) * pp.saturation;
    let quant = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    px[0] = quant(sat(r));
    px[1] = quant(sat(g));
    px[2] = quant(sat(b));
}

/// Apply the frame's color-grade post-process to a finished RGBA8 framebuffer, in place.
/// A no-op (returns `0`) when the packet carries no [`FramePostProcess`]. Otherwise every
/// pixel's R, G, B channels are graded (alpha untouched) and the pixel count
/// (`width * height`) is returned.
///
/// **Every backend calls this on its output**, so the graded look renders identically on
/// Canvas 2D, WebGPU, and WebGL — the effect is neutral frame data, not a
/// backend-specific feature.
pub fn apply_frame_postprocess(rgba: &mut [u8], width: u32, height: u32, packet: &FramePacket) -> u64 {
    packet
        .postprocess()
        .map(|pp| {
            rgba.chunks_exact_mut(4).for_each(|px| grade_pixel(px, pp));
            u64::from(width) * u64::from(height)
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_packet::{FrameFeatureSet, FramePacket, FrameViewport};

    /// A 2x2 packet, optionally carrying a post-process.
    fn packet(pp: Option<FramePostProcess>) -> FramePacket {
        let base = FramePacket::new(
            0,
            0,
            FrameViewport::new(2, 2),
            [0.0, 0.0, 0.0, 1.0],
            None,
            Vec::new(),
            Vec::new(),
            [0.0; 16],
            FrameFeatureSet::new(false, false, 0, 0),
        );
        match pp {
            Some(p) => base.with_postprocess(p),
            None => base,
        }
    }

    #[test]
    fn preset_new_debug_and_equality() {
        let c = FramePostProcess::cinematic();
        assert_eq!(c.exposure, 0.96);
        assert_eq!(c.contrast, 1.14);
        assert_eq!(c.saturation, 1.18);
        let n = FramePostProcess::new(0.5, 2.0, 0.25);
        assert_eq!(n.exposure, 0.5);
        assert_eq!(n.contrast, 2.0);
        assert_eq!(n.saturation, 0.25);
        assert_eq!(c, FramePostProcess::cinematic());
        assert_ne!(c, n);
        assert!(format!("{c:?}").contains("FramePostProcess"));
    }

    #[test]
    fn no_postprocess_is_a_no_op() {
        let mut rgba = vec![10u8, 20, 30, 255, 40, 50, 60, 128, 70, 80, 90, 200, 100, 110, 120, 64];
        let before = rgba.clone();
        assert_eq!(apply_frame_postprocess(&mut rgba, 2, 2, &packet(None)), 0);
        assert_eq!(rgba, before);
    }

    #[test]
    fn identity_grade_returns_count_and_preserves_pixel_and_alpha() {
        // exposure 1, contrast 1, saturation 1 → the grade is the identity map.
        let pp = FramePostProcess::new(1.0, 1.0, 1.0);
        let mut rgba = vec![80u8, 160, 240, 200, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0];
        let count = apply_frame_postprocess(&mut rgba, 2, 2, &packet(Some(pp)));
        assert_eq!(count, 4);
        // 80/255*255+0.5 rounds back to 80, etc.; alpha 200 untouched.
        assert_eq!(&rgba[0..4], &[80, 160, 240, 200]);
    }

    #[test]
    fn exposure_only_scales_and_clamps() {
        // contrast 1 + saturation 1 → grade reduces to the exposure scale.
        let pp = FramePostProcess::new(2.0, 1.0, 1.0);
        let mut rgba = vec![100u8, 200, 0, 77, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        apply_frame_postprocess(&mut rgba, 2, 2, &packet(Some(pp)));
        assert_eq!(rgba[0], 200); // 100/255*2 = 0.784 → 200
        assert_eq!(rgba[1], 255); // 200/255*2 = 1.57 → clamp 255
        assert_eq!(rgba[2], 0); // 0 stays 0
        assert_eq!(rgba[3], 77); // alpha untouched
    }

    #[test]
    fn contrast_deepens_darks_and_lifts_lights() {
        // contrast 2 around the 0.5 pivot: a dark channel collapses toward 0, a light
        // one saturates toward 1, a mid stays put (saturation 1 keeps channels).
        let pp = FramePostProcess::new(1.0, 2.0, 1.0);
        let mut rgba = vec![64u8, 192, 128, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        apply_frame_postprocess(&mut rgba, 2, 2, &packet(Some(pp)));
        assert_eq!(rgba[0], 1); // (0.251-0.5)*2+0.5 = 0.002 → 1
        assert_eq!(rgba[1], 255); // (0.753-0.5)*2+0.5 = 1.006 → clamp 255
        assert_eq!(rgba[2], 129); // (0.502-0.5)*2+0.5 = 0.504 → 129
    }

    #[test]
    fn saturation_pushes_channels_from_luma_but_leaves_grey() {
        // A warm pixel gets more saturated (R up, B toward 0); a neutral-grey pixel is
        // unchanged because every channel already equals the luma.
        let warm = FramePostProcess::new(1.0, 1.0, 2.0);
        let mut rgba = vec![200u8, 100, 50, 255, 128, 128, 128, 255, 0, 0, 0, 0, 0, 0, 0, 0];
        apply_frame_postprocess(&mut rgba, 2, 2, &packet(Some(warm)));
        assert_eq!(rgba[0], 255); // pushed above 1.0 → clamp
        assert_eq!(rgba[1], 82); // toward-luma distance doubled
        assert_eq!(rgba[2], 0); // pushed below 0 → clamp
        assert_eq!(&rgba[4..7], &[128, 128, 128]); // grey unchanged by saturation
    }
}
