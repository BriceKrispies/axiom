//! Backend-neutral **bloom**: how much of a pixel's brightness spills into the
//! pixels around it, and how the surplus above white is rolled off rather than
//! clipped.
//!
//! Bloom is the difference between a bright patch and a *light*. Without it, a
//! material authored to emit more than `1.0` simply clamps: the emissive term is
//! added, the surplus is thrown away by the 8-bit sRGB target, and a lamp reads
//! as a flat white sticker. With it, the surplus is what decides how far the
//! light bleeds, so authoring a value above one finally means something.
//!
//! Carried as neutral frame data — like [`crate::FrameAmbient`] and
//! [`crate::FrameSky`] — and gated by its own [`crate::RenderCapability::Bloom`],
//! so a backend without the render targets to do it declares the drop rather
//! than silently ignoring the frame's intent. Deliberately NOT gated by
//! [`crate::RenderCapability::PostProcess`], which is the whole-image colour
//! grade: the Canvas 2D backend genuinely performs that and genuinely cannot
//! bloom, so one bit covering both would force it to lie about one of them.
//!
//! The two pure functions here, [`FrameBloom::contribution`] and
//! [`FrameBloom::tonemap`], are the reference implementations the GPU post chain
//! mirrors. Keeping them here is what lets the curve be tested and tuned without
//! a GPU, and what stops "the threshold" meaning two different things in two
//! places.

use axiom_kernel::Ratio;

/// Bloom parameters: which pixels spill, how far, and how hard.
///
/// The tuning scalars are stored as plain `f32` because every one of them is
/// consumed by arithmetic in this file; they cross the *public* boundary as
/// [`Ratio`], so no naked float reaches a caller.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameBloom {
    threshold: f32,
    knee: f32,
    intensity: f32,
    radius: f32,
}

impl FrameBloom {
    /// Assemble bloom parameters. Crate-internal, so no naked tuning scalar
    /// crosses the module facade — the public constructors are the presets
    /// below, matching [`crate::FramePostProcess`].
    pub(crate) const fn new(threshold: f32, knee: f32, intensity: f32, radius: f32) -> Self {
        FrameBloom {
            threshold,
            knee,
            intensity,
            radius,
        }
    }

    /// **A gentle night glow.** A low threshold, because in a night scene the
    /// things that should glow — a lamp, a reflector, a tail light — are often
    /// only a little above the surroundings rather than blazing; a wide soft
    /// knee so the transition into bloom is not a visible contour on a gradient;
    /// a restrained intensity, because the failure mode of bloom is a hazy
    /// smeared frame rather than too little glow; and a generous radius, which
    /// is what makes it read as a soft halo rather than a fringe.
    pub const fn moonlit() -> Self {
        FrameBloom::new(0.62, 0.35, 0.85, 2.6)
    }

    /// A tighter, brighter bloom for a daylit scene: only genuine highlights
    /// spill, and they spill a short distance.
    pub const fn highlights() -> Self {
        FrameBloom::new(1.0, 0.15, 0.55, 1.4)
    }

    /// Luminance at which a pixel begins to bloom.
    pub const fn threshold(&self) -> Ratio {
        Ratio::finite_or_zero(self.threshold)
    }

    /// Width of the soft knee below the threshold.
    pub const fn knee(&self) -> Ratio {
        Ratio::finite_or_zero(self.knee)
    }

    /// How strongly the blurred bright pass is added back.
    pub const fn intensity(&self) -> Ratio {
        Ratio::finite_or_zero(self.intensity)
    }

    /// Blur radius, in source pixels at the bloom pass's own resolution.
    pub const fn radius(&self) -> Ratio {
        Ratio::finite_or_zero(self.radius)
    }

    /// **The bright pass**, as a scale to apply to a pixel of luminance `luma`.
    ///
    /// A hard `luma > threshold` cut puts a visible contour across any smooth
    /// gradient that crosses the threshold — a night sky is exactly such a
    /// gradient, so the naive version is unusable here. This is the standard
    /// quadratic knee: below `threshold - knee` nothing blooms, above
    /// `threshold + knee` the full surplus blooms, and between them the response
    /// is a parabola that meets both ends smoothly.
    ///
    /// Returns a `0..1` scale rather than a colour, so the caller multiplies its
    /// own RGB by it and the hue of a bloomed highlight is the hue of the pixel.
    pub fn contribution(&self, luma: Ratio) -> Ratio {
        let luma = luma.get().max(0.0);
        let knee = self.knee.max(MIN_KNEE);
        // The knee curve, active in a `2 * knee` window centred on the threshold.
        let soft = (luma - self.threshold + knee).clamp(0.0, 2.0 * knee);
        let curved = soft * soft / (4.0 * knee);
        // Above the knee the plain surplus takes over; `max` is the join, and it
        // is exact at both ends by construction.
        let surplus = curved.max(luma - self.threshold);
        Ratio::finite_or_zero((surplus / luma.max(MIN_LUMA)).clamp(0.0, 1.0))
    }

