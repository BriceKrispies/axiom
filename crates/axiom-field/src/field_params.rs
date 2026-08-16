//! The parameter table — the mechanism that prevents variant explosion.
//!
//! A `Param` node reads slot *n*. **Changing a parameter's value does not change
//! the graph's structure, therefore does not change
//! [`crate::FieldGraph::digest`], therefore cannot cause a shader recompile.**
//! That is the single most important performance property of the whole design,
//! and it is true from this first commit: the digest folds each slot's *declared
//! type*, never its value.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};
use axiom_recipe::NodeId;

use crate::field_error::{FieldError, FieldErrorCode, FieldResult};
use crate::field_type::FieldType;
use crate::field_value::FieldValue;
use crate::ids::FieldParamSlot;

/// A type code in the parameter table that names no [`FieldType`]. A property of
/// the bytes, not of any node.
const UNKNOWN_TYPE: FieldError = FieldError::at(
    FieldErrorCode::UnknownType,
    NodeId::NULL,
    "a parameter slot declares a type code that names no field type",
);

/// Undecodable parameter bytes have no node: decoding stopped before one existed.
const MALFORMED_PARAMS: FieldError = FieldError::at(
    FieldErrorCode::MalformedData,
    NodeId::NULL,
    "the parameter table could not be decoded from its bytes",
);

/// A field's parameter table: a dense `slot -> value` array.
///
/// Dense, not sparse, and ordered by slot, so the table's bytes are canonical
/// and an evaluator can index it directly.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FieldParams {
    values: Vec<FieldValue>,
}

impl FieldParams {
    /// An empty table.
    pub fn new() -> Self {
        FieldParams::default()
    }

    /// How many slots the table holds.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// The value in `slot`, or `None` when the slot is past the table.
    pub fn get(&self, slot: FieldParamSlot) -> Option<FieldValue> {
        self.values.get(slot.index()).copied()
    }

    /// Every slot's value, in slot order.
    pub fn values(&self) -> &[FieldValue] {
        &self.values
    }

    /// The table with `slot` set to `value`.
    ///
    /// Total: a slot past the current end extends the table, filling any gap
    /// with [`FieldValue::ZERO`], so the table stays dense and the wire form
    /// stays canonical whatever order an author declares slots in.
    pub fn with(self, slot: FieldParamSlot, value: FieldValue) -> Self {
        let index = slot.index();
        let len = self.values.len().max(index + 1);
        FieldParams {
            values: (0..len)
                .map(|i| {
                    [self.values.get(i).copied().unwrap_or_default(), value]
                        [usize::from(i == index)]
                })
                .collect(),
        }
    }

    /// Append the table's canonical bytes: a `u32` slot count, then per slot a
    /// `u16` type code and four `u32` lane words.
    pub(crate) fn write_to(&self, writer: &mut BinaryWriter) {
        writer.write_u32(self.values.len() as u32);
        self.values.iter().for_each(|value| {
            writer.write_u16(value.ty().code());
            value
                .words()
                .iter()
                .for_each(|word| writer.write_u32(*word));
        });
    }

    /// Append only the *shape* of the table — the slot count and each slot's
    /// declared type. This is what [`crate::FieldGraph::digest`] folds, which is
    /// exactly why a value change cannot move the digest.
    pub(crate) fn write_types_to(&self, writer: &mut BinaryWriter) {
        writer.write_u32(self.values.len() as u32);
        self.values
            .iter()
            .for_each(|value| writer.write_u16(value.ty().code()));
    }

    /// Read a table written by [`Self::write_to`]. Bounds-checked throughout: a
    /// truncated buffer fails, and an unrecognised type code fails, rather than
    /// producing a value nobody can name.
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> FieldResult<FieldParams> {
        reader
            .read_u32()
            .map_err(|_| MALFORMED_PARAMS)
            .and_then(|count| {
                (0..count).try_fold(FieldParams::new(), |table, _| {
                    read_slot(reader).map(|value| {
                        let mut values = table.values;
                        values.push(value);
                        FieldParams { values }
                    })
                })
            })
    }
}

/// Read one slot: a `u16` type code then four `u32` lane words.
fn read_slot(reader: &mut BinaryReader<'_>) -> FieldResult<FieldValue> {
    reader
        .read_u16()
        .map_err(|_| MALFORMED_PARAMS)
        .and_then(|code| FieldType::from_code(code).ok_or(UNKNOWN_TYPE))
        .and_then(|ty| {
            read_words(reader)
                .map_err(|_| MALFORMED_PARAMS)
                .map(|words| FieldValue::from_words(ty, words))
        })
}

