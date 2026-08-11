//! A set of proposed changes, and how two sets collide.

use std::collections::BTreeMap;

use axiom_kernel::{BinaryReader, BinaryWriter, StableHash};

use crate::state_conflict::{StateConflict, StateOpRef};
use crate::state_error::StateError;
use crate::state_error_code::StateErrorCode;
use crate::state_id::StateId;
use crate::state_op::StateOp;
use crate::state_op_kind::StateOpKind;
use crate::state_origin::StateOrigin;
use crate::StateResult;

/// `"AXSP"` little-endian — the leading bytes of a serialized patch.
const MAGIC: u32 = 0x5053_5841;

/// An ordered set of proposed changes.
///
/// A patch is a *description* of change, not a change: it can be built, stored,
/// sent, hashed and inspected without touching any state. Applying it is a pure
/// function of a snapshot and the patch.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatePatch {
    ops: Vec<StateOp>,
}

impl StatePatch {
    /// Assemble from already-built operations.
    pub(crate) const fn from_ops(ops: Vec<StateOp>) -> Self {
        StatePatch { ops }
    }

    /// The operations, in the order they were declared.
    pub fn ops(&self) -> &[StateOp] {
        &self.ops
    }

    /// How many changes this patch proposes.
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Whether this patch proposes nothing.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// The patch's digest — order-sensitive, since order is part of its meaning.
    pub fn hash(&self) -> StableHash {
        let words: Vec<u64> = self
            .ops
            .iter()
            .flat_map(|op| {
                [
                    u64::from(op.kind().code()),
                    op.target().raw(),
                    op.origin().raw(),
                    u64::from(op.index()),
                    StableHash::of_bytes(op.key()).raw(),
                    StableHash::of_bytes(op.value()).raw(),
                ]
            })
            .collect();
        StableHash::of_words(&words)
    }

    /// Serialize to canonical little-endian bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        writer.write_u32(MAGIC);
        writer.write_u32(self.ops.len() as u32);
        self.ops.iter().for_each(|op| {
            writer.write_u8(op.kind().code());
            writer.write_u64(op.target().raw());
            writer.write_u64(op.origin().raw());
            writer.write_u32(op.index());
            writer.write_byte_slice(op.key());
            writer.write_byte_slice(op.value());
        });
        writer.into_bytes()
    }

    /// Deserialize from canonical bytes.
    pub fn from_bytes(bytes: &[u8]) -> StateResult<Self> {
        let mut reader = BinaryReader::new(bytes);
        read_magic(&mut reader)
            .and_then(|()| reader.read_u32().map_err(corrupted))
            .and_then(|count| {
                (0..count).try_fold(Vec::with_capacity(count as usize), |mut ops, _| {
                    read_op(&mut reader).map(|op| {
                        ops.push(op);
                        ops
                    })
                })
            })
            .map(StatePatch::from_ops)
    }
}

fn corrupted(cause: axiom_kernel::KernelError) -> StateError {
    StateError::new(
        StateErrorCode::CorruptedSnapshot,
        "the patch bytes are malformed or truncated",
    )
    .caused_by(cause)
}

fn read_magic(reader: &mut BinaryReader<'_>) -> StateResult<()> {
    reader.read_u32().map_err(corrupted).and_then(|magic| {
        (magic == MAGIC).then_some(()).ok_or(StateError::new(
            StateErrorCode::CorruptedSnapshot,
            "these bytes are not an Axiom state patch",
        ))
    })
}

