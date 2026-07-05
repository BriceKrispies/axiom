//! A deterministic, engine-owned capture of one frame's render artifact.
//!
//! # This is NOT pixel capture
//! A [`RenderReceipt`] captures the engine's *render contract* for a single
//! frame — the frame identity ([`FrameIndex`] + [`Tick`]) plus the ordered
//! [`RenderCommandList`] the engine produced — serialized to a stable byte
//! form and hashable. It contains **no pixels**: no framebuffer, no texture,
//! no GPU readback, no canvas, no screenshot. It is captured *before* any
//! platform presentation and is identical regardless of backend.
//!
//! Pixel-level comparison (golden-image testing) is a *later* concern that
//! belongs to a backend / offscreen render-target validation path, not here.
//! This boundary answers a different question: "did the engine produce the
//! exact same render commands for this frame?" — deterministically, on any
//! target, with no platform dependency.

use axiom_kernel::{BinaryWriter, FrameIndex, Tick};

use crate::render_command::RenderCommand;
use crate::render_command_list::RenderCommandList;

/// Magic + version prefixing every serialized receipt, so the byte format is
/// self-describing and a format change is detectable.
const RECEIPT_MAGIC: u32 = 0x4158_5243; // "AXRC" (Axiom Render Capture)
                                        // v2 adds the per-`SetMaterial` albedo texture id to the serialized stream.
const RECEIPT_VERSION: u32 = 2;

/// A deterministic capture of one frame's engine-owned render artifact.
///
/// Two receipts are equal iff their frame identity and serialized command
/// stream are byte-identical. The serialization is fully deterministic
/// (fixed field order, integer/IEEE-bit encoding via the kernel
/// [`BinaryWriter`]) — the same inputs always produce the same bytes on every
/// target, with no wall-clock, randomness, or global state involved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderReceipt {
    frame_index: FrameIndex,
    tick: Tick,
    command_count: u32,
    bytes: Vec<u8>,
}

impl RenderReceipt {
    /// Capture the render artifact for one frame: its identity plus the
    /// ordered command list, serialized deterministically.
    pub fn capture(frame_index: FrameIndex, tick: Tick, commands: &RenderCommandList) -> Self {
        let mut w = BinaryWriter::new();
        w.write_u32(RECEIPT_MAGIC);
        w.write_u32(RECEIPT_VERSION);
        w.write_u64(frame_index.raw());
        w.write_u64(tick.raw());

        let command_count = commands.len().min(u32::MAX as usize) as u32;
        w.write_u32(command_count);
        commands
            .commands()
            .iter()
            .for_each(|command| Self::write_command(&mut w, command));

        RenderReceipt {
            frame_index,
            tick,
            command_count,
            bytes: w.into_bytes(),
        }
    }

    /// Serialize one command: a kind tag followed by its payload, in a fixed
    /// field order. Because commands are written in list order, the byte
    /// stream is order-sensitive by construction.
    fn write_command(w: &mut BinaryWriter, command: &RenderCommand) {
        w.write_u32(command.kind_code());
        // Branchless gated dispatch: each accessor yields `Some(payload)` for
        // exactly its own kind and `None` otherwise, so at most one closure
        // runs. The `.or_else` chain preserves the same per-kind serialization
        // the original `match` arms produced, in the same field order.
        command
            .as_clear_color()
            .map(|color| color.iter().for_each(|c| w.write_f32(*c)))
            .or_else(|| {
                command.as_camera().map(|(view, projection)| {
                    view.as_cols_array().iter().for_each(|c| w.write_f32(*c));
                    projection
                        .as_cols_array()
                        .iter()
                        .for_each(|c| w.write_f32(*c));
                })
            })
            .or_else(|| {
                command
                    .as_pipeline()
                    .map(|pipeline_id| w.write_u32(pipeline_id))
            })
            .or_else(|| command.as_mesh_id().map(|mesh_id| w.write_u64(mesh_id)))
            .or_else(|| {
                command.as_material_id().map(|material_id| {
                    w.write_u64(material_id);
                    // Same SetMaterial command, so the texture accessor is
                    // always `Some`; `unwrap_or(0)` keeps it branchless.
                    w.write_u64(command.as_material_texture_id().unwrap_or(0));
                })
            })
            .or_else(|| {
                command.as_draw_indexed().map(|(index_count, world)| {
                    w.write_u32(index_count);
                    world.as_cols_array().iter().for_each(|c| w.write_f32(*c));
                })
            });
    }

    /// The frame this receipt captured.
    pub const fn frame_index(&self) -> FrameIndex {
        self.frame_index
    }

    /// The tick this receipt captured.
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// Number of commands captured.
    pub const fn command_count(&self) -> u32 {
        self.command_count
    }

    /// The deterministic serialized byte form. Identical inputs → identical
    /// bytes; this is the canonical artifact for byte comparison.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }

