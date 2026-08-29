//! **Whether this device can hold a high-dynamic-range colour attachment** —
//! resolved from what the adapter reports, never asserted from a policy.
//!
//! The peer of `texture_sampling`, and pure for the same reason: the
//! rule that decides what a frame is allowed to render into is impossible to
//! debug from inside a render pass, so it lives here in plain booleans, compiled
//! everywhere and measured by the coverage gate, while the wasm-only binding does
//! nothing but ask the adapter and hand the answers over.
//!
//! # What this replaces
//!
//! The GPU post chain used to state, in prose, that an `Rgba16Float`
//! intermediate was "deliberately not taken": this engine requests WebGL2
//! downlevel limits on *both* browser arms to hold them at capability parity, and
//! half-float render targets are not guaranteed under those limits, so asking for
//! one would be a capability split exactly where the engine worked to avoid one.
//!
//! The reasoning was sound and the conclusion was still wrong, because it
//! answered a question about *this device* with a fact about a *class* of
//! devices. Parity is a property of the declared contract, not of the ceiling:
//! [`axiom_host::RenderCapability::HdrTargets`] makes the split explicit and
//! degradable, so an arm that cannot hold HDR renders the identical passes into
//! [`axiom_host::HostAttachmentFormat::Rgba8UnormSrgb`] and *says so*, while an
//! arm that can stops being held to the other's ceiling.
//!
//! # Why both usages, and not just the attachment
//!
//! An HDR intermediate that cannot be sampled is useless to this crate: every
//! pass downstream of the scene (the bright pass, both blurs, the composite) is a
//! fullscreen triangle that samples the previous target. A format that is
//! render-attachment-only would let the scene pass succeed and the chain that
//! consumes it fail — the same trap `surface_encode::scene_target_format` avoids
//! by requiring both usages before it upgrades the intermediate to sRGB.

use axiom_host::{BackendCapabilityProfile, FrameTonemap, HostAttachmentFormat, RenderCapability};

/// Whether this device genuinely has HDR colour targets: the half-float colour
/// format must be usable **both** as a render attachment and as a sampled
/// texture (see the module docs for why one without the other is not enough).
///
/// Both flags come from the adapter's reported format features, so a device that
/// wgpu fills in from an assumption of compliance and a device that measured its
/// own hardware are treated identically — whatever it claims is what gets
/// reported.
pub(crate) const fn device_hdr_targets(render_attachment: bool, sampled: bool) -> bool {
    render_attachment & sampled
}

/// `base` with [`RenderCapability::HdrTargets`] granted when the bound device
/// reported one — what a backend's profile becomes at bind.
///
/// It only ever **grants**. Every restriction a host set before the bind
/// survives it, because a device can add a capability it genuinely has and can
/// never take back one a host declined; the reverse — recomputing the profile
/// from scratch on bind — would silently undo an fps lever the moment the surface
/// came up.
pub(crate) fn grant_hdr_targets(
    base: BackendCapabilityProfile,
    device_has_hdr: bool,
) -> BackendCapabilityProfile {
    [base, base.with(RenderCapability::HdrTargets)][usize::from(device_has_hdr)]
}

/// The profile a backend holds **before any device has been bound** — the full
/// set with every *device-resolved* capability cleared.
///
/// A backend that has resolved nothing must not claim a capability it cannot
/// know without an adapter. There are two, and they are cleared here together
/// because they are the same kind of claim: [`RenderCapability::HdrTargets`],
/// granted by [`grant_hdr_targets`] from the adapter's reported format features,
/// and [`RenderCapability::GBuffer`], granted by
/// [`crate::gbuffer::grant_gbuffer`] from the device's colour-attachment limits.
/// Every other capability in the set is a property of *this source* — the
/// shaders and evaluators either exist or they do not — and so is knowable
/// without a device.
///
/// It is also the honest answer for the native off-screen capture path, whose
/// target is a single `Rgba8UnormSrgb` texture by construction rather than by
/// negotiation.
pub(crate) fn unresolved_capability_profile() -> BackendCapabilityProfile {
    BackendCapabilityProfile::all()
        .without(RenderCapability::HdrTargets)
        .without(RenderCapability::GBuffer)
}