fn read_op(reader: &mut BinaryReader<'_>) -> StateResult<StateOp> {
    reader
        .read_u8()
        .map_err(corrupted)
        .and_then(|code| {
            StateOpKind::from_code(code).ok_or(StateError::new(
                StateErrorCode::CorruptedSnapshot,
                "the stored operation code names no state operation",
            ))
        })
        .and_then(|kind| {
            reader
                .read_u64()
                .map_err(corrupted)
                .map(|target| (kind, StateId::from_raw(target)))
        })
        .and_then(|(kind, target)| {
            reader
                .read_u64()
                .map_err(corrupted)
                .map(|origin| (kind, target, StateOrigin::from_raw(origin)))
        })
        .and_then(|(kind, target, origin)| {
            reader
                .read_u32()
                .map_err(corrupted)
                .map(|index| (kind, target, origin, index))
        })
        .and_then(|(kind, target, origin, index)| {
            reader
                .read_byte_slice()
                .map_err(corrupted)
                .map(<[u8]>::to_vec)
                .and_then(|key| {
                    reader
                        .read_byte_slice()
                        .map_err(corrupted)
                        .map(|value| {
                            StateOp::new(kind, target, origin, key, index, value.to_vec())
                        })
                })
        })
}

/// The addressable location of an operation: which state, and which part of it.
type Granule = (u64, Vec<u8>);

