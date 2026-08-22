//! **Which passes exist at all** — `init()`'s construction block, transcribed.
//!
//! ```js
//! this.gbuffer   = new GBuffer();
//! this.gtao      = q.gtao ? new Gtao() : null;
//! this.contact   = this.qLevel >= 1 ? new ContactShadows() : null;
//! this.ssr       = q.ssr ? new Ssr() : null;
//! this.taa       = q.taa ? new Taa() : null;
//! this.motionBlur= q.motionBlur ? new MotionBlur() : null;
//! this.dof       = this.qLevel >= 1 ? new DepthOfField() : null;
//! this.bloom     = q.bloom ? new Bloom(this.qLevel >= 2 ? 6 : 5) : null;
//! this.exposure  = new AutoExposure();
//! this.fxaa      = q.taa ? null : createFxaa();
//! this.needsPrepass = true;
//! ```
//!
//! Two of those gates are the *level* and five are named preset flags, and the
//! distinction is not cosmetic: contact shadows and the ADS depth of field are
//! on at `medium` **without** having a preset flag of their own, so a reader
//! looking only at `QUALITY_PRESETS` would conclude they never run.
//!
//! # Existing is not running
//!
//! Every screen-space consumer is gated **twice** in the source: once here (does
//! the object exist) and once per frame (`this.gtao && this.needsPrepass`).
//! `needsPrepass` is unconditionally `true` there, with the reason stated — the
//! depth and velocity textures "are part of the public contract (soft
//! particles, SSR, motion blur) even when our own effects are off". The
//! `runs_*` methods below keep both gates separate for the same reason: a
//! future frame that turned the prepass off must turn its five consumers off
//! with it, and one fused boolean would let them drift.
//!
//! # Where a capability enters, and where it must not
//!
//! Axiom has one fact the source could not have: whether the device can hold
//! the G-buffer at all. `Rgba8Unorm` cannot store a UV-space velocity — every
//! useful magnitude quantizes to zero — so the declared degradation for
//! [`axiom_host::RenderCapability::GBuffer`] is a
//! [`axiom_host::CapabilityDegradation::Drop`], not a substitute. That is
//! exactly `needsPrepass = false`, and it is the one place a capability is
//! consulted here.
//!
//! It is deliberately **not** consulted for bloom, post-processing or shadows,
//! even though [`axiom_host::RenderCapability`] has a bit for each. Those bits
//! describe what *this crate's frame graph offers a frame*; gating the frame
//! graph on them would be circular — the pipeline asking itself for permission
//! to be the pipeline. [`axiom_host::RenderCapability::HdrTargets`] is a genuine
//! device fact and is consulted, but by [`super::targets`], where it decides a
//! *format* rather than whether a pass runs.

use axiom_host::BackendCapabilityProfile;

use super::quality::{CsmConfig, QualityTier};
use crate::gbuffer::gbuffer_attachments_available;

/// What `init()` built, for one tier on one device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FramePipeline {
    tier: QualityTier,
    csm: CsmConfig,
    gtao: bool,
    contact: bool,
    ssr: bool,
    taa: bool,
    motion_blur: bool,
    dof: bool,
    bloom_levels: Option<usize>,
    fxaa: bool,
    view_samples: u32,
    needs_prepass: bool,
    max_anisotropy: u32,
}

impl FramePipeline {
    /// `init()`, minus the GPU.
    ///
    /// `device_max_anisotropy` is
    /// `renderer.capabilities.getMaxAnisotropy()`; the source takes the
    /// `Math.min` of it and the preset's request.
    pub(crate) fn resolve(
        tier: QualityTier,
        profile: BackendCapabilityProfile,
        device_max_anisotropy: u32,
    ) -> Self {
        let preset = tier.preset();
        let level = tier.level();
        Self {
            tier,
            csm: tier.csm(),
            gtao: preset.gtao,
            // `this.qLevel >= 1` — no preset flag of its own.
            contact: level >= 1,
            ssr: preset.ssr,
            taa: preset.taa,
            motion_blur: preset.motion_blur,
            // `this.qLevel >= 1`, likewise.
            dof: level >= 1,
            bloom_levels: tier.bloom_levels(),
            // `q.taa ? null : createFxaa()` — the two are mutually exclusive by
            // construction, which is why the composite's sharpen term keys off
            // TAA and the LDR intermediate keys off FXAA.
            fxaa: !preset.taa,
            view_samples: tier.view_samples(),
            // `this.needsPrepass = true`, degraded by the one device fact the
            // source could not have. See the module docs.
            needs_prepass: gbuffer_attachments_available(profile),
            max_anisotropy: preset.anisotropy.min(device_max_anisotropy),
        }
    }

    /// The tier this pipeline was resolved for.
    pub(crate) const fn tier(&self) -> QualityTier {
        self.tier
    }

