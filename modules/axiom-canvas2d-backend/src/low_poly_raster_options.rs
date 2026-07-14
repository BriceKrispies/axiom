//! Tuning for the low-resolution software rasterizer: the internal framebuffer
//! size (resolved from a [`CanvasQualityPreset`]), the debug overlay, the
//! terrain level-of-detail cap, the frame pixel budget, and the
//! [`CanvasDepthCueProfile`] (fog, lighting, height tint, contact shadows,
//! outlines, distance falloff, vertical grade).
//!
//! These are backend-owned presentation knobs — derived from neither the scene
//! nor the game, only chosen here. The default is the shipping forced-fallback
//! look: the **Low** quality tier (`240×135`, nearest-neighbour upscaled to the
//! canvas), no debug overlay, generous terrain cap + pixel budget, and the
//! subtle [`CanvasDepthCueProfile::low_poly_framebuffer`] depth cues.
//!
//! ## Dynamic-resolution seam
//! Quality is a fixed tier chosen up front (the platform arm resolves it from a
//! `?quality=` query, defaulting to Low). A future dynamic-resolution policy
//! would, in the **platform** arm only (never the timer-free deterministic
//! core), step [`CanvasQualityPreset`] tiers by measured frame time and rebuild
//! these options via [`LowPolyRasterOptions::from_preset`].

use crate::canvas_depth_cue_profile::CanvasDepthCueProfile;
use crate::canvas_policy::{CanvasDebugOverlay, CanvasQualityPreset};

/// Default terrain LOD cap: above this many triangles a critical-coverage draw
/// keeps only its largest-area triangles (the smallest, sub-pixel at this
/// resolution, are dropped). Comfortably above the count of triangles that can
/// be *visible* at the low framebuffer resolution, so normal terrain keeps every
/// visible triangle (no holes); the cap only bites pathological draws.
const DEFAULT_MAX_TERRAIN_TRIANGLES: u32 = 200_000;
/// Default frame pixel budget (candidate-pixel estimate). Generous — only
/// pathological frames exhaust it; once exhausted, *decorative* draws are
/// skipped (never critical coverage). Tests use a tiny budget to force this.
const DEFAULT_PIXEL_BUDGET: u64 = 8_000_000;

/// Software-rasterizer options. `pub(crate)` — internal presentation policy that
/// never widens the module facade.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LowPolyRasterOptions {
    framebuffer_width: u32,
    framebuffer_height: u32,
    debug_overlay: CanvasDebugOverlay,
    max_triangles_per_terrain_draw: u32,
    pixel_budget: u64,
    depth_cues: CanvasDepthCueProfile,
    /// Which optional render capabilities this backend will attempt. Defaults to
    /// `all()`; the facade restricts it to skip capabilities Canvas 2D shouldn't do.
    capability: axiom_host::BackendCapabilityProfile,
}

impl Default for LowPolyRasterOptions {
    /// The shipping forced-fallback configuration: the **Low** tier, no overlay,
    /// generous terrain cap + pixel budget, subtle depth cues.
    fn default() -> Self {
        LowPolyRasterOptions::from_preset(CanvasQualityPreset::Low)
    }
}

impl LowPolyRasterOptions {
    /// Assemble options from all parts (used by tests to exercise overlays, tight
    /// terrain caps, tight budgets, and custom depth-cue profiles).
    pub(crate) fn new(
        framebuffer_width: u32,
        framebuffer_height: u32,
        debug_overlay: CanvasDebugOverlay,
        max_triangles_per_terrain_draw: u32,
        pixel_budget: u64,
        depth_cues: CanvasDepthCueProfile,
    ) -> Self {
        LowPolyRasterOptions {
            framebuffer_width,
            framebuffer_height,
            debug_overlay,
            max_triangles_per_terrain_draw,
            pixel_budget,
            depth_cues,
            // Default: attempt everything. The facade restricts Canvas 2D per config.
            capability: axiom_host::BackendCapabilityProfile::all(),
        }
    }

