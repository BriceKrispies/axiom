//! What state exists: the validated set of declarations.

use axiom_kernel::{SchemaVersion, StableHash};

use crate::state_decl::StateDecl;
use crate::state_error::StateError;
use crate::state_error_code::StateErrorCode;
use crate::state_id::StateId;
use crate::state_result::StateResult;
use crate::state_schema_id::StateSchemaId;

/// The declared shape of a game's or module's persistent state.
///
/// A schema is built once from a list of declarations, validated, and then
/// passed around as data. It is not a registry: nothing registers itself, there
/// is no global to look it up in, and building the same declarations twice
/// produces an identical schema.
///
/// Declarations are stored sorted by identity, so **the order they were written
/// in never reaches the identity, the bytes, the hash, the diff, or the
/// iteration order.** Insertion-order accidents are structurally impossible
/// rather than merely discouraged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSchema {
    name: &'static str,
    version: SchemaVersion,
    decls: Vec<StateDecl>,
    structure: StableHash,
    identity: StateSchemaId,
}

/// Reject a schema whose name is empty.
fn reject_empty_name(name: &'static str) -> StateResult<()> {
    (!name.is_empty())
        .then_some(())
        .ok_or(StateError::new(
            StateErrorCode::InvalidSchema,
            "a schema must have a non-empty name",
        ))
}

/// Reject a declaration whose path is empty — an empty path would give every
/// such slot the same identity.
fn reject_empty_path(decls: &[StateDecl]) -> StateResult<()> {
    decls
        .iter()
        .find(|decl| decl.path().is_empty())
        .map_or(Ok(()), |decl| {
            Err(StateError::at(
                StateErrorCode::InvalidSchema,
                decl.id(),
                "a declared state path must be non-empty",
            ))
        })
}

/// The two ways two declarations can end up sharing one identity. Indexed by
/// "the paths were equal", so the diagnostic distinguishes them without a branch
/// — and without an arm no test can reach.
const DUPLICATE_CAUSE: [&str; 2] = [
    "two declared state paths digest to the same identity",
    "two declarations share a state path",
];

