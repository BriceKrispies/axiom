//! The canonical byte encoding of a stored state's contents.
//!
//! Every declared state is stored as bytes, so the substrate can snapshot,
//! digest, diff, patch, migrate and describe it without naming a single game
//! type. Typing is recovered at the edges: the caller's key type says what to
//! decode into, and the stored [`crate::StateShapeId`] says whether that is
//! allowed.
//!
//! ```text
//! cell     := the value's own Reflect bytes
//! table    := u32 row_count, then per row: byte_slice(key), byte_slice(value)
//!             — rows ascending by ENCODED KEY BYTES
//! sequence := u32 item_count, then per item: byte_slice(item)
//!             — positional order, which is the meaning
//! ```
//!
//! ## Two orders, on purpose
//!
//! A table has a *typed* order (ascending by `K: Ord`, what game code sees) and a
//! *canonical* order (ascending by encoded key bytes, what the stored payload
//! uses). They coincide for single-byte and byte-string keys and diverge for
//! multi-byte integers, because little-endian bytes do not sort like the numbers
//! they encode.
//!
//! The canonical order has to be byte order, because the engine must insert,
//! remove and diff a row *without knowing `K`* — that is the whole point of
//! storing bytes. The property that actually matters is preserved either way:
//! the canonical bytes are a pure function of the table's logical content and
//! owe nothing to insertion order, so hashes, equality and diffs are stable. The
//! typed order is restored on decode by collecting into a `BTreeMap`.
//!
//! If that divergence ever becomes load-bearing, the fix belongs one layer down:
//! an order-preserving key encoding in the kernel (big-endian unsigned,
//! sign-flipped signed) would make the two orders coincide for every scalar.

use axiom_kernel::{BinaryReader, BinaryWriter, Reflect};

use crate::state_error::StateError;
use crate::state_error_code::StateErrorCode;
use crate::state_sequence::StateSequence;
use crate::state_table::StateTable;

/// A row of a table as the engine sees it: two opaque byte strings.
pub(crate) type RawRow = (Vec<u8>, Vec<u8>);

/// Wrap a decode failure from the kernel as a state failure.
fn corrupted(cause: axiom_kernel::KernelError) -> StateError {
    StateError::new(
        StateErrorCode::CorruptedSnapshot,
        "a stored state payload could not be decoded",
    )
    .caused_by(cause)
}

/// Wrap a value-level decode failure — the bytes were well-formed but did not
/// decode as the requested type.
fn mismatched(cause: axiom_kernel::KernelError) -> StateError {
    StateError::new(
        StateErrorCode::StateTypeMismatch,
        "a stored value did not decode as the requested type",
    )
    .caused_by(cause)
}

/// Encode one value as a cell payload.
pub(crate) fn encode_cell<T: Reflect>(value: &T) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    value.reflect_write(&mut writer);
    writer.into_bytes()
}

/// Decode a cell payload.
pub(crate) fn decode_cell<T: Reflect>(bytes: &[u8]) -> Result<T, StateError> {
    T::reflect_read(&mut BinaryReader::new(bytes)).map_err(mismatched)
}

/// Write engine-visible rows as a table payload, in canonical key-byte order.
pub(crate) fn write_rows(rows: &[RawRow]) -> Vec<u8> {
    let mut ordered: Vec<&RawRow> = rows.iter().collect();
    ordered.sort_by(|left, right| left.0.cmp(&right.0));
    let mut writer = BinaryWriter::new();
    writer.write_u32(ordered.len() as u32);
    ordered.iter().for_each(|(key, value)| {
        writer.write_byte_slice(key);
        writer.write_byte_slice(value);
    });
    writer.into_bytes()
}

/// Parse a table payload into engine-visible rows, without knowing the types.
pub(crate) fn parse_rows(bytes: &[u8]) -> Result<Vec<RawRow>, StateError> {
    let mut reader = BinaryReader::new(bytes);
    reader
        .read_u32()
        .map_err(corrupted)
        .and_then(|count| {
            (0..count).try_fold(Vec::with_capacity(count as usize), |mut rows, _| {
                reader
                    .read_byte_slice()
                    .map_err(corrupted)
                    .map(<[u8]>::to_vec)
                    .and_then(|key| {
                        reader
                            .read_byte_slice()
                            .map_err(corrupted)
                            .map(|value| (key, value.to_vec()))
                    })
                    .map(|row| {
                        rows.push(row);
                        rows
                    })
            })
        })
}