/// **The one place the HDR present path is switched on**, and therefore the one
/// place it degrades.
///
/// Two facts have to agree. The app must have *asked* — a
/// [`FrameTonemap`] on its [`axiom_host::FrameRenderLook`] — because a float
/// scene target is not a free quality upgrade: nothing above display white is
/// clamped any more, so every value the 8-bit intermediate used to crush lands
/// somewhere else, and an app authored against the crush would silently
/// re-grade. And the device must be *able*, which is the capability the bind
/// granted.
///
/// Returns the tone map the present pass should run, or `None` for the 8-bit
/// chain — which is both the "app authored nothing" answer and the honest
/// degradation for a device without the attachment. A caller reads the presence
/// of a value as "allocate the float target", so the two answers cannot drift
/// apart into a float target nobody tone maps or a tone map with no headroom to
/// work in.
pub(crate) fn hdr_scene_tonemap(
    authored: Option<FrameTonemap>,
    profile: BackendCapabilityProfile,
) -> Option<FrameTonemap> {
    authored.filter(|_| profile.supports_attachment(HostAttachmentFormat::Rgba16Float))
}

/// **The other half of that degradation**: the scene-linear scale the 8-bit
/// chain must still apply when [`hdr_scene_tonemap`] declined the float target.
///
/// A [`FrameTonemap`] bundles two quantities that are not the same kind of
/// thing. The CURVE needs headroom - it exists to decide where a value above
/// display white lands, so without a float attachment there is nothing for it to
/// do and dropping it is honest. The EXPOSURE does not: it is a plain
/// scene-linear multiply, the stop the frame is metered at, and every backend
/// can apply a multiply. Dropping both because they arrived in one struct does
/// not degrade the picture, it MIS-EXPOSES it.
///
/// That is not hypothetical. An app cannot fold its own metering into its
/// authored values, because the engine types a light's intensity as a
/// [`axiom_kernel::Ratio`] - it cannot express a sun brighter than one, so it
/// normalizes and hands the scale here instead. Measured on `axiom-shmup`, whose
/// exposure is 9.5: the frame rendered on a profile without
/// [`axiom_host::RenderCapability::HdrTargets`] came out at a median luminance
/// of 70/255 against 161 on the same frame with it - not a softer picture, a
/// black one.
///
/// Returns `1.0` - an exact identity - whenever the tone map SURVIVED (the HDR
/// composite applies the exposure itself, and applying it twice would be as
/// wrong as not at all) and whenever the app authored none.
///
/// # Why the caller applies this in the scene pass, not the composite
///
/// Because the 8-bit target clamps at the STORE. By the time a composite sees
/// the frame, everything above display white has already been crushed to white
/// and the bottom of the range has been quantized to a handful of levels;
/// stretching that by 9.5 recovers no highlight and bands everything else.
/// Applied to the fragment's own radiance the scale lands before the clamp, so
/// the frame uses the full 8-bit range and only genuine highlights clip - which
/// is exactly the 8-bit chain's declared substitute.
pub(crate) fn ldr_scene_exposure(authored: Option<FrameTonemap>, caps: u32) -> f32 {
    authored
        .filter(|_| caps & (axiom_host::RenderCapability::HdrTargets as u32) == 0)
        .map_or(1.0, |t| t.exposure().get())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two halves of the degradation are keyed on the SAME question, and
    /// this pins them to it: the scene pass applies an exposure exactly when the
    /// present pass declined the curve. A drift either way is a visible bug -
    /// both applying it is a doubly-exposed frame, neither is a black one.
    #[test]
    fn the_scene_exposure_survives_exactly_when_the_tone_map_does_not() {
        let capable = BackendCapabilityProfile::all();
        let incapable = capable.without(RenderCapability::HdrTargets);
        let authored = FrameTonemap::blended(
            axiom_kernel::Ratio::new(1.0).expect("finite"),
            axiom_kernel::Ratio::new(9.5).expect("finite"),
        );

        // Curve kept -> the composite meters, so the scene pass must not.
        assert!(hdr_scene_tonemap(Some(authored), capable).is_some());
        assert_eq!(ldr_scene_exposure(Some(authored), capable.bits()), 1.0);

        // Curve dropped -> the scene pass is the only place left to meter.
        assert!(hdr_scene_tonemap(Some(authored), incapable).is_none());
        assert_eq!(ldr_scene_exposure(Some(authored), incapable.bits()), 9.5);
    }

    /// A frame that authored no tone map is untouched on every profile. This is
    /// the byte-identity guarantee every app written before the HDR path existed
    /// relies on: an identity multiply, not a nearly-identity one.
    #[test]
    fn a_frame_with_no_tone_map_is_never_re_exposed() {
        let capable = BackendCapabilityProfile::all();
        let incapable = capable.without(RenderCapability::HdrTargets);
        assert_eq!(ldr_scene_exposure(None, capable.bits()), 1.0);
        assert_eq!(ldr_scene_exposure(None, incapable.bits()), 1.0);
    }

    /// Both usages are required, and the truth table says so in full — the
    /// render-attachment-only case is the one that would compile, bind, and then
    /// fail on the first pass that samples the intermediate.
    #[test]
    fn hdr_needs_the_format_to_be_both_drawable_into_and_samplable() {
        assert!(device_hdr_targets(true, true));
        assert!(!device_hdr_targets(true, false));
        assert!(!device_hdr_targets(false, true));
        assert!(!device_hdr_targets(false, false));
    }

    /// What a bind does to the profile: exactly one bit, in one direction. An
    /// unbound backend claims no HDR, a device that has it grants it back, and a
    /// device that does not leaves the profile untouched — so nothing else about
    /// the backend changes with the arm it lands on.
    #[test]
    fn a_bind_grants_the_hdr_bit_and_changes_nothing_else() {
        let unbound = unresolved_capability_profile();
        assert!(!unbound.contains(RenderCapability::HdrTargets));
        // The other device-resolved bit is cleared too, and this grant does not
        // touch it: `grant_gbuffer` is a separate answer to a separate question.
        assert!(!unbound.contains(RenderCapability::GBuffer));
        let capable = grant_hdr_targets(unbound, true);
        let incapable = grant_hdr_targets(unbound, false);
        assert_eq!(
            capable,
            BackendCapabilityProfile::all().without(RenderCapability::GBuffer)
        );
        assert!(!capable.contains(RenderCapability::GBuffer));
        assert_eq!(incapable, unbound);
        assert_eq!(
            capable.bits() ^ incapable.bits(),
            RenderCapability::HdrTargets as u32
        );
        // The arm without it still attempts everything it always did — including
        // the bloom chain, which is the whole reason HDR is a separate bit rather
        // than a stricter reading of `Bloom`.
        assert!(incapable.contains(RenderCapability::Bloom));
        assert!(incapable.contains(RenderCapability::PostProcess));
        assert!(incapable.contains(RenderCapability::Shadows));
    }

    /// The grant is additive, never a recomputation: a host that narrowed the
    /// profile before the surface came up keeps every restriction it set. The
    /// opposite — rebuilding the profile from the device — would silently undo an
    /// fps lever at bind time, which is the kind of bug nobody would look for.
    #[test]
    fn a_host_restriction_survives_the_bind_that_grants_hdr() {
        let restricted = unresolved_capability_profile()
            .without(RenderCapability::Volumetrics)
            .without(RenderCapability::Sdf);
        let bound = grant_hdr_targets(restricted, true);
        assert!(bound.contains(RenderCapability::HdrTargets));
        assert!(!bound.contains(RenderCapability::Volumetrics));
        assert!(!bound.contains(RenderCapability::Sdf));
        assert_ne!(bound, BackendCapabilityProfile::all());
        // Granting twice is granting once — a rebind cannot drift the profile.
        assert_eq!(grant_hdr_targets(bound, true), bound);
    }

    /// **The opt-in and the capability, and the four combinations of them.**
    ///
    /// The two `None` rows are the ones that matter: an app that authored no
    /// tone map gets the 8-bit chain even on a device that could hold a float
    /// target (its pixels must not move), and an app that authored one gets the
    /// 8-bit chain on a device that cannot (it must not fail to bind).
    #[test]
    fn the_hdr_present_needs_both_the_apps_request_and_the_devices_capability() {
        let capable = grant_hdr_targets(unresolved_capability_profile(), true);
        let incapable = grant_hdr_targets(unresolved_capability_profile(), false);
        let filmic = FrameTonemap::filmic();
        assert_eq!(hdr_scene_tonemap(Some(filmic), capable), Some(filmic));
        assert_eq!(
            hdr_scene_tonemap(Some(filmic), incapable),
            None,
            "a device without a float attachment presents the 8-bit chain, it does not fail"
        );
        assert_eq!(
            hdr_scene_tonemap(None, capable),
            None,
            "capability is not consent: an app that authored no tone map keeps its pixels"
        );
        assert_eq!(hdr_scene_tonemap(None, incapable), None);
    }

    /// The gate the post chain will consult, end to end: the refusal that used to
    /// be a comment is now a question a profile answers, and the answer carries
    /// its own substitute.
    #[test]
    fn the_resolved_profile_answers_the_attachment_question_the_post_chain_asks() {
        use axiom_host::HostAttachmentFormat;

        let capable = grant_hdr_targets(unresolved_capability_profile(), true);
        let incapable = grant_hdr_targets(unresolved_capability_profile(), false);
        assert!(capable.supports_attachment(HostAttachmentFormat::Rgba16Float));
        assert!(!incapable.supports_attachment(HostAttachmentFormat::Rgba16Float));
        // The chain still runs on the refusing arm — into the target it already
        // used before any of this existed.
        let substitute = HostAttachmentFormat::Rgba16Float.ldr_substitute();
        assert_eq!(substitute, HostAttachmentFormat::Rgba8UnormSrgb);
        assert!(incapable.supports_attachment(substitute));
    }
}
