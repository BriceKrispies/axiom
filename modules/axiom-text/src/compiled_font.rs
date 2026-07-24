//! The compiled `.axfont` runtime font asset: the one font representation the
//! text runtime consumes. External containers (TTF/OTF/WOFF/WOFF2) are compiled
//! into this by the offline `axiom-font-import` tool; nothing here parses a font
//! container or rasterizes a glyph.

use axiom_kernel::{BinaryReader, BinaryWriter};

use crate::codepoint_entry::CodepointEntry;
use crate::face_metrics::FaceMetrics;
use crate::font_table::{read_table, write_table};
use crate::glyph_metric::GlyphMetric;
use crate::import_provenance::ImportProvenance;
use crate::kern_pair::KernPair;
use crate::size_layer::SizeLayer;
use crate::text_error::{TextError, TextResult};

/// The eight magic bytes every `.axfont` opens with.
pub(crate) const MAGIC: [u8; 8] = *b"AXFONT\0\0";

/// The format version this runtime reads and writes.
pub(crate) const VERSION: u32 = 1;

/// A whole compiled font, decoded and validated from `.axfont` bytes. All tables
/// are stored sorted (codepoints by codepoint, glyphs by index, kerning by pair)
/// so every lookup is a binary search and the ordering is a decode-time integrity
/// check. Layout reads its size-independent metrics; a backend samples the
/// [`SizeLayer`] atlas pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledFont {
    /// A stable hash of the source font bytes the asset was compiled from
    /// (provenance/identity; never a determinism proof on its own).
    pub source_hash: u64,
    /// Face-wide metrics (names, em size, vertical extents, replacement).
    pub metrics: FaceMetrics,
    /// `codepoint → glyph` map, sorted ascending by codepoint.
    pub codepoints: Vec<CodepointEntry>,
    /// Per-glyph design-unit metrics, sorted ascending by glyph index.
    pub glyphs: Vec<GlyphMetric>,
    /// Kerning pairs, sorted ascending by `(left, right)`.
    pub kerning: Vec<KernPair>,
    /// One atlas layer per imported raster size.
    pub size_layers: Vec<SizeLayer>,
    /// How the asset was generated (host-neutral importer settings).
    pub provenance: ImportProvenance,
}

impl CompiledFont {
    /// The font family this asset advertises.
    pub fn family(&self) -> &str {
        &self.metrics.family
    }

    /// Resolve a codepoint to a glyph index, or `None` if unmapped.
    pub fn glyph_for_codepoint(&self, codepoint: u32) -> Option<u32> {
        self.codepoints
            .binary_search_by_key(&codepoint, |entry| entry.codepoint)
            .ok()
            .and_then(|index| self.codepoints.get(index))
            .map(|entry| entry.glyph)
    }

    /// The design-unit metrics for a glyph index, or `None` if absent.
    pub fn metric(&self, glyph: u32) -> Option<GlyphMetric> {
        self.glyphs
            .binary_search_by_key(&glyph, |g| g.glyph)
            .ok()
            .and_then(|index| self.glyphs.get(index))
            .copied()
    }

    /// The kerning adjustment for an ordered glyph pair (design units, `0` when
    /// the pair is unlisted).
    pub fn kern(&self, left: u32, right: u32) -> i32 {
        self.kerning
            .binary_search_by_key(&(left, right), |pair| pair.key())
            .ok()
            .and_then(|index| self.kerning.get(index))
            .map(|pair| pair.adjust)
            .unwrap_or(0)
    }

    /// The glyph index the font uses for unmapped characters.
    pub fn replacement_glyph(&self) -> Option<u32> {
        self.glyph_for_codepoint(self.metrics.replacement_codepoint)
    }

    /// The size layer whose raster size is nearest `pixel_size`.
    pub fn nearest_layer(&self, pixel_size: u32) -> Option<&SizeLayer> {
        self.size_layers
            .iter()
            .min_by_key(|layer| layer.pixel_size.abs_diff(pixel_size))
    }

