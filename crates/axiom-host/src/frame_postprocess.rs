//! Backend-neutral **color grade** post-process for a frame: an exposure scale, a
//! per-channel white-balance gain, a contrast S-curve, and a saturation adjustment
//! applied to a finished RGBA framebuffer, expressed as neutral frame data and realized
//! by a single pure post-process.
//!
//! This lives in `host` — not in any one backend — for the same reason as
//! [`crate::FrameVolumetrics`] and [`crate::FrameAmbient`]: the engine's contract is
//! *neutral frame in, pixels out, through **any** renderer*. A [`FramePacket`] carries
//! an optional [`FramePostProcess`]; every backend (Canvas 2D software raster,
//! WebGPU/WebGL via wgpu, …) calls [`apply_frame_postprocess`] on its output, so the
//! graded look is identical no matter which renderer produced the frame.
//!
//! The grade is the standard LDR "filmic look" chain, in order, per pixel:
//! 0. **black point** — subtract the frame's black floor and renormalize,
//!    `max(v - black, 0) / (1 - black)`, so the darkest thing the raster produced lands on
//!    true black while white stays white. This is the term a **low-key** frame needs and
//!    the one the rest of the chain structurally cannot supply: exposure is a *multiply*
//!    (it scales a lifted floor, it never removes it), and the contrast S-curve pivots on
//!    `0.5`, so on a night frame whose every pixel sits below `0.2` any `contrast > 1`
//!    drives the whole image negative and clamps it to a black rectangle. A floor is a
//!    *subtract*; there was no subtract in the chain. `black = 0` is the exact identity,
//!    so every frame authored before this term existed is byte-identical;
//! 1. **exposure + white balance** — scale each channel by the global exposure and by its
//!    own per-channel white-balance gain (a `< 1` red / `> 1` blue gain cools a warm frame
//!    toward daylight; neutral `[1, 1, 1]` is the identity). White balance rides here, with
//!    exposure, because both are pre-tone linear scales — a temperature shift is simply an
//!    *uneven* exposure. This is the term that lets a warm-cast raster be pulled to daylight
//!    in **one** neutral post stage, instead of re-tinting every material's albedo (an
//!    app-tier shortcut that can't touch sky/net/keeper uniformly);
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
use axiom_kernel::Ratio;

/// Tuning for the color-grade post-process, carried as neutral frame data: `exposure`
/// scales every channel uniformly, `white_balance` scales each channel independently
/// (`[1.0, 1.0, 1.0]` = neutral; drop red / lift blue to cool toward daylight), `contrast`
/// is the S-curve strength around the 0.5 pivot (`1.0` = unchanged, `>1` deepens), and
/// `saturation` scales the distance of each channel from the pixel's luma (`1.0` =
/// unchanged, `>1` richer). `black_point` is the display-encoded floor subtracted and
/// renormalized away before any of them (`0.0` = unchanged). Presence of a
/// `FramePostProcess` on a [`FramePacket`] *is* the enable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FramePostProcess {
    exposure: f32,
    white_balance: [f32; 3],
    contrast: f32,
    saturation: f32,
    black_point: f32,
}

impl FramePostProcess {
    /// Assemble grade parameters. Crate-internal: the public constructors are
    /// [`FramePostProcess::cinematic`] and [`FramePostProcess::low_key`] (presets), so no
    /// naked tuning scalar crosses the module facade.
    pub(crate) const fn new(
        exposure: f32,
        white_balance: [f32; 3],
        contrast: f32,
        saturation: f32,
        black_point: f32,
    ) -> Self {
        FramePostProcess {
            exposure,
            white_balance,
            contrast,
            saturation,
            black_point,
        }
    }

    /// The global exposure scale.
    pub const fn exposure(&self) -> Ratio {
        Ratio::finite_or_zero(self.exposure)
    }

    /// The per-channel white-balance gain.
    pub const fn white_balance(&self) -> [f32; 3] {
        self.white_balance
    }

