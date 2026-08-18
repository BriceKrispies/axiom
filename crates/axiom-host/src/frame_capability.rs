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

use crate::HostAttachmentFormat;

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
    /// A gradient sky with a celestial body in it, evaluated per pixel behind the
    /// scene ([`crate::FrameSky`]) instead of a flat clear colour.
    Sky = 1 << 8,
    /// A specular highlight term on lit materials — the second half of "this
    /// surface is being lit by something", which a Lambert-only shade cannot
    /// express however carefully its light values are tuned.
    Specular = 1 << 9,
    /// Bloom: bright pixels spilling into their neighbours through a blurred
    /// bright pass ([`crate::FrameBloom`]).
    ///
    /// Deliberately **not** folded into [`Self::PostProcess`]. That capability is
    /// the whole-image colour grade, which the Canvas 2D backend genuinely
    /// performs (a CPU loop over the finished framebuffer). Bloom is a different
    /// thing with a different cost: a bright pass and two blur passes through
    /// extra render targets, which the software rasterizer does not have. One bit
    /// covering both would force a backend that does one and not the other to
    /// either lie or drop the one it can do.
    Bloom = 1 << 10,
    /// Aerial perspective evaluated on the fragment's **world distance** from the
    /// camera — the Beer–Lambert extinction term of [`crate::FrameDepthFog`] —
    /// rather than on its normalized device depth alone.
    ///
    /// The split is not taste, it is what each backend physically holds. The GPU
    /// mesh pass interpolates a world position and the SDF pass marches one, so
    /// both can measure the metres of air a fragment is seen through. The Canvas
    /// 2D fog is a **post-pass over the finished image**, and all it has at that
    /// point is the z-buffer — a normalized depth, hyperbolic in distance, with
    /// no frustum constants beside it to invert. It therefore keeps the
    /// normalized-depth ramp it has always run, which is why this is the second
    /// [`CapabilityDegradation::Substitute`] and not a drop: nothing is missing
    /// from that arm, the same fog is evaluated in a coarser parameterization.
    AerialPerspective = 1 << 11,
    /// Shading a draw through an authored **procedural surface** — an
    /// [`crate::FrameDrawItem::surface_program`] naming a field-authored
    /// appearance description whose channels are expressions rather than
    /// constants.
    ///
    /// Appended, never renumbered: the eleven bits above are hardcoded as the
    /// same masks in the GPU main-pass WGSL, which is what
    /// `capability_bits_are_the_gpu_shader_contract` pins.
    ///
    /// Its degradation is a [`CapabilityDegradation::Substitute`], and the
    /// substitution is **per-triangle instead of per-fragment**. The Canvas 2D
    /// software backend genuinely renders a procedural surface: it cannot
    /// execute a program, but it can evaluate the surface's channel expressions
    /// on the CPU at each triangle's object-space centroid, which is the same
    /// fidelity relationship every other capability has on that backend. A
    /// backend that has neither a program nor a CPU evaluator omits it and
    /// reports [`crate::FrameFeature::ProceduralSurface`].
    ProceduralSurface = 1 << 12,
    /// **High-dynamic-range render targets**: colour attachments that hold values
    /// above `1.0` at more than eight bits a channel, so a fragment that emitted
    /// `4.0` is still `4.0` when a later pass samples it.
    ///
    /// Deliberately **not** folded into [`Self::Bloom`], for exactly the reason
    /// `Bloom` is not folded into [`Self::PostProcess`]. The GPU backend genuinely
    /// blooms today, and it blooms an 8-bit sRGB intermediate: the bright pass,
    /// the two blur passes and the composite all run, and the halo is real. What
    /// it cannot do is rank two blown highlights against each other, because both
    /// were clamped to white before the chain ever sampled them. One bit covering
    /// "blooms" and "blooms with headroom" would force that arm to either lie
    /// about the bloom it performs or drop the bloom it performs — the same trap
    /// [`crate::FrameBloom`]'s own docs describe.
    ///
    /// This bit exists because what it replaces was a **policy, not a
    /// measurement**. The engine requests WebGL2 downlevel limits on both browser
    /// arms to hold them at capability parity, half-float render targets are not
    /// guaranteed under those limits, and so the GPU post chain simply declined to
    /// ask for one. That preserved parity by making the ceiling *invisible*:
    /// nothing in the frame contract said the headroom was missing, no backend
    /// could report that it was, and a device that could hold an HDR target was
    /// held to the one that could not. Parity is kept by **declaring** a split,
    /// not by pretending it is absent — which is the move every capability above
    /// already made.
    ///
    /// Its degradation is a [`CapabilityDegradation::Substitute`], and the
    /// substitute is a value rather than prose:
    /// [`crate::HostAttachmentFormat::ldr_substitute`] maps every HDR attachment
    /// to [`crate::HostAttachmentFormat::Rgba8UnormSrgb`]. Nothing is omitted —
    /// the identical passes run into a coarser target — which is the same
    /// relationship [`Self::AerialPerspective`] has with the normalized-depth fog
    /// ramp.
    ///
    /// There is deliberately **no [`crate::FrameFeature`] peer yet.** A per-frame
    /// degradation report is keyed on the frame having authored something the
    /// backend could not honour, and no frame names an attachment format today —
    /// reporting one unconditionally would fire on every frame in the engine,
    /// which is the failure mode `FramePacket::uses_specular` exists to forbid.
    /// The reportable signal arrives with the pass vocabulary that carries an
    /// attachment request.
    ///
    /// Appended above every bit the GPU main-pass WGSL reads (that shader reads
    /// nothing above `2048`), so no existing mask moved.
    HdrTargets = 1 << 13,
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