    /// Serialize to `.axfont` bytes. Deterministic: the same font always yields
    /// the same bytes (no timestamps, paths, or map order).
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        MAGIC.iter().for_each(|byte| writer.write_u8(*byte));
        writer.write_u32(VERSION);
        writer.write_u64(self.source_hash);
        self.metrics.write_to(&mut writer);
        write_table(&mut writer, &self.codepoints, |entry, w| entry.write_to(w));
        write_table(&mut writer, &self.glyphs, |glyph, w| glyph.write_to(w));
        write_table(&mut writer, &self.kerning, |pair, w| pair.write_to(w));
        write_table(&mut writer, &self.size_layers, SizeLayer::write_to);
        self.provenance.write_to(&mut writer);
        writer.into_bytes()
    }

    /// Decode and fully validate `.axfont` bytes. Bad magic or a truncated
    /// section is `MalformedFont`; an unknown version is `UnsupportedFontVersion`;
    /// semantic problems surface as their specific error.
    pub fn decode(bytes: &[u8]) -> TextResult<CompiledFont> {
        let mut reader = BinaryReader::new(bytes);
        read_magic(&mut reader)
            .and_then(|()| read_version(&mut reader))
            .and_then(|()| reader.read_u64().map_err(|_| TextError::MalformedFont))
            .and_then(|source_hash| FaceMetrics::read_from(&mut reader).map(|m| (source_hash, m)))
            .and_then(|(source_hash, metrics)| {
                read_table(&mut reader, CodepointEntry::read_from)
                    .map(|codepoints| (source_hash, metrics, codepoints))
            })
            .and_then(|(source_hash, metrics, codepoints)| {
                read_table(&mut reader, GlyphMetric::read_from)
                    .map(|glyphs| (source_hash, metrics, codepoints, glyphs))
            })
            .and_then(|(source_hash, metrics, codepoints, glyphs)| {
                read_table(&mut reader, KernPair::read_from)
                    .map(|kerning| (source_hash, metrics, codepoints, glyphs, kerning))
            })
            .and_then(|(source_hash, metrics, codepoints, glyphs, kerning)| {
                read_table(&mut reader, SizeLayer::read_from)
                    .map(|layers| (source_hash, metrics, codepoints, glyphs, kerning, layers))
            })
            .and_then(
                |(source_hash, metrics, codepoints, glyphs, kerning, size_layers)| {
                    ImportProvenance::read_from(&mut reader).map(|provenance| CompiledFont {
                        source_hash,
                        metrics,
                        codepoints,
                        glyphs,
                        kerning,
                        size_layers,
                        provenance,
                    })
                },
            )
            .and_then(|font| font.validate().map(|()| font))
    }

    /// Verify every structural invariant a decoder relies on: valid metrics,
    /// strictly-ascending tables, glyph references that resolve, a real
    /// replacement glyph, and well-formed size layers.
    pub fn validate(&self) -> TextResult<()> {
        self.metrics
            .validate()
            .and_then(|()| {
                self.codepoints
                    .windows(2)
                    .all(|pair| pair[0].codepoint < pair[1].codepoint)
                    .then_some(())
                    .ok_or(TextError::DuplicateCodepoint)
            })
            .and_then(|()| {
                self.glyphs
                    .windows(2)
                    .all(|pair| pair[0].glyph < pair[1].glyph)
                    .then_some(())
                    .ok_or(TextError::DuplicateGlyph)
            })
            .and_then(|()| {
                self.kerning
                    .windows(2)
                    .all(|pair| pair[0].key() < pair[1].key())
                    .then_some(())
                    .ok_or(TextError::DuplicateGlyph)
            })
            .and_then(|()| {
                self.codepoints
                    .iter()
                    .all(|entry| self.metric(entry.glyph).is_some())
                    .then_some(())
                    .ok_or(TextError::MissingGlyph)
            })
            .and_then(|()| {
                self.replacement_glyph()
                    .and_then(|glyph| self.metric(glyph))
                    .map(|_| ())
                    .ok_or(TextError::MissingReplacementGlyph)
            })
            .and_then(|()| self.size_layers.iter().try_for_each(SizeLayer::validate))
    }
}

/// Read and check the 8 magic bytes.
fn read_magic(reader: &mut BinaryReader<'_>) -> TextResult<()> {
    reader
        .read_bytes::<8>()
        .map_err(|_| TextError::MalformedFont)
        .and_then(|got| (got == MAGIC).then_some(()).ok_or(TextError::MalformedFont))
}