    /// `this.q.renderScale`, the number [`super::targets::screen_sizing`] wants.
    pub(crate) const fn render_scale(&self) -> f64 {
        self.tier.preset().render_scale
    }

    /// The cascade configuration, post-clamp.
    pub(crate) const fn csm(&self) -> CsmConfig {
        self.csm
    }

    /// `Math.min(q.anisotropy, renderer.capabilities.getMaxAnisotropy())`.
    pub(crate) const fn max_anisotropy(&self) -> u32 {
        self.max_anisotropy
    }

    /// `this._viewSamples`.
    pub(crate) const fn view_samples(&self) -> u32 {
        self.view_samples
    }

    /// `this.bloom !== null`, and how deep its pyramid is.
    pub(crate) const fn bloom_levels(&self) -> Option<usize> {
        self.bloom_levels
    }

    /// `this.taa !== null`. Also decides the camera jitter, the CSM's per-frame
    /// jitter index and the composite's sharpen term.
    pub(crate) const fn taa(&self) -> bool {
        self.taa
    }

    /// `this.fxaa !== null` — and therefore whether `ldrRt` is allocated.
    pub(crate) const fn fxaa(&self) -> bool {
        self.fxaa
    }

    /// `this.needsPrepass`, after the G-buffer capability gate.
    pub(crate) const fn runs_prepass(&self) -> bool {
        self.needs_prepass
    }

    /// `this.csm.enabled` — always true in the source; there is no tier or
    /// setting that clears it, only the `dispose()` path. Carried as a method
    /// so the schedule reads the same shape for every pass.
    pub(crate) const fn runs_cascades(&self) -> bool {
        true
    }

    /// `this.gtao && this.needsPrepass`.
    pub(crate) const fn runs_gtao(&self) -> bool {
        self.gtao & self.needs_prepass
    }

    /// `this.contact && this.needsPrepass`.
    pub(crate) const fn runs_contact(&self) -> bool {
        self.contact & self.needs_prepass
    }

    /// `this.ssr && this.needsPrepass && !this._firstFrame`.
    ///
    /// The first-frame gate is not defensive: SSR colours its hits from the
    /// **previous** frame's resolved image, and on frame one there is not one.
    pub(crate) const fn runs_ssr(&self, first_frame: bool) -> bool {
        self.ssr & self.needs_prepass & !first_frame
    }

    /// `this.motionBlur !== null`. The only screen-space pass **not** gated on
    /// the prepass in the source, even though it samples the velocity buffer —
    /// a source defect, pinned rather than fixed. See
    /// `tests::motion_blur_is_the_one_velocity_consumer_the_source_forgot_to_gate`.
    pub(crate) const fn runs_motion_blur(&self) -> bool {
        self.motion_blur
    }

    /// `this.dof && this._adsT > 0.01 && this.needsPrepass`.
    pub(crate) fn runs_dof(&self, ads_t: f64) -> bool {
        self.dof & (ads_t > 0.01) & self.needs_prepass
    }

    /// `this.taa` at step 10 — the same object as [`Self::taa`], named for the
    /// schedule so every step reads `runs_*`.
    pub(crate) const fn runs_taa(&self) -> bool {
        self.taa
    }
}

#[cfg(test)]
mod tests {
    use super::FramePipeline;
    use crate::frame_graph::quality::{QualityTier, QUALITY_TIERS};
    use axiom_host::{BackendCapabilityProfile, RenderCapability};

    fn full() -> BackendCapabilityProfile {
        BackendCapabilityProfile::all()
    }

    /// The construction block, tier by tier. This table *is* the boot line's
    /// second half plus the two level-gated passes it never mentions.
    #[test]
    fn the_tier_decides_which_passes_are_constructed() {
        let built: Vec<(bool, bool, bool, bool, bool, bool, bool)> = QUALITY_TIERS
            .iter()
            .map(|&t| {
                let p = FramePipeline::resolve(t, full(), 16);
                (
                    p.runs_gtao(),
                    p.runs_contact(),
                    p.runs_ssr(false),
                    p.taa(),
                    p.runs_motion_blur(),
                    p.runs_dof(1.0),
                    p.fxaa(),
                )
            })
            .collect();
        assert_eq!(
            built,
            vec![
                // low: gtao, contact, ssr, taa, mb, dof, fxaa
                (false, false, false, false, false, false, true),
                (true, true, false, true, true, true, false),
                (true, true, true, true, true, true, false),
                (true, true, true, true, true, true, false),
            ]
        );
    }