    /// Resolve options for a quality tier at the tier's canonical **16:9**
    /// framebuffer dimensions, plus the shipping defaults (no overlay, generous cap
    /// + budget, subtle depth cues). Used where no surface aspect is in hand (the
    /// [`Default`] look); the live backend resolves the aspect-correct framebuffer
    /// via [`Self::from_preset_for_surface`].
    pub(crate) fn from_preset(preset: CanvasQualityPreset) -> Self {
        Self::from_dims(preset.dimensions())
    }

    /// Resolve options for a quality tier at a framebuffer that **preserves the
    /// surface's aspect ratio** (`surface_width × surface_height`), so the software
    /// framebuffer is the same shape the GPU renders — no aspect distortion on a
    /// non-16:9 surface. This is what the live backend uses; see
    /// [`CanvasQualityPreset::framebuffer_dims`].
    pub(crate) fn from_preset_for_surface(
        preset: CanvasQualityPreset,
        surface_width: u32,
        surface_height: u32,
    ) -> Self {
        Self::from_dims(preset.framebuffer_dims(surface_width, surface_height))
    }

    /// Shared assembly: framebuffer dimensions + the shipping defaults.
    fn from_dims((framebuffer_width, framebuffer_height): (u32, u32)) -> Self {
        LowPolyRasterOptions::new(
            framebuffer_width,
            framebuffer_height,
            CanvasDebugOverlay::None,
            DEFAULT_MAX_TERRAIN_TRIANGLES,
            DEFAULT_PIXEL_BUDGET,
            CanvasDepthCueProfile::low_poly_framebuffer(),
        )
    }

    /// The internal framebuffer width (device pixels).
    pub(crate) fn framebuffer_width(&self) -> u32 {
        self.framebuffer_width
    }

    /// The internal framebuffer height (device pixels).
    pub(crate) fn framebuffer_height(&self) -> u32 {
        self.framebuffer_height
    }

    /// The opt-in debug overlay (default [`CanvasDebugOverlay::None`]).
    pub(crate) fn debug_overlay(&self) -> CanvasDebugOverlay {
        self.debug_overlay
    }

    /// The per-draw terrain triangle cap before deterministic LOD decimation.
    pub(crate) fn max_triangles_per_terrain_draw(&self) -> u32 {
        self.max_triangles_per_terrain_draw
    }

    /// The frame candidate-pixel budget; decorative draws past it are skipped.
    pub(crate) fn pixel_budget(&self) -> u64 {
        self.pixel_budget
    }

    /// The Canvas depth-cue presentation profile (fog, lighting, tint, shadows,
    /// outlines, falloff, vertical grade).
    pub(crate) fn depth_cues(&self) -> CanvasDepthCueProfile {
        self.depth_cues
    }

    /// A copy of these options with the depth-cue profile replaced (the facade
    /// overrides the fog colour to the frame clear colour each frame).
    pub(crate) fn with_depth_cues(self, depth_cues: CanvasDepthCueProfile) -> Self {
        LowPolyRasterOptions { depth_cues, ..self }
    }

    /// The optional capabilities this backend will attempt (default `all()`).
    pub(crate) fn capability_profile(&self) -> axiom_host::BackendCapabilityProfile {
        self.capability
    }

