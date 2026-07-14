//! Backend-neutral **retro 32-bit render profile** for a frame: the parameters that give a
//! frame the retro 32-bit console look — low internal resolution + nearest upscale,
//! vertex snapping, flat (unlit) passthrough, distance fog, and reduced colour
//! depth with ordered (Bayer) dithering.
//!
//! Like [`crate::FramePostProcess`] / [`crate::FrameAmbient`], this is neutral
//! frame data: a [`FramePacket`] carries an optional [`FrameRetro32BitProfile`], and
//! *presence is the enable*. Unlike a pure colour grade, the retro 32-bit look is a mix of
//! effect natures, so the profile is realized in two ways from the SAME numbers:
//!
//! * **Post effects** (colour-depth quantize + ordered dither) — the pure
//!   whole-frame pass [`apply_frame_retro_32bit`], which every CPU-readback backend
//!   (Canvas 2D software raster, the native offscreen GPU readback) runs on its
//!   finished RGBA so they match byte-for-byte. The GPU *live* path (no readback)
//!   applies the same math in a WGSL post pass keyed on the same params.
//! * **Geometry / target effects** (vertex snap, internal resolution + nearest
//!   filter) and **fog** — read as parameter *values* by each backend's shader /
//!   projection / render-target setup (fog is in-shader on GPU, in-raster on
//!   Canvas 2D). Hence, unlike the post-only payloads, this profile exposes its
//!   fields through getters the backends consume.
//!
//! Normalized scalars are [`Ratio`] (no naked `f32` on the public surface); flags
//! are `u32` (0/1) so they arithmetic-select in branchless spine code and pack
//! directly into a uniform.

use axiom_kernel::Ratio;

use crate::frame_packet::FramePacket;

/// The neutral retro 32-bit render parameters. Presence of a `FrameRetro32BitProfile` on a
/// [`FramePacket`] *is* the enable; the individual fields tune each effect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameRetro32BitProfile {
    fog_near: Ratio,
    fog_far: Ratio,
    fog_strength: Ratio,
    fog_color: [f32; 3],
    color_levels: [u32; 3],
    dither_strength: Ratio,
    dither_tier: u32,
    snap: u32,
    unlit: u32,
    internal_width: u32,
    internal_height: u32,
    nearest_filter: u32,
}

