//! The compile-time binding between a declared path and its value type.
//!
//! A key is a zero-sized marker type the caller declares. It carries no data and
//! no behaviour — these traits have no methods at all, which is deliberate: a
//! trait with methods could be made into a trait object, and a trait object on a
//! public boundary hides an implementation that may retain state the engine
//! cannot prove. These are pure generic bounds.
//!
//! Because the identity and the value type come from the *same* key type, a call
//! site cannot disagree with itself: asking for the wrong type under a path is a
//! compile error, not a runtime check.
//!
//! ```ignore
//! snapshot.cell::<Score>()          // -> ScoreState, by construction
//! patch.set_cell::<Score>(&ball)    // compile error: expected ScoreState
//! patch.set_cell::<Ghosts>(&x)      // compile error: Ghosts is not a CellKey
//! ```

use axiom_kernel::Reflect;

use crate::state_id::StateId;
use crate::state_kind::StateKind;
use crate::state_shape_id::StateShapeId;

/// A declared state slot: a stable path and the storage shape it uses.
pub trait StateKey {
    /// The declared path, e.g. `"puzzle/tick"`. This string *is* the identity.
    const PATH: &'static str;

    /// Which storage shape this slot uses.
    const KIND: StateKind;

    /// The identity of this slot. A pure function of [`Self::PATH`].
    fn id() -> StateId {
        StateId::of_path(Self::PATH)
    }
}

/// A slot holding exactly one typed value.
pub trait CellKey: StateKey {
    /// The stored value type.
    type Value: Reflect;

    /// The shape identity of this slot's contents.
    fn shape() -> StateShapeId {
        StateShapeId::cell_of::<Self::Value>()
    }
}

/// A slot holding a deterministically ordered keyed collection.
pub trait TableKey: StateKey {
    /// The row key type.
    type Key: Reflect + Ord + Clone;
    /// The row value type.
    type Value: Reflect;

    /// The shape identity of this slot's contents.
    fn shape() -> StateShapeId {
        StateShapeId::table_of::<Self::Key, Self::Value>()
    }
}

/// A slot holding an explicitly ordered sequence, where position is meaning.
pub trait SequenceKey: StateKey {
    /// The item type.
    type Item: Reflect;

    /// The shape identity of this slot's contents.
    fn shape() -> StateShapeId {
        StateShapeId::sequence_of::<Self::Item>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tick;
    impl StateKey for Tick {
        const PATH: &'static str = "test/tick";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Tick {
        type Value = u64;
    }

    struct Rows;
    impl StateKey for Rows {
        const PATH: &'static str = "test/rows";
        const KIND: StateKind = StateKind::Table;
    }
    impl TableKey for Rows {
        type Key = u32;
        type Value = u64;
    }

    struct Log;
    impl StateKey for Log {
        const PATH: &'static str = "test/log";
        const KIND: StateKind = StateKind::Sequence;
    }
    impl SequenceKey for Log {
        type Item = u32;
    }

    #[test]
    fn a_key_identity_is_the_digest_of_its_declared_path() {
        assert_eq!(Tick::id(), StateId::of_path("test/tick"));
        assert_eq!(Rows::id(), StateId::of_path("test/rows"));
        assert_eq!(Log::id(), StateId::of_path("test/log"));
    }

    #[test]
    fn distinct_keys_have_distinct_identities() {
        assert_ne!(Tick::id(), Rows::id());
        assert_ne!(Rows::id(), Log::id());
    }

    #[test]
    fn each_key_declares_its_storage_kind() {
        assert_eq!(Tick::KIND, StateKind::Cell);
        assert_eq!(Rows::KIND, StateKind::Table);
        assert_eq!(Log::KIND, StateKind::Sequence);
    }

    #[test]
    fn a_key_shape_identity_matches_its_value_types() {
        assert_eq!(
            <Tick as CellKey>::shape(),
            StateShapeId::cell_of::<u64>()
        );
        assert_eq!(
            <Rows as TableKey>::shape(),
            StateShapeId::table_of::<u32, u64>()
        );
        assert_eq!(
            <Log as SequenceKey>::shape(),
            StateShapeId::sequence_of::<u32>()
        );
    }
}
