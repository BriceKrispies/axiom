//! Opaque value-type id handles the neutral 2D draw contract traffics in.
//!
//! All four are plain `Copy` newtypes carrying no behaviour — the nouns the
//! `axiom-draw2d` builder returns ([`TransformDepth`], [`PaintId`]) or accepts
//! ([`TextureId`], [`FontHandle`]). They are part of the host-owned neutral 2D
//! draw contract every render backend names.

/// A texture the backend will bind, resolved **in the app** (fetch/decode) and
/// named here by a stable id. `axiom-draw2d` never loads pixels; it only names
/// the resolved handle (the same fetch-in-the-app rule as `axiom-assets`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextureId(u64);

impl TextureId {
    /// Construct from a raw `u64`.
    pub const fn from_raw(raw: u64) -> Self {
        TextureId(raw)
    }

    /// The underlying raw value.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// A baked font the backend binds as a glyph atlas, resolved **in the app**.
/// `axiom-draw2d` is glyph-index-only: it names the font and carries glyph
/// sub-rects/advances, never rasterizing a typeface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontHandle(u64);

impl FontHandle {
    /// Construct from a raw `u64`.
    pub const fn from_raw(raw: u64) -> Self {
        FontHandle(raw)
    }

    /// The underlying raw value.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// A marker into the transform stack returned when the `axiom-draw2d` builder
/// pushes a transform: the stack depth to restore to on the matching pop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TransformDepth(usize);

impl TransformDepth {
    /// Construct from a raw depth.
    pub const fn from_raw(raw: usize) -> Self {
        TransformDepth(raw)
    }

    /// The underlying stack depth.
    pub const fn raw(self) -> usize {
        self.0
    }
}

/// A handle into a frame's paint table, returned when the `axiom-draw2d`
/// builder registers a linear/radial gradient and referenced from a
/// [`crate::Fill2d`]. A zero-based index into the per-frame table; it is only
/// valid within the frame that minted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PaintId(u32);

impl PaintId {
    /// Construct from a raw zero-based index.
    pub const fn from_raw(raw: u32) -> Self {
        PaintId(raw)
    }

    /// The underlying zero-based index.
    pub const fn raw(self) -> u32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_id_round_trips() {
        assert_eq!(TextureId::from_raw(9).raw(), 9);
        assert_eq!(TextureId::from_raw(9), TextureId::from_raw(9));
        assert_ne!(TextureId::from_raw(9), TextureId::from_raw(10));
    }

    #[test]
    fn font_handle_round_trips() {
        assert_eq!(FontHandle::from_raw(4).raw(), 4);
        assert_ne!(FontHandle::from_raw(4), FontHandle::from_raw(5));
    }

    #[test]
    fn transform_depth_round_trips() {
        assert_eq!(TransformDepth::from_raw(3).raw(), 3);
        assert!(TransformDepth::from_raw(1) < TransformDepth::from_raw(2));
    }

    #[test]
    fn paint_id_round_trips() {
        assert_eq!(PaintId::from_raw(0).raw(), 0);
        assert_ne!(PaintId::from_raw(0), PaintId::from_raw(1));
    }
}
