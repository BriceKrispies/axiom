//! What changed between two snapshots.

use std::collections::BTreeMap;

use axiom_kernel::StableHash;

use crate::state_entry::StateEntry;
use crate::state_granule::StateGranule;
use crate::state_id::StateId;
use crate::state_kind::StateKind;
use crate::state_payload::{parse_items, parse_rows};
use crate::state_snapshot::StateSnapshot;
use crate::StateResult;

/// How a value changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum StateChangeKind {
    /// It did not exist before and does now.
    Added = 0,
    /// It existed before and does not now.
    Removed = 1,
    /// It existed both times, with different contents.
    Replaced = 2,
}

const CHANGE_NAMES: [&str; 3] = ["added", "removed", "replaced"];

impl StateChangeKind {
    /// The stable kebab-case name.
    pub const fn name(self) -> &'static str {
        CHANGE_NAMES[self as usize]
    }
}

/// Classify by presence: index = `before as usize * 2 + after as usize`.
///
/// A table, not a `match`: the "absent from both" slot cannot arise (a granule
/// enters the union only by being present somewhere), and as a table element it
/// is simply never selected — where a `match` arm would have been an
/// unreachable region no test could cover.
const CLASSIFY: [Option<StateChangeKind>; 4] = [
    None,
    Some(StateChangeKind::Added),
    Some(StateChangeKind::Removed),
    Some(StateChangeKind::Replaced),
];

/// One side of a change: whether the value was there, and what it was.
///
/// Carries bytes and a digest, never a `Debug` rendering — a game value is under
/// no obligation to implement `Debug`, and making identity depend on a text
/// rendering would make the diff a formatting artifact. A caller who wants the
/// typed value calls [`Self::decode`] and opts into the type itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateValueRef {
    present: bool,
    bytes: Vec<u8>,
}

impl StateValueRef {
    /// A value that was there.
    pub const fn present(bytes: Vec<u8>) -> Self {
        StateValueRef {
            present: true,
            bytes,
        }
    }

    /// A value that was not there.
    pub const fn absent() -> Self {
        StateValueRef {
            present: false,
            bytes: Vec::new(),
        }
    }

    /// Whether the value existed on this side of the change.
    pub const fn is_present(&self) -> bool {
        self.present
    }

    /// The canonical bytes; empty when absent.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The value's digest.
    pub fn hash(&self) -> StableHash {
        StableHash::of_bytes(&self.bytes)
    }

    /// Decode as `T`, for a caller that knows the type.
    pub fn decode<T: axiom_kernel::Reflect>(&self) -> StateResult<T> {
        crate::state_payload::decode_cell::<T>(&self.bytes)
    }
}

/// One change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateChange {
    state: StateId,
    kind: StateChangeKind,
    granule: StateGranule,
    before: StateValueRef,
    after: StateValueRef,
}

impl StateChange {
    /// Which state changed.
    pub const fn state(&self) -> StateId {
        self.state
    }

    /// How it changed.
    pub const fn kind(&self) -> StateChangeKind {
        self.kind
    }

    /// Which part of it changed.
    pub const fn granule(&self) -> &StateGranule {
        &self.granule
    }

    /// What it was.
    pub const fn before(&self) -> &StateValueRef {
        &self.before
    }

    /// What it is.
    pub const fn after(&self) -> &StateValueRef {
        &self.after
    }
}

/// An ordered account of everything that changed between two snapshots.
///
/// Ordered by state identity, then by granule, so the same pair of snapshots
/// always produces byte-identical output — which is what makes a diff usable in
/// a test, a golden artifact, or an agent's inspection of a tick.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StateDiff {
    changes: Vec<StateChange>,
}

impl StateDiff {
    /// Every change, in deterministic order.
    pub fn changes(&self) -> &[StateChange] {
        &self.changes
    }

    /// How many changes there were.
    pub fn len(&self) -> usize {
        self.changes.len()
    }

