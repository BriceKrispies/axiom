//! Abstract **render-target attachment format** — the precision and channel
//! layout an off-screen pass writes into, named without naming a GPU API's
//! format enumeration.
//!
//! # Why this is a sibling of `HostColorFormat`, not more variants inside it
//!
//! [`crate::HostColorFormat`] answers one question: *what layout will a window
//! surface present in?* It is negotiated with a swap chain, it is always
//! display-encoded, and the two byte orders it lists are the two a browser
//! realistically offers. Every value it can hold is a legal answer to that
//! question, which is what makes it a closed, honest enum.
//!
//! An off-screen attachment is a different question with a disjoint answer set:
//! a depth buffer, a two-channel velocity target, a one-channel linear depth, a
//! four-channel HDR colour. None of those is something a surface can present
//! in. Folding them into `HostColorFormat` would let a
//! [`crate::HostSurfaceDescriptor`] ask a window to present in a depth format —
//! not a wider contract, a nonsensical one — and it would leave every consumer
//! of that type carrying arms it can never legally receive. So the two stay
//! apart, and the host layer keeps its rule intact: it names the abstraction,
//! the backend maps it onto whatever its device calls the same thing.
//!
//! # The discriminant is a bit
//!
//! Same trick as [`crate::RenderCapability`]: each variant's discriminant is the
//! bit it occupies, so the two questions a backend asks a format — *is this a
//! depth attachment?* and *does this need HDR colour targets?* — are mask tests
//! over a constant, not a match that grows an arm per format.

/// A render-target attachment format, in neutral terms. The discriminant is the
/// format's bit, so the predicates below are mask tests (see the module docs).
///
/// The set is exactly what the engine's multi-pass frame graph needs to be able
/// to name: an LDR colour target, HDR colour at two widths, the narrow
/// half-float pair a packed prepass channel wants, a single full-float channel,
/// and a depth attachment.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostAttachmentFormat {
    /// 8-bit RGBA, sRGB-encoded — the low-dynamic-range colour attachment every
    /// arm can render into and sample. This is what the GPU scene target and
    /// the whole post chain resolve to today, and it is the
    /// [`CapabilityDegradation::Substitute`](crate::CapabilityDegradation) every
    /// HDR format below degrades to.
    Rgba8UnormSrgb = 1 << 0,
    /// Four half-float channels: HDR colour that still holds a value above one
    /// after the pass that wrote it, so a later pass can rank two bright pixels
    /// against each other instead of finding both clamped to white.
    Rgba16Float = 1 << 1,
    /// Two half-float channels — a packed pair with no colour meaning: an
    /// octahedral normal, or a UV-space velocity delta.
    Rg16Float = 1 << 2,
    /// One full-float channel: a linear distance rather than a colour (view-space
    /// depth in metres, a shadow cascade's stored depth). Half-float is not
    /// enough here — its mantissa runs out well inside a scene's depth range.
    R32Float = 1 << 3,
    /// Four full-float channels: the format a reduction target wants, where the
    /// result is one number read by the next frame (a log-luminance average) and
    /// half-float rounding would quantize an exposure.
    Rgba32Float = 1 << 4,
    /// A full-float **depth** attachment. Bound to the depth slot, never the
    /// colour slot — which is why it is a variant of this type at all rather
    /// than something a colour-format enum has to pretend to hold.
    Depth32Float = 1 << 5,
}

/// The formats that are a depth attachment rather than a colour target. A mask,
/// so the test does not grow a branch when a stencil-carrying depth format is
/// added beside it.
const DEPTH_ATTACHMENT_BITS: u32 = HostAttachmentFormat::Depth32Float as u32;

/// The colour formats that need [`crate::RenderCapability::HdrTargets`].
///
/// Everything float and colour. [`HostAttachmentFormat::Rgba8UnormSrgb`] is
/// excluded because it is the universally-available target, and
/// [`HostAttachmentFormat::Depth32Float`] because a float *depth* buffer is
/// core on every arm this engine binds — it is never sampled as colour and
/// never carries a value the colour pipeline has to hold.
const HDR_ATTACHMENT_BITS: u32 = HostAttachmentFormat::Rgba16Float as u32
    | HostAttachmentFormat::Rg16Float as u32
    | HostAttachmentFormat::R32Float as u32
    | HostAttachmentFormat::Rgba32Float as u32;

impl HostAttachmentFormat {
    /// Whether this attachment binds to the depth slot rather than a colour slot.
    pub const fn is_depth(self) -> bool {
        (self as u32) & DEPTH_ATTACHMENT_BITS != 0
    }

