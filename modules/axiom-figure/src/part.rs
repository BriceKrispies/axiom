//! One part of an articulated figure: a bone-like node plus a render box.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};
use axiom_math::{Transform, Vec3};

/// A single part of a [`crate::FigureDefinition`]. A part is a bone (a parent
/// link and a rest local transform) fused with the data an app needs to draw it:
/// a `box_size` (the low-poly box extents) and an opaque `tag` the game assigns
/// to pick a material/role. The animation mechanism poses the bone; this type
/// carries the render box the pose moves around. `tag` is opaque here on
/// purpose — naming what a tag *means* would be gameplay meaning leaking into a
/// generic figure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FigurePart {
    /// Index of this part's parent in the figure's part list, or `None` for a
    /// root. A valid figure keeps `parent < own index`.
    pub parent: Option<u32>,
    /// Rest local transform, relative to the parent part.
    pub rest: Transform,
    /// The render box extents for this part.
    pub box_size: Vec3,
    /// The box center's offset from the part's pivot, in the part's local space.
    /// A limb box pivots at its joint (the part origin) but is centered along its
    /// segment; this offset expresses that. Zero centers the box on the pivot.
    pub box_offset: Vec3,
    /// An opaque, game-defined tag (e.g. a material or role selector).
    pub tag: u32,
}

impl FigurePart {
    /// A root part (no parent).
    pub const fn root(rest: Transform, box_size: Vec3, box_offset: Vec3, tag: u32) -> Self {
        Self {
            parent: None,
            rest,
            box_size,
            box_offset,
            tag,
        }
    }

    /// A child part parented to the part at index `parent`.
    pub const fn child(
        parent: u32,
        rest: Transform,
        box_size: Vec3,
        box_offset: Vec3,
        tag: u32,
    ) -> Self {
        Self {
            parent: Some(parent),
            rest,
            box_size,
            box_offset,
            tag,
        }
    }

    /// Append the part's bytes: a one-byte parent tag (`0` root, `1` child + the
    /// parent's `u32` index), then the rest transform, box size, and tag.
    pub(crate) fn write_to(self, writer: &mut BinaryWriter) {
        writer.write_u8(u8::from(self.parent.is_some()));
        self.parent.iter().for_each(|parent| writer.write_u32(*parent));
        self.rest.write_to(writer);
        self.box_size.write_to(writer);
        self.box_offset.write_to(writer);
        writer.write_u32(self.tag);
    }

    /// Read a part written by [`FigurePart::write_to`].
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<FigurePart> {
        reader
            .read_tagged(&[|_| Ok(None), |r| r.read_u32().map(Some)])
            .and_then(|parent| {
                Transform::read_from(reader).and_then(|rest| {
                    Vec3::read_from(reader).and_then(|box_size| {
                        Vec3::read_from(reader).and_then(|box_offset| {
                            reader.read_u32().map(|tag| FigurePart {
                                parent,
                                rest,
                                box_size,
                                box_offset,
                                tag,
                            })
                        })
                    })
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(part: FigurePart) -> FigurePart {
        let mut w = BinaryWriter::new();
        part.write_to(&mut w);
        let bytes = w.into_bytes();
        FigurePart::read_from(&mut BinaryReader::new(&bytes)).unwrap()
    }

    #[test]
    fn root_and_child_construct_and_round_trip() {
        let root = FigurePart::root(
            Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::ZERO,
            7,
        );
        assert_eq!(root.parent, None);
        assert_eq!(root.tag, 7);
        assert_eq!(round_trip(root), root);

        let child = FigurePart::child(
            0,
            Transform::from_translation(Vec3::new(0.0, -0.4, 0.0)),
            Vec3::new(0.2, 0.8, 0.2),
            Vec3::new(0.0, -0.4, 0.0),
            3,
        );
        assert_eq!(child.parent, Some(0));
        assert_eq!(child.box_offset, Vec3::new(0.0, -0.4, 0.0));
        assert_eq!(round_trip(child), child);
    }

    #[test]
    fn truncated_part_fails_to_decode() {
        let part = FigurePart::root(Transform::IDENTITY, Vec3::new(1.0, 1.0, 1.0), Vec3::ZERO, 1);
        let mut w = BinaryWriter::new();
        part.write_to(&mut w);
        let bytes = w.into_bytes();
        assert!(FigurePart::read_from(&mut BinaryReader::new(&bytes[..bytes.len() - 1])).is_err());
    }
}
