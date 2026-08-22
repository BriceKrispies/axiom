//! **`this.settings`** — the frame graph's live tuning block, transcribed with
//! the reasoning that set each number.
//!
//! Forty values, none of them arbitrary: the source carries a paragraph of
//! measurement behind most of them, and several were changed *away* from the
//! obvious value for a stated reason. The ones worth reading before touching
//! anything:
//!
//! - **`bloom_threshold = 1.6`, not 0.85.** A daylight sky lands around 1.0-1.5
//!   in exposure-scaled linear light, so at 0.85 the *sky* was the brightest
//!   thing in the pyramid and the widest mip smeared it four or five pixels over
//!   every roofline: an enemy on a balcony measured 3% contrast against the
//!   cloud behind him.
//! - **`ao_intensity = 1.1`, down from 1.7.** Occlusion is a shaping tool, not a
//!   darkening tool; 1.7 with no bounce fill behind it turned every corner into
//!   a black hole.
//! - **`sky_fill 0.32` / `ground_fill 0.013` / `bounce_fill 0.008` /
//!   `ibl_diffuse 0.030`** are a *budget*, and the budget is what sets the
//!   key:fill ratio. Real direct sun runs 5-8:1 against its own shade; at the
//!   previous values the indirect terms supplied 42% of every lit value and the
//!   ratio collapsed to 2.4:1 — "a frame with no sun in it, only a bright
//!   ambient".
//! - **`practical_gain = 0.55`.** Twenty interior bulbs and twenty-two sodium
//!   lamps are the only light in a closed room and the loudest thing in a night
//!   street. At unity an interior metered within 1.3 stops of the sunlit facade
//!   framed in its own doorway; a real one runs 4-5.
//! - **`chromatic = 0.0011`.** At 0.0018 the R/B split reached most of a pixel
//!   in the corners, which the sharpen filter then turned into visible fringing.
//! - **`dof_max_coc = 3.3`, down 40% from 5.5**, at which "the near and mid
//!   ground of an ADS frame was a watercolour smear that hid the very thing the
//!   sights are pointed at".
//!
//! # Where each one is consumed, and in what colour space
//!
//! The source's note, which decides several of the magnitudes: chromatic
//! aberration, bloom and the `cos^4` lens vignette are **linear-light** lens
//! effects and happen *before* the tone map — a vignette applied to code values
//! is a flat multiply that makes display white unreachable anywhere but the
//! centre of the frame. The composite then tone-maps, encodes to sRGB, and
//! applies the grade LUT, grain and dither in **display** space. So
//! [`FrameSettings::vignette`] and [`FrameSettings::grain`] are code-value
//! amplitudes and [`FrameSettings::chromatic`] is not.
//!
//! # Cross-checks
//!
//! Four of these numbers, plus the metering's EV window, already exist
//! elsewhere in this crate, ported by other slices from the same source. They are asserted equal here rather than
//! re-derived, so a future edit to either copy fails a test instead of quietly
//! forking the frame's look: see
//! `tests::the_settings_agree_with_the_passes_that_already_ported_them`.