    /// The contrast S-curve strength around the `0.5` pivot.
    pub const fn contrast(&self) -> Ratio {
        Ratio::finite_or_zero(self.contrast)
    }

    /// The saturation scale about the pixel's Rec.709 luma.
    pub const fn saturation(&self) -> Ratio {
        Ratio::finite_or_zero(self.saturation)
    }

    /// The display-encoded black floor removed before the rest of the chain.
    pub const fn black_point(&self) -> Ratio {
        Ratio::finite_or_zero(self.black_point)
    }

    /// The public constructor: a tuned filmic preset that counters a washed-out,
    /// flat-midtone raster — a near-neutral exposure, a gentle cool daylight white balance
    /// (warm red eased down, blue lifted) so the warm-brown raster reads as sunlit daylight,
    /// gentle contrast to give the midtones punch without crushing shadows to black, and a
    /// saturation boost to enrich the palette. Presets keep the raw tuning scalars off the
    /// public surface.
    ///
    /// Retuned from the earlier heavy `(0.88, 1.32, 1.35)` grade: that combination dimmed
    /// the whole frame (0.88), crushed the warm crowd/backdrop into near-black (1.32 around
    /// the 0.5 pivot), and pushed the turf into a neon green (1.35) — the opposite of a
    /// bright, sunlit, punchy-not-crushed reference. Exposure is now lifted to neutral so
    /// the backdrop reads, a cool white balance shifts the whole frame off warm-brown toward
    /// the reference's daylight cast (a shift no per-channel exposure alone could make),
    /// contrast eased so shadows deepen without clipping to black, and saturation tamed so
    /// the vivid albedo stays vivid rather than radioactive.
    pub const fn cinematic() -> Self {
        FramePostProcess::new(1.02, [0.98, 1.0, 1.06], 1.10, 1.18, 0.0)
    }