impl FrameRetro32BitProfile {
    /// Assemble a retro 32-bit profile from its parts. Public (unlike
    /// [`FramePostProcess::new`](crate::FramePostProcess)) because the app derives
    /// a profile from its own style descriptor — same reason
    /// [`FrameAmbient::new`](crate::FrameAmbient) is public. `fog_*` are normalized
    /// [0,1] depths; `color_levels` is levels/channel (32 = 5-bit); `dither_tier`
    /// selects the Bayer matrix (0→2×2, 1→4×4, 2→8×8); `snap` / `unlit` /
    /// `nearest_filter` are 0/1 flags; `internal_*` is the render resolution
    /// (0 = native).
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        fog_near: Ratio,
        fog_far: Ratio,
        fog_strength: Ratio,
        fog_color: [f32; 3],
        color_levels: [u32; 3],
        dither_strength: Ratio,
        dither_tier: u32,
        snap: u32,
        unlit: u32,
        internal_width: u32,
        internal_height: u32,
        nearest_filter: u32,
    ) -> Self {
        FrameRetro32BitProfile {
            fog_near,
            fog_far,
            fog_strength,
            fog_color,
            color_levels,
            dither_strength,
            dither_tier,
            snap,
            unlit,
            internal_width,
            internal_height,
            nearest_filter,
        }
    }

    /// The full-on retro 32-bit console preset: 384×240 (8:5) internal resolution with a
    /// nearest upscale, vertex snapping and flat unlit passthrough on, aggressive
    /// ~12-level colour depth with a clearly-visible 4×4 Bayer dither, and a
    /// moderate dark distance fog.
    pub const fn retro_32bit() -> Self {
        FrameRetro32BitProfile::new(
            Ratio::finite_or_zero(0.85),
            Ratio::finite_or_zero(1.0),
            Ratio::finite_or_zero(0.35),
            [0.05, 0.06, 0.10],
            [12, 12, 12],
            Ratio::finite_or_zero(1.0),
            1,
            1,
            1,
            384,
            240,
            1,
        )
    }

    /// A crisper "arcade" preset: higher internal resolution, 6-bit colour with a
    /// gentle 2×2 dither and lighter fog — retro 32-bit-flavoured but cleaner.
    pub const fn arcade() -> Self {
        FrameRetro32BitProfile::new(
            Ratio::finite_or_zero(0.88),
            Ratio::finite_or_zero(1.0),
            Ratio::finite_or_zero(0.2),
            [0.06, 0.07, 0.11],
            [64, 64, 64],
            Ratio::finite_or_zero(0.5),
            0,
            1,
            1,
            480,
            300,
            1,
        )
    }

    /// Fog start depth (normalized 0..1).
    pub const fn fog_near(&self) -> Ratio {
        self.fog_near
    }
    /// Fog full-density depth (normalized 0..1).
    pub const fn fog_far(&self) -> Ratio {
        self.fog_far
    }
    /// Maximum fog density (0..1).
    pub const fn fog_strength(&self) -> Ratio {
        self.fog_strength
    }
    /// Linear-RGB fog colour.
    pub const fn fog_color(&self) -> [f32; 3] {
        self.fog_color
    }
    /// Quantization levels per R/G/B channel.
    pub const fn color_levels(&self) -> [u32; 3] {
        self.color_levels
    }
    /// Ordered-dither strength (0..1, in units of one quantization step).
    pub const fn dither_strength(&self) -> Ratio {
        self.dither_strength
    }
    /// Bayer matrix tier: 0→2×2, 1→4×4, 2→8×8.
    pub const fn dither_tier(&self) -> u32 {
        self.dither_tier
    }
    /// Whether vertex snapping is on (0/1).
    pub const fn snap(&self) -> u32 {
        self.snap
    }
    /// Whether flat unlit passthrough is on (0/1).
    pub const fn unlit(&self) -> u32 {
        self.unlit
    }
    /// Internal render width (0 = native).
    pub const fn internal_width(&self) -> u32 {
        self.internal_width
    }
    /// Internal render height (0 = native).
    pub const fn internal_height(&self) -> u32 {
        self.internal_height
    }
    /// Whether the internal→output upscale uses nearest filtering (0/1).
    pub const fn nearest_filter(&self) -> u32 {
        self.nearest_filter
    }
}

// --- ordered-dither (Bayer) tables --------------------------------------------
// These MUST stay byte-identical to the WGSL `retro_32bit_post` matrices so the GPU-live
// quantize matches the CPU `apply_frame_retro_32bit` used by canvas2d + offscreen.

#[rustfmt::skip]
const BAYER2: [u32; 4] = [
    0, 2,
    3, 1,
];

#[rustfmt::skip]
const BAYER4: [u32; 16] = [
     0,  8,  2, 10,
    12,  4, 14,  6,
     3, 11,  1,  9,
    15,  7, 13,  5,
];

#[rustfmt::skip]
const BAYER8: [u32; 64] = [
     0, 32,  8, 40,  2, 34, 10, 42,
    48, 16, 56, 24, 50, 18, 58, 26,
    12, 44,  4, 36, 14, 46,  6, 38,
    60, 28, 52, 20, 62, 30, 54, 22,
     3, 35, 11, 43,  1, 33,  9, 41,
    51, 19, 59, 27, 49, 17, 57, 25,
    15, 47,  7, 39, 13, 45,  5, 37,
    63, 31, 55, 23, 61, 29, 53, 21,
];

const BAYER_TABLES: [&[u32]; 3] = [&BAYER2, &BAYER4, &BAYER8];
const BAYER_DIM: [u32; 3] = [2, 4, 8];

/// The centered ordered-dither offset in `[-0.5, 0.5)` for pixel `(x, y)` under
/// the given `tier`'s Bayer matrix. Pure table index + arithmetic — no branch.
fn bayer_threshold(x: u32, y: u32, tier: u32) -> f32 {
    let n = BAYER_DIM[tier as usize];
    let cell = BAYER_TABLES[tier as usize][((y % n) * n + (x % n)) as usize];
    (cell as f32 + 0.5) / (n * n) as f32 - 0.5
}