/// Read the four lane words of one slot.
fn read_words(reader: &mut BinaryReader<'_>) -> KernelResult<[u32; 4]> {
    (0..4).try_fold([0_u32; 4], |mut words, index| {
        reader.read_u32().map(|word| {
            words[index] = word;
            words
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Vec2, Vec3};
    use axiom_recipe::Scalar;

    fn table() -> FieldParams {
        FieldParams::new()
            .with(FieldParamSlot::from_raw(0), FieldValue::scalar(Scalar::new(0.5)))
            .with(FieldParamSlot::from_raw(1), FieldValue::vec3(Vec3::new(1.0, 2.0, 3.0)))
    }

    fn bytes_of(params: &FieldParams) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        params.write_to(&mut writer);
        writer.into_bytes()
    }

    #[test]
    fn an_empty_table_has_no_slots() {
        let empty = FieldParams::new();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.get(FieldParamSlot::from_raw(0)), None);
        assert_eq!(empty.values(), &[]);
    }

    #[test]
    fn setting_a_slot_reads_it_back() {
        let params = table();
        assert_eq!(params.len(), 2);
        assert!(!params.is_empty());
        assert_eq!(
            params.get(FieldParamSlot::from_raw(0)),
            Some(FieldValue::scalar(Scalar::new(0.5)))
        );
        assert_eq!(
            params.get(FieldParamSlot::from_raw(1)),
            Some(FieldValue::vec3(Vec3::new(1.0, 2.0, 3.0)))
        );
        assert_eq!(params.get(FieldParamSlot::from_raw(2)), None);
        assert_eq!(params.values().len(), 2);
    }

    #[test]
    fn a_gap_is_filled_with_zero_so_the_table_stays_dense() {
        let sparse = FieldParams::new().with(
            FieldParamSlot::from_raw(3),
            FieldValue::vec2(Vec2::new(1.0, 1.0)),
        );
        assert_eq!(sparse.len(), 4);
        assert_eq!(sparse.get(FieldParamSlot::from_raw(0)), Some(FieldValue::ZERO));
        assert_eq!(sparse.get(FieldParamSlot::from_raw(2)), Some(FieldValue::ZERO));
        assert_eq!(
            sparse.get(FieldParamSlot::from_raw(3)),
            Some(FieldValue::vec2(Vec2::new(1.0, 1.0)))
        );
    }

    #[test]
    fn setting_an_existing_slot_replaces_it_without_growing_the_table() {
        let params = table().with(
            FieldParamSlot::from_raw(0),
            FieldValue::scalar(Scalar::new(9.0)),
        );
        assert_eq!(params.len(), 2);
        assert_eq!(
            params.get(FieldParamSlot::from_raw(0)),
            Some(FieldValue::scalar(Scalar::new(9.0)))
        );
    }

    #[test]
    fn the_table_round_trips_through_its_bytes() {
        let params = table();
        let bytes = bytes_of(&params);
        let decoded = FieldParams::read_from(&mut BinaryReader::new(&bytes))
            .expect("a table writes bytes it can read back");
        assert_eq!(decoded, params);
    }

    #[test]
    fn an_empty_table_round_trips() {
        let bytes = bytes_of(&FieldParams::new());
        assert_eq!(
            FieldParams::read_from(&mut BinaryReader::new(&bytes)),
            Ok(FieldParams::new())
        );
    }

    #[test]
    fn every_truncation_of_the_table_fails_cleanly() {
        let bytes = bytes_of(&table());
        (0..bytes.len()).for_each(|n| {
            let error = FieldParams::read_from(&mut BinaryReader::new(&bytes[..n]))
                .expect_err("a truncated table cannot decode");
            assert_eq!(error.kind(), FieldErrorCode::MalformedData);
            assert_eq!(error.node(), NodeId::NULL);
        });
    }

    #[test]
    fn an_unknown_type_code_is_rejected() {
        let mut writer = BinaryWriter::new();
        writer.write_u32(1);
        writer.write_u16(99);
        (0..4).for_each(|_| writer.write_u32(0));
        let bytes = writer.into_bytes();
        let error = FieldParams::read_from(&mut BinaryReader::new(&bytes))
            .expect_err("type code 99 names no field type");
        assert_eq!(error.kind(), FieldErrorCode::UnknownType);
        assert_eq!(error.code(), 4);
    }

    #[test]
    fn the_type_only_form_carries_the_shape_and_not_the_values() {
        let one = table();
        let other = table().with(
            FieldParamSlot::from_raw(0),
            FieldValue::scalar(Scalar::new(-42.0)),
        );
        let shape_of = |params: &FieldParams| {
            let mut writer = BinaryWriter::new();
            params.write_types_to(&mut writer);
            writer.into_bytes()
        };
        assert_eq!(shape_of(&one), shape_of(&other));
        assert_ne!(bytes_of(&one), bytes_of(&other));
        // The shape is 4 count bytes + one u16 per slot.
        assert_eq!(shape_of(&one).len(), 4 + 2 * one.len());
    }
}