    /// Contact shadows and the ADS depth of field are on from `medium` up and
    /// have no preset flag — the two passes a reader of `QUALITY_PRESETS` alone
    /// would conclude never run.
    #[test]
    fn the_two_level_gated_passes_have_no_preset_flag() {
        let low = FramePipeline::resolve(QualityTier::Low, full(), 16);
        let medium = FramePipeline::resolve(QualityTier::Medium, full(), 16);
        assert!(!low.runs_contact() & !low.runs_dof(1.0));
        assert!(medium.runs_contact() & medium.runs_dof(1.0));
        // ...and `medium` differs from `low` in three preset flags, none of
        // which is named "contact" or "dof".
        assert!(!QualityTier::Low.preset().gtao & QualityTier::Medium.preset().gtao);
        assert!(!QualityTier::Low.preset().taa & QualityTier::Medium.preset().taa);
        assert!(!QualityTier::Low.preset().motion_blur & QualityTier::Medium.preset().motion_blur);
    }

    /// TAA and FXAA are mutually exclusive by construction, at every tier.
    #[test]
    fn taa_and_fxaa_are_never_both_on() {
        assert!(QUALITY_TIERS.iter().all(|&t| {
            let p = FramePipeline::resolve(t, full(), 16);
            p.taa() != p.fxaa()
        }));
    }

    /// The ADS depth of field's threshold is `> 0.01`, not `> 0` — a sight
    /// picture one percent of the way up runs nothing.
    #[test]
    fn the_ads_depth_of_field_has_a_dead_zone_at_the_bottom() {
        let ultra = FramePipeline::resolve(QualityTier::Ultra, full(), 16);
        assert!(!ultra.runs_dof(0.0));
        assert!(!ultra.runs_dof(0.01), "the comparison is strict");
        assert!(ultra.runs_dof(0.0100001));
        assert!(ultra.runs_dof(1.0));
    }

    /// SSR is skipped on frame one because there is no previous frame to
    /// colour its hits from.
    #[test]
    fn ssr_does_not_run_on_the_first_frame() {
        let ultra = FramePipeline::resolve(QualityTier::Ultra, full(), 16);
        assert!(!ultra.runs_ssr(true));
        assert!(ultra.runs_ssr(false));
    }

    /// A device that cannot bind the G-buffer's attachment set drops the
    /// prepass **and every consumer of it** — the declared `Drop` degradation.
    /// Motion blur is the exception, and that is the source's bug, not ours.
    #[test]
    fn motion_blur_is_the_one_velocity_consumer_the_source_forgot_to_gate() {
        let no_gbuffer = full().without(RenderCapability::GBuffer);
        let p = FramePipeline::resolve(QualityTier::Ultra, no_gbuffer, 16);
        assert!(!p.runs_prepass());
        assert!(!p.runs_gtao());
        assert!(!p.runs_contact());
        assert!(!p.runs_ssr(false));
        assert!(!p.runs_dof(1.0));
        // ...and yet:
        assert!(
            p.runs_motion_blur(),
            "index.js:1441 gates motion blur on `this.motionBlur` alone, though \
             MotionBlur.render reads gbuffer.velocityTexture — pinned, not fixed"
        );
        // The cascades are independent of the G-buffer and still run.
        assert!(p.runs_cascades());
        // TAA likewise reads velocity and is likewise ungated in the source.
        assert!(p.runs_taa());
    }

    /// Losing HDR targets alone does not lose the G-buffer, because
    /// `gbuffer_attachments_available` asks about both — so this device drops
    /// the prepass too, for the *other* reason (its float attachments).
    #[test]
    fn a_device_without_hdr_attachments_cannot_hold_the_gbuffer_either() {
        let ldr_only = full().without(RenderCapability::HdrTargets);
        let p = FramePipeline::resolve(QualityTier::Ultra, ldr_only, 16);
        assert!(!p.runs_prepass());
    }

    /// The anisotropy request is clamped against what the adapter reports.
    #[test]
    fn anisotropy_is_the_lesser_of_the_request_and_the_device() {
        assert_eq!(
            FramePipeline::resolve(QualityTier::Ultra, full(), 4).max_anisotropy(),
            4
        );
        assert_eq!(
            FramePipeline::resolve(QualityTier::Ultra, full(), 16).max_anisotropy(),
            16
        );
        assert_eq!(
            FramePipeline::resolve(QualityTier::Low, full(), 16).max_anisotropy(),
            4,
            "the preset asks for 4 and the device offering more changes nothing"
        );
    }

    /// The pass-through accessors carry the tier's own answers unchanged.
    #[test]
    fn the_pipeline_reports_the_tier_it_was_resolved_for() {
        let p = FramePipeline::resolve(QualityTier::High, full(), 16);
        assert_eq!(p.tier(), QualityTier::High);
        assert_eq!(p.render_scale(), 1.0);
        assert_eq!(p.csm(), QualityTier::High.csm());
        assert_eq!(p.view_samples(), 4);
        assert_eq!(p.bloom_levels(), QualityTier::High.bloom_levels());
    }
}
