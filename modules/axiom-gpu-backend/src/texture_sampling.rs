//! The sampler configuration a [`TextureSampling`] mode resolves to, and the
//! anisotropy clamp that keeps it inside what the device actually supports.
//!
//! This is the decision half of material sampling, kept pure and free of `wgpu`
//! so it is compiled and measured on every build rather than only inside the GPU
//! arm's `cfg`. [`crate::scene_renderer`] owns the other half: turning the
//! [`FilterKind`]s below into `wgpu::FilterMode`s and handing the whole thing to
//! `create_sampler`.
//!
//! ## Why anisotropy has to be clamped here rather than simply requested
//!
//! Anisotropic filtering is not universally available — it rides on an extension
//! (`EXT_texture_filter_anisotropic` on the WebGL2 arm this engine actually runs
//! on in the browser), and a device that lacks it reports no anisotropy support at
//! all. There are then two numbers that can disagree: what the material asked for,
//! and what the device can give.
//!
//! `wgpu` will silently clamp an over-large request down to what the device
//! supports, so a naive `anisotropy_clamp: 16` never errors. That silence is
//! exactly the problem: it means the engine cannot say what it actually got, and
//! nothing can test the decision. Resolving it here makes the clamp explicit,
//! deterministic, and assertable, and leaves `scene_renderer` passing a value that
//! is already valid rather than one it hopes will be corrected downstream.
//!
//! ## Why `Crisp` still requests linear minification
//!
//! Both modes minify linearly across a real mip chain. They differ only in
//! magnification and anisotropy. That is not a compromise — it is the whole
//! point: hard texels are a property of a *magnified* texture, and there is no
//! look that is served by point-sampling a *minified* one, only aliasing. See
//! [`axiom_host::TextureSampling`].

use axiom_host::TextureSampling;

/// The two filter modes a sampler axis can take, named without `wgpu` so this
/// module stays pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilterKind {
    /// Point sampling — the nearest single texel.
    Nearest,
    /// Linear interpolation between neighbouring texels (or, for the mipmap
    /// axis, between the two bracketing levels).
    Linear,
}

/// A resolved material sampler: one filter per axis plus the anisotropy clamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SamplerConfig {
    /// Filter used when a texel covers more than one pixel.
    pub(crate) mag: FilterKind,
    /// Filter used when a pixel covers more than one texel.
    pub(crate) min: FilterKind,
    /// Filter used between mip levels.
    pub(crate) mipmap: FilterKind,
    /// Maximum sample ratio along the major axis of the pixel footprint. `1`
    /// means isotropic — no anisotropic filtering.
    pub(crate) anisotropy: u16,
}

/// The highest anisotropy this engine will ever ask for.
///
/// Sixteen is the ceiling of `wgpu`'s own sampler interface, and it is what every
/// device that supports the feature at all reports — the WebGL2 extension's
/// `MAX_TEXTURE_MAX_ANISOTROPY_EXT` is 16 on effectively all desktop and mobile
/// hardware. Asking for the ceiling is right for the surfaces that opt in: they
/// are the ones seen at the most extreme grazing angles, where the footprint
/// ratio far exceeds any value a sampler can offer, so anything less is simply
/// blurrier for no saving.
pub(crate) const MAX_ANISOTROPY: u16 = 16;

/// The sampler a material's [`TextureSampling`] mode resolves to on a device
/// whose maximum supported anisotropy is `device_max`.
///
/// `device_max` is what the adapter reports (`1` when the device has no
/// anisotropic filtering at all). The returned `anisotropy` is always a value the
/// device can honour, and always at least `1` — a zero clamp is not a legal
/// sampler, and a device reporting `0` is reporting "none", which is `1`.
///
/// Note that an [`TextureSampling::Anisotropic`] material on a device with no
/// anisotropy support degrades to plain trilinear with **linear magnification**,
/// not back to [`TextureSampling::Crisp`]. That is deliberate: the material asked
/// for a smooth surface because it recedes, and silently restoring hard texels on
/// exactly the low-end devices least able to resolve them would trade one artifact
/// for a worse one.
pub(crate) fn sampler_config(sampling: TextureSampling, device_max: u16) -> SamplerConfig {
    let anisotropic = usize::from(sampling == TextureSampling::Anisotropic);
    SamplerConfig {
        // The only axis the two modes disagree on, and the reason the choice is
        // per material: anisotropy requires linear magnification, which is the
        // one thing the engine's look cannot afford everywhere.
        mag: [FilterKind::Nearest, FilterKind::Linear][anisotropic],
        min: FilterKind::Linear,
        mipmap: FilterKind::Linear,
        anisotropy: [1, MAX_ANISOTROPY.min(device_max.max(1))][anisotropic],
    }
}

