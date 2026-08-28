//! The backend-neutral, uniform submission report returned by every render
//! backend.
//!
//! `FrameSubmissionReport` is the single observable result of presenting one
//! [`crate::FramePacket`]. The GPU backend returns it (with no degradation); the
//! Canvas 2D software backend returns it with its dropped/approximated features
//! enumerated and its per-frame rasterization stats attached. It carries only
//! primitives, neutral enums, and the neutral [`FrameRasterStats`] block — no
//! GPU or browser API names — so the host contract stays platform-free.

use crate::FrameRasterStats;

/// Which backend produced a frame. Neutral identities, deliberately free of
/// graphics-API spellings: `GpuPrimary` is the primary hardware path,
/// `GpuFallback` the secondary hardware path, and `Canvas2d` the software
/// last-resort path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendKind {
    /// The primary hardware GPU path.
    GpuPrimary,
    /// The secondary hardware GPU path.
    GpuFallback,
    /// The software, last-resort 2D path.
    Canvas2d,
}

/// A capability a backend may have to drop or approximate when presenting a
/// frame, reported per frame so degradation is observable to apps and
/// telemetry. Neutral names — no graphics-API terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameFeature {
    /// Cast/received shadows.
    Shadows,
    /// Sampling a material's albedo image (vs. a flat fallback colour).
    AlbedoSampling,
    /// Point-light distance falloff.
    PointLightFalloff,
    /// Perspective-correct albedo interpolation across a primitive.
    PerspectiveCorrectAlbedo,
    /// More than one light contributing to a fragment.
    MultiLight,
    /// Any post-processing pass.
    PostProcessing,
    /// A per-pixel sky evaluated behind the scene (a backend without it clears to
    /// the frame's flat colour instead).
    Sky,
    /// The view-dependent specular highlight term on lit surfaces.
    SpecularHighlight,
    /// Bright pixels spilling into their neighbours. Distinct from
    /// [`Self::PostProcessing`]: a backend can grade a frame it cannot bloom.
    Bloom,
    /// The depth fog's Beer–Lambert term, evaluated on the fragment's world
    /// distance. A backend without it substitutes the same fog's
    /// normalized-depth ramp (see
    /// [`RenderCapability::AerialPerspective`](crate::RenderCapability)).
    AerialPerspective,
    /// An authored procedural surface (a draw's
    /// [`FrameDrawItem::surface_program`](crate::FrameDrawItem)) that this
    /// backend could not honour. A backend that has neither a program nor a CPU
    /// evaluator for the surface's channels renders its constant fallback colour
    /// and reports this; a backend that evaluates the channels per triangle
    /// instead of per fragment reports nothing, because it honoured them (see
    /// [`RenderCapability::ProceduralSurface`](crate::RenderCapability)).
    ProceduralSurface,
    /// The per-fragment **ornament** of a runtime material surface — parallax
    /// occlusion, de-tiling, procedural weathering, repair patches, cloth and
    /// macro relief — traded away for fill rate.
    ///
    /// Distinct from [`Self::ProceduralSurface`], and the two are different
    /// facts: that one says the backend could not honour the authored surface at
    /// all and rendered a fallback; this one says it honoured it, at the
    /// *identity* fidelity the source's own lean tier defines — projection,
    /// albedo, tint, roughness, metalness, normal map, micro detail, macro bands
    /// and the vertex-mask lane, with the decoration stacked on top of them
    /// omitted. See [`RenderCapability::SurfaceOrnament`](crate::RenderCapability).
    ///
    /// Raised only by a frame that actually draws with a program composed that
    /// way, never as a standing property of the backend.
    SurfaceOrnament,
}

/// The uniform result of presenting one frame through any backend: which backend
/// ran, the frame identity, draw accounting, the features dropped or
/// approximated, and the neutral per-frame rasterization stats
/// ([`FrameRasterStats`], all zero for a non-rasterizing hardware backend). Two
/// reports are equal iff every field is equal.
///
/// `critical_coverage_skipped` is the invariant: visible critical coverage
/// (terrain/ground) is degraded to a cheaper representation or decimated, never
/// dropped, so it is zero in every healthy frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameSubmissionReport {
    backend: BackendKind,
    frame_index: u64,
    tick: u64,
    submitted_draws: u32,
    skipped_draws: u32,
    critical_coverage_skipped: u32,
    degraded_materials: u32,
    degraded_features: Vec<FrameFeature>,
    raster: FrameRasterStats,
}

