//! The **tone map** a frame is presented through: the app's opt-in to a
//! scene-referred, high-dynamic-range present path.
//!
//! This is the one part of [`crate::FrameRenderLook`] that is not a description
//! of the picture — it is a statement about the **numbers behind** the picture.
//! Everything else on the look (ambient, fog, sky, bloom, grade) describes an
//! image a backend already knew how to produce. A tone map says the frame's
//! shading is *radiance*, not *display colour*: a fragment may legitimately emit
//! `4.0`, and the present pass is the thing that decides where `4.0` lands on a
//! screen whose brightest byte is `1.0`.
//!
//! # Why it is an opt-in and not a default
//!
//! Because it changes what the intermediate target *is*. Without it a backend
//! renders into an 8-bit sRGB intermediate, which clamps at display white before
//! any post pass can see the frame; with it a capable backend renders into a
//! float target and nothing is clamped until the present. Those are two
//! different images, not two qualities of one image — every value the old path
//! silently crushed to white now has somewhere to go, and every existing app in
//! this repo was authored against the crush.
//!
//! So the opt-in is the whole contract: **a look with no tone map presents the
//! bytes it always did**, and a look with one asks for the other path and gets
//! it wherever [`crate::RenderCapability::HdrTargets`] is granted. A backend
//! without that capability declares the drop, exactly as it does for a sky or a
//! bloom it cannot afford, and renders the 8-bit chain.
//!
//! # What the curve is
//!
//! [`FrameTonemap::filmic`] is **AgX** — a log-space filmic curve with a
//! Rec.2020 working space and a per-channel inset/outset pair, which is what
//! keeps a blown highlight desaturating toward white the way film does instead
//! of clipping to a flat primary. It is *not* ACES and it is not the reciprocal
//! shoulder the 8-bit chain uses; those differ from it in every constant.
//!
//! `strength` blends between the two: `0` is exactly the shoulder the LDR chain
//! already applies (so a tone map at zero strength is the identity against
//! today's arithmetic, which is what makes the blend measurable rather than a
//! matter of taste), `1` is AgX alone. `exposure` is the **scene-linear** scale
//! applied to radiance *before* the curve — the stop the frame is metered at —
//! and it is deliberately not [`crate::FramePostProcess::exposure`], which is a
//! display-referred multiply applied to already-encoded values after the curve.
//! Two different quantities that happen to share a word.

use axiom_kernel::Ratio;

/// How a frame's scene-referred radiance becomes display colour.
///
/// Presence of a `FrameTonemap` on a [`crate::FrameRenderLook`] *is* the request
/// for a high-dynamic-range scene target; see the module docs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameTonemap {
    strength: f32,
    exposure: f32,
}

impl FrameTonemap {
    /// The shipped filmic look: AgX at full strength, scene exposure unchanged.
    ///
    /// The curve's own `slope` / `power` / `saturation` are not parameters here.
    /// They are constants of the transcription (`1.0`, `1.0`, `1.08`), and the
    /// source is emphatic about why: `slope` multiplies the *log-normalised*
    /// value, so nudging it is not "a little brighter", it is a whole-stop move
    /// applied after AgX has already placed mid grey.
    pub const fn filmic() -> Self {
        FrameTonemap {
            strength: 1.0,
            exposure: 1.0,
        }
    }

    /// A tone map at an explicit blend `strength` and scene-linear `exposure`.
    ///
    /// `strength` `0` is the LDR chain's reciprocal shoulder — the arithmetic the
    /// present path already ran — so it is the honest zero end of the blend and
    /// the reference a strength sweep is measured against. `exposure` scales
    /// radiance before the curve; `1.0` leaves the scene at the level it was
    /// shaded.
    pub const fn blended(strength: Ratio, exposure: Ratio) -> Self {
        FrameTonemap {
            strength: strength.get(),
            exposure: exposure.get(),
        }
    }

    /// How far toward AgX the present blends: `0` the LDR shoulder, `1` AgX.
    pub const fn strength(&self) -> Ratio {
        Ratio::finite_or_zero(self.strength)
    }

    /// The scene-linear exposure applied to radiance before the curve.
    pub const fn exposure(&self) -> Ratio {
        Ratio::finite_or_zero(self.exposure)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shipped preset, and the two lanes it fixes.
    #[test]
    fn the_filmic_preset_is_full_strength_at_unit_exposure() {
        let t = FrameTonemap::filmic();
        assert_eq!(t.strength().get(), 1.0);
        assert_eq!(t.exposure().get(), 1.0);
        assert_eq!(t, FrameTonemap::filmic());
    }

    /// The blend carries both lanes through unchanged, and zero strength is
    /// reachable — the end of the sweep that is bit-identical to the LDR
    /// shoulder is the one that has to be expressible.
    #[test]
    fn a_blended_tonemap_carries_its_strength_and_exposure() {
        let off = FrameTonemap::blended(Ratio::finite_or_zero(0.0), Ratio::finite_or_zero(1.0));
        assert_eq!(off.strength().get(), 0.0);
        assert_eq!(off.exposure().get(), 1.0);
        let half = FrameTonemap::blended(Ratio::finite_or_zero(0.5), Ratio::finite_or_zero(0.25));
        assert_eq!(half.strength().get(), 0.5);
        assert_eq!(half.exposure().get(), 0.25);
        assert_ne!(off, half);
        assert_ne!(half, FrameTonemap::filmic());
        // A non-finite lane degrades to zero rather than poisoning the present.
        let bad = FrameTonemap::blended(
            Ratio::finite_or_zero(f32::NAN),
            Ratio::finite_or_zero(f32::INFINITY),
        );
        assert_eq!(bad.strength().get(), 0.0);
        assert_eq!(bad.exposure().get(), 0.0);
        assert!(format!("{bad:?}").contains("FrameTonemap"));
    }
}
