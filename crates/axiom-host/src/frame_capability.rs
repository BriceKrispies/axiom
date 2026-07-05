//! Backend **render capabilities** — the mechanism that lets one neutral frame drive
//! every renderer while no renderer is forced to attempt what it cannot do well, and
//! every feature a backend *would* skip is a declared, reported degradation rather
//! than a silent no-op.
//!
//! The frame always carries the full-richness scene (textured surfaces, alpha-cutout
//! foliage, normal-mapped detail, PCF shadows, volumetric light, SDF, …). Each
//! backend holds a [`BackendCapabilityProfile`] — the set of capabilities it will
//! *attempt*. The hardware GPU backends use [`BackendCapabilityProfile::all`]
//! (attempt everything); the Canvas 2D software rasterizer uses
//! [`BackendCapabilityProfile::canvas2d`], which drops the shader-only capabilities
//! (albedo sampling, alpha cutout, normal mapping) and substitutes the directional
//! PCF shadow with a cheaper planar contact shadow, while still running the CPU SDF
//! march and the CPU post effects. A backend consults its profile before realizing
//! an optional effect, so turning a capability off is a pure config change — the
//! content stays whole, and what a backend can't do is [`RenderCapability::degradation`]-ed
//! (a cheaper substitute or a reported drop), never dropped in silence.

/// A single render capability a backend may support. The discriminant is the bit the
/// capability occupies in a [`BackendCapabilityProfile`], so `cap as u32` is its mask
/// (no branching needed to test membership). The bit values are a stable contract:
/// the GPU main-pass WGSL reads the same `Textures`/`AlphaMask`/`NormalMapping`/`Shadows`
/// bits out of the frame's capability word (pinned by
/// `capability_bits_are_the_gpu_shader_contract`).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderCapability {
    /// Sampling a material's albedo image (vs. a flat fallback colour).
    Textures = 1 << 0,
    /// Per-fragment alpha masking / cutout sampled from a material texture (foliage
    /// leaf-alpha cards).
    AlphaMask = 1 << 1,
    /// Perturbing the geometric normal by a tangent-space normal map.
    NormalMapping = 1 << 2,
    /// The directional-light depth-map PCF shadow.
    Shadows = 1 << 3,
    /// SDF raymarch scene composited over the rasterized meshes.
    Sdf = 1 << 4,
    /// Screen-space volumetric light (god-ray) post-pass.
    Volumetrics = 1 << 5,
    /// A post-process stack (tone-map / bloom / colour grade).
    PostProcess = 1 << 6,
    /// The retro 32-bit console render profile (colour-depth quantize + ordered
    /// dither on the finished frame; low-res + nearest + vertex snap upstream).
    Retro32Bit = 1 << 7,
}

/// How a backend that lacks a [`RenderCapability`] degrades it. A capability is
/// never silently no-op'd: it is either rendered with a cheaper stand-in
/// ([`Self::Substitute`]) or omitted and reported ([`Self::Drop`]). This is the
/// declared policy the backends and the cross-backend parity proofs assert against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityDegradation {
    /// A cheaper stand-in is rendered in the capability's place (e.g. the PCF
    /// shadow is replaced by a planar contact shadow).
    Substitute,
    /// The capability is omitted from the frame and reported in the submission
    /// report's degraded-features list (e.g. albedo sampling → flat colour).
    Drop,
}

impl RenderCapability {
    /// The declared degradation for a backend that lacks this capability. Only the
    /// directional [`Self::Shadows`] has a cheaper stand-in (a planar contact
    /// shadow); every other capability degrades to an explicit, reported drop.
    pub const fn degradation(self) -> CapabilityDegradation {
        let is_substitutable = (self as u32) == (RenderCapability::Shadows as u32);
        [CapabilityDegradation::Drop, CapabilityDegradation::Substitute][is_substitutable as usize]
    }
}

/// Every known capability's bit, OR-ed together — the `all()` set.
const ALL_CAPABILITY_BITS: u32 = RenderCapability::Textures as u32
    | RenderCapability::AlphaMask as u32
    | RenderCapability::NormalMapping as u32
    | RenderCapability::Shadows as u32
    | RenderCapability::Sdf as u32
    | RenderCapability::Volumetrics as u32
    | RenderCapability::PostProcess as u32
    | RenderCapability::Retro32Bit as u32;

/// The set of render capabilities a backend will attempt. The hardware GPU backends
/// use [`Self::all`]; the Canvas 2D software backend uses [`Self::canvas2d`]. Restrict
/// any profile further (via [`Self::without`]) to shut specific capabilities off for a
/// backend that shouldn't attempt them (an fps/legibility lever).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendCapabilityProfile {
    bits: u32,
}

impl BackendCapabilityProfile {
    /// Every capability on — the hardware GPU backends' profile (attempt
    /// everything: textures, cutout, normal maps, PCF shadows, SDF, volumetrics,
    /// post-process, retro).
    pub const fn all() -> Self {
        BackendCapabilityProfile { bits: ALL_CAPABILITY_BITS }
    }

    /// No optional capabilities — a base-only backend.
    pub const fn none() -> Self {
        BackendCapabilityProfile { bits: 0 }
    }

    /// The Canvas 2D software rasterizer's real capability set: it rasterizes flat,
    /// so it drops the shader-only [`RenderCapability::Textures`],
    /// [`RenderCapability::AlphaMask`], and [`RenderCapability::NormalMapping`], and
    /// substitutes the directional [`RenderCapability::Shadows`] with a planar
    /// contact shadow — while still running the CPU [`RenderCapability::Sdf`] march
    /// and the neutral CPU post effects (volumetrics, post-process, retro). This is
    /// the profile the live Canvas 2D backend defaults to, so it degrades from the
    /// one full-richness frame instead of being handed a lesser scene.
    pub const fn canvas2d() -> Self {
        Self::all()
            .without(RenderCapability::Textures)
            .without(RenderCapability::AlphaMask)
            .without(RenderCapability::NormalMapping)
            .without(RenderCapability::Shadows)
    }