/// `this.settings`, field for field and in source order.
///
/// `f64` throughout: these are JavaScript numbers that reach a uniform through
/// one narrowing, and several are multiplied by a scene-derived quantity before
/// they get there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FrameSettings {
    /// EV; positive is darker. Added to the sky's own published compensation.
    pub(crate) exposure_bias: f64,
    /// The metering key.
    pub(crate) exposure_key: f64,
    /// `false` snaps the exposure rather than freezing it — see
    /// [`super::frame_inputs::metering_dt`].
    pub(crate) auto_exposure: bool,
    /// Gain on the finished, thresholded pyramid. **Added**, not mixed.
    pub(crate) bloom_strength: f64,
    /// Soft-knee threshold in exposure-scaled linear light.
    pub(crate) bloom_threshold: f64,
    /// Knee width.
    pub(crate) bloom_knee: f64,
    /// Lateral chromatic aberration, in linear light.
    pub(crate) chromatic: f64,
    /// Vignette amplitude, in **display** code values.
    pub(crate) vignette: f64,
    /// The vignette the frame closes to while the sights are up.
    pub(crate) ads_vignette: f64,
    /// Grain amplitude, in display code values.
    pub(crate) grain: f64,
    /// Maximum circle of confusion, in pixels **at 1080p**.
    pub(crate) dof_max_coc: f64,
    /// Fraction of the CoC budget the near field gets.
    pub(crate) dof_near_ratio: f64,
    /// Nearest focus distance, metres.
    pub(crate) dof_focus_min: f64,
    /// Farthest focus distance, metres.
    pub(crate) dof_focus_max: f64,
    /// Where the far defocus ramp begins, as a multiple of the focus distance.
    pub(crate) dof_far_start: f64,
    /// The far ramp's length, metres.
    pub(crate) dof_far_range: f64,
    /// Near-field CoC scale.
    pub(crate) dof_near_scale: f64,
    /// Composite sharpen, applied only when TAA is on.
    pub(crate) sharpen: f64,
    /// Grade LUT blend.
    pub(crate) lut_strength: f64,
    /// Shutter angle, as a fraction of the frame interval.
    pub(crate) shutter: f64,
    /// GTAO world radius, metres.
    pub(crate) ao_radius: f64,
    /// GTAO strength.
    pub(crate) ao_intensity: f64,
    /// Contact-shadow ray length in metres at 1x distance scaling. It has to
    /// resolve the 0-40 cm range, because that is the gap a cascade texel
    /// cannot see and the difference between a crate sitting on the floor and a
    /// crate stickered onto it.
    pub(crate) contact_length: f64,
    /// How much of the sun term a full contact hit removes.
    pub(crate) contact_strength: f64,
    /// The cool skylight band, as a fraction of the sky's own published
    /// irradiance. The frame's only strongly chromatic indirect term.
    pub(crate) sky_fill: f64,
    /// The warm band bounced off the street, as a fraction of the key.
    pub(crate) ground_fill: f64,
    /// The wrap term — the shaded side of the street lit by the sunlit side.
    pub(crate) bounce_fill: f64,
    /// Diffuse scale on the PMREM sky cubemap: the only place the total
    /// indirect budget can be controlled from. Specular radiance is left alone,
    /// because that is reflection rather than fill.
    pub(crate) ibl_diffuse: f64,
    /// Indirect floor inside a coarse interior volume, so a doorway does not
    /// read as a hole cut in a card.
    pub(crate) interior_indirect: f64,
    /// Global trim on room and street practicals — lights registered at or
    /// below [`super::lighting::PRACTICAL_RANGE`].
    pub(crate) practical_gain: f64,
    /// How much sky the viewmodel can see past the shooter's own body.
    pub(crate) view_fill_occlusion: f64,
    /// Viewmodel key, as a fraction of the shaped scene level.
    pub(crate) view_key_scale: f64,
    /// Ceiling on the viewmodel key. **Dead in the source**: the shaping
    /// clamps its own ratio at one, so the key is bounded by
    /// `REF_DAYLIGHT * view_key_scale = 2.53` and can never reach 2.6. See
    /// [`super::lighting::view_rig`].
    pub(crate) view_key_max: f64,
    /// Viewmodel fill, as a ratio of its key.
    pub(crate) view_fill_ratio: f64,
    /// Viewmodel rim, as a ratio of its key.
    pub(crate) view_rim_ratio: f64,
    /// Viewmodel hemisphere, as a ratio of its key, so it follows the time of
    /// day instead of blowing the gun out at night.
    pub(crate) view_hemi_ratio: f64,
    /// Warm ground bounce from below, ~1.5 stops under the key — about what a
    /// sand street returns, and what lifts the support glove out of the
    /// handguard's own cast shadow.
    pub(crate) view_bounce_ratio: f64,
    /// Sub-linear shaping exponent on the viewmodel key. A no-op in full
    /// daylight; the bias that keeps the weapon legible at night.
    pub(crate) view_key_gamma: f64,
    /// Cascade shadow strength.
    pub(crate) shadow_strength: f64,
    /// `tan` of the sun's angular radius, for the PCSS penumbra estimate.
    pub(crate) sun_softness: f64,
}

