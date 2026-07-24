//! The count-prefixed table codec shared by every `.axfont` section.
//!
//! One clearly-named concept: a `u64` length followed by that many records.
//! Every table in the compiled font (codepoints, glyph metrics, kerning, atlas
//! pages, glyph rasters, size layers) is written and read through these two
//! functions, so the length-prefix convention and the oversized-count guard live
//! in exactly one place.

use axiom_kernel::{BinaryReader, BinaryWriter};

use crate::text_error::{TextError, TextResult};

/// The largest record count any single table may declare. A count past this is
/// rejected as `MalformedFont` before a byte of the body is read, so a corrupt
/// length cannot drive an unbounded allocation.
pub(crate) const MAX_TABLE_LEN: u64 = 1 << 24;

/// Write a `u64` count then each record via `write_item`.
pub(crate) fn write_table<T>(
    writer: &mut BinaryWriter,
    items: &[T],
    mut write_item: impl FnMut(&T, &mut BinaryWriter),
) {
    writer.write_u64(items.len() as u64);
    items.iter().for_each(|item| write_item(item, writer));
}

/// Read a table written by [`write_table`]. A declared count past
/// [`MAX_TABLE_LEN`], or any short record, is `MalformedFont`.
pub(crate) fn read_table<T>(
    reader: &mut BinaryReader<'_>,
    mut read_item: impl FnMut(&mut BinaryReader<'_>) -> TextResult<T>,
) -> TextResult<Vec<T>> {
    reader
        .read_u64()
        .map_err(|_| TextError::MalformedFont)
        .and_then(|count| {
            (count <= MAX_TABLE_LEN)
                .then_some(count)
                .ok_or(TextError::MalformedFont)
        })
        .and_then(|count| {
            (0..count).try_fold(Vec::new(), |mut items, _| {
                read_item(reader).map(|item| {
                    items.push(item);
                    items
                })
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_u32(value: &u32, writer: &mut BinaryWriter) {
        writer.write_u32(*value);
    }

    fn read_u32(reader: &mut BinaryReader<'_>) -> TextResult<u32> {
        reader.read_u32().map_err(|_| TextError::MalformedFont)
    }

    #[test]
    fn round_trips_a_table() {
        let mut w = BinaryWriter::new();
        write_table(&mut w, &[10u32, 20, 30], write_u32);
        let bytes = w.into_bytes();
        assert_eq!(
            read_table(&mut BinaryReader::new(&bytes), read_u32).unwrap(),
            vec![10, 20, 30]
        );
    }

    #[test]
    fn empty_table_round_trips() {
        let mut w = BinaryWriter::new();
        write_table(&mut w, &[] as &[u32], write_u32);
        let bytes = w.into_bytes();
        assert_eq!(
            read_table(&mut BinaryReader::new(&bytes), read_u32).unwrap(),
            Vec::<u32>::new()
        );
    }

    #[test]
    fn oversized_count_is_rejected_before_reading_body() {
        let mut w = BinaryWriter::new();
        w.write_u64(MAX_TABLE_LEN + 1);
        let bytes = w.into_bytes();
        assert_eq!(
            read_table(&mut BinaryReader::new(&bytes), read_u32),
            Err(TextError::MalformedFont)
        );
    }

    #[test]
    fn truncated_count_and_truncated_body_are_malformed() {
        assert_eq!(
            read_table(&mut BinaryReader::new(&[0, 0]), read_u32),
            Err(TextError::MalformedFont)
        );
        let mut w = BinaryWriter::new();
        w.write_u64(2);
        w.write_u32(1); // only one of the two promised records
        let bytes = w.into_bytes();
        assert_eq!(
            read_table(&mut BinaryReader::new(&bytes), read_u32),
            Err(TextError::MalformedFont)
        );
    }
}