    /// A deterministic 64-bit FNV-1a hash of the serialized bytes.
    ///
    /// Uses a fixed-seed FNV-1a — **not** `std`'s `DefaultHasher`, which is
    /// randomly seeded per process and would break reproducibility.
    pub fn hash(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        self.bytes
            .iter()
            .fold(FNV_OFFSET, |h, &b| (h ^ b as u64).wrapping_mul(FNV_PRIME))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Mat4;

    fn list_with(commands: &[RenderCommand]) -> RenderCommandList {
        let mut l = RenderCommandList::with_capacity(commands.len());
        for c in commands {
            l.push(*c);
        }
        l
    }

    fn cube_commands() -> Vec<RenderCommand> {
        vec![
            RenderCommand::clear_frame([0.05, 0.06, 0.08, 1.0]),
            RenderCommand::set_camera(Mat4::IDENTITY, Mat4::IDENTITY),
            RenderCommand::set_pipeline(1),
            RenderCommand::set_mesh(7),
            RenderCommand::set_material(9, 11),
            RenderCommand::draw_indexed(7, 0, 36, Mat4::IDENTITY),
        ]
    }

    #[test]
    fn same_input_produces_byte_identical_capture() {
        let list = list_with(&cube_commands());
        let a = RenderReceipt::capture(FrameIndex::new(3), Tick::new(180), &list);
        let b = RenderReceipt::capture(FrameIndex::new(3), Tick::new(180), &list);
        assert_eq!(a.bytes(), b.bytes());
        assert_eq!(a.hash(), b.hash());
        assert_eq!(a, b);
    }

    #[test]
    fn command_order_changes_the_capture() {
        let forward = list_with(&cube_commands());
        let mut swapped_commands = cube_commands();
        swapped_commands.swap(3, 4); // SetMesh <-> SetMaterial
        let swapped = list_with(&swapped_commands);

        let a = RenderReceipt::capture(FrameIndex::new(0), Tick::new(0), &forward);
        let b = RenderReceipt::capture(FrameIndex::new(0), Tick::new(0), &swapped);
        assert_ne!(a.bytes(), b.bytes());
        assert_ne!(a.hash(), b.hash());
    }

    #[test]
    fn material_texture_id_is_part_of_the_capture() {
        let a = RenderReceipt::capture(
            FrameIndex::new(0),
            Tick::new(0),
            &list_with(&[RenderCommand::set_material(9, 1)]),
        );
        let b = RenderReceipt::capture(
            FrameIndex::new(0),
            Tick::new(0),
            &list_with(&[RenderCommand::set_material(9, 2)]),
        );
        assert_ne!(a.bytes(), b.bytes());
    }

    #[test]
    fn different_frame_index_changes_the_capture() {
        let list = list_with(&cube_commands());
        let a = RenderReceipt::capture(FrameIndex::new(0), Tick::new(10), &list);
        let b = RenderReceipt::capture(FrameIndex::new(1), Tick::new(10), &list);
        assert_ne!(a, b);
        assert_ne!(a.bytes(), b.bytes());
    }

    #[test]
    fn different_tick_changes_the_capture() {
        let list = list_with(&cube_commands());
        let a = RenderReceipt::capture(FrameIndex::new(5), Tick::new(10), &list);
        let b = RenderReceipt::capture(FrameIndex::new(5), Tick::new(11), &list);
        assert_ne!(a, b);
        assert_ne!(a.bytes(), b.bytes());
    }

    #[test]
    fn command_payload_changes_the_capture() {
        let a = RenderReceipt::capture(
            FrameIndex::new(0),
            Tick::new(0),
            &list_with(&[RenderCommand::set_pipeline(1)]),
        );
        let b = RenderReceipt::capture(
            FrameIndex::new(0),
            Tick::new(0),
            &list_with(&[RenderCommand::set_pipeline(2)]),
        );
        assert_ne!(a.bytes(), b.bytes());
    }

    #[test]
    fn capture_records_identity_and_count() {
        let list = list_with(&cube_commands());
        let r = RenderReceipt::capture(FrameIndex::new(4), Tick::new(99), &list);
        assert_eq!(r.frame_index().raw(), 4);
        assert_eq!(r.tick().raw(), 99);
        assert_eq!(r.command_count(), 6);
        assert!(r.byte_len() > 0);
    }

    #[test]
    fn byte_len_equals_bytes_length_and_grows_with_commands() {
        // Kills `byte_len -> 1`: byte_len must equal bytes().len() exactly,
        // which for any real capture is far larger than 1.
        let empty =
            RenderReceipt::capture(FrameIndex::new(0), Tick::new(0), &RenderCommandList::new());
        // Header is magic(4)+version(4)+frame(8)+tick(8)+count(4) = 28 bytes.
        assert_eq!(empty.byte_len(), 28);
        assert_eq!(empty.byte_len(), empty.bytes().len());

        let full = RenderReceipt::capture(
            FrameIndex::new(0),
            Tick::new(0),
            &list_with(&cube_commands()),
        );
        assert_eq!(full.byte_len(), full.bytes().len());
        assert!(full.byte_len() > empty.byte_len());
    }

    #[test]
    fn empty_list_still_captures_identity() {
        let a = RenderReceipt::capture(FrameIndex::new(1), Tick::new(2), &RenderCommandList::new());
        let b = RenderReceipt::capture(FrameIndex::new(1), Tick::new(2), &RenderCommandList::new());
        assert_eq!(a, b);
        assert_eq!(a.command_count(), 0);
    }
}
