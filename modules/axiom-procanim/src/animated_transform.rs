//! [`AnimatedTransform`] — a deterministic per-frame transform, fixed-point.

use axiom_kernel::{BinaryWriter, StableHash};

/// One entity's animated transform at a tick: a position **offset** (milliunits),
/// a **yaw** (milliradians), and a **scale** (per-mille, `1000` = ×1). All
/// integers — no naked floats — so it is deterministic across platforms; an app
/// converts to the engine's f32 `Transform` at the GPU edge. Returned by
/// [`crate::ProcAnimApi`] and read through its methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimatedTransform {
    offset: [i32; 3],
    yaw: i32,
    scale: [i32; 3],
}

impl AnimatedTransform {
    pub(crate) fn new(offset: [i32; 3], yaw: i32, scale: [i32; 3]) -> Self {
        AnimatedTransform { offset, yaw, scale }
    }

    /// The position offset `[x, y, z]` in milliunits (add to a base position).
    pub fn offset(self) -> [i32; 3] {
        self.offset
    }

    /// The yaw rotation in milliradians (`0..=6283`, one full turn).
    pub fn yaw(self) -> i32 {
        self.yaw
    }

    /// The scale `[x, y, z]` in per-mille (`1000` = ×1).
    pub fn scale(self) -> [i32; 3] {
        self.scale
    }

    /// The canonical bytes: offset, yaw, then scale (each little-endian).
    pub fn to_bytes(self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        self.offset.iter().for_each(|&v| writer.write_u32(v as u32));
        writer.write_u32(self.yaw as u32);
        self.scale.iter().for_each(|&v| writer.write_u32(v as u32));
        writer.into_bytes()
    }

    /// The stable digest over [`Self::to_bytes`].
    pub fn digest(self) -> StableHash {
        StableHash::of_bytes(&self.to_bytes())
    }
}