/// Quantize one 0..1 channel to `levels` bands with an ordered-dither offset.
/// Round-to-nearest via `+0.5` truncation; `levels` floored to 2 so the step is
/// never zero. Branchless (`.max`, `.clamp`, arithmetic).
fn quantize_channel(v: f32, levels: u32, threshold: f32, strength: f32) -> f32 {
    let steps = (levels.max(2) - 1) as f32;
    let dithered = (v + threshold * strength / steps).clamp(0.0, 1.0);
    (((dithered * steps) + 0.5) as u32 as f32) / steps
}

/// Quantize + dither one RGBA8 pixel's R/G/B in place (alpha untouched).
fn retro_32bit_pixel(px: &mut [u8], profile: &FrameRetro32BitProfile, threshold: f32) {
    let lin = |b: u8| f32::from(b) / 255.0;
    let levels = profile.color_levels;
    let s = profile.dither_strength.get();
    let quant = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    px[0] = quant(quantize_channel(lin(px[0]), levels[0], threshold, s));
    px[1] = quant(quantize_channel(lin(px[1]), levels[1], threshold, s));
    px[2] = quant(quantize_channel(lin(px[2]), levels[2], threshold, s));
}

/// Apply the frame's retro 32-bit colour-depth quantization + ordered dithering to a
/// finished RGBA8 framebuffer, in place. A no-op (returns `0`) when the packet
/// carries no [`FrameRetro32BitProfile`]; otherwise every pixel's R,G,B is quantized
/// (alpha untouched) and the pixel count (`width * height`) is returned.
///
/// Fog, vertex snap, and the low-resolution target are **not** done here — they
/// are geometry/target-stage effects the backends realize from the profile's
/// fields. This is the whole-frame post shared by the CPU-readback backends
/// (Canvas 2D, offscreen GPU); the GPU-live path mirrors it in WGSL.
pub fn apply_frame_retro_32bit(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    packet: &FramePacket,
) -> u64 {
    packet
        .retro_32bit()
        .map(|profile| {
            rgba.chunks_exact_mut(4).enumerate().for_each(|(i, px)| {
                let x = (i as u32) % width.max(1);
                let y = (i as u32) / width.max(1);
                retro_32bit_pixel(px, profile, bayer_threshold(x, y, profile.dither_tier));
            });
            u64::from(width) * u64::from(height)
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_packet::{FrameFeatureSet, FramePacket, FrameViewport};

    fn ratio(v: f32) -> Ratio {
        Ratio::finite_or_zero(v)
    }

    /// A `w*h` packet, optionally carrying a retro 32-bit profile.
    fn packet(w: u32, h: u32, retro_32bit: Option<FrameRetro32BitProfile>) -> FramePacket {
        let base = FramePacket::new(
            0,
            0,
            FrameViewport::new(w, h),
            [0.0, 0.0, 0.0, 1.0],
            None,
            Vec::new(),
            Vec::new(),
            [0.0; 16],
            FrameFeatureSet::new(false, false, 0, 0),
        );
        match retro_32bit {
            Some(p) => base.with_retro_32bit_profile(p),
            None => base,
        }
    }

    #[test]
    fn presets_getters_debug_and_equality() {
        let p = FrameRetro32BitProfile::retro_32bit();
        assert_eq!(p.fog_near().get(), 0.85);
        assert_eq!(p.fog_far().get(), 1.0);
        assert_eq!(p.fog_strength().get(), 0.35);
        assert_eq!(p.fog_color(), [0.05, 0.06, 0.10]);
        assert_eq!(p.color_levels(), [12, 12, 12]);
        assert_eq!(p.dither_strength().get(), 1.0);
        assert_eq!(p.dither_tier(), 1);
        assert_eq!(p.snap(), 1);
        assert_eq!(p.unlit(), 1);
        assert_eq!(p.internal_width(), 384);
        assert_eq!(p.internal_height(), 240);
        assert_eq!(p.nearest_filter(), 1);
        let a = FrameRetro32BitProfile::arcade();
        assert_eq!(a.color_levels(), [64, 64, 64]);
        assert_eq!(a.dither_tier(), 0);
        assert_eq!(a.dither_strength().get(), 0.5);
        assert_eq!(a.internal_width(), 480);
        assert_eq!(a.internal_height(), 300);
        assert_eq!(p, FrameRetro32BitProfile::retro_32bit());
        assert_ne!(p, a);
        assert!(format!("{p:?}").contains("FrameRetro32BitProfile"));
    }

    #[test]
    fn no_profile_is_a_no_op() {
        let mut rgba = vec![
            10u8, 20, 30, 255, 40, 50, 60, 128, 70, 80, 90, 200, 100, 110, 120, 64,
        ];
        let before = rgba.clone();
        assert_eq!(
            apply_frame_retro_32bit(&mut rgba, 2, 2, &packet(2, 2, None)),
            0
        );
        assert_eq!(rgba, before);
    }

    #[test]
    fn full_color_no_dither_is_near_identity_and_counts() {
        // 256 levels, zero dither → quantize maps each byte back to itself.
        let p = FrameRetro32BitProfile::new(
            ratio(0.0),
            ratio(1.0),
            ratio(0.0),
            [0.0; 3],
            [256, 256, 256],
            ratio(0.0),
            1,
            0,
            0,
            0,
            0,
            0,
        );
        let mut rgba = vec![
            80u8, 160, 240, 200, 0, 0, 0, 255, 255, 255, 255, 0, 17, 33, 199, 5,
        ];
        let count = apply_frame_retro_32bit(&mut rgba, 2, 2, &packet(2, 2, Some(p)));
        assert_eq!(count, 4);
        assert_eq!(&rgba[0..4], &[80, 160, 240, 200]); // preserved, alpha untouched
        assert_eq!(&rgba[8..12], &[255, 255, 255, 0]);
    }

    #[test]
    fn two_level_quantize_snaps_to_black_or_white() {
        // 2 levels, no dither: below mid → 0, above mid → 255.
        let p = FrameRetro32BitProfile::new(
            ratio(0.0),
            ratio(1.0),
            ratio(0.0),
            [0.0; 3],
            [2, 2, 2],
            ratio(0.0),
            0,
            0,
            0,
            0,
            0,
            0,
        );
        let mut rgba = vec![
            100u8, 200, 128, 255, 10, 250, 130, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        apply_frame_retro_32bit(&mut rgba, 2, 2, &packet(2, 2, Some(p)));
        assert_eq!(rgba[0], 0); // 100/255=0.39 → 0
        assert_eq!(rgba[1], 255); // 200/255=0.78 → 255
        assert_eq!(rgba[3], 255); // alpha untouched
    }

    #[test]
    fn dither_offsets_a_midtone_by_bayer_cell() {
        // A flat mid-grey through a 2-level quantize + full dither: the ordered
        // matrix pushes some pixels to white and others to black (not all equal).
        let p = FrameRetro32BitProfile::new(
            ratio(0.0),
            ratio(1.0),
            ratio(0.0),
            [0.0; 3],
            [2, 2, 2],
            ratio(1.0),
            0,
            0,
            0,
            0,
            0,
            0,
        );
        let mut rgba = vec![128u8; 16]; // 2x2 mid-grey
        apply_frame_retro_32bit(&mut rgba, 2, 2, &packet(2, 2, Some(p)));
        let luminance: Vec<u8> = rgba.chunks_exact(4).map(|px| px[0]).collect();
        assert!(luminance.iter().any(|&v| v == 0));
        assert!(luminance.iter().any(|&v| v == 255));
    }

    #[test]
    fn bayer_threshold_covers_all_tiers_and_is_centered() {
        // Each tier indexes its matrix; the cell(0,0)=0 maps to the most-negative
        // offset, and offsets stay within [-0.5, 0.5).
        [0u32, 1, 2].iter().for_each(|&tier| {
            let t = bayer_threshold(0, 0, tier);
            assert!(t >= -0.5 && t < 0.0);
            let big = bayer_threshold(1, 0, tier);
            assert!((-0.5..0.5).contains(&big));
        });
    }
}
