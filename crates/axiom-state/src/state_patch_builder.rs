//! Authoring a patch, with the target's type checked at the call site.

use crate::state_access::StateAccess;
use crate::state_error::StateError;
use crate::state_error_code::StateErrorCode;
use crate::state_key::{CellKey, SequenceKey, TableKey};
use crate::state_op::StateOp;
use crate::state_op_kind::StateOpKind;
use crate::state_origin::StateOrigin;
use crate::state_patch::StatePatch;
use crate::state_payload::encode_cell;
use crate::StateResult;

/// Builds a [`StatePatch`] for one author.
///
/// Each method is generic over the *key* of the state it changes, so the value
/// type is fixed by the target: `set_cell::<Score>(&ball_state)` does not
/// compile, and neither does a table operation aimed at a cell. That is the
/// difference between a typed patch API and a stringly-typed one — mistakes are
/// rejected where they are written rather than where they are applied.
///
/// A builder carries one [`StateOrigin`], which is what makes conflict detection
/// meaningful: everything one author writes is self-consistent by construction,
/// so a conflict can only arise between independently authored patches.
///
/// When opened from a [`crate::StateView`] the builder also carries that view's
/// declared write set, and an operation outside it is refused.
#[derive(Debug, Clone)]
pub struct StatePatchBuilder {
    origin: StateOrigin,
    allowed: Option<StateAccess>,
    ops: Vec<StateOp>,
    fault: Option<StateError>,
}

impl StatePatchBuilder {
    /// Author a patch as `origin`, with no declared write set.
    pub fn new(origin: StateOrigin) -> Self {
        StatePatchBuilder {
            origin,
            allowed: None,
            ops: Vec::new(),
            fault: None,
        }
    }

    /// Author a patch as `origin`, restricted to `access`'s declared writes.
    pub(crate) fn restricted(origin: StateOrigin, access: StateAccess) -> Self {
        StatePatchBuilder {
            origin,
            allowed: Some(access),
            ops: Vec::new(),
            fault: None,
        }
    }

    /// Who is authoring this patch.
    pub const fn origin(&self) -> StateOrigin {
        self.origin
    }

    /// Record an operation, or the first fault that prevents it.
    fn push(mut self, op: StateOp) -> Self {
        let denied = self
            .allowed
            .as_ref()
            .map(|access| access.declares_write(op.target()))
            .map_or(false, |declared| !declared);
        let fault = denied.then(|| {
            StateError::at(
                StateErrorCode::UndeclaredAccess,
                op.target(),
                "this computation did not declare a write to that state",
            )
        });
        (!denied).then(|| self.ops.push(op));
        self.fault = self.fault.or(fault);
        self
    }

    /// Replace a cell's value.
    pub fn set_cell<K: CellKey>(self, value: &K::Value) -> Self {
        let op = StateOp::new(
            StateOpKind::SetCell,
            K::id(),
            self.origin,
            Vec::new(),
            0,
            encode_cell(value),
        );
        self.push(op)
    }

    /// Add a row that must not already exist.
    pub fn table_insert<K: TableKey>(self, key: &K::Key, value: &K::Value) -> Self {
        let op = StateOp::new(
            StateOpKind::TableInsert,
            K::id(),
            self.origin,
            encode_cell(key),
            0,
            encode_cell(value),
        );
        self.push(op)
    }

    /// Replace a row that must already exist.
    pub fn table_update<K: TableKey>(self, key: &K::Key, value: &K::Value) -> Self {
        let op = StateOp::new(
            StateOpKind::TableUpdate,
            K::id(),
            self.origin,
            encode_cell(key),
            0,
            encode_cell(value),
        );
        self.push(op)
    }

    /// Remove a row that must already exist.
    pub fn table_remove<K: TableKey>(self, key: &K::Key) -> Self {
        let op = StateOp::new(
            StateOpKind::TableRemove,
            K::id(),
            self.origin,
            encode_cell(key),
            0,
            Vec::new(),
        );
        self.push(op)
    }

    /// Insert an item at a position, shifting later items right.
    pub fn sequence_insert<K: SequenceKey>(self, index: u32, item: &K::Item) -> Self {
        let op = StateOp::new(
            StateOpKind::SequenceInsert,
            K::id(),
            self.origin,
            Vec::new(),
            index,
            encode_cell(item),
        );
        self.push(op)
    }

    /// Replace the item at a position.
    pub fn sequence_replace<K: SequenceKey>(self, index: u32, item: &K::Item) -> Self {
        let op = StateOp::new(
            StateOpKind::SequenceReplace,
            K::id(),
            self.origin,
            Vec::new(),
            index,
            encode_cell(item),
        );
        self.push(op)
    }

    /// Remove the item at a position, shifting later items left.
    pub fn sequence_remove<K: SequenceKey>(self, index: u32) -> Self {
        let op = StateOp::new(
            StateOpKind::SequenceRemove,
            K::id(),
            self.origin,
            Vec::new(),
            index,
            Vec::new(),
        );
        self.push(op)
    }

    /// Add an item at the end.
    pub fn sequence_append<K: SequenceKey>(self, item: &K::Item) -> Self {
        let op = StateOp::new(
            StateOpKind::SequenceAppend,
            K::id(),
            self.origin,
            Vec::new(),
            0,
            encode_cell(item),
        );
        self.push(op)
    }