    /// The **sunlit** preset: [`FramePostProcess::cinematic`]'s sibling for a frame whose
    /// light source is *in shot*, where `cinematic`'s two signature moves are backwards.
    ///
    /// `cinematic` is authored for a raster that arrives warm-brown and flat: it eases red
    /// down, lifts blue, and pushes saturation to `1.18` to put colour back. Both moves are
    /// corrections, and a raster that does **not** arrive warm-brown gets the correction
    /// anyway. A midday exterior is exactly that raster. Its sky and its haze are already
    /// the most saturated things in frame — a blue-green primary and a pale scatter — so the
    /// cool white balance takes red out of a frame that has almost none to spare, and the
    /// saturation lift then multiplies each channel's *distance from luma*, driving the
    /// deficient red further down while pushing the already-dominant blue up. The two
    /// compound: the frame ends up red-starved and over-saturated at once, which reads as
    /// a cold, electric cast rather than as sunlight.
    ///
    /// So this preset inverts both and leaves the rest alone:
    ///
    /// * **White balance `[1.04, 1.00, 1.05]`** — warm, not cool, but only just. Sunlight *is*
    ///   warm; the sky is blue precisely because the air took that red out of the beam, so the
    ///   red belongs in the light and the blue stays in the sky. Blue keeps a slight lift
    ///   because the shade on a clear day genuinely is sky-lit.
    ///
    ///   This eases red from `1.15`, and the reason is a **double count**, not a change of
    ///   mind about the direction. The clause above — "the red belongs in the light" — is the
    ///   preset's own argument for why the *key* should carry the warmth, and it was written
    ///   when the one frame on this preset still ran a near-white key of `(1.0, 0.955, 0.88)`.
    ///   That key has since been re-gelled, off a measured sunlit-minus-shaded inversion of
    ///   the reference road, to a golden `(1.0, 0.58, 0.27)`. The light now carries the warmth
    ///   the preset said it should — and this grade went on multiplying another 15% of red on
    ///   top of it. Two warm terms stacked, and only one of them was ever measured.
    ///
    ///   The stack is visible as clipping, which is the tell that separates a grade defect
    ///   from a lighting one. Sampling the frame's largest surface: the near road reads
    ///   `(255, 150, 80)` — red pinned at the ceiling while green sits at `150`. Its raw
    ///   raster value inverts to `195`, comfortably unclipped; the `1.08 x 1.15 = 1.242` red
    ///   gain is what drove it over. Across the road plane the graded ratio is `R/G = 1.50`,
    ///   against a reference road that measures a flat neutral `(88, 88, 88)`, `R/G = 1.00`.
    ///   At `1.04` the same pixels land `(229, 150, 85)` and `R/G = 1.36` — the near road
    ///   un-clips and recovers real surface detail, and a third of the excess warmth goes.
    ///
    ///   **Only red moves.** Blue stays at `1.05` rather than being lifted to meet it, because
    ///   the headroom is not there: [`FramePostProcess::sunlit`]'s one caller authors a sky
    ///   dome whose blue already grades to `0.967` pre-clamp, and the `1.10` that would have
    ///   balanced this correction from the other side takes it to `1.016` — clipping the dome
    ///   to a constant and flattening the very gradient the `1.08` exposure is held down to
    ///   protect. A correction that costs the sky its gradient is not a correction.
    ///
    ///   Green is the anchor and does not move either, which makes this **exposure-neutral by
    ///   construction** — the whole point, because the frame's level is the key's decision and
    ///   not this stage's. Rec. 709 luma on a neutral costs `0.2126 x -0.11 = -0.023`, i.e. a
    ///   mid-grey of `128` renders at `125`. Three levels is not a re-exposure. And the preset
    ///   stays a *warm* one — red still leads green, which is the invariant that keeps it
    ///   distinct from `cinematic` rather than collapsing into it.
    /// * **Saturation `1.02`** — essentially the identity. A daylight exterior's colour comes
    ///   from its albedo and its atmosphere, both authored; a global push away from luma only
    ///   exaggerates whichever channel already won.
    /// * **Exposure `1.08`** — a modest lift, held down deliberately. An exterior's sky sits
    ///   near the top of the range already, and the display target is 8-bit: a bigger lift
    ///   clips the dome's blue and flattens the gradient into a constant, which costs more
    ///   than the stop is worth.
    /// * **Contrast `1.10` and black point `0.0`** are `cinematic`'s, unchanged. Neither was
    ///   ever the defect — a sunlit frame's tonal range comes from its cast shadows, and
    ///   there is no lifted floor under a bright sky to subtract away.
    pub const fn sunlit() -> Self {
        FramePostProcess::new(1.08, [1.04, 1.0, 1.05], 1.10, 1.02, 0.0)
    }

    /// The **low-key** preset: a pure black-point lift-removal, everything else neutral.
    ///
    /// A night raster's problem is never its highlights — a moon, a headlamp and a lane
    /// marking all reach the top of the range on their own. Its problem is that the
    /// *floor* never reaches the bottom: a hemisphere ambient lights the lit and the unlit
    /// face equally, an atmospheric fog adds a constant to everything far away, and each
    /// one is a term that can be reduced but not driven to zero without also erasing the
    /// shadowed side of every object that needs to stay readable. The result is a frame
    /// whose whites are correct and whose blacks sit a tenth of the way up the range —
    /// which the eye reads as *grey daylight, dimmed*, not as night.
    ///
    /// Removing that floor is one subtract on the finished image, and it is the only
    /// operation in this chain that leaves the highlights where they are: `0.16` maps a
    /// raster floor of ≈`0.18` to ≈`0.03` and a highlight of `0.82` to `0.79`. Exposure,
    /// white balance, contrast and saturation are all held at their identity, because a
    /// low-key frame has no midtones to separate and a mid-pivot contrast would crush the
    /// image it is meant to deepen.
    pub const fn low_key() -> Self {
        FramePostProcess::new(1.0, [1.0, 1.0, 1.0], 1.0, 1.0, 0.16)
    }
}