/// The device's usable maximum anisotropy: what it *can* do, held to what its
/// device tier says it should be asked to *afford*.
///
/// A device either supports anisotropic filtering — in which case `wgpu`'s
/// interface exposes it up to [`MAX_ANISOTROPY`] — or it does not, in which case
/// the only legal clamp is `1`. There is no third answer to translate.
///
/// `tier_max` is the second half, and it is not redundant. `supported` is a
/// *capability* answer, and on the WebGPU arm it is not even a measured one: wgpu
/// fills that backend's downlevel flags from `DownlevelCapabilities::default()`
/// because "WebGPU is assumed to be fully compliant", so it reads `true` on a
/// phone exactly as on a workstation. Sixteen taps per pixel across a road that
/// recedes to the horizon is affordable on one and not on the other, and no
/// capability flag will ever say so. The tier does
/// ([`axiom_host::HostDeviceProfile::max_anisotropy`]), so the resolved clamp is
/// the smaller of the two.
pub(crate) fn device_max_anisotropy(supported: bool, tier_max: u16) -> u16 {
    [1, MAX_ANISOTROPY.min(tier_max)][usize::from(supported)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default mode must not change how a magnified texture looks. This is
    /// the engine's retro identity, and it is the assertion that fails if someone
    /// "simplifies" the two modes into one.
    #[test]
    fn crisp_keeps_nearest_magnification_and_takes_no_anisotropy() {
        let c = sampler_config(TextureSampling::Crisp, 16);
        assert_eq!(c.mag, FilterKind::Nearest, "hard texels up close are the look");
        assert_eq!(c.anisotropy, 1, "the default surface asks for no anisotropy");
    }

    /// …but it still minifies properly. Point-sampled minification is the defect
    /// this whole module exists to remove, and it is not part of the look.
    #[test]
    fn both_modes_minify_linearly_across_the_mip_chain() {
        for mode in [TextureSampling::Crisp, TextureSampling::Anisotropic] {
            let c = sampler_config(mode, 16);
            assert_eq!(c.min, FilterKind::Linear, "{mode:?} point-samples minification");
            assert_eq!(c.mipmap, FilterKind::Linear, "{mode:?} does not blend mip levels");
        }
    }

    #[test]
    fn anisotropic_takes_the_device_maximum_and_linear_magnification() {
        let c = sampler_config(TextureSampling::Anisotropic, 16);
        assert_eq!(c.anisotropy, 16);
        assert_eq!(
            c.mag,
            FilterKind::Linear,
            "anisotropy requires linear magnification — the hardware validates it"
        );
    }

    /// The clamp. Asking for more than the device has is the one way to turn a
    /// sampler into a validation error, and it must be impossible to construct
    /// here rather than corrected silently downstream.
    #[test]
    fn anisotropy_never_exceeds_what_the_device_reports() {
        for device_max in [1u16, 2, 4, 8, 16] {
            let c = sampler_config(TextureSampling::Anisotropic, device_max);
            assert!(
                c.anisotropy <= device_max,
                "asked for {} on a device that supports {device_max}",
                c.anisotropy
            );
            assert!(c.anisotropy >= 1, "a zero anisotropy clamp is not a legal sampler");
        }
        // A device reporting more than the interface allows is still capped at
        // the interface's ceiling.
        assert_eq!(sampler_config(TextureSampling::Anisotropic, 64).anisotropy, MAX_ANISOTROPY);
        // And a device reporting nonsense (`0` — "none", spelled wrong) resolves
        // to the isotropic `1` rather than to an illegal clamp.
        assert_eq!(sampler_config(TextureSampling::Anisotropic, 0).anisotropy, 1);
    }

    /// A device without the extension still gets the smooth surface it asked
    /// for, just without the extra taps — never a silent return to hard texels.
    #[test]
    fn an_unsupported_device_degrades_to_trilinear_not_back_to_crisp() {
        let c = sampler_config(TextureSampling::Anisotropic, 1);
        assert_eq!(c.anisotropy, 1);
        assert_eq!(c.mag, FilterKind::Linear);
        assert_eq!(c.min, FilterKind::Linear);
    }

    #[test]
    fn the_device_maximum_follows_whether_the_feature_is_reported() {
        assert_eq!(device_max_anisotropy(true, MAX_ANISOTROPY), MAX_ANISOTROPY);
        assert_eq!(device_max_anisotropy(false, MAX_ANISOTROPY), 1);
    }

    /// **The regression this pair of arguments exists to prevent.** A supported
    /// device on the mobile tier must not be handed the desktop tap budget — that
    /// is exactly what the WebGPU arm did on every phone, because its `supported`
    /// flag is an assumption of compliance rather than a measurement.
    #[test]
    fn the_tier_budget_caps_a_device_that_reports_full_support() {
        assert_eq!(
            device_max_anisotropy(true, 4),
            4,
            "a capable device on the mobile tier still only spends the tier's taps"
        );
        assert_eq!(
            sampler_config(TextureSampling::Anisotropic, device_max_anisotropy(true, 4)).anisotropy,
            4
        );
    }

    /// The two limits compose in both directions: neither can raise the other.
    #[test]
    fn the_resolved_clamp_is_the_smaller_of_capability_and_tier() {
        // A tier asking for more than the interface allows is still capped.
        assert_eq!(device_max_anisotropy(true, 64), MAX_ANISOTROPY);
        // An unsupported device is `1` no matter how generous the tier is.
        assert_eq!(device_max_anisotropy(false, 16), 1);
        assert_eq!(device_max_anisotropy(false, 4), 1);
    }
}