    /// Whether the two snapshots were equivalent.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// The diff's digest.
    pub fn hash(&self) -> StableHash {
        let words: Vec<u64> = self
            .changes
            .iter()
            .flat_map(|change| {
                [
                    change.state.raw(),
                    change.kind as u64,
                    change.granule.hash().raw(),
                    change.before.hash().raw(),
                    change.after.hash().raw(),
                ]
            })
            .collect();
        StableHash::of_words(&words)
    }
}

/// Compare two snapshots.
///
/// Sequences are compared **by position**: an insertion at the head therefore
/// reports every later position as replaced. That is a deliberately small model —
/// an edit-script would be a loop-heavy alignment algorithm serving no
/// requirement here — and it is documented rather than hidden.
pub fn diff(before: &StateSnapshot, after: &StateSnapshot) -> StateResult<StateDiff> {
    let mut union: BTreeMap<StateId, (Option<&StateEntry>, Option<&StateEntry>)> = BTreeMap::new();
    before.entries().iter().for_each(|(id, entry)| {
        union.entry(*id).or_default().0 = Some(entry);
    });
    after.entries().iter().for_each(|(id, entry)| {
        union.entry(*id).or_default().1 = Some(entry);
    });

    union
        .into_iter()
        .try_fold(Vec::new(), |changes, (id, sides)| {
            compare(id, sides, changes)
        })
        .map(|changes| StateDiff { changes })
}

/// The per-shape comparators, indexed by `StateKind as usize`.
type Compare = fn(StateId, &StateEntry, &StateEntry, Vec<StateChange>) -> StateResult<Vec<StateChange>>;

const COMPARE: [Compare; 3] = [compare_cell, compare_table, compare_sequence];

/// Compare one state's two sides.
fn compare(
    id: StateId,
    sides: (Option<&StateEntry>, Option<&StateEntry>),
    mut changes: Vec<StateChange>,
) -> StateResult<Vec<StateChange>> {
    let (before, after) = sides;
    // Present on both sides: compare contents by shape. Present on one side
    // only: the whole state appeared or disappeared.
    let both = before.zip(after);
    let one_sided = CLASSIFY
        [usize::from(before.is_some()) * 2 + usize::from(after.is_some())]
    .filter(|_| both.is_none());
    one_sided.map(|kind| {
        let entry = before.or(after);
        changes.push(StateChange {
            state: id,
            kind,
            granule: StateGranule::whole(entry.map_or(StateKind::Cell, StateEntry::kind)),
            before: side(before),
            after: side(after),
        });
    });
    both.map_or(Ok(changes.clone()), |(old, new)| {
        COMPARE[old.kind() as usize](id, old, new, changes)
    })
}

/// A whole entry's payload as one side of a change.
fn side(entry: Option<&StateEntry>) -> StateValueRef {
    entry.map_or_else(StateValueRef::absent, |entry| {
        StateValueRef::present(entry.payload().to_vec())
    })
}

fn compare_cell(
    id: StateId,
    old: &StateEntry,
    new: &StateEntry,
    mut changes: Vec<StateChange>,
) -> StateResult<Vec<StateChange>> {
    (old.payload() != new.payload()).then(|| {
        changes.push(StateChange {
            state: id,
            kind: StateChangeKind::Replaced,
            granule: StateGranule::whole(StateKind::Cell),
            before: StateValueRef::present(old.payload().to_vec()),
            after: StateValueRef::present(new.payload().to_vec()),
        });
    });
    Ok(changes)
}