    /// Whether this profile will attempt `cap`.
    pub const fn contains(&self, cap: RenderCapability) -> bool {
        self.bits & (cap as u32) != 0
    }

    /// The raw capability mask (the OR of every attempted capability's bit). The GPU
    /// main-pass shader reads this word to gate its per-fragment features; see
    /// [`RenderCapability`].
    pub const fn bits(&self) -> u32 {
        self.bits
    }

    /// This profile with `cap` turned on.
    pub const fn with(self, cap: RenderCapability) -> Self {
        BackendCapabilityProfile { bits: self.bits | (cap as u32) }
    }

    /// This profile with `cap` turned off — the config lever for restricting a backend
    /// (e.g. `BackendCapabilityProfile::all().without(RenderCapability::Volumetrics)`).
    pub const fn without(self, cap: RenderCapability) -> Self {
        BackendCapabilityProfile { bits: self.bits & !(cap as u32) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAPS: [RenderCapability; 8] = [
        RenderCapability::Textures,
        RenderCapability::AlphaMask,
        RenderCapability::NormalMapping,
        RenderCapability::Shadows,
        RenderCapability::Sdf,
        RenderCapability::Volumetrics,
        RenderCapability::PostProcess,
        RenderCapability::Retro32Bit,
    ];

    #[test]
    fn all_contains_every_capability_none_contains_nothing() {
        let all = BackendCapabilityProfile::all();
        let none = BackendCapabilityProfile::none();
        CAPS.iter().for_each(|&c| {
            assert!(all.contains(c));
            assert!(!none.contains(c));
        });
        assert_ne!(all, none);
        assert_eq!(none.bits(), 0);
        assert_eq!(all.bits(), 0b1111_1111);
        assert!(format!("{all:?}").contains("BackendCapabilityProfile"));
        assert!(format!("{:?}", RenderCapability::Textures).contains("Textures"));
    }

    #[test]
    fn without_turns_one_off_leaving_the_rest_with_restores() {
        let p = BackendCapabilityProfile::all().without(RenderCapability::Volumetrics);
        assert!(!p.contains(RenderCapability::Volumetrics));
        // The other capabilities stay on.
        assert!(p.contains(RenderCapability::Sdf));
        assert!(p.contains(RenderCapability::Textures));
        // `with` restores it, back to the full set.
        assert_eq!(p.with(RenderCapability::Volumetrics), BackendCapabilityProfile::all());
        // Bits are distinct per capability (no two share a bit).
        let one = BackendCapabilityProfile::none().with(RenderCapability::AlphaMask);
        assert!(one.contains(RenderCapability::AlphaMask));
        assert!(!one.contains(RenderCapability::Sdf));
        assert_eq!(one.bits(), RenderCapability::AlphaMask as u32);
    }

    #[test]
    fn canvas2d_profile_drops_the_shader_features_and_keeps_the_cpu_ones() {
        let c = BackendCapabilityProfile::canvas2d();
        // The flat rasterizer cannot sample albedo, cutout, normal-map, or PCF-shadow.
        assert!(!c.contains(RenderCapability::Textures));
        assert!(!c.contains(RenderCapability::AlphaMask));
        assert!(!c.contains(RenderCapability::NormalMapping));
        assert!(!c.contains(RenderCapability::Shadows));
        // It still runs the CPU SDF march and the neutral CPU post effects.
        assert!(c.contains(RenderCapability::Sdf));
        assert!(c.contains(RenderCapability::Volumetrics));
        assert!(c.contains(RenderCapability::PostProcess));
        assert!(c.contains(RenderCapability::Retro32Bit));
        // It is a strict subset of the full GPU profile.
        assert_ne!(c, BackendCapabilityProfile::all());
        assert_eq!(c.bits() & !BackendCapabilityProfile::all().bits(), 0);
    }

    #[test]
    fn degradation_policy_is_substitute_only_for_shadows() {
        // The directional shadow degrades to a cheaper planar contact-shadow stand-in.
        assert_eq!(
            RenderCapability::Shadows.degradation(),
            CapabilityDegradation::Substitute
        );
        // Every other capability degrades to an explicit, reported drop.
        CAPS.iter()
            .filter(|&&c| (c as u32) != (RenderCapability::Shadows as u32))
            .for_each(|&c| assert_eq!(c.degradation(), CapabilityDegradation::Drop));
        assert_ne!(CapabilityDegradation::Substitute, CapabilityDegradation::Drop);
        assert!(format!("{:?}", CapabilityDegradation::Drop).contains("Drop"));
    }

    #[test]
    fn capability_bits_are_the_gpu_shader_contract() {
        // Pinned: the GPU main-pass WGSL hardcodes these masks (TEXTURES=1u, …).
        assert_eq!(RenderCapability::Textures as u32, 1);
        assert_eq!(RenderCapability::AlphaMask as u32, 2);
        assert_eq!(RenderCapability::NormalMapping as u32, 4);
        assert_eq!(RenderCapability::Shadows as u32, 8);
        assert_eq!(RenderCapability::Sdf as u32, 16);
        assert_eq!(RenderCapability::Volumetrics as u32, 32);
        assert_eq!(RenderCapability::PostProcess as u32, 64);
        assert_eq!(RenderCapability::Retro32Bit as u32, 128);
    }
}