/// Encode a typed table as a payload.
pub(crate) fn encode_table<K: Reflect + Ord, V: Reflect>(table: &StateTable<K, V>) -> Vec<u8> {
    let rows: Vec<RawRow> = table
        .rows()
        .into_iter()
        .map(|(key, value)| (encode_cell(key), encode_cell(value)))
        .collect();
    write_rows(&rows)
}

/// Decode a payload into a typed table, restoring `K: Ord` order.
pub(crate) fn decode_table<K: Reflect + Ord + Clone, V: Reflect>(
    bytes: &[u8],
) -> Result<StateTable<K, V>, StateError> {
    parse_rows(bytes).and_then(|rows| {
        rows.into_iter()
            .try_fold(StateTable::new(), |table, (key, value)| {
                decode_cell::<K>(&key).and_then(|key| {
                    decode_cell::<V>(&value).map(|value| table.with(key, value))
                })
            })
    })
}

/// Write engine-visible items as a sequence payload, preserving order.
pub(crate) fn write_items(items: &[Vec<u8>]) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    writer.write_u32(items.len() as u32);
    items
        .iter()
        .for_each(|item| writer.write_byte_slice(item));
    writer.into_bytes()
}

/// Parse a sequence payload into engine-visible items.
pub(crate) fn parse_items(bytes: &[u8]) -> Result<Vec<Vec<u8>>, StateError> {
    let mut reader = BinaryReader::new(bytes);
    reader.read_u32().map_err(corrupted).and_then(|count| {
        (0..count).try_fold(Vec::with_capacity(count as usize), |mut items, _| {
            reader
                .read_byte_slice()
                .map_err(corrupted)
                .map(<[u8]>::to_vec)
                .map(|item| {
                    items.push(item);
                    items
                })
        })
    })
}

/// Encode a typed sequence as a payload.
pub(crate) fn encode_sequence<T: Reflect>(sequence: &StateSequence<T>) -> Vec<u8> {
    let items: Vec<Vec<u8>> = sequence.items().iter().map(encode_cell).collect();
    write_items(&items)
}