    /// **The highlight rolloff.** Compress a channel that may exceed `1.0` into
    /// `0..1` with a knee, instead of clipping it.
    ///
    /// Without this, everything above white becomes exactly white and all the
    /// shape in a bright area is lost — the surplus that bloom exists to spend
    /// is thrown away before bloom ever sees it. Below [`ROLLOFF_KNEE`] the
    /// response is the identity, so ordinary midtones are untouched and a frame
    /// with no highlights is unchanged.
    pub fn tonemap(&self, channel: Ratio) -> Ratio {
        let x = channel.get().max(0.0);
        let over = (x - ROLLOFF_KNEE).max(0.0);
        // A reciprocal shoulder: approaches 1.0 asymptotically, never reaches it,
        // and is continuous in value and slope at the knee.
        let span = 1.0 - ROLLOFF_KNEE;
        let shoulder = span * over / (span + over);
        Ratio::finite_or_zero((x.min(ROLLOFF_KNEE) + shoulder).clamp(0.0, 1.0))
    }
}

/// Luminance below which a pixel is treated as black for the bright pass, so the
/// division cannot blow up.
const MIN_LUMA: f32 = 1.0e-4;

/// The narrowest knee the curve is evaluated with, so a zero knee degrades to a
/// hard threshold rather than dividing by zero.
const MIN_KNEE: f32 = 1.0e-4;

/// Where the highlight rolloff leaves the identity and starts to compress.
///
/// Private as a scalar (it is arithmetic here); the GPU post chain reads it
/// through [`rolloff_knee`], which is the typed public form.
const ROLLOFF_KNEE: f32 = 0.75;

/// Where the highlight rolloff leaves the identity and starts to compress — the
/// value a backend's tonemap shader must mirror to stay in step with
/// [`FrameBloom::tonemap`].
pub const fn rolloff_knee() -> Ratio {
    Ratio::finite_or_zero(ROLLOFF_KNEE)
}

