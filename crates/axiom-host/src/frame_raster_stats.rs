//! Per-frame software-rasterization statistics — a neutral telemetry block.
//!
//! [`FrameRasterStats`] carries the per-frame numbers a *software* render
//! backend (the Canvas 2D path) produces when it rasterizes a
//! [`crate::FramePacket`] into a low-resolution colour + depth framebuffer: the
//! framebuffer size, draw/triangle/cull counts, depth-test counts, terrain
//! preservation, the pixel budget, and — grouped into [`FrameDepthCueStats`] —
//! the depth-cue stage's accounting. A hardware backend that runs no CPU
//! rasterizer reports [`FrameRasterStats::ZERO`].
//!
//! These are plain `u32`/`u64`/`bool` counts (plus a `&'static str` cue-profile
//! label) — no GPU, browser, or DOM names — so they live cleanly on the
//! platform-free host contract alongside [`crate::FrameSubmissionReport`], which
//! embeds one. Public fields: a plain telemetry DTO, built field-by-field by the
//! backend. Per-frame *timings* are deliberately **not** here (they require a
//! wall clock the deterministic core may not read — the platform arm logs them).

/// The software backend's depth-cue stage accounting (fog, fake lighting, height
/// tint, falloff, contact shadows, outlines, vertical grade). All neutral counts;
/// zero when a cue is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameDepthCueStats {
    /// Triangles whose flat colour was modulated by fake directional lighting.
    pub lit_triangles: u32,
    /// Triangles whose flat colour received a height/elevation tint.
    pub height_tinted_triangles: u32,
    /// Triangles whose flat colour received distance detail/colour falloff.
    pub distance_falloff_applied_triangles: u32,
    /// Pixels mixed toward the fog colour by the depth-fog post-pass.
    pub depth_fog_applied_pixels: u64,
    /// Pixels adjusted by the camera-relative vertical colour grade.
    pub vertical_grade_applied_pixels: u64,
    /// Contact-shadow blobs drawn (one per important object that emitted one).
    pub contact_shadows_drawn: u32,
    /// Framebuffer pixels darkened by contact-shadow blobs.
    pub contact_shadow_pixels: u64,
    /// Important objects given a depth-weighted silhouette outline.
    pub outlined_objects: u32,
    /// Framebuffer pixels written by object outlines.
    pub outline_pixels: u64,
    /// Far horizon/terrain silhouette bands drawn (0 when the cue is disabled).
    pub horizon_silhouette_drawn: u32,
    /// The name of the depth-cue profile that shaded this frame (`""` for a
    /// hardware backend that applies no software cues).
    pub depth_cue_profile_name: &'static str,
}

impl FrameDepthCueStats {
    /// The all-zero cue stats a non-cue backend reports.
    pub const ZERO: FrameDepthCueStats = FrameDepthCueStats {
        lit_triangles: 0,
        height_tinted_triangles: 0,
        distance_falloff_applied_triangles: 0,
        depth_fog_applied_pixels: 0,
        vertical_grade_applied_pixels: 0,
        contact_shadows_drawn: 0,
        contact_shadow_pixels: 0,
        outlined_objects: 0,
        outline_pixels: 0,
        horizon_silhouette_drawn: 0,
        depth_cue_profile_name: "",
    };
}

/// The CPU rasterizer's per-frame accounting. Two values are equal iff every
/// field is equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameRasterStats {
    /// Internal framebuffer width (device pixels) the frame rasterized at.
    pub framebuffer_width: u32,
    /// Internal framebuffer height (device pixels) the frame rasterized at.
    pub framebuffer_height: u32,
    /// Draws with a resolved mesh that were projected (rasterization attempted).
    pub projected_draws: u32,
    /// Triangles that projected validly (all vertices in front of the near plane).
    pub projected_triangles: u32,
    /// Triangles dropped before the pixel loop (offscreen / below min area).
    pub culled_triangles: u32,
    /// Triangles actually rasterized into the framebuffer.
    pub rasterized_triangles: u32,
    /// Triangles dropped as degenerate (zero/near-zero screen area).
    pub skipped_degenerate_triangles: u32,
    /// Triangles dropped because a vertex projected at/behind the near plane.
    pub skipped_invalid_projection_triangles: u32,
    /// Pixels examined inside triangle bounding boxes (the raster work proxy).
    pub candidate_pixels: u64,
    /// Fragments that reached the per-pixel depth test (covered an in-bounds pixel).
    pub depth_tested_pixels: u64,
    /// Fragments that passed the depth test and wrote colour + depth.
    pub depth_written_pixels: u64,
    /// Fragments that failed the depth test (occluded by a nearer fragment).
    pub depth_rejected_pixels: u64,
    /// Terrain/critical-coverage draws preserved (rasterized, never skipped).
    pub terrain_draws_preserved: u32,
    /// Terrain triangles dropped by deterministic level-of-detail decimation.
    pub terrain_triangles_decimated: u32,
    /// Distinct objects (by `object_id`) that contributed surviving geometry.
    pub rasterized_objects: u32,
    /// Decorative draws skipped to stay within the frame pixel budget.
    pub skipped_decorative_draws: u32,
    /// Whether the frame pixel budget was exhausted (later decorative draws
    /// degraded by importance). Critical coverage is never skipped for budget.
    pub budget_exhausted: bool,
    /// The depth-cue stage's per-frame accounting.
    pub depth_cues: FrameDepthCueStats,
}

