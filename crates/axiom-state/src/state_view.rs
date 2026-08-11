//! A snapshot seen through one computation's declared access.

use crate::state_access::StateAccess;
use crate::state_error::StateError;
use crate::state_error_code::StateErrorCode;
use crate::state_id::StateId;
use crate::state_key::{CellKey, SequenceKey, TableKey};
use crate::state_origin::StateOrigin;
use crate::state_patch_builder::StatePatchBuilder;
use crate::state_sequence::StateSequence;
use crate::state_snapshot::StateSnapshot;
use crate::state_table::StateTable;
use crate::StateResult;

/// A snapshot restricted to what a computation declared it needs.
///
/// Reading anything undeclared fails with `UndeclaredAccess`, and a patch opened
/// from the view refuses undeclared writes. A computation declaring the ball, the
/// batter and the rules therefore *cannot* casually consult the scoreboard —
/// reaching around the view means asking for the `&StateSnapshot` directly,
/// which is a visible act somebody can review, not an accident.
///
/// The view is also where the cost of byte-backed storage is paid once instead
/// of per read: a computation opens a view, materializes its declared reads into
/// ordinary Rust values, works on those at full speed, and emits one patch.
#[derive(Debug, Clone, Copy)]
pub struct StateView<'a> {
    access: &'a StateAccess,
    snapshot: &'a StateSnapshot,
}

impl<'a> StateView<'a> {
    /// Look at `snapshot` through `access`.
    pub const fn open(access: &'a StateAccess, snapshot: &'a StateSnapshot) -> Self {
        StateView { access, snapshot }
    }

