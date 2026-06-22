//! Backend-owned Canvas presentation policy — the visual-drift seam.
//!
//! These types let the Canvas 2D backend intentionally drift in *presentation*
//! (an intentional low-resolution software-rendered look) while preserving
//! render *intent*. Every decision here is derived only from the
//! backend-neutral [`axiom_host::FramePacket`] plus the backend's own resource
//! tables — never from scene/game/resource modules. The game never branches on
//! "if canvas"; the backend chooses its framebuffer resolution and terrain
//! level-of-detail from the frame and the rasterizer options. This is where
//! Canvas2D is *allowed* to differ from the GPU backends — and the only place.

/// The Canvas visual style. v1 ships one: a true low-resolution software
/// framebuffer (colour buffer + per-pixel z-buffer), upscaled to the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CanvasVisualProfile {
    /// Project + rasterize the packet into a small RGBA+depth framebuffer and
    /// blit it to the canvas (nearest-neighbour upscale). The shipping look.
    LowPolyFramebuffer,
}

/// An opt-in debugging overlay applied to the software framebuffer. The
/// `LowPolyFramebuffer` profile defaults to [`Self::None`]: a solid, depth-tested
/// low-poly image with no wireframe. The overlays exist to diagnose the
/// rasterizer, never as the default look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CanvasDebugOverlay {
    /// No overlay — the shipping solid-filled look.
    None,
    /// Paint only triangle edges (per-pixel barycentric wireframe).
    TriangleEdges,
    /// Replace the colour buffer with a grayscale visualization of the z-buffer.
    DepthBuffer,
    /// Paint each triangle's screen bounding-box border.
    Bounds,
}

impl CanvasDebugOverlay {
    /// A stable dense index `0..4` for branchless table selection in the
    /// rasterizer's per-pixel paint mask.
    pub(crate) fn index(self) -> usize {
        [
            CanvasDebugOverlay::None,
            CanvasDebugOverlay::TriangleEdges,
            CanvasDebugOverlay::DepthBuffer,
            CanvasDebugOverlay::Bounds,
        ]
        .iter()
        .position(|o| *o == self)
        .unwrap_or(0)
    }
}

/// A discrete internal-resolution quality tier for the software framebuffer.
/// Lower tiers rasterize far fewer pixels (cost scales with width×height), so
/// the forced-Canvas2D fallback defaults to a low tier; the platform arm may
/// resolve a tier from a query parameter (and a future dynamic-resolution policy
/// could step tiers by measured frame time — the documented seam in
/// `low_poly_raster_options.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CanvasQualityPreset {
    /// 160×90 — cheapest.
    UltraLow,
    /// 240×135 — the forced-fallback default.
    Low,
    /// 320×180.
    Medium,
    /// 426×240 — sharpest (not the default for a no-GPU device).
    High,
}

impl CanvasQualityPreset {
    /// All tiers in ascending resolution order — the single source of truth for
    /// indexing and level resolution.
    const ORDER: [CanvasQualityPreset; 4] = [
        CanvasQualityPreset::UltraLow,
        CanvasQualityPreset::Low,
        CanvasQualityPreset::Medium,
        CanvasQualityPreset::High,
    ];

    /// A stable `0..4` index (ascending resolution) for table selection.
    pub(crate) fn index(self) -> usize {
        Self::ORDER.iter().position(|p| *p == self).unwrap_or(0)
    }

    /// The internal framebuffer dimensions `(width, height)` for this tier.
    pub(crate) fn dimensions(self) -> (u32, u32) {
        [(160, 90), (240, 135), (320, 180), (426, 240)][self.index()]
    }

    /// Resolve a tier from a numeric level (`0` = UltraLow … `3` = High),
    /// clamped into range. The platform arm maps a `?quality=` query to a level;
    /// constructing through the `ORDER` table keeps every tier reachable.
    pub(crate) fn from_level(level: u8) -> Self {
        Self::ORDER[(level as usize).min(Self::ORDER.len() - 1)]
    }
}

/// How important a draw's *visible coverage* is. Classified by the backend from
/// projected screen coverage (a FramePacket-derived heuristic), never from
/// scene/game knowledge. The software renderer treats `CriticalCoverage`
/// (terrain/ground) specially: it is never skipped and is the only kind eligible
/// for level-of-detail triangle decimation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CanvasFallbackImportance {
    /// Fills a large part of the view (terrain / ground / sky). Never skipped
    /// while it contributes visible coverage — decimate, don't drop.
    CriticalCoverage,
    /// A normal object: preserve its identity and a readable silhouette.
    GameplayObject,
    /// Minor visible contribution.
    Decorative,
    /// No visible coverage (fully behind the camera / degenerate).
    DebugOnly,
}

/// Coverage fraction (of the screen) at/above which a draw is CriticalCoverage.
const CRITICAL_COVERAGE_FRACTION: f32 = 0.12;
/// Coverage fraction at/above which a draw is at least a GameplayObject.
const GAMEPLAY_COVERAGE_FRACTION: f32 = 0.004;