impl FrameSubmissionReport {
    /// Assemble a submission report from its parts.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        backend: BackendKind,
        frame_index: u64,
        tick: u64,
        submitted_draws: u32,
        skipped_draws: u32,
        critical_coverage_skipped: u32,
        degraded_materials: u32,
        degraded_features: Vec<FrameFeature>,
        raster: FrameRasterStats,
    ) -> Self {
        FrameSubmissionReport {
            backend,
            frame_index,
            tick,
            submitted_draws,
            skipped_draws,
            critical_coverage_skipped,
            degraded_materials,
            degraded_features,
            raster,
        }
    }

    /// Which backend produced the frame.
    pub const fn backend(&self) -> BackendKind {
        self.backend
    }

    /// The frame index reported.
    pub const fn frame_index(&self) -> u64 {
        self.frame_index
    }

    /// The simulation tick reported.
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// The number of draws actually submitted (rasterized at least in part).
    pub const fn submitted_draws(&self) -> u32 {
        self.submitted_draws
    }

    /// The number of draws skipped (e.g. unknown mesh/material).
    pub const fn skipped_draws(&self) -> u32 {
        self.skipped_draws
    }

    /// Visible critical coverage that was dropped entirely — the invariant: this
    /// is zero in every healthy frame (critical coverage degrades, never drops).
    pub const fn critical_coverage_skipped(&self) -> u32 {
        self.critical_coverage_skipped
    }

    /// The number of materials presented in a degraded form.
    pub const fn degraded_materials(&self) -> u32 {
        self.degraded_materials
    }

    /// The features that were dropped or approximated this frame.
    pub fn degraded_features(&self) -> &[FrameFeature] {
        &self.degraded_features
    }

    /// The count of dropped/approximated features (`degraded_features().len()`).
    pub fn unsupported_features(&self) -> u32 {
        self.degraded_features.len() as u32
    }

    /// The per-frame software-rasterization stats (framebuffer size, triangle
    /// and depth-test counts, terrain preservation). All zero for a hardware
    /// backend that runs no CPU rasterizer.
    pub const fn raster(&self) -> &FrameRasterStats {
        &self.raster
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_kind_variants_compare_and_format() {
        let kinds = [
            BackendKind::GpuPrimary,
            BackendKind::GpuFallback,
            BackendKind::Canvas2d,
        ];
        assert_ne!(kinds[0], kinds[1]);
        assert_ne!(kinds[1], kinds[2]);
        assert_ne!(kinds[0], kinds[2]);
        assert_eq!(kinds[0], BackendKind::GpuPrimary);
        assert!(format!("{:?}", kinds[0]).contains("GpuPrimary"));
        assert!(format!("{:?}", kinds[1]).contains("GpuFallback"));
        assert!(format!("{:?}", kinds[2]).contains("Canvas2d"));
    }

    #[test]
    fn frame_feature_variants_compare_and_format() {
        let features = [
            FrameFeature::Shadows,
            FrameFeature::AlbedoSampling,
            FrameFeature::PointLightFalloff,
            FrameFeature::PerspectiveCorrectAlbedo,
            FrameFeature::MultiLight,
            FrameFeature::PostProcessing,
            FrameFeature::Sky,
            FrameFeature::SpecularHighlight,
            FrameFeature::Bloom,
            FrameFeature::AerialPerspective,
            FrameFeature::ProceduralSurface,
            FrameFeature::SurfaceOrnament,
        ];
        // "The surface could not be honoured at all" and "the surface was
        // honoured without its ornament" are different facts, so they are
        // different features — the same reason Bloom is not PostProcessing.
        assert_ne!(
            FrameFeature::SurfaceOrnament,
            FrameFeature::ProceduralSurface
        );
        // Bloom is its own feature, not a spelling of PostProcessing: a backend
        // that grades but cannot bloom must be able to report exactly that.
        assert_ne!(FrameFeature::Bloom, FrameFeature::PostProcessing);
        features.windows(2).for_each(|w| assert_ne!(w[0], w[1]));
        assert_eq!(features[0], FrameFeature::Shadows);
        features
            .iter()
            .for_each(|f| assert!(!format!("{f:?}").is_empty()));
    }

    #[test]
    fn report_accessors_round_trip() {
        let stats = FrameRasterStats {
            framebuffer_width: 320,
            framebuffer_height: 180,
            rasterized_triangles: 1200,
            ..FrameRasterStats::ZERO
        };
        let report = FrameSubmissionReport::new(
            BackendKind::Canvas2d,
            4,   // frame_index
            240, // tick
            5,   // submitted
            2,   // skipped
            0,   // critical coverage skipped (the invariant)
            1,   // degraded materials
            vec![FrameFeature::Shadows, FrameFeature::AlbedoSampling],
            stats,
        );
        assert_eq!(report.backend(), BackendKind::Canvas2d);
        assert_eq!(report.frame_index(), 4);
        assert_eq!(report.tick(), 240);
        assert_eq!(report.submitted_draws(), 5);
        assert_eq!(report.skipped_draws(), 2);
        assert_eq!(report.critical_coverage_skipped(), 0);
        assert_eq!(report.degraded_materials(), 1);
        assert_eq!(
            report.degraded_features(),
            &[FrameFeature::Shadows, FrameFeature::AlbedoSampling]
        );
        assert_eq!(report.unsupported_features(), 2);
        assert_eq!(report.raster(), &stats);
        assert_eq!(report.raster().rasterized_triangles, 1200);
        assert!(format!("{report:?}").contains("FrameSubmissionReport"));
    }

    #[test]
    fn report_equality_requires_all_fields() {
        let a = FrameSubmissionReport::new(
            BackendKind::GpuPrimary,
            1,
            1,
            3,
            0,
            0,
            0,
            Vec::new(),
            FrameRasterStats::ZERO,
        );
        let b = FrameSubmissionReport::new(
            BackendKind::GpuPrimary,
            1,
            1,
            3,
            0,
            0,
            0,
            Vec::new(),
            FrameRasterStats::ZERO,
        );
        let c = FrameSubmissionReport::new(
            BackendKind::GpuFallback,
            1,
            1,
            3,
            0,
            0,
            0,
            Vec::new(),
            FrameRasterStats::ZERO,
        );
        assert_eq!(a.clone(), b);
        assert_ne!(a, c);
        assert_eq!(a.unsupported_features(), 0);
        assert_eq!(a.critical_coverage_skipped(), 0);
        assert_eq!(a.raster(), &FrameRasterStats::ZERO);
    }
}