    /// The declaration this view enforces.
    pub const fn access(&self) -> &'a StateAccess {
        self.access
    }

    /// Refuse an identity the computation did not declare as a read.
    fn permit_read(&self, id: StateId) -> StateResult<()> {
        self.access
            .declares_read(id)
            .then_some(())
            .ok_or(StateError::at(
                StateErrorCode::UndeclaredAccess,
                id,
                "this computation did not declare a read of that state",
            ))
    }

    /// Read a declared cell.
    pub fn cell<K: CellKey>(&self) -> StateResult<K::Value> {
        self.permit_read(K::id())
            .and_then(|()| self.snapshot.cell::<K>())
    }

    /// Read a declared table.
    pub fn table<K: TableKey>(&self) -> StateResult<StateTable<K::Key, K::Value>> {
        self.permit_read(K::id())
            .and_then(|()| self.snapshot.table::<K>())
    }

    /// Read a declared sequence.
    pub fn sequence<K: SequenceKey>(&self) -> StateResult<StateSequence<K::Item>> {
        self.permit_read(K::id())
            .and_then(|()| self.snapshot.sequence::<K>())
    }

    /// Start a patch confined to this computation's declared writes.
    pub fn patch(&self, origin: StateOrigin) -> StatePatchBuilder {
        StatePatchBuilder::restricted(origin, self.access.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_decl::StateDecl;
    use crate::state_key::StateKey;
    use crate::state_kind::StateKind;
    use crate::state_schema::StateSchema;
    use crate::state_snapshot_builder::StateSnapshotBuilder;
    use axiom_kernel::SchemaVersion;

    struct Ball;
    impl StateKey for Ball {
        const PATH: &'static str = "view/ball";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Ball {
        type Value = u64;
    }

    struct Runners;
    impl StateKey for Runners {
        const PATH: &'static str = "view/runners";
        const KIND: StateKind = StateKind::Table;
    }
    impl TableKey for Runners {
        type Key = u32;
        type Value = u64;
    }

    struct Events;
    impl StateKey for Events {
        const PATH: &'static str = "view/events";
        const KIND: StateKind = StateKind::Sequence;
    }
    impl SequenceKey for Events {
        type Item = u32;
    }

    struct Scoreboard;
    impl StateKey for Scoreboard {
        const PATH: &'static str = "view/scoreboard";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Scoreboard {
        type Value = u64;
    }

    fn schema() -> StateSchema {
        StateSchema::build(
            "view",
            SchemaVersion::new(1, 0),
            &[
                StateDecl::cell::<Ball>(),
                StateDecl::table::<Runners>(),
                StateDecl::sequence::<Events>(),
                StateDecl::cell::<Scoreboard>(),
            ],
        )
        .expect("valid")
    }

    fn snapshot() -> StateSnapshot {
        StateSnapshotBuilder::new(&schema())
            .with_cell::<Ball>(&3)
            .with_table::<Runners>(&StateTable::new().with(1, 10))
            .with_sequence::<Events>(&StateSequence::new().appended(5))
            .with_cell::<Scoreboard>(&99)
            .build()
            .expect("complete")
    }

    fn declared() -> StateAccess {
        StateAccess::none()
            .read::<Ball>()
            .read::<Runners>()
            .read::<Events>()
            .write::<Ball>()
    }

    #[test]
    fn a_declared_read_of_every_shape_succeeds() {
        let access = declared();
        let stored = snapshot();
        let view = StateView::open(&access, &stored);
        assert_eq!(view.cell::<Ball>(), Ok(3));
        assert_eq!(view.table::<Runners>().expect("table").get(&1), Some(&10));
        assert_eq!(view.sequence::<Events>().expect("seq").items(), &[5]);
    }

    #[test]
    fn an_undeclared_read_is_refused_even_though_the_snapshot_holds_it() {
        let access = declared();
        let stored = snapshot();
        let view = StateView::open(&access, &stored);
        let error = view.cell::<Scoreboard>().unwrap_err();
        assert_eq!(error.code(), StateErrorCode::UndeclaredAccess);
        assert_eq!(error.state(), Scoreboard::id());
        assert_eq!(
            stored.cell::<Scoreboard>(),
            Ok(99),
            "the snapshot itself still holds it — the view is what refuses"
        );
    }

    #[test]
    fn an_undeclared_table_or_sequence_read_is_refused() {
        let access = StateAccess::none().read::<Ball>();
        let stored = snapshot();
        let view = StateView::open(&access, &stored);
        assert_eq!(
            view.table::<Runners>().unwrap_err().code(),
            StateErrorCode::UndeclaredAccess
        );
        assert_eq!(
            view.sequence::<Events>().unwrap_err().code(),
            StateErrorCode::UndeclaredAccess
        );
    }

    #[test]
    fn a_view_reports_the_declaration_it_enforces() {
        let access = declared();
        let stored = snapshot();
        let view = StateView::open(&access, &stored);
        assert_eq!(view.access(), &access);
        assert!(view.access().declares_write(Ball::id()));
    }

    #[test]
    fn a_patch_opened_from_a_view_accepts_a_declared_write() {
        let access = declared();
        let stored = snapshot();
        let view = StateView::open(&access, &stored);
        let patch = view
            .patch(StateOrigin::of_name("sim"))
            .set_cell::<Ball>(&4)
            .build()
            .expect("the write was declared");
        assert_eq!(patch.len(), 1);
        assert_eq!(patch.ops()[0].origin(), StateOrigin::of_name("sim"));
    }

    #[test]
    fn a_patch_opened_from_a_view_refuses_an_undeclared_write() {
        let access = declared();
        let stored = snapshot();
        let view = StateView::open(&access, &stored);
        let error = view
            .patch(StateOrigin::of_name("sim"))
            .set_cell::<Scoreboard>(&0)
            .build()
            .unwrap_err();
        assert_eq!(error.code(), StateErrorCode::UndeclaredAccess);
        assert_eq!(error.state(), Scoreboard::id());
    }

    #[test]
    fn declaring_a_read_does_not_grant_a_write() {
        let access = StateAccess::none().read::<Scoreboard>();
        let stored = snapshot();
        let view = StateView::open(&access, &stored);
        assert_eq!(view.cell::<Scoreboard>(), Ok(99));
        assert_eq!(
            view.patch(StateOrigin::ANONYMOUS)
                .set_cell::<Scoreboard>(&0)
                .build()
                .unwrap_err()
                .code(),
            StateErrorCode::UndeclaredAccess
        );
    }

    #[test]
    fn a_view_never_mutates_the_snapshot_it_looks_at() {
        let access = declared();
        let stored = snapshot();
        let before = stored.hash();
        let view = StateView::open(&access, &stored);
        let _ = view.cell::<Ball>();
        let _ = view.patch(StateOrigin::ANONYMOUS).set_cell::<Ball>(&123).build();
        assert_eq!(stored.hash(), before);
        assert_eq!(stored.cell::<Ball>(), Ok(3));
    }
}