/// Classify a draw's importance from the screen area it covers (device px²)
/// relative to the viewport — a backend-owned, FramePacket-derived heuristic.
/// A draw contributing no visible coverage (fully behind the camera / degenerate)
/// is DebugOnly; otherwise large coverage ⇒ terrain/ground ⇒ CriticalCoverage,
/// medium ⇒ GameplayObject, small ⇒ Decorative.
pub(crate) fn classify(coverage_px2: f32, screen_px2: f32) -> CanvasFallbackImportance {
    let fraction = coverage_px2 / screen_px2.max(1.0);
    let visible = coverage_px2 > 0.0;
    let critical = fraction >= CRITICAL_COVERAGE_FRACTION;
    let gameplay = fraction >= GAMEPLAY_COVERAGE_FRACTION;
    // Branchless table selects: critical overrides gameplay overrides decorative;
    // no visible coverage at all ⇒ debug-only.
    let base = [
        CanvasFallbackImportance::Decorative,
        CanvasFallbackImportance::GameplayObject,
    ][usize::from(gameplay)];
    let visible_importance =
        [base, CanvasFallbackImportance::CriticalCoverage][usize::from(critical)];
    [CanvasFallbackImportance::DebugOnly, visible_importance][usize::from(visible)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_by_coverage_fraction() {
        let screen = 1000.0;
        // ≥12% → critical (terrain/ground).
        assert_eq!(
            classify(200.0, screen),
            CanvasFallbackImportance::CriticalCoverage
        );
        // ≥0.4% and <12% → gameplay object.
        assert_eq!(
            classify(50.0, screen),
            CanvasFallbackImportance::GameplayObject
        );
        // <0.4% but visible → decorative.
        assert_eq!(classify(1.0, screen), CanvasFallbackImportance::Decorative);
        // No visible coverage → debug-only (free to skip); also no div-by-zero.
        assert_eq!(classify(0.0, screen), CanvasFallbackImportance::DebugOnly);
        assert_eq!(classify(0.0, 0.0), CanvasFallbackImportance::DebugOnly);
    }

    #[test]
    fn overlay_index_is_dense_and_distinct() {
        let overlays = [
            CanvasDebugOverlay::None,
            CanvasDebugOverlay::TriangleEdges,
            CanvasDebugOverlay::DepthBuffer,
            CanvasDebugOverlay::Bounds,
        ];
        overlays
            .iter()
            .enumerate()
            .for_each(|(i, o)| assert_eq!(o.index(), i));
        overlays.windows(2).for_each(|w| assert_ne!(w[0], w[1]));
        assert_eq!(CanvasDebugOverlay::None.index(), 0);
    }

    #[test]
    fn seam_types_are_distinct_and_formattable() {
        let profile = CanvasVisualProfile::LowPolyFramebuffer;
        assert_eq!(profile, CanvasVisualProfile::LowPolyFramebuffer);
        assert!(format!("{profile:?}").contains("LowPolyFramebuffer"));

        let importances = [
            CanvasFallbackImportance::CriticalCoverage,
            CanvasFallbackImportance::GameplayObject,
            CanvasFallbackImportance::Decorative,
            CanvasFallbackImportance::DebugOnly,
        ];
        importances.windows(2).for_each(|w| assert_ne!(w[0], w[1]));
        importances
            .iter()
            .for_each(|i| assert!(!format!("{i:?}").is_empty()));

        assert!(format!("{:?}", CanvasDebugOverlay::DepthBuffer).contains("DepthBuffer"));
    }

    #[test]
    fn quality_preset_dimensions_and_levels() {
        assert_eq!(CanvasQualityPreset::UltraLow.dimensions(), (160, 90));
        assert_eq!(CanvasQualityPreset::Low.dimensions(), (240, 135));
        assert_eq!(CanvasQualityPreset::Medium.dimensions(), (320, 180));
        assert_eq!(CanvasQualityPreset::High.dimensions(), (426, 240));
        // Indices are dense and ascending.
        [
            CanvasQualityPreset::UltraLow,
            CanvasQualityPreset::Low,
            CanvasQualityPreset::Medium,
            CanvasQualityPreset::High,
        ]
        .iter()
        .enumerate()
        .for_each(|(i, p)| assert_eq!(p.index(), i));
        // Lowering a tier reduces the pixel count.
        let px = |p: CanvasQualityPreset| {
            let (w, h) = p.dimensions();
            w * h
        };
        assert!(px(CanvasQualityPreset::UltraLow) < px(CanvasQualityPreset::Low));
        assert!(px(CanvasQualityPreset::Low) < px(CanvasQualityPreset::Medium));
        assert!(px(CanvasQualityPreset::Medium) < px(CanvasQualityPreset::High));
        // Levels resolve deterministically and clamp.
        assert_eq!(CanvasQualityPreset::from_level(0), CanvasQualityPreset::UltraLow);
        assert_eq!(CanvasQualityPreset::from_level(1), CanvasQualityPreset::Low);
        assert_eq!(CanvasQualityPreset::from_level(3), CanvasQualityPreset::High);
        assert_eq!(CanvasQualityPreset::from_level(99), CanvasQualityPreset::High);
        assert_eq!(CanvasQualityPreset::from_level(1), CanvasQualityPreset::from_level(1));
    }
}