/// `this.settings`, exactly as `init()` writes it.
pub(crate) const SOURCE_SETTINGS: FrameSettings = FrameSettings {
    exposure_bias: 0.0,
    exposure_key: 1.06,
    auto_exposure: true,
    bloom_strength: 0.14,
    bloom_threshold: 1.6,
    bloom_knee: 0.9,
    chromatic: 0.0011,
    vignette: 0.24,
    ads_vignette: 0.34,
    grain: 0.010,
    dof_max_coc: 3.3,
    dof_near_ratio: 0.38,
    dof_focus_min: 3.0,
    dof_focus_max: 18.0,
    dof_far_start: 1.15,
    dof_far_range: 18.0,
    dof_near_scale: 0.55,
    sharpen: 0.25,
    lut_strength: 1.0,
    shutter: 0.42,
    ao_radius: 1.35,
    ao_intensity: 1.1,
    contact_length: 0.4,
    contact_strength: 1.0,
    sky_fill: 0.32,
    ground_fill: 0.013,
    bounce_fill: 0.008,
    ibl_diffuse: 0.030,
    interior_indirect: 0.035,
    practical_gain: 0.55,
    view_fill_occlusion: 0.45,
    view_key_scale: 0.55,
    view_key_max: 2.6,
    view_fill_ratio: 0.3,
    view_rim_ratio: 0.5,
    view_hemi_ratio: 0.16,
    view_bounce_ratio: 0.34,
    view_key_gamma: 0.65,
    shadow_strength: 1.0,
    sun_softness: 0.024,
};

/// `this.exposure.setLimits(-4.3, 20)` — the EV100 window the meter is held
/// inside.
///
/// The lower limit is a **night exposure lock**: a moonlit street meters at
/// EV100 -5.2, and letting the meter chase that turns night into an overcast
/// afternoon. Daylight shots meter between -1 and -2.1, so it only ever binds
/// after dark.
pub(crate) const EXPOSURE_LIMITS: (f64, f64) = (-4.3, 20.0);

/// The vignette while the sights are up: `s.vignette + (s.adsVignette -
/// s.vignette) * this._adsT`.
///
/// Written `a + (b - a) * t`, which is the source's grouping and **not**
/// `MathUtils.lerp`'s `(1 - t) * a + t * b`. The two disagree in the last bits
/// and this is the frame's outermost multiply, so the grouping is the
/// specification.
pub(crate) fn ads_vignette(settings: &FrameSettings, ads_t: f64) -> f64 {
    settings.vignette + (settings.ads_vignette - settings.vignette) * ads_t
}

/// `cu.uGrade.value.z = this.taa ? s.sharpen : 0` — the composite's sharpen
/// term exists only where a temporal filter has softened the image.
pub(crate) fn composite_sharpen(settings: &FrameSettings, taa: bool) -> f64 {
    [0.0, settings.sharpen][usize::from(taa)]
}

/// `cu.uGrade.value.x = bloomTex ? s.bloomStrength : 0` — with no pyramid the
/// composite adds nothing rather than adding the colour buffer to itself.
///
/// Worth stating, because `cu.tBloom.value = bloomTex ?? color` binds the
/// colour buffer as the bloom texture when there is no pyramid: the strength is
/// the only thing stopping that from doubling the frame's brightness.
pub(crate) fn composite_bloom_strength(settings: &FrameSettings, has_bloom: bool) -> f64 {
    [0.0, settings.bloom_strength][usize::from(has_bloom)]
}

#[cfg(test)]
mod tests {
    use super::{
        ads_vignette, composite_bloom_strength, composite_sharpen, EXPOSURE_LIMITS,
        SOURCE_SETTINGS,
    };
    use crate::bloom_pyramid::SOURCE_SETTINGS as BLOOM_SETTINGS;
    use crate::exposure::{EXPOSURE_KEY, SCENE_MAX_EV, SCENE_MIN_EV};

