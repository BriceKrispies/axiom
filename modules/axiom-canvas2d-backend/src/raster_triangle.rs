//! A screen-space triangle the software rasterizer fills.
//!
//! A [`RasterTriangle`] is three projected [`RasterVertex`]es plus the flat
//! colour the rasterizer writes and the owning object id. v1 is flat-shaded: the
//! triangle's colour is the mean of its vertices' colours (the per-vertex colours
//! and object id are folded in at construction, via [`RasterTriangle::from_vertices`],
//! so the per-pixel loop only needs one colour). Depth still varies per pixel —
//! it is interpolated from the vertices' NDC z — so occlusion is real.

use crate::raster_vertex::RasterVertex;

/// A flat-shaded screen-space triangle: its three projected vertices, the owning
/// `object_id`, and the flat linear RGBA `color` written to every covered pixel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RasterTriangle {
    vertices: [RasterVertex; 3],
    object_id: u64,
    color: [f32; 4],
}

impl RasterTriangle {
    /// Build a triangle from three projected vertices and an explicit, already
    /// depth-cue-shaded flat colour (lighting + height tint + falloff baked in by
    /// `frame_packet_raster`). The object id is taken from the first vertex.
    pub(crate) fn shaded(vertices: [RasterVertex; 3], color: [f32; 4]) -> Self {
        RasterTriangle {
            vertices,
            object_id: vertices[0].object_id(),
            color,
        }
    }

    /// The component-wise mean of a triangle's vertices' colours — the flat base
    /// colour before depth-cue shading.
    pub(crate) fn base_color(vertices: &[RasterVertex; 3]) -> [f32; 4] {
        let sum = vertices.iter().fold([0.0_f32; 4], |acc, v| {
            let c = v.color();
            [acc[0] + c[0], acc[1] + c[1], acc[2] + c[2], acc[3] + c[3]]
        });
        [sum[0] / 3.0, sum[1] / 3.0, sum[2] / 3.0, sum[3] / 3.0]
    }

    /// The three projected vertices (for edge-function / depth interpolation).
    pub(crate) fn vertices(&self) -> &[RasterVertex; 3] {
        &self.vertices
    }

    /// The owning draw's stable object id (written to the hit-test buffer).
    pub(crate) fn object_id(&self) -> u64 {
        self.object_id
    }

    /// The flat linear RGBA fill colour.
    pub(crate) fn color(&self) -> [f32; 4] {
        self.color
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_colour_means_vertices_and_shaded_carries_id_and_colour() {
        let v0 = RasterVertex::new(0.0, 0.0, 0.0, [0.0, 0.0, 0.0, 1.0], 5);
        let v1 = RasterVertex::new(3.0, 0.0, 0.0, [0.6, 0.0, 0.0, 1.0], 5);
        let v2 = RasterVertex::new(0.0, 3.0, 0.0, [0.0, 0.6, 0.3, 1.0], 5);
        let verts = [v0, v1, v2];
        assert_eq!(RasterTriangle::base_color(&verts), [0.2, 0.2, 0.1, 1.0]);
        let t = RasterTriangle::shaded(verts, [0.5, 0.4, 0.3, 1.0]);
        assert_eq!(t.color(), [0.5, 0.4, 0.3, 1.0]);
        assert_eq!(t.object_id(), 5);
        assert_eq!(t.vertices()[1], v1);
        assert!(format!("{t:?}").contains("RasterTriangle"));
        assert_eq!(t, RasterTriangle::shaded(verts, [0.5, 0.4, 0.3, 1.0]));
    }
}