    /// Whether a backend must hold [`crate::RenderCapability::HdrTargets`] to use
    /// this attachment. Ask
    /// [`BackendCapabilityProfile::supports_attachment`](crate::BackendCapabilityProfile::supports_attachment)
    /// rather than this directly when a profile is in hand — that is the gate a
    /// backend consults, and it answers for both kinds of format at once.
    pub const fn requires_hdr_targets(self) -> bool {
        (self as u32) & HDR_ATTACHMENT_BITS != 0
    }

    /// The attachment a backend **without** HDR targets uses in this one's place:
    /// [`crate::CapabilityDegradation::Substitute`] expressed as a value rather
    /// than as prose.
    ///
    /// Every HDR colour format collapses to [`Self::Rgba8UnormSrgb`]; the LDR
    /// colour target and the depth attachment are their own substitutes. This is
    /// what makes the degradation checkable: a pass declares the attachment it
    /// wants, and the arm that cannot hold it renders the identical pass into the
    /// substitute rather than being skipped.
    pub const fn ldr_substitute(self) -> Self {
        [self, HostAttachmentFormat::Rgba8UnormSrgb][self.requires_hdr_targets() as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [HostAttachmentFormat; 6] = [
        HostAttachmentFormat::Rgba8UnormSrgb,
        HostAttachmentFormat::Rgba16Float,
        HostAttachmentFormat::Rg16Float,
        HostAttachmentFormat::R32Float,
        HostAttachmentFormat::Rgba32Float,
        HostAttachmentFormat::Depth32Float,
    ];

    /// Each variant owns its own bit — the property both mask predicates rest on.
    /// A duplicated discriminant would silently make one format answer another's
    /// question, so it is pinned by counting set bits rather than by eyeballing
    /// the literals.
    #[test]
    fn every_format_owns_a_distinct_bit() {
        let union = ALL.iter().fold(0_u32, |acc, &f| acc | f as u32);
        assert_eq!(union.count_ones() as usize, ALL.len());
        assert_eq!(union, 0b11_1111);
        ALL.windows(2).for_each(|w| assert_ne!(w[0], w[1]));
        assert_eq!(HostAttachmentFormat::Rgba16Float, ALL[1]);
        assert!(format!("{:?}", ALL[1]).contains("Rgba16Float"));
    }

    /// The depth attachment is the only one bound to the depth slot, and it is
    /// deliberately *not* an HDR format: a float depth buffer is core on every
    /// arm, so gating it behind the HDR capability would make cascaded shadows
    /// unavailable on a device that can render them perfectly well.
    #[test]
    fn depth_is_the_only_depth_slot_and_needs_no_hdr_capability() {
        assert!(HostAttachmentFormat::Depth32Float.is_depth());
        assert!(!HostAttachmentFormat::Depth32Float.requires_hdr_targets());
        ALL.iter()
            .filter(|&&f| f != HostAttachmentFormat::Depth32Float)
            .for_each(|&f| assert!(!f.is_depth(), "{f:?} is a colour target"));
    }

    /// The line the capability draws: the 8-bit colour target is universal, every
    /// float colour target is not.
    #[test]
    fn every_float_colour_target_requires_the_hdr_capability() {
        assert!(!HostAttachmentFormat::Rgba8UnormSrgb.requires_hdr_targets());
        [
            HostAttachmentFormat::Rgba16Float,
            HostAttachmentFormat::Rg16Float,
            HostAttachmentFormat::R32Float,
            HostAttachmentFormat::Rgba32Float,
        ]
        .iter()
        .for_each(|&f| assert!(f.requires_hdr_targets(), "{f:?} holds values above one"));
    }

    /// The substitute is total — every format has one — and it is a fixed point:
    /// substituting twice cannot land anywhere a backend still could not render.
    #[test]
    fn the_substitute_collapses_hdr_to_the_universal_target_and_is_a_fixed_point() {
        assert_eq!(
            HostAttachmentFormat::Rgba16Float.ldr_substitute(),
            HostAttachmentFormat::Rgba8UnormSrgb
        );
        assert_eq!(
            HostAttachmentFormat::R32Float.ldr_substitute(),
            HostAttachmentFormat::Rgba8UnormSrgb
        );
        // Depth substitutes to itself: it was never the thing that needed HDR.
        assert_eq!(
            HostAttachmentFormat::Depth32Float.ldr_substitute(),
            HostAttachmentFormat::Depth32Float
        );
        ALL.iter().for_each(|&f| {
            let once = f.ldr_substitute();
            assert!(!once.requires_hdr_targets(), "{f:?} substituted to an HDR format");
            assert_eq!(once.ldr_substitute(), once, "{f:?} did not settle");
            // A colour target never substitutes into the depth slot, and depth
            // never substitutes out of it.
            assert_eq!(once.is_depth(), f.is_depth());
        });
    }
}