    /// Finish, or report the first thing that went wrong.
    pub fn build(self) -> StateResult<StatePatch> {
        self.fault
            .map_or_else(|| Ok(StatePatch::from_ops(self.ops.clone())), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_key::StateKey;
    use crate::state_kind::StateKind;

    struct Tick;
    impl StateKey for Tick {
        const PATH: &'static str = "author/tick";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Tick {
        type Value = u64;
    }

    struct Rows;
    impl StateKey for Rows {
        const PATH: &'static str = "author/rows";
        const KIND: StateKind = StateKind::Table;
    }
    impl TableKey for Rows {
        type Key = u32;
        type Value = u64;
    }

    struct Log;
    impl StateKey for Log {
        const PATH: &'static str = "author/log";
        const KIND: StateKind = StateKind::Sequence;
    }
    impl SequenceKey for Log {
        type Item = u32;
    }

    fn author() -> StatePatchBuilder {
        StatePatchBuilder::new(StateOrigin::of_name("author"))
    }

    #[test]
    fn an_unopened_builder_produces_an_empty_patch() {
        let patch = author().build().expect("valid");
        assert!(patch.is_empty());
    }

    #[test]
    fn the_builder_reports_its_origin_and_stamps_every_operation_with_it() {
        let builder = author();
        assert_eq!(builder.origin(), StateOrigin::of_name("author"));
        let patch = builder.set_cell::<Tick>(&1).build().expect("valid");
        assert_eq!(patch.ops()[0].origin(), StateOrigin::of_name("author"));
    }

    #[test]
    fn every_operation_kind_can_be_authored_and_targets_its_state() {
        let patch = author()
            .set_cell::<Tick>(&1)
            .table_insert::<Rows>(&1, &10)
            .table_update::<Rows>(&1, &11)
            .table_remove::<Rows>(&1)
            .sequence_insert::<Log>(0, &1)
            .sequence_replace::<Log>(0, &2)
            .sequence_remove::<Log>(0)
            .sequence_append::<Log>(&3)
            .build()
            .expect("valid");
        let kinds: Vec<StateOpKind> = patch.ops().iter().map(StateOp::kind).collect();
        assert_eq!(
            kinds,
            vec![
                StateOpKind::SetCell,
                StateOpKind::TableInsert,
                StateOpKind::TableUpdate,
                StateOpKind::TableRemove,
                StateOpKind::SequenceInsert,
                StateOpKind::SequenceReplace,
                StateOpKind::SequenceRemove,
                StateOpKind::SequenceAppend,
            ]
        );
        assert_eq!(patch.ops()[0].target(), Tick::id());
        assert_eq!(patch.ops()[1].target(), Rows::id());
        assert_eq!(patch.ops()[4].target(), Log::id());
    }

    #[test]
    fn table_operations_carry_their_encoded_key_and_value() {
        let patch = author().table_insert::<Rows>(&7, &70).build().expect("valid");
        assert_eq!(patch.ops()[0].key(), encode_cell(&7_u32));
        assert_eq!(patch.ops()[0].value(), encode_cell(&70_u64));
    }

    #[test]
    fn sequence_operations_carry_their_position() {
        let patch = author().sequence_replace::<Log>(3, &1).build().expect("valid");
        assert_eq!(patch.ops()[0].index(), 3);
    }

    #[test]
    fn a_removal_carries_no_value() {
        let patch = author().table_remove::<Rows>(&1).build().expect("valid");
        assert!(patch.ops()[0].value().is_empty());
    }

    #[test]
    fn operations_keep_the_order_they_were_authored_in() {
        let patch = author()
            .set_cell::<Tick>(&1)
            .set_cell::<Tick>(&2)
            .build()
            .expect("valid");
        assert_eq!(patch.len(), 2);
        assert_eq!(patch.ops()[0].value(), encode_cell(&1_u64));
        assert_eq!(patch.ops()[1].value(), encode_cell(&2_u64));
    }

    #[test]
    fn a_restricted_builder_accepts_a_declared_write() {
        let access = StateAccess::none().write::<Tick>();
        let patch = StatePatchBuilder::restricted(StateOrigin::ANONYMOUS, access)
            .set_cell::<Tick>(&1)
            .build()
            .expect("the write was declared");
        assert_eq!(patch.len(), 1);
    }

    #[test]
    fn a_restricted_builder_refuses_an_undeclared_write() {
        let access = StateAccess::none().write::<Tick>();
        let error = StatePatchBuilder::restricted(StateOrigin::ANONYMOUS, access)
            .table_insert::<Rows>(&1, &10)
            .build()
            .unwrap_err();
        assert_eq!(error.code(), StateErrorCode::UndeclaredAccess);
        assert_eq!(error.state(), Rows::id());
    }

    #[test]
    fn a_refused_write_is_not_recorded_and_the_first_fault_wins() {
        let access = StateAccess::none().write::<Tick>();
        let error = StatePatchBuilder::restricted(StateOrigin::ANONYMOUS, access)
            .table_insert::<Rows>(&1, &10)
            .sequence_append::<Log>(&1)
            .build()
            .unwrap_err();
        assert_eq!(error.state(), Rows::id(), "the first refusal is reported");
    }
}