/// Reject two declarations that would occupy the same identity.
///
/// This is one check rather than two because a shared path *implies* a shared
/// identity: checking paths separately would leave the collision check
/// unreachable by any test, and an unreachable check is one nobody can trust.
/// Sorting by identity catches both causes at once, and the message names which
/// one it was.
fn reject_duplicate_identity(decls: &[StateDecl]) -> StateResult<()> {
    let mut seen: Vec<(u64, &'static str)> = decls
        .iter()
        .map(|decl| (decl.id().raw(), decl.path()))
        .collect();
    seen.sort_unstable();
    seen.windows(2)
        .find(|pair| pair[0].0 == pair[1].0)
        .map_or(Ok(()), |pair| {
            Err(StateError::at(
                StateErrorCode::DuplicateStateIdentity,
                StateId::from_raw(pair[0].0),
                DUPLICATE_CAUSE[usize::from(pair[0].1 == pair[1].1)],
            ))
        })
}

/// Fold the declarations into a shape-only digest: identity, kind, and value
/// shape of each, in identity order. Version-independent on purpose — it answers
/// "did the shape actually change?", which is what a migration test needs.
fn structure_hash(decls: &[StateDecl]) -> StableHash {
    let words: Vec<u64> = decls
        .iter()
        .flat_map(|decl| {
            [
                decl.id().raw(),
                u64::from(decl.kind().code()),
                decl.shape().raw(),
            ]
        })
        .collect();
    StableHash::of_words(&words)
}

impl StateSchema {
    /// Build and validate a schema.
    ///
    /// Rejects, deterministically: an empty schema name, an empty declared path,
    /// two declarations sharing a path, and two distinct paths whose identities
    /// collide.
    pub fn build(
        name: &'static str,
        version: SchemaVersion,
        decls: &[StateDecl],
    ) -> StateResult<StateSchema> {
        reject_empty_name(name)
            .and_then(|()| reject_empty_path(decls))
            .and_then(|()| reject_duplicate_identity(decls))
            .map(|()| Self::assemble(name, version, decls))
    }

    /// Sort the declarations by identity and derive the digests.
    fn assemble(name: &'static str, version: SchemaVersion, decls: &[StateDecl]) -> StateSchema {
        let mut sorted = decls.to_vec();
        sorted.sort_unstable_by_key(|decl| decl.id().raw());
        let structure = structure_hash(&sorted);
        StateSchema {
            name,
            version,
            decls: sorted,
            structure,
            identity: StateSchemaId::of(name, version, structure),
        }
    }

    /// The schema's name.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The schema's version.
    pub const fn version(&self) -> SchemaVersion {
        self.version
    }

    /// The identity of this schema at this version and shape.
    pub const fn identity(&self) -> StateSchemaId {
        self.identity
    }

    /// The shape-only digest, independent of name and version.
    pub const fn structure_hash(&self) -> StableHash {
        self.structure
    }

    /// Every declaration, ascending by identity.
    pub fn decls(&self) -> &[StateDecl] {
        &self.decls
    }

    /// Look up one declaration, or fail naming the identity.
    pub fn decl(&self, id: StateId) -> StateResult<StateDecl> {
        self.decls
            .binary_search_by_key(&id.raw(), |decl| decl.id().raw())
            .ok()
            .and_then(|index| self.decls.get(index).copied())
            .ok_or(StateError::at(
                StateErrorCode::UnknownStateIdentity,
                id,
                "this schema declares no state with that identity",
            ))
    }

    /// Whether this schema declares `id`.
    pub fn declares(&self, id: StateId) -> bool {
        self.decl(id).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_key::{CellKey, SequenceKey, StateKey, TableKey};
    use crate::state_kind::StateKind;

    struct Tick;
    impl StateKey for Tick {
        const PATH: &'static str = "test/tick";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Tick {
        type Value = u64;
    }

    struct Solved;
    impl StateKey for Solved {
        const PATH: &'static str = "test/solved";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Solved {
        type Value = bool;
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

    /// A second key declaring the same path as `Tick` — a duplicate.
    struct TickAgain;
    impl StateKey for TickAgain {
        const PATH: &'static str = "test/tick";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for TickAgain {
        type Value = u64;
    }

    /// A key with an empty path.
    struct Nameless;
    impl StateKey for Nameless {
        const PATH: &'static str = "";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Nameless {
        type Value = u64;
    }

    fn version() -> SchemaVersion {
        SchemaVersion::new(1, 0)
    }

    fn valid() -> StateSchema {
        StateSchema::build(
            "test",
            version(),
            &[
                StateDecl::cell::<Tick>(),
                StateDecl::table::<Rows>(),
                StateDecl::sequence::<Log>(),
            ],
        )
        .expect("the declarations are valid")
    }

    #[test]
    fn a_valid_schema_is_accepted_and_keeps_its_name_and_version() {
        let schema = valid();
        assert_eq!(schema.name(), "test");
        assert_eq!(schema.version(), version());
        assert_eq!(schema.decls().len(), 3);
    }

    #[test]
    fn an_empty_schema_name_is_rejected() {
        let failed = StateSchema::build("", version(), &[StateDecl::cell::<Tick>()]);
        assert_eq!(
            failed.unwrap_err().code(),
            StateErrorCode::InvalidSchema
        );
    }

    #[test]
    fn an_empty_declared_path_is_rejected() {
        let failed = StateSchema::build("test", version(), &[StateDecl::cell::<Nameless>()]);
        assert_eq!(
            failed.unwrap_err().code(),
            StateErrorCode::InvalidSchema
        );
    }

    #[test]
    fn two_declarations_sharing_a_path_are_rejected() {
        let failed = StateSchema::build(
            "test",
            version(),
            &[StateDecl::cell::<Tick>(), StateDecl::cell::<TickAgain>()],
        );
        let error = failed.unwrap_err();
        assert_eq!(error.code(), StateErrorCode::DuplicateStateIdentity);
        assert_eq!(error.state(), StateId::of_path("test/tick"));
    }

    #[test]
    fn a_schema_with_no_declarations_is_valid() {
        let schema = StateSchema::build("empty", version(), &[]).expect("no declarations is fine");
        assert!(schema.decls().is_empty());
    }

    #[test]
    fn declarations_are_stored_ascending_by_identity() {
        let schema = valid();
        let ids: Vec<u64> = schema.decls().iter().map(|d| d.id().raw()).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn declaration_order_never_reaches_the_identity() {
        let one = StateSchema::build(
            "test",
            version(),
            &[
                StateDecl::cell::<Tick>(),
                StateDecl::table::<Rows>(),
                StateDecl::sequence::<Log>(),
            ],
        )
        .expect("valid");
        let other = StateSchema::build(
            "test",
            version(),
            &[
                StateDecl::sequence::<Log>(),
                StateDecl::cell::<Tick>(),
                StateDecl::table::<Rows>(),
            ],
        )
        .expect("valid");
        assert_eq!(one.identity(), other.identity());
        assert_eq!(one.structure_hash(), other.structure_hash());
        assert_eq!(one, other);
    }

    #[test]
    fn a_changed_shape_changes_both_digests() {
        let base = valid();
        let extended = StateSchema::build(
            "test",
            version(),
            &[
                StateDecl::cell::<Tick>(),
                StateDecl::table::<Rows>(),
                StateDecl::sequence::<Log>(),
                StateDecl::cell::<Solved>(),
            ],
        )
        .expect("valid");
        assert_ne!(base.structure_hash(), extended.structure_hash());
        assert_ne!(base.identity(), extended.identity());
    }

    #[test]
    fn a_changed_version_changes_the_identity_but_not_the_shape() {
        let base = valid();
        let bumped = StateSchema::build(
            "test",
            SchemaVersion::new(2, 0),
            &[
                StateDecl::cell::<Tick>(),
                StateDecl::table::<Rows>(),
                StateDecl::sequence::<Log>(),
            ],
        )
        .expect("valid");
        assert_eq!(base.structure_hash(), bumped.structure_hash());
        assert_ne!(base.identity(), bumped.identity());
    }

    #[test]
    fn a_declared_state_can_be_looked_up_by_identity() {
        let schema = valid();
        let decl = schema.decl(Tick::id()).expect("tick is declared");
        assert_eq!(decl.path(), "test/tick");
        assert!(schema.declares(Rows::id()));
    }

    #[test]
    fn an_undeclared_identity_is_rejected_by_name() {
        let schema = valid();
        let missing = StateId::of_path("test/absent");
        let error = schema.decl(missing).unwrap_err();
        assert_eq!(error.code(), StateErrorCode::UnknownStateIdentity);
        assert_eq!(error.state(), missing);
        assert!(!schema.declares(missing));
    }
}
