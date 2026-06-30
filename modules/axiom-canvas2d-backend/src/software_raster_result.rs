//! The finished software-rasterized frame and its statistics.
//!
//! [`SoftwareRasterResult`] is what [`crate::software_rasterizer::SoftwareRasterizer`]
//! returns: the RGBA8 framebuffer bytes (the blit source), its size, the
//! geometry-conversion stats, and the per-pixel raster + post-pass counters. The
//! fields are `pub(crate)` so the rasterizer assembles it directly; the accessors
//! are the read surface the backend report and tests consume.

use crate::frame_packet_raster::ConversionStats;

/// The finished frame: the RGBA8 framebuffer bytes (the blit source), the
/// framebuffer size, the conversion stats, and the per-pixel raster stats.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SoftwareRasterResult {
    pub(crate) rgba: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) conv: ConversionStats,
    pub(crate) rasterized_triangles: u32,
    pub(crate) candidate_pixels: u64,
    pub(crate) depth_tested_pixels: u64,
    pub(crate) depth_written_pixels: u64,
    pub(crate) depth_fog_applied_pixels: u64,
    pub(crate) vertical_grade_applied_pixels: u64,
    pub(crate) contact_shadows_drawn: u32,
    pub(crate) contact_shadow_pixels: u64,
    pub(crate) outlined_objects: u32,
    pub(crate) outline_pixels: u64,
    pub(crate) horizon_silhouette_drawn: u32,
}

impl SoftwareRasterResult {
    /// The framebuffer RGBA8 bytes (row-major, top-left origin) — the blit source.
    pub(crate) fn rgba_bytes(&self) -> &[u8] {
        &self.rgba
    }

    /// Framebuffer width in pixels.
    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    /// Framebuffer height in pixels.
    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    /// The geometry-conversion stats (projection / cull / LOD / budget).
    pub(crate) fn conversion(&self) -> &ConversionStats {
        &self.conv
    }

    /// Triangles actually rasterized (post-cull, non-degenerate).
    pub(crate) fn rasterized_triangles(&self) -> u32 {
        self.rasterized_triangles
    }

    /// Pixels examined inside triangle bounding boxes (the raster work proxy).
    pub(crate) fn candidate_pixels(&self) -> u64 {
        self.candidate_pixels
    }

    /// Fragments that reached the depth test (covered an in-bounds pixel).
    pub(crate) fn depth_tested_pixels(&self) -> u64 {
        self.depth_tested_pixels
    }

    /// Fragments that passed the depth test and wrote.
    pub(crate) fn depth_written_pixels(&self) -> u64 {
        self.depth_written_pixels
    }

    /// Fragments that failed the depth test (occluded).
    pub(crate) fn depth_rejected_pixels(&self) -> u64 {
        self.depth_tested_pixels - self.depth_written_pixels
    }

    /// Pixels mixed toward the fog colour by the depth-fog post-pass.
    pub(crate) fn depth_fog_applied_pixels(&self) -> u64 {
        self.depth_fog_applied_pixels
    }

    /// Pixels adjusted by the camera-relative vertical colour grade.
    pub(crate) fn vertical_grade_applied_pixels(&self) -> u64 {
        self.vertical_grade_applied_pixels
    }

    /// Contact-shadow blobs drawn (one per important object).
    pub(crate) fn contact_shadows_drawn(&self) -> u32 {
        self.contact_shadows_drawn
    }

    /// Framebuffer pixels darkened by contact-shadow blobs.
    pub(crate) fn contact_shadow_pixels(&self) -> u64 {
        self.contact_shadow_pixels
    }

    /// Important objects given a depth-weighted silhouette outline.
    pub(crate) fn outlined_objects(&self) -> u32 {
        self.outlined_objects
    }

    /// Framebuffer pixels written by object outlines.
    pub(crate) fn outline_pixels(&self) -> u64 {
        self.outline_pixels
    }

    /// Far horizon/terrain silhouette bands drawn (0 when the cue is off).
    pub(crate) fn horizon_silhouette_drawn(&self) -> u32 {
        self.horizon_silhouette_drawn
    }
}
