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

use axiom_host::{BackendCapabilityProfile, RenderCapability};

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
/// set with [`RenderCapability::HdrTargets`] cleared.
///
/// A backend that has resolved nothing must not claim the one capability it
/// cannot know without an adapter; the bit is granted only by
/// [`grant_hdr_targets`]. It is also the honest answer for the native off-screen
/// capture path, whose target is an `Rgba8UnormSrgb` texture by construction
/// rather than by negotiation.
pub(crate) fn unresolved_capability_profile() -> BackendCapabilityProfile {
    BackendCapabilityProfile::all().without(RenderCapability::HdrTargets)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let capable = grant_hdr_targets(unbound, true);
        let incapable = grant_hdr_targets(unbound, false);
        assert_eq!(capable, BackendCapabilityProfile::all());
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