/// Read and check the format version.
fn read_version(reader: &mut BinaryReader<'_>) -> TextResult<()> {
    reader
        .read_u32()
        .map_err(|_| TextError::MalformedFont)
        .and_then(|version| {
            (version == VERSION)
                .then_some(())
                .ok_or(TextError::UnsupportedFontVersion)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fallback_font::default_font;

    #[test]
    fn round_trips_and_looks_up() {
        let font = default_font();
        let bytes = font.encode();
        let decoded = CompiledFont::decode(&bytes).unwrap();
        assert_eq!(decoded, font);
        // Lookups.
        let h = font.glyph_for_codepoint('H' as u32).unwrap();
        assert!(font.metric(h).is_some());
        assert_eq!(font.kern(h, h), 0, "unlisted pairs kern to zero");
        assert!(font.replacement_glyph().is_some());
        assert_eq!(font.nearest_layer(9999).unwrap().pixel_size, 8);
        assert!(font.metric(99999).is_none());
    }

    #[test]
    fn kerning_pairs_are_looked_up_and_round_trip() {
        let mut font = default_font();
        // The fallback has no kerning; add a pair so the hit path and the
        // kerning write-closure are exercised.
        font.kerning = vec![KernPair {
            left: 1,
            right: 2,
            adjust: -30,
        }];
        assert_eq!(font.validate(), Ok(()));
        assert_eq!(font.kern(1, 2), -30, "listed pair returns its adjustment");
        assert_eq!(font.kern(9, 9), 0, "unlisted pair is zero");
        let bytes = font.encode();
        assert_eq!(CompiledFont::decode(&bytes).unwrap(), font);
    }

    #[test]
    fn bad_magic_and_bad_version_are_distinct() {
        let mut bytes = default_font().encode();
        let good = bytes.clone();
        bytes[0] = 0;
        assert_eq!(CompiledFont::decode(&bytes), Err(TextError::MalformedFont));
        // Corrupt the version u32 (immediately after the 8 magic bytes).
        let mut wrong_version = good;
        wrong_version[8] = 9;
        assert_eq!(
            CompiledFont::decode(&wrong_version),
            Err(TextError::UnsupportedFontVersion)
        );
    }

    #[test]
    fn truncation_anywhere_is_malformed() {
        let bytes = default_font().encode();
        assert_eq!(CompiledFont::decode(&[]), Err(TextError::MalformedFont));
        assert_eq!(
            CompiledFont::decode(&bytes[..bytes.len() - 1]),
            Err(TextError::MalformedFont)
        );
    }

    #[test]
    fn duplicate_or_unsorted_codepoint_is_rejected() {
        let mut font = default_font();
        // Break the ascending-codepoint invariant.
        font.codepoints.reverse();
        assert_eq!(font.validate(), Err(TextError::DuplicateCodepoint));
    }

    #[test]
    fn duplicate_or_unsorted_glyph_is_rejected() {
        let mut font = default_font();
        font.glyphs.reverse();
        assert_eq!(font.validate(), Err(TextError::DuplicateGlyph));
    }

    #[test]
    fn unsorted_kerning_is_rejected() {
        let mut font = default_font();
        font.kerning = vec![
            KernPair {
                left: 5,
                right: 1,
                adjust: 0,
            },
            KernPair {
                left: 1,
                right: 1,
                adjust: 0,
            },
        ];
        assert_eq!(font.validate(), Err(TextError::DuplicateGlyph));
    }

    #[test]
    fn codepoint_pointing_at_missing_glyph_is_rejected() {
        let mut font = default_font();
        font.codepoints.push(CodepointEntry {
            codepoint: 0xFFFF,
            glyph: 100000,
        });
        font.codepoints.sort_by_key(|entry| entry.codepoint);
        assert_eq!(font.validate(), Err(TextError::MissingGlyph));
    }

    #[test]
    fn missing_replacement_glyph_is_rejected() {
        let mut font = default_font();
        font.metrics.replacement_codepoint = 0x1FFFF; // unmapped
        assert_eq!(font.validate(), Err(TextError::MissingReplacementGlyph));
    }

    #[test]
    fn invalid_metrics_and_bad_size_layer_propagate() {
        let mut bad_metrics = default_font();
        bad_metrics.metrics.units_per_em = 0;
        assert_eq!(bad_metrics.validate(), Err(TextError::InvalidFontMetrics));

        let mut bad_layer = default_font();
        bad_layer.size_layers[0].pixel_size = 0;
        assert_eq!(bad_layer.validate(), Err(TextError::InvalidAtlasDimensions));
    }
}