/// Rec.709 luminance of a linear-RGB colour — the weighting the bright pass and
/// the grade both measure brightness with, so "bright" means one thing.
pub fn luminance(rgb: [f32; 3]) -> Ratio {
    Ratio::finite_or_zero(0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests read the typed accessors as plain scalars.
    fn r(v: Ratio) -> f32 {
        v.get()
    }

    fn q(v: f32) -> Ratio {
        Ratio::finite_or_zero(v)
    }

    #[test]
    fn the_presets_are_ordered_the_way_their_names_claim() {
        let night = FrameBloom::moonlit();
        let day = FrameBloom::highlights();
        assert!(
            r(night.threshold()) < r(day.threshold()),
            "a night scene blooms things that are only a little bright"
        );
        assert!(r(night.knee()) > r(day.knee()), "with a softer transition");
        assert!(r(night.intensity()) > r(day.intensity()), "and more of it");
        assert!(r(night.radius()) > r(day.radius()), "spread wider");
        assert_ne!(night, day);
        assert_eq!(night, FrameBloom::moonlit());
        assert!(format!("{night:?}").contains("FrameBloom"));
    }

    #[test]
    fn nothing_below_the_knee_blooms_and_everything_bright_does() {
        let b = FrameBloom::moonlit();
        assert_eq!(r(b.contribution(q(0.0))), 0.0, "black does not glow");
        assert_eq!(
            r(b.contribution(q(r(b.threshold()) - r(b.knee())))),
            0.0,
            "and neither does the bottom of the knee"
        );
        assert!(r(b.contribution(b.threshold())) > 0.0, "the threshold itself glows a little");
        // The scale is the *fraction* of the pixel that blooms, so it approaches
        // one asymptotically: at 4x white a bit over four fifths of the pixel
        // spills, and only an arbitrarily bright pixel spills all of it.
        assert!(r(b.contribution(q(4.0))) > 0.8, "a genuine light mostly glows");
        assert!(r(b.contribution(q(1_000.0))) > 0.99, "and a blinding one glows fully");
        assert!(r(b.contribution(q(1.0e6))) <= 1.0, "the scale is bounded");
    }

    /// The whole reason for the knee: the response has to be smooth, or a
    /// gradient crossing the threshold shows a contour line.
    #[test]
    fn the_bright_pass_rises_smoothly_with_no_step() {
        let b = FrameBloom::moonlit();
        let samples: Vec<f32> = (0..=400).map(|i| r(b.contribution(q(i as f32 * 0.01)))).collect();
        let mut previous = 0.0;
        for (i, value) in samples.iter().enumerate() {
            assert!(*value >= previous - 1.0e-6, "step {i} went backwards");
            assert!(
                *value - previous < 0.05,
                "step {i} jumped from {previous} to {value}"
            );
            previous = *value;
        }
        assert!(previous > 0.8, "and it gets most of the way there: {previous}");
    }

    #[test]
    fn a_collapsed_knee_degrades_to_a_hard_threshold_rather_than_dividing_by_zero() {
        let hard = FrameBloom::new(1.0, 0.0, 1.0, 1.0);
        assert_eq!(r(hard.contribution(q(0.5))), 0.0);
        assert!(r(hard.contribution(q(2.0))) > 0.0);
        assert!(r(hard.contribution(q(2.0))).is_finite());
        // And a negative luminance is treated as black, not as a negative scale.
        assert_eq!(r(hard.contribution(q(-5.0))), 0.0);
    }

    #[test]
    fn the_rolloff_is_the_identity_below_the_knee_and_compresses_above_it() {
        let b = FrameBloom::moonlit();
        for x in [0.0f32, 0.1, 0.4, ROLLOFF_KNEE] {
            assert!((r(b.tonemap(q(x))) - x).abs() < 1.0e-6, "{x} was not left alone");
        }
        assert!(r(b.tonemap(q(1.0))) < 1.0, "white itself is pulled under one");
        assert!(r(b.tonemap(q(1.0))) > ROLLOFF_KNEE, "but well above the knee");
        assert!(r(b.tonemap(q(100.0))) < 1.0, "and a huge value never reaches one");
        assert!(r(b.tonemap(q(4.0))) > r(b.tonemap(q(2.0))), "brighter still reads brighter");
        assert_eq!(r(b.tonemap(q(-3.0))), 0.0, "and negatives clamp to black");
    }

    /// The point of the rolloff: two different over-white values must stay
    /// distinguishable, which is exactly what clipping destroys.
    #[test]
    fn over_white_values_stay_distinguishable_instead_of_clipping_together() {
        let b = FrameBloom::moonlit();
        let a = r(b.tonemap(q(1.5)));
        let c = r(b.tonemap(q(3.0)));
        assert!(c - a > 0.02, "1.5 and 3.0 collapsed together: {a} vs {c}");
        assert!(a < 1.0 && c < 1.0);
    }

    /// The shader mirrors this knee, so the exported value and the curve's
    /// actual inflection have to be the same number — a drift between them is a
    /// visible seam between the CPU reference and the GPU composite.
    #[test]
    fn the_exported_rolloff_knee_is_where_the_curve_actually_bends() {
        let b = FrameBloom::moonlit();
        let knee = rolloff_knee();
        assert_eq!(knee.get(), ROLLOFF_KNEE);
        // Identity up to the knee...
        assert!((r(b.tonemap(knee)) - knee.get()).abs() < 1.0e-6);
        // ...and compressing above it.
        assert!(r(b.tonemap(q(knee.get() + 0.2))) < knee.get() + 0.2);
    }

    #[test]
    fn luminance_is_rec_709_weighted() {
        assert_eq!(r(luminance([0.0, 0.0, 0.0])), 0.0);
        assert!((r(luminance([1.0, 1.0, 1.0])) - 1.0).abs() < 1.0e-6, "white is one");
        // Green carries most of the perceived brightness, blue almost none.
        assert!(r(luminance([0.0, 1.0, 0.0])) > r(luminance([1.0, 0.0, 0.0])));
        assert!(r(luminance([1.0, 0.0, 0.0])) > r(luminance([0.0, 0.0, 1.0])));
    }
}