/// The capabilities that have a cheaper stand-in rather than an omission: the
/// directional [`RenderCapability::Shadows`] (a planar contact shadow),
/// [`RenderCapability::AerialPerspective`] (the normalized-depth fog ramp),
/// [`RenderCapability::ProceduralSurface`] (the surface's channels evaluated per
/// triangle rather than per fragment) and [`RenderCapability::HdrTargets`] (the
/// same passes rendered into [`crate::HostAttachmentFormat::Rgba8UnormSrgb`]). A
/// mask rather than a chain of comparisons, so the set grows without the test
/// growing a branch.
const SUBSTITUTED_CAPABILITY_BITS: u32 = RenderCapability::Shadows as u32
    | RenderCapability::AerialPerspective as u32
    | RenderCapability::ProceduralSurface as u32
    | RenderCapability::HdrTargets as u32;

impl RenderCapability {
    /// The declared degradation for a backend that lacks this capability. The
    /// capabilities in [`SUBSTITUTED_CAPABILITY_BITS`] have a cheaper stand-in;
    /// every other capability degrades to an explicit, reported drop.
    pub const fn degradation(self) -> CapabilityDegradation {
        let is_substitutable = (self as u32) & SUBSTITUTED_CAPABILITY_BITS != 0;
        [
            CapabilityDegradation::Drop,
            CapabilityDegradation::Substitute,
        ][is_substitutable as usize]
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
    | RenderCapability::Retro32Bit as u32
    | RenderCapability::Sky as u32
    | RenderCapability::Specular as u32
    | RenderCapability::Bloom as u32
    | RenderCapability::AerialPerspective as u32
    | RenderCapability::ProceduralSurface as u32
    | RenderCapability::HdrTargets as u32;

/// The set of render capabilities a backend will attempt. The hardware GPU backends
/// use [`Self::all`]; the Canvas 2D software backend uses [`Self::canvas2d`]. Restrict
/// any profile further (via [`Self::without`]) to shut specific capabilities off for a
/// backend that shouldn't attempt them (an fps/legibility lever).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendCapabilityProfile {
    bits: u32,
}

impl BackendCapabilityProfile {
    /// **Every capability on** — the full set, and the upper bound every other
    /// profile is a subset of (textures, cutout, normal maps, PCF shadows, SDF,
    /// volumetrics, post-process, retro, sky, specular, bloom, aerial
    /// perspective, procedural surfaces, HDR targets).
    ///
    /// This is the *set*, not a backend's answer. A hardware GPU backend starts
    /// here and clears what its device did not actually resolve —
    /// [`RenderCapability::HdrTargets`] in particular is a property of the
    /// adapter, not of the code path, so a backend that claimed it unconditionally
    /// would be reporting a policy instead of a measurement.
    pub const fn all() -> Self {
        BackendCapabilityProfile {
            bits: ALL_CAPABILITY_BITS,
        }
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
    ///
    /// It also drops the three **shader-and-render-target** capabilities the
    /// software path has no answer for: [`RenderCapability::Sky`] (a per-pixel
    /// radiance evaluation behind the whole scene — the flat rasterizer clears to
    /// one colour), [`RenderCapability::Specular`] (its shading is per-triangle
    /// and view-independent, so there is no fragment normal to catch a highlight
    /// with), and [`RenderCapability::Bloom`] (a bright pass plus two blur passes
    /// through extra render targets). Each is a *declared, reported* drop — the
    /// frame still carries all three, and the Canvas 2D report enumerates what it
    /// could not honour.
    ///
    /// It also drops [`RenderCapability::AerialPerspective`] — the *second*
    /// substitute alongside the directional shadow. Its fog is a post-pass over
    /// the finished image with only a z-buffer to read, so it evaluates the
    /// frame's [`crate::FrameDepthFog`] in normalized depth exactly as it always
    /// has, while the GPU arms additionally evaluate the extinction term on the
    /// world distance they have. Same authored fog, coarser parameterization.
    ///
    /// It **keeps** [`RenderCapability::ProceduralSurface`], the third
    /// substitute. The flat rasterizer cannot execute a program, but a surface's
    /// channels are field expressions and a field is a pure function the CPU can
    /// evaluate — so this backend shades an authored surface at each triangle's
    /// object-space centroid instead of at each fragment. That is a coarser
    /// sampling of the same authored appearance, not a missing one, which is
    /// exactly what a substitute is.
    ///
    /// It drops [`RenderCapability::HdrTargets`] too, and for the most literal
    /// reason of any of these: this backend has no render targets at all. Its
    /// framebuffer is a `Vec<u8>` of display-encoded bytes — the shape
    /// [`crate::apply_frame_postprocess`] loops over — so a value above one has
    /// nowhere to be stored between passes, whatever the device underneath is
    /// capable of. The substitute is the target it already writes,
    /// [`crate::HostAttachmentFormat::Rgba8UnormSrgb`].
    pub const fn canvas2d() -> Self {
        Self::all()
            .without(RenderCapability::Textures)
            .without(RenderCapability::AlphaMask)
            .without(RenderCapability::NormalMapping)
            .without(RenderCapability::Shadows)
            .without(RenderCapability::Sky)
            .without(RenderCapability::Specular)
            .without(RenderCapability::Bloom)
            .without(RenderCapability::AerialPerspective)
            .without(RenderCapability::HdrTargets)
    }

    /// Whether this profile will attempt `cap`.
    pub const fn contains(&self, cap: RenderCapability) -> bool {
        self.bits & (cap as u32) != 0
    }

    /// **Whether this profile may render into `format`** — the gate that replaces
    /// the GPU post chain's hardcoded refusal of a half-float intermediate.
    ///
    /// An HDR attachment needs [`RenderCapability::HdrTargets`]; the 8-bit colour
    /// target and the depth attachment are available on every arm. A backend asks
    /// its own declared profile here, so what it can hold is a fact carried in
    /// data — set from what its device actually resolved — instead of a comment
    /// asserting what a class of devices probably cannot do. When the answer is
    /// `false`, the pass still runs: it renders into
    /// [`HostAttachmentFormat::ldr_substitute`], which is the declared
    /// [`CapabilityDegradation::Substitute`].
    pub const fn supports_attachment(&self, format: HostAttachmentFormat) -> bool {
        self.contains(RenderCapability::HdrTargets) | !format.requires_hdr_targets()
    }

    /// The raw capability mask (the OR of every attempted capability's bit). The GPU
    /// main-pass shader reads this word to gate its per-fragment features; see
    /// [`RenderCapability`].
    pub const fn bits(&self) -> u32 {
        self.bits
    }

    /// This profile with `cap` turned on.
    pub const fn with(self, cap: RenderCapability) -> Self {
        BackendCapabilityProfile {
            bits: self.bits | (cap as u32),
        }
    }

    /// This profile with `cap` turned off — the config lever for restricting a backend
    /// (e.g. `BackendCapabilityProfile::all().without(RenderCapability::Volumetrics)`).
    pub const fn without(self, cap: RenderCapability) -> Self {
        BackendCapabilityProfile {
            bits: self.bits & !(cap as u32),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAPS: [RenderCapability; 14] = [
        RenderCapability::Textures,
        RenderCapability::AlphaMask,
        RenderCapability::NormalMapping,
        RenderCapability::Shadows,
        RenderCapability::Sdf,
        RenderCapability::Volumetrics,
        RenderCapability::PostProcess,
        RenderCapability::Retro32Bit,
        RenderCapability::Sky,
        RenderCapability::Specular,
        RenderCapability::Bloom,
        RenderCapability::AerialPerspective,
        RenderCapability::ProceduralSurface,
        RenderCapability::HdrTargets,
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
        assert_eq!(all.bits(), 0b11_1111_1111_1111);
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
        assert_eq!(
            p.with(RenderCapability::Volumetrics),
            BackendCapabilityProfile::all()
        );
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
        // Nor can it evaluate a per-pixel sky, catch a view-dependent highlight,
        // or afford the bloom chain's extra render targets.
        assert!(!c.contains(RenderCapability::Sky));
        assert!(!c.contains(RenderCapability::Specular));
        assert!(!c.contains(RenderCapability::Bloom));
        // Nor can its post-pass fog measure world distance — it has a z-buffer
        // and nothing else — so the extinction term is substituted by the
        // normalized-depth ramp it already ran.
        assert!(!c.contains(RenderCapability::AerialPerspective));
        // It DOES attempt a procedural surface: it evaluates the surface's field
        // expressions on the CPU, once per triangle at the object-space centroid,
        // instead of per fragment. Coarser sampling of the same authored
        // appearance — the third substitute, not a fourth drop.
        assert!(c.contains(RenderCapability::ProceduralSurface));
        assert_eq!(
            RenderCapability::ProceduralSurface.degradation(),
            CapabilityDegradation::Substitute
        );
        // And it has no render targets at all — its framebuffer is a byte
        // vector — so an HDR attachment has nowhere to live between passes.
        assert!(!c.contains(RenderCapability::HdrTargets));
        // It still runs the CPU SDF march and the neutral CPU post effects. In
        // particular the whole-image colour grade survives: `PostProcess` is the
        // grade, not the bloom, which is exactly why they are separate bits.
        assert!(c.contains(RenderCapability::Sdf));
        assert!(c.contains(RenderCapability::Volumetrics));
        assert!(c.contains(RenderCapability::PostProcess));
        assert!(c.contains(RenderCapability::Retro32Bit));
        // It is a strict subset of the full GPU profile.
        assert_ne!(c, BackendCapabilityProfile::all());
        assert_eq!(c.bits() & !BackendCapabilityProfile::all().bits(), 0);
    }

    #[test]
    fn degradation_policy_is_substitute_only_for_the_declared_stand_ins() {
        // The directional shadow degrades to a cheaper planar contact-shadow stand-in.
        assert_eq!(
            RenderCapability::Shadows.degradation(),
            CapabilityDegradation::Substitute
        );
        // The distance-based fog term degrades to the normalized-depth ramp.
        assert_eq!(
            RenderCapability::AerialPerspective.degradation(),
            CapabilityDegradation::Substitute
        );
        // An authored procedural surface degrades to a per-triangle CPU
        // evaluation of the same channels.
        assert_eq!(
            RenderCapability::ProceduralSurface.degradation(),
            CapabilityDegradation::Substitute
        );
        // An HDR attachment degrades to the 8-bit sRGB target the whole engine
        // already renders into: the passes still run, at coarser precision.
        assert_eq!(
            RenderCapability::HdrTargets.degradation(),
            CapabilityDegradation::Substitute
        );
        // Every other capability degrades to an explicit, reported drop.
        CAPS.iter()
            .filter(|&&c| (c as u32) & SUBSTITUTED_CAPABILITY_BITS == 0)
            .for_each(|&c| assert_eq!(c.degradation(), CapabilityDegradation::Drop));
        assert_ne!(
            CapabilityDegradation::Substitute,
            CapabilityDegradation::Drop
        );
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
        assert_eq!(RenderCapability::Sky as u32, 256);
        assert_eq!(RenderCapability::Specular as u32, 512);
        assert_eq!(RenderCapability::Bloom as u32, 1024);
        assert_eq!(RenderCapability::AerialPerspective as u32, 2048);
        // Appended in 07-backend-lowering, above every bit the WGSL reads, so no
        // existing mask moved and the cross-language contract above still holds.
        assert_eq!(RenderCapability::ProceduralSurface as u32, 4096);
        // Appended above every mask the WGSL reads, for the same reason bit 12
        // was: the cross-language contract above is unchanged by adding it.
        assert_eq!(RenderCapability::HdrTargets as u32, 8192);
        // Every bit is distinct: the OR of all of them has as many set bits as
        // there are capabilities, which a duplicated discriminant would break.
        assert_eq!(
            BackendCapabilityProfile::all().bits().count_ones() as usize,
            CAPS.len()
        );
    }

    /// The whole point of the bit: a backend asks its profile which attachments
    /// it may render into, instead of a comment asserting what a class of
    /// devices probably cannot do.
    #[test]
    fn the_attachment_gate_follows_the_hdr_bit_and_nothing_else() {
        let hdr = BackendCapabilityProfile::all();
        let ldr = hdr.without(RenderCapability::HdrTargets);
        let formats = [
            HostAttachmentFormat::Rgba8UnormSrgb,
            HostAttachmentFormat::Rgba16Float,
            HostAttachmentFormat::Rg16Float,
            HostAttachmentFormat::R32Float,
            HostAttachmentFormat::Rgba32Float,
            HostAttachmentFormat::Depth32Float,
        ];
        // A device that resolved HDR targets may render into every attachment.
        formats
            .iter()
            .for_each(|&f| assert!(hdr.supports_attachment(f), "{f:?} was refused"));
        // Without the bit, exactly the float colour targets are refused — the
        // 8-bit target and the depth buffer are still available, so a shadow
        // cascade and the existing post chain are unaffected.
        assert!(ldr.supports_attachment(HostAttachmentFormat::Rgba8UnormSrgb));
        assert!(ldr.supports_attachment(HostAttachmentFormat::Depth32Float));
        assert!(!ldr.supports_attachment(HostAttachmentFormat::Rgba16Float));
        assert!(!ldr.supports_attachment(HostAttachmentFormat::R32Float));
        // And the refusal is never a dead end: the substitute is always something
        // the same profile accepts, which is what makes this a Substitute rather
        // than a Drop.
        formats.iter().for_each(|&f| {
            assert!(
                ldr.supports_attachment(f.ldr_substitute()),
                "{f:?} substituted to something the same backend still refuses"
            );
        });
        // The Canvas 2D software rasterizer is on the refusing side of the line.
        assert!(!BackendCapabilityProfile::canvas2d()
            .supports_attachment(HostAttachmentFormat::Rgba16Float));
    }
}