    /// A copy of these options with the capability profile replaced — the facade's
    /// config lever for restricting what Canvas 2D attempts.
    pub(crate) fn with_capability_profile(
        self,
        capability: axiom_host::BackendCapabilityProfile,
    ) -> Self {
        LowPolyRasterOptions { capability, ..self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_low_tier_shipping_look_with_subtle_cues() {
        let o = LowPolyRasterOptions::default();
        assert_eq!(o.framebuffer_width(), 240);
        assert_eq!(o.framebuffer_height(), 135);
        assert_eq!(o.debug_overlay(), CanvasDebugOverlay::None);
        assert_eq!(o.max_triangles_per_terrain_draw(), 200_000);
        assert_eq!(o.pixel_budget(), 8_000_000);
        assert!(o.depth_cues().fog.enabled);
        assert!(o.depth_cues().lighting.enabled);
        assert!(format!("{o:?}").contains("LowPolyRasterOptions"));
    }

    #[test]
    fn from_preset_resolves_each_tier_to_exact_dimensions() {
        let dims = |p| {
            let o = LowPolyRasterOptions::from_preset(p);
            (o.framebuffer_width(), o.framebuffer_height())
        };
        assert_eq!(dims(CanvasQualityPreset::UltraLow), (160, 90));
        assert_eq!(dims(CanvasQualityPreset::Low), (240, 135));
        assert_eq!(dims(CanvasQualityPreset::Medium), (320, 180));
        assert_eq!(dims(CanvasQualityPreset::High), (426, 240));
        assert_eq!(
            LowPolyRasterOptions::from_preset(CanvasQualityPreset::Medium),
            LowPolyRasterOptions::from_preset(CanvasQualityPreset::Medium)
        );
    }

    #[test]
    fn from_preset_for_surface_resolves_aspect_matched_dimensions() {
        // The live backend's path: the framebuffer preserves the SURFACE aspect,
        // so an 8:5 (960×600) surface gets an 8:5 framebuffer (240×150 at Low),
        // never a distorting 16:9 — while every other option keeps its default.
        let o = LowPolyRasterOptions::from_preset_for_surface(CanvasQualityPreset::Low, 960, 600);
        assert_eq!((o.framebuffer_width(), o.framebuffer_height()), (240, 150));
        assert_eq!(o.debug_overlay(), CanvasDebugOverlay::None);
        assert_eq!(o.max_triangles_per_terrain_draw(), 200_000);
        assert_eq!(o.pixel_budget(), 8_000_000);
        assert!(o.depth_cues().fog.enabled);
        // A 16:9 surface reproduces the canonical 16:9 pairing (== from_preset).
        assert_eq!(
            LowPolyRasterOptions::from_preset_for_surface(CanvasQualityPreset::Low, 1920, 1080),
            LowPolyRasterOptions::from_preset(CanvasQualityPreset::Low)
        );
    }

    #[test]
    fn lower_preset_reduces_framebuffer_pixel_count() {
        let px = |p| {
            let o = LowPolyRasterOptions::from_preset(p);
            o.framebuffer_width() as u64 * o.framebuffer_height() as u64
        };
        assert!(px(CanvasQualityPreset::Low) < px(CanvasQualityPreset::Medium));
        assert!(px(CanvasQualityPreset::UltraLow) < px(CanvasQualityPreset::Low));
    }

    #[test]
    fn with_depth_cues_replaces_only_the_profile() {
        let mut cues = CanvasDepthCueProfile::low_poly_framebuffer();
        cues.fog.strength = 0.99;
        let o = LowPolyRasterOptions::default().with_depth_cues(cues);
        assert_eq!(o.depth_cues().fog.strength, 0.99);
        assert_eq!(o.framebuffer_width(), 240);
    }

    #[test]
    fn new_round_trips_every_field() {
        let cues = CanvasDepthCueProfile::low_poly_framebuffer();
        let o =
            LowPolyRasterOptions::new(64, 48, CanvasDebugOverlay::DepthBuffer, 128, 5_000, cues);
        assert_eq!(o.framebuffer_width(), 64);
        assert_eq!(o.framebuffer_height(), 48);
        assert_eq!(o.debug_overlay(), CanvasDebugOverlay::DepthBuffer);
        assert_eq!(o.max_triangles_per_terrain_draw(), 128);
        assert_eq!(o.pixel_budget(), 5_000);
        assert_eq!(o.depth_cues(), cues);
        assert_ne!(o, LowPolyRasterOptions::default());
    }
}