/// Decode a payload into a typed sequence.
pub(crate) fn decode_sequence<T: Reflect>(bytes: &[u8]) -> Result<StateSequence<T>, StateError> {
    parse_items(bytes).and_then(|items| {
        items
            .into_iter()
            .try_fold(StateSequence::new(), |sequence, item| {
                decode_cell::<T>(&item).map(|item| sequence.appended(item))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cell_value_round_trips() {
        let encoded = encode_cell(&42_u64);
        assert_eq!(decode_cell::<u64>(&encoded), Ok(42));
    }

    #[test]
    fn a_cell_that_does_not_decode_as_the_requested_type_is_a_mismatch() {
        // A `u8` payload is one byte; asking for a `u64` runs out of bytes.
        let encoded = encode_cell(&1_u8);
        let error = decode_cell::<u64>(&encoded).unwrap_err();
        assert_eq!(error.code(), StateErrorCode::StateTypeMismatch);
        assert!(error.cause().is_some());
    }

    #[test]
    fn a_table_round_trips_and_restores_typed_order() {
        let table: StateTable<u32, u64> = StateTable::new().with(3, 30).with(1, 10).with(2, 20);
        let decoded = decode_table::<u32, u64>(&encode_table(&table)).expect("round trip");
        assert_eq!(decoded, table);
        assert_eq!(decoded.keys(), vec![&1, &2, &3]);
    }

    #[test]
    fn an_empty_table_round_trips() {
        let table: StateTable<u32, u64> = StateTable::new();
        assert_eq!(
            decode_table::<u32, u64>(&encode_table(&table)).expect("round trip"),
            table
        );
    }

    #[test]
    fn table_bytes_are_canonical_regardless_of_insertion_order() {
        let forwards: StateTable<u32, u64> = StateTable::new().with(1, 10).with(2, 20);
        let backwards: StateTable<u32, u64> = StateTable::new().with(2, 20).with(1, 10);
        assert_eq!(encode_table(&forwards), encode_table(&backwards));
    }

    #[test]
    fn table_rows_are_stored_ascending_by_encoded_key_bytes() {
        let table: StateTable<u32, u64> = StateTable::new().with(1, 10).with(256, 20);
        let rows = parse_rows(&encode_table(&table)).expect("parses");
        let keys: Vec<Vec<u8>> = rows.iter().map(|(key, _)| key.clone()).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        // 256u32 encodes little-endian as 00 01 00 00 and therefore sorts BEFORE
        // 1u32's 01 00 00 00 — the documented divergence from `K: Ord`.
        assert_eq!(keys[0], vec![0, 1, 0, 0]);
    }

    #[test]
    fn a_truncated_table_payload_is_corrupt() {
        let table: StateTable<u32, u64> = StateTable::new().with(1, 10);
        let encoded = encode_table(&table);
        let error = parse_rows(&encoded[..encoded.len() - 1]).unwrap_err();
        assert_eq!(error.code(), StateErrorCode::CorruptedSnapshot);
        assert!(error.cause().is_some());
    }

    #[test]
    fn a_table_payload_with_no_count_is_corrupt() {
        assert_eq!(
            parse_rows(&[]).unwrap_err().code(),
            StateErrorCode::CorruptedSnapshot
        );
    }

    #[test]
    fn a_table_row_whose_value_does_not_decode_is_a_mismatch() {
        // One row keyed by a u32 whose value is a single byte — not a u64.
        let rows = vec![(encode_cell(&1_u32), encode_cell(&1_u8))];
        let error = decode_table::<u32, u64>(&write_rows(&rows)).unwrap_err();
        assert_eq!(error.code(), StateErrorCode::StateTypeMismatch);
    }

    #[test]
    fn a_table_row_whose_key_does_not_decode_is_a_mismatch() {
        let rows = vec![(encode_cell(&1_u8), encode_cell(&1_u64))];
        let error = decode_table::<u32, u64>(&write_rows(&rows)).unwrap_err();
        assert_eq!(error.code(), StateErrorCode::StateTypeMismatch);
    }

    #[test]
    fn a_sequence_round_trips_and_keeps_its_order() {
        let sequence: StateSequence<u32> = StateSequence::new().appended(3).appended(1).appended(2);
        let decoded = decode_sequence::<u32>(&encode_sequence(&sequence)).expect("round trip");
        assert_eq!(decoded, sequence);
        assert_eq!(decoded.items(), &[3, 1, 2]);
    }

    #[test]
    fn an_empty_sequence_round_trips() {
        let sequence: StateSequence<u32> = StateSequence::new();
        assert_eq!(
            decode_sequence::<u32>(&encode_sequence(&sequence)).expect("round trip"),
            sequence
        );
    }

    #[test]
    fn a_truncated_sequence_payload_is_corrupt() {
        let sequence: StateSequence<u32> = StateSequence::new().appended(1);
        let encoded = encode_sequence(&sequence);
        let error = parse_items(&encoded[..encoded.len() - 1]).unwrap_err();
        assert_eq!(error.code(), StateErrorCode::CorruptedSnapshot);
    }

    #[test]
    fn a_sequence_payload_with_no_count_is_corrupt() {
        assert_eq!(
            parse_items(&[]).unwrap_err().code(),
            StateErrorCode::CorruptedSnapshot
        );
    }

    #[test]
    fn a_sequence_item_that_does_not_decode_is_a_mismatch() {
        let items = vec![encode_cell(&1_u8)];
        let error = decode_sequence::<u64>(&write_items(&items)).unwrap_err();
        assert_eq!(error.code(), StateErrorCode::StateTypeMismatch);
    }

    #[test]
    fn engine_visible_rows_and_items_round_trip_without_types() {
        let rows: Vec<RawRow> = vec![(vec![1], vec![10]), (vec![2], vec![20])];
        assert_eq!(parse_rows(&write_rows(&rows)).expect("parses"), rows);

        let items: Vec<Vec<u8>> = vec![vec![1], vec![2, 3]];
        assert_eq!(parse_items(&write_items(&items)).expect("parses"), items);
    }
}
