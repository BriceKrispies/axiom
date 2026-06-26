//! A projected, screen-space vertex ready for triangle rasterization.
//!
//! A [`RasterVertex`] is the result of running a mesh vertex through a draw's
//! `mvp` and the perspective divide (see `frame_packet_raster`): an x/y in
//! framebuffer pixels, a depth in NDC z (smaller = nearer), the resolved flat
//! RGBA colour, and the owning object's stable id. It carries no browser types
//! and no matrices — pure projected data the rasterizer interpolates.

/// One projected vertex in framebuffer space. `depth` is NDC z (smaller =
/// nearer, the rasterizer's depth convention); `color` is the vertex's resolved
/// linear RGBA (mesh colour × draw colour); `object_id` is the owning draw's
/// stable identity (preserved for per-pixel hit-testing).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RasterVertex {
    x: f32,
    y: f32,
    depth: f32,
    color: [f32; 4],
    object_id: u64,
}

impl RasterVertex {
    /// A vertex at framebuffer `(x, y)`, NDC `depth`, linear `color`, owned by
    /// `object_id`.
    pub(crate) fn new(x: f32, y: f32, depth: f32, color: [f32; 4], object_id: u64) -> Self {
        RasterVertex {
            x,
            y,
            depth,
            color,
            object_id,
        }
    }

    /// Framebuffer x (device pixels, may be fractional / off-screen).
    pub(crate) fn x(&self) -> f32 {
        self.x
    }

    /// Framebuffer y (device pixels, may be fractional / off-screen).
    pub(crate) fn y(&self) -> f32 {
        self.y
    }

    /// NDC depth (smaller = nearer).
    pub(crate) fn depth(&self) -> f32 {
        self.depth
    }

    /// The vertex's resolved linear RGBA colour.
    pub(crate) fn color(&self) -> [f32; 4] {
        self.color
    }

    /// The owning draw's stable object id.
    pub(crate) fn object_id(&self) -> u64 {
        self.object_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip() {
        let v = RasterVertex::new(12.0, 34.0, 0.25, [0.1, 0.2, 0.3, 1.0], 77);
        assert_eq!(v.x(), 12.0);
        assert_eq!(v.y(), 34.0);
        assert_eq!(v.depth(), 0.25);
        assert_eq!(v.color(), [0.1, 0.2, 0.3, 1.0]);
        assert_eq!(v.object_id(), 77);
        assert_eq!(
            v,
            RasterVertex::new(12.0, 34.0, 0.25, [0.1, 0.2, 0.3, 1.0], 77)
        );
        assert_ne!(
            v,
            RasterVertex::new(0.0, 34.0, 0.25, [0.1, 0.2, 0.3, 1.0], 77)
        );
        assert!(format!("{v:?}").contains("RasterVertex"));
    }
}