/// Find the first pair of operations, across `patches`, that cannot both apply.
///
/// Two operations collide when they target the same state, come from
/// **different** origins, and either spans the whole state or addresses the same
/// granule. Operations from the *same* origin never collide: one author wrote
/// them in a deliberate order, and honouring that order is the author's intent.
/// Conflicts therefore arise exactly where they should — between independently
/// authored patches.
///
/// Candidates are visited in `(patch index, operation index)` order, so which
/// conflict is reported is a deterministic function of the input.
pub fn detect_conflict(patches: &[StatePatch]) -> Option<StateConflict> {
    let mut seen: BTreeMap<Granule, StateOpRef> = BTreeMap::new();
    let mut spanning: BTreeMap<u64, StateOpRef> = BTreeMap::new();
    patches
        .iter()
        .enumerate()
        .flat_map(|(patch_index, patch)| {
            patch
                .ops()
                .iter()
                .enumerate()
                .map(move |(op_index, op)| (patch_index as u32, op_index as u32, op))
        })
        .find_map(|(patch_index, op_index, op)| {
            let here = StateOpRef::new(op.origin(), patch_index, op_index, op.kind());
            let target = op.target().raw();
            let granule = op.granule();
            // A whole-state operation collides with anything else on that state;
            // a granular one collides with a spanning operation or with the same
            // granule. Both lookups run, and `.or()` picks the first hit — no
            // short-circuit, so no branch.
            let against_span = spanning
                .get(&target)
                .copied()
                .filter(|earlier| earlier.origin() != op.origin());
            let against_granule = seen
                .get(&(target, granule.clone()))
                .copied()
                .filter(|earlier| earlier.origin() != op.origin());
            let against_any_granule = op
                .kind()
                .is_whole_entry()
                .then(|| {
                    seen.iter()
                        .find(|((state, _), earlier)| {
                            (*state == target) & (earlier.origin() != op.origin())
                        })
                        .map(|(_, earlier)| *earlier)
                })
                .flatten();
            let collision = against_span.or(against_granule).or(against_any_granule);
            op.kind()
                .is_whole_entry()
                .then(|| spanning.insert(target, here));
            seen.entry((target, granule.clone())).or_insert(here);
            collision.map(|earlier| {
                StateConflict::new(
                    op.target(),
                    StableHash::of_bytes(&granule),
                    earlier,
                    here,
                )
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> StateId {
        StateId::of_path("patch/target")
    }

    fn other_target() -> StateId {
        StateId::of_path("patch/other")
    }

    fn op(kind: StateOpKind, origin: &str, key: Vec<u8>, index: u32) -> StateOp {
        StateOp::new(
            kind,
            target(),
            StateOrigin::of_name(origin),
            key,
            index,
            vec![1],
        )
    }

    fn patch(ops: Vec<StateOp>) -> StatePatch {
        StatePatch::from_ops(ops)
    }

    #[test]
    fn an_empty_patch_proposes_nothing() {
        let empty = StatePatch::default();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert!(empty.ops().is_empty());
    }

    #[test]
    fn a_patch_keeps_its_operations_in_order() {
        let built = patch(vec![
            op(StateOpKind::SetCell, "a", vec![], 0),
            op(StateOpKind::SetCell, "a", vec![], 1),
        ]);
        assert_eq!(built.len(), 2);
        assert_eq!(built.ops()[1].index(), 1);
    }

    #[test]
    fn a_patch_round_trips_through_its_bytes() {
        let built = patch(vec![
            op(StateOpKind::TableInsert, "a", vec![7], 0),
            op(StateOpKind::SequenceReplace, "b", vec![], 3),
        ]);
        let bytes = built.to_bytes();
        let restored = StatePatch::from_bytes(&bytes).expect("round trip");
        assert_eq!(restored, built);
        assert_eq!(restored.to_bytes(), bytes);
        assert_eq!(restored.hash(), built.hash());
    }

    #[test]
    fn an_empty_patch_round_trips() {
        let restored = StatePatch::from_bytes(&StatePatch::default().to_bytes()).expect("round trip");
        assert!(restored.is_empty());
    }

    #[test]
    fn bytes_that_are_not_a_patch_are_rejected() {
        assert_eq!(
            StatePatch::from_bytes(&[0, 0, 0, 0]).unwrap_err().code(),
            StateErrorCode::CorruptedSnapshot
        );
    }

    #[test]
    fn truncation_at_every_prefix_is_rejected_and_never_panics() {
        let bytes = patch(vec![op(StateOpKind::TableInsert, "a", vec![7], 0)]).to_bytes();
        (0..bytes.len()).for_each(|len| {
            assert!(
                StatePatch::from_bytes(&bytes[..len]).is_err(),
                "a patch truncated to {len} bytes must not decode"
            );
        });
    }

    #[test]
    fn an_out_of_range_operation_code_is_rejected() {
        let mut bytes = patch(vec![op(StateOpKind::SetCell, "a", vec![], 0)]).to_bytes();
        bytes[8] = 200; // magic(4) + count(4) = the first op's kind byte
        assert_eq!(
            StatePatch::from_bytes(&bytes).unwrap_err().code(),
            StateErrorCode::CorruptedSnapshot
        );
    }

    #[test]
    fn the_patch_hash_is_stable_and_order_sensitive() {
        let one = patch(vec![
            op(StateOpKind::SetCell, "a", vec![], 0),
            op(StateOpKind::SetCell, "a", vec![], 1),
        ]);
        let reversed = patch(vec![
            op(StateOpKind::SetCell, "a", vec![], 1),
            op(StateOpKind::SetCell, "a", vec![], 0),
        ]);
        assert_eq!(one.hash(), one.clone().hash());
        assert_ne!(one.hash(), reversed.hash());
    }

    #[test]
    fn two_origins_writing_one_cell_conflict() {
        let conflict = detect_conflict(&[
            patch(vec![op(StateOpKind::SetCell, "a", vec![], 0)]),
            patch(vec![op(StateOpKind::SetCell, "b", vec![], 0)]),
        ])
        .expect("a conflict");
        assert_eq!(conflict.state(), target());
        assert_eq!(conflict.left().origin(), StateOrigin::of_name("a"));
        assert_eq!(conflict.right().origin(), StateOrigin::of_name("b"));
        assert_eq!(conflict.left().patch_index(), 0);
        assert_eq!(conflict.right().patch_index(), 1);
    }

    #[test]
    fn one_origin_writing_one_cell_twice_does_not_conflict() {
        assert!(detect_conflict(&[
            patch(vec![op(StateOpKind::SetCell, "a", vec![], 0)]),
            patch(vec![op(StateOpKind::SetCell, "a", vec![], 0)]),
        ])
        .is_none());
    }

    #[test]
    fn two_origins_writing_different_rows_do_not_conflict() {
        assert!(detect_conflict(&[
            patch(vec![op(StateOpKind::TableUpdate, "a", vec![1], 0)]),
            patch(vec![op(StateOpKind::TableUpdate, "b", vec![2], 0)]),
        ])
        .is_none());
    }

    #[test]
    fn two_origins_writing_the_same_row_conflict() {
        let conflict = detect_conflict(&[
            patch(vec![op(StateOpKind::TableUpdate, "a", vec![1], 0)]),
            patch(vec![op(StateOpKind::TableRemove, "b", vec![1], 0)]),
        ])
        .expect("a conflict");
        assert_eq!(conflict.granule(), StableHash::of_bytes(&[1]));
        assert_eq!(conflict.right().kind(), StateOpKind::TableRemove);
    }

    #[test]
    fn two_origins_touching_different_states_do_not_conflict() {
        let elsewhere = StateOp::new(
            StateOpKind::SetCell,
            other_target(),
            StateOrigin::of_name("b"),
            vec![],
            0,
            vec![1],
        );
        assert!(detect_conflict(&[
            patch(vec![op(StateOpKind::SetCell, "a", vec![], 0)]),
            patch(vec![elsewhere]),
        ])
        .is_none());
    }

    #[test]
    fn two_origins_replacing_different_positions_do_not_conflict() {
        assert!(detect_conflict(&[
            patch(vec![op(StateOpKind::SequenceReplace, "a", vec![], 1)]),
            patch(vec![op(StateOpKind::SequenceReplace, "b", vec![], 2)]),
        ])
        .is_none());
    }

    #[test]
    fn a_position_shifting_operation_conflicts_with_any_other_write_to_that_sequence() {
        // An insert moves every later item, so it cannot be reconciled with a
        // replace addressed by position — even a different position.
        let insert_then_replace = detect_conflict(&[
            patch(vec![op(StateOpKind::SequenceInsert, "a", vec![], 0)]),
            patch(vec![op(StateOpKind::SequenceReplace, "b", vec![], 5)]),
        ]);
        assert!(insert_then_replace.is_some());

        // And in the other order, where the spanning operation arrives second.
        let replace_then_remove = detect_conflict(&[
            patch(vec![op(StateOpKind::SequenceReplace, "a", vec![], 5)]),
            patch(vec![op(StateOpKind::SequenceRemove, "b", vec![], 0)]),
        ]);
        assert!(replace_then_remove.is_some());
    }

    #[test]
    fn two_origins_appending_to_one_sequence_conflict() {
        assert!(detect_conflict(&[
            patch(vec![op(StateOpKind::SequenceAppend, "a", vec![], 0)]),
            patch(vec![op(StateOpKind::SequenceAppend, "b", vec![], 0)]),
        ])
        .is_some());
    }

    #[test]
    fn conflicts_within_a_single_patch_from_two_origins_are_found() {
        let mixed = patch(vec![
            op(StateOpKind::SetCell, "a", vec![], 0),
            op(StateOpKind::SetCell, "b", vec![], 0),
        ]);
        let conflict = detect_conflict(&[mixed]).expect("a conflict");
        assert_eq!(conflict.left().op_index(), 0);
        assert_eq!(conflict.right().op_index(), 1);
    }

    #[test]
    fn no_patches_and_empty_patches_conflict_with_nothing() {
        assert!(detect_conflict(&[]).is_none());
        assert!(detect_conflict(&[StatePatch::default()]).is_none());
    }

    #[test]
    fn conflict_detection_is_deterministic() {
        let patches = [
            patch(vec![op(StateOpKind::SetCell, "a", vec![], 0)]),
            patch(vec![op(StateOpKind::SetCell, "b", vec![], 0)]),
            patch(vec![op(StateOpKind::SetCell, "c", vec![], 0)]),
        ];
        assert_eq!(detect_conflict(&patches), detect_conflict(&patches));
    }
}
