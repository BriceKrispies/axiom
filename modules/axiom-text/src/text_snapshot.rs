//! A deterministic snapshot of a text's evaluated glyph batch at a tick.
//!
//! The same fonts, text state, configuration, and explicit tick always produce a
//! byte-identical snapshot — the replay/verification artifact. Equality and the
//! hash are defined on those bytes, so a test can prove determinism by comparing
//! two snapshots directly.

use axiom_kernel::BinaryWriter;

use crate::glyph_batch::GlyphBatch;

/// A byte-exact, deterministic capture of a text's glyph batch at one tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSnapshot {
    tick: u64,
    glyph_count: u32,
    bytes: Vec<u8>,
}

impl TextSnapshot {
    /// Encode a batch at `tick` into a deterministic snapshot.
    pub(crate) fn of(batch: &GlyphBatch, tick: u64) -> TextSnapshot {
        let mut writer = BinaryWriter::new();
        writer.write_u64(tick);
        batch.placement.write_to(&mut writer);
        writer.write_u64(batch.glyphs.len() as u64);
        batch.glyphs.iter().for_each(|g| {
            writer.write_u64(g.order);
            writer.write_u32(g.glyph);
            writer.write_u32(g.page);
            [g.position.x, g.position.y, g.size.x, g.size.y]
                .into_iter()
                .for_each(|v| writer.write_f32(v));
            g.color.write_to(&mut writer);
            writer.write_u32(g.source_start);
        });
        TextSnapshot {
            tick,
            glyph_count: batch.glyphs.len() as u32,
            bytes: writer.into_bytes(),
        }
    }

    /// The tick this snapshot was taken at.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// The number of glyphs captured.
    pub fn glyph_count(&self) -> u32 {
        self.glyph_count
    }

    /// The raw deterministic bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// A stable content hash (a diagnostic index, not a determinism proof — byte
    /// equality is).
    pub fn hash(&self) -> u64 {
        self.bytes.iter().fold(1469598103934665603u64, |h, byte| {
            (h ^ u64::from(*byte)).wrapping_mul(1099511628211)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Rgba;
    use crate::font_registry::FontHandle;
    use crate::glyph_batch::GlyphInstance;
    use crate::placement::TextPlacement;
    use axiom_math::Vec2;

    fn instance(order: u64) -> GlyphInstance {
        GlyphInstance {
            font: FontHandle {
                index: 0,
                generation: 0,
            },
            page: 0,
            glyph: 3,
            source_start: 0,
            source_len: 1,
            position: Vec2::new(1.0, 2.0),
            size: Vec2::new(4.0, 8.0),
            uv_x: 0,
            uv_y: 0,
            uv_w: 5,
            uv_h: 7,
            color: Rgba::WHITE,
            outline_width: axiom_host::Pixels::new(0.0).unwrap(),
            outline_color: Rgba::BLACK,
            shadow_offset: Vec2::ZERO,
            shadow_color: Rgba::TRANSPARENT,
            order,
        }
    }

    #[test]
    fn identical_batches_and_ticks_snapshot_byte_identically() {
        let batch = GlyphBatch {
            placement: TextPlacement::default(),
            glyphs: vec![instance(0), instance(1)],
        };
        let a = TextSnapshot::of(&batch, 42);
        let b = TextSnapshot::of(&batch, 42);
        assert_eq!(a, b);
        assert_eq!(a.hash(), b.hash());
        assert_eq!(a.tick(), 42);
        assert_eq!(a.glyph_count(), 2);
        assert!(!a.bytes().is_empty());
    }

    #[test]
    fn different_tick_changes_the_snapshot() {
        let batch = GlyphBatch {
            placement: TextPlacement::default(),
            glyphs: vec![instance(0)],
        };
        assert_ne!(TextSnapshot::of(&batch, 1), TextSnapshot::of(&batch, 2));
    }
}