/// Grade one RGBA8 pixel's R/G/B in place (alpha untouched): black-point floor removal →
/// (exposure × white-balance) → contrast S-curve → saturation toward Rec.709 luma → clamp +
/// re-quantize. Pure arithmetic.
///
/// The floor is removed **first**, on the display-encoded value, because that is the space
/// the lift lives in: the raster wrote a byte, and "no pixel is darker than 46/255" is a
/// statement about bytes. `(1 - black)` is floored so a degenerate `black_point` of `1.0`
/// cannot divide by zero; the trailing `max(0)` is what makes the subtract a floor removal
/// rather than a sign flip.
fn grade_pixel(px: &mut [u8], pp: &FramePostProcess) {
    let floored = |b: u8| {
        ((f32::from(b) / 255.0 - pp.black_point) / (1.0 - pp.black_point).max(1.0e-6)).max(0.0)
    };
    let lin = |b: u8, wb: f32| floored(b) * pp.exposure * wb;
    let contrast = |v: f32| (v - 0.5) * pp.contrast + 0.5;
    let (r, g, b) = (
        contrast(lin(px[0], pp.white_balance[0])),
        contrast(lin(px[1], pp.white_balance[1])),
        contrast(lin(px[2], pp.white_balance[2])),
    );
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
pub fn apply_frame_postprocess(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    packet: &FramePacket,
) -> u64 {
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
        assert_eq!(c.exposure, 1.02);
        assert_eq!(c.white_balance, [0.98, 1.0, 1.06]);
        assert_eq!(c.contrast, 1.10);
        assert_eq!(c.saturation, 1.18);
        let n = FramePostProcess::new(0.5, [0.3, 0.6, 0.9], 2.0, 0.25, 0.5);
        assert_eq!(n.exposure, 0.5);
        assert_eq!(n.white_balance, [0.3, 0.6, 0.9]);
        assert_eq!(n.contrast, 2.0);
        assert_eq!(n.saturation, 0.25);
        assert_eq!(c, FramePostProcess::cinematic());
        assert_ne!(c, n);
        assert!(format!("{c:?}").contains("FramePostProcess"));
    }

    /// The **sunlit** preset is `cinematic`'s inverse on the two knobs that decide
    /// whether a daylight raster reads as sunlight or as a cold electric cast, and the
    /// same as it on the two that were never the defect. Asserted as a *relationship*
    /// rather than as four literals, because that relationship is the whole reason the
    /// preset exists: the day either preset is re-tuned into agreeing with the other on
    /// warmth or on saturation, one of them is redundant and this fires.
    #[test]
    fn sunlit_warms_and_de_saturates_where_cinematic_cools_and_enriches() {
        let (s, c) = (FramePostProcess::sunlit(), FramePostProcess::cinematic());
        assert_eq!(s.exposure, 1.08);
        assert_eq!(s.white_balance, [1.04, 1.0, 1.05]);
        assert_eq!(s.contrast, 1.10);
        assert_eq!(s.saturation, 1.02);

        // Warm, where cinematic is cool: red gains on green, and it is cinematic's red
        // that loses. A sunlit frame's warmth cannot come from exposure — that scales
        // every channel — so it has to live in this ratio or nowhere.
        assert!(s.white_balance[0] > s.white_balance[1], "sunlit is warm");
        assert!(c.white_balance[0] < c.white_balance[1], "cinematic is cool");

        // Near-identity saturation, where cinematic pushes hard. A push away from luma
        // exaggerates whichever channel already dominates, which is the wrong operation
        // on a frame whose sky is already the most saturated thing in it.
        assert!((s.saturation - 1.0).abs() < 0.1, "sunlit trusts the authored albedo");
        assert!(c.saturation > s.saturation, "cinematic enriches, sunlit does not");

        // ...and the two knobs that were never the defect are shared exactly, so a frame
        // can move between the presets without its tonal range or its floor moving.
        assert_eq!(s.contrast, c.contrast);
        assert_eq!(s.black_point, c.black_point);
        assert_ne!(s, c);
    }

    #[test]
    fn no_postprocess_is_a_no_op() {
        let mut rgba = vec![
            10u8, 20, 30, 255, 40, 50, 60, 128, 70, 80, 90, 200, 100, 110, 120, 64,
        ];
        let before = rgba.clone();
        assert_eq!(apply_frame_postprocess(&mut rgba, 2, 2, &packet(None)), 0);
        assert_eq!(rgba, before);
    }

    #[test]
    fn identity_grade_returns_count_and_preserves_pixel_and_alpha() {
        // exposure 1, neutral white balance, contrast 1, saturation 1 → the grade is the
        // identity map.
        let pp = FramePostProcess::new(1.0, [1.0, 1.0, 1.0], 1.0, 1.0, 0.0);
        let mut rgba = vec![80u8, 160, 240, 200, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0];
        let count = apply_frame_postprocess(&mut rgba, 2, 2, &packet(Some(pp)));
        assert_eq!(count, 4);
        // 80/255*255+0.5 rounds back to 80, etc.; alpha 200 untouched.
        assert_eq!(&rgba[0..4], &[80, 160, 240, 200]);
    }

    #[test]
    fn exposure_only_scales_and_clamps() {
        // neutral white balance + contrast 1 + saturation 1 → grade reduces to the exposure
        // scale.
        let pp = FramePostProcess::new(2.0, [1.0, 1.0, 1.0], 1.0, 1.0, 0.0);
        let mut rgba = vec![100u8, 200, 0, 77, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        apply_frame_postprocess(&mut rgba, 2, 2, &packet(Some(pp)));
        assert_eq!(rgba[0], 200); // 100/255*2 = 0.784 → 200
        assert_eq!(rgba[1], 255); // 200/255*2 = 1.57 → clamp 255
        assert_eq!(rgba[2], 0); // 0 stays 0
        assert_eq!(rgba[3], 77); // alpha untouched
    }

    #[test]
    fn white_balance_tints_each_channel_independently() {
        // exposure 1 + contrast 1 + saturation 1 → the grade reduces to the per-channel
        // white-balance gain: a mid-grey pixel splits by channel (red boosted, green held,
        // blue halved), which uniform exposure alone could never do.
        let pp = FramePostProcess::new(1.0, [2.0, 1.0, 0.5], 1.0, 1.0, 0.0);
        let mut rgba = vec![128u8, 128, 128, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        apply_frame_postprocess(&mut rgba, 2, 2, &packet(Some(pp)));
        assert_eq!(rgba[0], 255); // 128/255*2 = 1.004 → clamp 255
        assert_eq!(rgba[1], 128); // 128/255*1 = 0.502 → 128
        assert_eq!(rgba[2], 64); // 128/255*0.5 = 0.251 → 64
        assert_eq!(rgba[3], 255); // alpha untouched
    }

    #[test]
    fn contrast_deepens_darks_and_lifts_lights() {
        // contrast 2 around the 0.5 pivot: a dark channel collapses toward 0, a light
        // one saturates toward 1, a mid stays put (neutral WB + saturation 1 keep channels).
        let pp = FramePostProcess::new(1.0, [1.0, 1.0, 1.0], 2.0, 1.0, 0.0);
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
        let warm = FramePostProcess::new(1.0, [1.0, 1.0, 1.0], 1.0, 2.0, 0.0);
        let mut rgba = vec![
            200u8, 100, 50, 255, 128, 128, 128, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        apply_frame_postprocess(&mut rgba, 2, 2, &packet(Some(warm)));
        assert_eq!(rgba[0], 255); // pushed above 1.0 → clamp
        assert_eq!(rgba[1], 82); // toward-luma distance doubled
        assert_eq!(rgba[2], 0); // pushed below 0 → clamp
        assert_eq!(&rgba[4..7], &[128, 128, 128]); // grey unchanged by saturation
    }

    /// Every field is readable across the crate boundary, because a GPU backend packing
    /// the grade into a uniform cannot see the private fields the CPU path reads directly.
    #[test]
    fn every_grade_parameter_is_readable() {
        let pp = FramePostProcess::new(0.5, [0.3, 0.6, 0.9], 2.0, 0.25, 0.5);
        assert_eq!(pp.exposure().get(), 0.5);
        assert_eq!(pp.white_balance(), [0.3, 0.6, 0.9]);
        assert_eq!(pp.contrast().get(), 2.0);
        assert_eq!(pp.saturation().get(), 0.25);
        assert_eq!(pp.black_point().get(), 0.5);
    }

    /// A zero black point must be the *exact* identity, or every frame graded before the
    /// term existed changes the day it lands.
    #[test]
    fn a_zero_black_point_leaves_every_pixel_where_it_was() {
        let none = FramePostProcess::new(1.0, [1.0, 1.0, 1.0], 1.0, 1.0, 0.0);
        let mut rgba = vec![0u8, 1, 46, 255, 128, 200, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0];
        apply_frame_postprocess(&mut rgba, 2, 2, &packet(Some(none)));
        assert_eq!(&rgba[0..3], &[0, 1, 46]);
        assert_eq!(&rgba[4..7], &[128, 200, 255]);
        assert_eq!(FramePostProcess::cinematic().black_point().get(), 0.0);
    }

    /// The floor removal: everything at or under the black point lands on true black, and
    /// white stays white — the property that separates it from an exposure cut.
    #[test]
    fn the_black_point_lands_the_floor_on_black_and_leaves_white_alone() {
        // black 0.5: 128/255 = 0.502 → 0.004; 64 is under the floor → 0; 255 → 255.
        let pp = FramePostProcess::new(1.0, [1.0, 1.0, 1.0], 1.0, 1.0, 0.5);
        let mut rgba = vec![128u8, 64, 255, 200, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        apply_frame_postprocess(&mut rgba, 2, 2, &packet(Some(pp)));
        assert_eq!(rgba[0], 1); // just above the floor: nearly black, not clipped away
        assert_eq!(rgba[1], 0); // below the floor: true black
        assert_eq!(rgba[2], 255); // white is untouched — this is not an exposure cut
        assert_eq!(rgba[3], 200); // alpha untouched
    }

    /// A degenerate `black_point` of `1.0` (every pixel at or under the floor) must yield a
    /// black frame, not a division by zero.
    #[test]
    fn a_full_black_point_is_finite_and_yields_black() {
        let pp = FramePostProcess::new(1.0, [1.0, 1.0, 1.0], 1.0, 1.0, 1.0);
        let mut rgba = vec![255u8, 128, 1, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        apply_frame_postprocess(&mut rgba, 2, 2, &packet(Some(pp)));
        assert_eq!(&rgba[0..3], &[0, 0, 0]);
    }

    /// The low-key preset is a pure floor removal: identity everywhere else, and the
    /// measured night-raster floor (≈0.18) really does land near black while a highlight
    /// (≈0.82) barely moves.
    #[test]
    fn low_key_removes_the_floor_and_holds_the_highlights() {
        let lk = FramePostProcess::low_key();
        assert_eq!(lk.exposure().get(), 1.0);
        assert_eq!(lk.white_balance(), [1.0, 1.0, 1.0]);
        assert_eq!(lk.contrast().get(), 1.0);
        assert_eq!(lk.saturation().get(), 1.0);
        assert_eq!(lk.black_point().get(), 0.16);
        assert_ne!(lk, FramePostProcess::cinematic());

        let mut rgba = vec![47u8, 26, 209, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        apply_frame_postprocess(&mut rgba, 2, 2, &packet(Some(lk)));
        assert!(rgba[0] < 10, "the raster floor collapses to near-black");
        assert_eq!(rgba[1], 0, "below the floor is true black");
        assert!(rgba[2] > 190, "the highlight is essentially where it was");
    }
}