impl FrameRasterStats {
    /// The all-zero stats a non-rasterizing backend (a hardware GPU path)
    /// reports — it runs no CPU rasterizer, so every count is zero.
    pub const ZERO: FrameRasterStats = FrameRasterStats {
        framebuffer_width: 0,
        framebuffer_height: 0,
        projected_draws: 0,
        projected_triangles: 0,
        culled_triangles: 0,
        rasterized_triangles: 0,
        skipped_degenerate_triangles: 0,
        skipped_invalid_projection_triangles: 0,
        candidate_pixels: 0,
        depth_tested_pixels: 0,
        depth_written_pixels: 0,
        depth_rejected_pixels: 0,
        terrain_draws_preserved: 0,
        terrain_triangles_decimated: 0,
        rasterized_objects: 0,
        skipped_decorative_draws: 0,
        budget_exhausted: false,
        depth_cues: FrameDepthCueStats::ZERO,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_round_trip() {
        let s = FrameRasterStats {
            framebuffer_width: 240,
            framebuffer_height: 135,
            projected_draws: 3,
            projected_triangles: 1500,
            culled_triangles: 300,
            rasterized_triangles: 1200,
            skipped_degenerate_triangles: 4,
            skipped_invalid_projection_triangles: 7,
            candidate_pixels: 80_000,
            depth_tested_pixels: 50_000,
            depth_written_pixels: 30_000,
            depth_rejected_pixels: 20_000,
            terrain_draws_preserved: 2,
            terrain_triangles_decimated: 900,
            rasterized_objects: 5,
            skipped_decorative_draws: 1,
            budget_exhausted: true,
            depth_cues: FrameDepthCueStats {
                lit_triangles: 1200,
                height_tinted_triangles: 1200,
                distance_falloff_applied_triangles: 1200,
                depth_fog_applied_pixels: 32_400,
                vertical_grade_applied_pixels: 32_400,
                contact_shadows_drawn: 2,
                contact_shadow_pixels: 600,
                outlined_objects: 2,
                outline_pixels: 240,
                horizon_silhouette_drawn: 0,
                depth_cue_profile_name: "low-poly-framebuffer",
            },
        };
        assert_eq!(s.framebuffer_width, 240);
        assert_eq!(s.projected_triangles, 1500);
        assert_eq!(s.culled_triangles, 300);
        assert_eq!(s.candidate_pixels, 80_000);
        assert_eq!(s.rasterized_objects, 5);
        assert!(s.budget_exhausted);
        assert_eq!(s.depth_cues.lit_triangles, 1200);
        assert_eq!(s.depth_cues.contact_shadows_drawn, 2);
        assert_eq!(s.depth_cues.contact_shadow_pixels, 600);
        assert_eq!(s.depth_cues.outlined_objects, 2);
        assert_eq!(s.depth_cues.horizon_silhouette_drawn, 0);
        assert_eq!(s.depth_cues.depth_cue_profile_name, "low-poly-framebuffer");
        assert!(format!("{s:?}").contains("FrameRasterStats"));
    }

    #[test]
    fn zero_is_all_zero_and_is_the_default() {
        let z = FrameRasterStats::ZERO;
        assert_eq!(z.framebuffer_width, 0);
        assert_eq!(z.candidate_pixels, 0);
        assert_eq!(z.rasterized_objects, 0);
        assert!(!z.budget_exhausted);
        assert_eq!(z.depth_cues, FrameDepthCueStats::ZERO);
        assert_eq!(z.depth_cues.depth_cue_profile_name, "");
        assert_eq!(z, FrameRasterStats::default());
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = FrameRasterStats {
            framebuffer_width: 320,
            ..FrameRasterStats::ZERO
        };
        assert_eq!(a, a);
        assert_ne!(
            a,
            FrameRasterStats {
                framebuffer_width: 321,
                ..FrameRasterStats::ZERO
            }
        );
        assert_ne!(a, FrameRasterStats::ZERO);
        // Cue stats participate in equality.
        assert_ne!(
            FrameRasterStats {
                depth_cues: FrameDepthCueStats {
                    lit_triangles: 1,
                    ..FrameDepthCueStats::ZERO
                },
                ..FrameRasterStats::ZERO
            },
            FrameRasterStats::ZERO
        );
    }
}