fn compare_table(
    id: StateId,
    old: &StateEntry,
    new: &StateEntry,
    changes: Vec<StateChange>,
) -> StateResult<Vec<StateChange>> {
    parse_rows(old.payload()).and_then(|old_rows| {
        parse_rows(new.payload()).map(|new_rows| {
            let mut union: BTreeMap<Vec<u8>, (Option<Vec<u8>>, Option<Vec<u8>>)> = BTreeMap::new();
            old_rows.into_iter().for_each(|(key, value)| {
                union.entry(key).or_default().0 = Some(value);
            });
            new_rows.into_iter().for_each(|(key, value)| {
                union.entry(key).or_default().1 = Some(value);
            });
            union
                .into_iter()
                .filter(|(_, (old, new))| old != new)
                .fold(changes, |mut changes, (key, (old, new))| {
                    CLASSIFY[usize::from(old.is_some()) * 2 + usize::from(new.is_some())].map(
                        |kind| {
                            changes.push(StateChange {
                                state: id,
                                kind,
                                granule: StateGranule::row(key),
                                before: bytes_side(old),
                                after: bytes_side(new),
                            });
                        },
                    );
                    changes
                })
        })
    })
}

fn compare_sequence(
    id: StateId,
    old: &StateEntry,
    new: &StateEntry,
    changes: Vec<StateChange>,
) -> StateResult<Vec<StateChange>> {
    parse_items(old.payload()).and_then(|old_items| {
        parse_items(new.payload()).map(|new_items| {
            (0..old_items.len().max(new_items.len())).fold(changes, |mut changes, at| {
                let old = old_items.get(at).cloned();
                let new = new_items.get(at).cloned();
                (old != new)
                    .then(|| {
                        CLASSIFY[usize::from(old.is_some()) * 2 + usize::from(new.is_some())].map(
                            |kind| {
                                changes.push(StateChange {
                                    state: id,
                                    kind,
                                    granule: StateGranule::position(at as u32),
                                    before: bytes_side(old),
                                    after: bytes_side(new),
                                });
                            },
                        )
                    })
                    .flatten();
                changes
            })
        })
    })
}

