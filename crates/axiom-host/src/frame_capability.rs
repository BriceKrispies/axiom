//! Backend **render capabilities** — the mechanism that lets one neutral frame drive
//! every renderer while no renderer is forced to attempt what it cannot do well.
//!
//! The frame always carries the full-richness scene (dense foliage, volumetric light,
//! SDF, …). Each backend holds a [`BackendCapabilityProfile`] — the set of capabilities
//! it will *attempt*. Every backend defaults to [`BackendCapabilityProfile::all`]
//! (attempt everything); in practice only the Canvas 2D software rasterizer is
//! configured with a restricted profile, so it **skips** the capabilities it can't do
//! legibly/fast (keeping it readable at high fps) while the WebGPU / WebGL2 backends keep
//! the full set. A backend consults its profile before realizing an optional effect, so
//! turning a capability off is a pure config change — the content stays whole.

/// A single render capability a backend may support. The discriminant is the bit the
/// capability occupies in a [`BackendCapabilityProfile`], so `cap as u32` is its mask
/// (no branching needed to test membership).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderCapability {
    /// Screen-space volumetric light (god-ray) post-pass.
    Volumetrics = 1 << 0,
    /// SDF raymarch scene composited over the rasterized meshes.
    Sdf = 1 << 1,
    /// Per-fragment alpha masking / cutout sampled from a material texture.
    AlphaMask = 1 << 2,
    /// Dense instanced detail geometry (e.g. foliage / litter at high instance counts).
    DetailInstancing = 1 << 3,
    /// A post-process stack (tone-map / bloom / colour grade).
    PostProcess = 1 << 4,
    /// The retro 32-bit console render profile (low-res + nearest, vertex snap, flat
    /// passthrough, distance fog, colour-depth quantize + ordered dither).
    Retro32Bit = 1 << 5,
}

/// Every known capability's bit, OR-ed together — the `all()` set.
const ALL_CAPABILITY_BITS: u32 = RenderCapability::Volumetrics as u32
    | RenderCapability::Sdf as u32
    | RenderCapability::AlphaMask as u32
    | RenderCapability::DetailInstancing as u32
    | RenderCapability::PostProcess as u32
    | RenderCapability::Retro32Bit as u32;

/// The set of render capabilities a backend will attempt. Default for every backend is
/// [`Self::all`]; restrict it (via [`Self::without`]) to shut specific capabilities off
/// for a backend that shouldn't attempt them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendCapabilityProfile {
    bits: u32,
}

impl BackendCapabilityProfile {
    /// Every capability on — the default profile (attempt everything). This is what the
    /// WebGPU and WebGL2 backends use.
    pub const fn all() -> Self {
        BackendCapabilityProfile { bits: ALL_CAPABILITY_BITS }
    }

    /// No optional capabilities — a base-only backend.
    pub const fn none() -> Self {
        BackendCapabilityProfile { bits: 0 }
    }

    /// Whether this profile will attempt `cap`.
    pub const fn contains(&self, cap: RenderCapability) -> bool {
        self.bits & (cap as u32) != 0
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

    const CAPS: [RenderCapability; 6] = [
        RenderCapability::Volumetrics,
        RenderCapability::Sdf,
        RenderCapability::AlphaMask,
        RenderCapability::DetailInstancing,
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
        assert!(format!("{all:?}").contains("BackendCapabilityProfile"));
        assert!(format!("{:?}", RenderCapability::Volumetrics).contains("Volumetrics"));
    }

    #[test]
    fn without_turns_one_off_leaving_the_rest_with_restores() {
        let p = BackendCapabilityProfile::all().without(RenderCapability::Volumetrics);
        assert!(!p.contains(RenderCapability::Volumetrics));
        // The other capabilities stay on.
        assert!(p.contains(RenderCapability::Sdf));
        assert!(p.contains(RenderCapability::DetailInstancing));
        // `with` restores it, back to the full set.
        assert_eq!(p.with(RenderCapability::Volumetrics), BackendCapabilityProfile::all());
        // Bits are distinct per capability (no two share a bit).
        let one = BackendCapabilityProfile::none().with(RenderCapability::AlphaMask);
        assert!(one.contains(RenderCapability::AlphaMask));
        assert!(!one.contains(RenderCapability::Sdf));
    }
}