    /// Four of these numbers were ported independently by other slices from the
    /// same source file. Asserting the agreement here is what stops the two
    /// copies forking; a change to either side fails this test.
    #[test]
    fn the_settings_agree_with_the_passes_that_already_ported_them() {
        assert_eq!(
            SOURCE_SETTINGS.bloom_threshold as f32,
            BLOOM_SETTINGS.threshold
        );
        assert_eq!(SOURCE_SETTINGS.bloom_knee as f32, BLOOM_SETTINGS.knee);
        assert_eq!(SOURCE_SETTINGS.bloom_strength as f32, BLOOM_SETTINGS.strength);
        assert_eq!(SOURCE_SETTINGS.exposure_key as f32, EXPOSURE_KEY);
        assert_eq!(EXPOSURE_LIMITS.0 as f32, SCENE_MIN_EV);
        assert_eq!(EXPOSURE_LIMITS.1 as f32, SCENE_MAX_EV);
    }

    /// The three numbers the source explicitly changed *away* from an obvious
    /// value, each with a measurement behind it. Pinned so a "round number"
    /// tidy-up has to argue with the measurement.
    #[test]
    fn the_deliberately_unobvious_numbers_are_the_measured_ones() {
        // 1.6, not 0.85: below 1.0-1.5 the daylight sky itself blooms.
        assert_eq!(SOURCE_SETTINGS.bloom_threshold, 1.6);
        // 1.1, not 1.7: occlusion shapes, it does not darken.
        assert_eq!(SOURCE_SETTINGS.ao_intensity, 1.1);
        // 3.3, not 5.5: 40% off, so an ADS frame is not a watercolour.
        assert_eq!(SOURCE_SETTINGS.dof_max_coc, 3.3);
        // 0.0011, not 0.0018: under a pixel of split in the corners.
        assert_eq!(SOURCE_SETTINGS.chromatic, 0.0011);
        // 0.55: half a stop off every practical.
        assert_eq!(SOURCE_SETTINGS.practical_gain, 0.55);
    }

    /// The indirect budget, which is what sets the key:fill ratio.
    #[test]
    fn the_indirect_budget_is_small_and_the_sky_band_dominates_it() {
        assert_eq!(SOURCE_SETTINGS.sky_fill, 0.32);
        assert_eq!(SOURCE_SETTINGS.ground_fill, 0.013);
        assert_eq!(SOURCE_SETTINGS.bounce_fill, 0.008);
        assert_eq!(SOURCE_SETTINGS.ibl_diffuse, 0.030);
        assert_eq!(SOURCE_SETTINGS.interior_indirect, 0.035);
        // The cool band is the largest by an order of magnitude — the frame's
        // only strongly chromatic indirect term.
        assert!(SOURCE_SETTINGS.sky_fill > 10.0 * SOURCE_SETTINGS.ibl_diffuse);
    }

    /// The vignette ramps between two authored values, grouped the source's way.
    #[test]
    fn the_ads_vignette_closes_in_with_the_sight_picture() {
        assert_eq!(ads_vignette(&SOURCE_SETTINGS, 0.0), SOURCE_SETTINGS.vignette);
        assert_eq!(
            ads_vignette(&SOURCE_SETTINGS, 1.0),
            SOURCE_SETTINGS.ads_vignette
        );
        // `a + (b - a) * t`, not `(1 - t) * a + t * b`.
        let t = 0.37;
        let expected = 0.24 + (0.34 - 0.24) * t;
        assert_eq!(ads_vignette(&SOURCE_SETTINGS, t), expected);
        // The two forms are algebraically equal and numerically are not; this
        // pins which one the frame runs.
        let lerp = (1.0 - t) * 0.24 + t * 0.34;
        assert!(
            (expected - lerp).abs() < 1e-15,
            "the two groupings differ by {}",
            (expected - lerp).abs()
        );
    }

    /// Sharpen exists only with TAA; bloom strength only with a pyramid.
    #[test]
    fn two_composite_terms_are_switched_off_rather_than_left_to_a_null() {
        assert_eq!(composite_sharpen(&SOURCE_SETTINGS, true), 0.25);
        assert_eq!(composite_sharpen(&SOURCE_SETTINGS, false), 0.0);
        assert_eq!(composite_bloom_strength(&SOURCE_SETTINGS, true), 0.14);
        assert_eq!(
            composite_bloom_strength(&SOURCE_SETTINGS, false),
            0.0,
            "`tBloom` binds the colour buffer when there is no pyramid, so a \
             non-zero strength here would add the frame to itself"
        );
    }
}