fn bytes_side(bytes: Option<Vec<u8>>) -> StateValueRef {
    bytes.map_or_else(StateValueRef::absent, StateValueRef::present)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_apply::apply;
    use crate::state_decl::StateDecl;
    use crate::state_key::{CellKey, SequenceKey, StateKey, TableKey};
    use crate::state_origin::StateOrigin;
    use crate::state_patch_builder::StatePatchBuilder;
    use crate::state_schema::StateSchema;
    use crate::state_sequence::StateSequence;
    use crate::state_snapshot_builder::StateSnapshotBuilder;
    use crate::state_table::StateTable;
    use axiom_kernel::SchemaVersion;

    struct Tick;
    impl StateKey for Tick {
        const PATH: &'static str = "diff/tick";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Tick {
        type Value = u64;
    }

    struct Rows;
    impl StateKey for Rows {
        const PATH: &'static str = "diff/rows";
        const KIND: StateKind = StateKind::Table;
    }
    impl TableKey for Rows {
        type Key = u32;
        type Value = u64;
    }

    struct Log;
    impl StateKey for Log {
        const PATH: &'static str = "diff/log";
        const KIND: StateKind = StateKind::Sequence;
    }
    impl SequenceKey for Log {
        type Item = u32;
    }

    fn schema() -> StateSchema {
        StateSchema::build(
            "diff",
            SchemaVersion::new(1, 0),
            &[
                StateDecl::cell::<Tick>(),
                StateDecl::table::<Rows>(),
                StateDecl::sequence::<Log>(),
            ],
        )
        .expect("valid")
    }

    fn base() -> StateSnapshot {
        StateSnapshotBuilder::new(&schema())
            .with_cell::<Tick>(&1)
            .with_table::<Rows>(&StateTable::new().with(1, 10).with(2, 20))
            .with_sequence::<Log>(&StateSequence::new().appended(7).appended(8))
            .build()
            .expect("complete")
    }

    fn after(build: impl FnOnce(StatePatchBuilder) -> StatePatchBuilder) -> StateSnapshot {
        let patch = build(StatePatchBuilder::new(StateOrigin::of_name("t")))
            .build()
            .expect("valid");
        apply(&schema(), &base(), &patch).expect("applies")
    }

    #[test]
    fn a_snapshot_does_not_differ_from_itself() {
        let diff = diff(&base(), &base()).expect("diffs");
        assert!(diff.is_empty());
        assert_eq!(diff.len(), 0);
        assert!(diff.changes().is_empty());
    }

    #[test]
    fn a_changed_cell_is_reported_as_replaced() {
        let next = after(|p| p.set_cell::<Tick>(&2));
        let diff = diff(&base(), &next).expect("diffs");
        assert_eq!(diff.len(), 1);
        let change = &diff.changes()[0];
        assert_eq!(change.state(), Tick::id());
        assert_eq!(change.kind(), StateChangeKind::Replaced);
        assert_eq!(change.granule().kind(), StateKind::Cell);
        assert_eq!(change.before().decode::<u64>(), Ok(1));
        assert_eq!(change.after().decode::<u64>(), Ok(2));
        assert!(change.before().is_present());
        assert!(change.after().is_present());
        assert_ne!(change.before().hash(), change.after().hash());
    }

    #[test]
    fn an_added_row_is_reported_with_no_before() {
        let next = after(|p| p.table_insert::<Rows>(&3, &30));
        let diff = diff(&base(), &next).expect("diffs");
        assert_eq!(diff.len(), 1);
        let change = &diff.changes()[0];
        assert_eq!(change.kind(), StateChangeKind::Added);
        assert_eq!(change.granule().kind(), StateKind::Table);
        assert!(!change.before().is_present());
        assert!(change.before().bytes().is_empty());
        assert_eq!(change.after().decode::<u64>(), Ok(30));
    }

    #[test]
    fn a_removed_row_is_reported_with_no_after() {
        let next = after(|p| p.table_remove::<Rows>(&1));
        let diff = diff(&base(), &next).expect("diffs");
        assert_eq!(diff.len(), 1);
        let change = &diff.changes()[0];
        assert_eq!(change.kind(), StateChangeKind::Removed);
        assert!(change.before().is_present());
        assert!(!change.after().is_present());
    }

    #[test]
    fn a_replaced_row_names_its_key() {
        let next = after(|p| p.table_update::<Rows>(&2, &99));
        let diff = diff(&base(), &next).expect("diffs");
        let change = &diff.changes()[0];
        assert_eq!(change.kind(), StateChangeKind::Replaced);
        assert_eq!(
            change.granule().key(),
            crate::state_payload::encode_cell(&2_u32)
        );
    }

    #[test]
    fn an_untouched_row_is_not_reported() {
        let next = after(|p| p.table_update::<Rows>(&1, &99));
        let diff = diff(&base(), &next).expect("diffs");
        assert_eq!(diff.len(), 1, "only the row that moved is reported");
    }

    #[test]
    fn a_replaced_sequence_item_names_its_position() {
        let next = after(|p| p.sequence_replace::<Log>(1, &99));
        let diff = diff(&base(), &next).expect("diffs");
        assert_eq!(diff.len(), 1);
        let change = &diff.changes()[0];
        assert_eq!(change.kind(), StateChangeKind::Replaced);
        assert_eq!(change.granule().kind(), StateKind::Sequence);
        assert_eq!(change.granule().index(), 1);
    }

    #[test]
    fn an_appended_item_is_added_at_its_position() {
        let next = after(|p| p.sequence_append::<Log>(&9));
        let diff = diff(&base(), &next).expect("diffs");
        assert_eq!(diff.len(), 1);
        assert_eq!(diff.changes()[0].kind(), StateChangeKind::Added);
        assert_eq!(diff.changes()[0].granule().index(), 2);
    }

    #[test]
    fn a_removed_item_shortens_the_sequence() {
        let next = after(|p| p.sequence_remove::<Log>(0));
        let diff = diff(&base(), &next).expect("diffs");
        // Position 0 becomes 8 (replaced) and position 1 disappears (removed).
        assert_eq!(diff.len(), 2);
        assert_eq!(diff.changes()[0].kind(), StateChangeKind::Replaced);
        assert_eq!(diff.changes()[1].kind(), StateChangeKind::Removed);
    }

    #[test]
    fn an_insertion_at_the_head_reports_every_later_position() {
        // The documented consequence of comparing by position rather than by
        // computing an edit script.
        let next = after(|p| p.sequence_insert::<Log>(0, &1));
        let diff = diff(&base(), &next).expect("diffs");
        assert_eq!(diff.len(), 3);
    }

    #[test]
    fn a_state_present_in_only_one_snapshot_is_added_or_removed_whole() {
        let wide = schema();
        let narrow = StateSchema::build(
            "diff",
            SchemaVersion::new(1, 0),
            &[StateDecl::cell::<Tick>()],
        )
        .expect("valid");
        let small = StateSnapshotBuilder::new(&narrow)
            .with_cell::<Tick>(&1)
            .build()
            .expect("complete");
        let big = StateSnapshotBuilder::new(&wide)
            .with_cell::<Tick>(&1)
            .with_table::<Rows>(&StateTable::new())
            .with_sequence::<Log>(&StateSequence::new())
            .build()
            .expect("complete");

        let grew = diff(&small, &big).expect("diffs");
        assert_eq!(grew.len(), 2);
        assert!(grew
            .changes()
            .iter()
            .all(|c| c.kind() == StateChangeKind::Added));

        let shrank = diff(&big, &small).expect("diffs");
        assert_eq!(shrank.len(), 2);
        assert!(shrank
            .changes()
            .iter()
            .all(|c| c.kind() == StateChangeKind::Removed));
    }

    #[test]
    fn changes_are_ordered_by_state_then_granule_and_repeat_identically() {
        let next = after(|p| {
            p.set_cell::<Tick>(&2)
                .table_insert::<Rows>(&3, &30)
                .sequence_append::<Log>(&9)
        });
        let once = diff(&base(), &next).expect("diffs");
        let twice = diff(&base(), &next).expect("diffs");
        assert_eq!(once, twice);
        assert_eq!(once.hash(), twice.hash());
        let states: Vec<StateId> = once.changes().iter().map(StateChange::state).collect();
        let mut sorted = states.clone();
        sorted.sort_unstable();
        assert_eq!(states, sorted);
    }

    #[test]
    fn the_diff_digest_moves_when_the_change_set_moves() {
        let one = diff(&base(), &after(|p| p.set_cell::<Tick>(&2))).expect("diffs");
        let other = diff(&base(), &after(|p| p.set_cell::<Tick>(&3))).expect("diffs");
        assert_ne!(one.hash(), other.hash());
        assert_eq!(StateDiff::default().hash(), StateDiff::default().hash());
    }

    #[test]
    fn change_kinds_have_stable_names() {
        assert_eq!(StateChangeKind::Added.name(), "added");
        assert_eq!(StateChangeKind::Removed.name(), "removed");
        assert_eq!(StateChangeKind::Replaced.name(), "replaced");
    }

    #[test]
    fn a_corrupt_payload_is_reported_rather_than_panicking() {
        let mut entries = base().entries().clone();
        let broken = entries.get(&Rows::id()).expect("rows").with_payload(vec![9]);
        entries.insert(Rows::id(), broken);
        let damaged = StateSnapshot::from_parts(base().schema_id(), base().version(), entries);
        assert!(diff(&base(), &damaged).is_err());

        let mut entries = base().entries().clone();
        let broken = entries.get(&Log::id()).expect("log").with_payload(vec![9]);
        entries.insert(Log::id(), broken);
        let damaged = StateSnapshot::from_parts(base().schema_id(), base().version(), entries);
        assert!(diff(&base(), &damaged).is_err());
    }
}
